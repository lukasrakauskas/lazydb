# lazydb

A minimal lazygit-style TUI for databases. Built with Rust + ratatui.

Supports **MySQL**, **PostgreSQL**, **SQLite**, and **MSSQL**. The DB layer is a single trait (`src/db/mod.rs`),
so adding a backend = one `match` arm in `db::open` + one impl module.

## Install

```sh
# From source (requires Rust 1.95+)
cargo install lazydb

# Or build from git
cargo install --git https://github.com/lukasrakauskas/lazydb

# Run from this repo
cargo run
```

Pre-built binaries for Linux, macOS (x86_64 + ARM), and Windows are
available on the [Releases](https://github.com/lukasrakauskas/lazydb/releases) page.



## Features

- Save/load connections to `~/.config/lazydb/connections.toml`
- Connect to a saved connection (verified with a ping)
- Write SQL in an in-app editor and run it against the active connection
- Result table with scrollable rows; rows-affected + elapsed for DML
- Togglable features modal (`f`) — settings persist in the config file
  - **Readable binary fields**: render binary columns readably — 16-byte values as UUIDs (`BIN_TO_UUID` style, e.g. `01b4e92f-…`), other binaries as hex (`0x…`); valid-UTF8 bytes pass through as text
- Optional: `lazydb path/to/script.sql` preloads the editor
- SSL/TLS for PostgreSQL
- OS keychain integration
- SSH tunneling via system `ssh` command (no extra deps)
- Per-connection query timeout
- MSSQL backend

## Run

```sh
cargo run
# or preload a script:
cargo run -- query.sql
```

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` | cycle focus: Connections → Editor → Results → Schema |
| `1` / `2` / `3` / `4` | jump focus to Connections / Editor / Results / Schema (not while editing) |
| `j`/`k`, ↑/↓ | move selection (Connections) / move the cell cursor row (Results); viewport auto-follows |
| `h`/`l`, ←/→ | move the cell cursor column (Results); viewport auto-follows |
| `PgUp`/`PgDn` | scroll the viewport a page (Results; cursor stays) · `Home`/`End` jump the cursor to first/last row |
| mouse wheel / trackpad | scroll the viewport (hover the Results pane); the cell cursor stays put |
| `Enter` (Connections) | connect to selected |
| `n` | new connection form (`Enter` save, `Esc` cancel, `Tab` next field, `Ctrl+K` cycle DB type, `Ctrl+T` test) |
| `f` | features modal (`Space` toggle, `j/k` move, `Esc`/`f`/`q` close) — not while editing |
| `d` | delete selected connection |
| `Enter` / `l` (Schema) | expand table → `rows` / `columns` / `constraints` / `indexes`; selecting one prefills + runs the query · `h` collapse |
| `Ctrl+R` / `F5` / `Option+Enter` | run SQL in the editor |
| `Shift+Up` / `Shift+Down` | recall older / newer query (in the editor) |
| `y` (Results) | copy the cursor row as JSON to the clipboard (the highlighted cell's row) |
| `/` (Results) | open a fuzzy filter (`neo_frizbee`, per-cell); type to narrow live, `Enter` commits (keeps filter, nav resumes) · `Esc`/`/` cancel · `/` re-opens to edit |
| `d` (Results) | deselect — drop the row highlight (copy/nav re-select on the next move) |
| mouse click (Results) | select the clicked cell; clears any active filter |
| `Ctrl+S` (Results) | export the whole (unfiltered) result set as CSV to the clipboard |
| `Ctrl+Q` / `Ctrl+C` / `q`* | quit (*`q` types while in the editor) |

Queries run on a background thread so the UI stays responsive.

## Adding a DB backend

1. Implement `Database` in `src/db/<name>.rs` (`ping`, `execute_script`, `boxed_clone`).
2. Add `pub mod <name>;` and a match arm in `db::open`.
3. Set `kind` on the connection — the form's Type row (`Ctrl+K`) cycles `FormState::KINDS`;
   add the string there + a `default_port` arm + an impl module.

## Known limitations (ponytail: deliberate minimal scope)

- SQL is split on `;` naively — `;` inside string literals/comments will mis-split.
- Only the first result set's columns are displayed; later sets in a multi-statement
  run contribute only their `rows_affected`.
- Connection passwords are stored in plaintext in the config file.
- PostgreSQL uses the text protocol (`simple_query`), which materializes the whole result
  set before the row-limit guard truncates (MySQL streams and stops early); a very large
  SELECT still downloads fully. Upgrade path: binary-protocol streaming.
