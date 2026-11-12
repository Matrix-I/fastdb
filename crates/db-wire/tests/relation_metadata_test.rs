//! Native read-only relation metadata and source fidelity.
use db_wire::{DatabaseDriver, SqliteDriver};

#[tokio::test]
async fn sqlite_native_indexes_include_implicit_unique_and_without_rowid_primary_keys() {
    let driver = SqliteDriver::new_memory();
    driver
        .execute_schema_ddl(
            "CREATE TABLE rowid_only (id INTEGER PRIMARY KEY);
        CREATE TABLE native_rows (id INTEGER PRIMARY KEY, label TEXT UNIQUE);
        CREATE INDEX native_partial ON native_rows(label) WHERE label IS NOT NULL;
        CREATE TABLE compound (a TEXT, b INTEGER, PRIMARY KEY (a,b)) WITHOUT ROWID;
        CREATE VIEW native_view AS SELECT label FROM native_rows",
        )
        .await
        .unwrap();
    let indexes = driver.list_all_indexes().await.unwrap();
    assert_eq!(
        indexes,
        vec![
            (
                "main".into(),
                "compound".into(),
                "sqlite_autoindex_compound_1".into()
            ),
            ("main".into(), "native_rows".into(), "native_partial".into()),
            (
                "main".into(),
                "native_rows".into(),
                "sqlite_autoindex_native_rows_1".into()
            ),
        ]
    );
    driver
        .execute_schema_ddl("DROP VIEW native_view; DROP TABLE native_rows; DROP TABLE compound")
        .await
        .unwrap();
    assert!(
        driver.list_all_indexes().await.unwrap().is_empty(),
        "a rowid alias has no separate index object"
    );
}

#[tokio::test]
async fn sqlite_csv_import_accepts_the_drivers_native_rowid_metadata() {
    use db_wire::transfer::{
        import::{CommitPolicy, ImportSettings, run_csv_import},
        mapping::{ColumnMapping, Conversion, Mapping},
    };
    use std::{io::Cursor, sync::Arc};
    let driver = Arc::new(SqliteDriver::new_memory());
    driver
        .execute_query("CREATE TABLE native_import (id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let metadata = driver.list_columns("main", "native_import").await.unwrap();
    let mapping = Mapping::new(
        1,
        vec![ColumnMapping {
            source: 0,
            target: metadata[0].clone(),
            conversion: Conversion::Auto,
        }],
    )
    .unwrap();
    let outcome = run_csv_import(
        driver.clone(),
        Cursor::new(b"id\n1\n\\N\n"),
        ImportSettings {
            schema: "main".into(),
            table: "native_import".into(),
            expected_object_id: None,
            csv: Default::default(),
            mapping,
            commit: CommitPolicy::Atomic,
            skip_invalid_values: false,
        },
        Arc::default(),
    )
    .await;
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(outcome.progress.committed, 2);
    let (pager, rows, _) = driver
        .execute_query("SELECT id FROM native_import ORDER BY id")
        .await
        .unwrap();
    assert_eq!(rows, 2);
    assert_eq!(pager.get_cell(1, 0).as_deref(), Some(b"2".as_slice()));
}

#[tokio::test]
async fn sqlite_native_columns_work_through_read_only_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory
        .path()
        .join("metadata.sqlite")
        .to_string_lossy()
        .into_owned();
    let config = db_wire::ConnectionConfig {
        driver: db_wire::DbDriverKind::Sqlite,
        database,
        ..Default::default()
    };
    let writable = SqliteDriver::new(config.clone()).unwrap();
    writable.execute_query("CREATE TABLE native_columns (id INTEGER PRIMARY KEY, label TEXT UNIQUE DEFAULT '', generated TEXT AS (upper(label)) STORED)").await.unwrap();
    let readonly = SqliteDriver::new(db_wire::ConnectionConfig {
        read_only: true,
        ..config
    })
    .unwrap();
    let columns = readonly
        .list_columns("main", "native_columns")
        .await
        .unwrap();
    assert_eq!(columns.len(), 3);
    assert_eq!(columns[1].default_val.as_deref(), Some("''"));
    assert_eq!(columns[2].name, "generated");
    assert_eq!(
        readonly.list_all_indexes().await.unwrap(),
        vec![(
            "main".into(),
            "native_columns".into(),
            "sqlite_autoindex_native_columns_1".into()
        )]
    );
    assert!(
        readonly
            .execute_query("INSERT INTO native_columns (label) VALUES ('no')")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sqlite_primary_key_nullability_matches_real_storage_semantics() {
    let driver = SqliteDriver::new_memory();
    for (name, definition, nullable) in [
        ("rowid_alias", "(id INTEGER PRIMARY KEY)", false),
        ("text_key", "(id TEXT PRIMARY KEY)", true),
        ("descending_key", "(id INTEGER PRIMARY KEY DESC)", true),
        ("strict_key", "(id TEXT PRIMARY KEY) STRICT", false),
        (
            "without_rowid_key",
            "(id TEXT PRIMARY KEY) WITHOUT ROWID",
            false,
        ),
    ] {
        driver
            .execute_query(&format!("CREATE TABLE {name} {definition}"))
            .await
            .unwrap();
        let columns = driver.list_columns("main", name).await.unwrap();
        assert_eq!(columns[0].nullable, nullable, "{name}");
        let inserted = driver
            .execute_query(&format!("INSERT INTO {name} VALUES (NULL)"))
            .await;
        if name == "rowid_alias" {
            inserted.unwrap();
            let (_, rows, _) = driver
                .execute_query("SELECT id FROM rowid_alias WHERE id IS NULL")
                .await
                .unwrap();
            assert_eq!(rows, 0);
        } else if nullable {
            inserted.unwrap();
            let (_, rows, _) = driver
                .execute_query(&format!("SELECT id FROM {name} WHERE id IS NULL"))
                .await
                .unwrap();
            assert_eq!(rows, 1);
        } else {
            assert!(inserted.is_err(), "{name}");
        }
    }
}

#[tokio::test]
async fn sqlite_columns_keep_native_types_defaults_generated_columns_and_keys() {
    let driver = SqliteDriver::new_memory();
    driver
        .execute_query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    driver.execute_query("CREATE TABLE \"quoted\"\" 名\" (id INTEGER PRIMARY KEY, label VARCHAR(17) DEFAULT 'NULL', blank TEXT DEFAULT '', missing TEXT, parent_id INTEGER, calculated TEXT GENERATED ALWAYS AS (lower(label)) STORED, nil TEXT DEFAULT NULL, FOREIGN KEY(PARENT_ID) REFERENCES parent(id))").await.unwrap();
    let columns = driver.list_columns("main", "quoted\" 名").await.unwrap();
    assert_eq!(columns.len(), 7);
    let label = columns
        .iter()
        .find(|column| column.name == "label")
        .unwrap();
    assert_eq!(label.raw_type.as_deref(), Some("VARCHAR(17)"));
    assert_eq!(label.default_val.as_deref(), Some("'NULL'"));
    assert_eq!(
        columns
            .iter()
            .find(|column| column.name == "blank")
            .unwrap()
            .default_val
            .as_deref(),
        Some("''")
    );
    assert_eq!(
        columns
            .iter()
            .find(|column| column.name == "missing")
            .unwrap()
            .default_val,
        None
    );
    assert_eq!(
        columns
            .iter()
            .find(|column| column.name == "nil")
            .unwrap()
            .default_val
            .as_deref(),
        Some("NULL")
    );
    assert!(
        driver
            .list_columns("main", "absent")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        driver
            .list_columns("absent_schema", "quoted\" 名")
            .await
            .is_err()
    );
    assert!(
        columns
            .iter()
            .find(|column| column.name == "parent_id")
            .unwrap()
            .is_foreign_key
    );
    assert_eq!(
        columns
            .iter()
            .find(|column| column.name == "calculated")
            .unwrap()
            .raw_type
            .as_deref(),
        Some("TEXT")
    );
}

#[tokio::test]
async fn sqlite_relation_ddl_replays_table_indexes_and_triggers() {
    let source = SqliteDriver::new_memory();
    let destination = SqliteDriver::new_memory();
    let table = "order\"s";
    source.execute_schema_ddl(
        r#"CREATE TABLE "order""s" (id INTEGER PRIMARY KEY, label TEXT, calculated TEXT AS (lower(label)) STORED) STRICT;
           CREATE INDEX labels ON "order""s"(label);
           CREATE TRIGGER fill_label AFTER INSERT ON "order""s"
           WHEN new.label IS NULL BEGIN UPDATE "order""s" SET label='FILLED' WHERE id=new.id; END;"#,
    ).await.unwrap();
    let ddl = source
        .load_relation_ddl("main", table)
        .await
        .unwrap()
        .unwrap();
    destination.execute_schema_ddl(&ddl).await.unwrap();
    destination
        .execute_query(r#"INSERT INTO "order""s" (id) VALUES (1)"#)
        .await
        .unwrap();
    let (rows, count, _) = destination
        .execute_query(r#"SELECT calculated FROM "order""s" INDEXED BY labels"#)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(rows.get_cell(0, 0).as_deref(), Some(b"filled".as_slice()));
    assert!(
        source
            .load_relation_ddl("main", "missing")
            .await
            .unwrap()
            .is_none()
    );
}
