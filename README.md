# codex-cleaner

`codex-cleaner` is a small Rust CLI for pruning old generated Codex state from a
local Codex home directory.

It is dry-run by default. It reports what would be removed, and only mutates
files or SQLite databases when `--apply` is passed.

## What It Cleans

The default cleanup pass uses a retention window and targets generated state
under the Codex home directory:

- `cache/`
- `plugins/cache/`
- `tmp/`
- `.tmp/`
- old session rows and matching rollout files
- old log rows in `logs_2.sqlite`
- orphan log rows whose thread no longer exists

Optional flags can also prune stale memory stage-1 rows and remaining diagnostic
log rows.

## Install

Install from GitHub with Cargo:

```sh
cargo install --git https://github.com/msdx321/codex-cleaner.git
```

Install from a local checkout:

```sh
cargo install --path .
```

Or build from source without installing:

```sh
cargo build --release
```

The compiled binary will be at:

```sh
target/release/codex-cleaner
```

## Usage

Preview cleanup work without deleting anything:

```sh
cargo run -- --codex-home ~/.codex
```

Apply the cleanup:

```sh
cargo run -- --codex-home ~/.codex --apply
```

Use a custom retention window:

```sh
cargo run -- --codex-home ~/.codex --days 60
```

Emit JSON:

```sh
cargo run -- --codex-home ~/.codex --json
```

## Options

```text
Usage: codex-cleaner [OPTIONS]

Options:
      --codex-home <CODEX_HOME>  Codex home directory. Defaults to CODEX_HOME or ~/.codex
      --days <DAYS>              Retention window in days [default: 30]
      --apply                    Delete files and rows. Without this flag, only report planned work
      --prune-memories           Also prune stale, unselected memory stage-1 rows
      --prune-diagnostics        Delete all remaining SQLite log rows, including active-thread and threadless diagnostics
      --json                     Emit JSON instead of human-readable output
  -h, --help                     Print help
  -V, --version                  Print version
```

## Safety Notes

- Run without `--apply` first.
- `--days` must be non-negative.
- Relative rollout paths are resolved under the Codex home directory.
- Session rollout files are only removed when their path is under
  `sessions/` or `archived_sessions/` and the filename contains the thread id.
- Database maintenance uses SQLite checkpoint, vacuum, and optimize operations
  after deleting rows.

## Development

Run the standard checks:

```sh
cargo check
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

For a local behavior check, run a dry run against a Codex home directory:

```sh
cargo run -- --codex-home ~/.codex
```

## License

MIT. See [LICENSE](LICENSE).
