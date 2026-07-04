use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};

use crate::summary::Summary;

const MEMORY_STAGE1_KIND: &str = "memory_stage1";
const MEMORY_CONSOLIDATE_KIND: &str = "memory_consolidate_global";
const MEMORY_CONSOLIDATE_KEY: &str = "global";

#[derive(Debug)]
struct SessionRow {
    id: String,
    rollout_path: PathBuf,
}

pub fn clean_sqlite(
    codex_home: &Path,
    cutoff_unix: i64,
    apply: bool,
    prune_memories: bool,
    summary: &mut Summary,
) {
    if let Err(err) = clean_sessions(codex_home, cutoff_unix, apply, summary) {
        summary.warn(format!("session cleanup failed: {err:#}"));
    }
    if let Err(err) = clean_logs_by_age(codex_home, cutoff_unix, apply, summary) {
        summary.warn(format!("log cleanup failed: {err:#}"));
    }
    if prune_memories
        && let Err(err) = clean_stale_memories(codex_home, cutoff_unix, apply, summary)
    {
        summary.warn(format!("memory cleanup failed: {err:#}"));
    }
}

fn open_existing(path: &Path) -> anyhow::Result<Option<Connection>> {
    if !path.exists() {
        return Ok(None);
    }
    let connection =
        Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(Some(connection))
}

fn clean_logs_by_age(
    codex_home: &Path,
    cutoff_unix: i64,
    apply: bool,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    let path = codex_home.join("logs_2.sqlite");
    let Some(connection) = open_existing(&path)? else {
        return Ok(());
    };

    let matched: u64 = connection.query_row(
        "SELECT COUNT(*) FROM logs WHERE ts < ?",
        params![cutoff_unix],
        |row| row.get(0),
    )?;
    summary.bucket_mut("logs-db").matched_rows += matched;

    if apply && matched > 0 {
        let deleted = connection
            .execute("DELETE FROM logs WHERE ts < ?", params![cutoff_unix])
            .context("delete old logs")? as u64;
        summary.bucket_mut("logs-db").deleted_rows += deleted;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA optimize;")
            .context("maintain logs database")?;
    }

    Ok(())
}

fn clean_stale_memories(
    codex_home: &Path,
    cutoff_unix: i64,
    apply: bool,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    let path = codex_home.join("memories_1.sqlite");
    let Some(mut connection) = open_existing(&path)? else {
        return Ok(());
    };
    let matched: u64 = connection.query_row(
        "SELECT COUNT(*) FROM stage1_outputs WHERE selected_for_phase2 = 0 AND COALESCE(last_usage, source_updated_at) < ?",
        params![cutoff_unix],
        |row| row.get(0),
    )?;
    summary.bucket_mut("memories-db").matched_rows += matched;

    if apply && matched > 0 {
        let tx = connection.transaction()?;
        let deleted = tx.execute(
            "DELETE FROM stage1_outputs WHERE selected_for_phase2 = 0 AND COALESCE(last_usage, source_updated_at) < ?",
            params![cutoff_unix],
        )? as u64;
        tx.commit()?;
        summary.bucket_mut("memories-db").deleted_rows += deleted;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA optimize;")
            .context("maintain memories database")?;
    }

    Ok(())
}

fn clean_sessions(
    codex_home: &Path,
    cutoff_unix: i64,
    apply: bool,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    let state_path = codex_home.join("state_5.sqlite");
    let Some(state) = open_existing(&state_path)? else {
        return Ok(());
    };

    let mut stmt = state.prepare(
        "SELECT id, rollout_path FROM threads WHERE updated_at < ? ORDER BY updated_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![cutoff_unix], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                rollout_path: PathBuf::from(row.get::<_, String>(1)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    summary.bucket_mut("sessions").matched_rows += rows.len() as u64;

    for session in &rows {
        let Some(scoped_path) = scoped_rollout_path(codex_home, &session.rollout_path) else {
            summary.bucket_mut("sessions").skipped += 1;
            summary.warn(format!(
                "skipped session {} because rollout path is outside Codex sessions: {}",
                session.id,
                session.rollout_path.display()
            ));
            continue;
        };
        if !scoped_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(&session.id))
        {
            summary.bucket_mut("sessions").skipped += 1;
            summary.warn(format!(
                "skipped session {} because rollout filename does not contain the thread id: {}",
                session.id,
                scoped_path.display()
            ));
            continue;
        }

        for path in rollout_delete_candidates(&scoped_path) {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => {
                    let len = metadata.len();
                    {
                        let bucket = summary.bucket_mut("sessions");
                        bucket.matched_files += 1;
                        bucket.matched_bytes += len;
                    }
                    if apply {
                        fs::remove_file(&path)
                            .with_context(|| format!("remove {}", path.display()))?;
                        let bucket = summary.bucket_mut("sessions");
                        bucket.deleted_files += 1;
                        bucket.deleted_bytes += len;
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
            }
        }

        if apply {
            delete_session_rows(codex_home, &state, &session.id)?;
            summary.bucket_mut("sessions").deleted_rows += 1;
        }
    }

    if apply && !rows.is_empty() {
        state
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA optimize;")
            .context("maintain state database")?;
        maintain_optional_db(&codex_home.join("logs_2.sqlite"), summary, "logs-db")?;
        maintain_optional_db(
            &codex_home.join("memories_1.sqlite"),
            summary,
            "memories-db",
        )?;
        maintain_optional_db(&codex_home.join("goals_1.sqlite"), summary, "goals-db")?;
    }

    Ok(())
}

fn delete_session_rows(
    codex_home: &Path,
    state: &Connection,
    thread_id: &str,
) -> anyhow::Result<()> {
    if let Some(logs) = open_existing(&codex_home.join("logs_2.sqlite"))? {
        logs.execute("DELETE FROM logs WHERE thread_id = ?", params![thread_id])?;
    }

    if let Some(mut memories) = open_existing(&codex_home.join("memories_1.sqlite"))? {
        let tx = memories.transaction()?;
        let selected: Option<i64> = tx
            .query_row(
                "SELECT selected_for_phase2 FROM stage1_outputs WHERE thread_id = ?",
                params![thread_id],
                |row| row.get(0),
            )
            .optional()?;
        let deleted = tx.execute(
            "DELETE FROM stage1_outputs WHERE thread_id = ?",
            params![thread_id],
        )?;
        tx.execute(
            "DELETE FROM jobs WHERE kind = ? AND job_key = ?",
            params![MEMORY_STAGE1_KIND, thread_id],
        )?;
        if deleted > 0 && selected.is_some_and(|value| value != 0) {
            enqueue_global_consolidation(&tx)?;
        }
        tx.commit()?;
    }

    if let Some(goals) = open_existing(&codex_home.join("goals_1.sqlite"))? {
        goals.execute(
            "DELETE FROM thread_goals WHERE thread_id = ?",
            params![thread_id],
        )?;
    }

    state.execute(
        "DELETE FROM thread_dynamic_tools WHERE thread_id = ?",
        params![thread_id],
    )?;
    state.execute(
        "UPDATE agent_job_items SET assigned_thread_id = NULL, updated_at = strftime('%s','now') WHERE assigned_thread_id = ?",
        params![thread_id],
    )?;
    state.execute(
        "DELETE FROM thread_spawn_edges WHERE parent_thread_id = ? OR child_thread_id = ?",
        params![thread_id, thread_id],
    )?;
    state.execute("DELETE FROM threads WHERE id = ?", params![thread_id])?;

    Ok(())
}

fn enqueue_global_consolidation(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    tx.execute(
        r#"
INSERT INTO jobs (
    kind,
    job_key,
    status,
    worker_id,
    ownership_token,
    started_at,
    finished_at,
    lease_until,
    retry_at,
    retry_remaining,
    last_error,
    input_watermark,
    last_success_watermark
) VALUES (?, ?, 'pending', NULL, NULL, NULL, NULL, NULL, NULL, 3, NULL, strftime('%s','now'), 0)
ON CONFLICT(kind, job_key) DO UPDATE SET
    status = CASE
        WHEN jobs.status = 'running' THEN 'running'
        ELSE 'pending'
    END,
    retry_at = CASE
        WHEN jobs.status = 'running' THEN jobs.retry_at
        ELSE NULL
    END,
    retry_remaining = max(jobs.retry_remaining, excluded.retry_remaining),
    input_watermark = CASE
        WHEN excluded.input_watermark > COALESCE(jobs.input_watermark, 0)
            THEN excluded.input_watermark
        ELSE COALESCE(jobs.input_watermark, 0) + 1
    END
        "#,
        params![MEMORY_CONSOLIDATE_KIND, MEMORY_CONSOLIDATE_KEY],
    )?;
    Ok(())
}

fn maintain_optional_db(path: &Path, summary: &mut Summary, bucket: &str) -> anyhow::Result<()> {
    let Some(connection) = open_existing(path)? else {
        return Ok(());
    };
    if let Err(err) =
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA optimize;")
    {
        summary.warn(format!("failed to maintain {}: {err}", path.display()));
    }
    summary.bucket_mut(bucket);
    Ok(())
}

fn scoped_rollout_path(codex_home: &Path, raw: &Path) -> Option<PathBuf> {
    let path = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        codex_home.join(raw)
    };
    let normalized = normalize_components(&path)?;
    let sessions = normalize_components(&codex_home.join("sessions"))?;
    let archived = normalize_components(&codex_home.join("archived_sessions"))?;
    (normalized.starts_with(&sessions) || normalized.starts_with(&archived)).then_some(normalized)
}

fn normalize_components(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
        }
    }
    Some(normalized)
}

fn rollout_delete_candidates(path: &Path) -> Vec<PathBuf> {
    let path_string = path.to_string_lossy();
    if let Some(plain) = path_string.strip_suffix(".zst") {
        let plain = PathBuf::from(plain);
        vec![plain, path.to_path_buf()]
    } else {
        vec![path.to_path_buf(), path.with_extension("jsonl.zst")]
    }
}
