use std::sync::Arc;
use std::time::Instant;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mem_engine::{ColumnMeta, DataType};
use parking_lot::Mutex;
use rusqlite::types::ValueRef;
use rusqlite::Connection;

use crate::stream::{report_error, RowSink};
use crate::traits::{row_count_error, ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};

pub struct SqliteDriver {
    pub config: ConnectionConfig,
    conn: Arc<Mutex<Connection>>,
    interrupt_handle: Arc<Mutex<rusqlite::InterruptHandle>>,
}

impl SqliteDriver {
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        let conn = if config.database == ":memory:" || config.database.is_empty() {
            Connection::open_in_memory()?
        } else {
            Connection::open(&config.database)
                .with_context(|| format!("Failed to open SQLite database: {}", config.database))?
        };

        let interrupt_handle = conn.get_interrupt_handle();

        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
            interrupt_handle: Arc::new(Mutex::new(interrupt_handle)),
        })
    }

    pub fn new_memory() -> Self {
        let mut config = ConnectionConfig::default();
        config.database = ":memory:".to_string();
        Self::new(config).expect("Failed to initialize in-memory SQLite database")
    }

    fn stream_blocking(conn: &Mutex<Connection>, sql: &str, event_tx: &flume::Sender<StreamEvent>) -> Result<()> {
        let start_time = Instant::now();
        let conn = conn.lock();
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| report_error(event_tx, format!("SQLite prepare error: {e}")))?;

        let columns: Vec<ColumnMeta> = stmt
            .columns()
            .iter()
            .map(|c| ColumnMeta::new(c.name(), c.decl_type().map(sqlite_decl_data_type).unwrap_or(DataType::Text)))
            .collect();
        let mut sink = RowSink::start(columns, event_tx.clone(), start_time)?;

        let mut rows = stmt
            .query([])
            .map_err(|e| sink.fail(format!("SQLite query error: {e}")))?;

        while let Some(row) = rows.next().map_err(|e| sink.fail(format!("SQLite query error: {e}")))? {
            for i in 0..sink.cells_mut().len() {
                let value = row.get_ref(i).map_err(|e| sink.fail(format!("SQLite read error: {e}")))?;
                sink.cells_mut()[i] = match value {
                    ValueRef::Null => None,
                    ValueRef::Integer(v) => Some(v.to_string().into_bytes()),
                    ValueRef::Real(v) => Some(v.to_string().into_bytes()),
                    ValueRef::Text(v) => Some(v.to_vec()),
                    ValueRef::Blob(v) => Some(v.to_vec()),
                };
            }
            sink.commit_row()?;
        }

        sink.finish();
        Ok(())
    }
}

/// Maps a declared column type to the engine's DataType
fn sqlite_decl_data_type(decl_type: &str) -> DataType {
    match decl_type.to_uppercase().as_str() {
        "INTEGER" | "INT" => DataType::Int4,
        "BIGINT" => DataType::Int8,
        "REAL" | "FLOAT" | "DOUBLE" => DataType::Float8,
        "BLOB" => DataType::Bytea,
        _ => DataType::Text,
    }
}

#[async_trait]
impl DatabaseDriver for SqliteDriver {
    async fn ping(&self) -> Result<()> {
        let conn = self.conn.lock();
        // `execute` rejects statements that return rows, so read the row instead
        conn.query_row("SELECT 1;", [], |_| Ok(()))?;
        Ok(())
    }

    async fn execute_streaming(&self, sql: &str, event_tx: flume::Sender<StreamEvent>) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let sql = sql.to_string();
        // rusqlite is synchronous; keep it off the async workers so cancel_active_query can interrupt it
        crate::get_runtime()
            .spawn_blocking(move || Self::stream_blocking(&conn, &sql, &event_tx))
            .await
            .map_err(|e| anyhow::anyhow!("SQLite worker failed: {e}"))?
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        let conn = Arc::clone(&self.conn);
        let statements = statements.to_vec();
        crate::get_runtime()
            .spawn_blocking(move || {
                let mut conn = conn.lock();
                // Dropping the transaction without commit rolls it back
                let tx = conn.transaction()?;
                for sql in &statements {
                    let affected = tx.execute(sql, [])? as u64;
                    if affected != 1 {
                        return Err(row_count_error(sql, affected));
                    }
                }
                tx.commit()?;
                Ok(statements.len() as u64)
            })
            .await
            .map_err(|e| anyhow::anyhow!("SQLite worker failed: {e}"))?
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.interrupt_handle.lock().interrupt();
        Ok(())
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT 'main', name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name;"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(TableSummary {
                schema: row.get(0)?,
                name: row.get(1)?,
                table_type: row.get(2)?,
                estimated_rows: None,
            })
        })?;

        let mut tables = Vec::new();
        for r in rows {
            tables.push(r?);
        }
        Ok(tables)
    }

    async fn list_columns(&self, _schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{table}\");"))?;

        let rows = stmt.query_map([], |row| {
            let name: String = row.get(1)?;
            let dtype_str: String = row.get(2)?;
            let notnull: i32 = row.get(3)?;
            let pk: i32 = row.get(5)?;

            Ok(ColumnMeta::new(name, sqlite_decl_data_type(&dtype_str))
                .with_nullable(notnull == 0)
                .with_pk(pk > 0))
        })?;

        let mut cols = Vec::new();
        for r in rows {
            cols.push(r?);
        }
        Ok(cols)
    }

    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT 'main', tbl_name, name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%' ORDER BY tbl_name, name;"
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }
}
