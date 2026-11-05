use super::*;
use crate::script::ScriptSession;

pub(super) struct PostgresScriptSession {
    client: tokio::sync::Mutex<Option<Client>>,
    cancellation: PgCancellation,
    notices: NoticeTarget,
    read_only: bool,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl PostgresScriptSession {
    pub(super) fn new(
        connection: EstablishedConnection,
        notices: NoticeTarget,
        read_only: bool,
    ) -> Self {
        Self {
            client: tokio::sync::Mutex::new(Some(connection.client)),
            cancellation: connection.cancellation,
            notices,
            read_only,
            task: Mutex::new(Some(connection.task)),
        }
    }
}

#[async_trait]
impl ScriptSession for PostgresScriptSession {
    async fn insert_batch(&self, batch: crate::transfer::batch::Batch) -> Result<u64> {
        anyhow::ensure!(!self.read_only, "{}", read_only::DENIED);
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Script session is closed"))?;
        super::transfer::insert(client, batch).await
    }

    async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| report_error(&tx, "Script session is closed"))?;
        // Never call get_or_connect: a lost session is terminal for this run.
        let sql = if self.read_only {
            read_only::prepare(client, sql)
                .await
                .map_err(|error| report_error(&tx, format!("{error:#}")))?
        } else {
            sql
        };
        stream_client(client, sql, tx, Instant::now(), &self.notices, None, false).await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.cancellation.cancel().await
    }

    async fn finish(&self) -> Result<()> {
        let client = self.client.lock().await.take();
        let result = match client {
            Some(client) => client
                .batch_execute("ROLLBACK")
                .await
                .map_err(|e| anyhow::anyhow!(pg_error_message(&e))),
            None => Ok(()),
        };
        self.abort();
        result
    }

    fn abort(&self) {
        if let Some(task) = self.task.lock().take() {
            task.abort();
        }
    }
}

impl Drop for PostgresScriptSession {
    fn drop(&mut self) {
        self.abort();
    }
}
