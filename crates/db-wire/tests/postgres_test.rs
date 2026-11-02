//! Integration tests against a live PostgreSQL server.
//! Set FASTDB_TEST_PG_PORT (and optionally FASTDB_TEST_PG_HOST / FASTDB_TEST_PG_USER) to run them.

use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};
use std::sync::Arc;

// Both fixtures enumerate all visible relations; keep their DROP/CREATE phases
// separate so one test cannot remove catalogs while the other deparses them.
static CATALOG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        for name in ["inherited", "typed"] {
            let ddl = details.iter().find(|(s, t, _)| s == &schema && t == name).unwrap().2.ddl.as_ref().unwrap();
            assert!(ddl.starts_with("-- DDL unavailable:"));
            assert!(!ddl.contains("CREATE TABLE"));
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
