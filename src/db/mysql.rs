use anyhow::Result;
use mysql::{Opts, Pool, Value, prelude::*};

use super::{Connection, Database, ExecutionResult};

pub struct Mysql {
    pool: Pool,
}

impl Mysql {
    pub fn open(conn: &Connection) -> Result<Self> {
        // ponytail: password used in plaintext; fine for a local dev tool.
        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            pct(&conn.username),
            pct(&conn.password),
            pct(&conn.host),
            conn.port,
            pct(&conn.database),
        );
        let pool = Pool::new(Opts::from_url(&url)?)?;
        Ok(Self { pool })
    }
}

impl Database for Mysql {
    fn ping(&self) -> Result<()> {
        let mut conn = self.pool.get_conn()?;
        let _: Option<i64> = conn.query_first("SELECT 1")?;
        Ok(())
    }

    fn execute_script(&self, sql: &str) -> Result<ExecutionResult> {
        let mut conn = self.pool.get_conn()?;
        let mut columns: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut rows_affected = 0u64;

        // ponytail: naive split on ';' — breaks on ';' inside string literals or
        // comments. Fine for typical scripts; swap in a real tokenizer if needed.
        for part in sql.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let mut result = conn.query_iter(part)?;
            let set_cols: Vec<String> =
                result.columns().as_ref().iter().map(|c| c.name_str().to_string()).collect();

            if set_cols.is_empty() {
                for _ in result.by_ref() {}
            } else {
                if columns.is_empty() {
                    columns = set_cols.clone();
                }
                for row in result.by_ref() {
                    let row = row?;
                    let mut r = Vec::with_capacity(set_cols.len());
                    for i in 0..set_cols.len() {
                        let v: Value = row.as_ref(i).cloned().unwrap_or(Value::NULL);
                        r.push(value_to_string(v));
                    }
                    rows.push(r);
                }
            }
            rows_affected = result.affected_rows();
        }

        Ok(ExecutionResult {
            columns,
            rows,
            rows_affected,
            elapsed_ms: 0,
        })
    }

    fn boxed_clone(&self) -> Box<dyn Database> {
        Box::new(Self { pool: self.pool.clone() })
    }
}

fn value_to_string(v: Value) -> String {
    use Value::*;
    match v {
        NULL => "NULL".into(),
        Int(i) => i.to_string(),
        UInt(u) => u.to_string(),
        Float(f) => f.to_string(),
        Double(d) => d.to_string(),
        Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
        Date(y, m, d, h, mi, s, us) => {
            format!("{y}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
        }
        Time(neg, d, h, mi, s, us) => {
            format!("{}{d}d {h:02}:{mi:02}:{s:02}.{us:06}", if neg { "-" } else { "" })
        }
    }
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
