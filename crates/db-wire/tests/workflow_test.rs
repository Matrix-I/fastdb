//! Automated data-path acceptance; desktop interaction is intentionally separate.
use db_wire::export::{export_to_csv, export_to_sql_inserts_for_target};
use db_wire::sql::{RowChanges, SqlDialect};
use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver, StreamEvent};

#[tokio::test]
async fn sqlite_browse_edit_failure_recovery_and_export_readback() {
    let source = SqliteDriver::new_memory();
    let other = SqliteDriver::new_memory();
    for driver in [&source, &other] {
        driver
            .execute_query("CREATE TABLE sample(id INTEGER PRIMARY KEY, value TEXT)")
            .await
            .unwrap();
    }
    source
        .execute_query("INSERT INTO sample VALUES (1,'before'),(2,NULL),(3,'')")
        .await
        .unwrap();
    other
        .execute_query("INSERT INTO sample VALUES (1,'other connection')")
        .await
        .unwrap();
    assert!(
        source
            .list_tables()
            .await
            .unwrap()
            .iter()
            .any(|t| t.name == "sample")
    );
    let columns = source.list_columns("main", "sample").await.unwrap();
    assert!(columns[0].is_primary_key);
    let (sender, receiver) = flume::unbounded();
    source
        .execute_streaming("SELECT id,value FROM sample ORDER BY id", sender)
        .await
        .unwrap();
    let events: Vec<_> = receiver.drain().collect();
    let StreamEvent::SchemaReady(original) = &events[0] else {
        panic!("missing streamed schema");
    };
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Completed { total_rows: 3, .. })
    ));
    let mut draft = RowChanges::default();
    draft
        .changes
        .insert((0, 1), Some("名,'quoted'\nline".into()));
    let planned = draft
        .plan_sql(
            "\"sample\"",
            original,
            &[0],
            SqlDialect(DbDriverKind::Sqlite),
        )
        .unwrap();
    assert_eq!(source.apply_row_changes(&planned).await.unwrap(), 1);
    let failed = vec![
        "UPDATE sample SET value='must roll back' WHERE id=1".into(),
        "UPDATE sample SET value='missing' WHERE id=99".into(),
    ];
    assert!(source.apply_row_changes(&failed).await.is_err());
    let (saved, count, _) = source
        .execute_query("SELECT id,value FROM sample ORDER BY id")
        .await
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        saved.get_cell(0, 1),
        Some("名,'quoted'\nline".as_bytes().to_vec())
    );
    assert_eq!(saved.get_cell(1, 1), None);
    assert_eq!(saved.get_cell(2, 1), Some(Vec::new()));
    let mut csv = Vec::new();
    assert_eq!(export_to_csv(&saved, &mut csv).unwrap(), 3);
    assert_eq!(
        String::from_utf8(csv).unwrap(),
        "id,value\n1,\"名,'quoted'\nline\"\n2,\n3,\n"
    );
    let destination = SqliteDriver::new_memory();
    destination
        .execute_query("CREATE TABLE sample(id INTEGER PRIMARY KEY,value TEXT)")
        .await
        .unwrap();
    let mut sql = Vec::new();
    assert_eq!(
        export_to_sql_inserts_for_target(
            "\"sample\"",
            SqlDialect(DbDriverKind::Sqlite),
            &saved,
            &mut sql,
            1,
            count
        )
        .unwrap(),
        3
    );
    for statement in String::from_utf8(sql).unwrap().split_inclusive(';') {
        if !statement.trim().is_empty() {
            destination.execute_query(statement).await.unwrap();
        }
    }
    let (restored, rows, _) = destination
        .execute_query("SELECT id,value FROM sample ORDER BY id")
        .await
        .unwrap();
    assert_eq!(rows, count);
    for row in 0..count {
        for col in 0..2 {
            assert_eq!(restored.get_cell(row, col), saved.get_cell(row, col));
        }
    }
    let (isolated, _, _) = other
        .execute_query("SELECT value FROM sample WHERE id=1")
        .await
        .unwrap();
    assert_eq!(isolated.get_cell(0, 0), Some(b"other connection".to_vec()));
}
