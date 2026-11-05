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
        crate::get_runtime().spawn_blocking(move || {
            let slot = slot.lock();
            anyhow::ensure!(!closed.load(Ordering::SeqCst), "Script session is closed");
            let connection = slot.as_ref().context("Script session is closed")?.lock();
            anyhow::ensure!(connection.is_autocommit(), "Transfer requires an idle session");
            connection.execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE")?;
            let ordinary: bool = connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM pragma_table_list WHERE schema = 'main' AND name = ?1 AND type = 'table')",
                [&table], |row| row.get(0),
            )?;
            anyhow::ensure!(ordinary, "SQLite transfers require an ordinary table");
            let mut metadata = connection.prepare(r#"SELECT name, type, "notnull", pk FROM pragma_table_info(?1)"#)?;
            let current = metadata.query_map([&table], |row| Ok((row.get::<_, String>(0)?, (row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))))?
                .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
            for column in &columns {
                anyhow::ensure!(
                    current.get(&column.name).is_some_and(|(kind, not_null, primary_key)| sqlite_decl_data_type(kind) == column.data_type && (*not_null == 0) == column.nullable && (*primary_key > 0) == column.is_primary_key),
                    "Destination column changed since mapping: {}", column.name,
                );
            }
            Ok(())
        }).await?
    }

    async fn insert_batch(&self, batch: crate::transfer::batch::Batch) -> Result<u64> {
        let slot = self.connection.clone();
        let closed = self.closed.clone();
        crate::get_runtime()
            .spawn_blocking(move || {
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
                    Ok(statement.execute(rusqlite::params_from_iter(values))? as u64)
                })();
                if closed.load(Ordering::SeqCst) {
                    slot.take();
                }
                result
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
        crate::get_runtime()
            .spawn_blocking(move || {
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
