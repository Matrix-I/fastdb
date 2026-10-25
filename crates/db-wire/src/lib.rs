use std::sync::Arc;
use anyhow::Result;

pub mod traits;
mod stream;
pub mod ssh_tunnel;
pub mod export;
pub mod postgres;
pub mod mysql;
pub mod sqlite;

pub use traits::*;
pub use ssh_tunnel::*;
pub use export::*;
pub use postgres::*;
pub use mysql::*;
pub use sqlite::*;

pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("fastdb-worker")
            .build()
            .expect("Failed to initialize FastDB background tokio runtime")
    })
}

/// Aborts the wrapped task when the awaiting caller is dropped (e.g. on timeout)
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs a future on the shared FastDB runtime so the sockets and background tasks it
/// creates (connection drivers, pool recyclers) outlive the caller's runtime.
pub async fn run_on_shared_runtime<F, T>(fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let mut task = AbortOnDrop(get_runtime().spawn(fut));
    (&mut task.0)
        .await
        .map_err(|e| anyhow::anyhow!("Background database task failed: {e}"))?
}

pub fn create_driver(config: ConnectionConfig) -> Result<Arc<dyn DatabaseDriver>> {
    match config.driver {
        DbDriverKind::Postgres => Ok(Arc::new(PostgresDriver::new(config))),
        DbDriverKind::MySql => Ok(Arc::new(MySqlDriver::new(config))),
        DbDriverKind::Sqlite => Ok(Arc::new(SqliteDriver::new(config)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn test_shared_runtime_tasks_outlive_caller_runtime() {
        let temp_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // A background task spawned while serving the caller (like mysql_async's pool recycler)
        let request_tx = temp_rt
            .block_on(run_on_shared_runtime(async {
                let (request_tx, request_rx) = flume::unbounded::<flume::Sender<u32>>();
                tokio::spawn(async move {
                    while let Ok(reply_tx) = request_rx.recv_async().await {
                        let _ = reply_tx.send(42);
                    }
                });
                Ok(request_tx)
            }))
            .unwrap();
        drop(temp_rt);

        let reply = get_runtime().block_on(async {
            let (reply_tx, reply_rx) = flume::bounded(1);
            request_tx.send(reply_tx).unwrap();
            tokio::time::timeout(Duration::from_secs(2), reply_rx.recv_async()).await
        });
        assert_eq!(reply.unwrap().unwrap(), 42, "background task died with the caller's runtime");
    }

    #[test]
    fn test_shared_runtime_task_is_aborted_when_caller_gives_up() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);

        let temp_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = temp_rt.block_on(async {
            tokio::time::timeout(
                Duration::from_millis(20),
                run_on_shared_runtime(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    flag.store(true, Ordering::SeqCst);
                    Ok(())
                }),
            )
            .await
        });
        assert!(outcome.is_err());

        std::thread::sleep(Duration::from_millis(400));
        assert!(!finished.load(Ordering::SeqCst), "abandoned task kept running");
    }
}
