use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, TransactionBehavior, params};

use crate::summary::Summary;

const MEMORY_CONSOLIDATE_KIND: &str = "memory_consolidate_global";
const MEMORY_CONSOLIDATE_KEY: &str = "global";

const DATABASES: &[(&str, &str)] = &[
    ("state", "state_5.sqlite"),
    ("logs", "logs_2.sqlite"),
    ("memories", "memories_1.sqlite"),
    ("goals", "goals_1.sqlite"),
    ("queue", "queue_1.sqlite"),
    ("history", "thread_history_1.sqlite"),
];

pub fn clean_sqlite(
    codex_home: &Path,
    cutoff_unix: i64,
    args: &crate::cli::Args,
    summary: &mut Summary,
) {
    if let Err(err) = clean_databases(
        codex_home,
        args.sqlite_home.as_deref().unwrap_or(codex_home),
        cutoff_unix,
        args.apply,
        args.prune_memories,
        args.prune_diagnostics,
        summary,
    ) {
        summary.warn(format!("database cleanup failed: {err:#}"));
    }
}

fn clean_databases(
    home: &Path,
    sqlite_home: &Path,
    cutoff: i64,
    apply: bool,
    prune_memories: bool,
    prune_diagnostics: bool,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    let sqlite_home = sqlite_home
        .canonicalize()
        .context("resolve SQLite directory")?;
    // Coordinate with current Codex writers without creating lock files in dry-run.
    let lock_path = home.join("thread-writer-locks/.coordination.lock");
    let _coordination = if lock_path.try_exists()? {
        ensure_no_symlinks(home, &lock_path)?;
        let file = File::open(&lock_path)?;
        file.try_lock()
            .context("Codex writer coordination is busy; retry when Codex is idle")?;
        Some(file)
    } else {
        None
    };
    let mut connection = Connection::open_in_memory()?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let mut attached = BTreeSet::new();
    for entry in fs::read_dir(&sqlite_home)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        if DATABASES.iter().any(|(_, filename)| {
            let prefix = filename.rsplit_once('_').expect("versioned database").0;
            name.starts_with(&format!("{prefix}_"))
                && name.ends_with(".sqlite")
                && name != *filename
        }) {
            anyhow::bail!("unsupported database version {name}; refusing to guess its schema");
        }
    }
    for &(schema, filename) in DATABASES {
        let path = sqlite_home.join(filename);
        if !path.try_exists()? {
            continue;
        }
        ensure_no_symlinks(&sqlite_home, &path)?;
        // URI mode prevents creating missing databases and makes dry-run read-only.
        let path = path.to_str().context("database path is not UTF-8")?;
        let encoded = path
            .replace('%', "%25")
            .replace('?', "%3F")
            .replace('#', "%23");
        let mode = if apply { "rw" } else { "ro" };
        connection.execute(
            &format!("ATTACH DATABASE ? AS {schema}"),
            [format!("file:{encoded}?mode={mode}")],
        )?;
        attached.insert(schema);
    }
    let behavior = if apply {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    };
    let tx = connection.transaction_with_behavior(behavior)?;
    tx.execute_batch("CREATE TEMP TABLE cleanup_threads (id TEXT PRIMARY KEY)")?;
    let files = if attached.contains("state") {
        plan_sessions(&tx, home, cutoff, &attached, summary)?
    } else {
        Vec::new()
    };
    // Plan all statements before committing any database changes. Each row belongs
    // to one bucket in both preview and apply, even when several rules match it.
    let mut changes = Vec::new();
    if attached.contains("logs") {
        let orphan = if attached.contains("state") {
            "thread_id IS NOT NULL AND (thread_id IN (SELECT id FROM cleanup_threads) OR NOT EXISTS (SELECT 1 FROM state.threads WHERE id = logs.thread_id))"
        } else {
            "0"
        };
        for (bucket, predicate) in [
            ("logs-db", format!("ts < {cutoff}")),
            ("orphan-logs-db", format!("ts >= {cutoff} AND ({orphan})")),
            (
                "diagnostic-logs-db",
                format!("ts >= {cutoff} AND NOT ({orphan})"),
            ),
        ] {
            if bucket == "diagnostic-logs-db" && !prune_diagnostics {
                continue;
            }
            prune_rows(
                &tx,
                "logs.logs",
                &predicate,
                bucket,
                apply,
                summary,
                &mut changes,
            )?;
        }
    }
    if attached.contains("memories") {
        let predicate = if prune_memories {
            format!(
                "thread_id IN (SELECT id FROM cleanup_threads) OR (selected_for_phase2 = 0 AND MAX(source_updated_at, generated_at, COALESCE(last_usage, 0)) < {cutoff})"
            )
        } else {
            "thread_id IN (SELECT id FROM cleanup_threads)".to_owned()
        };
        let selected: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM memories.stage1_outputs WHERE selected_for_phase2 != 0 AND ({predicate}))"), [], |row| row.get(0))?;
        prune_rows(
            &tx,
            "memories.stage1_outputs",
            &predicate,
            "memories-db",
            apply,
            summary,
            &mut changes,
        )?;
        prune_rows(
            &tx,
            "memories.jobs",
            "kind = 'memory_stage1' AND job_key IN (SELECT id FROM cleanup_threads)",
            "memory-jobs-db",
            apply,
            summary,
            &mut changes,
        )?;
        if apply && selected {
            enqueue_global_consolidation(&tx)?;
        }
    }
    for (schema, tables, bucket) in [
        (
            "history",
            &[
                "thread_items",
                "thread_turns",
                "thread_realtime_items",
                "thread_history_projection_state",
            ][..],
            "history-db",
        ),
        (
            "queue",
            &["queued_items", "queued_thread_revisions"][..],
            "queue-db",
        ),
        (
            "goals",
            &["thread_goal_continuation_deferrals", "thread_goals"][..],
            "goals-db",
        ),
        (
            "state",
            &["thread_dynamic_tools", "thread_artifacts"][..],
            "session-metadata-db",
        ),
    ] {
        if !attached.contains(schema) {
            continue;
        }
        for table in tables {
            if table_exists(&tx, schema, table)? {
                prune_rows(
                    &tx,
                    &format!("{schema}.{table}"),
                    "thread_id IN (SELECT id FROM cleanup_threads)",
                    bucket,
                    apply,
                    summary,
                    &mut changes,
                )?;
            }
        }
    }
    if attached.contains("state") {
        if table_exists(&tx, "state", "agent_job_items")? && apply {
            tx.execute("UPDATE state.agent_job_items SET assigned_thread_id = NULL, updated_at = strftime('%s','now') WHERE assigned_thread_id IN (SELECT id FROM cleanup_threads)", [])?;
        }
        if table_exists(&tx, "state", "thread_spawn_edges")? {
            prune_rows(
                &tx,
                "state.thread_spawn_edges",
                "parent_thread_id IN (SELECT id FROM cleanup_threads) OR child_thread_id IN (SELECT id FROM cleanup_threads)",
                "session-metadata-db",
                apply,
                summary,
                &mut changes,
            )?;
        }
        prune_rows(
            &tx,
            "state.threads",
            "id IN (SELECT id FROM cleanup_threads)",
            "sessions",
            apply,
            summary,
            &mut changes,
        )?;
    }
    tx.commit()?;
    if apply {
        for (_, bucket, count) in &changes {
            summary.bucket_mut(bucket).deleted_rows += count;
        }
        // Remove files only after SQL succeeds, so a schema error never destroys a rollout.
        for (path, len) in files {
            match fs::remove_file(&path) {
                Ok(()) => {
                    let bucket = summary.bucket_mut("sessions");
                    bucket.deleted_files += 1;
                    bucket.deleted_bytes += len;
                }
                Err(err) => summary.warn(format!("remove {}: {err}", path.display())),
            }
        }
        let changed_databases = changes
            .iter()
            .map(|(schema, _, _)| schema.as_str())
            .collect::<BTreeSet<_>>();
        for schema in changed_databases {
            if let Err(err) = maintain_database(&connection, schema) {
                summary.warn(format!("maintain {schema} database: {err:#}"));
            }
        }
    }
    Ok(())
}

fn maintain_database(connection: &Connection, schema: &str) -> anyhow::Result<()> {
    let busy: i64 = connection.query_row(
        &format!("PRAGMA {schema}.wal_checkpoint(TRUNCATE)"),
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        busy == 0,
        "checkpoint blocked by another database reader or writer"
    );
    connection.execute_batch(&format!("VACUUM {schema}; PRAGMA {schema}.optimize;"))?;
    Ok(())
}

fn prune_rows(
    connection: &Connection,
    table: &str,
    predicate: &str,
    bucket: &str,
    apply: bool,
    summary: &mut Summary,
    changes: &mut Vec<(String, String, u64)>,
) -> anyhow::Result<()> {
    let count: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
        [],
        |row| row.get(0),
    )?;
    let count = u64::try_from(count)?;
    summary.bucket_mut(bucket).matched_rows += count;
    if apply && count > 0 {
        let deleted = connection.execute(&format!("DELETE FROM {table} WHERE {predicate}"), [])?;
        changes.push((
            table
                .split_once('.')
                .context("qualified table name")?
                .0
                .to_owned(),
            bucket.to_owned(),
            deleted as u64,
        ));
    }
    Ok(())
}

fn table_exists(connection: &Connection, schema: &str, table: &str) -> anyhow::Result<bool> {
    Ok(connection.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM {schema}.sqlite_schema WHERE type = 'table' AND name = ?)"
        ),
        [table],
        |row| row.get(0),
    )?)
}

fn plan_sessions(
    connection: &Connection,
    home: &Path,
    cutoff: i64,
    attached: &BTreeSet<&str>,
    summary: &mut Summary,
) -> anyhow::Result<Vec<(PathBuf, u64)>> {
    let columns = connection
        .prepare("PRAGMA state.table_info(threads)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut conditions = vec![format!("updated_at < {cutoff}")];
    if columns.contains("recency_at") {
        conditions.push(format!("recency_at < {cutoff}"));
    }
    for column in ["updated_at_ms", "recency_at_ms"] {
        if columns.contains(column) {
            conditions.push(format!("COALESCE({column}, 0) < {}", cutoff * 1000));
        }
    }
    if columns.contains("is_pinned") {
        conditions.push("is_pinned = 0".to_owned());
    }
    if attached.contains("queue") {
        conditions.push(
            "NOT EXISTS (SELECT 1 FROM queue.queued_items WHERE thread_id = threads.id)".to_owned(),
        );
    }
    if attached.contains("goals") {
        conditions.push("NOT EXISTS (SELECT 1 FROM goals.thread_goals WHERE thread_id = threads.id AND status != 'complete')".to_owned());
    }
    if attached.contains("history") {
        conditions.push("NOT EXISTS (SELECT 1 FROM history.thread_turns WHERE thread_id = threads.id AND completed_at IS NULL)".to_owned());
    }
    let query = format!(
        "SELECT id, rollout_path FROM state.threads WHERE {} ORDER BY updated_at, id",
        conditions.join(" AND ")
    );
    let rows = connection
        .prepare(&query)?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut files = Vec::new();
    for (id, raw) in rows {
        match session_files(home, &id, Path::new(&raw), cutoff) {
            Ok(Some(candidates)) => {
                connection.execute("INSERT INTO cleanup_threads VALUES (?)", [&id])?;
                let bucket = summary.bucket_mut("sessions");
                bucket.matched_files += candidates.len() as u64;
                bucket.matched_bytes += candidates.iter().map(|(_, len)| len).sum::<u64>();
                files.extend(candidates);
            }
            Ok(None) => summary.bucket_mut("sessions").skipped += 1,
            Err(err) => {
                summary.bucket_mut("sessions").skipped += 1;
                summary.warn(format!("skip session {id}: {err:#}"));
            }
        }
    }
    Ok(files)
}

fn session_files(
    home: &Path,
    id: &str,
    raw: &Path,
    cutoff: i64,
) -> anyhow::Result<Option<Vec<(PathBuf, u64)>>> {
    anyhow::ensure!(
        id.len() == 36
            && id
                .chars()
                .enumerate()
                .all(|(i, c)| if [8, 13, 18, 23].contains(&i) {
                    c == '-'
                } else {
                    c.is_ascii_hexdigit()
                }),
        "invalid thread id"
    );
    if home
        .join("thread-writer-locks")
        .join(format!("{id}.lock"))
        .try_exists()?
    {
        return Ok(None);
    }
    let path = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        home.join(raw)
    };
    anyhow::ensure!(
        path.starts_with(home.join("sessions")) || path.starts_with(home.join("archived_sessions")),
        "rollout is outside session directories"
    );
    ensure_no_symlinks(home, &path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid rollout filename")?;
    let plain = name.strip_suffix(".zst").unwrap_or(name);
    anyhow::ensure!(
        plain.starts_with("rollout-") && plain.ends_with(&format!("-{id}.jsonl")),
        "rollout filename does not match thread id"
    );
    let mut files = Vec::new();
    for candidate in [
        path.with_file_name(plain),
        path.with_file_name(format!("{plain}.zst")),
    ] {
        ensure_no_symlinks(home, &candidate)?;
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                anyhow::ensure!(metadata.is_file(), "rollout is not a regular file");
                let modified: chrono::DateTime<chrono::Utc> = metadata.modified()?.into();
                if modified.timestamp() >= cutoff {
                    return Ok(None);
                }
                files.push((candidate, metadata.len()));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(Some(files))
}

fn ensure_no_symlinks(home: &Path, path: &Path) -> anyhow::Result<()> {
    let relative = path.strip_prefix(home).context("path escapes Codex home")?;
    let mut current = home.to_path_buf();
    for component in relative.components() {
        anyhow::ensure!(
            matches!(component, Component::Normal(_)),
            "non-normal path component"
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "refusing symlink {}",
                current.display()
            ),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
fn enqueue_global_consolidation(tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    tx.execute(
        r#"
INSERT INTO memories.jobs (
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
