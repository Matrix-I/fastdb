use super::*;
use crate::script::ScriptSession;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

const STATE_UNKNOWN: u8 = 0;
const STATE_IDLE: u8 = 1;
const STATE_ACTIVE: u8 = 2;
const COMMAND_CAPACITY: usize = 1;
const STATE_TIMEOUT: Duration = Duration::from_secs(2);
const CANCEL_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const CANCEL_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const DISCONNECT_TIMEOUT: Duration = Duration::from_millis(500);

enum CloseReason {
    Idle,
    Interrupted,
    Lost,
}

enum Action {
    Sql {
        sql: String,
        events: flume::Sender<StreamEvent>,
    },
    Insert(crate::transfer::batch::Batch),
    BeginTransfer {
        schema: String,
        table: String,
        columns: Vec<crate::ColumnMeta>,
    },
}
struct Command {
    action: Action,
    reply: tokio::sync::oneshot::Sender<Result<u64>>,
}

pub(super) struct MySqlScriptSession {
    commands: tokio::sync::mpsc::Sender<Command>,
    read_only: bool,
    close: tokio::sync::watch::Sender<bool>,
    finished: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    closed: Arc<AtomicBool>,
    transaction: Arc<AtomicU8>,
}

impl MySqlScriptSession {
    pub(super) fn new(
        connection: mysql_async::Conn,
        tunnel: Option<crate::SshTunnel>,
        read_only: bool,
    ) -> Self {
        let (commands, receiver) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
        let (close, closing) = tokio::sync::watch::channel(false);
        let (completed, finished) = tokio::sync::watch::channel(None);
        let transaction = Arc::new(AtomicU8::new(STATE_IDLE));
        let state = transaction.clone();
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = closed.clone();
        let session_id = format!("MySQL#{:x}", connection.id());
        crate::get_runtime().spawn(crate::sql_observer::with_session(session_id, async move {
            let result = worker(
                connection,
                receiver,
                closing,
                state,
                worker_closed.clone(),
                read_only,
            )
            .await;
            drop(tunnel);
            worker_closed.store(true, Ordering::SeqCst);
            completed.send_replace(Some(result.map_err(|error| format!("{error:#}"))));
        }));
        Self {
            commands,
            read_only,
            close,
            finished,
            closed,
            transaction,
        }
    }

    async fn dispatch(&self, action: Action) -> Result<u64> {
        anyhow::ensure!(
            !self.closed.load(Ordering::SeqCst),
            "Script session is closed"
        );
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(Command { action, reply })
            .await
            .map_err(|_| anyhow::anyhow!("MySQL script session is closed"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("MySQL script session closed during execution"))?
    }

    async fn close_and_wait(&self) -> Result<()> {
        self.abort();
        let mut finished = self.finished.clone();
        finished
            .wait_for(|result| result.is_some())
            .await
            .map_err(|_| {
                anyhow::anyhow!("MySQL session worker stopped without cleanup acknowledgement")
            })?
            .as_ref()
            .expect("completion checked")
            .clone()
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl ScriptSession for MySqlScriptSession {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn in_transaction(&self) -> Option<bool> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        match self.transaction.load(Ordering::SeqCst) {
            STATE_IDLE => Some(false),
            STATE_ACTIVE => Some(true),
            _ => None,
        }
    }

    async fn execute_streaming(&self, sql: &str, events: flume::Sender<StreamEvent>) -> Result<()> {
        self.dispatch(Action::Sql {
            sql: sql.into(),
            events,
        })
        .await
        .map(|_| ())
    }

    async fn begin_transfer(
        &self,
        schema: &str,
        table: &str,
        _expected_object_id: Option<&str>,
        columns: &[crate::ColumnMeta],
    ) -> Result<()> {
        anyhow::ensure!(!self.read_only, "{}", read_only::DENIED);
        self.dispatch(Action::BeginTransfer {
            schema: schema.to_string(),
            table: table.to_string(),
            columns: columns.to_vec(),
        })
        .await
        .map(|_| ())
    }

    async fn insert_batch(&self, batch: crate::transfer::batch::Batch) -> Result<u64> {
        anyhow::ensure!(!self.read_only, "{}", read_only::DENIED);
        self.dispatch(Action::Insert(batch)).await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.close_and_wait().await
    }
    async fn finish(&self) -> Result<()> {
        self.close_and_wait().await
    }

    fn abort(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.close.send_replace(true);
    }
}

impl Drop for MySqlScriptSession {
    fn drop(&mut self) {
        self.abort();
    }
}

async fn worker(
    mut connection: mysql_async::Conn,
    mut commands: tokio::sync::mpsc::Receiver<Command>,
    mut closing: tokio::sync::watch::Receiver<bool>,
    transaction: Arc<AtomicU8>,
    closed: Arc<AtomicBool>,
    read_only: bool,
) -> Result<()> {
    let reason = loop {
        let command = tokio::select! {
            biased;
            _ = closing.changed() => break CloseReason::Idle,
            command = commands.recv() => match command { Some(command) => command, None => break CloseReason::Idle },
        };
        if *closing.borrow() {
            break CloseReason::Idle;
        }
        let executed = tokio::select! {
            biased;
            _ = closing.changed() => None,
            result = async {
                match command.action {
                    Action::Sql { sql, events } => MySqlDriver::stream_connection(&mut connection, sql, events, None, Instant::now(), read_only).await.map(|_| 0),
                    Action::Insert(batch) => {
                        let sql = batch.sql(crate::DbDriverKind::MySql)?;
                        let values = batch.into_values().map(|value| match value {
                            crate::transfer::mapping::Value::Null => mysql_async::Value::NULL,
                            crate::transfer::mapping::Value::Boolean(value) => mysql_async::Value::Int(i64::from(value)),
                            crate::transfer::mapping::Value::Text(text) => mysql_async::Value::Bytes(text.into_bytes()),
                            crate::transfer::mapping::Value::Binary(bytes) => mysql_async::Value::Bytes(bytes),
                        }).collect::<Vec<_>>();
                        async { crate::sql_observer::observe!("MySQL", sql, |query| connection.exec_drop(query, Params::Positional(values)).await, |_result| Some(connection.affected_rows())) }.await?;
                        Ok(connection.affected_rows())
                    }
                    Action::BeginTransfer { schema, table, columns } => {
                        let check_sql = "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = ? AND table_name = ? AND table_type = 'BASE TABLE'";
                        let rows: Vec<u64> = async { crate::sql_observer::observe!("MySQL", check_sql, |query| connection.exec(query, (schema.clone(), table.clone())).await) }.await?;
                        anyhow::ensure!(rows.first().copied().unwrap_or(0) > 0, "MySQL destination table not found or is a view");

                        let col_sql = "SELECT column_name, data_type, is_nullable, column_key FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position";
                        let col_rows: Vec<(String, String, String, String)> = async { crate::sql_observer::observe!("MySQL", col_sql, |query| connection.exec(query, (schema.clone(), table.clone())).await) }.await?;
                        let current: Vec<ColumnMeta> = col_rows.into_iter().map(|(n, dt, is_null, k)| info_schema_column(n, &dt, &is_null, &k)).collect();
                        for expected in &columns {
                            anyhow::ensure!(
                                current.iter().any(|c| c.name == expected.name && c.data_type == expected.data_type && c.nullable == expected.nullable),
                                "Destination column changed since mapping: {}",
                                expected.name
                            );
                        }
                        async { crate::sql_observer::observe!("MySQL", "START TRANSACTION", |query| connection.query_drop(query).await) }.await?;
                        Ok(0)
                    }
                }
            } => Some(result),
        };
        let Some(result) = executed else {
            break CloseReason::Interrupted;
        };
        // An error packet has no transaction flags. A side-effect-free command
        // obtains fresh server state, including InnoDB's automatic deadlock rollback.
        let probe_failure = if result.is_err() {
            tokio::select! {
                biased;
                _ = closing.changed() => break CloseReason::Interrupted,
                refreshed = tokio::time::timeout(STATE_TIMEOUT, async { crate::sql_observer::observe!("MySQL", "DO 0", |query| connection.query_drop(query).await) }) => match refreshed {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) if error.is_fatal() => Some(CloseReason::Lost),
                    Ok(Err(_)) | Err(_) => Some(CloseReason::Interrupted),
                },
            }
        } else {
            None
        };
        let state = if probe_failure.is_none() {
            connection
                .last_ok_packet()
                .map(|packet| {
                    if packet
                        .status_flags()
                        .contains(mysql_async::consts::StatusFlags::SERVER_STATUS_IN_TRANS)
                    {
                        STATE_ACTIVE
                    } else {
                        STATE_IDLE
                    }
                })
                .unwrap_or(STATE_UNKNOWN)
        } else {
            STATE_UNKNOWN
        };
        transaction.store(state, Ordering::SeqCst);
        if probe_failure.is_some() {
            closed.store(true, Ordering::SeqCst);
        }
        let _ = command.reply.send(result);
        if let Some(reason) = probe_failure {
            break reason;
        }
    };
    transaction.store(STATE_UNKNOWN, Ordering::SeqCst);
    close_connection(connection, reason).await
}

async fn close_connection(mut connection: mysql_async::Conn, reason: CloseReason) -> Result<()> {
    // Keep the original socket alive until KILL is acknowledged so its server ID
    // cannot be recycled before the cancellation command targets it.
    let cleanup = match reason {
        CloseReason::Interrupted => {
            kill_connection(connection.opts().clone(), connection.id()).await
        }
        CloseReason::Lost => Ok(()),
        CloseReason::Idle => tokio::time::timeout(STATE_TIMEOUT, async {
            crate::sql_observer::observe!("MySQL", "ROLLBACK", |query| connection
                .query_drop(query)
                .await)
        })
        .await
        .map_err(|_| anyhow::anyhow!("MySQL rollback timed out"))
        .and_then(|result| result.map_err(Into::into)),
    };
    // Explicit disconnect marks the connection terminal before any await. Unlike
    // dropping a live Conn, this cannot spawn an unbounded pending-result drain.
    let disconnected = tokio::time::timeout(DISCONNECT_TIMEOUT, connection.disconnect()).await;
    cleanup?;
    // The server already closed a killed connection; a broken pipe is expected.
    if matches!(reason, CloseReason::Idle) {
        disconnected.map_err(|_| anyhow::anyhow!("MySQL disconnect timed out"))??;
    }
    Ok(())
}

async fn kill_connection(opts: Opts, id: u32) -> Result<()> {
    let mut control = tokio::time::timeout(CANCEL_CONNECT_TIMEOUT, mysql_async::Conn::new(opts))
        .await
        .map_err(|_| anyhow::anyhow!("MySQL cancellation connection timed out"))??;
    let killed = tokio::time::timeout(CANCEL_COMMAND_TIMEOUT, async {
        crate::sql_observer::observe!("MySQL", format!("KILL CONNECTION {id}"), |query| control
            .query_drop(query)
            .await)
    })
    .await
    .map_err(|_| anyhow::anyhow!("MySQL cancellation command timed out"))
    .and_then(|result| result.map_err(anyhow::Error::from));
    // Keep ownership outside the timed query, then explicitly disconnect even
    // after failure so Conn::drop cannot spawn a pending-result drain.
    let disconnected = tokio::time::timeout(DISCONNECT_TIMEOUT, control.disconnect()).await;
    killed?;
    disconnected.map_err(|_| anyhow::anyhow!("MySQL cancellation disconnect timed out"))??;
    Ok(())
}
