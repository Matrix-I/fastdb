use super::*;
use crate::script::ScriptSession;

pub(super) struct PostgresScriptSession {
    client: tokio::sync::Mutex<Option<Client>>,
    cancellation: PgCancellation,
    notices: NoticeTarget,
    read_only: bool,
    transfer_relation: std::sync::atomic::AtomicU32,
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
            transfer_relation: std::sync::atomic::AtomicU32::new(0),
            task: Mutex::new(Some(connection.task)),
        }
    }
}

#[async_trait]
impl ScriptSession for PostgresScriptSession {
    async fn begin_transfer(
        &self,
        schema: &str,
        table: &str,
        expected_object_id: Option<&str>,
        columns: &[ColumnMeta],
    ) -> Result<()> {
        anyhow::ensure!(!self.read_only, "{}", read_only::DENIED);
        let guard = self.client.lock().await;
        let client = guard.as_ref().context("Script session is closed")?;
        async {
            crate::sql_observer::observe!(
                "PostgreSQL",
                "BEGIN; SET LOCAL lock_timeout = '5s'; SET LOCAL statement_timeout = '30s'",
                |query| client.batch_execute(query,).await
            )
        }
        .await?;
        let target =
            crate::sql::SqlDialect(crate::DbDriverKind::Postgres).qualified_table(schema, table);
        async {
            crate::sql_observer::observe!(
                "PostgreSQL",
                &format!("LOCK TABLE ONLY {target} IN ROW EXCLUSIVE MODE"),
                |query| client.batch_execute(query).await
            )
        }
        .await?;
        let expected_object_id =
            expected_object_id.context("PostgreSQL import requires a captured table identity")?;
        let ordinary: bool = async { crate::sql_observer::observe!("PostgreSQL", "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND c.oid::text = $3 AND c.relkind = 'r' AND NOT c.relispartition AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits i WHERE i.inhparent = c.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits i WHERE i.inhrelid = c.oid))", |query| client.query_one(query,
            &[&schema, &table, &expected_object_id],
        ).await, |_row| Some(1)) }.await?.get(0);
        anyhow::ensure!(
            ordinary,
            "PostgreSQL destination identity changed or is not an ordinary table without inheritance"
        );
        let current = metadata::columns(client, Some(schema), Some(table))
            .await?
            .into_iter()
            .flat_map(|(_, _, columns)| columns)
            .collect::<Vec<_>>();
        for expected in columns {
            anyhow::ensure!(
                current.iter().any(|column| column.name == expected.name
                    && column.data_type == expected.data_type
                    && column.nullable == expected.nullable
                    && column.is_primary_key == expected.is_primary_key
                    && column.default_val == expected.default_val
                    && column.raw_type == expected.raw_type
                    && column.collation == expected.collation
                    && column.is_foreign_key == expected.is_foreign_key),
                "Destination column changed since mapping: {}",
                expected.name
            );
        }
        self.transfer_relation.store(
            expected_object_id.parse()?,
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(())
    }

    async fn insert_batch(&self, batch: crate::transfer::batch::Batch) -> Result<u64> {
        anyhow::ensure!(!self.read_only, "{}", read_only::DENIED);
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Script session is closed"))?;
        let relation = self
            .transfer_relation
            .load(std::sync::atomic::Ordering::SeqCst);
        super::transfer::insert(client, batch, (relation != 0).then_some(relation)).await
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
            Some(client) => match async {
                crate::sql_observer::observe!("PostgreSQL", "ROLLBACK", |query| client
                    .batch_execute(query)
                    .await)
            }
            .await
            {
                Ok(()) => Ok(()),
                Err(error)
                    if error.is_closed()
                        || error.code()
                            == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED)
                        || pg_error_message(&error).contains("canceling statement") =>
                {
                    Ok(())
                }
                Err(error) => Err(anyhow::anyhow!(pg_error_message(&error))),
            },
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
