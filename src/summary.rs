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
        let mode = if self.apply { "apply" } else { "dry-run" };
        println!("codex-cleaner {mode}");
        println!("codex_home: {}", self.codex_home);
        println!("cutoff: {}", self.cutoff);
        println!();

        for (name, bucket) in &self.buckets {
            if bucket.is_empty() {
                continue;
            }
            println!("{name}:");
            if bucket.matched_files > 0 || bucket.deleted_files > 0 || bucket.deleted_dirs > 0 {
                println!(
                    "  files: matched {}, deleted {}, bytes {}",
                    bucket.matched_files,
                    bucket.deleted_files,
                    display_bytes(if self.apply {
                        bucket.deleted_bytes
                    } else {
                        bucket.matched_bytes
                    })
                );
                if bucket.deleted_dirs > 0 {
                    println!("  empty dirs removed: {}", bucket.deleted_dirs);
                }
            }
            if bucket.matched_rows > 0 || bucket.deleted_rows > 0 {
                println!(
                    "  rows: matched {}, deleted {}",
                    bucket.matched_rows, bucket.deleted_rows
                );
            }
            if bucket.skipped > 0 {
                println!("  skipped: {}", bucket.skipped);
            }
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
