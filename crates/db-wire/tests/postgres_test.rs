//! Integration tests against a live PostgreSQL server.
//! Set FASTDB_TEST_PG_PORT (and optionally FASTDB_TEST_PG_HOST / FASTDB_TEST_PG_USER) to run them.

use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};
use std::sync::Arc;

// Both fixtures enumerate all visible relations; keep their DROP/CREATE phases
// separate so one test cannot remove catalogs while the other deparses them.
static CATALOG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn postgres_foreign_table_source_replays_native_options_and_partition() {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    let guard = CATALOG_TEST_LOCK.lock().unwrap();
    let outcome = db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb_fdw_{}", uuid::Uuid::new_v4().simple());
        let server = format!("\"{schema} server\"");
        driver.execute_schema_ddl(&format!(r#"
            CREATE SCHEMA {schema};
            CREATE FOREIGN DATA WRAPPER {schema};
            CREATE SERVER {server} FOREIGN DATA WRAPPER {schema};
            CREATE TABLE {schema}.parent (id integer NOT NULL) PARTITION BY RANGE (id);
            CREATE FOREIGN TABLE {schema}."remote table" (
                id integer OPTIONS ("remote name" 'x=y') NOT NULL,
                label text DEFAULT 'a',
                CONSTRAINT positive CHECK (id > 0)
            ) SERVER {server} OPTIONS ("remote table" 'a''b=c');
            CREATE FOREIGN TABLE {schema}.leaf PARTITION OF {schema}.parent
                FOR VALUES FROM (1) TO (10) SERVER {server};
            COMMENT ON FOREIGN TABLE {schema}."remote table" IS 'Foreign fixture';
        "#)).await.unwrap();
        let outcome = std::panic::AssertUnwindSafe(async {
            let details = driver.list_table_details().await.unwrap();
            for name in ["remote table", "leaf"] {
                let (_, _, details) = details.iter().find(|(s,t,_)| s == &schema && t == name).unwrap();
                let ddl = details.ddl.as_deref().unwrap();
                assert!(ddl.contains("CREATE FOREIGN TABLE"), "{ddl}");
                assert!(ddl.contains(&format!("SERVER {server}")), "{ddl}");
                if name == "remote table" {
                    assert!(ddl.contains(r#""remote table" 'a''b=c'"#));
                    assert!(ddl.contains(r#""remote name" 'x=y'"#));
                    assert!(ddl.contains("COMMENT ON FOREIGN TABLE"));
                } else {
                    assert!(ddl.contains("ATTACH PARTITION"));
                }
                let target = db_wire::sql::SqlDialect(DbDriverKind::Postgres).qualified_table(&schema, name);
                driver.execute_schema_ddl(&format!("DROP FOREIGN TABLE {target}")).await.unwrap();
                driver.execute_schema_ddl(ddl).await.unwrap();
                let restored = driver.list_table_details().await.unwrap();
                assert_eq!(
                    restored.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2.ddl.as_deref(),
                    Some(ddl)
                );
            }
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE; DROP SERVER {server}; DROP FOREIGN DATA WRAPPER {schema}")).await.unwrap();
        outcome
    });
    drop(guard);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn inherited_history_is_unavailable(setup: &str, reason: &str) {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    let catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    let outcome = db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb_inherit_history_{}", uuid::Uuid::new_v4().simple());
        driver.execute_schema_ddl(&format!("CREATE SCHEMA {schema}; {}", setup.replace("$schema", &schema))).await.unwrap();
        let outcome = std::panic::AssertUnwindSafe(async {
            let details = driver.list_table_details().await.unwrap();
            let native = &details.iter().find(|(s,t,_)| s == &schema && t == "child").unwrap().2;
            let ddl = native.ddl.as_ref().unwrap();
            if !ddl.starts_with("-- DDL unavailable:") {
                let before = driver.list_columns(&schema, "child").await.unwrap().into_iter().map(|c| c.name).collect::<Vec<_>>();
                driver.execute_schema_ddl(&format!("DROP TABLE {schema}.child")).await.unwrap();
                let replay = driver.execute_schema_ddl(ddl).await;
                let after = driver.list_columns(&schema, "child").await.unwrap_or_default().into_iter().map(|c| c.name).collect::<Vec<_>>();
                panic!("Unsafe inherited DDL was exported: {reason}; replay={replay:?}; columns before={before:?}, after={after:?}");
            }
            assert!(ddl.contains(reason), "{ddl}");
            assert!(!ddl.contains("CREATE TABLE"), "unavailable source must not include partial SQL");
            assert!(native.owner.is_some() && native.table_bytes.is_some(), "other Properties metadata remains available");
            assert!(!driver.list_columns(&schema, "child").await.unwrap().is_empty());
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
        outcome
    });
    drop(catalog_guard);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn postgres_inherited_history_default_conflict_is_unavailable() {
    inherited_history_is_unavailable(
        "CREATE TABLE $schema.p (a INTEGER DEFAULT 1); CREATE TABLE $schema.q (a INTEGER DEFAULT 1); CREATE TABLE $schema.child () INHERITS ($schema.p, $schema.q); ALTER TABLE ONLY $schema.q ALTER COLUMN a SET DEFAULT 2",
        "conflicting parent defaults",
    );
}

#[test]
fn postgres_inherited_history_local_column_without_default_is_unavailable() {
    inherited_history_is_unavailable(
        "CREATE TABLE $schema.p (a INTEGER DEFAULT 1); CREATE TABLE $schema.q (a INTEGER DEFAULT 1); CREATE TABLE $schema.child (a INTEGER) INHERITS ($schema.p, $schema.q); ALTER TABLE ONLY $schema.child ALTER COLUMN a DROP DEFAULT; ALTER TABLE ONLY $schema.q ALTER COLUMN a SET DEFAULT 2",
        "conflicting parent defaults",
    );
}

#[test]
fn postgres_inherited_history_column_order_is_unavailable() {
    inherited_history_is_unavailable(
        "CREATE TABLE $schema.p (a INTEGER); CREATE TABLE $schema.child (local_value TEXT) INHERITS ($schema.p); ALTER TABLE $schema.p ADD COLUMN late INTEGER",
        "inherited column order",
    );
}

#[test]
fn postgres_typed_and_inherited_ddl_preserves_native_column_and_constraint_ownership() {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb_inherit_{}", uuid::Uuid::new_v4().simple());
        driver.execute_schema_ddl(&format!("CREATE SCHEMA {schema}; CREATE TYPE {schema}.record_type AS (id INTEGER, label TEXT); CREATE TABLE {schema}.typed OF {schema}.record_type (id WITH OPTIONS NOT NULL, label WITH OPTIONS DEFAULT 'typed'); CREATE TABLE {schema}.parent_a (id INTEGER DEFAULT 1, label TEXT DEFAULT 'parent', CONSTRAINT positive CHECK (id > 0)); CREATE TABLE {schema}.parent_b (id INTEGER DEFAULT 1, other INTEGER DEFAULT 2); CREATE TABLE {schema}.child (id INTEGER DEFAULT 5, other INTEGER, local_value INTEGER DEFAULT 7, CONSTRAINT positive CHECK (id > 0)) INHERITS ({schema}.parent_a, {schema}.parent_b); ALTER TABLE ONLY {schema}.child ALTER COLUMN label SET DEFAULT 'child'; ALTER TABLE ONLY {schema}.child ALTER COLUMN other DROP DEFAULT; ALTER TABLE ONLY {schema}.child ALTER COLUMN other SET NOT NULL; CREATE TABLE {schema}.pure () INHERITS ({schema}.parent_a); ALTER TABLE ONLY {schema}.pure ALTER COLUMN label DROP DEFAULT; ALTER TABLE ONLY {schema}.parent_b ALTER COLUMN id SET DEFAULT 2")).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let signature = format!(r#"SELECT jsonb_agg(jsonb_build_object(
                'name', c.relname, 'type', c.reloftype::regtype::text,
                'columns', (SELECT jsonb_agg(jsonb_build_array(a.attname, a.attislocal, a.attinhcount, a.attnotnull, pg_get_expr(d.adbin, d.adrelid)) ORDER BY a.attnum) FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE a.attrelid=c.oid AND a.attnum > 0 AND NOT a.attisdropped),
                'parents', (SELECT jsonb_agg(h.inhparent::regclass::text ORDER BY h.inhseqno) FROM pg_inherits h WHERE h.inhrelid=c.oid),
                'constraints', (SELECT jsonb_agg(jsonb_build_array(k.conname, k.conislocal, k.coninhcount, k.convalidated, pg_get_constraintdef(k.oid)) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid)) ORDER BY c.relname)
                FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='{schema}' AND c.relname IN ('typed','child','pure')"#);
            let (before, _, _) = driver.execute_query(&signature).await.unwrap();
            let before = cell_text(&before, 0, 0).unwrap();
            let details = driver.list_table_details().await.unwrap();
            let mut saved = Vec::new();
            for name in ["typed", "child", "pure"] {
                let ddl = details.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2.ddl.clone().unwrap();
                assert!(!ddl.contains("unavailable"), "{name}: {ddl}");
                saved.push((name, ddl));
            }
            assert!(saved[0].1.contains(" OF "));
            assert!(saved[1].1.contains(" INHERITS ("));
            driver.execute_schema_ddl(&format!("DROP TABLE {schema}.typed, {schema}.child, {schema}.pure")).await.unwrap();
            for (_, ddl) in &saved { driver.execute_schema_ddl(ddl).await.unwrap(); }
            let (after, _, _) = driver.execute_query(&signature).await.unwrap();
            assert_eq!(cell_text(&after, 0, 0).as_deref(), Some(before.as_str()));
            let restored = driver.list_table_details().await.unwrap();
            for (name, ddl) in &saved {
                assert_eq!(restored.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2.ddl.as_ref(), Some(ddl));
            }
            driver.execute_schema_ddl(&format!("INSERT INTO {schema}.typed (id) VALUES (1); INSERT INTO {schema}.child (other) VALUES (3); INSERT INTO {schema}.pure DEFAULT VALUES")).await.unwrap();
            let (values, _, _) = driver.execute_query(&format!("SELECT id, label, local_value FROM ONLY {schema}.child")).await.unwrap();
            assert_eq!(cell_text(&values, 0, 0).as_deref(), Some("5"));
            assert_eq!(cell_text(&values, 0, 1).as_deref(), Some("child"));
            assert_eq!(cell_text(&values, 0, 2).as_deref(), Some("7"));
            let (values, _, _) = driver.execute_query(&format!("SELECT label FROM {schema}.pure")).await.unwrap();
            assert_eq!(values.get_cell(0, 0), None);
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}

#[test]
fn postgres_materialized_view_metadata_and_structural_ddl_replay() {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb_mv_{}", uuid::Uuid::new_v4().simple());
        let dialect = db_wire::sql::SqlDialect(DbDriverKind::Postgres);
        let name = "Quoted \"snapshot";
        let target = dialect.qualified_table(&schema, name);
        driver.execute_schema_ddl(&format!("CREATE SCHEMA {schema}; CREATE DOMAIN {schema}.exact_amount AS NUMERIC(18,4) NOT NULL; CREATE TABLE {schema}.source (id INTEGER, amount NUMERIC(12,2), balance {schema}.exact_amount); INSERT INTO {schema}.source VALUES (1, 1.25, 2.5000); CREATE MATERIALIZED VIEW {target} WITH (fillfactor=80) AS SELECT id, amount, balance FROM {schema}.source; CREATE UNIQUE INDEX snapshot_key ON {target} (id); COMMENT ON MATERIALIZED VIEW {target} IS 'native snapshot'; COMMENT ON COLUMN {target}.amount IS 'exact amount'; ALTER MATERIALIZED VIEW {target} ALTER COLUMN amount SET STATISTICS 42")).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            assert!(driver.list_tables().await.unwrap().iter().any(|t| t.schema == schema && t.name == name && t.table_type == "MATERIALIZED VIEW"));
            let columns = driver.list_columns(&schema, name).await.unwrap();
            assert_eq!(columns.len(), 3);
            let source_columns = driver.list_columns(&schema, "source").await.unwrap();
            assert_eq!(columns[2].data_type, source_columns[2].data_type);
            assert_eq!(columns[2].nullable, source_columns[2].nullable);
            assert_eq!(columns[2].raw_type, source_columns[2].raw_type);
            assert_eq!(columns[1].raw_type.as_deref(), Some("numeric(12,2)"));
            assert_eq!(columns[1].comment.as_deref(), Some("exact amount"));
            let all = driver.list_all_columns().await.unwrap();
            assert_eq!(&all.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2, &columns);
            let details = driver.list_table_details().await.unwrap();
            let native = &details.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2;
            assert_eq!(native.comment.as_deref(), Some("native snapshot"));
            assert!(native.table_bytes.is_some());
            assert!(native.indexes.iter().any(|i| i.name == "snapshot_key" && i.unique));
            let ddl = native.ddl.as_ref().unwrap();
            assert!(ddl.contains("CREATE MATERIALIZED VIEW"), "{ddl}");
            assert!(ddl.contains("WITH NO DATA"), "{ddl}");
            assert!(ddl.contains("SET STATISTICS 42"), "{ddl}");
            driver.execute_schema_ddl(&format!("DROP MATERIALIZED VIEW {target}")).await.unwrap();
            driver.execute_schema_ddl(ddl).await.unwrap();
            let restored = driver.list_table_details().await.unwrap();
            let restored = &restored.iter().find(|(s,t,_)| s == &schema && t == name).unwrap().2;
            assert_eq!(restored.ddl.as_ref(), Some(ddl));
            assert_eq!(restored.comment, native.comment);
            driver.execute_schema_ddl(&format!("REFRESH MATERIALIZED VIEW {target}")).await.unwrap();
            let (rows, count, _) = driver.execute_query(&format!("SELECT amount FROM {target}")).await.unwrap();
            assert_eq!(count, 1);
            assert_eq!(cell_text(&rows, 0, 0).as_deref(), Some("1.25"));
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}

#[test]
fn postgres_materialized_view_metadata_respects_relation_and_column_grants() {
    use futures::FutureExt as _;
    let Some(config) = test_config() else { return };
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    db_wire::get_runtime().block_on(async {
        let admin = PostgresDriver::new(config.clone());
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let schema = format!("fastdb_mv_acl_{suffix}");
        let role = format!("fastdb_mv_reader_{suffix}");
        admin.execute_schema_ddl(&format!("CREATE ROLE {role}; CREATE SCHEMA {schema}; CREATE MATERIALIZED VIEW {schema}.allowed AS SELECT 1 AS id, 2 AS restricted; CREATE MATERIALIZED VIEW {schema}.hidden AS SELECT 3 AS secret; GRANT USAGE ON SCHEMA {schema} TO {role}; GRANT SELECT(id) ON {schema}.allowed TO {role}")).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let reader = PostgresDriver::new(config);
            reader.execute_query(&format!("SET ROLE {role}")).await.unwrap();
            let (identity, _, _) = reader.execute_query("SELECT current_user").await.unwrap();
            assert_eq!(cell_text(&identity, 0, 0).as_deref(), Some(role.as_str()));
            let tables = reader.list_tables().await.unwrap();
            assert!(tables.iter().any(|t| t.schema == schema && t.name == "allowed"));
            assert!(!tables.iter().any(|t| t.schema == schema && t.name == "hidden"));
            let columns = reader.list_columns(&schema, "allowed").await.unwrap();
            assert_eq!(columns.len(), 1);
            assert_eq!(columns[0].name, "id");
            assert!(reader.list_columns(&schema, "hidden").await.unwrap().is_empty());
            let details = reader.list_table_details().await.unwrap();
            assert!(details.iter().any(|(s,t,_)| s == &schema && t == "allowed"));
            assert!(!details.iter().any(|(s,t,_)| s == &schema && t == "hidden"));
        }).catch_unwind().await;
        admin.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE; DROP ROLE {role}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}

fn test_config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        // CI sets FASTDB_REQUIRE_DB_TESTS so a misconfigured job fails instead of skipping
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_REQUIRE_DB_TESTS is set but FASTDB_TEST_PG_PORT is not"
        );
        return None;
    };
    let port = port
        .parse()
        .expect("FASTDB_TEST_PG_PORT must be a port number");
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port,
        database: "postgres".to_string(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".to_string()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
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
fn test_postgres_binary_decoding_matches_server_text_output() {
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = PostgresDriver::new(config);
    let rt = db_wire::get_runtime();

    rt.block_on(async {
        driver
            .execute_query("DROP TYPE IF EXISTS fastdb_mood")
            .await
            .unwrap();
        driver
            .execute_query("CREATE TYPE fastdb_mood AS ENUM ('happy', 'sad')")
            .await
            .unwrap();
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
        .block_on(
            driver.execute_query(
                "SELECT NULL::numeric, 12.5::money, '192.168.1.10'::inet, '::1'::inet",
            ),
        )
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
    let (pager, _, _) = temp_rt
        .block_on(driver.execute_query("SELECT pg_backend_pid()"))
        .unwrap();
    let first_pid = cell_text(&pager, 0, 0).unwrap();
    drop(temp_rt);

    // The same backend connection must still be usable from the shared runtime
    let (pager, _, _) = db_wire::get_runtime()
        .block_on(driver.execute_query("SELECT pg_backend_pid()"))
        .unwrap();
    assert_eq!(
        cell_text(&pager, 0, 0).unwrap(),
        first_pid,
        "connection was lost and silently reopened"
    );
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
    assert!(
        appended.len() > 1,
        "rows must arrive in several batches, got {appended:?}"
    );
    assert!(appended.windows(2).all(|w| w[0] < w[1]));
    assert!(matches!(
        events.last(),
        Some(db_wire::StreamEvent::Completed {
            total_rows: 20000,
            ..
        })
    ));
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
    let query = rt.spawn(async move {
        runner
            .execute_streaming("SELECT pg_sleep(30)", event_tx)
            .await
    });

    rt.block_on(async {
        assert!(matches!(
            event_rx.recv_async().await,
            Ok(db_wire::StreamEvent::SchemaReady(_))
        ));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        driver.cancel_active_query().await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), query)
            .await
            .expect("cancelled query must stop promptly")
            .unwrap();
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("canceling statement"),
            "unexpected error: {err}"
        );
    });
}

#[test]
fn test_postgres_estimated_and_analyzed_json_plans() {
    use db_wire::query_plan::{QueryPlan, explain_sql};
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = PostgresDriver::new(config);
    db_wire::get_runtime().block_on(async {
        for analyze in [false, true] {
            let sql = explain_sql(
                DbDriverKind::Postgres,
                "SELECT sum(n) FROM generate_series(1, 10) AS n",
                analyze,
            )
            .unwrap();
            let (pager, rows, _) = driver.execute_query(&sql).await.unwrap();
            assert_eq!(rows, 1);
            let plan = QueryPlan::parse(&pager.get_cell(0, 0).unwrap()).unwrap();
            assert!(plan.nodes.len() >= 2);
            assert_eq!(plan.nodes[0].title, "Aggregate");
            assert_eq!(plan.summary.contains("Execution Time"), analyze);
            assert_eq!(plan.nodes[0].details.contains("Actual Rows"), analyze);
        }
    });
}

#[test]
fn test_postgres_catalog_details_and_native_columns() {
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    let driver = PostgresDriver::new(config);
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb metadata {}", std::process::id());
        let dialect = db_wire::sql::SqlDialect(DbDriverKind::Postgres);
        let qschema = dialect.quote_ident(&schema);
        let child_name = "Child's \"Rows\"";
        let child = dialect.qualified_table(&schema, child_name);
        let parent = dialect.qualified_table(&schema, "parent");
        let partitioned = dialect.qualified_table(&schema, "partitioned");
        let view = dialect.qualified_table(&schema, "view");
        for sql in [
            format!("CREATE SCHEMA {qschema}"),
            format!("CREATE TABLE {parent} (a integer, b integer, PRIMARY KEY (a, b))"),
            format!("CREATE UNLOGGED TABLE {child} (a integer, b integer, label varchar(42) DEFAULT 'x', amount numeric(12,2), CONSTRAINT composite_fk FOREIGN KEY (b, a) REFERENCES {parent}(a, b) ON UPDATE CASCADE ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED)"),
            format!("ALTER TABLE {child} ADD CONSTRAINT positive CHECK (a > 0) NOT VALID"),
            format!("COMMENT ON TABLE {child} IS 'A real table comment'"),
            format!("COMMENT ON COLUMN {child}.label IS 'A real column comment'"),
            format!("ALTER TABLE {child} ENABLE ROW LEVEL SECURITY"),
            format!("ALTER TABLE {child} FORCE ROW LEVEL SECURITY"),
            format!("CREATE UNIQUE INDEX expression_idx ON {child} (lower(label)) INCLUDE (amount) WHERE a > 0"),
            format!("CREATE TABLE {partitioned} (id integer) PARTITION BY RANGE (id)"),
            format!("CREATE VIEW {view} AS SELECT a FROM {parent}"),
        ] {
            driver.execute_query(&sql).await.unwrap();
        }
        let details = driver.list_table_details().await.unwrap();
        let get = |name: &str| &details.iter().find(|(s, t, _)| s == &schema && t == name).unwrap().2;
        let d = get(child_name);
        assert!(d.object_id.as_ref().unwrap().parse::<u32>().unwrap() > 0);
        assert!(d.owner.as_ref().is_some_and(|s| !s.is_empty()));
        assert!(d.tablespace.as_ref().is_some_and(|s| !s.is_empty()));
        assert_eq!(d.comment.as_deref(), Some("A real table comment"));
        assert_eq!(d.persistence.as_deref(), Some("Unlogged"));
        assert_eq!(d.row_security, Some(true));
        assert_eq!(d.force_row_security, Some(true));
        assert_eq!(d.partitioned, Some(false));
        assert_eq!(d.estimated_rows, None, "unanalyzed rows must stay unknown");
        assert!(d.total_bytes.unwrap() >= d.table_bytes.unwrap());
        assert_eq!(d.total_bytes, Some(d.table_bytes.unwrap() + d.index_bytes.unwrap()));
        assert_eq!(d.last_analyze, None);
        let index = d.indexes.iter().find(|i| i.name == "expression_idx").unwrap();
        assert_eq!(index.method, "btree");
        assert!(index.unique && index.valid);
        assert_eq!(index.columns.len(), 2);
        assert!(index.columns[0].contains("lower"));
        assert_eq!(index.columns[1], "amount");
        assert!(index.definition.contains("WHERE"));
        assert!(index.definition.contains("INCLUDE"));
        let fk = d.constraints.iter().find(|c| c.kind == "FOREIGN KEY").unwrap();
        assert_eq!(fk.columns, ["b", "a"]);
        for fragment in ["REFERENCES", "ON UPDATE CASCADE", "ON DELETE SET NULL", "DEFERRABLE INITIALLY DEFERRED"] {
            assert!(fk.definition.contains(fragment), "{}", fk.definition);
        }
        assert!(!d.constraints.iter().find(|c| c.name == "positive").unwrap().validated);
        assert_eq!(get("partitioned").partitioned, Some(true));
        assert_eq!(get("partitioned").partition_key.as_deref(), Some("RANGE (id)"));
        assert_eq!(get("partitioned").total_bytes, None, "parent has no own storage");
        assert_eq!(get("view").estimated_rows, None);
        assert_eq!(get("view").table_bytes, None);
        assert_eq!(get("view").row_security, None);
        let columns = driver.list_columns(&schema, child_name).await.unwrap();
        let label = columns.iter().find(|c| c.name == "label").unwrap();
        assert_eq!(label.raw_type.as_deref(), Some("character varying(42)"));
        assert_eq!(label.comment.as_deref(), Some("A real column comment"));
        assert!(columns.iter().find(|c| c.name == "a").unwrap().is_foreign_key);
        assert_eq!(columns.iter().find(|c| c.name == "amount").unwrap().raw_type.as_deref(), Some("numeric(12,2)"));
        assert!(driver.list_columns(&schema, "parent").await.unwrap().iter().all(|c| c.is_primary_key));
        assert!(driver.list_columns(&schema, "missing").await.unwrap().is_empty());

        driver.execute_query(&format!("ANALYZE {child}")).await.unwrap();
        let refreshed = driver.list_table_details().await.unwrap();
        let refreshed = &refreshed.iter().find(|(s, t, _)| s == &schema && t == child_name).unwrap().2;
        assert_eq!(refreshed.estimated_rows, Some(0), "known empty is distinct from unknown");
        assert!(refreshed.last_analyze.is_some());
        let all = driver.list_all_columns().await.unwrap();
        let all_child = &all.iter().find(|(s, t, _)| s == &schema && t == child_name).unwrap().2;
        assert_eq!(all_child.len(), columns.len());
        assert_eq!(all_child[2].raw_type, label.raw_type);
        driver.execute_query(&format!("DROP SCHEMA {qschema} CASCADE")).await.unwrap();
        assert!(!driver.list_table_details().await.unwrap().iter().any(|(s, _, _)| s == &schema));
    });
}

#[test]
fn test_postgres_ddl_round_trip() {
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    let Some(config) = test_config() else {
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return;
    };
    db_wire::get_runtime().block_on(async {
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&config.host).port(config.port).user(&config.user).dbname(&config.database);
        if let Some(password) = &config.password { cfg.password(password); }
        let (client, connection) = cfg.connect(tokio_postgres::NoTls).await.unwrap();
        let task = tokio::spawn(async move { connection.await.unwrap(); });
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb ddl {}", std::process::id());
        let q = db_wire::sql::SqlDialect(DbDriverKind::Postgres);
        let table = q.qualified_table(&schema, "quoted\"table");
        let parent = q.qualified_table(&schema, "parent");
        let leaf = q.qualified_table(&schema, "leaf");
        let view = q.qualified_table(&schema, "view");
        let empty = q.qualified_table(&schema, "empty");
        let qs = q.quote_ident(&schema);
        let setup = format!(r#"
            CREATE SCHEMA {qs};
            CREATE FUNCTION {qs}.touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
        "#);
        client.batch_execute(&format!("DROP SCHEMA IF EXISTS {qs} CASCADE; {setup}")).await.unwrap();
        client.batch_execute(&format!(r#"
            CREATE UNLOGGED TABLE {table} (
                "key" bigint GENERATED ALWAYS AS IDENTITY (START WITH 12 INCREMENT BY 3 CACHE 5),
                serial_id serial,
                alternate smallint GENERATED BY DEFAULT AS IDENTITY (START 7 MINVALUE 2 MAXVALUE 100 INCREMENT 2 CACHE 3 CYCLE),
                "name""quoted" varchar(42) COLLATE "C" DEFAULT 'it''s fine' NOT NULL,
                value numeric(12,3),
                doubled numeric GENERATED ALWAYS AS (value * 2) STORED,
                CONSTRAINT z_pk PRIMARY KEY ("key") WITH (fillfactor=70),
                CONSTRAINT a_self FOREIGN KEY (serial_id) REFERENCES {table}("key") DEFERRABLE INITIALLY DEFERRED,
                CONSTRAINT uq UNIQUE ("name""quoted") INCLUDE (value)
            ) WITH (fillfactor=80);
            ALTER TABLE {table} ADD CONSTRAINT positive CHECK (value > 0) NOT VALID;
            CREATE INDEX partial_expr ON {table} ((lower("name""quoted"))) INCLUDE (value) WHERE value > 1;
            ALTER TABLE {table} ALTER COLUMN "name""quoted" SET STORAGE EXTERNAL;
            ALTER TABLE {table} ALTER COLUMN value SET STATISTICS 120;
            ALTER TABLE {table} REPLICA IDENTITY FULL;
            ALTER TABLE {table} CLUSTER ON z_pk;
            COMMENT ON TABLE {table} IS E'table comment\nsecond line';
            COMMENT ON COLUMN {table}.value IS 'value''s comment';
            COMMENT ON CONSTRAINT positive ON {table} IS 'check comment';
            COMMENT ON INDEX {qs}.partial_expr IS 'index comment';
            CREATE POLICY "policy""name" ON {table} AS RESTRICTIVE FOR UPDATE TO PUBLIC USING (value > 1) WITH CHECK (value > 2);
            COMMENT ON POLICY "policy""name" ON {table} IS 'policy comment';
            ALTER TABLE {table} ENABLE ROW LEVEL SECURITY;
            ALTER TABLE {table} FORCE ROW LEVEL SECURITY;
            CREATE TRIGGER touch BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION {qs}.touch();
            ALTER TABLE {table} DISABLE TRIGGER touch;
            CREATE RULE no_delete AS ON DELETE TO {table} DO INSTEAD NOTHING;
            ALTER TABLE {table} ENABLE ALWAYS RULE no_delete;
            COMMENT ON TRIGGER touch ON {table} IS 'trigger comment';
            GRANT SELECT ON {table} TO PUBLIC;
            REVOKE INSERT ON {table} FROM CURRENT_USER;
            GRANT UPDATE (value) ON {table} TO PUBLIC;
            GRANT USAGE ON SEQUENCE {qs}."quoted""table_serial_id_seq" TO PUBLIC;
            CREATE STATISTICS {qs}.value_stats (dependencies) ON value, serial_id FROM {table};
            COMMENT ON STATISTICS {qs}.value_stats IS 'stats comment';
            CREATE TABLE {parent} (id integer PRIMARY KEY, label text DEFAULT 'parent', CHECK (id >= 0)) PARTITION BY RANGE (id);
            CREATE TABLE {leaf} PARTITION OF {parent} FOR VALUES FROM (0) TO (100);
            CREATE VIEW {view} WITH (security_barrier=true) AS SELECT "key", value FROM {table};
            ALTER VIEW {view} ALTER COLUMN value SET DEFAULT 12;
            COMMENT ON VIEW {view} IS 'view comment';
            CREATE TABLE {empty} ();
            CREATE TABLE {qs}.invalid_source (id integer);
            INSERT INTO {qs}.invalid_source VALUES (1), (1);
            CREATE TABLE {qs}.base (id integer);
            CREATE TABLE {qs}.inherited () INHERITS ({qs}.base);
            CREATE TYPE {qs}.record_type AS (id integer);
            CREATE TABLE {qs}.typed OF {qs}.record_type;
        "#)).await.unwrap();
        let error = client.batch_execute(&format!("CREATE UNIQUE INDEX CONCURRENTLY invalid_index ON {qs}.invalid_source (id)")).await.unwrap_err();
        assert_eq!(error.code(), Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION));
        let details = driver.list_table_details().await.unwrap();
        let invalid = details.iter().find(|(s, t, _)| s == &schema && t == "invalid_source").unwrap().2.ddl.as_ref().unwrap();
        assert!(invalid.contains("-- WARNING: invalid index omitted: invalid_index"));
        assert!(!invalid.contains("CREATE UNIQUE INDEX"));
        for (name, clause) in [("inherited", " INHERITS ("), ("typed", " OF ")] {
            let ddl = details.iter().find(|(s, t, _)| s == &schema && t == name).unwrap().2.ddl.as_ref().unwrap();
            assert!(!ddl.contains("unavailable"), "{ddl}");
            assert!(ddl.contains("CREATE TABLE"), "{ddl}");
            assert!(ddl.contains(clause), "{ddl}");
        }
        let mut saved = Vec::new();
        for name in ["quoted\"table", "parent", "leaf", "view", "empty"] {
            let ddl = details.iter().find(|(s, t, _)| s == &schema && t == name).unwrap().2.ddl.clone().unwrap();
            assert!(!ddl.contains("unavailable"), "{ddl}");
            saved.push((name, ddl));
        }
        client.batch_execute(&format!("DROP SCHEMA {qs} CASCADE; {setup}")).await.unwrap();
        for (_, ddl) in &saved {
            client.batch_execute(ddl).await.unwrap_or_else(|error| panic!("{error:?}\n{ddl}"));
        }
        let restored = driver.list_table_details().await.unwrap();
        for (name, ddl) in &saved {
            let actual = restored.iter().find(|(s, t, _)| s == &schema && t == name).unwrap().2.ddl.as_ref().unwrap();
            assert_eq!(actual, ddl, "DDL changed after recreating {name}");
        }
        client.batch_execute(&format!("DROP SCHEMA {qs} CASCADE")).await.unwrap();
        task.abort();
    });
}

#[test]
fn postgres_foreign_keys_preserve_schema_identity_and_composite_mapping() {
    let Some(cfg) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(cfg);
        let schema = format!("fastdb_er_{}", std::process::id());
        driver.execute_query(&format!("CREATE SCHEMA {schema}")).await.unwrap();
        for sql in [
            format!("CREATE TABLE {schema}.parent (a integer, b integer, PRIMARY KEY(a,b))"),
            format!("CREATE TABLE {schema}.child (x integer, y integer, CONSTRAINT composite_fk FOREIGN KEY(x,y) REFERENCES {schema}.parent(a,b))"),
            format!("CREATE TABLE {schema}.self_ref (id integer PRIMARY KEY, parent integer REFERENCES {schema}.self_ref(id))"),
        ] { driver.execute_query(&sql).await.unwrap(); }
        let keys = driver.list_foreign_keys().await.unwrap().unwrap();
        let mappings: Vec<_> = keys.iter().filter(|k| k.source_schema == schema && k.source_table == "child").collect();
        assert_eq!(mappings.len(), 2);
        assert_eq!((&*mappings[0].source_column, &*mappings[0].target_column), ("x", "a"));
        assert_eq!((&*mappings[1].source_column, &*mappings[1].target_column), ("y", "b"));
        assert!(mappings.iter().all(|k| k.target_schema == schema && k.target_table == "parent" && k.name == "composite_fk"));
        assert!(keys.iter().any(|k| k.source_schema == schema && k.source_table == "self_ref" && k.target_table == "self_ref"));
        driver.execute_query(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
    });
}

#[test]
fn structured_filter_literals_preserve_unicode_and_backslashes_under_server_modes() {
    let Some(cfg) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        use db_wire::filter::{Criterion, FilterOp, FilterSpec};
        let driver = PostgresDriver::new(cfg);
        driver
            .execute_query("SET standard_conforming_strings=off")
            .await
            .unwrap();
        driver
            .execute_query("CREATE TEMP TABLE filter_literal_data (value text)")
            .await
            .unwrap();
        let value = r#"Tiếng Việt \path 'quoted' a LIMIT b"#;
        let spec = FilterSpec {
            criteria: vec![Criterion {
                column: "value".into(),
                op: FilterOp::Equal,
                value: value.into(),
            }],
            ..Default::default()
        };
        let predicate = spec.sql(DbDriverKind::Postgres, &["value".into()]).unwrap();
        let literal = predicate
            .split_once(" = ")
            .unwrap()
            .1
            .strip_suffix(')')
            .unwrap();
        driver
            .execute_query(&format!(
                "INSERT INTO filter_literal_data VALUES ({literal})"
            ))
            .await
            .unwrap();
        let (pager, rows, _) = driver
            .execute_query(&format!(
                "SELECT value FROM filter_literal_data {predicate} LIMIT 200"
            ))
            .await
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(cell_text(&pager, 0, 0).as_deref(), Some(value));
        driver
            .execute_query("SET standard_conforming_strings=on")
            .await
            .unwrap();
    });
}

#[test]
fn structured_numeric_filter_uses_index_at_realistic_volume() {
    let Some(cfg) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        use db_wire::filter::{Criterion, FilterOp, FilterOrder, FilterSpec};
        let driver = PostgresDriver::new(cfg);
        for sql in [
            "SET statement_timeout='15s'",
            "CREATE TEMP TABLE filter_plan_data (id integer PRIMARY KEY, payload text)",
            "INSERT INTO filter_plan_data SELECT g, repeat('x',100) FROM generate_series(1,100000) g",
            "ANALYZE filter_plan_data",
        ] {
            driver.execute_query(sql).await.unwrap();
        }
        let predicate = FilterSpec {
            criteria: vec![Criterion {
                column: "id".into(),
                op: FilterOp::GreaterEqual,
                value: "99990".into(),
            }],
            order: Some(FilterOrder {
                column: "id".into(),
                descending: false,
            }),
            ..Default::default()
        }
        .sql(DbDriverKind::Postgres, &["id".into()])
        .unwrap();
        let (pager, rows, _) = driver.execute_query(&format!(
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT id FROM filter_plan_data {predicate} LIMIT 200"
        )).await.unwrap();
        assert_eq!(rows, 1);
        let text = cell_text(&pager, 0, 0).unwrap();
        let plan: serde_json::Value = serde_json::from_str(&text).unwrap();
        let scan = &plan[0]["Plan"]["Plans"][0];
        assert!(matches!(scan["Node Type"].as_str(), Some("Index Scan" | "Index Only Scan")), "{text}");
        assert_eq!(scan["Actual Rows"], 11);
        assert!(scan["Index Cond"].as_str().unwrap().contains("id >= 99990"), "{text}");
        eprintln!("structured filter plan (100000 rows): {text}");
    });
}

#[test]
fn value_edits_roundtrip_binary_json_xml_empty_and_null() {
    let Some(config) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        driver.execute_query("SET statement_timeout='15s'").await.unwrap();
        driver.execute_query("CREATE TEMP TABLE value_edit_data (id integer PRIMARY KEY, bytes bytea, document json, markup xml, value text)").await.unwrap();
        driver.execute_query("INSERT INTO value_edit_data VALUES (1, NULL, NULL, NULL, NULL), (2, NULL, NULL, NULL, NULL)").await.unwrap();
        let (pager, _, _) = driver.execute_query("SELECT id, bytes, document, markup, value FROM value_edit_data ORDER BY id").await.unwrap();
        let raw = b"\\xdead\0\xff";
        let json = r#"{"n":0.12345678901234567890123456789,"k":1,"k":2}"#;
        let xml = "<a> hello <b/> world </a>";
        let mut edits = db_wire::sql::RowChanges::default();
        for (column, value) in [(1, db_wire::value::hex_text(raw)), (2, json.into()), (3, xml.into()), (4, "".into())] {
            edits.changes.insert((0, column), Some(value));
        }
        edits.changes.insert((1, 1), Some(db_wire::value::hex_text(b"")));
        let sql = edits.plan_sql("value_edit_data", &pager, &[0], db_wire::sql::SqlDialect(DbDriverKind::Postgres)).unwrap();
        assert_eq!(driver.apply_row_changes(&sql).await.unwrap(), 2);
        let (stored, _, _) = driver.execute_query("SELECT id, bytes, document, markup, value FROM value_edit_data ORDER BY id").await.unwrap();
        assert_eq!(db_wire::value::source_binary(&stored.get_cell(0, 1).unwrap(), DbDriverKind::Postgres).unwrap(), raw);
        assert_eq!(cell_text(&stored, 0, 2).as_deref(), Some(json));
        assert_eq!(cell_text(&stored, 0, 3).as_deref(), Some(xml));
        assert_eq!(cell_text(&stored, 0, 4).as_deref(), Some(""));
        assert_eq!(cell_text(&stored, 1, 1).as_deref(), Some("\\x"));
        assert_eq!(stored.get_cell(1, 4), None);
        for mode in ["on", "off"] {
            driver.execute_query(&format!("SET standard_conforming_strings={mode}")).await.unwrap();
            let text = "quote\\' and \\ path 名";
            let mut update = db_wire::sql::RowChanges::default();
            update.changes.insert((1, 4), Some(text.into()));
            let sql = update.plan_sql("value_edit_data", &stored, &[0], db_wire::sql::SqlDialect(DbDriverKind::Postgres)).unwrap();
            driver.apply_row_changes(&sql).await.unwrap();
            let (value, _, _) = driver.execute_query("SELECT value FROM value_edit_data WHERE id=2").await.unwrap();
            assert_eq!(cell_text(&value, 0, 0).as_deref(), Some(text));
        }
        let mut deletion = db_wire::sql::RowChanges::default();
        deletion.deleted_rows.insert(0);
        // JSON/XML lack equality operators; check binary original predicates separately.
        let (binary, _, _) = driver.execute_query("SELECT id, bytes FROM value_edit_data ORDER BY id").await.unwrap();
        let sql = deletion.plan_sql("value_edit_data", &binary, &[], db_wire::sql::SqlDialect(DbDriverKind::Postgres)).unwrap();
        assert!(sql[0].contains("pg_catalog.decode"));
        assert_eq!(driver.apply_row_changes(&sql).await.unwrap(), 1);
    });
}

#[test]
fn postgres_connection_deadline_covers_a_stalled_startup_handshake() {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    db_wire::get_runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let driver = PostgresDriver::new(ConnectionConfig {
            host: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
            use_unix_socket: false,
            connect_timeout: Duration::from_millis(200),
            ..Default::default()
        });
        let peer = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut startup = [0; 8];
            socket.read_exact(&mut startup).await.unwrap();
            // Hold the socket open without sending a PostgreSQL response.
            std::future::pending::<()>().await;
        };
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = driver.ping() => result,
                _ = peer => unreachable!(),
            }
        })
        .await
        .expect("driver deadline did not stop the stalled handshake");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("PostgreSQL connection timed out")
        );
    });
}

#[test]
fn postgres_read_only_blocks_native_and_disguised_writes() {
    let Some(config) = test_config() else { return };
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    db_wire::get_runtime().block_on(async {
        let writer = PostgresDriver::new(config.clone());
        let schema = format!("fastdb_ro_{}", uuid::Uuid::new_v4().simple());
        writer.execute_schema_ddl(&format!("CREATE SCHEMA {schema}; CREATE TABLE {schema}.items (id int PRIMARY KEY); INSERT INTO {schema}.items VALUES (1)")).await.unwrap();
        let reader = PostgresDriver::new(ConnectionConfig { read_only: true, ..config });
        let result = async {
            anyhow::ensure!(reader.is_read_only() && !reader.supports_transfer());
            let (pager, _, _) = reader.execute_query("SHOW transaction_read_only").await?;
            anyhow::ensure!(pager.get_cell(0, 0).as_deref() == Some(b"on".as_slice()));
            let write = format!("WITH written AS (INSERT INTO {schema}.items VALUES (2) RETURNING id) SELECT id FROM written");
            let error = reader.execute_query(&write).await.err().expect("write was accepted");
            anyhow::ensure!(format!("{error:#}").contains("read-only transaction"));
            anyhow::ensure!(reader.execute_schema_ddl(&format!("DROP TABLE {schema}.items")).await.is_err());
            anyhow::ensure!(reader.apply_row_changes(&[format!("DELETE FROM {schema}.items")]).await.is_err());
            let session = reader.open_script_session().await?;
            let batch = db_wire::transfer::batch::Batch::new(
                &schema,
                "items",
                vec!["id".into()],
                vec![vec![db_wire::transfer::mapping::Value::Text("2".into())]],
            )?;
            anyhow::ensure!(session.insert_batch(batch).await.is_err());
            let run = |sql: String| {
                let session = &session;
                async move {
                    let (tx, _rx) = flume::bounded(16);
                    session.execute_streaming(&sql, tx).await
                }
            };
            run("SELECT set_config('default_transaction_read_only', 'off', false)".into()).await?;
            for sql in ["BEGIN READ WRITE".to_owned(), write] {
                anyhow::ensure!(run(sql).await.is_err());
            }
            run(format!("SET search_path TO {schema}")).await?;
            run("BEGIN; ".into()).await?;
            run("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ".into()).await?;
            run("SAVEPOINT safe".into()).await?;
            anyhow::ensure!(run("SET TRANSACTION READ WRITE".into()).await.is_err());
            run(format!("/* outer /* nested */ comment */ SELECT id FROM {schema}.items")).await?;
            run("ROLLBACK TO SAVEPOINT safe".into()).await?;
            run("RELEASE SAVEPOINT safe".into()).await?;
            run("ROLLBACK".into()).await?;
            session.finish().await?;
            let (pager, rows, _) = writer.execute_query(&format!("SELECT id FROM {schema}.items")).await?;
            anyhow::ensure!(rows == 1 && pager.get_cell(0, 0).as_deref() == Some(b"1".as_slice()));
            Ok::<_, anyhow::Error>(())
        }.await;
        writer.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
        result.unwrap();
    });
}

#[test]
fn postgres_application_name_reaches_normal_and_script_connections() {
    let Some(config) = test_config() else { return };
    let driver = PostgresDriver::new(ConnectionConfig {
        application_name: Some("FastDB reporting".into()),
        ..config
    });
    db_wire::get_runtime().block_on(async {
        let (pager, _, _) = driver.execute_query("SHOW application_name").await.unwrap();
        assert_eq!(
            pager.get_cell(0, 0).as_deref(),
            Some(b"FastDB reporting".as_slice())
        );
        let session = driver.open_script_session().await.unwrap();
        let (events, results) = flume::bounded(16);
        session
            .execute_streaming("SHOW application_name", events)
            .await
            .unwrap();
        assert!(results.try_iter().any(|event| matches!(
            event, db_wire::StreamEvent::SchemaReady(pager)
                if pager.get_cell(0, 0).as_deref() == Some(b"FastDB reporting".as_slice())
        )));
        session.finish().await.unwrap();
    });
}
