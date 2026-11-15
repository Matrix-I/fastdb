//! Live PostgreSQL monitoring and signaling on dedicated fixture connections.

use db_wire::postgres::session_monitor::{SessionMonitor, SessionSignal, SessionSnapshot};
use db_wire::{ConnectionConfig, DbDriverKind};
use std::time::Duration;
use tokio_postgres::{Client, NoTls, error::SqlState};

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return None;
    };
    let env = |name: &str| std::env::var(format!("FASTDB_TEST_PG_{name}")).ok();
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: env("DATABASE").unwrap_or_else(|| "postgres".into()),
        user: env("USER").unwrap_or_else(|| "postgres".into()),
        password: env("PASSWORD"),
        use_unix_socket: false,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}

async fn client(config: &ConnectionConfig) -> Client {
    let mut settings = tokio_postgres::Config::new();
    settings
        .host(&config.host)
        .port(config.port)
        .user(&config.user)
        .dbname(&config.database);
    if let Some(password) = &config.password {
        settings.password(password);
    }
    let (client, connection) = settings.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn pid(client: &Client) -> i32 {
    client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0)
}

async fn until(
    monitor: &SessionMonitor,
    ready: impl Fn(&SessionSnapshot) -> bool,
) -> SessionSnapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = monitor.snapshot().await.unwrap();
            if ready(&snapshot) {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expected PostgreSQL activity did not appear")
}

#[test]
fn postgres_monitor_refreshes_blocking_chain_and_signals_only_selected_backend() {
    let Some(config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let monitor = SessionMonitor::connect(config.clone()).await.unwrap();
            let root = client(&config).await;
            let middle = client(&config).await;
            let leaf = client(&config).await;
            let bystander = client(&config).await;
            let root_pid = pid(&root).await;
            let middle_pid = pid(&middle).await;
            let leaf_pid = pid(&leaf).await;
            let bystander_pid = pid(&bystander).await;
            // A unique advisory-lock key space for this fixture, derived from its live PID.
            let hold = "SELECT pg_advisory_lock($1, $2)";
            root.query_one(hold, &[&root_pid, &1_i32]).await.unwrap();
            middle.query_one(hold, &[&root_pid, &2_i32]).await.unwrap();
            let middle_wait = tokio::spawn(async move {
                let error = middle
                    .query_one(hold, &[&root_pid, &1_i32])
                    .await
                    .unwrap_err();
                (middle, error)
            });
            let leaf_wait = tokio::spawn(async move {
                leaf.query_one(hold, &[&root_pid, &2_i32]).await.unwrap();
                leaf
            });
            let snapshot = until(&monitor, |snapshot| {
                snapshot
                    .rows
                    .iter()
                    .any(|row| row.pid == middle_pid && row.blockers == [root_pid])
                    && snapshot
                        .rows
                        .iter()
                        .any(|row| row.pid == leaf_pid && row.blockers == [middle_pid])
            })
            .await;
            let middle_row = snapshot
                .rows
                .iter()
                .find(|row| row.pid == middle_pid)
                .unwrap();
            assert_eq!(middle_row.state.as_deref(), Some("active"));
            assert_eq!(middle_row.wait_type.as_deref(), Some("Lock"));
            assert_eq!(middle_row.active_query.as_deref(), Some(hold));
            assert!(middle_row.active_ms.is_some());
            let target = middle_row.identity.as_ref().unwrap();
            let mut stale = target.clone();
            stale.backend_start -= chrono::Duration::seconds(1);
            assert!(
                monitor
                    .signal(&stale, SessionSignal::Terminate)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("changed")
            );
            stale = target.clone();
            stale.query_start = stale
                .query_start
                .map(|start| start - chrono::Duration::seconds(1));
            assert!(
                monitor
                    .signal(&stale, SessionSignal::Cancel)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("changed")
            );
            let readonly = SessionMonitor::connect(ConnectionConfig {
                read_only: true,
                ..config.clone()
            })
            .await
            .unwrap();
            assert!(
                readonly
                    .snapshot()
                    .await
                    .unwrap()
                    .rows
                    .iter()
                    .any(|row| row.pid == root_pid)
            );
            assert!(
                readonly
                    .signal(target, SessionSignal::Terminate)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("read-only")
            );
            assert!(!middle_wait.is_finished() && !leaf_wait.is_finished());

            monitor.signal(target, SessionSignal::Cancel).await.unwrap();
            let (middle, error) = middle_wait.await.unwrap();
            assert_eq!(error.code(), Some(&SqlState::QUERY_CANCELED));
            assert_eq!(pid(&middle).await, middle_pid);
            let refreshed = until(&monitor, |snapshot| {
                snapshot.rows.iter().any(|row| {
                    row.pid == middle_pid
                        && row.blockers.is_empty()
                        && row.state.as_deref() == Some("idle")
                }) && snapshot
                    .rows
                    .iter()
                    .any(|row| row.pid == leaf_pid && row.blockers == [middle_pid])
            })
            .await;
            assert!(
                monitor
                    .signal(target, SessionSignal::Cancel)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("changed")
            );
            let target = refreshed
                .rows
                .iter()
                .find(|row| row.pid == middle_pid)
                .unwrap()
                .identity
                .as_ref()
                .unwrap();
            monitor
                .signal(target, SessionSignal::Terminate)
                .await
                .unwrap();
            let leaf = leaf_wait.await.unwrap();
            assert_eq!(pid(&leaf).await, leaf_pid);
            let final_snapshot = until(&monitor, |snapshot| {
                snapshot.rows.iter().all(|row| row.pid != middle_pid)
            })
            .await;
            assert!(final_snapshot.rows.iter().any(|row| row.pid == root_pid));
            assert!(
                final_snapshot
                    .rows
                    .iter()
                    .any(|row| row.pid == bystander_pid)
            );
            assert!(
                final_snapshot
                    .rows
                    .iter()
                    .find(|row| row.pid == leaf_pid)
                    .unwrap()
                    .blockers
                    .is_empty()
            );
            assert!(middle.query_one("SELECT 1", &[]).await.is_err());
            assert_eq!(pid(&bystander).await, bystander_pid);
        })
        .await
        .expect("monitor fixture timed out");
    });
}

#[test]
fn postgres_monitor_respects_visibility_and_native_signal_permissions() {
    use futures::FutureExt as _;
    let Some(config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        let (owner, target, target_pid) = tokio::time::timeout(Duration::from_secs(10), async {
            let owner = client(&config).await;
            let target = client(&config).await;
            let target_pid = pid(&target).await;
            (owner, target, target_pid)
        })
        .await
        .expect("fixture setup timed out");
        // Only generated hexadecimal data is interpolated into the fixture DDL.
        let role = format!("fastdb_monitor_{}", uuid::Uuid::new_v4().simple());
        let password = uuid::Uuid::new_v4().simple().to_string();
        let work = tokio::time::timeout(
            Duration::from_secs(20),
            std::panic::AssertUnwindSafe(async {
                owner
                    .batch_execute(&format!("CREATE ROLE {role} LOGIN PASSWORD '{password}'"))
                    .await
                    .unwrap();
                let restricted = ConnectionConfig {
                    user: role.clone(),
                    password: Some(password),
                    ..config
                };
                let unprivileged = SessionMonitor::connect(restricted.clone()).await.unwrap();
                let snapshot = unprivileged.snapshot().await.unwrap();
                let row = snapshot
                    .rows
                    .iter()
                    .find(|row| row.pid == target_pid)
                    .unwrap();
                assert!(
                    row.identity.is_none() && row.state.is_none() && row.active_query.is_none()
                );
                owner
                    .batch_execute(&format!("GRANT pg_read_all_stats TO {role}"))
                    .await
                    .unwrap();
                let reader = SessionMonitor::connect(restricted).await.unwrap();
                let snapshot = reader.snapshot().await.unwrap();
                let row = snapshot
                    .rows
                    .iter()
                    .find(|row| row.pid == target_pid)
                    .unwrap();
                let identity = row
                    .identity
                    .as_ref()
                    .expect("pg_read_all_stats exposes backend identity");
                let error = reader
                    .signal(identity, SessionSignal::Terminate)
                    .await
                    .unwrap_err();
                let pg_error = error.downcast_ref::<tokio_postgres::Error>().unwrap();
                assert_eq!(pg_error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
                assert!(error.to_string().contains("permission"));
                assert_eq!(pid(&target).await, target_pid);
            })
            .catch_unwind(),
        )
        .await;
        let cleanup = tokio::time::timeout(
            Duration::from_secs(5),
            owner.batch_execute(&format!("DROP ROLE IF EXISTS {role}")),
        )
        .await;
        if !matches!(cleanup, Ok(Ok(()))) {
            eprintln!("Permission fixture cleanup failed: {cleanup:?}");
        }
        match work {
            Ok(Ok(())) => cleanup
                .expect("permission fixture cleanup timed out")
                .expect("permission fixture cleanup failed"),
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => panic!("permission fixture timed out"),
        }
    });
}
