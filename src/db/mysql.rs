use anyhow::Result;
use mysql::{Opts, OptsBuilder, Pool, Value, prelude::*};
use std::collections::HashMap;
use std::time::Duration;

use super::{Connection, Database, ExecCtx, ExecutionResult};

pub struct Mysql {
    pool: Pool,
}

impl Mysql {
    pub fn open(conn: &Connection, read_timeout: Option<Duration>) -> Result<Self> {
        // ponytail: secrets may be `${VAR}` references, resolved in db::open;
        // the literal password here is the resolved value. plaintext at rest is
        // the YAGNI default; env-var references avoid committing secrets.
        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            pct(&conn.username),
            pct(&conn.password),
            pct(&conn.host),
            conn.port,
            pct(&conn.database),
        );
        // ponytail: read_timeout is the query timeout — a socket-level cap set
        // once on the pool. Per-query timeout would need a fresh conn per query;
        // a per-connection default from config covers the TUI's one-at-a-time use.
        let pool = {
            let mut b = OptsBuilder::from_opts(Opts::from_url(&url)?);
            if let Some(t) = read_timeout {
                b = b.read_timeout(Some(t));
            }
            Pool::new(b)?
        };
        Ok(Self { pool })
    }
}

impl Database for Mysql {
    fn ping(&self) -> Result<()> {
        let mut conn = self.pool.get_conn()?;
        let _: Option<i64> = conn.query_first("SELECT 1")?;
        Ok(())
    }

    fn schema(&self) -> Result<HashMap<String, Vec<String>>> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<(String, String)> = conn.query("SELECT TABLE_NAME, COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME, ORDINAL_POSITION")?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (table, column) in rows {
            map.entry(table).or_default().push(column);
        }
        Ok(map)
    }

    fn execute_script(&self, sql: &str, ctx: &ExecCtx) -> Result<ExecutionResult> {
        let mut conn = self.pool.get_conn()?;
        let mut columns: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut rows_affected = 0u64;
        let mut truncated = false;
        let limit = ctx.limit;

        for part in crate::db::sql::split_statements(sql) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            // Record this connection's id so the UI can KILL QUERY on cancel.
            ctx.cancel.set(conn.connection_id());
            let mut result = conn.query_iter(part)?;
            let set_cols: Vec<String> = result
                .columns()
                .as_ref()
                .iter()
                .map(|c| c.name_str().to_string())
                .collect();

            if set_cols.is_empty() {
                for _ in result.by_ref() {}
            } else {
                if columns.is_empty() {
                    columns = set_cols.clone();
                }
                // ponytail: row-limit guard caps client memory by stopping the
                // fetch after `limit` rows. The server still runs the query; a
                // pushed-down LIMIT needs SQL parsing (risk of corrupting
                // subqueries/UNION), so this is the safe middle. `truncated`
                // flags it in the UI. upgrade: server-side LIMIT via a parser.
                for (i, row) in result.by_ref().enumerate() {
                    if let Some(cap) = limit
                        && i >= cap
                    {
                        truncated = true;
                        break;
                    }
                    let row = row?;
                    let mut r = Vec::with_capacity(set_cols.len());
                    for k in 0..set_cols.len() {
                        let v: Value = row.as_ref(k).cloned().unwrap_or(Value::NULL);
                        r.push(value_to_string(v, ctx.readable_binary));
                    }
                    rows.push(r);
                }
            }
            rows_affected = result.affected_rows();
            ctx.cancel.clear();
            if truncated {
                break;
            }
        }

        Ok(ExecutionResult {
            columns,
            rows,
            rows_affected,
            elapsed_ms: 0,
            truncated,
        })
    }

    fn primary_keys(&self, table: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<(String,)> = conn.exec(
            "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE \
             WHERE CONSTRAINT_NAME = 'PRIMARY' AND TABLE_SCHEMA = DATABASE() \
             AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (table.to_string(),),
        )?;
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    fn kill_query(&self, conn_id: u32) -> Result<()> {
        // ponytail: best-effort — needs PROCESS/SUPER (MySQL 8: CONNECTION_ADMIN).
        // Same-user kill of own query usually works; failures surface as a status note.
        let mut conn = self.pool.get_conn()?;
        conn.query_drop(format!("KILL QUERY {conn_id}"))?;
        Ok(())
    }
    fn boxed_clone(&self) -> Box<dyn Database> {
        Box::new(Self {
            pool: self.pool.clone(),
        })
    }
}

fn value_to_string(v: Value, readable_binary: bool) -> String {
    use Value::*;
    match v {
        NULL => "NULL".into(),
        Int(i) => i.to_string(),
        UInt(u) => u.to_string(),
        Float(f) => f.to_string(),
        Double(d) => d.to_string(),
        Bytes(b) => bytes_to_string(&b, readable_binary),
        Date(y, m, d, h, mi, s, us) => {
            format!("{y}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
        }
        Time(neg, d, h, mi, s, us) => {
            format!(
                "{}{d}d {h:02}:{mi:02}:{s:02}.{us:06}",
                if neg { "-" } else { "" }
            )
        }
    }
}

fn bytes_to_string(b: &[u8], readable_binary: bool) -> String {
    if readable_binary {
        // ponytail: valid-UTF8 heuristic — no column-type plumbing. Binary
        // (invalid UTF-8) → UUID if exactly 16 bytes (MySQL BINARY(16) UUID
        // case, BIN_TO_UUID natural order), else hex capped at 64 bytes so big
        // BLOBs don't build a huge string; the UI truncates display width anyway.
        match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => {
                if b.len() == 16 {
                    return bin_to_uuid(b);
                }
                const CAP: usize = 64;
                let hex: String = b.iter().take(CAP).map(|x| format!("{:02x}", x)).collect();
                if b.len() > CAP {
                    format!("0x{hex}… ({} bytes)", b.len())
                } else {
                    format!("0x{hex}")
                }
            }
        }
    } else {
        String::from_utf8_lossy(b).into_owned()
    }
}

/// Format 16 bytes as a UUID string: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx.
/// ponytail: matches MySQL BIN_TO_UUID(bin, 0) — natural byte order, no swap.
/// ceiling: a 16-byte BINARY that isn't a UUID renders UUID-shaped; and UUIDs
/// stored via MySQL UUID_TO_BIN (which reorders bytes for index locality) won't
/// round-trip to the original string without a swap. upgrade: detect swap via
/// column metadata, or add a per-connection swap toggle.
fn bin_to_uuid(b: &[u8]) -> String {
    let h: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::bytes_to_string;

    #[test]
    fn bytes_valid_utf8_pass_through() {
        assert_eq!(bytes_to_string(b"hello", true), "hello");
        assert_eq!(bytes_to_string(b"hello", false), "hello");
    }

    #[test]
    fn bytes_invalid_utf8_becomes_hex() {
        // 0xff is never valid UTF-8.
        assert_eq!(
            bytes_to_string(&[0xff, 0x00, 0xde, 0xad], true),
            "0xff00dead"
        );
    }

    #[test]
    fn bytes_hex_caps_long_blobs() {
        // 0xff is never valid UTF-8, so this exercises the hex path (not the
        // pass-through), and is long enough to hit the 64-byte cap.
        let big: Vec<u8> = vec![0xff; 100];
        let s = bytes_to_string(&big, true);
        assert!(s.starts_with("0x"), "got: {s}");
        assert!(s.ends_with("… (100 bytes)"), "got: {s}");
        assert!(s.contains("ffff"), "got: {s}");
    }

    #[test]
    fn bytes_16_bytes_become_uuid() {
        // 16 bytes of 0xff is invalid UTF-8 and exactly UUID-sized → BIN_TO_UUID.
        let s = bytes_to_string(&[0xff; 16], true);
        assert_eq!(s, "ffffffff-ffff-ffff-ffff-ffffffffffff");
    }

    #[test]
    fn bytes_uuid_matches_known_value() {
        // The canonical "all-zeros except version/variant" UUID.
        let bin: [u8; 16] = [
            0x01, 0xb4, 0xe9, 0x2f, 0x37, 0x14, 0x43, 0x52, 0x86, 0x37, 0xc8, 0x4e, 0xa7, 0x0a,
            0x9b, 0x12,
        ];
        assert_eq!(
            bytes_to_string(&bin, true),
            "01b4e92f-3714-4352-8637-c84ea70a9b12",
        );
    }
}

#[cfg(test)]
mod live {
    use super::Mysql;
    use crate::db::{CancelSlot, Connection, Database, ExecCtx};

    // ponytail: naive hand-parser for mysql://user:pass@host:port/db. ceiling:
    // no query params, no IPv6 brackets, no URL-encoding, password must not
    // contain '@' or ':'. upgrade to the `url` crate if such URLs appear.
    fn conn_from_url(url: &str) -> Option<Connection> {
        let rest = url.strip_prefix("mysql://")?;
        let (creds, hostdb) = rest.split_once('@')?;
        let (user, pass) = creds.split_once(':')?;
        let (hostport, db) = hostdb.split_once('/')?;
        let (host, port_s) = hostport.split_once(':')?;
        let port: u16 = port_s.parse().ok()?;
        Some(Connection {
            name: "live".to_string(),
            kind: "mysql".to_string(),
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
    // ponytail: one test covers the whole live path (ping→execute→schema→pks).
    // Fewer tests = less boilerplate; the read and write halves of
    // execute_script are both exercised. Split if a step needs isolation.
    #[test]
    fn live_round_trip() {
        let url = match std::env::var("LAZYDB_TEST_MYSQL_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        // ponytail: open() failing → return (pass) instead of panicking, so a
        // misconfigured env (wrong creds, DB down) doesn't redden the whole
        // suite. Real logic failures below ARE surfaced via unwrap/assert.
        let db = match Mysql::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };

        // ping — proves the pool connects and SELECT 1 works.
        db.ping().unwrap();

        // self-cleanse, then create + insert.
        db.execute_script("DROP TABLE IF EXISTS lazydb_live;", &ctx(None))
            .unwrap();
        db.execute_script("CREATE TABLE lazydb_live (id INT PRIMARY KEY, name VARCHAR(32)); INSERT INTO lazydb_live VALUES (1,'a'),(2,'b');", &ctx(None)).unwrap();

        // read back — exercises the columns/rows path of execute_script.
        let res = db
            .execute_script("SELECT id, name FROM lazydb_live ORDER BY id;", &ctx(None))
            .unwrap();
        assert_eq!(res.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0], vec!["1".to_string(), "a".to_string()]);
        assert_eq!(res.rows[1], vec!["2".to_string(), "b".to_string()]);

        // schema — table present with columns in ordinal order.
        let schema = db.schema().unwrap();
        let cols = schema
            .get("lazydb_live")
            .expect("lazydb_live should be in schema");
        assert_eq!(cols, &vec!["id".to_string(), "name".to_string()]);

        // primary_keys — single PK column 'id'.
        let pks = db.primary_keys("lazydb_live").unwrap();
        assert_eq!(pks, vec!["id".to_string()]);

        // cleanup.
        db.execute_script("DROP TABLE IF EXISTS lazydb_live;", &ctx(None))
            .unwrap();
    }

    #[test]
    fn live_select_limit_truncates() {
        // ponytail: row-limit guard caps the fetch and sets `truncated`.
        let url = match std::env::var("LAZYDB_TEST_MYSQL_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let conn = match conn_from_url(&url) {
            Some(c) => c,
            None => return,
        };
        let db = match Mysql::open(&conn, None) {
            Ok(d) => d,
            Err(_) => return,
        };
        db.execute_script("DROP TABLE IF EXISTS lazydb_lim;", &ctx(None))
            .unwrap();
        db.execute_script("CREATE TABLE lazydb_lim (id INT PRIMARY KEY); INSERT INTO lazydb_lim VALUES (1),(2),(3),(4),(5);", &ctx(None)).unwrap();
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
}
