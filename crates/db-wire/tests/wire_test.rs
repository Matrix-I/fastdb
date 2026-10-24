use std::sync::Arc;
use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent};
use mem_engine::QueryResultPager;

#[tokio::test]
async fn test_sqlite_driver_streaming_and_cancellation() {
    let driver = SqliteDriver::new_memory();

    // Create a table and populate test rows
    let pager_init = Arc::new(QueryResultPager::new(vec![]).unwrap());
    let (tx_dummy, _rx_dummy) = flume::unbounded();

    driver
        .execute_streaming(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL);",
            pager_init.clone(),
            tx_dummy.clone(),
        )
        .await
        .unwrap();

    driver
        .execute_streaming(
            "INSERT INTO users (id, name, score) VALUES (1, 'Alice', 95.5), (2, 'Bob', 88.0), (3, 'Charlie', NULL);",
            pager_init,
            tx_dummy,
        )
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

    // Test execute_streaming with pager and events
    let pager = Arc::new(QueryResultPager::new(cols).unwrap());
    let (event_tx, event_rx) = flume::unbounded();

    let query_fut = driver.execute_streaming(
        "SELECT id, name, score FROM users ORDER BY id ASC;",
        pager.clone(),
        event_tx,
    );

    query_fut.await.unwrap();

    assert_eq!(pager.total_rows(), 3);
    assert_eq!(pager.get_cell(0, 0), Some(b"1".to_vec()));
    assert_eq!(pager.get_cell(0, 1), Some(b"Alice".to_vec()));
    assert_eq!(pager.get_cell(2, 2), None); // Charlie score is NULL

    // Verify stream events
    let mut received_schema = false;
    let mut received_completed = false;

    while let Ok(event) = event_rx.try_recv() {
        match event {
            StreamEvent::SchemaReady(c) => {
                assert_eq!(c.len(), 3);
                received_schema = true;
            }
            StreamEvent::Completed { total_rows, .. } => {
                assert_eq!(total_rows, 3);
                received_completed = true;
            }
            _ => {}
        }
    }

    assert!(received_schema);
    assert!(received_completed);
}
