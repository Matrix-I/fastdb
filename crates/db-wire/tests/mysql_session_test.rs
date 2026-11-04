use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, StreamEvent};

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_MYSQL_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_MYSQL_PORT not set");
        return None;
    };
    Some(ConnectionConfig {
        driver: DbDriverKind::MySql,
        host: std::env::var("FASTDB_TEST_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        user: std::env::var("FASTDB_TEST_MYSQL_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
        database: std::env::var("FASTDB_TEST_MYSQL_DATABASE").unwrap_or_else(|_| "mysql".into()),
        use_unix_socket: false,
        ..Default::default()
    })
}

async fn execute(session: &dyn db_wire::script::ScriptSession, sql: &str) -> Vec<StreamEvent> {
    let (tx, rx) = flume::unbounded();
    session.execute_streaming(sql, tx).await.unwrap();
    rx.drain().collect()
}

fn cell(events: &[StreamEvent]) -> Vec<u8> {
    events
        .iter()
        .find_map(|event| match event {
            StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
            _ => None,
        })
        .expect("one result cell")
}

#[test]
fn mysql_session_keeps_temp_objects_settings_and_savepoints_without_pool_leakage() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = MySqlDriver::new(config);
        assert!(driver.supports_script_session());
        let session = driver.open_script_session().await.unwrap();
        let identity = cell(&execute(session.as_ref(), "SELECT CONNECTION_ID()").await);
        execute(session.as_ref(), "SET @fastdb_session = 42").await;
        execute(
            session.as_ref(),
            "CREATE TEMPORARY TABLE fastdb_session_items (id INTEGER PRIMARY KEY) ENGINE=InnoDB",
        )
        .await;
        execute(session.as_ref(), "BEGIN").await;
        assert_eq!(session.in_transaction(), Some(true));
        execute(
            session.as_ref(),
            "INSERT INTO fastdb_session_items VALUES (1)",
        )
        .await;
        execute(session.as_ref(), "SAVEPOINT saved").await;
        execute(
            session.as_ref(),
            "INSERT INTO fastdb_session_items VALUES (2)",
        )
        .await;
        execute(session.as_ref(), "ROLLBACK TO saved").await;
        execute(session.as_ref(), "COMMIT").await;
        assert_eq!(session.in_transaction(), Some(false));
        assert_eq!(
            cell(&execute(session.as_ref(), "SELECT sum(id) FROM fastdb_session_items").await),
            b"1"
        );
        assert_eq!(
            cell(&execute(session.as_ref(), "SELECT @fastdb_session").await),
            b"42"
        );
        assert_eq!(
            cell(&execute(session.as_ref(), "SELECT CONNECTION_ID()").await),
            identity
        );
        let (observer, _, _) = driver
            .execute_query("SELECT CONNECTION_ID(), @fastdb_session")
            .await
            .unwrap();
        assert_ne!(observer.get_cell(0, 0).unwrap(), identity);
        assert_eq!(observer.get_cell(0, 1), None);
        assert!(
            driver
                .execute_query("SELECT * FROM fastdb_session_items")
                .await
                .is_err()
        );
        session.finish().await.unwrap();
        let (tx, _rx) = flume::unbounded();
        assert!(session.execute_streaming("SELECT 1", tx).await.is_err());
    });
}

#[test]
fn mysql_session_survives_the_callers_runtime() {
    let Some(config) = config() else {
        return;
    };
    let driver = MySqlDriver::new(config);
    let caller = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let session = caller.block_on(driver.open_script_session()).unwrap();
    drop(caller);
    db_wire::get_runtime().block_on(async {
        assert_eq!(cell(&execute(session.as_ref(), "SELECT 42").await), b"42");
        session.finish().await.unwrap();
    });
}

#[test]
fn mysql_session_cancel_stops_the_actual_server_query_and_rolls_back() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let observer = MySqlDriver::new(config);
        let table = format!("fastdb_session_{}", uuid::Uuid::new_v4().simple());
        observer.execute_query(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY) ENGINE=InnoDB")).await.unwrap();
        let session = observer.open_script_session().await.unwrap();
        let id = String::from_utf8(cell(&execute(session.as_ref(), "SELECT CONNECTION_ID()").await)).unwrap().parse::<u32>().unwrap();
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), &format!("INSERT INTO {table} VALUES (1)")).await;
        let worker_session = session.clone();
        let (tx, _rx) = flume::unbounded();
        let worker = tokio::spawn(async move { worker_session.execute_streaming("SELECT SLEEP(30)", tx).await });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let (_, count, _) = observer.execute_query(&format!("SELECT ID FROM information_schema.PROCESSLIST WHERE ID = {id} AND INFO = 'SELECT SLEEP(30)'")).await.unwrap();
                if count == 1 { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("server must acknowledge the active query");
        session.cancel_active_query().await.unwrap();
        assert!(worker.await.unwrap().is_err());
        let (pager, _, _) = observer.execute_query(&format!("SELECT count(*) FROM {table}")).await.unwrap();
        assert_eq!(pager.get_cell(0, 0), Some(b"0".to_vec()));
        let (_, count, _) = observer.execute_query(&format!("SELECT ID FROM information_schema.PROCESSLIST WHERE ID = {id}")).await.unwrap();
        assert_eq!(count, 0);
        observer.execute_query(&format!("DROP TABLE {table}")).await.unwrap();
        session.finish().await.unwrap();
    });
}

#[test]
fn mysql_native_state_covers_error_recovery_implicit_commit_and_connection_loss() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let observer = MySqlDriver::new(config);
        let table = format!("fastdb_session_{}", uuid::Uuid::new_v4().simple());
        let other = format!("{table}_ddl");
        observer
            .execute_query(&format!(
                "CREATE TABLE {table} (id INTEGER PRIMARY KEY) ENGINE=InnoDB"
            ))
            .await
            .unwrap();
        let session = observer.open_script_session().await.unwrap();
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), &format!("INSERT INTO {table} VALUES (1)")).await;
        let (tx, _rx) = flume::unbounded();
        assert!(
            session
                .execute_streaming(&format!("INSERT INTO {table} VALUES (1)"), tx)
                .await
                .unwrap_err()
                .to_string()
                .contains("Duplicate")
        );
        assert_eq!(session.in_transaction(), Some(true));
        execute(
            session.as_ref(),
            &format!("CREATE TABLE {other} (id INTEGER) ENGINE=InnoDB"),
        )
        .await;
        assert_eq!(session.in_transaction(), Some(false));
        execute(session.as_ref(), "ROLLBACK").await;
        let (pager, _, _) = observer
            .execute_query(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap();
        assert_eq!(pager.get_cell(0, 0), Some(b"1".to_vec()));
        let id = String::from_utf8(cell(
            &execute(session.as_ref(), "SELECT CONNECTION_ID()").await,
        ))
        .unwrap()
        .parse::<u32>()
        .unwrap();
        observer
            .execute_query(&format!("KILL CONNECTION {id}"))
            .await
            .unwrap();
        for _ in 0..2 {
            let (tx, _rx) = flume::unbounded();
            assert!(
                session
                    .execute_streaming(&format!("INSERT INTO {table} VALUES (2)"), tx)
                    .await
                    .is_err()
            );
        }
        assert!(session.is_closed());
        let (pager, _, _) = observer
            .execute_query(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap();
        assert_eq!(pager.get_cell(0, 0), Some(b"1".to_vec()));
        observer
            .execute_query(&format!("DROP TABLE {other}, {table}"))
            .await
            .unwrap();
    });
}

#[test]
fn mysql_tab_runs_keep_state_and_owner_close_removes_server_session() {
    use db_wire::script::{ScriptControl, ScriptEvent, TabSession, run_tab_script};
    use std::sync::Arc;
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(MySqlDriver::new(config));
        let owner = Arc::new(TabSession::default());
        let mut id = None;
        for sql in ["SET @fastdb_tab = 42", "SELECT @fastdb_tab", "BEGIN", "SELECT CONNECTION_ID()"] {
            let (tx, rx) = flume::unbounded();
            run_tab_script(driver.clone(), vec![sql.into()], Arc::new(ScriptControl::default()), tx, Arc::downgrade(&owner)).await;
            let events: Vec<_> = rx.drain().collect();
            assert!(events.iter().any(|event| matches!(event, ScriptEvent::Finished(outcome) if outcome.error.is_none() && outcome.cleanup_error.is_none() && !outcome.cancelled)), "{events:?}");
            if sql == "SELECT @fastdb_tab" || sql == "SELECT CONNECTION_ID()" {
                let value = events.iter().find_map(|event| match event {
                    ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => pager.get_cell(0, 0),
                    _ => None,
                }).unwrap();
                if sql == "SELECT @fastdb_tab" { assert_eq!(value, b"42"); }
                else { id = Some(String::from_utf8(value).unwrap().parse::<u32>().unwrap()); }
            }
            if sql == "BEGIN" {
                assert!(events.iter().any(|event| matches!(event, ScriptEvent::TransactionState { active: true })));
            }
        }
        drop(owner);
        let id = id.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let (_, count, _) = driver.execute_query(&format!("SELECT ID FROM information_schema.PROCESSLIST WHERE ID = {id}")).await.unwrap();
                if count == 0 { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("closing a SQL tab must close its server connection");
    });
}
