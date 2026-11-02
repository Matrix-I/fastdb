use db_wire::filter::{Criterion, FilterOp, FilterOrder, FilterSpec};
use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver};
#[tokio::test]
async fn structured_filters_execute_literal_values_nulls_ranges_and_order() {
    let driver = SqliteDriver::new_memory();
    driver
        .execute_query(
            "CREATE TABLE data(id INTEGER PRIMARY KEY,name TEXT,score INTEGER,note TEXT)",
        )
        .await
        .unwrap();
    driver.execute_query("INSERT INTO data VALUES(1,'x'' OR 1=1 --',5,NULL),(2,'normal',20,'x'),(3,'NULL',30,NULL),(4,'',NULL,'')").await.unwrap();
    let columns = vec!["id".into(), "name".into(), "score".into(), "note".into()];
    let spec = FilterSpec {
        criteria: vec![
            Criterion {
                column: "name".into(),
                op: FilterOp::Equal,
                value: "x' OR 1=1 --".into(),
            },
            Criterion {
                column: "note".into(),
                op: FilterOp::IsNull,
                value: String::new(),
            },
        ],
        ..Default::default()
    };
    let sql = format!(
        "SELECT id FROM data {} LIMIT 200",
        spec.sql(DbDriverKind::Sqlite, &columns).unwrap()
    );
    let (pager, rows, _) = driver.execute_query(&sql).await.unwrap();
    assert_eq!(rows, 1);
    assert_eq!(pager.get_cell(0, 0).as_deref(), Some(b"1".as_slice()));
    let spec = FilterSpec {
        criteria: vec![
            Criterion {
                column: "score".into(),
                op: FilterOp::Less,
                value: "10".into(),
            },
            Criterion {
                column: "name".into(),
                op: FilterOp::Equal,
                value: "normal".into(),
            },
        ],
        match_all: false,
        order: Some(FilterOrder {
            column: "id".into(),
            descending: true,
        }),
    };
    let (pager, rows, _) = driver
        .execute_query(&format!(
            "SELECT id FROM data {}",
            spec.sql(DbDriverKind::Sqlite, &columns).unwrap()
        ))
        .await
        .unwrap();
    assert_eq!(rows, 2);
    assert_eq!(pager.get_cell(0, 0).as_deref(), Some(b"2".as_slice()));
    assert_eq!(pager.get_cell(1, 0).as_deref(), Some(b"1".as_slice()));
    for (value, id) in [("NULL", "3"), ("", "4")] {
        let spec = FilterSpec {
            criteria: vec![Criterion {
                column: "name".into(),
                op: FilterOp::Equal,
                value: value.into(),
            }],
            ..Default::default()
        };
        let (pager, rows, _) = driver
            .execute_query(&format!(
                "SELECT id FROM data {}",
                spec.sql(DbDriverKind::Sqlite, &columns).unwrap()
            ))
            .await
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(pager.get_cell(0, 0).as_deref(), Some(id.as_bytes()));
    }
}
