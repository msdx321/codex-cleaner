use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about = "Prune old generated Codex state")]
pub struct Args {
    /// Codex home directory. Defaults to CODEX_HOME or ~/.codex.
    #[arg(long)]
    pub codex_home: Option<PathBuf>,

    /// Retention window in days.
    #[arg(long, default_value_t = 30)]
    pub days: i64,

    /// Delete files and rows. Without this flag, only report planned work.
    #[arg(long)]
    pub apply: bool,

    /// Also prune stale, unselected memory stage-1 rows.
    #[arg(long)]
    pub prune_memories: bool,

    /// Delete all remaining SQLite log rows, including active-thread and threadless diagnostics.
    #[arg(long)]
    pub prune_diagnostics: bool,

    /// Emit JSON instead of human-readable output.
    #[arg(long)]
    pub json: bool,
}

impl Args {
    pub fn codex_home(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = self.codex_home.clone() {
            return Ok(path);
        }
        if let Some(path) = std::env::var_os("CODEX_HOME") {
            return Ok(PathBuf::from(path));
        }
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not resolve home directory"))?;
        Ok(home.join(".codex"))
    }
}
