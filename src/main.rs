mod cli;
mod fs_clean;
mod sqlite_clean;
mod summary;

use std::time::{Duration as StdDuration, SystemTime};

use anyhow::Context;
use chrono::{Duration, Utc};
use clap::Parser;

use crate::cli::Args;
use crate::summary::Summary;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.days < 0 {
        anyhow::bail!("--days must be non-negative");
    }

    let codex_home = args
        .codex_home()?
        .canonicalize()
        .context("resolve Codex home")?;
    anyhow::ensure!(codex_home.is_dir(), "Codex home must be a directory");
    let cutoff_dt = Utc::now()
        .checked_sub_signed(Duration::try_days(args.days).context("retention window overflow")?)
        .context("retention cutoff overflow")?;
    let cutoff_unix = cutoff_dt.timestamp();
    let cutoff_system = SystemTime::UNIX_EPOCH
        .checked_add(StdDuration::from_secs(cutoff_unix.try_into()?))
        .context("retention cutoff before unix epoch")?;

    let mut summary = Summary::new(&codex_home, cutoff_unix, cutoff_dt.to_rfc3339(), args.apply);

    if args.compact_memories {
        let path = fs_clean::write_memory_compaction_note(&codex_home, args.apply)?;
        summary.memory_compaction_note = Some(path.display().to_string());
    }
    fs_clean::clean_generated_trees(&codex_home, cutoff_system, args.apply, &mut summary);
    sqlite_clean::clean_sqlite(&codex_home, cutoff_unix, &args, &mut summary);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        summary.print_human();
    }

    anyhow::ensure!(
        summary.warnings.is_empty(),
        "cleanup incomplete; see warnings above"
    );
    Ok(())
}
