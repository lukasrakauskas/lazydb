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
- [ ] DB type picker in the connection form (currently hardcoded `"mysql"`).
      Deferred: a picker with one wired backend is speculative UI; add with the
      second backend.
- [x] Test-connection button in the form (reuse existing Ping job).  `Ctrl+T` in
      the form opens + pings; result shows in the status line.
- [ ] SSL/TLS options.

### Query execution

- [x] Query cancellation (Esc or Ctrl+C while running). A `CancelSlot`
      (`Arc<AtomicU32>`) carries the running query's connection id; cancel sends
      `KILL QUERY` from a side connection. Best-effort (needs PROCESS/SUPER).
- [x] Configurable query timeout — `query_timeout_secs` in config becomes the
      pool's socket `read_timeout`.
- [ ] Display all result sets from multi-statement runs, not just the first
      (tabbed or stacked results).  Deferred: needs an `Output` refactor (active
      result-set index) — separate ticket.
- [x] Row limit guard — `select_limit` in config caps the fetch per result set
      and sets a `truncated` flag shown in the results title. Server-side LIMIT
      injection / pagination deferred (parsing risk).

### Second backend

- [ ] PostgreSQL. The `Database` trait exists; this validates the abstraction and roughly
      doubles the addressable audience. SQLite after (easy win: file-path-only connections,
      great for demos and tests).

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
- [ ] Persist per-connection state: last database, query history scoped per connection.

## P3 - Project maturity

- [x] CI: fmt + clippy + test on every push (GitLab CI or GitHub Actions).
- [x] Integration tests against real MySQL (docker-compose or testcontainers) - the DB
      layer currently has zero live-path coverage.
- [ ] Release pipeline: tagged releases with prebuilt binaries (cargo-dist),
      publish to crates.io / Homebrew tap.
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
- [ ] SSH tunneling - connect to DBs behind a bastion. Adds a dep (`russh` or shell out
      to `ssh -L`) and connection lifecycle complexity. High value for remote DBs;
      basically a second product.

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
