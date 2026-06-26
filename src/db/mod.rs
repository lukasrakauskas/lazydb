use std::collections::HashMap;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod mysql;

#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

pub struct ExecutionResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: u64,
    pub elapsed_ms: u128,
}

/// One trait, one impl per backend. New DB = a match arm in `open` + a module.
/// ponytail: only mysql wired; postgres/sqlite later = new arm + impl.
pub trait Database: Send + 'static {
    fn ping(&self) -> Result<()>;
    fn execute_script(&self, sql: &str, readable_binary: bool) -> Result<ExecutionResult>;
    /// Table → its columns, in ordinal order. Used for schema-aware completion.
    /// ponytail: one INFORMATION_SCHEMA query; tables with zero columns won't
    /// appear (rare). upgrade: a separate TABLES query if you need empty tables.
    fn schema(&self) -> Result<HashMap<String, Vec<String>>>;
    fn primary_keys(&self, table: &str) -> Result<Vec<String>>;
    fn boxed_clone(&self) -> Box<dyn Database>;
}

pub fn open(conn: &Connection) -> Result<Box<dyn Database>> {
    match conn.kind.as_str() {
        "mysql" => Ok(mysql::Mysql::open(conn)?.boxed_clone()),
        other => Err(anyhow::anyhow!("unsupported db kind: {other}")),
    }
}
