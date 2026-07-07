# Contributing to lazydb

lazydb is a lazygit-style TUI for databases (Rust 2024, ratatui + crossterm + mysql).
The ethos is **ponytail**: minimal code, no unrequested abstractions, deletion over
addition. Match that style and you'll fit right in.

## Building & testing

```
cargo run                        # run the TUI; `cargo run -- script.sql` preloads the editor
cargo test                       # unit tests (no DB needed)
cargo fmt
cargo clippy --all-targets -- -D warnings
```

Rule: **fmt clean + clippy clean + tests pass** before a commit is mergeable. CI
enforces it. The live-MySQL integration tests key off `LAZYDB_TEST_MYSQL_URL`
(set to a `mysql://user:pass@host/db` string); without it those tests are no-ops,
so `cargo test` runs anywhere with no external dependencies.

## Code style

The ponytail ladder, as practiced here — stop at the first rung that holds:

- Does it need to exist? Speculative need = skip it, say so in one line. (YAGNI)
- Already in the codebase? Reuse the helper/type/pattern a few files over.
- Stdlib does it? Use it. Native platform feature? Use that.
- One installed dep over a new dep; never add a dep when a few lines do.
- Shortest working diff wins — *once* you've read the whole flow the change touches.

Don't add: an interface with one implementation, a factory for one product, a config
for a value that never changes, scaffolding "for later". Mark deliberate shortcuts
with a comment so they read as intent, not ignorance:

```
// ponytail: global lock, per-account locks if throughput matters
```

Name the ceiling and the upgrade path. Deletion over addition. Bug fix = root cause
in the shared spot, not a guard in every caller. Non-trivial logic leaves behind the
smallest check that fails if it breaks (`assert` in a `__main__`/`#[test]`, no
framework sprawl unless asked).

## Architecture

Three designs the roadmap calls out as worth preserving:

**Trait-based DB layer.** `src/db/mod.rs` defines `Database` (`ping`,
`execute_script`, `schema`, `primary_keys`, `boxed_clone`) — one trait, one impl
module per backend (`src/db/mysql.rs`). `db::open` is a plain `match` on
`Connection.kind`. Adding a backend = one impl module + one match arm; the trait
absorbs the abstraction, no framework needed. `boxed_clone` lets a query run on a
cloned handle on a background thread.

**Background job model.** Long-running DB work runs on a `std::thread` via
`spawn_job` in `src/app/job.rs`. It returns a `std::sync::mpsc::Receiver<JobResult>`;
`Job`/`JobResult` are flat enums (one variant per job kind). The TUI's main loop
polls the channel each tick with `try_recv`, so the UI stays responsive while a
query runs. No async runtime — threads + mpsc is enough at this scale.

**View-aware keymap.** `src/shortcuts.rs` resolves the active `View` from primitive
state via `current_view`: modals win over focus, and autocomplete is a transient
sub-mode of the editor. Each `View` has a `&[Binding]` table; `active(view)` chains
view-specific → common pane chrome → global, and `match_key` takes the first match.
So the same key (`d`, `j`, `q`) can mean different things in different panes without
a state machine — just a per-view table and one resolution function that takes
primitives, not `&App`, keeping this module decoupled.

**Config.** `~/.config/lazydb/connections.toml`, serde round-tripped via
`src/config.rs`. `Config` holds `connections` + `features`; the `[features]` table
is `#[serde(default)]` so config files written before it existed still load. Add a
feature = one field + one `get`/`set` arm + one `LIST` entry.

## Adding a DB backend

See the **Adding a DB backend** section in [README.md](README.md): implement
`Database` in `src/db/<name>.rs`, add `pub mod <name>;` and a `db::open` match arm,
set `kind` on the connection.

## Roadmap

Priorities live in [ROADMAP.md](ROADMAP.md): **P0 done**, **P1/P2 are the active
frontiers** (connection management + query execution gaps, then UX polish). P3 is
project maturity — CI, integration tests, releases, this doc.
