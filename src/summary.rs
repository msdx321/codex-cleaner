use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct Bucket {
    pub matched_files: u64,
    pub deleted_files: u64,
    pub deleted_dirs: u64,
    pub matched_bytes: u64,
    pub deleted_bytes: u64,
    pub matched_rows: u64,
    pub deleted_rows: u64,
    pub skipped: u64,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub codex_home: String,
    pub cutoff_unix: i64,
    pub cutoff: String,
    pub apply: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_compaction_note: Option<String>,
    pub buckets: BTreeMap<String, Bucket>,
    pub warnings: Vec<String>,
}

impl Summary {
    pub fn new(codex_home: &Path, cutoff_unix: i64, cutoff: String, apply: bool) -> Self {
        Self {
            codex_home: codex_home.display().to_string(),
            cutoff_unix,
            cutoff,
            apply,
            memory_compaction_note: None,
            buckets: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    pub fn bucket_mut(&mut self, name: &str) -> &mut Bucket {
        self.buckets.entry(name.to_string()).or_default()
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    pub fn print_human(&self) {
        let mode = if self.apply {
            "Cleanup results"
        } else {
            "Cleanup preview (no changes)"
        };
        println!("codex-cleaner | {mode}");
        println!("Home:   {}", self.codex_home);
        println!("Before: {}", self.cutoff);
        println!();

        let width = self
            .buckets
            .keys()
            .map(String::len)
            .max()
            .unwrap_or(8)
            .max(8);
        if self.apply {
            println!(
                "Files and DB rows show removed/matched; Dirs shows empty directories removed."
            );
        }
        println!(
            "{:<width$}  {:>15}  {:>15}  {:>12}  {:>7}  {:>7}",
            "Category", "Files", "DB rows", "File bytes", "Dirs", "Skipped"
        );
        let mut total = Bucket::default();
        for (name, bucket) in &self.buckets {
            if bucket.is_empty() {
                continue;
            }
            bucket.print_row(name, width, self.apply);
            total.matched_files += bucket.matched_files;
            total.deleted_files += bucket.deleted_files;
            total.matched_rows += bucket.matched_rows;
            total.deleted_rows += bucket.deleted_rows;
            total.matched_bytes += bucket.matched_bytes;
            total.deleted_bytes += bucket.deleted_bytes;
            total.deleted_dirs += bucket.deleted_dirs;
            total.skipped += bucket.skipped;
        }
        total.print_row("Total", width, self.apply);
        println!();
        if total.matched_files == 0 && total.matched_rows == 0 {
            println!("No expired files or database rows matched.");
        }
        println!("File bytes exclude space recovered by SQLite maintenance.");
        if !self.apply {
            println!("Empty directories are checked during apply.");
        }

        if let Some(path) = &self.memory_compaction_note {
            let action = if self.apply { "wrote" } else { "would write" };
            println!("memory compaction note: {action} {path}");
        }

        if !self.warnings.is_empty() {
            println!();
            println!("warnings:");
            for warning in &self.warnings {
                println!("  - {warning}");
            }
        }
    }
}

impl Bucket {
    fn print_row(&self, name: &str, width: usize, apply: bool) {
        let count = |matched, deleted| {
            if apply {
                format!("{deleted}/{matched}")
            } else {
                format!("{matched}")
            }
        };
        let bytes = if apply {
            self.deleted_bytes
        } else {
            self.matched_bytes
        };
        let dirs = if apply {
            self.deleted_dirs.to_string()
        } else {
            "-".to_owned()
        };
        println!(
            "{name:<width$}  {:>15}  {:>15}  {:>12}  {:>7}  {:>7}",
            count(self.matched_files, self.deleted_files),
            count(self.matched_rows, self.deleted_rows),
            display_bytes(bytes),
            dirs,
            self.skipped,
        );
    }

    fn is_empty(&self) -> bool {
        self.matched_files == 0
            && self.deleted_files == 0
            && self.deleted_dirs == 0
            && self.matched_bytes == 0
            && self.deleted_bytes == 0
            && self.matched_rows == 0
            && self.deleted_rows == 0
            && self.skipped == 0
    }
}

fn display_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
