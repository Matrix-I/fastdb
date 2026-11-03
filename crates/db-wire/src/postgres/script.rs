use super::*;
use crate::script::ScriptSession;

pub(super) struct PostgresScriptSession {
    client: tokio::sync::Mutex<Option<Client>>,
    token: CancelToken,
    config: ConnectionConfig,
    notices: NoticeTarget,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl PostgresScriptSession {
    pub(super) fn new(
        client: Client,
        task: tokio::task::JoinHandle<()>,
        config: ConnectionConfig,
        notices: NoticeTarget,
    ) -> Self {
        Self {
            token: client.cancel_token(),
            client: tokio::sync::Mutex::new(Some(client)),
            config,
            notices,
            task: Mutex::new(Some(task)),
        }
    }
}

#[async_trait]
impl ScriptSession for PostgresScriptSession {
    async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| report_error(&tx, "Script session is closed"))?;
        // Never call get_or_connect: a lost session is terminal for this run.
        stream_client(client, sql, tx, Instant::now(), &self.notices, None).await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        if self.config.tls_enabled {
            self.token
                .cancel_query(tls_connector(&self.config)?)
                .await?;
        } else {
            self.token.cancel_query(NoTls).await?;
        }
        Ok(())
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
