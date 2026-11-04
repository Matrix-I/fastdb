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

struct Command {
    sql: String,
    events: flume::Sender<StreamEvent>,
    reply: tokio::sync::oneshot::Sender<Result<()>>,
}

pub(super) struct MySqlScriptSession {
    commands: tokio::sync::mpsc::Sender<Command>,
    close: tokio::sync::watch::Sender<bool>,
    finished: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    closed: Arc<AtomicBool>,
    transaction: Arc<AtomicU8>,
}

impl MySqlScriptSession {
    pub(super) fn new(connection: mysql_async::Conn) -> Self {
        let (commands, receiver) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
        let (close, closing) = tokio::sync::watch::channel(false);
        let (completed, finished) = tokio::sync::watch::channel(None);
        let transaction = Arc::new(AtomicU8::new(STATE_IDLE));
        let state = transaction.clone();
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = closed.clone();
        crate::get_runtime().spawn(async move {
            let result = worker(connection, receiver, closing, state, worker_closed.clone()).await;
            worker_closed.store(true, Ordering::SeqCst);
            completed.send_replace(Some(result.map_err(|error| format!("{error:#}"))));
        });
        Self {
            commands,
            close,
            finished,
            closed,
            transaction,
        }
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
        anyhow::ensure!(
            !self.closed.load(Ordering::SeqCst),
            "Script session is closed"
        );
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(Command {
                sql: sql.into(),
                events,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("MySQL script session is closed"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("MySQL script session closed during execution"))?
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
            result = MySqlDriver::stream_connection(&mut connection, command.sql, command.events, None, Instant::now()) => Some(result),
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
                refreshed = tokio::time::timeout(STATE_TIMEOUT, connection.query_drop("DO 0")) => match refreshed {
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
        CloseReason::Idle => tokio::time::timeout(STATE_TIMEOUT, connection.query_drop("ROLLBACK"))
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
    let killed = tokio::time::timeout(
        CANCEL_COMMAND_TIMEOUT,
        control.query_drop(format!("KILL CONNECTION {id}")),
    )
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
