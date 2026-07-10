# lazydb Roadmap v2

Fresh assessment based on the current codebase (v0.1.0, MySQL-only TUI). Ordered by impact.
Strengths worth preserving: trait-based DB layer, view-aware keymap, background job model,
~69 unit tests, ponytail-style minimalism.

## P0 - Correctness and safety (before anything else)

- [x] Fix `save_form()` status bug: error message from a failed save is overwritten by "Saved."
      (`src/app/mod.rs`).
- [x] Escape/validate table names in `primary_keys()` - table name is interpolated directly
      into SQL (`src/db/mysql.rs`). Use parameterized query against INFORMATION_SCHEMA.
- [x] Confirmation prompt before deleting a connection (`d` currently deletes instantly).
- [x] Harden the destructive-query guard: cover UPDATE without WHERE, multi-statement
      scripts where only one statement is destructive.
- [x] Replace naive `;` statement splitting with a tokenizer-aware splitter (strings,
      comments, DELIMITER). This silently corrupts real-world scripts today.
- [x] Report script preload failures (`lazydb missing.sql` currently fails silently).
- [x] Fix LICENSE copyright holder (currently "The Bootstrap Authors" placeholder).

## P1 - Core gaps that block daily-driver use

### Connection management

- [x] Edit existing connection (currently create + delete only).  `e` on a
      connection opens the form pre-filled; save overwrites in place.
- [x] Stop storing passwords in plaintext TOML — `${VAR}` / `$VAR` references in
      connection fields are resolved from the environment at open time (unset →
      left literal so a missing secret is visible). OS keychain (keyring crate) is
      the remaining upgrade.
- [x] DB type picker in the connection form (was hardcoded `"mysql"`). The
      form's Type row cycles via `Ctrl+K` (`FormState::KINDS`); the default port
      auto-swaps with the kind. Landed with the postgres backend.
- [x] Test-connection button in the form (reuse existing Ping job).  `Ctrl+T` in
      the form opens + pings; result shows in the status line.
- [x] SSL/TLS options.

### Query execution

- [x] Query cancellation (Esc or Ctrl+C while running). A `CancelSlot`
      (`Arc<AtomicU32>`) carries the running query's connection id; cancel sends
      `KILL QUERY` from a side connection. Best-effort (needs PROCESS/SUPER).
- [x] Configurable query timeout — `query_timeout_secs` in config becomes the
      pool's socket `read_timeout`.
- [x] Display all result sets from multi-statement runs, not just the first
      (Tab/Shift+Tab cycles between them).  Each statement's result is preserved
      in `ExecutionResult::all_results`; `App` stores the full list and the
      active index.
- [x] Row limit guard — `select_limit` in config caps the fetch per result set
      and sets a `truncated` flag shown in the results title. Server-side LIMIT
      injection / pagination deferred (parsing risk).

### Second backend

- [x] PostgreSQL. The `Database` trait now has two impls (`mysql`, `postgres`);
      `db::open` matches on `kind`. Shared single `Client` behind a `Mutex` (one
      query at a time), query timeout via server-side `statement_timeout`, cancel via
      `pg_cancel_backend` on a side connection. SQLite next (file-path-only, easy).

#### PostgreSQL — deliberate shortcuts (upgrade paths)
Each is a `ponytail:` comment in `src/db/postgres.rs` (the last one in
`src/app/util.rs`); listed here so they're tracked, not lost. Ceilings/upgrades:

- [ ] Streaming row cap. `simple_query` (text protocol) materializes the whole
      result before `select_limit` truncates — a huge SELECT downloads fully,
      unlike mysql which streams and stops early. Upgrade: binary-protocol
      `query_raw` / `RowIter` for a streaming cap.
- [ ] Connection pool. One shared `Client` behind a `Mutex` (one query at a
      time, matches the TUI's usage). Upgrade: `r2d2` pool if concurrent
      queries ever run.
- [ ] Multi-schema browsing. `schema()` and `primary_keys()` filter
      `current_schema()` only — tables in other search_path schemas don't
      appear. Upgrade: scan `current_schemas(false)` minus the system schemas;
      pass an explicit schema to `primary_keys` if needed.
- [ ] Structured index view. The schema pane's Indexes uses `pg_indexes`
      (indexname + indexdef string), not a cols/unique breakdown. Upgrade:
      `pg_index` join for a structured view.
- [ ] Configurable connect timeout. Hardcoded 10s cap so a firewalled host
      can't hang the worker (a hung connect can't be cancelled — `kill_query`
      opens a side conn to the same dead host). Upgrade: per-connection or
      config-driven.
- [ ] Error detail. `pg_err` surfaces the server message only (no
      `detail()`/`hint()`/SQLSTATE `code()`) — postgres `Error::Display` is
      bare "db error" otherwise. Upgrade: include when a bare message stops
      being enough.
- [x] `readable_binary` parity. No-op on postgres: text-protocol bytea already
      renders as `\x..` hex (mysql needs it for its `Value` enum). Only
      relevant again if we move to the binary protocol (raw bytea bytes).
      `bytes_to_string` is now `pub` and reused by the sqlite backend too.
- [ ] Quoted-identifier cell edit. `extract_table_name` (`src/app/util.rs`)
      only understands backtick-quoted identifiers (mysql); a postgres query
      like `SELECT * FROM "MixedCase"` won't yield a table name, so cell-edit
      can't build the UPDATE. Upgrade: also strip a leading `"`.
- [ ] SSL/TLS. Built with `NoTls`; remote/cloud Postgres usually needs it.
      Already tracked above ("SSL/TLS options") — postgres makes it concrete.
- [x] SQLite. `src/db/sqlite.rs`, `rusqlite` dependency, listed in
      `FormState::KINDS`, wired in `db::open`. No query cancellation (sync conn,
      single-thread — `kill_query` returns an error). 6 unit tests (round-trip,
      schema, primary keys, limit truncation, DML affected-rows).

## P2 - UX polish

### Results pane

- [ ] Column sort (cycle asc/desc/none on the cursor column).
- [ ] Column hide/resize; unicode-width-aware column sizing (CJK/emoji break alignment).
- [ ] Full-cell inspect popup for long values (JSON blobs, long text) instead of truncation.
- [ ] Export to file (CSV/JSON with a path prompt), not clipboard-only; export the
      filtered view as an option.
- [ ] NULL-aware cell editing (distinguish NULL from empty string, allow setting NULL).
- [ ] Row insert and row delete from the results pane (generate INSERT/DELETE like cell
      edit generates UPDATE).

### Editor

- [ ] Save editor buffer to file (there is load-on-startup but no save).
- [ ] Resizable/collapsible editor pane (fixed 8 rows wastes space on big terminals,
      cramps on small ones).
- [ ] Format-SQL action.
- [ ] Autocomplete: resolve table aliases (`t.` -> columns of aliased table).
- [ ] EXPLAIN shortcut with readable output.

### Schema pane

- [ ] Filter/search within the schema tree (reuse the fuzzy filter).
- [ ] Show views, procedures, triggers - not just tables.
- [ ] Include empty tables (current INFORMATION_SCHEMA.COLUMNS query skips them).

### General

- [ ] Full-screen help modal (`?`) with all keybindings; repurpose current `?` debug
      toggle to a hidden flag.
- [ ] `--help`/`--version` CLI flags (clap or hand-rolled).
- [ ] Built-in clipboard (arboard/OSC 52) instead of shelling out to pbcopy/xclip.
- [x] Persist per-connection state: last database, query history scoped per connection.

## P3 - Project maturity

- [x] CI: fmt + clippy + test on every push (GitLab CI or GitHub Actions).
- [x] Integration tests against real MySQL (docker-compose or testcontainers) - the DB
      layer currently has zero live-path coverage.
- [x] Release pipeline: tagged releases with prebuilt binaries (cargo-dist),
      publish to crates.io (Homebrew tap still pending).
- [x] rustfmt.toml + clippy lints committed; deny.toml for dependency audit.
- [x] Structured logging behind a `--log-file` flag (helps debug terminal-mangling bugs
      that are invisible in a TUI).
- [x] CONTRIBUTING.md + short architecture doc (the trait/job/keymap design deserves
      a paragraph each).

## Leftover ideas from v1 roadmap

Candidates from the original ROADMAP.md not yet re-prioritized into a tier above.

- [ ] Saved queries - name a snippet, persist to `~/.config/lazydb/snippets.toml`
      (mirrors the `Config` pattern), load via a picker. Good for the 5 queries everyone
      runs daily.
- [ ] Transaction control - explicit begin/commit/rollback hotkeys + autocommit toggle.
      MySQL is autocommit-by-default; the toggle matters once multi-statement scripts
      run. Needs a `set_autocommit` method on `Database`.
- [ ] Row counts in schema browser - annotate each table with an estimated row count
      (`SHOW TABLE STATUS` / `pg_class.reltuples` / `sqlite_stat1`).
- [x] SSH tunneling - connect to DBs behind a bastion. Uses system `ssh -L`, no extra deps.

## Deliberately out of scope for now

- Multiple simultaneous connections / tabs - large state refactor, low demand at this stage.
- Theming/config for keybindings - wait until the default bindings settle.
- ER diagrams, query builder GUIs - against the tool's lazygit-style ethos.

## Suggested sequencing

1. P0 in one hardening pass (small diffs, mostly localized).
2. CI first from P3 (cheap, protects everything after).
3. Connection editing + password handling (P1) - biggest trust blocker.
4. Query cancel + multi-result sets (P1).
5. PostgreSQL backend (P1) - after cancel/timeout work so the trait grows once.
6. P2 items opportunistically, prioritizing sort, export-to-file, help modal.
