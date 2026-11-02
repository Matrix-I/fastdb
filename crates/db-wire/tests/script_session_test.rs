//! Live, isolated PostgreSQL script session integration tests.
use db_wire::{
    ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver, QueryResultPager, StreamEvent,
    script::{ScriptControl, ScriptEvent, ScriptOutcome},
};
use std::{sync::Arc, time::Duration};

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return None;
    };
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
        database: "postgres".into(),
        use_unix_socket: false,
        ..Default::default()
    })
}

async fn collect(
    driver: Arc<dyn DatabaseDriver>,
    statements: Vec<String>,
) -> (Vec<(String, Arc<QueryResultPager>)>, ScriptOutcome) {
    let (tx, rx) = flume::unbounded();
    let task = tokio::spawn(db_wire::script::run_script(
        driver,
        statements,
        Arc::new(ScriptControl::default()),
        tx,
    ));
    let mut sql = String::new();
    let mut results = Vec::new();
    let outcome = loop {
        match rx.recv_async().await.unwrap() {
            ScriptEvent::StatementStarted { sql: next } => sql = next,
            ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => {
                results.push((sql.clone(), pager))
            }
            ScriptEvent::Finished(outcome) => break outcome,
            _ => {}
        }
    };
    task.await.unwrap();
    (results, outcome)
}

fn text(p: &QueryResultPager, row: u64, col: usize) -> Option<String> {
    p.get_cell(row, col).map(|b| String::from_utf8(b).unwrap())
}

#[test]
fn script_uses_one_isolated_backend_with_temp_tables_and_session_settings() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(cfg));
        let (observer, _, _) = driver.execute_query("SELECT pg_backend_pid()").await.unwrap();
        let statements = ["SELECT pg_backend_pid()", "SET fastdb.script_test = 'scoped'", "BEGIN", "CREATE TEMP TABLE fastdb_script_temp (value text)", "INSERT INTO fastdb_script_temp VALUES ('kept')", "SET LOCAL statement_timeout = '2s'", "SELECT value, pg_backend_pid(), current_setting('fastdb.script_test'), current_setting('statement_timeout') FROM fastdb_script_temp", "COMMIT", "SELECT * FROM fastdb_script_temp"];
        let (results, outcome) = collect(driver.clone(), statements.map(String::from).to_vec()).await;
        assert!(outcome.error.is_none() && outcome.cleanup_error.is_none() && !outcome.cancelled, "{outcome:?}");
        assert_eq!(results.len(), statements.len());
        assert_ne!(text(&observer, 0, 0), text(&results[0].1, 0, 0));
        let p = &results[6].1;
        assert_eq!(text(p, 0, 0).as_deref(), Some("kept"));
        assert_eq!(text(p, 0, 1), text(&results[0].1, 0, 0));
        assert_eq!(text(p, 0, 2).as_deref(), Some("scoped"));
        assert_eq!(text(p, 0, 3).as_deref(), Some("2s"));
        let (outside, _, _) = driver.execute_query("SELECT to_regclass('pg_temp.fastdb_script_temp')::text, current_setting('fastdb.script_test', true), pg_backend_pid()").await.unwrap();
        assert_eq!(text(&outside, 0, 0), None);
        assert_eq!(text(&outside, 0, 1), None);
        assert_eq!(text(&outside, 0, 2), text(&observer, 0, 0));
        let (second, outcome) = collect(driver, vec!["SELECT to_regclass('pg_temp.fastdb_script_temp')::text, current_setting('fastdb.script_test', true)".into()]).await;
        assert!(outcome.error.is_none());
        assert_eq!(text(&second[0].1, 0, 0), None);
        assert_eq!(text(&second[0].1, 0, 1), None);
    });
}

#[test]
fn explicit_commit_rollback_and_error_cleanup_preserve_only_committed_work() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(cfg));
        let table = format!("fastdb_script_tx_{}", std::process::id());
        driver
            .execute_query(&format!("CREATE TABLE {table} (id integer PRIMARY KEY)"))
            .await
            .unwrap();
        let statements = vec![
            format!("INSERT INTO {table} VALUES (1)"),
            "BEGIN".into(),
            format!("INSERT INTO {table} VALUES (2)"),
            "COMMIT".into(),
            "BEGIN".into(),
            format!("INSERT INTO {table} VALUES (3)"),
            "ROLLBACK".into(),
            "BEGIN".into(),
            format!("INSERT INTO {table} VALUES (4)"),
            "SELECT 1/0".into(),
            format!("INSERT INTO {table} VALUES (5)"),
            "COMMIT".into(),
        ];
        let (_, outcome) = collect(driver.clone(), statements).await;
        assert!(outcome.error.as_ref().unwrap().contains("division by zero"));
        assert!(outcome.cleanup_error.is_none(), "{outcome:?}");
        let (p, count, _) = driver
            .execute_query(&format!("SELECT id FROM {table} ORDER BY id"))
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(text(&p, 0, 0).as_deref(), Some("1"));
        assert_eq!(text(&p, 1, 0).as_deref(), Some("2"));
        // A direct caller leaving a transaction open is still cleaned up by finish.
        let (_, outcome) = collect(
            driver.clone(),
            vec!["BEGIN".into(), format!("INSERT INTO {table} VALUES (9)")],
        )
        .await;
        assert!(outcome.error.is_none() && outcome.cleanup_error.is_none());
        let (p, _, _) = driver
            .execute_query(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap();
        assert_eq!(text(&p, 0, 0).as_deref(), Some("2"));
        driver
            .execute_query(&format!("DROP TABLE {table}"))
            .await
            .unwrap();
    });
}

#[test]
fn cancellation_and_connection_loss_stop_the_run_without_reconnect_or_replay() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(cfg.clone()));
        let observer = PostgresDriver::new(cfg);
        for terminate in [false, true] {
            let table = format!("fastdb_script_stop_{}_{}", std::process::id(), u8::from(terminate));
            let name = table.clone();
            driver.execute_query(&format!("CREATE TABLE {table} (id integer PRIMARY KEY)")).await.unwrap();
            let (tx, rx) = flume::unbounded();
            let control = Arc::new(ScriptControl::default());
            let task = tokio::spawn(db_wire::script::run_script(driver.clone(), vec![format!("SET application_name = '{name}'"), "BEGIN".into(), format!("INSERT INTO {table} VALUES (1)"), "SELECT pg_sleep(30)".into(), format!("INSERT INTO {table} VALUES (2)"), "COMMIT".into()], control.clone(), tx));
            let pid = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let (p, n, _) = observer.execute_query(&format!("SELECT pid FROM pg_stat_activity WHERE application_name = '{name}' AND state = 'active' AND query = 'SELECT pg_sleep(30)'" )).await.unwrap();
                    if n == 1 { break text(&p, 0, 0).unwrap(); }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            // Metadata/row-edit work on the shared driver stays outside the script tx.
            driver.apply_row_changes(&[format!("INSERT INTO {table} VALUES (9)")]).await.unwrap();
            if terminate {
                observer.execute_query(&format!("SELECT pg_terminate_backend({pid})")).await.unwrap();
            } else { control.request_cancel(); }
            let outcome = tokio::time::timeout(Duration::from_secs(12), async {
                loop { if let ScriptEvent::Finished(outcome) = rx.recv_async().await.unwrap() { break outcome; } }
            }).await.unwrap();
            task.await.unwrap();
            assert_eq!(outcome.cancelled, !terminate, "{outcome:?}");
            if terminate { assert!(outcome.error.is_some()); }
            control.wait_finished().await;
            let (p, n, _) = driver.execute_query(&format!("SELECT id FROM {table}")).await.unwrap();
            assert_eq!(n, 1);
            assert_eq!(text(&p, 0, 0).as_deref(), Some("9"));
            let (_, fresh) = collect(driver.clone(), vec!["SELECT 1".into()]).await;
            assert!(fresh.error.is_none() && fresh.cleanup_error.is_none(), "{fresh:?}");
            driver.execute_query(&format!("DROP TABLE {table}")).await.unwrap();
        }
    });
}

#[test]
fn pinned_session_socket_outlives_the_callers_runtime() {
    let Some(cfg) = config() else {
        return;
    };
    let driver = PostgresDriver::new(cfg);
    let caller = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let session = caller.block_on(driver.open_script_session()).unwrap();
    drop(caller);
    db_wire::get_runtime().block_on(async {
        let (tx, rx) = flume::unbounded();
        session.execute_streaming("SELECT 42", tx).await.unwrap();
        let pager = rx
            .drain()
            .find_map(|event| match event {
                StreamEvent::SchemaReady(pager) => Some(pager),
                _ => None,
            })
            .unwrap();
        assert_eq!(text(&pager, 0, 0).as_deref(), Some("42"));
        session.finish().await.unwrap();
    });
}

#[test]
fn persistent_tab_runs_recover_with_savepoints() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver: Arc<dyn DatabaseDriver> = Arc::new(PostgresDriver::new(cfg));
        let owner = Arc::new(db_wire::script::TabSession::default());
        for (sql, fails) in [
            ("CREATE TEMP TABLE tab_session_test (value integer)", false),
            ("BEGIN", false),
            ("INSERT INTO tab_session_test VALUES (1)", false),
            ("SAVEPOINT s", false),
            ("SELECT 1/0", true),
            ("ROLLBACK TO s", false),
            ("INSERT INTO tab_session_test VALUES (2)", false),
            ("RELEASE s", false),
            ("COMMIT", false),
            ("SELECT sum(value) FROM tab_session_test", false),
        ] {
            let (tx, rx) = flume::unbounded();
            db_wire::script::run_tab_script(
                driver.clone(),
                vec![sql.into()],
                Arc::new(ScriptControl::default()),
                tx,
                Arc::downgrade(&owner),
            )
            .await;
            let mut outcome = None;
            for event in rx.drain() {
                match event {
                    ScriptEvent::Finished(result) => outcome = Some(result),
                    ScriptEvent::Stream(StreamEvent::SchemaReady(pager))
                        if sql.starts_with("SELECT sum") =>
                    {
                        assert_eq!(text(&pager, 0, 0).as_deref(), Some("3"));
                    }
                    _ => {}
                }
            }
            assert_eq!(outcome.unwrap().error.is_some(), fails, "{sql}");
        }
        // A second SQL tab on the same saved connection has its own backend.
        let second = Arc::new(db_wire::script::TabSession::default());
        let (tx, rx) = flume::unbounded();
        db_wire::script::run_tab_script(
            driver,
            vec!["SELECT * FROM tab_session_test".into()],
            Arc::new(ScriptControl::default()),
            tx,
            Arc::downgrade(&second),
        )
        .await;
        assert!(rx.drain().any(|event| matches!(
            event,
            ScriptEvent::Finished(ScriptOutcome { error: Some(_), .. })
        )));
    });
}

#[test]
fn tab_query_plans_share_settings_temp_tables_and_uncommitted_changes() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver: Arc<dyn DatabaseDriver> = Arc::new(PostgresDriver::new(cfg));
        let owner = Arc::new(db_wire::script::TabSession::default());
        async fn execute(
            driver: Arc<dyn DatabaseDriver>,
            owner: &Arc<db_wire::script::TabSession>,
            sql: String,
        ) -> (Option<Arc<QueryResultPager>>, ScriptOutcome) {
            let (tx, rx) = flume::unbounded();
            db_wire::script::run_tab_script(
                driver,
                vec![sql],
                Arc::new(ScriptControl::default()),
                tx,
                Arc::downgrade(owner),
            )
            .await;
            let mut pager = None;
            let mut outcome = None;
            for event in rx.drain() {
                match event {
                    ScriptEvent::Stream(StreamEvent::SchemaReady(result)) => pager = Some(result),
                    ScriptEvent::Finished(result) => outcome = Some(result),
                    _ => {}
                }
            }
            (pager, outcome.unwrap())
        }
        for sql in [
            "CREATE TEMP TABLE plan_tab_test (id integer PRIMARY KEY, value integer)",
            "BEGIN",
            "INSERT INTO plan_tab_test VALUES (1, 10)",
            "SET LOCAL enable_seqscan = off",
            "SAVEPOINT before_plan",
        ] {
            let (_, outcome) = execute(driver.clone(), &owner, sql.into()).await;
            assert!(outcome.error.is_none(), "{outcome:?}");
        }
        for analyze in [false, true] {
            let sql = db_wire::query_plan::explain_sql(
                DbDriverKind::Postgres,
                "UPDATE plan_tab_test SET value = value + 1 WHERE id = 1",
                analyze,
            )
            .unwrap();
            let (pager, outcome) = execute(driver.clone(), &owner, sql).await;
            assert!(outcome.error.is_none(), "{outcome:?}");
            let json = text(&pager.unwrap(), 0, 0).unwrap();
            let plan = db_wire::query_plan::QueryPlan::parse(json.as_bytes()).unwrap();
            assert!(
                plan.nodes
                    .iter()
                    .any(|node| node.title.contains("Index Scan")),
                "{json}"
            );
        }
        let (pager, outcome) = execute(
            driver.clone(),
            &owner,
            "SELECT value FROM plan_tab_test WHERE id = 1".into(),
        )
        .await;
        assert!(outcome.error.is_none());
        // Explain did not execute; Analyze executed exactly once in this transaction.
        assert_eq!(text(&pager.unwrap(), 0, 0).as_deref(), Some("11"));

        let sql =
            db_wire::query_plan::explain_sql(DbDriverKind::Postgres, "SELECT 1 / 0", true).unwrap();
        let (_, outcome) = execute(driver.clone(), &owner, sql).await;
        assert!(outcome.error.is_some());
        let (_, outcome) = execute(driver.clone(), &owner, "ROLLBACK TO before_plan".into()).await;
        assert!(outcome.error.is_none());
        let (pager, outcome) = execute(
            driver.clone(),
            &owner,
            "SELECT value FROM plan_tab_test WHERE id = 1".into(),
        )
        .await;
        assert!(outcome.error.is_none());
        assert_eq!(text(&pager.unwrap(), 0, 0).as_deref(), Some("10"));

        let second = Arc::new(db_wire::script::TabSession::default());
        let sql = db_wire::query_plan::explain_sql(
            DbDriverKind::Postgres,
            "SELECT * FROM plan_tab_test",
            false,
        )
        .unwrap();
        let (_, outcome) = execute(driver.clone(), &second, sql).await;
        assert!(outcome.error.is_some());
        let (_, outcome) = execute(driver, &owner, "ROLLBACK".into()).await;
        assert!(outcome.error.is_none());
    });
}

#[test]
fn cancelling_tab_analyze_rolls_back_and_allows_a_fresh_session() {
    let Some(cfg) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(PostgresDriver::new(cfg.clone()));
        let observer = PostgresDriver::new(cfg);
        let owner = Arc::new(db_wire::script::TabSession::default());
        let name = format!("fastdb_tab_analyze_cancel_{}", std::process::id());
        driver.execute_query(&format!("CREATE TABLE {name} (id integer)")).await.unwrap();
        let (tx, rx) = flume::unbounded();
        db_wire::script::run_tab_script(
            driver.clone(),
            vec![format!("SET application_name = '{name}'"), "BEGIN".into(),
                format!("INSERT INTO {name} VALUES (1)")],
            Arc::new(ScriptControl::default()), tx, Arc::downgrade(&owner),
        ).await;
        assert!(rx.drain().any(|event| matches!(event,
            ScriptEvent::Finished(ScriptOutcome { error: None, cancelled: false, .. }))));

        let sql = db_wire::query_plan::explain_sql(
            DbDriverKind::Postgres, "SELECT pg_sleep(30)", true).unwrap();
        let (tx, rx) = flume::unbounded();
        let control = Arc::new(ScriptControl::default());
        let task = tokio::spawn(db_wire::script::run_tab_script(
            driver.clone(), vec![sql], control.clone(), tx, Arc::downgrade(&owner),
        ));
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (_, count, _) = observer.execute_query(&format!(
                    "SELECT pid FROM pg_stat_activity WHERE application_name = '{name}' AND state = 'active' AND query LIKE 'EXPLAIN%'"
                )).await.unwrap();
                if count == 1 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        control.request_cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                if let ScriptEvent::Finished(outcome) = rx.recv_async().await.unwrap() {
                    break outcome;
                }
            }
        }).await.unwrap();
        task.await.unwrap();
        assert!(outcome.cancelled && outcome.cleanup_error.is_none(), "{outcome:?}");
        let (_, count, _) = observer.execute_query(&format!("SELECT * FROM {name}")).await.unwrap();
        assert_eq!(count, 0);
        // A late cancel belongs to the completed run, never the next tab operation.
        control.request_cancel();
        let (tx, rx) = flume::unbounded();
        db_wire::script::run_tab_script(
            driver.clone(), vec!["SELECT 42".into()],
            Arc::new(ScriptControl::default()), tx, Arc::downgrade(&owner),
        ).await;
        assert!(rx.drain().any(|event| matches!(event,
            ScriptEvent::Finished(ScriptOutcome { error: None, cancelled: false, .. }))));
        driver.execute_query(&format!("DROP TABLE {name}")).await.unwrap();
    });
}
