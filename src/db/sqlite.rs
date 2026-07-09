use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Connection as DbConn, Database, ExecCtx, ExecutionResult, StatementResult};

pub struct Sqlite {
    conn: Connection,
    path: PathBuf,
    timeout: Option<Duration>,
}

impl Sqlite {
    pub fn open(conn: &DbConn, read_timeout: Option<Duration>) -> Result<Self> {
        // ponytail: file-path only — in-memory, WAL, URI params not supported yet.
        // upgrade: parse `?mode=memory` etc. from the connection host field.
        let path: std::path::PathBuf = if conn.host.is_empty() {
            Path::new(&conn.database).to_path_buf()
        } else {
            Path::new(&conn.host).join(&conn.database)
        };
        let c = Connection::open(&path)?;
        if let Some(t) = read_timeout {
            c.busy_timeout(t)?;
        }
        // ponytail: WAL mode for concurrent reads (rusqlite is single-connection
        // single-threaded, but WAL helps when other processes read the DB).
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self {
            conn: c,
            path,
            timeout: read_timeout,
        })
    }
}

impl Database for Sqlite {
    fn kind(&self) -> &str {
        "sqlite"
    }

    fn ping(&self) -> Result<()> {
        self.conn.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }

    fn views(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut views = Vec::new();
        for row in rows {
            views.push(row?);
        }
        Ok(views)
    }

    fn schema(&self) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.name, p.name FROM sqlite_master m, pragma_table_info(m.name) p \
             WHERE m.type = 'table' ORDER BY m.name, p.cid",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (table, column) = row?;
            map.entry(table).or_default().push(column);
        }
        Ok(map)
    }

    fn execute_script(&self, sql: &str, ctx: &ExecCtx) -> Result<ExecutionResult> {
        let conn = &self.conn;
        let mut all_results: Vec<StatementResult> = Vec::new();
        let limit = ctx.limit;

        for part in crate::db::sql::split_statements(sql) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            let mut stmt = match conn.prepare(part) {
                Ok(s) => s,
                Err(_) => {
                    match conn.execute_batch(part) {
                        Ok(()) => {}
                        Err(e) => return Err(anyhow::anyhow!("sqlite: {e}")),
                    }
                    all_results.push(StatementResult {
                        columns: Vec::new(),
                        rows: Vec::new(),
                        rows_affected: conn.changes(),
                        truncated: false,
                    });
                    continue;
                }
            };

            let col_count = stmt.column_count();
            if col_count == 0 {
                let affected = stmt.execute([])?;
                all_results.push(StatementResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    rows_affected: affected as u64,
                    truncated: false,
                });
                continue;
            }

            let stmt_cols: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
                .collect();

            let mut stmt_rows: Vec<Vec<String>> = Vec::new();
            let mut stmt_truncated = false;
            let mut row_iter = stmt.query([])?;
            while let Some(r) = row_iter.next()? {
                if let Some(cap) = limit
                    && stmt_rows.len() >= cap
                {
                    stmt_truncated = true;
                    break;
                }
                let row: Vec<String> = (0..col_count)
                    .map(|i| match r.get_ref(i) {
                        Ok(rusqlite::types::ValueRef::Null) => "NULL".into(),
                        Ok(rusqlite::types::ValueRef::Integer(i)) => i.to_string(),
                        Ok(rusqlite::types::ValueRef::Real(f)) => f.to_string(),
                        Ok(rusqlite::types::ValueRef::Text(t)) => {
                            String::from_utf8_lossy(t).into_owned()
                        }
                        Ok(rusqlite::types::ValueRef::Blob(b)) => {
                            if ctx.readable_binary {
                                crate::db::mysql::bytes_to_string(b, true)
                            } else {
                                String::from_utf8_lossy(b).into_owned()
                            }
                        }
                        Err(_) => "NULL".into(),
                    })
                    .collect();
                stmt_rows.push(row);
            }
            all_results.push(StatementResult {
                columns: stmt_cols,
                rows: stmt_rows,
                rows_affected: 0,
                truncated: stmt_truncated,
            });
            // ponytail: truncated caps row collection for this result set only;
            // subsequent split statements still execute.
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
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY cid")?;
        let rows: Vec<String> = stmt
            .query_map([table], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    fn kill_query(&self, _conn_id: u32) -> Result<()> {
        // ponytail: sqlite is single-connection synchronous; no cancel mechanism.
        // A long query blocks the thread; user waits or kills the process.
        // upgrade: run queries on a separate thread with an interrupt flag,
        // calling `conn.interrupt()` on cancel.
        Err(anyhow::anyhow!(
            "SQLite does not support query cancellation"
        ))
    }

    fn boxed_clone(&self) -> Box<dyn Database> {
        let c = Connection::open(&self.path)
            .expect("failed to clone SQLite connection — check file permissions and disk space");
        if let Some(t) = self.timeout {
            c.busy_timeout(t)
                .expect("failed to set busy_timeout on cloned SQLite connection");
        }
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("failed to apply PRAGMAs on cloned SQLite connection");
        Box::new(Self {
            conn: c,
            path: self.path.clone(),
            timeout: self.timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CancelSlot;

    fn ctx(limit: Option<usize>) -> ExecCtx {
        ExecCtx {
            cancel: CancelSlot::new(),
            readable_binary: false,
            limit,
        }
    }

    #[test]
    fn sqlite_round_trip() {
        let db = make_temp_db();
        db.ping().unwrap();
        db.execute_script(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); \
             INSERT INTO t VALUES (1,'a'),(2,'b');",
            &ctx(None),
        )
        .unwrap();
        let res = db
            .execute_script("SELECT id, name FROM t ORDER BY id;", &ctx(None))
            .unwrap();
        assert_eq!(res.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0], vec!["1".to_string(), "a".to_string()]);
        assert_eq!(res.rows[1], vec!["2".to_string(), "b".to_string()]);
    }

    #[test]
    fn sqlite_schema() {
        let db = make_temp_db();
        db.execute_script(
            "CREATE TABLE t1 (a INT, b TEXT); CREATE TABLE t2 (x INT);",
            &ctx(None),
        )
        .unwrap();
        let s = db.schema().unwrap();
        assert_eq!(
            s.get("t1").map(|v| v.as_slice()),
            Some(&["a".into(), "b".into()][..])
        );
        assert_eq!(s.get("t2").map(|v| v.as_slice()), Some(&["x".into()][..]));
    }

    #[test]
    fn sqlite_primary_keys() {
        let db = make_temp_db();
        db.execute_script(
            "CREATE TABLE pk_t (id INT PRIMARY KEY, name TEXT); \
             CREATE TABLE no_pk (x INT);",
            &ctx(None),
        )
        .unwrap();
        assert_eq!(db.primary_keys("pk_t").unwrap(), vec!["id"]);
        assert!(db.primary_keys("no_pk").unwrap().is_empty());
    }

    #[test]
    fn sqlite_select_limit_truncates() {
        let db = make_temp_db();
        db.execute_script(
            "CREATE TABLE lim_t (id INT); \
             INSERT INTO lim_t VALUES (1),(2),(3),(4),(5);",
            &ctx(None),
        )
        .unwrap();
        let res = db
            .execute_script("SELECT id FROM lim_t ORDER BY id;", &ctx(Some(2)))
            .unwrap();
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0], vec!["1"]);
        assert_eq!(res.rows[1], vec!["2"]);
        assert!(res.truncated);
    }

    #[test]
    fn sqlite_dml_reports_affected_rows() {
        let db = make_temp_db();
        db.execute_script(
            "CREATE TABLE dml (id INT PRIMARY KEY); \
             INSERT INTO dml VALUES (1),(2);",
            &ctx(None),
        )
        .unwrap();
        let ins = db
            .execute_script("INSERT INTO dml VALUES (3),(4),(5);", &ctx(None))
            .unwrap();
        assert_eq!(ins.rows_affected, 3);
        let sel = db
            .execute_script("SELECT id FROM dml ORDER BY id;", &ctx(None))
            .unwrap();
        assert_eq!(sel.rows_affected, 0);
        assert_eq!(sel.rows.len(), 5);
    }

    fn make_temp_db() -> Sqlite {
        Sqlite {
            conn: Connection::open_in_memory().unwrap(),
            path: PathBuf::new(),
            timeout: None,
        }
    }
}
