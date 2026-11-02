//! Live TLS tests. ci/test-tls.sh starts isolated servers and supplies the settings.
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, PostgresDriver};
use std::{sync::Arc, time::Duration};

fn config(driver: DbDriverKind) -> Option<ConnectionConfig> {
    let prefix = if driver == DbDriverKind::Postgres {
        "FASTDB_TLS_PG"
    } else {
        "FASTDB_TLS_MYSQL"
    };
    let Ok(port) = std::env::var(format!("{prefix}_PORT")) else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_TLS_TESTS").is_none(),
            "missing {prefix}_PORT"
        );
        eprintln!("skipped: {prefix}_PORT not set");
        return None;
    };
    Some(ConnectionConfig {
        driver,
        host: "localhost".into(),
        port: port.parse().unwrap(),
        user: if driver == DbDriverKind::Postgres {
            "postgres"
        } else {
            "root"
        }
        .into(),
        database: if driver == DbDriverKind::Postgres {
            "postgres"
        } else {
            "mysql"
        }
        .into(),
        password: (driver == DbDriverKind::MySql).then(|| "fastdb".into()),
        // TLS must override socket discovery even for localhost.
        use_unix_socket: true,
        tls_enabled: true,
        tls_ca_cert: Some(std::env::var("FASTDB_TLS_CA").expect("FASTDB_TLS_CA required")),
        ..ConnectionConfig::default()
    })
}

async fn ping(config: ConnectionConfig) -> anyhow::Result<()> {
    tokio::time::timeout(
        Duration::from_secs(10),
        db_wire::create_driver(config)?.ping(),
    )
    .await?
}

#[test]
fn postgres_tls_verifies_identity_and_cancels() {
    let Some(config) = config(DbDriverKind::Postgres) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(config.clone()));
        let (pager, rows, _) = driver.execute_query("SELECT ssl::text, pg_backend_pid()::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await.unwrap();
        assert_eq!(rows, 1);
        assert_eq!(pager.get_cell(0, 0).unwrap(), b"true");
        let backend_pid = String::from_utf8(pager.get_cell(0, 1).unwrap()).unwrap();

        let mut untrusted = config.clone();
        untrusted.tls_ca_cert = None;
        assert!(ping(untrusted).await.is_err(), "private CA must not be trusted by default");
        let mut wrong_host = config.clone();
        wrong_host.host = "127.0.0.1".into(); // test certificate has DNS:localhost only
        assert!(ping(wrong_host).await.is_err(), "hostname must be verified");

        let query_driver = driver.clone();
        let (tx, rx) = flume::unbounded();
        let query = tokio::spawn(async move { query_driver.execute_streaming("SELECT pg_sleep(30)", tx).await });
        // Wait until this backend reports that pg_sleep is executing before cancelling.
        let observer = PostgresDriver::new(config);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (p, _, _) = observer.execute_query(&format!("SELECT count(*)::text FROM pg_stat_activity WHERE pid = {backend_pid} AND query = 'SELECT pg_sleep(30)' AND state = 'active'")).await.unwrap();
                if p.get_cell(0, 0).unwrap() != b"0" { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap();
        driver.cancel_active_query().await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), query).await.unwrap().unwrap().unwrap_err();
        assert!(error.to_string().contains("cancel"), "unexpected error: {error:#}");
        assert!(rx.drain().any(|e| matches!(e, db_wire::StreamEvent::Error(_))));
        driver.ping().await.unwrap();
    });
}

#[test]
fn mysql_tls_verifies_identity() {
    let Some(config) = config(DbDriverKind::MySql) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = MySqlDriver::new(config.clone());
        let (pager, rows, _) = driver
            .execute_query("SHOW SESSION STATUS LIKE 'Ssl_cipher'")
            .await
            .unwrap();
        assert_eq!(rows, 1);
        assert!(
            !pager.get_cell(0, 1).unwrap().is_empty(),
            "session must be encrypted"
        );
        let mut untrusted = config.clone();
        untrusted.tls_ca_cert = None;
        assert!(
            ping(untrusted).await.is_err(),
            "private CA must not be trusted by default"
        );
        let mut wrong_host = config;
        wrong_host.host = "127.0.0.1".into();
        assert!(ping(wrong_host).await.is_err(), "hostname must be verified");
    });
}

#[test]
fn postgres_tls_refuses_plaintext_server() {
    let Ok(port) = std::env::var("FASTDB_TLS_PG_PLAIN_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_TLS_TESTS").is_none(),
            "missing FASTDB_TLS_PG_PLAIN_PORT"
        );
        return;
    };
    let mut config = config(DbDriverKind::Postgres).unwrap();
    config.port = port.parse().unwrap();
    db_wire::get_runtime().block_on(async {
        assert!(
            ping(config.clone()).await.is_err(),
            "must not fall back to plaintext"
        );
        config.tls_enabled = false;
        config.use_unix_socket = false;
        ping(config).await.unwrap();
    });
}

#[test]
fn postgres_pinned_script_session_verifies_tls_and_cancels_its_own_backend() {
    use db_wire::{
        StreamEvent,
        script::{ScriptControl, ScriptEvent},
    };
    let Some(config) = config(DbDriverKind::Postgres) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(config.clone()));
        let mut untrusted = config.clone();
        untrusted.tls_ca_cert = None;
        assert!(PostgresDriver::new(untrusted).open_script_session().await.is_err());
        let mut wrong_host = config.clone();
        wrong_host.host = "127.0.0.1".into();
        assert!(PostgresDriver::new(wrong_host).open_script_session().await.is_err());
        let name = format!("fastdb_tls_script_{}", std::process::id());
        let control = Arc::new(ScriptControl::default());
        let (tx, rx) = flume::unbounded();
        let task = tokio::spawn(db_wire::script::run_script(driver.clone(), vec![
            format!("SET application_name = '{name}'"),
            "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()".into(),
            "SELECT pg_sleep(30)".into(),
            "SELECT 'must not run'".into(),
        ], control.clone(), tx));
        let observer = PostgresDriver::new(config);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (p, _, _) = observer.execute_query(&format!("SELECT count(*) FROM pg_stat_activity WHERE application_name = '{name}' AND state = 'active' AND query = 'SELECT pg_sleep(30)'" )).await.unwrap();
                if p.get_cell(0, 0).unwrap() != b"0" { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        control.request_cancel();
        let mut pagers = Vec::new();
        let outcome = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                match rx.recv_async().await.unwrap() {
                    ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => pagers.push(pager),
                    ScriptEvent::Finished(outcome) => break outcome,
                    ScriptEvent::StatementStarted { sql } => assert!(!sql.contains("must not run")),
                    _ => {},
                }
            }
        }).await.unwrap();
        task.await.unwrap();
        assert!(outcome.cancelled && outcome.cleanup_error.is_none(), "{outcome:?}");
        assert!(pagers.iter().any(|p| p.num_cols() == 1 && p.get_cell(0, 0).as_deref() == Some(b"true".as_slice())));
        driver.ping().await.unwrap();
    });
}
