use std::io::{self, Write};

use anyhow::Context;

use crate::cli::Args;

pub fn configure(args: &mut Args) -> anyhow::Result<bool> {
    println!("codex-cleaner | Interactive setup");
    println!("Codex home: {}", args.codex_home()?.display());
    if let Some(path) = &args.sqlite_home {
        println!("SQLite home: {}", path.display());
    }
    loop {
        println!("\n  1  Retention: {} days", args.days);
        println!(
            "  2  Prune stale memory rows: {}",
            on_off(args.prune_memories)
        );
        println!(
            "  3  Prune all diagnostic rows: {}",
            on_off(args.prune_diagnostics)
        );
        println!(
            "  4  Write memory compaction note: {}",
            on_off(args.compact_memories)
        );
        println!("  5  Preview cleanup (default)");
        println!("  0  Cancel");
        let Some(choice) = prompt("Choose [5]: ")? else {
            return Ok(false);
        };
        match choice.as_str() {
            "1" => {
                let Some(value) = prompt("Retention in days (0 includes recent files): ")? else {
                    return Ok(false);
                };
                match value.parse::<i64>() {
                    Ok(days) if days >= 0 => args.days = days,
                    _ => println!("Enter a non-negative whole number."),
                }
            }
            "2" => args.prune_memories = !args.prune_memories,
            "3" => {
                args.prune_diagnostics = !args.prune_diagnostics;
                if args.prune_diagnostics {
                    println!(
                        "Includes active-thread and threadless diagnostics, regardless of age."
                    );
                }
            }
            "4" => args.compact_memories = !args.compact_memories,
            "" | "5" => return Ok(true),
            "0" => return Ok(false),
            _ => println!("Choose a number from 0 to 5."),
        }
    }
}

pub fn confirm_apply() -> anyhow::Result<bool> {
    println!(
        "\nQuit Codex before applying cleanup. Eligible files and rows will be checked again."
    );
    loop {
        match prompt("Apply cleanup? [y/N]: ")?.as_deref() {
            Some("y" | "Y" | "yes" | "YES") => return Ok(true),
            None | Some("" | "n" | "N" | "no" | "NO") => return Ok(false),
            _ => println!("Enter yes or no."),
        }
    }
}

fn prompt(message: &str) -> anyhow::Result<Option<String>> {
    print!("{message}");
    io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    if io::stdin().read_line(&mut input).context("read response")? == 0 {
        return Ok(None);
    }
    Ok(Some(input.trim().to_owned()))
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
