use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiberius::Client;
use tokio::runtime::Builder;

use super::{Connection, Database, ExecCtx, ExecutionResult, StatementResult};

type Stream = tokio_util::compat::Compat<tokio::net::TcpStream>;

pub struct Mssql {
    client: Arc<Mutex<Client<Stream>>>,
    rt: Arc<tokio::runtime::Runtime>,
}

impl Mssql {
    pub fn open(conn: &Connection, _read_timeout: Option<Duration>) -> Result<Self> {
        let mut config = tiberius::Config::new();
        config.host(&conn.host);
        config.port(conn.port);
        config.authentication(tiberius::AuthMethod::sql_server(
            &conn.username,
            &conn.password,
        ));
        if !conn.database.is_empty() {
            config.database(&conn.database);
        }
        let rt = Arc::new(Builder::new_current_thread().build()?);
        let client = rt.block_on(async {
            let tcp = tokio::net::TcpStream::connect(config.get_addr())
                .await
                .map_err(|e| anyhow::anyhow!("mssql connect: {e}"))?;
            tcp.set_nodelay(true)?;
            let stream = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tcp);
            Client::connect(config, stream)
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))
        })?;
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            rt,
        })
    }
}

/// Extract column names from the first row's metadata.
fn col_names(row: &tiberius::Row) -> Vec<String> {
    row.columns().iter().map(|c| c.name().to_string()).collect()
}

impl Database for Mssql {
    fn kind(&self) -> &str {
        "mssql"
    }

    fn ping(&self) -> Result<()> {
        let mut client = self.client.lock().unwrap();
        self.rt.block_on(async {
            client
                .simple_query("SELECT 1")
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?
                .into_results()
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?;
            Ok(())
        })
    }

    fn schema(&self) -> Result<HashMap<String, Vec<String>>> {
        let mut client = self.client.lock().unwrap();
        self.rt.block_on(async {
            let results = client
                .simple_query(
                    "SELECT TABLE_NAME, COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS \
                     WHERE TABLE_SCHEMA = 'dbo' ORDER BY TABLE_NAME, ORDINAL_POSITION",
                )
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?
                .into_results()
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?;
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for row in results.into_iter().flatten() {
                let table: &str = row.get(0).unwrap_or("");
                let column: &str = row.get(1).unwrap_or("");
                map.entry(table.to_string())
                    .or_default()
                    .push(column.to_string());
            }
            Ok(map)
        })
    }

    fn views(&self) -> Result<Vec<String>> {
        let mut client = self.client.lock().unwrap();
        self.rt.block_on(async {
            let rows = client
                .simple_query(
                    "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.VIEWS \
                     WHERE TABLE_SCHEMA = 'dbo' ORDER BY TABLE_NAME",
                )
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?
                .into_results()
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?;
            Ok(rows
                .into_iter()
                .flatten()
                .filter_map(|r| r.get::<&str, _>(0).map(String::from))
                .collect())
        })
    }

    fn execute_script(&self, sql: &str, ctx: &ExecCtx) -> Result<ExecutionResult> {
        let mut client = self.client.lock().unwrap();
        self.rt.block_on(async {
            let mut all_results: Vec<StatementResult> = Vec::new();
            for part in crate::db::sql::split_statements(sql) {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let results = client
                    .simple_query(part)
                    .await
                    .map_err(|e| anyhow::anyhow!("mssql: {e}"))?
                    .into_results()
                    .await
                    .map_err(|e| anyhow::anyhow!("mssql: {e}"))?;
                let limit = ctx.limit;
                for result in &results {
                    let columns = result.first().map(col_names).unwrap_or_default();
                    if columns.is_empty() {
                        all_results.push(StatementResult {
                            columns: Vec::new(),
                            rows: Vec::new(),
                            rows_affected: 0,
                            truncated: false,
                        });
                    } else {
                        let mut rows: Vec<Vec<String>> = Vec::new();
                        let mut truncated = false;
                        for (i, row) in result.iter().enumerate() {
                            if let Some(cap) = limit
                                && i >= cap
                            {
                                truncated = true;
                                break;
                            }
                            let r: Vec<String> = (0..columns.len())
                                .map(|i| match row.get::<&str, _>(i) {
                                    Some(s) => s.to_string(),
                                    None => "NULL".into(),
                                })
                                .collect();
                            rows.push(r);
                        }
                        all_results.push(StatementResult {
                            columns,
                            rows,
                            rows_affected: 0,
                            truncated,
                        });
                    }
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
        })
    }

    fn primary_keys(&self, table: &str) -> Result<Vec<String>> {
        let mut client = self.client.lock().unwrap();
        self.rt.block_on(async {
            let rows = client
                .simple_query(&format!(
                    "SELECT k.COLUMN_NAME \
                     FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
                     JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE k \
                       ON k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
                      AND k.TABLE_SCHEMA = tc.TABLE_SCHEMA \
                     WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' \
                       AND tc.TABLE_SCHEMA = 'dbo' \
                       AND k.TABLE_NAME = '{table}' \
                     ORDER BY k.ORDINAL_POSITION"
                ))
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?
                .into_results()
                .await
                .map_err(|e| anyhow::anyhow!("mssql: {e}"))?;
            Ok(rows
                .into_iter()
                .flatten()
                .filter_map(|r| r.get::<&str, _>(0).map(String::from))
                .collect())
        })
    }

    fn kill_query(&self, _conn_id: u32) -> Result<()> {
        Err(anyhow::anyhow!("MSSQL cancellation is not yet supported"))
    }

    fn boxed_clone(&self) -> Box<dyn Database> {
        Box::new(Self {
            client: Arc::clone(&self.client),
            rt: Arc::clone(&self.rt),
        })
    }
}
