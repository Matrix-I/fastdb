use std::sync::Arc;
use std::time::Duration;
use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent};

#[tokio::test]
async fn test_sqlite_driver_streaming_and_cancellation() {
    let driver = SqliteDriver::new_memory();

    // Create a table and populate test rows
    driver
        .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL);")
        .await
        .unwrap();
    driver
        .execute_query("INSERT INTO users (id, name, score) VALUES (1, 'Alice', 95.5), (2, 'Bob', 88.0), (3, 'Charlie', NULL);")
        .await
        .unwrap();

    // Test list_tables
    let tables = driver.list_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "users"));

    // Test list_columns
    let cols = driver.list_columns("main", "users").await.unwrap();
    assert_eq!(cols.len(), 3);
    assert_eq!(cols[0].name, "id");
    assert!(cols[0].is_primary_key);
    assert_eq!(cols[1].name, "name");
    assert!(!cols[1].nullable);

    // Test execute_streaming: the driver creates the pager and announces it first
    let (event_tx, event_rx) = flume::unbounded();
    driver
        .execute_streaming("SELECT id, name, score FROM users ORDER BY id ASC;", event_tx)
        .await
        .unwrap();

    let events: Vec<StreamEvent> = event_rx.drain().collect();
    let StreamEvent::SchemaReady(pager) = &events[0] else {
        panic!("first event must be SchemaReady, got {:?}", events[0]);
    };
    assert!(matches!(events.last(), Some(StreamEvent::Completed { total_rows: 3, .. })));

    assert_eq!(pager.num_cols(), 3);
    assert_eq!(pager.columns[1].name, "name");
    // Result columns carry the declared column types
    assert_eq!(pager.columns[0].data_type, mem_engine::DataType::Int4);
    assert_eq!(pager.columns[2].data_type, mem_engine::DataType::Float8);
    assert_eq!(pager.total_rows(), 3);
    assert_eq!(pager.get_cell(0, 0), Some(b"1".to_vec()));
    assert_eq!(pager.get_cell(0, 1), Some(b"Alice".to_vec()));
    assert_eq!(pager.get_cell(2, 2), None); // Charlie score is NULL
}

#[tokio::test]
async fn test_sqlite_errors_are_reported_as_event_and_result() {
    let driver = SqliteDriver::new_memory();
    let (event_tx, event_rx) = flume::unbounded();

    let result = driver.execute_streaming("SELECT * FROM missing_table", event_tx).await;
    assert!(result.is_err());
    let events: Vec<StreamEvent> = event_rx.drain().collect();
    assert!(matches!(events.as_slice(), [StreamEvent::Error(msg)] if msg.contains("missing_table")));

    match driver.execute_query("SELECT * FROM missing_table").await {
        Ok(_) => panic!("query on a missing table must fail"),
        Err(e) => assert!(e.to_string().contains("missing_table")),
    }
}

#[tokio::test]
async fn test_sqlite_long_query_can_be_cancelled() {
    let driver = Arc::new(SqliteDriver::new_memory());
    let (event_tx, event_rx) = flume::unbounded();

    let runner = Arc::clone(&driver);
    let query = tokio::spawn(async move {
        runner
            .execute_streaming(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n",
                event_tx,
            )
            .await
    });

    // Wait until the statement is running, then interrupt it
    assert!(matches!(event_rx.recv_async().await, Ok(StreamEvent::SchemaReady(_))));
    tokio::time::sleep(Duration::from_millis(50)).await;
    driver.cancel_active_query().await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), query)
        .await
        .expect("cancelled query must stop promptly")
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("interrupt"));
    assert!(event_rx.drain().any(|e| matches!(e, StreamEvent::Error(_))));
}

#[tokio::test]
async fn test_sqlite_ping_succeeds() {
    let driver = SqliteDriver::new_memory();
    driver.ping().await.expect("SQLite ping must succeed on a healthy connection");
}
