use db_wire::{
    ColumnMeta, DataType, DatabaseDriver, SqliteDriver,
    transfer::{
        batch::MAX_BATCH_ROWS,
        import::{CommitPolicy, ImportControl, ImportSettings, run_csv_import},
        mapping::{ColumnMapping, Conversion, Mapping},
    },
};
use std::{io::Cursor, sync::Arc};

async fn destination() -> Arc<SqliteDriver> {
    let driver = Arc::new(SqliteDriver::new_memory());
    driver
        .execute_schema_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    driver
}

fn settings(commit: CommitPolicy) -> ImportSettings {
    ImportSettings {
        schema: "main".into(),
        table: "items".into(),
        expected_object_id: None,
        csv: Default::default(),
        mapping: Mapping::new(
            1,
            vec![ColumnMapping {
                source: 0,
                target: ColumnMeta::new("id", DataType::Int4).with_pk(true),
                conversion: Conversion::Integer,
            }],
        )
        .unwrap(),
        commit,
        skip_invalid_values: false,
    }
}

#[test]
fn import_rolls_back_or_reports_durable_batches_on_constraint_failure() {
    db_wire::get_runtime().block_on(async {
        let mut csv = String::from("id\n");
        for id in 1..=MAX_BATCH_ROWS {
            csv.push_str(&format!("{id}\n"));
        }
        csv.push_str("1\n");
        for (policy, committed) in [
            (CommitPolicy::Atomic, 0),
            (CommitPolicy::PerBatch, MAX_BATCH_ROWS as u64),
        ] {
            let driver = destination().await;
            let outcome = run_csv_import(
                driver.clone(),
                Cursor::new(csv.clone()),
                settings(policy),
                Arc::default(),
            )
            .await;
            assert!(outcome.error.is_some());
            assert!(outcome.cleanup_error.is_none(), "{outcome:?}");
            assert!(!outcome.commit_unknown);
            assert_eq!(outcome.progress.committed, committed);
            let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
            assert_eq!(rows, committed);
        }
    });
}

#[test]
fn import_can_skip_conversion_errors_but_rolls_back_on_invalid_csv() {
    db_wire::get_runtime().block_on(async {
        let driver = destination().await;
        let mut options = settings(CommitPolicy::Atomic);
        options.skip_invalid_values = true;
        let control = Arc::<ImportControl>::default();
        let progress = control.progress();
        let outcome = run_csv_import(
            driver.clone(),
            Cursor::new(b"id\n1\ninvalid\n3\n"),
            options,
            control,
        )
        .await;
        assert!(outcome.error.is_none(), "{outcome:?}");
        assert!(outcome.cleanup_error.is_none(), "{outcome:?}");
        assert_eq!(outcome.progress, *progress.borrow());
        assert_eq!(outcome.progress.rejected, 1);
        assert_eq!(outcome.progress.processed, 3);
        assert_eq!(outcome.progress.committed, 2);
        let (pager, rows, _) = driver
            .execute_query("SELECT id FROM items ORDER BY id")
            .await
            .unwrap();
        assert_eq!(rows, 2);
        assert_eq!(pager.get_cell(1, 0).as_deref(), Some(b"3".as_slice()));

        let mut options = settings(CommitPolicy::Atomic);
        options.skip_invalid_values = true;
        let outcome = run_csv_import(
            driver.clone(),
            Cursor::new(b"id\n4\n\"unterminated"),
            options,
            Arc::default(),
        )
        .await;
        assert!(outcome.error.unwrap().to_string().contains("CSV record 3"));
        let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(rows, 2);
    });
}

#[test]
fn import_enforces_foreign_keys_and_rejects_views() {
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(SqliteDriver::new_memory());
        driver
            .execute_schema_ddl(
                "CREATE TABLE parents (id INTEGER PRIMARY KEY);
             CREATE TABLE items (id INTEGER REFERENCES parents(id));
             CREATE VIEW item_view AS SELECT id FROM items;
             CREATE TRIGGER view_insert INSTEAD OF INSERT ON item_view
             BEGIN INSERT INTO items VALUES (NEW.id); END",
            )
            .await
            .unwrap();
        for table in ["items", "item_view"] {
            let mut options = settings(CommitPolicy::Atomic);
            options.table = table.into();
            options.mapping = Mapping::new(
                1,
                vec![ColumnMapping {
                    source: 0,
                    target: ColumnMeta::new("id", DataType::Int4),
                    conversion: Conversion::Integer,
                }],
            )
            .unwrap();
            let outcome = run_csv_import(
                driver.clone(),
                Cursor::new(b"id\n1\n"),
                options,
                Arc::default(),
            )
            .await;
            let error = outcome.error.as_ref().expect("Invalid import was accepted");
            if table == "items" {
                assert!(
                    format!("{error:#}").contains("FOREIGN KEY constraint failed"),
                    "{error:#}"
                );
            }
            assert!(outcome.cleanup_error.is_none(), "{outcome:?}");
            assert_eq!(outcome.progress.committed, 0);
        }
        let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(rows, 0);
    });
}

#[test]
fn cancelled_import_does_not_read_input_or_write_destination() {
    db_wire::get_runtime().block_on(async {
        let driver = destination().await;
        let control = Arc::<ImportControl>::default();
        control.cancel();
        let outcome = run_csv_import(
            driver.clone(),
            Cursor::new(b"invalid CSV source"),
            settings(CommitPolicy::Atomic),
            control,
        )
        .await;
        assert!(outcome.cancelled);
        assert_eq!(outcome.progress.committed, 0);
        let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(rows, 0);
    });
}

#[test]
fn import_runs_from_a_caller_without_tokio() {
    let driver = db_wire::get_runtime().block_on(destination());
    assert!(tokio::runtime::Handle::try_current().is_err());
    let outcome = futures::executor::block_on(run_csv_import(
        driver.clone(),
        Cursor::new(b"id\n7\n"),
        settings(CommitPolicy::Atomic),
        Arc::default(),
    ));
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(outcome.progress.committed, 1);
    let (pager, _, _) = db_wire::get_runtime()
        .block_on(driver.execute_query("SELECT id FROM items"))
        .unwrap();
    assert_eq!(pager.get_cell(0, 0).as_deref(), Some(b"7".as_slice()));
}

#[test]
fn empty_import_still_validates_its_destination() {
    db_wire::get_runtime().block_on(async {
        let driver = destination().await;
        let mut options = settings(CommitPolicy::Atomic);
        options.table = "missing".into();
        let outcome = run_csv_import(driver, Cursor::new(b"id\n"), options, Arc::default()).await;
        assert!(outcome.error.is_some());
        assert_eq!(outcome.progress.committed, 0);
    });
}

#[test]
fn import_rejects_a_destination_type_changed_after_mapping() {
    for declaration in ["id TEXT", "id INTEGER", "id INTEGER PRIMARY KEY NOT NULL"] {
        db_wire::get_runtime().block_on(async {
            let driver = destination().await;
            let options = settings(CommitPolicy::Atomic);
            driver
                .execute_schema_ddl(&format!(
                    "DROP TABLE items; CREATE TABLE items ({declaration})"
                ))
                .await
                .unwrap();
            let outcome = run_csv_import(
                driver.clone(),
                Cursor::new(b"id\n1\n"),
                options,
                Arc::default(),
            )
            .await;
            assert!(
                outcome
                    .error
                    .unwrap()
                    .to_string()
                    .contains("Destination column changed")
            );
            let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
            assert_eq!(rows, 0);
        });
    }
}

#[test]
fn cancellation_after_progress_rolls_back_the_atomic_import() {
    db_wire::get_runtime().block_on(async {
        let driver = destination().await;
        let csv = format!(
            "id\n{}",
            (1..=10_000).map(|id| format!("{id}\n")).collect::<String>()
        );
        let control = Arc::<ImportControl>::default();
        struct CancelAtRead {
            csv: String,
            control: Arc<ImportControl>,
            progress: tokio::sync::watch::Receiver<db_wire::transfer::import::ImportProgress>,
        }
        impl AsRef<[u8]> for CancelAtRead {
            fn as_ref(&self) -> &[u8] {
                if self.progress.borrow().processed >= MAX_BATCH_ROWS as u64 {
                    self.control.cancel();
                }
                self.csv.as_bytes()
            }
        }
        let input = CancelAtRead {
            csv,
            progress: control.progress(),
            control: control.clone(),
        };
        let outcome = run_csv_import(
            driver.clone(),
            Cursor::new(input),
            settings(CommitPolicy::Atomic),
            control,
        )
        .await;
        assert!(outcome.cancelled, "{outcome:?}");
        assert!(!outcome.commit_unknown);
        assert!(outcome.cleanup_error.is_none(), "{outcome:?}");
        assert_eq!(outcome.progress.committed, 0);
        let (_, rows, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(rows, 0);
    });
}

#[cfg(unix)]
#[test]
fn import_rejects_device_sources() {
    db_wire::get_runtime().block_on(async {
        let driver = destination().await;
        let source = std::fs::File::open("/dev/null").unwrap();
        let outcome = run_csv_import(
            driver,
            source,
            settings(CommitPolicy::Atomic),
            Arc::default(),
        )
        .await;
        assert!(outcome.error.unwrap().to_string().contains("regular file"));
    });
}

#[test]
fn postgres_import_reports_atomic_rollback_and_per_batch_durability() {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(db_wire::PostgresDriver::new(db_wire::ConnectionConfig {
            host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: port.parse().unwrap(),
            user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
            password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
            database: "postgres".into(),
            use_unix_socket: false,
            ..Default::default()
        }));
        let table = format!("fastdb_import_{}", uuid::Uuid::new_v4().simple());
        driver
            .execute_schema_ddl(&format!(
                "CREATE TABLE public.{table} (id INTEGER PRIMARY KEY)"
            ))
            .await
            .unwrap();
        let result = async {
            let (identity, _, _) = driver.execute_query(&format!("SELECT 'public.{table}'::regclass::oid::text")).await?;
            let object_id = String::from_utf8(identity.get_cell(0, 0).unwrap())?;
            let metadata = driver.list_columns("public", &table).await?;
            let csv = format!(
                "id\n{}1\n",
                (1..=MAX_BATCH_ROWS)
                    .map(|id| format!("{id}\n"))
                    .collect::<String>()
            );
            for (policy, expected) in [
                (CommitPolicy::Atomic, 0),
                (CommitPolicy::PerBatch, MAX_BATCH_ROWS as u64),
            ] {
                let mut options = settings(policy);
                options.schema = "public".into();
                options.table = table.clone();
                options.expected_object_id = Some(object_id.clone());
                options.mapping = Mapping::new(
                    1,
                    vec![ColumnMapping {
                        source: 0,
                        target: metadata[0].clone(),
                        conversion: Conversion::Integer,
                    }],
                )?;
                let outcome = run_csv_import(
                    driver.clone(),
                    Cursor::new(csv.clone()),
                    options,
                    Arc::default(),
                )
                .await;
                anyhow::ensure!(
                    outcome
                        .error
                        .as_ref()
                        .is_some_and(|error| format!("{error:#}").contains("duplicate key")),
                    "{outcome:?}"
                );
                anyhow::ensure!(outcome.cleanup_error.is_none(), "{outcome:?}");
                anyhow::ensure!(outcome.progress.committed == expected, "{outcome:?}");
                let (_, rows, _) = driver
                    .execute_query(&format!("SELECT id FROM public.{table}"))
                    .await?;
                anyhow::ensure!(rows == expected);
            }
            driver
                .execute_schema_ddl(&format!("TRUNCATE public.{table}"))
                .await?;
            let mut options = settings(CommitPolicy::Atomic);
            options.schema = "public".into();
            options.table = table.clone();
                options.expected_object_id = Some(object_id.clone());
            options.mapping = Mapping::new(
                1,
                vec![ColumnMapping {
                    source: 0,
                    target: metadata[0].clone(),
                    conversion: Conversion::Integer,
                }],
            )?;
            let outcome = run_csv_import(
                driver.clone(),
                Cursor::new(b"id\n9\n"),
                options,
                Arc::default(),
            )
            .await;
            anyhow::ensure!(
                outcome.error.is_none() && outcome.cleanup_error.is_none(),
                "{outcome:?}"
            );
            anyhow::ensure!(outcome.progress.committed == 1 && !outcome.commit_unknown);
            let (pager, rows, _) = driver
                .execute_query(&format!("SELECT id FROM public.{table}"))
                .await?;
            anyhow::ensure!(rows == 1 && pager.get_cell(0, 0).as_deref() == Some(b"9".as_slice()));
            driver.execute_schema_ddl(&format!("DROP TABLE public.{table}; CREATE TABLE public.{table} (id INTEGER PRIMARY KEY)")).await?;
            let mut options = settings(CommitPolicy::Atomic);
            options.schema = "public".into();
            options.table = table.clone();
            options.expected_object_id = Some(object_id);
            options.mapping = Mapping::new(1, vec![ColumnMapping {
                source: 0, target: metadata[0].clone(), conversion: Conversion::Integer,
            }])?;
            let outcome = run_csv_import(driver.clone(), Cursor::new(b"id\n10\n"), options, Arc::default()).await;
            anyhow::ensure!(outcome.error.as_ref().is_some_and(|error| format!("{error:#}").contains("destination identity changed")), "{outcome:?}");
            let (_, rows, _) = driver.execute_query(&format!("SELECT id FROM public.{table}")).await?;
            anyhow::ensure!(rows == 0);
            Ok::<_, anyhow::Error>(())
        }
        .await;
        driver
            .execute_schema_ddl(&format!("DROP TABLE public.{table}"))
            .await
            .unwrap();
        result.unwrap();
    });
}

#[test]
fn postgres_import_rejects_namespace_replacement_between_batches() {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(db_wire::PostgresDriver::new(db_wire::ConnectionConfig {
            host: "127.0.0.1".into(),
            port: port.parse().unwrap(),
            user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
            password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
            database: "postgres".into(),
            use_unix_socket: false,
            ..Default::default()
        }));
        let schema = format!("fastdb_import_{}", uuid::Uuid::new_v4().simple());
        let renamed = format!("{schema}_old");
        driver.execute_schema_ddl(&format!(
            "CREATE SCHEMA {schema}; CREATE TABLE {schema}.items (id INTEGER PRIMARY KEY)"
        )).await.unwrap();
        let result = async {
            let (pager, _, _) = driver.execute_query(&format!("SELECT '{schema}.items'::regclass::oid::text")).await?;
            let object_id = String::from_utf8(pager.get_cell(0, 0).unwrap())?;
            let columns = driver.list_columns(&schema, "items").await?;
            let control = Arc::<ImportControl>::default();
            let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            struct RenameOnRead {
                bytes: Vec<u8>,
                driver: Arc<db_wire::PostgresDriver>,
                progress: tokio::sync::watch::Receiver<db_wire::transfer::import::ImportProgress>,
                changed: Arc<std::sync::atomic::AtomicBool>,
                sql: String,
            }
            impl AsRef<[u8]> for RenameOnRead {
                fn as_ref(&self) -> &[u8] {
                    use std::sync::atomic::Ordering;
                    if self.progress.borrow().processed >= MAX_BATCH_ROWS as u64
                        && !self.changed.swap(true, Ordering::SeqCst)
                    {
                        db_wire::get_runtime().block_on(self.driver.execute_schema_ddl(&self.sql)).unwrap();
                    }
                    &self.bytes
                }
            }
            let input = RenameOnRead {
                bytes: format!("id\n{}", (1..=MAX_BATCH_ROWS + 4).map(|id| format!("{id}\n")).collect::<String>()).into_bytes(),
                driver: driver.clone(),
                progress: control.progress(),
                changed: changed.clone(),
                sql: format!("SET statement_timeout = '2s'; ALTER SCHEMA {schema} RENAME TO {renamed}; CREATE SCHEMA {schema}; CREATE TABLE {schema}.items (id INTEGER PRIMARY KEY)"),
            };
            let mut options = settings(CommitPolicy::Atomic);
            options.schema = schema.clone();
            options.expected_object_id = Some(object_id);
            options.mapping = Mapping::new(1, vec![ColumnMapping {
                source: 0, target: columns[0].clone(), conversion: Conversion::Integer,
            }])?;
            let outcome = run_csv_import(driver.clone(), Cursor::new(input), options, control).await;
            anyhow::ensure!(changed.load(std::sync::atomic::Ordering::SeqCst));
            anyhow::ensure!(outcome.error.as_ref().is_some_and(|error| format!("{error:#}").contains("identity changed before batch")), "{outcome:?}");
            for schema in [&schema, &renamed] {
                let (_, rows, _) = driver.execute_query(&format!("SELECT id FROM {schema}.items")).await?;
                anyhow::ensure!(rows == 0);
            }
            Ok::<_, anyhow::Error>(())
        }.await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE; DROP SCHEMA IF EXISTS {renamed} CASCADE")).await.unwrap();
        result.unwrap();
    });
}

#[test]
fn mysql_import_reports_atomic_rollback_and_per_batch_durability() {
    let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_MYSQL_PORT required"
        );
        return;
    };
    db_wire::get_runtime().block_on(async {
        let database =
            std::env::var("FASTDB_TEST_MYSQL_DATABASE").unwrap_or_else(|_| "mysql".into());
        let driver = Arc::new(db_wire::MySqlDriver::new(db_wire::ConnectionConfig {
            host: std::env::var("FASTDB_TEST_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: port.parse().unwrap(),
            user: std::env::var("FASTDB_TEST_MYSQL_USER").unwrap_or_else(|_| "root".into()),
            password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
            database: database.clone(),
            use_unix_socket: false,
            ..Default::default()
        }));
        let table = format!("fastdb_import_{}", uuid::Uuid::new_v4().simple());
        driver
            .execute_schema_ddl(&format!(
                "CREATE TABLE {table} (id INT PRIMARY KEY) ENGINE=InnoDB"
            ))
            .await
            .unwrap();
        let result = async {
            let metadata = driver.list_columns(&database, &table).await?;
            let csv = format!(
                "id\n{}1\n",
                (1..=MAX_BATCH_ROWS)
                    .map(|id| format!("{id}\n"))
                    .collect::<String>()
            );
            for (policy, expected) in [
                (CommitPolicy::Atomic, 0),
                (CommitPolicy::PerBatch, MAX_BATCH_ROWS as u64),
            ] {
                let mut options = settings(policy);
                options.schema = database.clone();
                options.table = table.clone();
                options.expected_object_id = None;
                options.mapping = Mapping::new(
                    1,
                    vec![ColumnMapping {
                        source: 0,
                        target: metadata[0].clone(),
                        conversion: Conversion::Integer,
                    }],
                )?;
                let outcome = run_csv_import(
                    driver.clone(),
                    Cursor::new(csv.clone()),
                    options,
                    Arc::default(),
                )
                .await;
                anyhow::ensure!(
                    outcome
                        .error
                        .as_ref()
                        .is_some_and(|error| format!("{error:#}").contains("Duplicate entry")),
                    "{outcome:?}"
                );
                anyhow::ensure!(outcome.cleanup_error.is_none(), "{outcome:?}");
                anyhow::ensure!(outcome.progress.committed == expected, "{outcome:?}");
                let (_, rows, _) = driver
                    .execute_query(&format!("SELECT id FROM {table}"))
                    .await?;
                anyhow::ensure!(rows == expected);
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        driver
            .execute_schema_ddl(&format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        result.unwrap();
    });
}

#[test]
fn table_to_table_transfer_copies_rows_with_isolation() {
    db_wire::get_runtime().block_on(async {
        let src = Arc::new(SqliteDriver::new_memory());
        let dest = Arc::new(SqliteDriver::new_memory());
        src.execute_schema_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
            .await
            .unwrap();
        dest.execute_schema_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
            .await
            .unwrap();

        for i in 1..=50 {
            src.execute_schema_ddl(&format!("INSERT INTO items VALUES ({i}, 'val_{i}')"))
                .await
                .unwrap();
        }

        let mut opts = settings(CommitPolicy::Atomic);
        opts.mapping = Mapping::new(
            2,
            vec![
                ColumnMapping {
                    source: 0,
                    target: ColumnMeta::new("id", DataType::Int4).with_pk(true),
                    conversion: Conversion::Integer,
                },
                ColumnMapping {
                    source: 1,
                    target: ColumnMeta::new("val", DataType::Text),
                    conversion: Conversion::Text,
                },
            ],
        )
        .unwrap();

        let outcome = db_wire::transfer::import::run_table_transfer(
            src.clone(),
            "SELECT id, val FROM items".into(),
            dest.clone(),
            opts,
            Arc::default(),
        )
        .await;

        assert!(outcome.error.is_none(), "{outcome:?}");
        assert_eq!(outcome.progress.committed, 50);

        let (_, rows, _) = dest
            .execute_query("SELECT id, val FROM items")
            .await
            .unwrap();
        assert_eq!(rows, 50);
    });
}

#[test]
fn full_export_streams_all_rows_to_csv_and_sql() {
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(SqliteDriver::new_memory());
        driver
            .execute_schema_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        for i in 1..=100 {
            driver
                .execute_schema_ddl(&format!("INSERT INTO items VALUES ({i}, 'name_{i}')"))
                .await
                .unwrap();
        }

        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("full_items.csv");
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (progress_tx, _) = tokio::sync::watch::channel(0);

        let settings = db_wire::transfer::export::FullExportSettings {
            format: db_wire::transfer::export::FullExportFormat::Csv,
            csv_options: db_wire::transfer::csv::CsvOptions::default(),
            target_table: "items".into(),
            dialect: db_wire::sql::SqlDialect(db_wire::DbDriverKind::Sqlite),
        };

        let count = db_wire::transfer::export::run_full_export(
            driver.clone(),
            "SELECT id, name FROM items".into(),
            &csv_path,
            settings,
            cancel_rx,
            progress_tx,
        )
        .await
        .unwrap();

        assert_eq!(count, 100);
        let content = std::fs::read_to_string(&csv_path).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 101); // 1 header + 100 rows
        assert_eq!(lines[0], "id,name");
        assert_eq!(lines[1], "1,name_1");

        let sql_path = dir.path().join("full_items.sql");
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (progress_tx, _) = tokio::sync::watch::channel(0);

        let settings = db_wire::transfer::export::FullExportSettings {
            format: db_wire::transfer::export::FullExportFormat::Sql,
            csv_options: db_wire::transfer::csv::CsvOptions::default(),
            target_table: "items".into(),
            dialect: db_wire::sql::SqlDialect(db_wire::DbDriverKind::Sqlite),
        };

        let count = db_wire::transfer::export::run_full_export(
            driver.clone(),
            "SELECT id, name FROM items".into(),
            &sql_path,
            settings,
            cancel_rx,
            progress_tx,
        )
        .await
        .unwrap();

        assert_eq!(count, 100);
        let content = std::fs::read_to_string(&sql_path).unwrap();
        assert!(content.contains(r#"INSERT INTO items ("id", "name") VALUES ("#));
    });
}

#[test]
fn full_export_reports_progress_and_cancel_keeps_previous_file() {
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(SqliteDriver::new_memory());
        driver
            .execute_schema_ddl("CREATE TABLE items (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        driver
            .execute_schema_ddl(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 30) \
                 INSERT INTO items SELECT i FROM n",
            )
            .await
            .unwrap();
        let settings = || db_wire::transfer::export::FullExportSettings {
            format: db_wire::transfer::export::FullExportFormat::Csv,
            csv_options: db_wire::transfer::csv::CsvOptions::default(),
            target_table: "items".into(),
            dialect: db_wire::sql::SqlDialect(db_wire::DbDriverKind::Sqlite),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("items.csv");
        std::fs::write(&path, "previous").unwrap();

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(true);
        let (progress_tx, _) = tokio::sync::watch::channel(0);
        let result = db_wire::transfer::export::run_full_export(
            driver.clone(),
            "SELECT id FROM items".into(),
            &path,
            settings(),
            cancel_rx,
            progress_tx,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        drop(cancel_tx);

        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (progress_tx, progress_rx) = tokio::sync::watch::channel(0);
        let count = db_wire::transfer::export::run_full_export(
            driver,
            "SELECT id FROM items".into(),
            &path,
            settings(),
            cancel_rx,
            progress_tx,
        )
        .await
        .unwrap();
        assert_eq!(count, 30);
        assert_eq!(*progress_rx.borrow(), 30);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    });
}
