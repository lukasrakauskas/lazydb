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

## Medium

- **Saved queries** — name a snippet, persist to
  `~/.config/lazydb/snippets.toml` (mirrors the existing `Config` pattern), load
  via a picker. Good for the 5 queries everyone runs daily.
- **Transaction control** — explicit begin/commit/rollback hotkeys +
  `autocommit` toggle. MySQL is autocommit-by-default; the toggle matters once
  multi-statement scripts run. Needs a `set_autocommit` method on `Database`.
- **Destructive-query warning** — confirm modal before `DROP`/`TRUNCATE`/
  `DELETE` without `WHERE`. Reuses the existing modal pattern (features /
  new-connection). Safety at a trust boundary — keep this kind.
- **Find/filter in results** — `/` opens a search row; filters displayed rows
  by substring. Results are in memory (`Vec`), so it's a fold + a separate
  cursor. Useful once result sets exceed the screen.

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

## Skip (common but bloat for this tool)

- Tabbed/multiple editors — one editor + query history covers the same need.
- Themes / colorscheme picker — one coherent palette; config for a value that
  never changes.
- Connection pooling dashboards — admin tooling, not a query client.
- Charting results — wrong tool; export and chart elsewhere.

## Recommended order

1. Query history
2. Schema browser pane
3. EXPLAIN
4. CSV export

All four are small, and the first two compound with what's already built.
