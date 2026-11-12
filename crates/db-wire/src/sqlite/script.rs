use super::*;
use crate::script::ScriptSession;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct SqliteScriptSession {
    connection: Arc<Mutex<Option<Arc<Mutex<Connection>>>>>,
    interrupt: rusqlite::InterruptHandle,
    closed: Arc<AtomicBool>,
}

impl SqliteScriptSession {
    pub(super) fn new(connection: Connection) -> Self {
        const CANCELLATION_CHECK_OPS: i32 = 1_000;
        let closed = Arc::new(AtomicBool::new(false));
        let cancelled = closed.clone();
        // InterruptHandle alone misses cancellation immediately before sqlite3_step.
        connection.progress_handler(
            CANCELLATION_CHECK_OPS,
            Some(move || cancelled.load(Ordering::SeqCst)),
        );
        Self {
            interrupt: connection.get_interrupt_handle(),
            connection: Arc::new(Mutex::new(Some(Arc::new(Mutex::new(connection))))),
            closed,
        }
    }
}

#[async_trait]
impl ScriptSession for SqliteScriptSession {
    async fn begin_transfer(
        &self,
        schema: &str,
        table: &str,
        _expected_object_id: Option<&str>,
        columns: &[ColumnMeta],
    ) -> Result<()> {
        anyhow::ensure!(
            schema == "main",
            "SQLite transfers require a table in the main schema"
        );
        let slot = self.connection.clone();
        let closed = self.closed.clone();
        let table = table.to_owned();
        let columns = columns.to_vec();
        let session = format!("SQLite#{:x}", Arc::as_ptr(&self.connection) as usize);
        crate::get_runtime().spawn_blocking(move || {
            crate::sql_observer::with_thread_session(session, || {
                let slot = slot.lock();
            anyhow::ensure!(!closed.load(Ordering::SeqCst), "Script session is closed");
            let connection = slot.as_ref().context("Script session is closed")?.lock();
            anyhow::ensure!(connection.is_autocommit(), "Transfer requires an idle session");
            crate::sql_observer::observe!("SQLite", "PRAGMA foreign_keys = ON; BEGIN IMMEDIATE", |query| connection.execute_batch(query))?;
            let ordinary: bool = crate::sql_observer::observe!("SQLite", "SELECT EXISTS (SELECT 1 FROM pragma_table_list WHERE schema = 'main' AND name = ?1 AND type = 'table')", |_query| connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM pragma_table_list WHERE schema = 'main' AND name = ?1 AND type = 'table')",
                [&table], |row| row.get(0),
            ), |_row| Some(1))?;
            anyhow::ensure!(ordinary, "SQLite transfers require an ordinary table");
            let current = super::metadata::columns(&connection, "main", &table)?;
            let by_name: std::collections::HashMap<_, _> = current.iter()
                .map(|(native, nullable)| (native.name.as_str(), (native, *nullable))).collect();
            for column in &columns {
                let (native, declared_nullable) = by_name.get(column.name.as_str())
                    .with_context(|| format!("Destination column changed since mapping: {}", column.name))?;
                anyhow::ensure!(!native.is_generated, "Destination column is generated: {}", column.name);
                // Legacy callers without a declared type used the raw PRAGMA nullability.
                let nullable_matches = native.nullable == column.nullable
                    || (column.raw_type.is_none() && *declared_nullable == column.nullable);
                anyhow::ensure!(
                    nullable_matches && native.data_type == column.data_type
                        && native.is_primary_key == column.is_primary_key
                        && column.raw_type.as_ref().is_none_or(|kind| native.raw_type.as_ref() == Some(kind)
                            && native.generates_key_on_null == column.generates_key_on_null),
                    "Destination column changed since mapping: {}", column.name,
                );
            }
            Ok(())
            })
        }).await?
    }

    async fn insert_batch(&self, batch: crate::transfer::batch::Batch) -> Result<u64> {
        let slot = self.connection.clone();
        let closed = self.closed.clone();
        let session = format!("SQLite#{:x}", Arc::as_ptr(&self.connection) as usize);
        crate::get_runtime()
            .spawn_blocking(move || {
                crate::sql_observer::with_thread_session(session, || {
                    let mut slot = slot.lock();
                    anyhow::ensure!(!closed.load(Ordering::SeqCst), "Script session is closed");
                    let result = (|| {
                        let connection = slot.as_ref().context("Script session is closed")?;
                        let connection = connection.lock();
                        let sql = batch.sql(crate::DbDriverKind::Sqlite)?;
                        let values = batch.into_values().map(|value| match value {
                            crate::transfer::mapping::Value::Null => rusqlite::types::Value::Null,
                            crate::transfer::mapping::Value::Boolean(value) => {
                                rusqlite::types::Value::Integer(i64::from(value))
                            }
                            crate::transfer::mapping::Value::Text(text) => {
                                rusqlite::types::Value::Text(text)
                            }
                            crate::transfer::mapping::Value::Binary(bytes) => {
                                rusqlite::types::Value::Blob(bytes)
                            }
                        });
                        let mut statement = connection.prepare(&sql)?;
                        Ok(crate::sql_observer::observe!(
                            "SQLite",
                            &sql,
                            |_query| statement.execute(rusqlite::params_from_iter(values)),
                            |rows| Some(*rows as u64)
                        )? as u64)
                    })();
                    if closed.load(Ordering::SeqCst) {
                        slot.take();
                    }
                    result
                })
            })
            .await
            .map_err(|error| anyhow::anyhow!("SQLite transfer worker failed: {error}"))?
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn in_transaction(&self) -> Option<bool> {
        let slot = self.connection.try_lock()?;
        let connection = slot.as_ref()?.try_lock()?;
        Some(!connection.is_autocommit())
    }

    async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
        let slot = self.connection.clone();
        let closed = self.closed.clone();
        let sql = sql.to_owned();
        let session = format!("SQLite#{:x}", Arc::as_ptr(&self.connection) as usize);
        crate::get_runtime()
            .spawn_blocking(move || {
                crate::sql_observer::with_thread_session(session, || {
                    // Hold the slot through execution so closure cannot race a queued
                    // worker. Cancellation interrupts independently of this lock.
                    let mut slot = slot.lock();
                    anyhow::ensure!(!closed.load(Ordering::SeqCst), "Script session is closed");
                    let connection = slot.as_ref().context("Script session is closed")?;
                    let result = SqliteDriver::stream_blocking(connection, &sql, &tx, None);
                    if closed.load(Ordering::SeqCst) {
                        slot.take();
                    }
                    result
                })
            })
            .await
            .map_err(|error| anyhow::anyhow!("SQLite session worker failed: {error}"))?
    }

    async fn cancel_active_query(&self) -> Result<()> {
        // SQLite resets an interrupt after execution finishes. Marking the session
        // closed also prevents a queued statement from starting after cancellation.
        self.abort();
        Ok(())
    }

    async fn finish(&self) -> Result<()> {
        self.abort();
        let slot = self.connection.clone();
        crate::get_runtime()
            .spawn_blocking(move || {
                // Closing the connection rolls back its transaction and destroys
                // TEMP objects. Wait for an interrupted worker before acknowledging.
                slot.lock().take();
            })
            .await
            .map_err(|error| anyhow::anyhow!("SQLite session cleanup failed: {error}"))?;
        Ok(())
    }

    fn abort(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.interrupt.interrupt();
        if let Some(mut slot) = self.connection.try_lock() {
            slot.take();
        }
    }
}

impl Drop for SqliteScriptSession {
    fn drop(&mut self) {
        self.abort();
    }
}
