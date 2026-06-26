# Roadmap

Candidate features for lazydb, ranked by value-for-effort in *this* codebase.
Each note records where it plugs in and whether it fits the minimal-TUI ethos.

## Quick wins

- ~~**Query history**~~ ✅ done — `Ctrl+Up`/`Ctrl+Down` in the editor recalls prior SQL from
  a ring buffer in `App`. ~30 lines, no deps. Highest value-per-line for a SQL
  tool. Plugs into the existing editor + `run_query`.
- ~~**Schema browser pane**~~ ✅ done — 4th focusable pane listing `tables → columns`
  on connect, used by autocomplete). A 4th focusable pane (or a `Tab`-cycled
  overlay inside Connections) listing `tables → columns` makes that data
  visible. ~60 lines; reuses loaded data.
- ~~**Copy row / export results**~~ ✅ done — `y` copies the selected row as JSON; `Ctrl+S` exports the result set as CSV
  current result set to CSV/JSON. `rows: Vec<Vec<String>>` is already in
  `Output::Table`. CSV needs no dep (`std::fs` + comma join + quote-escape).
- **EXPLAIN** — prefix the query with `EXPLAIN` via a toggle (`e`) or
  `Ctrl+E`. Zero new infra; reuses `execute_script` + the results table.
- ~~**Destructive-query warning**~~ ✅ done — confirm modal before `DROP`/`TRUNCATE`/
  `DELETE` without `WHERE`. Reuses the existing modal pattern (features /
  new-connection). Safety at a trust boundary — keep this kind.
- **Edit existing connection** — reopen the form with a saved connection's fields
  (e.g. `e` on a connection row) so host/credentials can be changed without
  editing TOML by hand. Trivial wiring — `Config::save` already exists.

## Medium

- ~~**Saved queries**~~ 🔲 unimplemented — name a snippet, persist to
  `~/.config/lazydb/snippets.toml` (mirrors the existing `Config` pattern), load
  via a picker. Good for the 5 queries everyone runs daily.
- ~~**Transaction control**~~ 🔲 unimplemented — explicit begin/commit/rollback hotkeys +
  `autocommit` toggle. MySQL is autocommit-by-default; the toggle matters once
  multi-statement scripts run. Needs a `set_autocommit` method on `Database`.
- ~~**Find/filter in results**~~ ✅ done — `/` opens a search row; filters displayed rows
  by substring. Results are in memory (`Vec`), so it's a fold + a separate
  cursor. Useful once result sets exceed the screen.
- **PostgreSQL / SQLite backend** — `Database` trait (`src/db/mod.rs`) was
  designed for this. Each backend = one `impl` module + a match arm in
  `db::open`. Postgres adds a `tokio-postgres` or `sqlx` dep; SQLite adds
  `rusqlite`. Single biggest feature gap vs the README promise.
- **Row counts in schema browser** — annotate each table in the schema pane with
  its estimated row count (`SHOW TABLE STATUS` / `pg_class.reltuples` /
  `sqlite_stat1`). Small, makes the viewer meaningfully more useful.
- **Schema search** — filter the table list in the schema pane by name (reuses
  the existing filter pattern from Results). Helpful once you have 50+ tables.
- **NULL display** — render `NULL` cells in the results table with a distinct
  color (e.g. dim gray + italic) so they're visually distinct from the literal
  string `"NULL"`. Trivial — one check in `draw_table`.
- **Result limit override** — show a user-adjustable `LIMIT n` in the results
  title bar; increment/decrement with `+`/`-` or a hotkey. Prevents accidental
  `SELECT * FROM millions` from locking the UI.

## Big (real value, real cost — question whether they fit)

- **Inline data editing** — edit a cell, write back with
  `UPDATE ... WHERE pk=...`. Requires the primary key (another
  `INFORMATION_SCHEMA` query) and a per-table edit mode. Large surface; easy to
  get wrong (data loss). This is where "minimal client" becomes a real tool.
- **SSH tunneling** — connect to DBs behind a bastion. Adds a dep (`russh` or
  shell out to `ssh -L`) and connection lifecycle complexity. High value for
  remote DBs, basically a second product.
- **Multiple result sets / multi-statement** — `execute_script` already splits
  on `;` but keeps only the last result set. Showing all (tabbed or stacked)
  helps when running scripts. Touches `Output` + the results pane.
- **EXPLAIN visualizer** — parse EXPLAIN into a tree widget. Ratatui has no
  tree builtin; hand-rolled. EXPLAIN-as-table (above) gets 80% of the value at
  5% of the cost.
- **SSL/TLS per-connection** — many production DBs require TLS. Currently the
  mysql crate passes the URL verbatim; adding `ssl-mode=REQUIRED` or a CA cert
  path needs connection-string plumbing in `db::open` and a new form field.
- **Keyboard config** — allow users to remap bindings via the config file
  (`keys.toml`). The `shortcuts.rs` keymap is a single match table; swapping a
  binding is ~30 lines of deserialization. Big surface area (every action is
  remappable) but the schema is small and stable.

## Skip (common but bloat for this tool)

- Tabbed/multiple editors — one editor + query history covers the same need.
- Themes / colorscheme picker — one coherent palette; config for a value that
  never changes.
- Connection pooling dashboards — admin tooling, not a query client.
- Charting results — wrong tool; export and chart elsewhere.
- Import/export connections — YAML/JSON dump of `connections.toml`; a shell
  one-liner does the same thing. Not worth a UI surface.
- Table creation/alter UI — SQL DDL is already expressible in the editor;
  a visual schema designer is a separate product.

## Recommended order

1. ~~Query history~~ ✅ done
2. ~~Schema browser pane~~ ✅ done
3. ~~Find/filter in results~~ ✅ done
4. ~~Destructive-query warning~~ ✅ done
5. EXPLAIN
6. Edit existing connection
7. PostgreSQL backend
8. Row counts in schema browser
9. Saved queries
