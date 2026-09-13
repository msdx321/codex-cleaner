mod cli;
mod fs_clean;
mod interactive;
mod sqlite_clean;
mod summary;

use std::io::{self, IsTerminal};
use std::path::Path;
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
    let mut args = Args::parse();
    let terminal = io::stdin().is_terminal() && io::stdout().is_terminal();
    let interactive = args.interactive || (terminal && !args.apply && !args.dry_run && !args.json);
    anyhow::ensure!(
        !interactive || terminal,
        "interactive mode requires a terminal on stdin and stdout; use --dry-run or --apply for scripts"
    );
    if interactive && !interactive::configure(&mut args)? {
        println!("Cancelled. No changes made.");
        return Ok(());
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

    let summary = clean(
        &args,
        &codex_home,
        cutoff_unix,
        cutoff_system,
        &cutoff_dt.to_rfc3339(),
    )?;
    report(&args, &summary)?;
    if interactive && interactive::confirm_apply()? {
        args.apply = true;
        let summary = clean(
            &args,
            &codex_home,
            cutoff_unix,
            cutoff_system,
            &cutoff_dt.to_rfc3339(),
        )?;
        report(&args, &summary)?;
    } else if interactive {
        println!("No changes made.");
    }
    Ok(())
}

fn clean(
    args: &Args,
    codex_home: &Path,
    cutoff_unix: i64,
    cutoff_system: SystemTime,
    cutoff: &str,
) -> anyhow::Result<Summary> {
    let mut summary = Summary::new(codex_home, cutoff_unix, cutoff.to_owned(), args.apply);

    if args.compact_memories {
        let path = fs_clean::write_memory_compaction_note(codex_home, args.apply)?;
        summary.memory_compaction_note = Some(path.display().to_string());
    }
    fs_clean::clean_generated_trees(codex_home, cutoff_system, args.apply, &mut summary);
    sqlite_clean::clean_sqlite(codex_home, cutoff_unix, args, &mut summary);
    Ok(summary)
}

fn report(args: &Args, summary: &Summary) -> anyhow::Result<()> {
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
