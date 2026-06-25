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

    fn execute_script(&self, sql: &str, readable_binary: bool) -> Result<ExecutionResult> {
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
                        r.push(value_to_string(v, readable_binary));
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
            format!("{}{d}d {h:02}:{mi:02}:{s:02}.{us:06}", if neg { "-" } else { "" })
        }
    }
}

fn bytes_to_string(b: &[u8], readable_binary: bool) -> String {
    if readable_binary {
        // ponytail: valid-UTF8 heuristic — no column-type plumbing. Binary
        // (invalid UTF-8) → hex, capped at 64 bytes so big BLOBs don't build a
        // huge string; the UI truncates display width anyway.
        match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => {
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
        assert_eq!(bytes_to_string(&[0xff, 0x00, 0xde, 0xad], true), "0xff00dead");
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
