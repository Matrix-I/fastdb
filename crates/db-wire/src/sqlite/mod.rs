use anyhow::{Context, Result};
use async_trait::async_trait;
use mem_engine::{ColumnMeta, DataType};
use parking_lot::Mutex;
use rusqlite::types::ValueRef;
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

mod data_search;
mod definition_search;
mod metadata;
mod script;

use crate::stream::{RowSink, report_error};
use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary, row_count_error};

pub struct SqliteDriver {
    pub config: ConnectionConfig,
    conn: Arc<Mutex<Connection>>,
    interrupt_handle: Arc<Mutex<rusqlite::InterruptHandle>>,
    session_database: PathBuf,
    read_only: bool,
}

fn open_connection(database: &Path, read_only: bool) -> Result<Connection> {
    if !read_only {
        return Ok(Connection::open(database)?);
    }
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.authorizer(Some(authorize_read_only));
    Ok(conn)
}

fn authorize_read_only(context: AuthContext<'_>) -> Authorization {
    let allowed = match context.action {
        AuthAction::Select
        | AuthAction::Read { .. }
        | AuthAction::Recursive
        | AuthAction::Transaction { .. }
        | AuthAction::Savepoint { .. } => true,
        AuthAction::Function { function_name } => {
            !function_name.eq_ignore_ascii_case("load_extension")
        }
        AuthAction::Pragma {
            pragma_name,
            pragma_value,
        } => {
            let pragma = pragma_name.to_ascii_lowercase();
            matches!(
                pragma.as_str(),
                "table_info"
                    | "table_xinfo"
                    | "table_list"
                    | "foreign_key_list"
                    | "index_list"
                    | "index_info"
                    | "index_xinfo"
            ) || (pragma_value.is_none()
                && matches!(
                    pragma.as_str(),
                    "query_only" | "database_list" | "schema_version" | "user_version"
                ))
        }
        _ => false,
    };
    if allowed {
        Authorization::Allow
    } else {
        Authorization::Deny
    }
}

impl SqliteDriver {
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        // Each driver owns a unique in-memory database; tab connections share its
        // main schema while keeping transaction state and TEMP objects isolated.
        let in_memory = config.database == ":memory:" || config.database.is_empty();
        anyhow::ensure!(
            !config.read_only || !in_memory,
            "Read-only SQLite requires an existing database file"
        );
        let read_only = config.read_only;
        let session_database = if in_memory {
            PathBuf::from(format!(
                "file:fastdb-{}?mode=memory&cache=shared",
                uuid::Uuid::new_v4()
            ))
        } else if read_only {
            // Capture the path once and bypass SQLite URI options.
            std::path::absolute(&config.database)?
        } else {
            PathBuf::from(&config.database)
        };
        let conn = open_connection(&session_database, read_only)
            .with_context(|| format!("Failed to open SQLite database: {}", config.database))?;

        let interrupt_handle = conn.get_interrupt_handle();

        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
            interrupt_handle: Arc::new(Mutex::new(interrupt_handle)),
            session_database,
            read_only,
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
        let mut observation = crate::sql_observer::Statement::start("SQLite", sql);
        let mut observed_rows = None;
        let outcome = (|| -> Result<()> {
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
            observed_rows = Some(if has_columns {
                sink.total_rows()
            } else if conn.total_changes() == changes_before {
                0
            } else {
                conn.changes()
            });
            sink.finish();
            Ok(())
        })();
        observation.finish(&outcome, observed_rows);
        outcome
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
    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn supports_transfer(&self) -> bool {
        !self.is_read_only()
    }

    fn supports_script_session(&self) -> bool {
        true
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        let database = self.session_database.clone();
        let read_only = self.is_read_only();
        crate::get_runtime()
            .spawn_blocking(move || {
                let connection = open_connection(&database, read_only)?;
                Ok(Arc::new(script::SqliteScriptSession::new(connection))
                    as Arc<dyn crate::script::ScriptSession>)
            })
            .await
            .map_err(|error| anyhow::anyhow!("SQLite session worker failed: {error}"))?
    }

    async fn search_definitions(
        &self,
        schema: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<crate::objects::DefinitionHit>>> {
        definition_search::search(
            self.session_database.clone(),
            self.conn.clone(),
            schema,
            needle,
            cancel,
        )
        .await
    }

    async fn search_table_data(
        &self,
        schema: &str,
        table: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<crate::objects::DataSearchHit>>> {
        data_search::search(
            self.session_database.clone(),
            self.conn.clone(),
            schema,
            table,
            needle,
            cancel,
        )
        .await
    }

    async fn read_data_search_row(
        &self,
        locator: &crate::objects::DataRowLocator,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<crate::objects::DataRowPreview>> {
        anyhow::ensure!(
            !cancel.is_disconnected() && cancel.try_recv().is_err(),
            "Data search cancelled"
        );
        let crate::objects::DataRowIdentity::Sqlite(row) = &locator.0 else {
            anyhow::bail!("Search result belongs to another provider");
        };
        Ok(Some(row.clone()))
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
        crate::sql_observer::observe!(
            "SQLite",
            "SELECT 1;",
            |query| conn.query_row(query, [], |_| Ok(())),
            |_row| Some(1)
        )?;
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
        anyhow::ensure!(!self.is_read_only(), "This SQLite connection is read-only");
        let conn = Arc::clone(&self.conn);
        let statements = statements.to_vec();
        crate::get_runtime()
            .spawn_blocking(move || {
                let mut conn = conn.lock();
                // Dropping the transaction without commit rolls it back
                let tx = crate::sql_observer::observe!("SQLite", "BEGIN DEFERRED", |_query| conn
                    .transaction())?;
                for sql in &statements {
                    let affected = crate::sql_observer::observe!(
                        "SQLite",
                        sql,
                        |query| tx.execute(query, []),
                        |rows| Some(*rows as u64)
                    )? as u64;
                    if affected != 1 {
                        return Err(row_count_error(sql, affected));
                    }
                }
                crate::sql_observer::observe!("SQLite", "COMMIT", |_query| tx.commit())?;
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
        crate::sql_observer::observe!(
            "SQLite",
            "SELECT 'main', name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name;",
            |query| {
                let conn = self.conn.lock();
                let mut stmt = conn.prepare(query)?;
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
            },
            |rows| Some(rows.len() as u64)
        )
    }

    async fn load_relation_ddl(&self, schema: &str, table: &str) -> Result<Option<String>> {
        anyhow::ensure!(
            schema == "main",
            "SQLite DDL is available for the main schema"
        );
        let table = table.to_owned();
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || metadata::relation_ddl(&conn.lock(), &table))
            .await
            .context("SQLite DDL metadata worker failed")?
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        self.native_columns(schema, table).await
    }

    async fn list_foreign_keys(&self) -> Result<Option<Vec<crate::ForeignKey>>> {
        crate::sql_observer::observe!(
            "SQLite",
            r#"SELECT m.name, f.id, f."from", f."table",
                        COALESCE(f."to", (SELECT p.name FROM pragma_table_info(f."table") p
                            WHERE p.pk = f.seq + 1), 'PRIMARY KEY')
                 FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f
                 WHERE m.type = 'table' ORDER BY m.name, f.id, f.seq"#,
            |_query| async {
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
            .await,
            |rows| rows.as_ref().map(|rows| rows.len() as u64)
        )
    }

    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || metadata::index_names(&conn.lock()))
            .await
            .context("SQLite index metadata worker failed")?
    }

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        anyhow::ensure!(!self.is_read_only(), "This SQLite connection is read-only");
        let conn = self.conn.lock();
        crate::sql_observer::observe!("SQLite", sql, |query| conn.execute_batch(query))?;
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
