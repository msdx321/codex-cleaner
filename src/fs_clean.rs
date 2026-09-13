use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;
use walkdir::{DirEntry, WalkDir};

use crate::summary::Summary;

const MEMORY_COMPACTION_NOTE: &str = include_str!("../templates/memory_compaction.md");

const GENERATED_TREES: &[(&str, &str)] = &[
    ("cache", "cache"),
    ("tmp", "tmp"),
    ("dot-tmp", ".tmp"),
    ("file-logs", "log"),
];
const TEMP_FILE_EXTENSIONS: &[&str] = &["tmp"];

#[derive(Clone, Copy)]
enum CleanupScope {
    GeneratedTree,
    TemporaryFiles,
}

impl CleanupScope {
    fn includes_entry(self, entry: &DirEntry) -> bool {
        if entry.depth() == 0 {
            return true;
        }
        let name = entry.file_name();
        // Codex owns these runtime locks and plugin checkouts, regardless of age.
        if matches!(
            name.to_str(),
            Some("arg0" | "plugins" | "marketplaces" | "bundled-marketplaces")
        ) || name.to_string_lossy().ends_with(".lock")
        {
            return false;
        }
        match self {
            Self::GeneratedTree => true,
            Self::TemporaryFiles => {
                name != ".git"
                    && !(entry.depth() == 1
                        && (GENERATED_TREES.iter().any(|(_, path)| name == *path)
                            || matches!(
                                name.to_str(),
                                Some("skills" | "attachments" | "memories" | "thread-writer-locks")
                            )))
            }
        }
    }

    fn matches_file(self, path: &Path) -> bool {
        match self {
            Self::GeneratedTree => true,
            Self::TemporaryFiles => path.extension().is_some_and(|extension| {
                TEMP_FILE_EXTENSIONS.iter().any(|item| extension == *item)
            }),
        }
    }

    fn removes_empty_dirs(self) -> bool {
        matches!(self, Self::GeneratedTree)
    }
}

pub fn clean_generated_trees(
    codex_home: &Path,
    cutoff: SystemTime,
    apply: bool,
    summary: &mut Summary,
) {
    for &(bucket, rel) in GENERATED_TREES {
        let root = codex_home.join(rel);
        if let Err(err) = clean_tree(
            bucket,
            &root,
            cutoff,
            apply,
            CleanupScope::GeneratedTree,
            summary,
        ) {
            summary.warn(format!("failed to clean {}: {err:#}", root.display()));
        }
    }
    if let Err(err) = clean_tree(
        "tmp-files",
        codex_home,
        cutoff,
        apply,
        CleanupScope::TemporaryFiles,
        summary,
    ) {
        summary.warn(format!("failed to clean .tmp files: {err:#}"));
    }
}

pub fn write_memory_compaction_note(codex_home: &Path, apply: bool) -> anyhow::Result<PathBuf> {
    let notes_dir = codex_home.join("memories/extensions/ad_hoc/notes");
    let path = notes_dir.join(format!(
        "{}-compact-memory.md",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    if apply {
        fs::create_dir_all(&notes_dir)
            .with_context(|| format!("create {}", notes_dir.display()))?;
        fs::write(&path, MEMORY_COMPACTION_NOTE)
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(path)
}

fn clean_tree(
    bucket: &str,
    root: &Path,
    cutoff: SystemTime,
    apply: bool,
    scope: CleanupScope,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    if !root.try_exists()? {
        return Ok(());
    }
    anyhow::ensure!(
        !fs::symlink_metadata(root)?.file_type().is_symlink(),
        "refusing symlinked cleanup root"
    );

    let mut dirs = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_entry(|entry| scope.includes_entry(entry))
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_dir() {
            if scope.removes_empty_dirs() && entry.metadata()?.modified()? < cutoff {
                dirs.push(path.to_path_buf());
            }
            continue;
        }
        if !entry.file_type().is_file() || !scope.matches_file(path) {
            continue;
        }

        let metadata = entry
            .metadata()
            .with_context(|| format!("read metadata for {}", path.display()))?;
        let modified = metadata
            .modified()
            .with_context(|| format!("read modified time for {}", path.display()))?;
        if modified >= cutoff {
            continue;
        }

        let len = metadata.len();
        {
            let bucket = summary.bucket_mut(bucket);
            bucket.matched_files += 1;
            bucket.matched_bytes += len;
        }
        if apply {
            fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
            let bucket = summary.bucket_mut(bucket);
            bucket.deleted_files += 1;
            bucket.deleted_bytes += len;
        }
    }

    if apply {
        dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for dir in dirs {
            if dir == root {
                continue;
            }
            if is_empty_dir(&dir)? {
                fs::remove_dir(&dir).with_context(|| format!("remove dir {}", dir.display()))?;
                summary.bucket_mut(bucket).deleted_dirs += 1;
            }
        }
    }

    Ok(())
}

fn is_empty_dir(path: &Path) -> anyhow::Result<bool> {
    Ok(fs::read_dir(path)?.next().is_none())
}
