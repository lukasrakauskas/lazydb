# lazydb — from hobby to professional

**Audience**: contributors, the author, anyone evaluating lazydb for daily use.

---

## 1. Current state (v0.2.0)

### What exists

| Layer | Status | Lines |
|-------|--------|-------|
| MySQL backend | Stable, tested | 416 |
| PostgreSQL backend | Stable, tested (5 live tests) | 545 |
| SQLite backend | Stable, tested | 319 |
| MSSQL backend | Stable, tested | ~225 |
| Multi-pane TUI (ratatui) | Fully functional | ~2,900 |
| Connection manager (save/load/edit/delete) | Complete | — |
| SQL editor (multi-line, syntax-highlighted) | Complete | 132 |
| Results grid (keyboard + mouse, scroll, filter, sort) | Complete | 550 |
| Schema browser (tables/views/columns/PKs/indexes) | Complete | — |
| Fuzzy row filter (neo_frizbee) | Complete | 176 |
| SQL highlighting (hand-rolled, ~90% coverage) | Complete | 314 |
| SQL tokenizer-aware statement splitter | Complete | 225 |
| Autocomplete (keywords + schema) | Complete | 226 |
| Query cancellation (MySQL/PG via side-conn) | Complete | — |
| Multi-result-set display | Complete | — |
| Row-level cell editing (inline UPDATE) | Complete | — |
| Row insert/delete from results | Complete | — |
| Export (CSV/JSON/JSONL to clipboard + file) | Complete | — |
| Cell inspect popup (scrollable) | Complete | — |
| Column sort (asc/desc/none) | Complete | — |
| Clipboard integration (arboard) | Complete | — |
| Config (TOML, `~/.config/lazydb/`) | Complete | 173 |
| Env-var reference in connection fields | Complete | — |
| Destructive-query guard | Complete | — |
| Confirmation modals (delete, destructive) | Complete | — |
| Background job model (threads + mpsc) | Complete | 76 |
| View-aware keybindings with shortcut bar | Complete | 1,664 |
| Debug logging (`--log-file`) | Complete | 111 |
| SSL/TLS (MySQL, PostgreSQL) | Complete | — |
| OS keychain integration | Complete | — |
| SSH tunneling (system `ssh`) | Complete | 86 |
| Per-connection query timeout | Complete | — |
| Session persistence (history, last-conn restore) | Complete | — |
| Release pipeline (cargo-dist, binaries) | Complete | — |
| CI (fmt + clippy + test) | Complete | — |
| Tests | ~97 (24 integration) | 653+ |
| clippy `deny`-all lints | Complete | — |
| deny.toml dependency audit | Complete | — |
| Keybinding reference | README | — |
| CONTRIBUTING.md + architecture notes | Written | 84 |

### What's half-done

- **Oracle**: No backend exists. Enterprise shops that use Oracle are still out of luck.

### What's consciously deferred (ponytail markers)

Single-thread job model, single `Client` behind Mutex (postgres), naive row-delete WHERE (not PK-based), EXPLAIN on first statement only, SQLite no cancellation, file-path-only SQLite, multiple connections/tabs, ER diagrams, query builder, theming, keybinding customization, telemetry, docs site, Oracle backend, multi-tab.

---

## 2. Competitive analysis

### The GUI giants

| Tool | Startup | RAM | DB count | Pricing |
|------|---------|-----|----------|---------|
| DataGrip | ~10s | ~1 GB | 30+ | $99/yr |
| DBeaver | ~15s | ~800 MB | 100+ | Free |
| TablePlus | ~1s | ~150 MB | ~20 | $89 lifetime |
| Beekeeper | ~3s | ~200 MB | 20+ | Free / $9/mo |

All are Java (DataGrip, DBeaver) or Electron (Beekeeper, TablePlus). All provide SSH tunnels, SSL, schema tree, data grid editing, export to multiple formats, query history, and tabbed editors.

**Their pain points (and our opportunity)**:
- DBeaver: bloated, Eclipse runtime, sluggish on large results
- DataGrip: subscription fatigue, overkill for quick lookups
- TablePlus: per-device licensing, no team features
- Beekeeper: Electron memory, laggy with large tables
- **All GUI tools**: non-trivial startup time, heavyweight, don't fit a terminal workflow

### The TUI competition

| Project | Lang | DB support | Maturity |
|---------|------|------------|----------|
| pgcli | Python | PG only | Mature REPL, ~12k stars |
| mycli | Python | MySQL only | Mature REPL, ~7.5k stars |
| usql | Go | ~10+ DBs | Mature REPL, ~9k stars |
| aymenhmaidi/lazydb | Go (bubbletea) | PG/MySQL/SQLite/Mongo/Redis | Functional beta |
| HalxDocs/lazydb | Go (bubbletea) | PG/MySQL/SQLite | Functional alpha |
| june3141/lazydb | Rust (ratatui) | PG only | Early alpha |

**Key insight**: No TUI tool has achieved "lazygit for databases" mindshare. The existing projects are early-stage solo efforts. There is no dominant TUI database client — the category is up for grabs.

### What professionals expect (minimum viable professional)

| Capability | TablePlus | DataGrip | DBeaver | lazydb v0.2 |
|------------|-----------|----------|---------|-------------|
| Multi-DB | ~20 | 30+ | 100+ | 4 |
| SSH tunnel | Yes | Yes | Yes | Yes |
| SSL/TLS | Yes | Yes | Yes | Yes |
| Keychain | macOS | Built-in | Master pass | Yes (optional) |
| Tabbed editors | Yes | Yes | Yes | **Single** |
| Schema tree | Yes | Yes | Yes | Yes |
| Cell editing | Staged | Inline | Inline | Inline |
| Export (any format) | Yes | Yes | Yes | CSV/JSON |
| Query history | Session + saved | Persistent | Persistent | Persistent |
| Autocomplete | Yes | Deep | Yes | Basic |
| DDL tools | No | Migration gen | ER diagram | **No** |
| Dark mode | Native | Theme | Theme | Yes |
| Binary distribution | DMG | JAR | Installer | Prebuilt binaries |
| Premium pricing | $89 | $99/yr | Free | FOSS |

**The gap is narrow — and narrowing.** A terminal tool does not need to match DataGrip's SQL intelligence. It needs:

1. **Connect securely** (SSL + SSH + keychain) to any database ✅
2. **Browse schema** quickly (tree view, filter, search)
3. **Query and see results** (editor + grid, fast export)
4. **Edit data** (inline cell editing, insert, delete)
5. **Stay out of the way** (instant startup, zero-config, keyboard-first)

lazydb satisfies all five for MySQL/PG/SQLite/MSSQL. The remaining gaps are Oracle, saved queries, and multi-tab.

---

## 3. Roadmap to professional viability

Organized into phases. Each phase ends with "a professional could use this at work without apologizing for it."

### Phase 1 — Secure connectivity (trust barrier) ✅

**Done.** You can connect to a production RDS/CloudSQL instance over SSL without storing a password in plaintext, even through a bastion.

| Item | Status |
|------|--------|
| SSL/TLS for PostgreSQL | ✅ Feature-gated `postgres-native-tls` |
| SSL/TLS for MySQL | ✅ Built-in via `mysql_config` |
| OS keychain integration | ✅ `keyring` crate, feature-gated |
| SSH tunnel | ✅ Sidecar `ssh -L` process |
| Per-connection query timeout | ✅ Config field, per-backend |

### Phase 2 — Enterprise backends

| Item | Status |
|------|--------|
| MSSQL backend | ✅ `tiberius` crate, TDS protocol, feature-gated |
| Oracle backend | ❌ No good Rust crate — `sibyl` or OCI-based, high effort |

**Gate (partial)**: MSSQL works. Oracle remains the last missing enterprise backend.

### Phase 3 — Result-set performance at scale

**The tool needs to handle million-row tables without freezing or OOMing.**

| Item | Effort | Why |
|------|--------|-----|
| PG streaming binary protocol | High (query_raw/RowIter instead of simple_query) | Current PG materializes full results before truncating |
| Virtual scrolling in results grid | Medium (render only visible rows) | 10k+ rows already causes UI stutter |
| Configurable page size + fetch-more | Medium | Users don't always want the first 1000 rows |
| Cancel slot: track conn_id per backend | Low | Already done for MySQL/PG; SQLite can't cancel |

**Gate**: After this phase, SELECT * FROM tables_1m rows renders instantly and doesn't lock the UI.

### Phase 4 — Session persistence & tabs

| Item | Status |
|------|--------|
| Persistent query history (per-connection) | ✅ Appended to config file, max 100 entries |
| Session restore (last DB, etc.) | ✅ `last_connection` saved on quit, restored on launch |
| Saved queries / snippets | ❌ Not yet — TOML file + picker UI pending |
| Multi-tab / multiple queries at once | ❌ State refactor, high effort |

**Gate (partial)**: Opening lazydb restores your last connection and query history. Saved snippets and tabs are still deferred.

### Phase 5 — Export & data movement

**Professionals need to get data out in the format their stakeholders need.**

| Item | Effort | Why |
|------|--------|-----|
| Export to XLSX | Low (calamine/rust_xlsxwriter) | Business users want Excel |
| Export to Parquet | Medium (arrow crate) | Data engineers |
| Export to INSERT statements | Low | Migration scripts, data seeding |
| Export filtered view | Low | Current export is unfiltered only |
| Import CSV/JSON | Medium | Professionals load data too |

**Gate**: After this phase, you can get data in and out in any format a colleague asks for.

### Phase 6 — Transaction control & safety

**With great power comes undo.**

| Item | Effort | Why |
|------|--------|-----|
| Begin/commit/rollback hotkeys | Medium (trait method, per-impl) | Auto-commit is scary without rollback |
| Autocommit toggle | Low (config + trait method) | Paired with explicit txns |
| Row-level change staging | High (local buffer + diff view) | TablePlus-killer feature — approve changes before committing |
| Undo last mutation | High | Hard to implement generically but huge trust-builder |

**Gate**: After this phase, you can safely experiment with destructive changes.

### Phase 7 — Project maturity

| Item | Status |
|------|--------|
| `cargo-dist` release pipeline | ✅ Prebuilt binaries for macOS/Linux/Windows |
| crates.io publish | ✅ `cargo install lazydb` |
| `--help` + `--version` | ✅ Hand-rolled CLI args |
| Semantic versioning + changelog | ✅ CHANGELOG.md + 0.2.0 tag |
| Homebrew tap / winget / scoop | ❌ Not yet |
| Telemetry opt-in | ❌ Deferred |
| Security audit of dependencies | ❌ Deferred |
| Website / docs site | ❌ Deferred |

**Gate (partial)**: `cargo install lazydb` and prebuilt binaries work. Homebrew and docs site are still pending.

### Phase 8 — Power-user features

**These are what turn "I can use this" into "I prefer this."**

| Item | Effort | Why |
|------|--------|-----|
| EXPLAIN visualization | Medium | Tree render of query plan |
| DDL completion / templates | Low | `Ctrl+Space` for `CREATE TABLE` skeleton |
| Schema compare / diff | High | Useful but scope risk — strict v2 material |
| Table data search across columns | Low | Current filter is row-focused |
| Cell-edit history per row | Medium | See/edit what you changed before committing |
| Auto-expand related tables (FK navigation) | Medium | Click a FK cell -> show the referenced row |
| Splittable outputs | High | Run two queries, compare results side-by-side |

**Gate**: After this phase, you prefer lazydb over TablePlus for daily work.

---

## 4. What to keep (don't change what works)

- **Trait-based DB layer** (`Database` trait, one impl per backend). Adding a backend is one file + one match arm. This is the right abstraction.
- **Background job model** (threads + mpsc). Simple, testable, no async runtime. Keep until there's a concrete reason to switch.
- **View-aware keymap** (shortcuts.rs). The view resolution is clean and extensible. The per-view binding tables make adding new features safe.
- **Ponytail minimalism**. The `ponytail:` comment convention, the YAGNI reflex, the refusal to add deps for what stdlib does — this keeps the project at 10k lines vs. 50k.
- **Config as TOML in `~/.config/`**. Standard XDG location, serde round-trip, `#[serde(default)]` for backward compat. No reason to change.
- **ratatui on crossterm**. Stable, well-maintained, cross-platform. The ecosystem is active.

---

## 5. What to change or add (structural)

### Backend trait evolution

The `Database` trait currently has 8 methods. As features are added (transaction control, SSL config, session variables), the trait should grow carefully. Each new method should be:

1. **Default-implemented** where possible (so existing backends compile without changes)
2. **Documented with its ceiling** (`ponytail: returns Unsupported for X until Y`)

### Dependency strategy

| Current | Suggestion | Why |
|---------|------------|-----|
| hand-rolled `--log-file` | Keep (18 lines) | Not worth clap's compile time |
| hand-rolled env-var resolution | Keep (12 lines) | Trivial, works |
| arboard clipboard | Keep | Works cross-platform |
| neo_frizbee fuzzy filter | Keep | SIMD-powered, excellent |
| `mysql` crate (Rust) | Keep | Minimal, active |
| `postgres` crate (Rust) | Keep | Standard, active |
| `rusqlite` (bundled) | Keep | De facto standard |
| No async runtime | Keep | Threads + mpsc is enough at this scale |
| hand-rolled CI | Keep | Works, simple yml |

**Add only when needed**:
- `keyring` crate for OS keychain integration
- `russh` or shell-out for SSH tunneling
- `tiberius` for MSSQL
- `rust_xlsxwriter` for XLSX export
- `clap` for CLI parsing

### Test strategy

Current test count (~97) is decent. Grow incrementally:
- Each new backend → minimum 3 live-path tests (connect, query, schema)
- Each new DB trait method → 1 unit test for error handling
- Each new UI feature → 2-3 app-state tests
- **Do not add test "for coverage"** — YAGNI applies

Critical missing test: **no end-to-end test** that starts the TUI, simulates keystrokes, and asserts screen output. This is hard with ratatui (no built-in test harness) but a smoke-test that constructs `App`, calls `handle_key` sequences, and inspects state would catch regressions.

---

## 6. Positioning & messaging

### Current positioning
> A minimal lazygit-style TUI for databases.

### Proposed positioning (for professional use)
> The terminal database client that loads instantly, connects securely, and gets out of your way.

### Key differentiators vs. incumbents
- **Instant startup** (vs. 5-15s for GUI tools)
- **Keyboard-first** (vs. mouse-dominated GUI workflows)
- **Single binary, no runtime deps** (vs. Java JRE or Electron)
- **Terminal-native** (works over SSH, tmux, CI/CD shells)
- **Opinionated, minimal** (vs. DataGrip's 10,000 options)

### Target audience tier
1. **Primary**: Backend engineers, SREs, data engineers who live in the terminal
2. **Secondary**: Anyone who uses `psql`/`mycli` today and wants a visual schema + grid
3. **Long tail**: DBAs who need quick access across multiple DB engines
4. **Not for**: Data analysts who need pivot tables, charting, or collaborative editing

---

## 7. Summary: what "professional" means by phase

| Phase | Professional can… | Status |
|-------|-------------------|--------|
| 1 | Connect to a production RDS/CloudSQL instance over SSL through a bastion | ✅ Done |
| 2 | Use Oracle or SQL Server at their company | 🟡 MSSQL done, Oracle pending |
| 3 | Browse 1M-row tables without freezing | ✅ Virtual scrolling (width calc capped at 1000 rows) |
| 4 | Resume a session, run a saved query, open multiple tabs | 🟡 History + restore + snippets done, tabs pending |
| 5 | Export data as CSV/XLSX/INSERT for a colleague | ✅ CSV/JSON/XLSX/INSERT all done |
| 6 | Roll back a mistaken UPDATE with one keystroke | ✅ Begin/commit/rollback hotkeys, autocommit toggle |
| 7 | `brew install lazydb` on a company laptop with security team approval | 🟡 cargo-install + binaries + formula done, tap pending |
| 8 | Prefer lazydb over their previous tool | 🟡 Phases 1-7 mostly done; PG streaming, page-size, Oracle, tabs remain |

---

## 8. Non-goals (reaffirmed)

- **ER diagrams** — not what a TUI does well
- **Query builder GUI** — against the tool's ethos
- **Team/collaboration features** — enterprise feature, huge scope
- **Dashboard/charting** — use a BI tool
- **AI-powered SQL generation** — table stakes in 2026 but orthogonal to this spec
- **Theming/keybinding customization** — wait until defaults are settled
- **Windows support** — it works today (crossterm), but no special effort

---

*Generated from codebase audit (v0.2.0, ~9,904 lines, ~97 tests, 3 backends) and competitive analysis. See ROADMAP.md for the tracked P0-P3 list; this spec supersedes the old roadmap's scope and sequencing.*
