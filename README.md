# lazydb

A minimal lazygit-style TUI for databases. Built with Rust + ratatui.

Currently supports **MySQL**. The DB layer is a single trait (`src/db/mod.rs`),
so adding a backend = one `match` arm in `db::open` + one impl module.

## Features

- Save/load connections to `~/.config/lazydb/connections.toml`
- Connect to a saved connection (verified with a ping)
- Write SQL in an in-app editor and run it against the active connection
- Result table with scrollable rows; rows-affected + elapsed for DML
- Optional: `lazydb path/to/script.sql` preloads the editor

## Run

```sh
cargo run
# or preload a script:
cargo run -- query.sql
```

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` | cycle focus: Connections → Editor → Results |
| `j`/`k`, arrows | move selection (Connections) / scroll rows (Results) |
| `h`/`l`, ←/→ | scroll columns horizontally (Results) |
| `PgUp`/`PgDn`, `Home`/`End` | scroll rows by page / jump (Results) |
| `Enter` (Connections) | connect to selected |
| `n` | new connection form (`Enter` save, `Esc` cancel, `Tab` next field) |
| `d` | delete selected connection |
| `Ctrl+R` / `F5` | run SQL in the editor |
| `Ctrl+Q` / `Ctrl+C` / `q`* | quit (*`q` types while in the editor) |

Queries run on a background thread so the UI stays responsive.

## Adding a DB backend

1. Implement `Database` in `src/db/<name>.rs` (`ping`, `execute_script`, `boxed_clone`).
2. Add `pub mod <name>;` and a match arm in `db::open`.
3. Set `kind` on the connection (the form hardcodes `"mysql"` — generalize when a 2nd backend lands).

## Known limitations (ponytail: deliberate minimal scope)

- SQL is split on `;` naively — `;` inside string literals/comments will mis-split.
- Only the first result set's columns are displayed; later sets in a multi-statement
  run contribute only their `rows_affected`.
- Connection passwords are stored in plaintext in the config file.
