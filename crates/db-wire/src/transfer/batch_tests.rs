use super::{Batch, MAX_BATCH_ROWS, MAX_PARAMETERS};
use crate::{DbDriverKind, transfer::mapping::Value};

#[test]
fn inserts_use_bound_placeholders_and_quoted_identifiers_only() {
    let batch = Batch::new(
        "s\"`",
        "t\"`",
        vec!["id".into(), "value".into()],
        vec![
            vec![
                Value::Text("1".into()),
                Value::Text("private'); DROP TABLE users;--".into()),
            ],
            vec![Value::Text("2".into()), Value::Null],
        ],
    )
    .unwrap();
    let pg = batch.sql(DbDriverKind::Postgres).unwrap();
    assert_eq!(
        pg,
        "INSERT INTO \"s\"\"`\".\"t\"\"`\" (\"id\", \"value\") VALUES ($1, $2), ($3, $4)"
    );
    assert!(!pg.contains("private"));
    assert!(
        batch
            .sql(DbDriverKind::MySql)
            .unwrap()
            .contains("VALUES (?, ?), (?, ?)")
    );
    assert!(
        batch
            .sql(DbDriverKind::Sqlite)
            .unwrap()
            .starts_with("INSERT INTO \"s\"\"`\".")
    );
    assert_eq!(batch.row_count(), 2);
    assert_eq!(batch.parameter_count(), 4);
}

#[test]
fn batch_rejects_invalid_names_shapes_and_resource_budgets() {
    let make = |rows| Batch::new("main", "items", vec!["value".into()], rows);
    assert!(make(vec![]).is_err());
    assert!(make(vec![vec![]]).is_err());
    assert!(make(vec![vec![Value::Null]; MAX_BATCH_ROWS + 1]).is_err());
    assert!(make(vec![vec![Value::Text("x".repeat(256 * 1024 + 1))]]).is_err());
    assert!(make(vec![vec![Value::Binary(vec![0; 256 * 1024])]; 5]).is_err());
    assert!(
        Batch::new(
            "main",
            "items",
            vec!["same".into(), "same".into()],
            vec![vec![Value::Null; 2]]
        )
        .is_err()
    );
    assert!(
        Batch::new(
            "main",
            "bad\0name",
            vec!["value".into()],
            vec![vec![Value::Null]]
        )
        .is_err()
    );
    let width = 1024;
    let columns = (0..width).map(|index| format!("c{index}")).collect();
    assert!(
        Batch::new(
            "main",
            "items",
            columns,
            vec![vec![Value::Null; width]; MAX_PARAMETERS / width + 1]
        )
        .is_err()
    );
}
