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
| CI (fmt + clippy + test) | Complete | — |
| Tests | ~97 (24 integration) | 653+ |
| clippy `deny`-all lints | Complete | — |
| deny.toml dependency audit | Complete | — |
| Keybinding reference | README | — |
| CONTRIBUTING.md + architecture notes | Written | 84 |

### What's half-done

- **SSL/TLS**: No backend has it. MySQL crate supports it via `mysql_config`; postgres needs `SslMode` and a `tls`-feature-gated dependency; SQLite doesn't need it.
- **Keychain integration**: Env-var references exist but OS keychain (macOS Keychain, Linux secret-service/libsecret) is missing. Passwords are still plaintext in the config file.
- **SSH tunneling**: Not supported. Blocks use for remote/cloud databases behind bastions.
- **Query timeout**: MySQL uses pool-level `read_timeout` (not per-query). Postgres uses server-side `statement_timeout`. SQLite uses `busy_timeout`.
- **Oracle / MSSQL**: No backends exist. Enterprise shops need them.
- **Release pipeline**: No `cargo-dist`, no Homebrew tap, no prebuilt binaries, not on crates.io.
- **Session persistence**: Query history is in-memory only. No per-connection state restoration.

### What's consciously deferred (ponytail markers)

Single-thread job model, single `Client` behind Mutex (postgres), naive row-delete WHERE (not PK-based), EXPLAIN on first statement only, one schema at a time (postgres), SQLite no cancellation, file-path-only SQLite, `--help`/`--version` (hand-rolled CLI arg), multiple connections/tabs, ER diagrams, query builder, theming, keybinding customization.

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
| Multi-DB | ~20 | 30+ | 100+ | 3 |
| SSH tunnel | Yes | Yes | Yes | **No** |
| SSL/TLS | Yes | Yes | Yes | **No** |
| Keychain | macOS | Built-in | Master pass | **Plaintext** |
| Tabbed editors | Yes | Yes | Yes | **Single** |
| Schema tree | Yes | Yes | Yes | Yes |
| Cell editing | Staged | Inline | Inline | Inline |
| Export (any format) | Yes | Yes | Yes | CSV/JSON |
| Query history | Session + saved | Persistent | Persistent | In-memory |
| Autocomplete | Yes | Deep | Yes | Basic |
| DDL tools | No | Migration gen | ER diagram | **No** |
| Dark mode | Native | Theme | Theme | Yes |
| Binary distribution | DMG | JAR | Installer | **cargo only** |
| Premium pricing | $89 | $99/yr | Free | FOSS |

**The gap is wide — but narrow for a TUI.** A terminal tool does not need to match DataGrip's SQL intelligence. It needs:

1. **Connect securely** (SSL + SSH + keychain) to any database
2. **Browse schema** quickly (tree view, filter, search)
3. **Query and see results** (editor + grid, fast export)
4. **Edit data** (inline cell editing, insert, delete)
5. **Stay out of the way** (instant startup, zero-config, keyboard-first)

No TUI tool satisfies all five today. That's the target.

---

## 3. Roadmap to professional viability

Organized into phases. Each phase ends with "a professional could use this at work without apologizing for it."

### Phase 1 — Secure connectivity (trust barrier)

**Without this, you can't connect to production or corporate databases.** Currently the #1 blocker.

| Item | Effort | Why |
|------|--------|-----|
| SSL/TLS for PostgreSQL | Low (feature-gate `postgres-native-tls` or `openssl`) | Cloud PG requires it (~60% of PG installs) |
| SSL/TLS for MySQL | Low (`mysql_config` already supports it) | Cloud MySQL/RDS require it |
| OS keychain integration | Medium (keyring crate, fallback to env vars) | Plaintext passwords are a dealbreaker in any org |
| SSH tunnel | High (sidecar SSH process or `russh` crate) | Required for bastion-host architectures |
| Connect timeout per-connection | Low (config field, passed to backend open) | Current hardcoded 10s is wrong if you know your network |

**Gate**: After this phase, you can connect to a production RDS/CloudSQL instance over SSL without storing a password in plaintext, even through a bastion.

### Phase 2 — Enterprise backends

**Without this, you can't work at a company that uses Oracle or SQL Server (~40% of enterprises).**

| Item | Effort | Why |
|------|--------|-----|
| MSSQL backend | Medium (tiberius crate, TDS protocol) | Heavy in .NET shops, financial services |
| Oracle backend | High (no good Rust crate — sibyl or OCI-based) | Heavy in banking, healthcare, legacy |

**Gate**: After this phase, you can use lazydb at any company regardless of DB vendor.

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

**Without this, every session starts from scratch. Professionals expect continuity.**

| Item | Effort | Why |
|------|--------|-----|
| Persistent query history (per-connection) | Medium (append to history file) | Current in-memory history is lost on quit |
| Session restore (last DB, open schema, etc.) | Low (save to config file on quit) | Small UX win, big perception shift |
| Saved queries / snippets | Low (TOML file, picker UI) | The 5 queries everyone runs daily |
| Multi-tab / multiple queries at once | High (state refactor, concurrent query model) | Biggest lift; single editor limits workflow |

**Gate**: After this phase, opening lazydb feels like resuming where you left off.

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

**Without releases, packaging, and community signals, professionals won't adopt.**

| Item | Effort | Why |
|------|--------|-----|
| `cargo-dist` release pipeline | Low | Prebuilt binaries for macOS/Linux/Windows |
| Homebrew tap / winget / scoop | Low | Devs want `brew install lazydb` |
| crates.io publish | Low | `cargo install lazydb` |
| `--help` + `--version` | Low | CLI basics |
| Semantic versioning + changelog | Low | Required for enterprise adoption |
| Telemetry opt-in (helpful defaults) | Low | The author needs to know what features are used |
| Security audit of dependencies | One-time | `cargo audit`, supply-chain confidence |
| Website / docs site | Low | Single page with keybindings, screenshots, install instructions |

**Gate**: After this phase, `brew install lazydb && lazydb` works on a fresh machine.

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

| Phase | Professional can… |
|-------|-------------------|
| 1 | Connect to a production RDS/CloudSQL instance over SSL through a bastion |
| 2 | Use Oracle or SQL Server at their company |
| 3 | Browse 1M-row tables without freezing |
| 4 | Resume a session, run a saved query, open multiple tabs |
| 5 | Export data as CSV/XLSX/INSERT for a colleague |
| 6 | Roll back a mistaken UPDATE with one keystroke |
| 7 | `brew install lazydb` on a company laptop with security team approval |
| 8 | Prefer lazydb over their previous tool |

**Priority for v1 "professional enough"**: Phases 1 + 7 (connect securely + install easily) give the highest perception value per effort. Phases 2-6 are feature depth that can follow.

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
