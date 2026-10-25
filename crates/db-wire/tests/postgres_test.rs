//! Integration tests against a live PostgreSQL server.
//! Set FASTDB_TEST_PG_PORT (and optionally FASTDB_TEST_PG_HOST / FASTDB_TEST_PG_USER) to run them.

use std::sync::Arc;
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};

fn test_config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        // CI sets FASTDB_REQUIRE_DB_TESTS so a misconfigured job fails instead of skipping
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_REQUIRE_DB_TESTS is set but FASTDB_TEST_PG_PORT is not"
        );
        return None;
    };
    let port = port.parse().expect("FASTDB_TEST_PG_PORT must be a port number");
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port,
        database: "postgres".to_string(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".to_string()),
        use_unix_socket: false,
        ..ConnectionConfig::default()
    })
}

fn cell_text(pager: &db_wire::QueryResultPager, row: u64, col: usize) -> Option<String> {
    pager.get_cell(row, col).map(|b| String::from_utf8(b).unwrap())
}

#[test]
fn test_postgres_binary_decoding_matches_server_text_output() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = PostgresDriver::new(config);
    let rt = db_wire::get_runtime();

    rt.block_on(async {
        driver.execute_query("DROP TYPE IF EXISTS fastdb_mood").await.unwrap();
        driver.execute_query("CREATE TYPE fastdb_mood AS ENUM ('happy', 'sad')").await.unwrap();
    });

    let exprs = [
        "123.45::numeric",
        "-0.000012::numeric",
        "100000000.0005::numeric",
        "0::numeric(10,2)",
        "'NaN'::numeric",
        "interval '1 year 2 mons 3 days 04:05:06.789'",
        "interval '-1 hour'",
        "'10.0.0.0/24'::cidr",
        "'08:00:2b:01:02:03'::macaddr",
        "'happy'::fastdb_mood",
        "ARRAY[1, NULL, 3]::int4[]",
        "ARRAY['b c', 'x', '']::text[]",
        "ARRAY[[1.5, 2], [3, 4]]::numeric[]",
        "'12:34:56+07'::timetz",
        "16384::oid",
        "'r'::\"char\"",
        "'{\"a\": 1}'::jsonb",
        "'infinity'::date",
        "'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid",
        "42::int8",
    ];

    for expr in exprs {
        let sql = format!("SELECT {expr}, ({expr})::text");
        let (pager, rows, _) = rt.block_on(driver.execute_query(&sql)).unwrap();
        assert_eq!(rows, 1);
        let decoded = cell_text(&pager, 0, 0);
        let server_text = cell_text(&pager, 0, 1);
        assert!(decoded.is_some(), "{expr} decoded as NULL");
        assert_eq!(decoded, server_text, "binary decoding mismatch for {expr}");
    }

    // text(inet) always appends the netmask, unlike inet's output function, so compare literally
    let (pager, _, _) = rt
        .block_on(driver.execute_query("SELECT NULL::numeric, 12.5::money, '192.168.1.10'::inet, '::1'::inet"))
        .unwrap();
    assert_eq!(cell_text(&pager, 0, 0), None);
    assert_eq!(cell_text(&pager, 0, 1).as_deref(), Some("12.50"));
    assert_eq!(cell_text(&pager, 0, 2).as_deref(), Some("192.168.1.10"));
    assert_eq!(cell_text(&pager, 0, 3).as_deref(), Some("::1"));
}

#[test]
fn test_postgres_connection_survives_short_lived_caller_runtime() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = Arc::new(PostgresDriver::new(config));

    // Connect from a throwaway runtime, as the UI used to do
    let temp_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (pager, _, _) = temp_rt.block_on(driver.execute_query("SELECT pg_backend_pid()")).unwrap();
    let first_pid = cell_text(&pager, 0, 0).unwrap();
    drop(temp_rt);

    // The same backend connection must still be usable from the shared runtime
    let (pager, _, _) = db_wire::get_runtime()
        .block_on(driver.execute_query("SELECT pg_backend_pid()"))
        .unwrap();
    assert_eq!(cell_text(&pager, 0, 0).unwrap(), first_pid, "connection was lost and silently reopened");
}

#[test]
fn test_postgres_streams_rows_incrementally() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = PostgresDriver::new(config);
    let (event_tx, event_rx) = flume::unbounded();

    db_wire::get_runtime()
        .block_on(driver.execute_streaming(
            "SELECT g AS id, repeat('x', 100) AS payload FROM generate_series(1, 20000) g",
            event_tx,
        ))
        .unwrap();

    let events: Vec<db_wire::StreamEvent> = event_rx.drain().collect();
    let db_wire::StreamEvent::SchemaReady(pager) = &events[0] else {
        panic!("first event must be SchemaReady, got {:?}", events[0]);
    };
    let appended: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            db_wire::StreamEvent::PageAppended { total_rows } => Some(*total_rows),
            _ => None,
        })
        .collect();
    assert!(appended.len() > 1, "rows must arrive in several batches, got {appended:?}");
    assert!(appended.windows(2).all(|w| w[0] < w[1]));
    assert!(matches!(events.last(), Some(db_wire::StreamEvent::Completed { total_rows: 20000, .. })));
    assert_eq!(cell_text(pager, 19_999, 0).as_deref(), Some("20000"));
}

#[test]
fn test_postgres_running_query_can_be_cancelled() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = Arc::new(PostgresDriver::new(config));
    let rt = db_wire::get_runtime();
    let (event_tx, event_rx) = flume::unbounded();

    let runner = Arc::clone(&driver);
    let query = rt.spawn(async move { runner.execute_streaming("SELECT pg_sleep(30)", event_tx).await });

    rt.block_on(async {
        assert!(matches!(event_rx.recv_async().await, Ok(db_wire::StreamEvent::SchemaReady(_))));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        driver.cancel_active_query().await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), query)
            .await
            .expect("cancelled query must stop promptly")
            .unwrap();
        let err = result.unwrap_err().to_string();
        assert!(err.contains("canceling statement"), "unexpected error: {err}");
    });
}
