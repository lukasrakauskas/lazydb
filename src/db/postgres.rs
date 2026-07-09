use anyhow::Result;
use postgres::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Connection, Database, ExecCtx, ExecutionResult, StatementResult};

/// ponytail: one shared `Client` behind a `Mutex` (the TUI runs one query at a
/// time — see `CancelSlot`), cloned via `Arc` so `boxed_clone` is cheap and
/// reconnect-free. mysql uses a `Pool`; postgres has no built-in pool and a
/// per-job reconnect would lose session state (temp tables, SET options), so a
/// single shared connection is the smaller correct choice. upgrade: `r2d2` pool
/// if concurrent queries ever run.
pub struct Postgres {
    client: Arc<Mutex<Client>>,
    cfg: PgCfg,
    /// Backend pid of the shared connection, fetched once at open. The UI sets
    /// this on the `CancelSlot` so `pg_cancel_backend` can target the running
    /// query. Constant for the life of the connection.
    pid: u32,
}

/// Connection pieces kept for `kill_query`'s side connection (can't reuse the
/// shared client — it's blocked running the query being cancelled) and for
/// `boxed_clone`'s reconnect-free Arc clone.
struct PgCfg {
    host: String,
    port: u16,
    user: String,
    password: String,
    dbname: String,
    statement_timeout_ms: Option<u64>,
}

impl PgCfg {
    fn from_conn(conn: &Connection, read_timeout: Option<Duration>) -> Self {
        Self {
            host: conn.host.clone(),
            port: conn.port,
            user: conn.username.clone(),
            password: conn.password.clone(),
            dbname: conn.database.clone(),
            // ponytail: query timeout maps to server-side `statement_timeout`
            // (the idiomatic postgres knob), not a socket read_timeout like
            // mysql — the sync `postgres` client exposes no per-query socket
            // timeout. None/0 = no limit (matches mysql's None/0 semantics).
            statement_timeout_ms: read_timeout.map(|d| d.as_millis() as u64),
        }
    }

    fn connect(&self) -> Result<Client> {
        let mut c = postgres::Config::new();
        if !self.host.is_empty() {
            c.host(&self.host);
        }
        c.port(self.port).user(&self.user).password(&self.password);
        if !self.dbname.is_empty() {
            c.dbname(&self.dbname);
        }
        // ponytail: 10s connect cap so a firewalled host doesn't hang the
        // worker forever (a hung connect can't be cancelled — kill_query opens
        // a side conn to the same dead host). upgrade: make configurable.
        c.connect_timeout(Duration::from_secs(10));
        c.connect(postgres::NoTls).map_err(pg_err)
    }
}

impl Postgres {
    pub fn open(conn: &Connection, read_timeout: Option<Duration>) -> Result<Self> {
        let cfg = PgCfg::from_conn(conn, read_timeout);
        let mut client = cfg.connect()?;
        if let Some(ms) = cfg.statement_timeout_ms {
            // `SET` is a utility command — no $1 params; the u64 is safe to
            // format directly (no injection surface).
            client
                .simple_query(&format!("SET statement_timeout = {ms}"))
                .map_err(pg_err)?;
        }
        let pid = pid_of(
            &client
                .simple_query("SELECT pg_backend_pid()")
                .map_err(pg_err)?,
        )
        .ok_or_else(|| anyhow::anyhow!("pg_backend_pid returned no row"))?;
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            cfg,
            pid,
        })
    }
}

impl Database for Postgres {
    fn kind(&self) -> &str {
        "postgres"
    }

    fn ping(&self) -> Result<()> {
        let _ = self
            .client
            .lock()
            .unwrap()
            .simple_query("SELECT 1")
            .map_err(pg_err)?;
        Ok(())
    }

    fn schema(&self) -> Result<HashMap<String, Vec<String>>> {
        // ponytail: current_schema() only — tables in other search_path schemas
        // won't appear. Covers the common case (tables in `public`). upgrade:
        // scan all current_schemas(false) minus the system ones.
        const SQL: &str = "SELECT table_name, column_name \
             FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             ORDER BY table_name, ordinal_position";
        let msgs = self
            .client
            .lock()
            .unwrap()
            .simple_query(SQL)
            .map_err(pg_err)?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for m in &msgs {
            if let postgres::SimpleQueryMessage::Row(r) = m
                && let (Some(t), Some(c)) = (r.get(0), r.get(1))
            {
                map.entry(t.to_string()).or_default().push(c.to_string());
            }
        }
        Ok(map)
    }

    fn execute_script(&self, sql: &str, ctx: &ExecCtx) -> Result<ExecutionResult> {
        let mut client = self.client.lock().unwrap();
        let mut all_results: Vec<StatementResult> = Vec::new();
        let limit = ctx.limit;

        for part in crate::db::sql::split_statements(sql) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            ctx.cancel.set(self.pid);
            let msgs = client.simple_query(part).map_err(pg_err)?;
            ctx.cancel.clear();

            let mut stmt_columns: Vec<String> = Vec::new();
            let mut stmt_rows: Vec<Vec<String>> = Vec::new();
            let mut stmt_affected = 0u64;
            let mut stmt_truncated = false;
            let mut had_result_set = false;

            for m in &msgs {
                match m {
                    postgres::SimpleQueryMessage::Row(r) => {
                        had_result_set = true;
                        if stmt_columns.is_empty() {
                            stmt_columns =
                                r.columns().iter().map(|c| c.name().to_string()).collect();
                        }
                        if let Some(cap) = limit
                            && stmt_rows.len() >= cap
                        {
                            stmt_truncated = true;
                            continue;
                        }
                        let row: Vec<String> = (0..r.columns().len())
                            .map(|i| r.get(i).map(String::from).unwrap_or_else(|| "NULL".into()))
                            .collect();
                        stmt_rows.push(row);
                    }
                    postgres::SimpleQueryMessage::CommandComplete(n) if !had_result_set => {
                        stmt_affected = *n;
                    }
                    _ => {}
                }
            }
            all_results.push(StatementResult {
                columns: stmt_columns,
                rows: stmt_rows,
                rows_affected: stmt_affected,
                truncated: stmt_truncated,
            });
            if stmt_truncated {
                break;
            }
        }

        let last = all_results.last().cloned().unwrap_or(StatementResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
            truncated: false,
        });
        Ok(ExecutionResult {
            columns: last.columns,
            rows: last.rows,
            rows_affected: last.rows_affected,
            elapsed_ms: 0,
            truncated: last.truncated,
            all_results,
        })
    }

    fn primary_keys(&self, table: &str) -> Result<Vec<String>> {
        // ponytail: parameterized on table_name (no regclass cast) so there's
        // no injection and no search_path resolution edge cases for mixed-case
        // / reserved-word names. Filters current_schema(); PK column order via
        // ordinal_position. upgrade: pass an explicit schema if multi-schema.
        const SQL: &str = "SELECT k.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage k \
               ON k.constraint_name = tc.constraint_name \
              AND k.table_schema = tc.table_schema \
             WHERE tc.constraint_type = 'PRIMARY KEY' \
               AND tc.table_schema = current_schema() \
               AND k.table_name = $1 \
             ORDER BY k.ordinal_position";
        let table = table.to_string();
        let rows = self
            .client
            .lock()
            .unwrap()
            .query(SQL, &[&table])
            .map_err(pg_err)?;
        Ok(rows.into_iter().map(|r| r.get::<_, String>(0)).collect())
    }

    fn kill_query(&self, conn_id: u32) -> Result<()> {
        // ponytail: best-effort — opens a SIDE connection (the shared client is
        // blocked running the query being cancelled, its mutex is held by the
        // worker). pg_cancel_backend cancels the current query, keeping the
        // session (mirrors MySQL KILL QUERY, not KILL CONNECTION). Needs no
        // special privilege for same-user cancel. Failures surface as a status
        // note.
        let mut conn = self.cfg.connect()?;
        let pid = conn_id as i32;
        let _: bool = conn
            .query_one("SELECT pg_cancel_backend($1)", &[&pid])
            .map_err(pg_err)?
            .get(0);
        Ok(())
    }

    fn boxed_clone(&self) -> Box<dyn Database> {
        // Reuses the same connection (Arc clone) — no reconnect, no new backend
        // pid. Jobs share the one connection; safe because the app runs one
        // query at a time.
        Box::new(Self {
            client: Arc::clone(&self.client),
            cfg: PgCfg {
                host: self.cfg.host.clone(),
                port: self.cfg.port,
                user: self.cfg.user.clone(),
                password: self.cfg.password.clone(),
                dbname: self.cfg.dbname.clone(),
                statement_timeout_ms: self.cfg.statement_timeout_ms,
            },
            pid: self.pid,
        })
    }
}

/// Surface a postgres server error's message — `postgres::Error`'s `Display` is
/// just "db error" with no detail, which would show uselessly in the TUI. Pull
/// the `DbError` message so a timeout, syntax error, etc. reads clearly.
/// ponytail: message only; detail()/hint()/code() omitted until needed.
fn pg_err(e: postgres::Error) -> anyhow::Error {
    match e.as_db_error() {
        Some(db) => anyhow::anyhow!("db error: {}", db.message()),
        None => anyhow::anyhow!("{e}"),
    }
}

/// Extract the backend pid from a `SELECT pg_backend_pid()` simple-query result.
fn pid_of(msgs: &[postgres::SimpleQueryMessage]) -> Option<u32> {
    for m in msgs {
        if let postgres::SimpleQueryMessage::Row(r) = m
            && let Some(s) = r.get(0)
        {
            return s.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod live {
    use super::Postgres;
    use crate::db::{CancelSlot, Connection, Database, ExecCtx};
    use std::time::Duration;

    // ponytail: naive hand-parser for postgres://user:pass@host:port/db. ceiling:
    // no query params, no IPv6 brackets, no URL-encoding, password must not
    // contain '@' or ':'. upgrade to the `url` crate if such URLs appear.
    fn conn_from_url(url: &str) -> Option<Connection> {
        let rest = url.strip_prefix("postgres://")?;
        let (creds, hostdb) = rest.split_once('@')?;
        let (user, pass) = creds.split_once(':')?;
        let (hostport, db) = hostdb.split_once('/')?;
        let (host, port_s) = hostport.split_once(':')?;
        let port: u16 = port_s.parse().ok()?;
        Some(Connection {
            name: "live".to_string(),
            kind: "postgres".to_string(),
            host: host.to_string(),
            port,
            username: user.to_string(),
            password: pass.to_string(),
            database: db.to_string(),
        })
    }

    fn ctx(limit: Option<usize>) -> ExecCtx {
        ExecCtx {
            cancel: CancelSlot::new(),
            readable_binary: false,
            limit,
        }
    }

    // ponytail: one test covers the whole live path (ping→execute→schema→pks),
    // mirroring the mysql live suite. open() failing → return (pass) so a
    // misconfigured env doesn't redden the suite; real logic failures unwrap.
    #[test]
    fn live_round_trip() {
        let url = match std::env::var("LAZYDB_TEST_POSTGRES_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Postgres::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };

        db.ping().unwrap();

        db.execute_script("DROP TABLE IF EXISTS lazydb_live;", &ctx(None))
            .unwrap();
        db.execute_script(
            "CREATE TABLE lazydb_live (id INT PRIMARY KEY, name TEXT); \
             INSERT INTO lazydb_live VALUES (1,'a'),(2,'b');",
            &ctx(None),
        )
        .unwrap();

        let res = db
            .execute_script("SELECT id, name FROM lazydb_live ORDER BY id;", &ctx(None))
            .unwrap();
        assert_eq!(res.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0], vec!["1".to_string(), "a".to_string()]);
        assert_eq!(res.rows[1], vec!["2".to_string(), "b".to_string()]);

        let schema = db.schema().unwrap();
        let cols = schema
            .get("lazydb_live")
            .expect("lazydb_live should be in schema");
        assert_eq!(cols, &vec!["id".to_string(), "name".to_string()]);

        let pks = db.primary_keys("lazydb_live").unwrap();
        assert_eq!(pks, vec!["id".to_string()]);

        db.execute_script("DROP TABLE IF EXISTS lazydb_live;", &ctx(None))
            .unwrap();
    }

    #[test]
    fn live_select_limit_truncates() {
        let url = match std::env::var("LAZYDB_TEST_POSTGRES_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Postgres::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };
        db.execute_script("DROP TABLE IF EXISTS lazydb_lim;", &ctx(None))
            .unwrap();
        db.execute_script(
            "CREATE TABLE lazydb_lim (id INT PRIMARY KEY); \
             INSERT INTO lazydb_lim VALUES (1),(2),(3),(4),(5);",
            &ctx(None),
        )
        .unwrap();
        let res = db
            .execute_script("SELECT id FROM lazydb_lim ORDER BY id;", &ctx(Some(2)))
            .unwrap();
        assert_eq!(res.rows.len(), 2, "should cap at 2 rows");
        assert_eq!(res.rows[0], vec!["1".to_string()]);
        assert_eq!(res.rows[1], vec!["2".to_string()]);
        assert!(res.truncated, "truncated flag must be set");
        db.execute_script("DROP TABLE IF EXISTS lazydb_lim;", &ctx(None))
            .unwrap();
    }

    #[test]
    fn live_dml_rows_affected() {
        // ponytail: DML reports rows_affected; a SELECT must NOT (parity with
        // mysql — count lives in rows.len()).
        let url = match std::env::var("LAZYDB_TEST_POSTGRES_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Postgres::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };
        db.execute_script("DROP TABLE IF EXISTS lazydb_dml;", &ctx(None))
            .unwrap();
        db.execute_script(
            "CREATE TABLE lazydb_dml (id INT PRIMARY KEY); \
             INSERT INTO lazydb_dml VALUES (1),(2);",
            &ctx(None),
        )
        .unwrap();
        let ins = db
            .execute_script("INSERT INTO lazydb_dml VALUES (3),(4),(5);", &ctx(None))
            .unwrap();
        assert_eq!(ins.rows_affected, 3);
        assert!(ins.columns.is_empty(), "INSERT has no result columns");
        let sel = db
            .execute_script("SELECT id FROM lazydb_dml ORDER BY id;", &ctx(None))
            .unwrap();
        assert_eq!(sel.rows_affected, 0, "SELECT must report 0 affected");
        assert_eq!(sel.rows.len(), 5);
        db.execute_script("DROP TABLE IF EXISTS lazydb_dml;", &ctx(None))
            .unwrap();
    }

    #[test]
    fn live_statement_timeout_aborts_long_query() {
        // ponytail: query_timeout maps to server-side statement_timeout (not a
        // socket read_timeout like mysql). 500ms cap must abort pg_sleep(3).
        let url = match std::env::var("LAZYDB_TEST_POSTGRES_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Postgres::open(&conn, Some(Duration::from_millis(500))) {
            Ok(d) => d,
            Err(_) => return,
        };
        let res = db.execute_script("SELECT pg_sleep(3)", &ctx(None));
        let err = match res {
            Ok(_) => panic!("statement_timeout should have aborted pg_sleep"),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains("timeout"),
            "expected a timeout error, got: {err}"
        );
    }

    #[test]
    fn live_cancel_aborts_running_query() {
        // ponytail: cancel opens a SIDE connection (the shared client is
        // blocked running the sleep, its mutex held by the worker) and calls
        // pg_cancel_backend(pid). Validates the no-deadlock side-conn design.
        let url = match std::env::var("LAZYDB_TEST_POSTGRES_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Postgres::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };
        let cancel = CancelSlot::new();
        let db_bg = db.boxed_clone();
        let cancel_bg = cancel.clone();
        let h = std::thread::spawn(move || {
            let ctx = ExecCtx {
                cancel: cancel_bg,
                readable_binary: false,
                limit: None,
            };
            db_bg.execute_script("SELECT pg_sleep(5)", &ctx)
        });
        // Wait for the worker to register its pid on the cancel slot.
        let mut waited = 0u64;
        while cancel.conn_id() == 0 && waited < 3000 {
            std::thread::sleep(Duration::from_millis(10));
            waited += 10;
        }
        let pid = cancel.conn_id();
        assert!(
            pid != 0,
            "pg_sleep should register its pid on the cancel slot"
        );
        db.kill_query(pid).unwrap();
        let res = h.join().unwrap();
        assert!(res.is_err(), "pg_sleep should have been cancelled");
        assert!(
            match res {
                Err(e) => e.to_string().contains("cancel"),
                _ => false,
            },
            "expected a cancel error"
        );
    }
}
