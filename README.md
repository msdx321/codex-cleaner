# codex-cleaner

`codex-cleaner` is a small Rust CLI for pruning old generated Codex state from a
local Codex home directory.

It is dry-run by default. It reports what would be removed, and only mutates
files or SQLite databases when `--apply` is passed.

## What It Cleans

The default cleanup pass uses a retention window and targets generated state
under the Codex home directory:

- `cache/`
- `tmp/`
- `.tmp/` (excluding runtime locks and plugin checkouts)
- old files in `log/`
- old session rows, matching rollout files, and associated history, metadata,
  memory, queue revision, and completed-goal rows
- old log rows in `logs_2.sqlite`
- orphan log rows whose thread no longer exists

Optional flags can also prune stale memory stage-1 rows and remaining diagnostic
log rows. `--compact-memories` writes an ad hoc note asking Codex to retain only
important memory and discard transient history and tool-assignment prompts.

## Install

Install the latest published release through Homebrew:

```sh
brew install msdx321/tap/codex-cleaner
```

The tap builds from a checksummed source archive and requires Rust at build time.
Publishing a stable GitHub release updates the tap. To build the repository's
default branch:

```sh
brew install --HEAD msdx321/tap/codex-cleaner
```

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

Preview or write a memory compaction note:

```sh
cargo run -- --codex-home ~/.codex --compact-memories
cargo run -- --codex-home ~/.codex --compact-memories --apply
```

## Options

```text
Usage: codex-cleaner [OPTIONS]

Options:
      --codex-home <CODEX_HOME>  Codex home directory. Defaults to CODEX_HOME or ~/.codex
      --sqlite-home <SQLITE_HOME> SQLite directory when Codex's sqlite_home setting differs
      --days <DAYS>              Retention window in days [default: 30]
      --apply                    Make changes. Without this flag, only report planned work
      --prune-memories           Also prune stale, unselected memory stage-1 rows
      --compact-memories         Write an ad hoc note requesting memory compaction
      --prune-diagnostics        Delete all remaining SQLite log rows, including active-thread and threadless diagnostics
      --json                     Emit JSON instead of human-readable output
  -h, --help                     Print help
  -V, --version                  Print version
```

## Compatibility and Safety

Checked against Codex CLI **0.154.0** and its upstream database schemas:
`state_5.sqlite`, `logs_2.sqlite`, `memories_1.sqlite`, `goals_1.sqlite`,
`queue_1.sqlite`, and `thread_history_1.sqlite`. Older optional tables are handled
when present. Unknown database file versions stop database cleanup with a warning.

- Run without `--apply` first. Quit Codex before applying cleanup. Current Codex
  writer locks are respected, but older clients and other filesystem writers may
  not participate in that coordination.
- Pinned sessions, sessions with queued input or unfinished goals/turns, and
  sessions with recent database activity or recently modified rollouts are kept.
- Installed `plugins/cache/` bundles, temporary plugin checkouts, `tmp/arg0/`,
  and lock files are preserved regardless of age. Configuration, credentials,
  skills, attachments, and generated memory files are outside the default scope.
- `--days` must be non-negative and fit a cutoff on or after the Unix epoch.
- Relative rollout paths resolve under the Codex home directory. Rollouts must
  be regular `.jsonl` or `.jsonl.zst` files in `sessions/` or `archived_sessions/`,
  with the thread UUID as the filename suffix. Symlinks and parent traversal are
  rejected.
- If you configured Codex's `sqlite_home`, pass the same directory using
  `--sqlite-home`. This tool does not load Codex's TOML configuration.
- Preview opens existing databases read-only and never creates missing databases.
  Each matching row is counted once, including logs associated with sessions
  selected for deletion.
- Apply executes database deletions in a transaction, enables foreign-key
  enforcement, and removes rollout files only after SQL succeeds. SQLite WAL
  does not guarantee cross-database atomicity during a machine crash. A filesystem
  deletion failure can leave a rollout behind after its database rows are deleted.
- Changed databases receive checkpoint, vacuum, and optimize operations once per
  pass. Warnings produce a nonzero exit code, including maintenance failures.
- `--compact-memories` only writes the requested compaction note; it does not
  directly rewrite the memory files.

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

## Homebrew Release Updates

Publishing a stable GitHub release triggers **Update Homebrew tap** in this
repository. It updates the formula in `msdx321/homebrew-tap`; the resulting
formula commit triggers the tap's verification CI. There is no scheduled polling.
Set `HOMEBREW_TAP_DEPLOY_KEY` in this repository's Actions secrets to an SSH private
key with write access to the tap, matching ProxyBear's release setup. The tap's
updater script and verification workflow must be merged before publishing.
