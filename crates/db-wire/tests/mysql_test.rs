//! Integration tests against a live MySQL server.
//! Set FASTDB_TEST_MYSQL_PORT (and optionally FASTDB_TEST_MYSQL_HOST / _USER / _PASSWORD / _DATABASE).

use std::sync::Arc;
use std::time::Duration;
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, StreamEvent};

fn test_config() -> Option<ConnectionConfig> {
    let port = std::env::var("FASTDB_TEST_MYSQL_PORT").ok()?.parse().ok()?;
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
    Some(ConnectionConfig {
        driver: DbDriverKind::MySql,
        host: env("FASTDB_TEST_MYSQL_HOST", "127.0.0.1"),
        port,
        database: env("FASTDB_TEST_MYSQL_DATABASE", "mysql"),
        user: env("FASTDB_TEST_MYSQL_USER", "root"),
        password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
        use_unix_socket: false,
        ..ConnectionConfig::default()
    })
}

fn cell_text(pager: &db_wire::QueryResultPager, row: u64, col: usize) -> Option<String> {
    pager.get_cell(row, col).map(|b| String::from_utf8(b).unwrap())
}

#[test]
fn test_mysql_streams_rows_and_survives_short_lived_caller_runtime() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_MYSQL_PORT not set");
        return;
    };
    let driver = MySqlDriver::new(config);

    // First use from a throwaway runtime, as the UI used to do
    let temp_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    temp_rt.block_on(driver.ping()).unwrap();
    drop(temp_rt);

    let (event_tx, event_rx) = flume::unbounded();
    db_wire::get_runtime()
        .block_on(driver.execute_streaming(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500) \
             SELECT i, NULL AS empty, CAST(i * 1.5 AS DECIMAL(10, 2)) AS price FROM n",
            event_tx,
        ))
        .unwrap();

    let events: Vec<StreamEvent> = event_rx.drain().collect();
    let StreamEvent::SchemaReady(pager) = &events[0] else {
        panic!("first event must be SchemaReady, got {:?}", events[0]);
    };
    assert!(matches!(events.last(), Some(StreamEvent::Completed { total_rows: 500, .. })));
    assert_eq!(cell_text(pager, 499, 0).as_deref(), Some("500"));
    assert_eq!(cell_text(pager, 0, 1), None);
    assert_eq!(cell_text(pager, 1, 2).as_deref(), Some("3.00"));

    // Connections returned to the pool after the temporary runtime died must still work
    let (pager, rows, _) = db_wire::get_runtime().block_on(driver.execute_query("SELECT 1")).unwrap();
    assert_eq!(rows, 1);
    assert_eq!(cell_text(&pager, 0, 0).as_deref(), Some("1"));
}

#[test]
fn test_mysql_running_query_can_be_cancelled() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_MYSQL_PORT not set");
        return;
    };
    let driver = Arc::new(MySqlDriver::new(config));
    let rt = db_wire::get_runtime();
    let (event_tx, event_rx) = flume::unbounded();

    // SLEEP() returns 1 instead of failing when killed, so use real work
    let runner = Arc::clone(&driver);
    let query = rt.spawn(async move {
        runner
            .execute_streaming(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000000000) \
                 SELECT /*+ SET_VAR(cte_max_recursion_depth = 1000000000) */ COUNT(*) FROM n",
                event_tx,
            )
            .await
    });

    rt.block_on(async {
        // MySQL sends result metadata only once the query finishes, so wait for it to be running
        tokio::time::sleep(Duration::from_millis(500)).await;
        driver.cancel_active_query().await.unwrap();

        let result = tokio::time::timeout(Duration::from_secs(10), query)
            .await
            .expect("cancelled query must stop promptly")
            .unwrap();
        let err = result.unwrap_err().to_string();
        assert!(err.contains("interrupted"), "unexpected error: {err}");
        assert!(event_rx.drain().any(|e| matches!(e, StreamEvent::Error(_))));
    });
}
