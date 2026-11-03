//! Deadline handling retains ownership until the provider has actually stopped.
use crate::{DatabaseDriver, StreamEvent};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const RELAY_CAPACITY: usize = 64;

/// Run-scoped flag checked after connection acquisition and before sending SQL.
#[derive(Default)]
pub struct ExecutionControl(AtomicBool);
impl ExecutionControl {
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.0.load(Ordering::SeqCst),
            "Statement timed out before execution"
        );
        Ok(())
    }
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(crate) async fn deadline(timeout: Option<Duration>) {
    match timeout {
        Some(duration) => tokio::time::sleep(duration).await,
        None => std::future::pending().await,
    }
}

/// A timeout requests cancellation once; it does not replay SQL or release the
/// workspace operation while a synchronous provider may still be running.
pub async fn execute_with_timeout(
    driver: Arc<dyn DatabaseDriver>,
    sql: String,
    tx: flume::Sender<StreamEvent>,
    timeout: Option<Duration>,
) {
    // Progress coalesces at the source, leaving room for the bounded notices
    // and terminal event without blocking a protocol worker.
    let (events, rx) = flume::bounded(RELAY_CAPACITY);
    let worker_driver = driver.clone();
    let control = Arc::new(ExecutionControl::default());
    let worker_control = control.clone();
    let worker = tokio::spawn(async move {
        worker_driver
            .execute_streaming_controlled(&sql, events, worker_control)
            .await
    });
    let timer = deadline(timeout);
    tokio::pin!(timer);
    let mut timed_out = false;
    let mut terminal = None;
    let mut cancellation = None;
    loop {
        tokio::select! {
            event = rx.recv_async() => match event {
                Ok(event @ (StreamEvent::Completed { .. } | StreamEvent::Error(_))) => terminal = Some(event),
                Ok(StreamEvent::PageAppended { .. }) if tx.len() >= crate::stream::MAX_PENDING_PROGRESS => {},
                Ok(event) => { let _ = tx.send(event); }
                Err(_) => break,
            },
            _ = &mut timer, if !timed_out && terminal.is_none() => {
                timed_out = true;
                control.cancel();
                let cancel_driver = driver.clone();
                let feedback = tx.clone();
                // Continue polling/draining the execution while cancellation is in flight.
                cancellation = Some(tokio::spawn(async move {
                    let result = tokio::time::timeout(Duration::from_secs(5), cancel_driver.cancel_active_query()).await;
                    let message = match result {
                        Ok(Ok(())) => "Timeout: cancellation requested; waiting for provider to stop".into(),
                        Ok(Err(error)) => format!("Timeout cancellation failed: {error:#}; waiting for provider to stop"),
                        Err(_) => "Timeout cancellation request exceeded 5s; waiting for provider to stop".into(),
                    };
                    let _ = feedback.send(StreamEvent::Notice(message));
                }));
            }
        }
    }
    if let Some(cancellation) = cancellation
        && let Err(error) = cancellation.await
    {
        let _ = tx.send(StreamEvent::Notice(format!(
            "Cancellation worker failed: {error}"
        )));
    }
    let result = worker.await;
    let final_event = if timed_out {
        StreamEvent::Error(
            "Statement timed out; earlier committed writes may remain applied".into(),
        )
    } else {
        match result {
            Ok(Ok(())) => terminal.unwrap_or_else(|| {
                StreamEvent::Error("Query stream closed before completion".into())
            }),
            Ok(Err(error)) => match terminal {
                Some(event @ StreamEvent::Error(_)) => event,
                _ => StreamEvent::Error(format!("{error:#}")),
            },
            Err(error) => StreamEvent::Error(format!("Query worker failed: {error}")),
        }
    };
    let _ = tx.send(final_event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnMeta, TableSummary};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct WaitingDriver {
        stop: tokio::sync::Semaphore,
        cancel_started: tokio::sync::watch::Sender<bool>,
        cancel_release: tokio::sync::Semaphore,
        cancellations: AtomicUsize,
        acquire: Option<Arc<tokio::sync::Semaphore>>,
        executions: AtomicUsize,
    }
    #[async_trait]
    impl DatabaseDriver for WaitingDriver {
        async fn execute_streaming(&self, _: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
            self.stop.acquire().await?.forget();
            tx.send(StreamEvent::Completed {
                total_rows: 0,
                elapsed_ms: 0,
            })?;
            Ok(())
        }
        async fn execute_streaming_controlled(
            &self,
            sql: &str,
            tx: flume::Sender<StreamEvent>,
            control: Arc<ExecutionControl>,
        ) -> Result<()> {
            if let Some(acquire) = &self.acquire {
                acquire.acquire().await?.forget();
            }
            control.check()?;
            self.executions.fetch_add(1, Ordering::SeqCst);
            self.execute_streaming(sql, tx).await
        }
        async fn cancel_active_query(&self) -> Result<()> {
            self.cancellations.fetch_add(1, Ordering::SeqCst);
            if self.acquire.is_none() {
                self.stop.add_permits(1);
            }
            self.cancel_started.send_replace(true);
            self.cancel_release.acquire().await?.forget();
            Ok(())
        }
        async fn ping(&self) -> Result<()> {
            unreachable!()
        }
        async fn apply_row_changes(&self, _: &[String]) -> Result<u64> {
            unreachable!()
        }
        async fn list_tables(&self) -> Result<Vec<TableSummary>> {
            unreachable!()
        }
        async fn list_columns(&self, _: &str, _: &str) -> Result<Vec<ColumnMeta>> {
            unreachable!()
        }
    }

    #[test]
    fn deadline_during_connection_acquisition_prevents_later_sql_execution() {
        crate::get_runtime().block_on(async {
            let acquire = Arc::new(tokio::sync::Semaphore::new(0));
            let driver = Arc::new(WaitingDriver {
                stop: tokio::sync::Semaphore::new(1),
                cancel_started: tokio::sync::watch::channel(false).0,
                cancel_release: tokio::sync::Semaphore::new(1),
                cancellations: AtomicUsize::new(0),
                acquire: Some(acquire.clone()), executions: AtomicUsize::new(0),
            });
            let (tx, rx) = flume::unbounded();
            let task = tokio::spawn(execute_with_timeout(driver.clone(), "must not execute".into(), tx, Some(Duration::ZERO)));
            driver.cancel_started.subscribe().wait_for(|v| *v).await.unwrap();
            acquire.add_permits(1);
            task.await.unwrap();
            assert_eq!(driver.executions.load(Ordering::SeqCst), 0);
            assert!(matches!(rx.drain().last(), Some(StreamEvent::Error(message)) if message.contains("timed out")));
        });
    }

    #[test]
    fn deadline_waits_for_cancellation_cleanup_and_suppresses_late_success() {
        crate::get_runtime().block_on(async {
            let driver = Arc::new(WaitingDriver {
                stop: tokio::sync::Semaphore::new(0),
                cancel_started: tokio::sync::watch::channel(false).0,
                cancel_release: tokio::sync::Semaphore::new(0),
                cancellations: AtomicUsize::new(0),
                acquire: None,
                executions: AtomicUsize::new(0),
            });
            let (tx, rx) = flume::unbounded();
            let worker = tokio::spawn(execute_with_timeout(driver.clone(), "wait".into(), tx, Some(Duration::ZERO)));
            driver.cancel_started.subscribe().wait_for(|v| *v).await.unwrap();
            assert!(!worker.is_finished());
            assert!(!rx.drain().any(|e| matches!(e, StreamEvent::Completed { .. } | StreamEvent::Error(_))));
            driver.cancel_release.add_permits(1);
            worker.await.unwrap();
            let events: Vec<_> = rx.drain().collect();
            assert_eq!(driver.cancellations.load(Ordering::SeqCst), 1);
            assert!(!events.iter().any(|e| matches!(e, StreamEvent::Completed { .. })));
            assert!(matches!(events.last(), Some(StreamEvent::Error(message)) if message.contains("timed out")));
        });
    }
}
