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
