use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[cfg(feature = "mssql")]
pub mod mssql;
pub mod mysql;
pub mod postgres;
pub mod sql;
pub mod sqlite;
pub mod ssh;
#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    /// Require TLS for this connection. If the server doesn't support it,
    /// the connection will fail. PostgreSQL requires the `ssl` feature.
    /// MySQL uses the built-in TLS support in the `minimal-rust` feature.
    #[serde(default)]
    pub ssl: bool,
    /// Use OS keychain for password storage instead of plaintext in config.
    /// Requires the `keychain` feature. When true, the password is stored in
    /// the OS keychain (service="lazydb", account=connection name) and the
    /// password field in the config file is empty.
    #[serde(default)]
    pub use_keychain: bool,
    /// SSH tunnel configuration. When ssh_enabled is true, lazydb shells out to
    /// the system `ssh` command to create a local forward before connecting.
    #[serde(default)]
    pub ssh_enabled: bool,
    #[serde(default)]
    pub ssh_host: String,
    /// SSH server port (default 22).
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    #[serde(default)]
    pub ssh_user: String,
    /// Path to SSH identity file (optional, defaults to ~/.ssh/id_*).
    #[serde(default)]
    pub ssh_keyfile: String,
    /// Per-connection query timeout in seconds. Overrides the global
    /// `query_timeout_secs` from config. 0 or None = no timeout (wait forever).
    #[serde(default)]
    pub query_timeout_secs: Option<u64>,
}

fn default_ssh_port() -> u16 {
    22
}

pub struct ExecutionResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: u64,
    pub elapsed_ms: u128,
    pub truncated: bool,
    // ponytail: multi-statement results — keep the last set in columns/rows
    // for backward compat, and provide the full list for tabbed display.
    // Only populated when a multi-statement script returns >1 result set.
    pub all_results: Vec<StatementResult>,
}

#[derive(Clone)]
pub struct StatementResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: u64,
    pub truncated: bool,
}

/// Shared, lock-free slot holding the connection id of the currently running
/// query (0 = idle). The worker sets it before executing and clears it after;
/// the UI reads it to issue `KILL QUERY` on cancel.
/// ponytail: Arc<AtomicU32> — one running query per app, no contention.
/// upgrade: per-query handles if we ever run several in parallel.
#[derive(Clone)]
pub struct CancelSlot(Arc<AtomicU32>);

impl CancelSlot {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU32::new(0)))
    }
    pub fn set(&self, id: u32) {
        self.0.store(id, Ordering::Relaxed);
    }
    pub fn clear(&self) {
        self.0.store(0, Ordering::Relaxed);
    }
    pub fn conn_id(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for CancelSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-execution context: the cancel slot, binary-rendering flag, and an
/// optional row cap (the row-limit guard). One struct keeps the trait method
/// to a single extra parameter.
pub struct ExecCtx {
    pub cancel: CancelSlot,
    pub readable_binary: bool,
    pub limit: Option<usize>,
}

/// One trait, one impl per backend. New DB = a match arm in `open` + a module.
/// ponytail: mysql + postgres wired; sqlite later = new arm + impl.
pub trait Database: Send + 'static {
    /// Backend identifier ("mysql" / "postgres" / …). Used to pick
    /// backend-specific SQL (schema-detail queries, identifier quoting) at the
    /// call site without a parallel App field — the backend knows its own kind.
    fn kind(&self) -> &str;
    fn ping(&self) -> Result<()>;
    fn execute_script(&self, sql: &str, ctx: &ExecCtx) -> Result<ExecutionResult>;
    /// Table → its columns, in ordinal order. Used for schema-aware completion.
    /// ponytail: one INFORMATION_SCHEMA query; tables with zero columns won't
    /// appear (rare). upgrade: a separate TABLES query if you need empty tables.
    fn schema(&self) -> Result<HashMap<String, Vec<String>>>;
    fn views(&self) -> Result<Vec<String>>;
    fn primary_keys(&self, table: &str) -> Result<Vec<String>>;
    fn kill_query(&self, conn_id: u32) -> Result<()>;
    fn boxed_clone(&self) -> Box<dyn Database>;
}

pub fn open(conn: &Connection, read_timeout: Option<Duration>) -> Result<Box<dyn Database>> {
    let conn = resolve_connection_env(conn);
    match conn.kind.as_str() {
        "mysql" => Ok(mysql::Mysql::open(&conn, read_timeout)?.boxed_clone()),
        "postgres" => Ok(postgres::Postgres::open(&conn, read_timeout)?.boxed_clone()),
        "sqlite" => Ok(sqlite::Sqlite::open(&conn, read_timeout)?.boxed_clone()),
        #[cfg(feature = "mssql")]
        "mssql" => Ok(mssql::Mssql::open(&conn, read_timeout)?.boxed_clone()),
        other => Err(anyhow::anyhow!("unsupported db kind: {other}")),
    }
}

/// Resolve `$VAR` / `${VAR}` references in a connection string field against the
/// process environment. Unset variables are left literal so a missing secret is
/// visible in the connection error instead of silently empty.
/// ponytail: hand-rolled, no shellexpand dep. ceiling: no `:-` defaults, no nested;
/// upgrade to `shellexpand` if those are needed.
pub fn resolve_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            let mut closed = false;
            for nc in chars.by_ref() {
                if nc == '}' {
                    closed = true;
                    break;
                }
                name.push(nc);
            }
            if closed {
                match std::env::var(&name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('$');
                        out.push('{');
                        out.push_str(&name);
                        out.push('}');
                    }
                }
            } else {
                out.push('$');
                out.push('{');
                out.push_str(&name);
            }
            continue;
        }
        let mut name = String::new();
        while let Some(&nc) = chars.peek() {
            if nc.is_ascii_alphanumeric() || nc == '_' {
                name.push(nc);
                chars.next();
            } else {
                break;
            }
        }
        if name.is_empty() {
            out.push('$');
        } else {
            match std::env::var(&name) {
                Ok(v) => out.push_str(&v),
                Err(_) => {
                    out.push('$');
                    out.push_str(&name);
                }
            }
        }
    }
    out
}

fn resolve_connection_env(c: &Connection) -> Connection {
    Connection {
        name: c.name.clone(),
        kind: c.kind.clone(),
        host: resolve_env(&c.host),
        port: c.port,
        username: resolve_env(&c.username),
        password: resolve_password(c),
        database: resolve_env(&c.database),
        ssl: c.ssl,
        use_keychain: c.use_keychain,
        ssh_enabled: c.ssh_enabled,
        ssh_host: resolve_env(&c.ssh_host),
        ssh_port: c.ssh_port,
        ssh_user: resolve_env(&c.ssh_user),
        ssh_keyfile: resolve_env(&c.ssh_keyfile),
        query_timeout_secs: c.query_timeout_secs,
    }
}

/// Resolve the password for a connection. Priority:
/// 1. Environment variable reference (`$VAR` / `${VAR}`)
/// 2. OS keychain (if `use_keychain` is true and `keychain` feature is enabled)
/// 3. Literal value from config
pub fn resolve_password(conn: &Connection) -> String {
    let from_env = resolve_env(&conn.password);
    if !from_env.is_empty() {
        return from_env;
    }
    if conn.use_keychain {
        #[cfg(feature = "keychain")]
        if let Some(pw) = keychain_get(&conn.name) {
            return pw;
        }
        #[cfg(not(feature = "keychain"))]
        crate::log::warn("keychain_disabled", &[("conn", &conn.name)]);
    }
    from_env
}

/// Store a password in the OS keychain (service="lazydb", account=name).
#[cfg(feature = "keychain")]
pub fn keychain_store(name: &str, password: &str) -> Result<()> {
    let entry = keyring::Entry::new("lazydb", name)?;
    entry.set_password(password)?;
    Ok(())
}

/// Read a password from the OS keychain (service="lazydb", account=name).
#[cfg(feature = "keychain")]
fn keychain_get(name: &str) -> Option<String> {
    keyring::Entry::new("lazydb", name)
        .ok()
        .and_then(|e| e.get_password().ok())
}

/// Delete a password from the OS keychain.
#[cfg(feature = "keychain")]
#[allow(dead_code)]
pub fn keychain_delete(name: &str) -> Result<()> {
    let entry = keyring::Entry::new("lazydb", name)?;
    entry.delete_credential()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CancelSlot, resolve_env};

    #[test]
    fn resolve_env_plain_unchanged() {
        assert_eq!(resolve_env("127.0.0.1"), "127.0.0.1");
        assert_eq!(resolve_env("user@host"), "user@host");
    }

    // ponytail: env mutation is process-global; use a unique var name and
    // remove it so parallel test threads don't trip over each other.
    #[test]
    fn resolve_env_dollar_and_braces() {
        let key = "LAZYDB_RESOLVE_ENV_TEST";
        unsafe { std::env::set_var(key, "secret") };
        assert_eq!(resolve_env(&format!("${key}")), "secret");
        assert_eq!(resolve_env(&format!("${{{key}}}/db")), "secret/db");
        assert_eq!(resolve_env(&format!("${key}-x")), "secret-x");
        assert_eq!(resolve_env(&format!("p${{{key}}}p")), "psecretp");
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn resolve_env_unset_left_literal() {
        assert_eq!(
            resolve_env("$LAZYDB_NOPE_UNDEF_XYZ_9f3a"),
            "$LAZYDB_NOPE_UNDEF_XYZ_9f3a"
        );
        assert_eq!(
            resolve_env("${LAZYDB_NOPE_UNDEF_XYZ_9f3a}"),
            "${LAZYDB_NOPE_UNDEF_XYZ_9f3a}"
        );
    }

    #[test]
    fn cancel_slot_round_trip() {
        let s = CancelSlot::new();
        assert_eq!(s.conn_id(), 0);
        s.set(42);
        assert_eq!(s.conn_id(), 42);
        s.clear();
        assert_eq!(s.conn_id(), 0);
    }
}
