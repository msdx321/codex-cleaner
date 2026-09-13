use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Preview and clean old generated Codex state",
    after_help = "Examples:\n  codex-cleaner\n  codex-cleaner --days 60\n  codex-cleaner --dry-run\n  codex-cleaner --apply --days 60\n  codex-cleaner --json\n\nDefaults to interactive cleanup in a terminal, or a preview when input/output is redirected."
)]
pub struct Args {
    /// Codex home directory. Defaults to CODEX_HOME or ~/.codex.
    #[arg(long, help_heading = "Locations")]
    pub codex_home: Option<PathBuf>,

    /// SQLite directory, if Codex's sqlite_home setting differs from CODEX_HOME.
    #[arg(long, help_heading = "Locations")]
    pub sqlite_home: Option<PathBuf>,

    /// Retention window in days.
    #[arg(short = 'd', long, default_value_t = 30, value_parser = clap::value_parser!(i64).range(0..), help_heading = "Cleanup")]
    pub days: i64,

    /// Apply cleanup directly without the interactive menu or confirmation.
    #[arg(long, conflicts_with_all = ["interactive", "dry_run"], help_heading = "Mode")]
    pub apply: bool,

    /// Configure cleanup, preview it, then confirm whether to apply.
    #[arg(short = 'i', long, conflicts_with_all = ["json", "dry_run"], help_heading = "Mode")]
    pub interactive: bool,

    /// Preview without prompting or changing anything.
    #[arg(short = 'n', long, help_heading = "Mode")]
    pub dry_run: bool,

    /// Also prune stale, unselected memory stage-1 rows.
    #[arg(long, help_heading = "Cleanup")]
    pub prune_memories: bool,

    /// Write an ad hoc note requesting memory compaction.
    #[arg(long, help_heading = "Cleanup")]
    pub compact_memories: bool,

    /// Delete all remaining SQLite log rows, including active-thread and threadless diagnostics.
    #[arg(long, help_heading = "Cleanup")]
    pub prune_diagnostics: bool,

    /// Emit JSON instead of human-readable output.
    #[arg(long, help_heading = "Output")]
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
