use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Context;
use walkdir::WalkDir;

use crate::summary::Summary;

pub fn clean_generated_trees(
    codex_home: &Path,
    cutoff: SystemTime,
    apply: bool,
    summary: &mut Summary,
) {
    for (bucket, rel) in [
        ("cache", "cache"),
        ("plugin-cache", "plugins/cache"),
        ("tmp", "tmp"),
        ("dot-tmp", ".tmp"),
    ] {
        let root = codex_home.join(rel);
        if let Err(err) = clean_tree(bucket, &root, cutoff, apply, summary) {
            summary.warn(format!("failed to clean {}: {err:#}", root.display()));
        }
    }
}

fn clean_tree(
    bucket: &str,
    root: &Path,
    cutoff: SystemTime,
    apply: bool,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let mut dirs = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_dir() {
            dirs.push(path.to_path_buf());
            continue;
        }
        if !entry.file_type().is_file() {
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
