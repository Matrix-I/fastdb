//! Run-scoped sessions. Cancellation is owned by the runner, so a late cancel
//! cannot reach a subsequent statement, rollback or another run's connection.
use crate::{DatabaseDriver, StreamEvent};
use anyhow::Result;
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

#[async_trait]
pub trait ScriptSession: Send + Sync {
    async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()>;
    async fn cancel_active_query(&self) -> Result<()>;
    /// Roll back any remaining transaction and close the session.
    async fn finish(&self) -> Result<()>;
    /// Force socket closure, including when graceful cleanup times out.
    fn abort(&self);
}

/// Tab owner. Dropping or disconnecting it closes the socket even while idle.
/// The worker only borrows the slot; it cannot keep a closed tab alive.
#[derive(Default)]
pub struct TabSession {
    session: std::sync::Mutex<Option<Arc<dyn ScriptSession>>>,
}
impl TabSession {
    pub fn close(&self) {
        if let Some(session) = self.session.lock().unwrap().take() {
            session.abort();
        }
    }
}
impl Drop for TabSession {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct ScriptControl {
    cancel: tokio::sync::watch::Sender<bool>,
    finished: tokio::sync::watch::Sender<bool>,
}
impl Default for ScriptControl {
    fn default() -> Self {
        Self {
            cancel: tokio::sync::watch::channel(false).0,
            finished: tokio::sync::watch::channel(false).0,
        }
    }
}
impl ScriptControl {
    pub fn request_cancel(&self) {
        self.cancel.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    pub async fn wait_finished(&self) {
        let mut rx = self.finished.subscribe();
        let _ = rx.wait_for(|finished| *finished).await;
    }
    async fn cancelled(&self) {
        let mut rx = self.cancel.subscribe();
        let _ = rx.wait_for(|cancel| *cancel).await;
    }
}

#[derive(Debug)]
pub enum ScriptEvent {
    StatementStarted { sql: String },
    Stream(StreamEvent),
    CleaningUp,
    Finished(ScriptOutcome),
}

#[derive(Debug, Default)]
pub struct ScriptOutcome {
    pub cancelled: bool,
    pub error: Option<String>,
    pub cleanup_error: Option<String>,
}

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

fn send(tx: &flume::Sender<ScriptEvent>, control: &ScriptControl, event: ScriptEvent) {
    if tx.send(event).is_err() {
        control.request_cancel();
    }
}

/// The input must have passed the provider-aware script preflight. Executes each
/// statement once and streams results without collecting result rows in memory.
pub async fn run_script(
    driver: Arc<dyn DatabaseDriver>,
    statements: Vec<String>,
    control: Arc<ScriptControl>,
    tx: flume::Sender<ScriptEvent>,
) {
    run_script_inner(driver, statements, control, tx, None).await;
}

/// Execute on the same physical connection across successive tab runs.
pub async fn run_tab_script(
    driver: Arc<dyn DatabaseDriver>,
    statements: Vec<String>,
    control: Arc<ScriptControl>,
    tx: flume::Sender<ScriptEvent>,
    owner: std::sync::Weak<TabSession>,
) {
    run_script_inner(driver, statements, control, tx, Some(owner)).await;
}

async fn run_script_inner(
    driver: Arc<dyn DatabaseDriver>,
    statements: Vec<String>,
    control: Arc<ScriptControl>,
    tx: flume::Sender<ScriptEvent>,
    owner: Option<std::sync::Weak<TabSession>>,
) {
    struct CompletionGuard(Arc<ScriptControl>);
    impl Drop for CompletionGuard {
        fn drop(&mut self) {
            self.0.finished.send_replace(true);
        }
    }
    let _completion = CompletionGuard(control.clone());
    let existing = owner
        .as_ref()
        .and_then(|o| o.upgrade())
        .and_then(|o| o.session.lock().unwrap().clone());
    let opened = tokio::select! {
        biased;
        _ = control.cancelled() => None,
        result = async {
            if let Some(session) = existing { Ok(session) }
            else { driver.open_script_session().await }
        } => Some(result),
    };
    let session = match opened {
        None => {
            send(
                &tx,
                &control,
                ScriptEvent::Finished(ScriptOutcome {
                    cancelled: true,
                    ..Default::default()
                }),
            );
            return;
        }
        Some(Err(error)) => {
            send(
                &tx,
                &control,
                ScriptEvent::Finished(ScriptOutcome {
                    error: Some(format!("Cannot open script session: {error:#}")),
                    ..Default::default()
                }),
            );
            return;
        }
        Some(Ok(session)) => session,
    };
    // Force-close on every exit path, including task abort or receiver loss.
    struct SessionGuard(Option<Arc<dyn ScriptSession>>);
    impl Drop for SessionGuard {
        fn drop(&mut self) {
            if let Some(session) = &self.0 {
                session.abort();
            }
        }
    }
    let mut guard = SessionGuard(Some(session.clone()));
    if let Some(owner) = &owner {
        if let Some(owner) = owner.upgrade() {
            *owner.session.lock().unwrap() = Some(session.clone());
        } else {
            control.request_cancel();
        }
    }
    let mut outcome = ScriptOutcome::default();
    for sql in statements {
        if control.is_cancelled() {
            break;
        }
        send(
            &tx,
            &control,
            ScriptEvent::StatementStarted { sql: sql.clone() },
        );
        if control.is_cancelled() {
            break;
        }
        let (row_tx, row_rx) = flume::unbounded();
        let events = tx.clone();
        let relay_control = control.clone();
        let relay = tokio::spawn(async move {
            while let Ok(event) = row_rx.recv_async().await {
                send(&events, &relay_control, ScriptEvent::Stream(event));
            }
        });
        let executed = {
            let execution = session.execute_streaming(&sql, row_tx);
            tokio::pin!(execution);
            tokio::select! {
                biased;
                _ = control.cancelled() => None,
                result = &mut execution => Some(result),
            }
        }; // Drop execution BEFORE sending cancellation: no later protocol step can run.
        match executed {
            None => {
                outcome.cleanup_error = match tokio::time::timeout(
                    CLEANUP_TIMEOUT,
                    session.cancel_active_query(),
                )
                .await
                {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("Cancellation failed: {error:#}")),
                    Err(_) => Some("Cancellation timed out; closing session".into()),
                };
            }
            Some(Err(error)) => outcome.error = Some(format!("{error:#}")),
            Some(Ok(())) => {}
        }
        // All events for this statement must precede the next StatementStarted.
        if let Err(error) = relay.await {
            outcome.error = Some(format!("Script event stream failed: {error}"));
        }
        if outcome.error.is_some() || control.is_cancelled() {
            break;
        }
    }
    outcome.cancelled = control.is_cancelled();
    if owner.as_ref().is_some_and(|o| o.strong_count() > 0)
        && !outcome.cancelled
        && outcome.cleanup_error.is_none()
    {
        guard.0 = None;
        send(&tx, &control, ScriptEvent::Finished(outcome));
        return;
    }
    if let Some(owner) = owner.and_then(|o| o.upgrade()) {
        owner.session.lock().unwrap().take();
    }
    send(&tx, &control, ScriptEvent::CleaningUp);
    let cleanup = match tokio::time::timeout(CLEANUP_TIMEOUT, session.finish()).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("Session cleanup failed: {error:#}")),
        Err(_) => Some("Session cleanup timed out; connection discarded".into()),
    };
    if let Some(error) = cleanup {
        outcome.cleanup_error = Some(match outcome.cleanup_error {
            Some(previous) => format!("{previous}; {error}"),
            None => error,
        });
    }
    session.abort();
    // A cancel during cleanup also prevents the caller from reporting success.
    outcome.cancelled |= control.is_cancelled();
    send(&tx, &control, ScriptEvent::Finished(outcome));
}

#[cfg(test)]
mod tests {
    use super::{ScriptControl, ScriptEvent, ScriptOutcome, ScriptSession, run_script};
    use crate::{ColumnMeta, DatabaseDriver, StreamEvent, TableSummary};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct FakeSession {
        calls: Mutex<Vec<String>>,
        started: tokio::sync::watch::Sender<bool>,
        cancelling: tokio::sync::watch::Sender<bool>,
        cancel_gate: tokio::sync::Semaphore,
        fail_cleanup: bool,
        aborted: AtomicBool,
        cancel_after: Option<Arc<ScriptControl>>,
    }
    impl FakeSession {
        fn new(fail_cleanup: bool, cancel_after: Option<Arc<ScriptControl>>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(vec![]),
                started: tokio::sync::watch::channel(false).0,
                cancelling: tokio::sync::watch::channel(false).0,
                cancel_gate: tokio::sync::Semaphore::new(0),
                fail_cleanup,
                aborted: AtomicBool::new(false),
                cancel_after,
            })
        }
    }
    #[async_trait]
    impl ScriptSession for FakeSession {
        async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
            self.calls.lock().unwrap().push(sql.into());
            if sql == "wait" {
                struct Guard<'a>(&'a FakeSession);
                impl Drop for Guard<'_> {
                    fn drop(&mut self) {
                        self.0.calls.lock().unwrap().push("drop_wait".into());
                    }
                }
                let _guard = Guard(self);
                self.started.send_replace(true);
                std::future::pending::<()>().await;
            }
            if sql == "fail" {
                anyhow::bail!("statement failed");
            }
            tx.send(StreamEvent::Completed {
                total_rows: 0,
                elapsed_ms: 0,
            })?;
            if let Some(control) = &self.cancel_after {
                control.request_cancel();
            }
            Ok(())
        }
        async fn cancel_active_query(&self) -> Result<()> {
            self.calls.lock().unwrap().push("cancel".into());
            self.cancelling.send_replace(true);
            self.cancel_gate.acquire().await.unwrap().forget();
            Ok(())
        }
        async fn finish(&self) -> Result<()> {
            self.calls.lock().unwrap().push("finish".into());
            if self.fail_cleanup {
                anyhow::bail!("rollback failed");
            }
            Ok(())
        }
        fn abort(&self) {
            if !self.aborted.swap(true, Ordering::SeqCst) {
                self.calls.lock().unwrap().push("abort".into());
            }
        }
    }
    struct FakeDriver {
        session: Arc<FakeSession>,
        opens: AtomicUsize,
    }
    #[async_trait]
    impl DatabaseDriver for FakeDriver {
        fn supports_script_session(&self) -> bool {
            true
        }
        async fn open_script_session(&self) -> Result<Arc<dyn ScriptSession>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Ok(self.session.clone())
        }
        async fn ping(&self) -> Result<()> {
            unreachable!()
        }
        async fn execute_streaming(&self, _: &str, _: flume::Sender<StreamEvent>) -> Result<()> {
            unreachable!()
        }
        async fn apply_row_changes(&self, _: &[String]) -> Result<u64> {
            unreachable!()
        }
        async fn cancel_active_query(&self) -> Result<()> {
            panic!("must cancel the dedicated session, not the shared driver")
        }
        async fn list_tables(&self) -> Result<Vec<TableSummary>> {
            unreachable!()
        }
        async fn list_columns(&self, _: &str, _: &str) -> Result<Vec<ColumnMeta>> {
            unreachable!()
        }
    }
    fn fake_driver(session: Arc<FakeSession>) -> Arc<FakeDriver> {
        Arc::new(FakeDriver {
            session,
            opens: AtomicUsize::new(0),
        })
    }
    fn outcome(rx: &flume::Receiver<ScriptEvent>) -> ScriptOutcome {
        rx.drain()
            .find_map(|event| match event {
                ScriptEvent::Finished(outcome) => Some(outcome),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn tab_session_reuses_connection_through_errors_and_closes_with_owner() {
        crate::get_runtime().block_on(async {
            let session = FakeSession::new(false, None);
            let driver = fake_driver(session.clone());
            let owner = Arc::new(super::TabSession::default());
            for sql in ["first", "fail", "recover"] {
                let (tx, rx) = flume::unbounded();
                super::run_tab_script(
                    driver.clone(),
                    vec![sql.into()],
                    Arc::new(ScriptControl::default()),
                    tx,
                    Arc::downgrade(&owner),
                )
                .await;
                assert_eq!(outcome(&rx).error.is_some(), sql == "fail");
            }
            assert_eq!(driver.opens.load(Ordering::SeqCst), 1);
            assert_eq!(*session.calls.lock().unwrap(), ["first", "fail", "recover"]);
            drop(owner);
            assert!(session.aborted.load(Ordering::SeqCst));
        });
    }

    #[test]
    fn first_error_stops_execution_and_cleanup_errors_remain_separate() {
        crate::get_runtime().block_on(async {
            let session = FakeSession::new(true, None);
            let driver = fake_driver(session.clone());
            let control = Arc::new(ScriptControl::default());
            let (tx, rx) = flume::unbounded();
            run_script(
                driver.clone(),
                vec!["first".into(), "fail".into(), "never".into()],
                control.clone(),
                tx,
            )
            .await;
            assert_eq!(
                *session.calls.lock().unwrap(),
                ["first", "fail", "finish", "abort"]
            );
            let result = outcome(&rx);
            assert_eq!(result.error.as_deref(), Some("statement failed"));
            assert!(result.cleanup_error.unwrap().contains("rollback failed"));
            assert_eq!(driver.opens.load(Ordering::SeqCst), 1);
            control.wait_finished().await;
        });
    }

    #[test]
    fn cancel_drops_execution_before_rpc_and_waits_for_ack_before_cleanup() {
        crate::get_runtime().block_on(async {
            let session = FakeSession::new(false, None);
            let control = Arc::new(ScriptControl::default());
            let (tx, rx) = flume::unbounded();
            let task = tokio::spawn(run_script(
                fake_driver(session.clone()),
                vec!["wait".into(), "never".into()],
                control.clone(),
                tx,
            ));
            session.started.subscribe().wait_for(|v| *v).await.unwrap();
            control.request_cancel();
            session
                .cancelling
                .subscribe()
                .wait_for(|v| *v)
                .await
                .unwrap();
            assert_eq!(
                *session.calls.lock().unwrap(),
                ["wait", "drop_wait", "cancel"]
            );
            session.cancel_gate.add_permits(1);
            task.await.unwrap();
            assert_eq!(
                *session.calls.lock().unwrap(),
                ["wait", "drop_wait", "cancel", "finish", "abort"]
            );
            assert!(outcome(&rx).cancelled);
            // A late request is just a flag; it can never send another server cancel.
            control.request_cancel();
            assert_eq!(
                session
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|s| s.as_str() == "cancel")
                    .count(),
                1
            );
        });
    }

    #[test]
    fn pre_cancel_and_cancellation_between_statements_do_not_execute_more_sql() {
        crate::get_runtime().block_on(async {
            let control = Arc::new(ScriptControl::default());
            control.request_cancel();
            let session = FakeSession::new(false, None);
            let driver = fake_driver(session);
            let (tx, rx) = flume::unbounded();
            run_script(driver.clone(), vec!["never".into()], control, tx).await;
            assert_eq!(driver.opens.load(Ordering::SeqCst), 0);
            assert!(outcome(&rx).cancelled);

            let control = Arc::new(ScriptControl::default());
            let session = FakeSession::new(false, Some(control.clone()));
            let (tx, rx) = flume::unbounded();
            run_script(
                fake_driver(session.clone()),
                vec!["first".into(), "never".into()],
                control,
                tx,
            )
            .await;
            assert_eq!(*session.calls.lock().unwrap(), ["first", "finish", "abort"]);
            assert!(outcome(&rx).cancelled);
        });
    }

    #[test]
    fn receiver_loss_cancels_work_and_task_abort_closes_the_session() {
        crate::get_runtime().block_on(async {
            let session = FakeSession::new(false, None);
            let (tx, rx) = flume::unbounded();
            drop(rx);
            run_script(
                fake_driver(session.clone()),
                vec!["never".into()],
                Arc::new(ScriptControl::default()),
                tx,
            )
            .await;
            assert_eq!(*session.calls.lock().unwrap(), ["finish", "abort"]);

            let session = FakeSession::new(false, None);
            let control = Arc::new(ScriptControl::default());
            let (tx, _rx) = flume::unbounded();
            let task = tokio::spawn(run_script(
                fake_driver(session.clone()),
                vec!["wait".into()],
                control.clone(),
                tx,
            ));
            session.started.subscribe().wait_for(|v| *v).await.unwrap();
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            assert!(session.aborted.load(Ordering::SeqCst));
            control.wait_finished().await;
        });
    }
}
