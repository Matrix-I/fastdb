//! Integration tests against a live MySQL server.
//! Set FASTDB_TEST_MYSQL_PORT (and optionally FASTDB_TEST_MYSQL_HOST / _USER / _PASSWORD / _DATABASE).

use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, StreamEvent};
use std::sync::Arc;
use std::time::Duration;

fn test_config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
        // CI sets FASTDB_REQUIRE_DB_TESTS so a misconfigured job fails instead of skipping
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_REQUIRE_DB_TESTS is set but FASTDB_TEST_MYSQL_PORT is not"
        );
        return None;
    };
    let port = port
        .parse()
        .expect("FASTDB_TEST_MYSQL_PORT must be a port number");
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
    pager
        .get_cell(row, col)
        .map(|b| String::from_utf8(b).unwrap())
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
             SELECT i, NULL AS missing, CAST(i * 1.5 AS DECIMAL(10, 2)) AS price FROM n",
            event_tx,
        ))
        .unwrap();

    let events: Vec<StreamEvent> = event_rx.drain().collect();
    let StreamEvent::SchemaReady(pager) = &events[0] else {
        panic!("first event must be SchemaReady, got {:?}", events[0]);
    };
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Completed {
            total_rows: 500,
            ..
        })
    ));
    assert_eq!(cell_text(pager, 499, 0).as_deref(), Some("500"));
    assert_eq!(cell_text(pager, 0, 1), None);
    assert_eq!(cell_text(pager, 1, 2).as_deref(), Some("3.00"));

    // Connections returned to the pool after the temporary runtime died must still work
    let (pager, rows, _) = db_wire::get_runtime()
        .block_on(driver.execute_query("SELECT 1"))
        .unwrap();
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

#[test]
fn mysql_foreign_keys_preserve_composite_order() {
    let Some(cfg) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let database = cfg.database.clone();
        let driver = MySqlDriver::new(cfg);
        let parent = format!("fastdb_er_parent_{}", std::process::id());
        let child = format!("fastdb_er_child_{}", std::process::id());
        driver.execute_query(&format!("CREATE TABLE {parent} (a INTEGER, b INTEGER, PRIMARY KEY(a,b))")).await.unwrap();
        driver.execute_query(&format!("CREATE TABLE {child} (x INTEGER, y INTEGER, FOREIGN KEY(x,y) REFERENCES {parent}(a,b))")).await.unwrap();
        let keys = driver.list_foreign_keys().await.unwrap().unwrap();
        let keys: Vec<_> = keys.iter().filter(|k| k.source_schema == database && k.source_table == child).collect();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].name, keys[1].name);
        assert_eq!((&*keys[0].source_column, &*keys[0].target_column), ("x", "a"));
        assert_eq!((&*keys[1].source_column, &*keys[1].target_column), ("y", "b"));
        assert!(keys.iter().all(|k| k.target_schema == database && k.target_table == parent));
        driver.execute_query(&format!("DROP TABLE {child}")).await.unwrap();
        driver.execute_query(&format!("DROP TABLE {parent}")).await.unwrap();
    });
}

#[test]
fn value_edits_preserve_binary_and_text_across_backslash_modes() {
    let Some(config) = test_config() else {
        return;
    };
    async fn execute(
        session: &dyn db_wire::script::ScriptSession,
        sql: &str,
    ) -> Option<Arc<db_wire::QueryResultPager>> {
        let (tx, rx) = flume::unbounded();
        tokio::time::timeout(Duration::from_secs(15), session.execute_streaming(sql, tx))
            .await
            .unwrap()
            .unwrap();
        rx.drain().find_map(|event| match event {
            StreamEvent::SchemaReady(pager) => Some(pager),
            _ => None,
        })
    }
    db_wire::get_runtime().block_on(async {
        let driver = MySqlDriver::new(config);
        // Pool checkout resets session state; keep mode and temporary table on one pinned session.
        let session = driver.open_script_session().await.unwrap();
        execute(session.as_ref(), "CREATE TEMPORARY TABLE value_edit_data (id INTEGER PRIMARY KEY, bytes BLOB, value TEXT)").await;
        execute(session.as_ref(), "INSERT INTO value_edit_data VALUES (1, NULL, NULL), (2, NULL, NULL)").await;
        for mode in ["", "NO_BACKSLASH_ESCAPES"] {
            execute(session.as_ref(), &format!("SET SESSION sql_mode='{mode}'")).await;
            execute(session.as_ref(), "UPDATE value_edit_data SET bytes=NULL, value=NULL").await;
            let pager = execute(session.as_ref(), "SELECT id, bytes, value FROM value_edit_data ORDER BY id").await.unwrap();
            let raw = b"\\xdead\0\xff";
            let text = "quote\\' and \\ path 名";
            let mut edits = db_wire::sql::RowChanges::default();
            edits.changes.insert((0, 1), Some(db_wire::value::hex_text(raw)));
            edits.changes.insert((0, 2), Some(text.into()));
            edits.changes.insert((1, 1), Some(db_wire::value::hex_text(b"")));
            let sql = edits.plan_sql("value_edit_data", &pager, &[0], db_wire::sql::SqlDialect(DbDriverKind::MySql)).unwrap();
            execute(session.as_ref(), "BEGIN").await;
            for statement in sql { execute(session.as_ref(), &statement).await; }
            execute(session.as_ref(), "COMMIT").await;
            let stored = execute(session.as_ref(), "SELECT id, bytes, value FROM value_edit_data ORDER BY id").await.unwrap();
            assert_eq!(stored.total_rows(), 2);
            assert_eq!(stored.get_cell(0, 1).unwrap(), raw);
            assert_eq!(stored.get_cell(0, 2).unwrap(), text.as_bytes());
            assert_eq!(stored.get_cell(1, 1).unwrap(), b"");
            assert_eq!(stored.get_cell(1, 2), None);
        }
    });
}

#[test]
fn mysql_table_save_roundtrips_exact_binary() {
    let Some(config) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = MySqlDriver::new(config);
        let table = format!("fastdb_value_{}", uuid::Uuid::new_v4().simple());
        driver
            .execute_query(&format!(
                "CREATE TABLE {table} (id INTEGER PRIMARY KEY, bytes BLOB)"
            ))
            .await
            .unwrap();
        let result: anyhow::Result<()> = async {
            driver
                .execute_query(&format!("INSERT INTO {table} VALUES (1, NULL)"))
                .await?;
            let (pager, _, _) = driver
                .execute_query(&format!("SELECT id, bytes FROM {table}"))
                .await?;
            let mut edits = db_wire::sql::RowChanges::default();
            let raw = b"\\xdead\0\xff";
            edits
                .changes
                .insert((0, 1), Some(db_wire::value::hex_text(raw)));
            let sql = edits.plan_sql(
                &table,
                &pager,
                &[0],
                db_wire::sql::SqlDialect(DbDriverKind::MySql),
            )?;
            assert_eq!(driver.apply_row_changes(&sql).await?, 1);
            let (stored, rows, _) = driver
                .execute_query(&format!("SELECT bytes FROM {table}"))
                .await?;
            assert_eq!(rows, 1);
            assert_eq!(stored.get_cell(0, 0).unwrap(), raw);
            Ok(())
        }
        .await;
        driver
            .execute_query(&format!("DROP TABLE {table}"))
            .await
            .unwrap();
        result.unwrap();
    });
}

#[test]
fn mysql_search_definitions_matches_literal_native_source() {
    use db_wire::objects::DefinitionKind;
    use futures::FutureExt;
    let Some(mut config) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_definitions_{}", std::process::id());
        let admin = MySqlDriver::new(config.clone());
        admin.execute_query(&format!("CREATE DATABASE {schema}")).await.unwrap();
        config.database = schema.clone();
        let driver = MySqlDriver::new(config);
        let result = std::panic::AssertUnwindSafe(async {
            for sql in [
                "CREATE TABLE sample (label VARCHAR(40))",
                "CREATE VIEW marker AS SELECT 'Needle_%' AS label",
                "CREATE VIEW decoy AS SELECT 'needleAB' AS label",
                "CREATE PROCEDURE source_proc() SELECT 'Needle_%' AS label",
                "CREATE TRIGGER source_trig BEFORE INSERT ON sample FOR EACH ROW SET NEW.label = 'Needle_%'",
            ] {
                driver.execute_query(sql).await.unwrap();
            }
            let (cancel, signal) = flume::bounded(1);
            let hits = driver.search_definitions(&schema, "needle_%", signal.clone()).await.unwrap().unwrap();
            assert_eq!(hits.iter().map(|hit| hit.kind).collect::<Vec<_>>(),
                       [DefinitionKind::Routine, DefinitionKind::Trigger, DefinitionKind::View]);
            assert_eq!(hits[1].object.qualifier, "sample");
            assert_eq!(hits[2].object.name, "marker");
            assert!(hits.iter().all(|hit| hit.source.as_deref().unwrap().contains("Needle_%")));
            for index in 0..52 {
                driver.execute_query(&format!("CREATE VIEW bulk_{index} AS SELECT 'bulk marker'")).await.unwrap();
            }
            let capped = driver.search_definitions(&schema, "bulk marker", signal.clone()).await.unwrap().unwrap();
            assert_eq!(capped.len(), db_wire::objects::DEFINITION_SEARCH_LIMIT + 1);
            let wide = "x".repeat(db_wire::objects::DEFINITION_SOURCE_LIMIT as usize);
            driver.execute_query(&format!("CREATE PROCEDURE wide_proc() SELECT 'wide marker {wide}'")).await.unwrap();
            let wide = driver.search_definitions(&schema, "wide marker", signal.clone()).await.unwrap().unwrap();
            assert_eq!(wide.len(), 1);
            assert!(wide[0].source.is_none());
            cancel.send(()).unwrap();
            assert!(driver.search_definitions(&schema, "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
        }).catch_unwind().await;
        admin.execute_query(&format!("DROP DATABASE {schema}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}

#[test]
fn mysql_data_search_matches_literals_and_opens_captured_rows() {
    use futures::FutureExt;
    use mysql_async::prelude::Queryable;
    let Some(mut config) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_data_search_{}", std::process::id());
        let admin = MySqlDriver::new(config.clone());
        admin.execute_query(&format!("CREATE DATABASE {schema}")).await.unwrap();
        config.database = schema.clone();
        let opts = mysql_async::OptsBuilder::default()
            .ip_or_hostname(config.host.clone()).tcp_port(config.port)
            .user(Some(config.user.clone())).pass(config.password.clone())
            .db_name(Some(schema.clone())).prefer_socket(false);
        let driver = MySqlDriver::new(config);
        let result = std::panic::AssertUnwindSafe(async {
            for sql in [
                "CREATE TABLE `odd``name` (value VARCHAR(80), blank TEXT, payload BLOB)",
                "INSERT INTO `odd``name` VALUES ('Needle_% literal', NULL, X'00'), ('needleAB', '', X'6e6565646c655f25')",
                "CREATE VIEW scope_view AS SELECT value FROM `odd``name`",
            ] { driver.execute_query(sql).await.unwrap(); }
            let (cancel, signal) = flume::bounded(1);
            let hits = driver.search_table_data(&schema, "odd`name", "needle_%", signal.clone()).await.unwrap().unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].column, "value");
            driver.execute_query("DELETE FROM `odd``name`").await.unwrap();
            let row = driver.read_data_search_row(&hits[0].locator, signal.clone()).await.unwrap().unwrap();
            assert_eq!(row.columns, [("value".into(), Some("Needle_% literal".into())), ("blank".into(), None)]);
            assert_eq!(row.omitted_columns, 1);
            assert!(driver.search_table_data(&schema, "scope_view", "needle", signal.clone()).await.unwrap_err().to_string().contains("base table"));
            let columns = (0..33).map(|index| format!("c{index} TEXT")).collect::<Vec<_>>().join(", ");
            driver.execute_query(&format!("CREATE TABLE wide ({columns})")).await.unwrap();
            driver.execute_query("INSERT INTO wide (c0, c32) WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 60) SELECT REPEAT('x',600), 'bulk marker' FROM n").await.unwrap();
            let capped = driver.search_table_data(&schema, "wide", "bulk marker", signal.clone()).await.unwrap().unwrap();
            assert_eq!(capped.len(), db_wire::objects::DATA_SEARCH_LIMIT + 1);
            let preview = driver.read_data_search_row(&capped[0].locator, signal.clone()).await.unwrap().unwrap();
            assert_eq!(preview.columns.len(), 32);
            assert_eq!(preview.omitted_columns, 1);
            assert_eq!(preview.columns[0].1.as_ref().unwrap().len(), 512);
            assert_eq!(preview.columns[31], ("c32".into(), Some("bulk marker".into())));
            let mut blocker = mysql_async::Conn::new(opts).await.unwrap();
            blocker.query_drop("LOCK TABLES wide WRITE").await.unwrap();
            let (send_cancel, active_signal) = flume::bounded(1);
            let search = driver.search_table_data(&schema, "wide", "bulk marker", active_signal);
            let monitor = async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let (processes, _, _) = admin.execute_query(
                            "SHOW FULL PROCESSLIST"
                        ).await.unwrap();
                        if (0..processes.total_rows()).any(|row| {
                            cell_text(&processes, row, 3).as_deref() == Some(&schema)
                                && cell_text(&processes, row, 6).is_some_and(|state| state.contains("metadata lock"))
                        }) { break; }
                    }
                }).await.expect("search must reach the metadata lock");
                send_cancel.send_async(()).await.unwrap();
            };
            let (outcome, ()) = tokio::join!(search, monitor);
            blocker.query_drop("UNLOCK TABLES").await.unwrap();
            blocker.disconnect().await.unwrap();
            assert_eq!(outcome.unwrap_err().to_string(), "Data search cancelled");
            assert_eq!(driver.search_table_data(&schema, "wide", "bulk marker", signal.clone()).await.unwrap().unwrap().len(), 51);
            cancel.send(()).unwrap();
            assert!(driver.search_table_data(&schema, "odd`name", "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
        }).catch_unwind().await;
        admin.execute_query(&format!("DROP DATABASE {schema}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}

#[test]
fn mysql_relation_ddl_preserves_quoted_table_and_native_view_source() {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_ddl_{}", uuid::Uuid::new_v4().simple());
        let admin = MySqlDriver::new(config.clone());
        admin.execute_query(&format!("CREATE DATABASE {schema}")).await.unwrap();
        let driver = MySqlDriver::new(ConnectionConfig { database: schema.clone(), ..config });
        let result = std::panic::AssertUnwindSafe(async {
            driver.execute_query("CREATE TABLE `order``s` (id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY, label VARCHAR(40) COLLATE utf8mb4_bin DEFAULT '' COMMENT 'native label', computed INT AS (length(label)) STORED, amount DECIMAL(24,8) DEFAULT 1.25, KEY lookup (label))").await.unwrap();
            let columns = driver.list_columns(&schema, "order`s").await.unwrap();
            assert_eq!(columns[0].raw_type.as_deref(), Some("int unsigned"));
            assert!(columns[0].generates_key_on_null);
            assert!(!columns[1].generates_key_on_null);
            assert_eq!(columns[1].raw_type.as_deref(), Some("varchar(40)"));
            assert_eq!(columns[1].default_val.as_deref(), Some(""));
            assert_eq!(columns[1].collation.as_deref(), Some("utf8mb4_bin"));
            assert_eq!(columns[1].comment.as_deref(), Some("native label"));
            assert!(columns[2].is_generated);
            assert_eq!(columns[3].raw_type.as_deref(), Some("decimal(24,8)"));
            assert_eq!(columns[3].default_val.as_deref(), Some("1.25000000"));
            let all = driver.list_all_columns().await.unwrap();
            let (_, _, bulk) = all.iter().find(|(s, t, _)| s == &schema && t == "order`s").unwrap();
            assert_eq!(bulk[0].raw_type, columns[0].raw_type);
            assert_eq!(bulk[1].comment, columns[1].comment);
            assert!(bulk[2].is_generated);
            driver.execute_query("CREATE VIEW named_view AS SELECT id FROM `order``s`").await.unwrap();
            let table = driver.load_relation_ddl(&schema, "order`s").await.unwrap().unwrap();
            let view = driver.load_relation_ddl(&schema, "named_view").await.unwrap().unwrap();
            assert!(table.contains("`order``s`"));
            assert!(table.contains("GENERATED ALWAYS AS"));
            assert!(table.contains("KEY `lookup`"));
            assert!(view.contains("VIEW `named_view`"));
            assert!(view.contains("`order``s`"));
        }).catch_unwind().await;
        admin.execute_query(&format!("DROP DATABASE {schema}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}
