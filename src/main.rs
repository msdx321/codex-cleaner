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

    let codex_home = args.codex_home()?;
    let cutoff_dt = Utc::now()
        .checked_sub_signed(Duration::days(args.days))
        .context("retention cutoff overflow")?;
    let cutoff_unix = cutoff_dt.timestamp();
    let cutoff_system = SystemTime::UNIX_EPOCH
        .checked_add(StdDuration::from_secs(cutoff_unix.try_into()?))
        .context("retention cutoff before unix epoch")?;

    let mut summary = Summary::new(&codex_home, cutoff_unix, cutoff_dt.to_rfc3339(), args.apply);

    fs_clean::clean_generated_trees(&codex_home, cutoff_system, args.apply, &mut summary);
    sqlite_clean::clean_sqlite(
        &codex_home,
        cutoff_unix,
        args.apply,
        args.prune_memories,
        &mut summary,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        summary.print_human();
    }

    Ok(())
}
