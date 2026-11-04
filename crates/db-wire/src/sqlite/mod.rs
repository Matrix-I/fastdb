use anyhow::{Context, Result};
use async_trait::async_trait;
use mem_engine::{ColumnMeta, DataType};
use parking_lot::Mutex;
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use std::sync::Arc;
use std::time::Instant;

mod script;

use crate::stream::{RowSink, report_error};
use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary, row_count_error};

pub struct SqliteDriver {
    pub config: ConnectionConfig,
    conn: Arc<Mutex<Connection>>,
    interrupt_handle: Arc<Mutex<rusqlite::InterruptHandle>>,
    session_database: String,
}

impl SqliteDriver {
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        // Each driver owns a unique in-memory database; tab connections share its
        // main schema while keeping transaction state and TEMP objects isolated.
        let session_database = if config.database == ":memory:" || config.database.is_empty() {
            format!(
                "file:fastdb-{}?mode=memory&cache=shared",
                uuid::Uuid::new_v4()
            )
        } else {
            config.database.clone()
        };
        let conn = Connection::open(&session_database)
            .with_context(|| format!("Failed to open SQLite database: {}", config.database))?;

        let interrupt_handle = conn.get_interrupt_handle();

        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
            interrupt_handle: Arc::new(Mutex::new(interrupt_handle)),
            session_database,
        })
    }

    pub fn new_memory() -> Self {
        let config = ConnectionConfig {
            database: ":memory:".to_string(),
            ..ConnectionConfig::default()
        };
        Self::new(config).expect("Failed to initialize in-memory SQLite database")
    }

    fn stream_blocking(
        conn: &Mutex<Connection>,
        sql: &str,
        event_tx: &flume::Sender<StreamEvent>,
        control: Option<&crate::execution::ExecutionControl>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let conn = conn.lock();
        if let Some(control) = control {
            control.check()?;
        }
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| report_error(event_tx, format!("SQLite prepare error: {e}")))?;

        let columns: Vec<ColumnMeta> = stmt
            .columns()
            .iter()
            .map(|c| {
                ColumnMeta::new(
                    c.name(),
                    c.decl_type()
                        .map(sqlite_decl_data_type)
                        .unwrap_or(DataType::Text),
                )
            })
            .collect();
        let has_columns = !columns.is_empty();
        let changes_before = conn.total_changes();
        let mut sink = RowSink::start(columns, event_tx.clone(), start_time)?;

        if let Some(control) = control {
            control.check()?;
        }
        let mut rows = stmt
            .query([])
            .map_err(|e| sink.fail(format!("SQLite query error: {e}")))?;

        while let Some(row) = rows
            .next()
            .map_err(|e| sink.fail(format!("SQLite query error: {e}")))?
        {
            for i in 0..sink.cells_mut().len() {
                let value = row
                    .get_ref(i)
                    .map_err(|e| sink.fail(format!("SQLite read error: {e}")))?;
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

        drop(rows);
        drop(stmt);
        if !has_columns {
            let affected_rows = if conn.total_changes() == changes_before {
                0
            } else {
                conn.changes()
            };
            let _ = event_tx.send(StreamEvent::CommandComplete {
                affected_rows: Some(affected_rows),
            });
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
    fn supports_script_session(&self) -> bool {
        true
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        let database = self.session_database.clone();
        crate::get_runtime()
            .spawn_blocking(move || {
                let connection = Connection::open(database)?;
                Ok(Arc::new(script::SqliteScriptSession::new(connection))
                    as Arc<dyn crate::script::ScriptSession>)
            })
            .await
            .map_err(|error| anyhow::anyhow!("SQLite session worker failed: {error}"))?
    }

    fn metadata_capabilities(&self) -> crate::metadata::MetadataCapabilities {
        use crate::catalog::CatalogSection as C;
        use crate::metadata::{MetadataCapabilities, MetadataFeature as F};
        MetadataCapabilities::new(
            &[F::Columns, F::Indexes, F::ForeignKeys],
            &[
                F::Catalog(C::Partitions),
                F::Catalog(C::Rules),
                F::Catalog(C::Policies),
                F::Catalog(C::Permissions),
            ],
        )
    }

    async fn ping(&self) -> Result<()> {
        let conn = self.conn.lock();
        // `execute` rejects statements that return rows, so read the row instead
        conn.query_row("SELECT 1;", [], |_| Ok(()))?;
        Ok(())
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let sql = sql.to_string();
        // rusqlite is synchronous; keep it off the async workers so cancel_active_query can interrupt it
        crate::get_runtime()
            .spawn_blocking(move || Self::stream_blocking(&conn, &sql, &event_tx, None))
            .await
            .map_err(|e| anyhow::anyhow!("SQLite worker failed: {e}"))?
    }

    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        crate::get_runtime()
            .spawn_blocking(move || Self::stream_blocking(&conn, &sql, &tx, Some(&control)))
            .await
            .map_err(|error| anyhow::anyhow!("SQLite worker failed: {error}"))?
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
        let mut stmt = conn.prepare("SELECT * FROM pragma_table_info(?1)")?;

        let rows = stmt.query_map([table], |row| {
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

    async fn list_foreign_keys(&self) -> Result<Option<Vec<crate::ForeignKey>>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock();
            let mut stmt = conn.prepare(
                r#"SELECT m.name, f.id, f."from", f."table",
                        COALESCE(f."to", (SELECT p.name FROM pragma_table_info(f."table") p
                            WHERE p.pk = f.seq + 1), 'PRIMARY KEY')
                 FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f
                 WHERE m.type = 'table' ORDER BY m.name, f.id, f.seq"#,
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(crate::ForeignKey {
                    name: format!("fk_{}", r.get::<_, i64>(1)?),
                    source_schema: "main".into(),
                    source_table: r.get(0)?,
                    source_column: r.get(2)?,
                    target_schema: "main".into(),
                    target_table: r.get(3)?,
                    target_column: r.get(4)?,
                })
            })?;
            Ok(Some(rows.collect::<rusqlite::Result<Vec<_>>>()?))
        })
        .await?
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

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(sql)?;
        Ok(())
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;

    #[test]
    fn timed_out_queued_write_does_not_execute_after_connection_unlocks() {
        let driver = Arc::new(SqliteDriver::new_memory());
        let connection = driver.conn.lock();
        connection
            .execute_batch("CREATE TABLE pending (id INTEGER)")
            .unwrap();
        crate::get_runtime().block_on(async {
            let (tx, rx) = flume::unbounded();
            let task = tokio::spawn(crate::execution::execute_with_timeout(
                driver.clone(), "INSERT INTO pending VALUES (1)".into(), tx,
                Some(std::time::Duration::ZERO),
            ));
            // The cancellation acknowledgement proves timeout happened while the
            // real blocking worker was unable to acquire this connection.
            while let Ok(event) = rx.recv_async().await {
                if matches!(event, StreamEvent::Notice(message) if message.contains("cancellation requested")) { break; }
            }
            drop(connection);
            task.await.unwrap();
            assert!(matches!(rx.drain().last(), Some(StreamEvent::Error(message)) if message.contains("timed out")));
        });
        let count: i64 = driver
            .conn
            .lock()
            .query_row("SELECT count(*) FROM pending", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
