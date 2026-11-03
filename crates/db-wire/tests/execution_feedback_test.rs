//! Real SQLite execution and native feedback, without external services.
use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent};

fn run(driver: &SqliteDriver, sql: &str) -> Vec<StreamEvent> {
    let (tx, rx) = flume::unbounded();
    db_wire::get_runtime()
        .block_on(driver.execute_streaming(sql, tx))
        .unwrap();
    rx.drain().collect()
}

#[test]
fn sqlite_reports_native_counts_separately_from_result_rows() {
    let driver = SqliteDriver::new_memory();
    run(&driver, "CREATE TABLE items (id INTEGER PRIMARY KEY)");
    let inserted = run(&driver, "INSERT INTO items VALUES (1), (2), (3)");
    assert!(inserted.iter().any(|e| matches!(
        e,
        StreamEvent::CommandComplete {
            affected_rows: Some(3)
        }
    )));
    assert!(matches!(
        inserted.last(),
        Some(StreamEvent::Completed { total_rows: 0, .. })
    ));
    let selected = run(&driver, "SELECT id FROM items ORDER BY id");
    assert!(
        !selected
            .iter()
            .any(|e| matches!(e, StreamEvent::CommandComplete { .. }))
    );
    assert!(matches!(
        selected.last(),
        Some(StreamEvent::Completed { total_rows: 3, .. })
    ));
    let empty_update = run(&driver, "UPDATE items SET id = 9 WHERE id = 10");
    assert!(empty_update.iter().any(|e| matches!(
        e,
        StreamEvent::CommandComplete {
            affected_rows: Some(0)
        }
    )));
    let ddl = run(&driver, "CREATE TABLE other (id INTEGER)");
    assert!(ddl.iter().any(|e| matches!(
        e,
        StreamEvent::CommandComplete {
            affected_rows: Some(0)
        }
    )));
    let deleted = run(&driver, "DELETE FROM items WHERE id = 2");
    assert!(deleted.iter().any(|e| matches!(
        e,
        StreamEvent::CommandComplete {
            affected_rows: Some(1)
        }
    )));
    let (pager, rows, _) = db_wire::get_runtime()
        .block_on(driver.execute_query("SELECT id FROM items ORDER BY id"))
        .unwrap();
    assert_eq!(rows, 2);
    assert_eq!(pager.get_cell(1, 0).unwrap(), b"3");
}

#[test]
fn ordinary_deadline_wrapper_preserves_errors_and_real_rows() {
    db_wire::get_runtime().block_on(async {
        let driver = std::sync::Arc::new(SqliteDriver::new_memory());
        for (sql, succeeds) in [("SELECT 42", true), ("SELECT FROM", false)] {
            let (tx, rx) = flume::unbounded();
            db_wire::execution::execute_with_timeout(driver.clone(), sql.into(), tx, None).await;
            let events: Vec<_> = rx.drain().collect();
            assert_eq!(events.iter().filter(|e| matches!(e, StreamEvent::Completed { .. } | StreamEvent::Error(_))).count(), 1);
            assert_eq!(matches!(events.last(), Some(StreamEvent::Completed { total_rows: 1, .. })), succeeds);
            if succeeds {
                let StreamEvent::SchemaReady(pager) = &events[0] else { panic!("missing pager") };
                assert_eq!(pager.get_cell(0, 0).unwrap(), b"42");
            } else { assert!(matches!(events.last(), Some(StreamEvent::Error(message)) if message.contains("SQLite prepare error"))); }
        }
    });
}

fn live_config(prefix: &str, kind: db_wire::DbDriverKind) -> Option<db_wire::ConnectionConfig> {
    let env = |key: &str| std::env::var(format!("FASTDB_TEST_{prefix}_{key}"));
    let Ok(port) = env("PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_{prefix}_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_{prefix}_PORT not set");
        return None;
    };
    Some(db_wire::ConnectionConfig {
        driver: kind,
        port: port.parse().unwrap(),
        use_unix_socket: false,
        host: env("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        user: env("USER")
            .unwrap_or_else(|_| if prefix == "PG" { "postgres" } else { "root" }.into()),
        database: env("DATABASE").unwrap_or_else(|_| {
            if prefix == "PG" {
                "postgres"
            } else {
                "fastdb_test"
            }
            .into()
        }),
        password: env("PASSWORD").ok(),
        ..Default::default()
    })
}

#[test]
fn postgres_native_notices_counts_positions_and_prior_outcomes() {
    let Some(config) = live_config("PG", db_wire::DbDriverKind::Postgres) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = std::sync::Arc::new(db_wire::PostgresDriver::new(config));
        let (tx, rx) = flume::unbounded();
        db_wire::script::run_script(driver, vec![
            "CREATE TEMP TABLE p104 (id integer)".into(),
            "INSERT INTO p104 VALUES (1), (2)".into(),
            "DO $$ BEGIN RAISE NOTICE 'feedback-test'; END $$".into(),
            "SELECT id FROM p104 ORDER BY id".into(),
            "SELECT missing FROM p104".into(),
            "SELECT 999".into(),
        ], std::sync::Arc::default(), tx).await;
        let events: Vec<_> = rx.drain().collect();
        use db_wire::script::ScriptEvent;
        assert_eq!(events.iter().filter(|e| matches!(e, ScriptEvent::StatementStarted { .. })).count(), 5);
        assert!(events.iter().any(|e| matches!(e, ScriptEvent::Stream(StreamEvent::CommandComplete { affected_rows: Some(2) }))));
        assert!(events.iter().any(|e| matches!(e, ScriptEvent::Stream(StreamEvent::Notice(message)) if message.contains("feedback-test"))));
        assert!(events.iter().any(|e| matches!(e, ScriptEvent::Stream(StreamEvent::Error(message)) if message.contains("character 8"))));
        assert!(events.iter().any(|e| matches!(e, ScriptEvent::Stream(StreamEvent::Completed { total_rows: 2, .. }))));
        assert!(events.iter().any(|e| matches!(e, ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) if pager.get_cell(1, 0).as_deref() == Some(b"2".as_slice()))));
    });
}

#[test]
fn mysql_native_warning_details_and_affected_count() {
    let Some(config) = live_config("MYSQL", db_wire::DbDriverKind::MySql) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = db_wire::create_driver(config).unwrap();
        let (tx, rx) = flume::unbounded();
        driver.execute_streaming("SELECT CAST('not-a-number' AS UNSIGNED)", tx).await.unwrap();
        let events: Vec<_> = rx.drain().collect();
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Notice(message) if message.contains("server warning"))));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Notice(message) if message.contains("1292") && message.contains("not-a-number"))));
        let (tx, rx) = flume::unbounded();
        driver.execute_streaming("DO 1", tx).await.unwrap();
        assert!(rx.drain().any(|e| matches!(e, StreamEvent::CommandComplete { affected_rows: Some(0) })));
    });
}
