use db_wire::catalog::CatalogSection;
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver, SqliteDriver};
fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return None;
    };
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: "postgres".into(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
        use_unix_socket: false,
        ..Default::default()
    })
}
#[tokio::test]
async fn unsupported_catalog_is_not_an_empty_loaded_section() {
    assert!(
        SqliteDriver::new_memory()
            .load_catalog_section("main", "missing", CatalogSection::Policies)
            .await
            .unwrap()
            .is_none()
    );
}
#[test]
fn postgres_catalog_sections_read_actual_objects_and_navigation_targets() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async{
        let driver=PostgresDriver::new(config);
        let s=format!("fastdb_catalog_{}",std::process::id());
        driver.execute_query(&format!("CREATE SCHEMA {s}")).await.unwrap();
        for sql in [
            format!("CREATE TABLE {s}.base (id integer PRIMARY KEY, note text)"),
            format!("CREATE TABLE {s}.child (id integer REFERENCES {s}.base(id))"),
            format!("CREATE VIEW {s}.dependent AS SELECT * FROM {s}.base"),
            format!("CREATE FUNCTION {s}.touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$"),
            format!("CREATE TRIGGER touch BEFORE INSERT ON {s}.base FOR EACH ROW EXECUTE FUNCTION {s}.touch()"),
            format!("CREATE RULE update_notice AS ON UPDATE TO {s}.base DO ALSO NOTIFY fastdb_catalog_event"),
            format!("ALTER TABLE {s}.base ENABLE ROW LEVEL SECURITY"),
            format!("CREATE POLICY read_all ON {s}.base FOR SELECT TO PUBLIC USING (true)"),
            format!("GRANT INSERT ON {s}.base TO PUBLIC"),
            format!("GRANT SELECT(id) ON {s}.base TO PUBLIC"),
            format!("CREATE TABLE {s}.parts (id integer) PARTITION BY RANGE(id)"),
            format!("CREATE TABLE {s}.part0 PARTITION OF {s}.parts FOR VALUES FROM (0) TO (10)"),
        ]{driver.execute_query(&sql).await.unwrap();}
        let rows=driver.load_catalog_section(&s,"dependent",CatalogSection::Dependencies).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.relation.as_ref()==Some(&(s.clone(),"base".into()))),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"base",CatalogSection::References).await.unwrap().unwrap();
        for table in ["child","dependent"]{
            assert!(rows.iter().any(|r|r.relation.as_ref()==Some(&(s.clone(),table.into()))),"{rows:?}");
        }
        let rows=driver.load_catalog_section(&s,"base",CatalogSection::Triggers).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.name=="touch" && r.definition.contains("CREATE TRIGGER")),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"base",CatalogSection::Rules).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.name=="update_notice" && r.definition.contains("NOTIFY")),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"base",CatalogSection::Policies).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.name=="read_all" && r.definition.contains("PUBLIC") && r.definition.contains("RLS enabled: true")),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"base",CatalogSection::Permissions).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.name=="PUBLIC" && r.kind=="TABLE INSERT"),"{rows:?}");
        assert!(rows.iter().any(|r|r.name=="PUBLIC" && r.kind.contains("COLUMN id SELECT")),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"parts",CatalogSection::Partitions).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.relation.as_ref()==Some(&(s.clone(),"part0".into()))&&r.definition.contains("FROM")),"{rows:?}");
        let rows=driver.load_catalog_section(&s,"part0",CatalogSection::Partitions).await.unwrap().unwrap();
        assert!(rows.iter().any(|r|r.kind=="Parent relation"));
        let rows=driver.load_catalog_section(&s,"child",CatalogSection::Policies).await.unwrap().unwrap();
        assert!(rows.is_empty());
        assert!(driver.load_catalog_section(&s,"missing",CatalogSection::Rules).await.is_err());
        driver.execute_query(&format!("DROP SCHEMA {s} CASCADE")).await.unwrap();
    });
}

#[tokio::test]
async fn sqlite_object_explorer_is_explicitly_unsupported() {
    use db_wire::objects::ObjectKind;
    assert!(
        SqliteDriver::new_memory()
            .list_objects(ObjectKind::Routine, 0)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn postgres_object_discovery_and_source_preserve_native_identity() {
    use db_wire::objects::{ObjectKind, ObjectSummary};
    let Some(config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        driver.execute_query("CREATE SCHEMA fastdb_object_fixture").await.unwrap();
        let result = async {
            driver.execute_query("CREATE SEQUENCE fastdb_object_fixture.counter;").await?;
            driver.execute_query("CREATE TYPE fastdb_object_fixture.status AS ENUM ('open','closed')").await?;
            driver.execute_query("CREATE FUNCTION fastdb_object_fixture.plus_one(integer) RETURNS integer LANGUAGE sql AS 'SELECT $1 + 1'").await?;
            driver.execute_query("CREATE TABLE fastdb_object_fixture.rows(id integer)").await?;
            driver.execute_query("CREATE FUNCTION fastdb_object_fixture.trigger_fn() RETURNS trigger LANGUAGE plpgsql AS 'BEGIN RETURN NEW; END'").await?;
            driver.execute_query("CREATE TRIGGER change BEFORE INSERT ON fastdb_object_fixture.rows FOR EACH ROW EXECUTE FUNCTION fastdb_object_fixture.trigger_fn()").await?;
            for i in 0..102 {
                driver.execute_query(&format!("CREATE SEQUENCE fastdb_object_fixture.page_{i}")).await?;
            }
            let mut cursor=0;
            let mut ids=std::collections::HashSet::new();
            let mut count=0;
            loop {
                let page=driver.list_objects(ObjectKind::Sequence,cursor).await?.unwrap();
                assert!(page.objects.len()<=100);
                for object in page.objects {
                    assert!(object.id>cursor);
                    assert!(ids.insert(object.id));
                    if object.schema=="fastdb_object_fixture" { count+=1; }
                }
                match page.next {Some(next)=>{assert!(next>cursor);cursor=next;},None=>break}
            }
            assert_eq!(count,103);
            for (kind, name, expected) in [
                (ObjectKind::Trigger, "change", "CREATE TRIGGER"),
                (ObjectKind::Sequence, "counter", "Increment"),
                (ObjectKind::Type, "status", "'open'"),
                (ObjectKind::Routine, "plus_one", "CREATE OR REPLACE FUNCTION"),
            ] {
                let mut after = 0;
                let object = loop {
                    let page = driver.list_objects(kind, after).await?.unwrap();
                    if let Some(object) = page.objects.into_iter().find(|o| o.schema == "fastdb_object_fixture" && o.name == name) { break object; }
                    after = page.next.expect("fixture must be discovered");
                };
                assert!(driver.object_source(kind, &object).await?.unwrap().contains(expected));
                let stale = ObjectSummary { name: "renamed".into(), ..object };
                assert!(driver.object_source(kind, &stale).await.is_err());
            }
            anyhow::Ok(())
        }.await;
        driver.execute_query("DROP SCHEMA fastdb_object_fixture CASCADE").await.unwrap();
        result.unwrap();
    });
}

#[test]
fn postgres_definition_search_preserves_schema_literals_and_overload_identity() {
    use db_wire::objects::{DEFINITION_SEARCH_LIMIT, DefinitionKind};
    use futures::FutureExt;
    let Some(config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let schema = format!("FastDB source {}", std::process::id());
        let other = format!("fastdb_source_other_{}", std::process::id());
        driver.execute_schema_ddl(&format!(r#"CREATE SCHEMA "{schema}"; CREATE SCHEMA {other}"#)).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            driver.execute_schema_ddl(&format!(r#"
                CREATE VIEW "{schema}".report AS SELECT 'Needle_%'::text AS label;
                CREATE VIEW "{schema}".decoy AS SELECT 'needleAB'::text AS label;
                CREATE VIEW {other}.report AS SELECT 'Needle_%'::text AS label;
                CREATE FUNCTION "{schema}".report(integer) RETURNS text LANGUAGE sql AS $$ SELECT 'Needle_% int' $$;
                CREATE FUNCTION "{schema}".report(text) RETURNS text LANGUAGE sql AS $$ SELECT 'Needle_% text' $$;
                CREATE PROCEDURE "{schema}".report() LANGUAGE plpgsql AS $$ BEGIN PERFORM 'Needle_% proc'; END $$;
                CREATE TABLE "{schema}".entries (label text UNIQUE);
                CREATE TABLE "{schema}".children (label text REFERENCES "{schema}".entries(label));
                CREATE MATERIALIZED VIEW {other}.materialized AS SELECT 'extra definition marker'::text AS label;
                CREATE MATERIALIZED VIEW "{schema}".materialized AS SELECT 'extra definition marker'::text AS label;
                CREATE INDEX label_index ON "{schema}".entries(label) WHERE label = 'extra definition marker';
                CREATE RULE audit_rule AS ON INSERT TO "{schema}".entries DO ALSO NOTIFY "extra definition marker";
                CREATE FUNCTION "{schema}".audit_trigger() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
                CREATE TRIGGER audit_trigger BEFORE INSERT ON "{schema}".entries FOR EACH ROW WHEN (NEW.label = 'extra definition marker') EXECUTE FUNCTION "{schema}".audit_trigger();
                ALTER TABLE "{schema}".entries ADD CONSTRAINT label_check CHECK (label <> 'family_% marker');
                CREATE DOMAIN "{schema}".label_type AS text CONSTRAINT domain_check CHECK (VALUE <> 'family_% marker');
                CREATE TABLE {other}.entries (label text CONSTRAINT label_check CHECK (label <> 'family_% marker'));
                CREATE POLICY label_policy ON "{schema}".entries USING (label <> 'family_% marker') WITH CHECK (label <> 'family_% marker');
                CREATE STATISTICS "{schema}".label_stats ON (label = 'family_% marker') FROM "{schema}".entries;
                CREATE AGGREGATE "{schema}".total(integer) (SFUNC = int4pl, STYPE = integer);
                DO $$ BEGIN
                    EXECUTE format('CREATE VIEW "{schema}".wide AS SELECT %L::text AS label', repeat('wide source', 6200));
                    FOR i IN 1..60 LOOP
                        EXECUTE format('CREATE VIEW "{schema}".%I AS SELECT %L::text AS label', 'overflow_' || i, 'bulk match');
                    END LOOP;
                END $$;
            "#)).await?;
            let (cancel, signal) = flume::bounded(1);
            let hits = driver.search_definitions(&schema, "needle_%", signal.clone()).await?.unwrap();
            assert_eq!(hits.len(), 4);
            assert_eq!(hits.iter().filter(|hit| hit.kind == DefinitionKind::View).count(), 1);
            assert!(hits.iter().all(|hit| hit.object.schema == schema && hit.object.name == "report"));
            for marker in ["Needle_% int", "Needle_% text", "Needle_% proc"] {
                assert_eq!(hits.iter().filter(|hit| hit.source.as_deref().is_some_and(|source| source.contains(marker))).count(), 1);
            }
            assert_eq!(hits.iter().filter(|hit| hit.kind == DefinitionKind::Routine).map(|hit| hit.object.id).collect::<std::collections::HashSet<_>>().len(), 3);
            let extra = driver.search_definitions(&schema, "extra definition marker", signal.clone()).await?.unwrap();
            assert_eq!(extra.len(), 4);
            for (kind, name, qualifier) in [
                (DefinitionKind::MaterializedView, "materialized", ""),
                (DefinitionKind::Index, "label_index", "entries"),
                (DefinitionKind::Rule, "audit_rule", "entries"),
                (DefinitionKind::Trigger, "audit_trigger", "entries"),
            ] {
                assert!(extra.iter().any(|hit| hit.kind == kind && hit.object.name == name
                    && hit.object.qualifier == qualifier
                    && hit.source.as_deref().is_some_and(|source| source.contains("extra definition marker"))));
            }
            let internal = driver.search_definitions(&schema, "RI_FKey", signal.clone()).await?.unwrap();
            let families = driver.search_definitions(&schema, "family_% marker", signal.clone()).await?.unwrap();
            assert_eq!(families.len(), 4, "{families:?}");
            for (kind, name, owner, clauses) in [
                (DefinitionKind::Constraint, "label_check", "entries", &["CHECK"][..]),
                (DefinitionKind::Constraint, "domain_check", "label_type", &["CHECK"][..]),
                (DefinitionKind::Policy, "label_policy", "entries", &["USING", "WITH CHECK"][..]),
                (DefinitionKind::Statistics, "label_stats", "", &["CREATE STATISTICS", "entries"][..]),
            ] {
                assert!(families.iter().any(|hit| hit.kind == kind && hit.object.schema == schema
                    && hit.object.name == name && hit.object.qualifier == owner
                    && hit.source.as_deref().is_some_and(|source| clauses.iter().all(|clause| source.contains(clause)))),
                    "missing {name}: {families:?}");
            }
            assert!(internal.is_empty(), "{internal:?}");
            let oversized = driver.search_definitions(&schema, "wide source", signal.clone()).await?.unwrap();
            assert_eq!(oversized.len(), 1);
            assert_eq!(oversized[0].object.name, "wide");
            assert!(oversized[0].source.is_none());
            let bounded = driver.search_definitions(&schema, "bulk match", signal.clone()).await?.unwrap();
            assert_eq!(bounded.len(), DEFINITION_SEARCH_LIMIT + 1);
            assert!(driver.search_definitions(&schema, "'; DROP SCHEMA \"FastDB source\"; --", signal.clone()).await?.unwrap().is_empty());
            assert!(driver.search_definitions("fastdb_source_missing", "needle", signal.clone()).await.is_err());
            cancel.send(()).unwrap();
            assert!(driver.search_definitions(&schema, "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
            anyhow::Ok(())
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!(r#"DROP SCHEMA "{schema}" CASCADE; DROP SCHEMA {other} CASCADE"#)).await.unwrap();
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic)).unwrap();
    });
}

#[tokio::test]
async fn sqlite_definition_search_matches_native_source_and_respects_scope() {
    use db_wire::objects::DefinitionKind;
    let driver = SqliteDriver::new_memory();
    for sql in [
        "CREATE TABLE items (value TEXT DEFAULT 'Needle_%')",
        "CREATE TABLE decoy (value TEXT DEFAULT 'needleAB')",
        "CREATE VIEW example AS SELECT 'Needle_%' AS value",
        "CREATE INDEX ix ON items(value) WHERE value = 'Needle_%'",
        "CREATE TRIGGER tr AFTER INSERT ON items WHEN NEW.value = 'Needle_%' BEGIN SELECT 1; END",
    ] {
        driver.execute_query(sql).await.unwrap();
    }
    let (cancel, signal) = flume::bounded(1);
    let hits = driver
        .search_definitions("main", "needle_%", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.kind).collect::<Vec<_>>(),
        [
            DefinitionKind::Table,
            DefinitionKind::View,
            DefinitionKind::Index,
            DefinitionKind::Trigger,
        ]
    );
    assert_eq!(hits[2].object.qualifier, "items");
    assert!(
        hits.iter()
            .all(|hit| hit.source.as_deref().unwrap().contains("Needle_%"))
    );
    assert!(
        driver
            .search_definitions("temp", "needle", signal.clone())
            .await
            .is_err()
    );
    for index in 0..52 {
        driver
            .execute_query(&format!("CREATE VIEW bulk_{index} AS SELECT 'bulk marker'"))
            .await
            .unwrap();
    }
    let capped = driver
        .search_definitions("main", "bulk marker", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(capped.len(), db_wire::objects::DEFINITION_SEARCH_LIMIT + 1);
    let wide = "x".repeat(db_wire::objects::DEFINITION_SOURCE_LIMIT as usize);
    driver
        .execute_query(&format!("CREATE VIEW wide AS SELECT 'wide marker {wide}'"))
        .await
        .unwrap();
    let wide = driver
        .search_definitions("main", "wide marker", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wide.len(), 1);
    assert!(wide[0].source.is_none());
    cancel.send(()).unwrap();
    assert!(
        driver
            .search_definitions("main", "needle", signal)
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn postgres_searches_cancel_a_blocked_backend() {
    let Some(mut config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_search_cancel_{}", std::process::id());
        config.application_name = Some(schema.clone());
        let mut fixture = tokio_postgres::Config::new();
        fixture
            .host(&config.host)
            .port(config.port)
            .user(&config.user)
            .dbname(&config.database);
        if let Some(password) = &config.password {
            fixture.password(password);
        }
        let (mut client, connection) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fixture.connect(tokio_postgres::NoTls),
        )
        .await
        .unwrap()
        .unwrap();
        let connection = tokio::spawn(connection);
        client
            .batch_execute(&format!(
                "SET statement_timeout = '5s'; CREATE SCHEMA {schema}; \
             CREATE TABLE {schema}.base (marker text); \
             CREATE VIEW {schema}.view AS SELECT marker FROM {schema}.base"
            ))
            .await
            .unwrap();
        let driver = PostgresDriver::new(config);
        let result = async {
            let blocker = client.transaction().await?;
            blocker
                .batch_execute(&format!(
                    "LOCK TABLE {schema}.base IN ACCESS EXCLUSIVE MODE"
                ))
                .await?;
            let mut errors = Vec::new();
            for definition in [true, false] {
                let (cancel, signal) = flume::bounded(1);
                let search = async {
                    if definition {
                        driver
                            .search_definitions(&schema, "marker", signal)
                            .await
                            .map(|_| ())
                    } else {
                        driver
                            .search_table_data(&schema, "base", "marker", signal)
                            .await
                            .map(|_| ())
                    }
                };
                let monitor = async {
                    wait_for_search_lock(&blocker, &schema, true).await?;
                    cancel.send_async(()).await?;
                    wait_for_search_lock(&blocker, &schema, false).await
                };
                let (search, monitor) = tokio::join!(search, monitor);
                monitor?;
                errors.push(search);
            }
            blocker.rollback().await?;
            anyhow::Ok(errors)
        }
        .await;
        client
            .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
        drop(client);
        connection.await.unwrap().unwrap();
        let errors: Vec<_> = result
            .unwrap()
            .into_iter()
            .map(|outcome| outcome.unwrap_err().to_string())
            .collect();
        assert_eq!(
            errors,
            ["Definition search cancelled", "Data search cancelled"]
        );
    });
}

async fn wait_for_search_lock(
    monitor: &tokio_postgres::Transaction<'_>,
    application: &str,
    expected: bool,
) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            monitor
                .simple_query("SELECT pg_stat_clear_snapshot()")
                .await?;
            let locked: bool = monitor
                .query_one(
                    "SELECT EXISTS (SELECT 1 FROM pg_stat_activity \
                 WHERE application_name = $1 AND wait_event_type = 'Lock')",
                    &[&application],
                )
                .await?
                .get(0);
            if locked == expected {
                return anyhow::Ok(());
            }
        }
    })
    .await?
}

#[test]
fn postgres_table_data_search_reopens_only_the_matching_row_version() {
    let Some(config) = config() else { return };
    db_wire::get_runtime().block_on(async {
        use futures::FutureExt;
        let driver = PostgresDriver::new(config);
        let schema = format!("fastdb_data_search_{}", std::process::id());
        let other = format!("{schema}_other");
        driver.execute_schema_ddl(&format!(r#"
            CREATE SCHEMA {schema};
            CREATE SCHEMA {other};
            CREATE TABLE {other}."odd""table" (value text);
            INSERT INTO {other}."odd""table" VALUES ('needle_% outside');
            CREATE TABLE {schema}.only_binary (value bytea);
            CREATE TABLE {schema}." " (value text);
            INSERT INTO {schema}." " VALUES ('space marker');
            CREATE TABLE {schema}."odd""table" (id integer, "display value" text COLLATE "C", payload bytea, other text COLLATE "POSIX");
            INSERT INTO {schema}."odd""table" VALUES
              (1, 'Needle_% literal', NULL, NULL), (2, 'needleAB decoy', NULL, ''), (3, NULL, decode('6e6565646c655f25','hex'), NULL);
        "#)).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let (_cancel, signal) = flume::bounded(1);
            let hits = driver.search_table_data(&schema, r#"odd"table"#, "needle_%", signal.clone()).await?.unwrap();
            assert_eq!(hits.len(), 1);
            let error = driver.search_table_data(&schema, "only_binary", "needle", signal.clone()).await.unwrap_err();
            assert!(error.to_string().contains("No supported scalar columns"));
            let spaced = driver.search_table_data(&schema, " ", "space marker", signal.clone()).await?.unwrap();
            assert_eq!(spaced.len(), 1);
            assert_eq!(spaced[0].preview, "space marker");
            assert_eq!(hits[0].column, "display value");
            assert_eq!(hits[0].preview, "Needle_% literal");
            let row = driver.read_data_search_row(&hits[0].locator, signal.clone()).await?.unwrap();
            assert_eq!(row.columns, vec![
                ("id".into(), Some("1".into())),
                ("display value".into(), Some("Needle_% literal".into())),
                ("other".into(), None),
            ]);
            assert_eq!(row.omitted_columns, 1);
            driver.execute_query(&format!(r#"VACUUM FULL {schema}."odd""table""#)).await?;
            let error = driver.read_data_search_row(&hits[0].locator, signal.clone()).await.unwrap_err();
            assert!(error.to_string().contains("changed"));
            let hits = driver.search_table_data(&schema, r#"odd"table"#, "needle_%", signal.clone()).await?.unwrap();
            assert_eq!(hits.len(), 1);
            driver.execute_query(&format!(r#"UPDATE {schema}."odd""table" SET "display value" = 'changed' WHERE id = 1"#)).await?;
            let error = driver.read_data_search_row(&hits[0].locator, signal.clone()).await.unwrap_err();
            assert!(error.to_string().contains("changed"));
            driver.execute_query(&format!(r#"INSERT INTO {schema}."odd""table" (id, "display value") SELECT n, 'bulk marker' FROM generate_series(1, 70) n"#)).await?;
            assert_eq!(driver.search_table_data(&schema, r#"odd"table"#, "bulk marker", signal.clone()).await?.unwrap().len(), 51);
            assert!(driver.search_table_data(&schema, r#"odd"table"#, "'; DROP TABLE xyz; --", signal).await?.unwrap().is_empty());
            anyhow::Ok(())
        }).catch_unwind().await;
        driver.execute_schema_ddl(&format!("DROP SCHEMA {schema} CASCADE; DROP SCHEMA {other} CASCADE")).await.unwrap();
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic)).unwrap();
    });
}

#[tokio::test]
async fn sqlite_definition_search_reopens_writable_file_uri_read_only() {
    let directory = tempfile::tempdir().unwrap();
    for (index, options) in ["mode=rwc", "mode=rwc#fragment", "m%6fde=rwc", "mode=r%77c"]
        .into_iter()
        .enumerate()
    {
        let path = directory.path().join(format!("search-{index}.sqlite"));
        let driver = SqliteDriver::new(ConnectionConfig {
            database: format!("file:{}?{options}", path.display()),
            ..Default::default()
        })
        .unwrap();
        driver
            .execute_query("CREATE TABLE uri_marker (value TEXT)")
            .await
            .unwrap();
        let (_cancel, signal) = flume::bounded(1);
        let hits = driver
            .search_definitions("main", "uri_marker", signal)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hits[0].object.name, "uri_marker");
    }
}

#[tokio::test]
async fn sqlite_definition_search_sees_private_memory_uri_objects() {
    let driver = SqliteDriver::new(ConnectionConfig {
        database: "file::memory:?cache=private".into(),
        ..Default::default()
    })
    .unwrap();
    driver
        .execute_query("CREATE TABLE memory_marker (value TEXT)")
        .await
        .unwrap();
    let (_cancel, signal) = flume::bounded(1);
    let hits = driver
        .search_definitions("main", "memory_marker", signal)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].object.name, "memory_marker");
}

#[tokio::test]
async fn sqlite_data_search_returns_captured_keyless_and_without_rowid_matches() {
    let driver = SqliteDriver::new_memory();
    for sql in [
        r#"CREATE TABLE "odd""table" (label, blank, bytes)"#,
        r#"INSERT INTO "odd""table" VALUES ('Needle_% literal', NULL, X'6e6565646c655f25'), ('needleAB', '', X'6e6565646c655f25')"#,
        r#"INSERT INTO "odd""table" VALUES ('prefix' || char(0) || 'Needle_%', NULL, NULL)"#,
        "CREATE TABLE keyed (a TEXT, b INTEGER, PRIMARY KEY (a, b)) WITHOUT ROWID",
        "INSERT INTO keyed VALUES ('Needle_% keyed', 1)",
    ] {
        driver.execute_query(sql).await.unwrap();
    }
    let (cancel, signal) = flume::bounded(1);
    let keyless = driver
        .search_table_data("main", r#"odd"table"#, "needle_%", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(keyless.len(), 1);
    assert_eq!(keyless[0].column, "label");
    let keyed = driver
        .search_table_data("main", "keyed", "needle_%", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(keyed.len(), 1);
    driver
        .execute_query(r#"DELETE FROM "odd""table""#)
        .await
        .unwrap();
    let preview = driver
        .read_data_search_row(&keyless[0].locator, signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        preview.columns,
        [
            ("label".into(), Some("Needle_% literal".into())),
            ("blank".into(), None),
            ("bytes".into(), Some("[BLOB]".into())),
        ]
    );
    assert!(
        driver
            .search_table_data("temp", "keyed", "needle", signal.clone())
            .await
            .is_err()
    );
    let cols = (0..33)
        .map(|index| format!("c{index} TEXT"))
        .collect::<Vec<_>>()
        .join(", ");
    driver
        .execute_query(&format!("CREATE TABLE wide ({cols})"))
        .await
        .unwrap();
    driver.execute_query("INSERT INTO wide (c0, c32) WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 60) SELECT hex(zeroblob(300)), 'bulk marker' FROM n").await.unwrap();
    let wide = driver
        .search_table_data("main", "wide", "bulk marker", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wide.len(), db_wire::objects::DATA_SEARCH_LIMIT + 1);
    let preview = driver
        .read_data_search_row(&wide[0].locator, signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.columns.len(), 32);
    assert_eq!(preview.omitted_columns, 1);
    assert_eq!(preview.columns[0].1.as_ref().unwrap().len(), 512);
    assert_eq!(
        preview.columns[31],
        ("c32".into(), Some("bulk marker".into()))
    );
    cancel.send(()).unwrap();
    assert!(
        driver
            .search_table_data("main", "keyed", "needle", signal)
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[tokio::test]
async fn sqlite_data_search_reads_committed_file_rows_and_rejects_views() {
    let directory = tempfile::tempdir().unwrap();
    let driver = SqliteDriver::new(ConnectionConfig {
        database: directory
            .path()
            .join("search.sqlite")
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    })
    .unwrap();
    for sql in [
        "CREATE TABLE data (value INTEGER)",
        "INSERT INTO data VALUES (42001)",
        "CREATE VIEW scope_view AS SELECT value FROM data",
    ] {
        driver.execute_query(sql).await.unwrap();
    }
    driver
        .execute_query("CREATE TABLE malformed (value TEXT)")
        .await
        .unwrap();
    driver
        .execute_query("INSERT INTO malformed VALUES (CAST(X'FF6E6565646C65' AS TEXT))")
        .await
        .unwrap();
    let (_cancel, signal) = flume::bounded(1);
    let hits = driver
        .search_table_data("main", "data", "420", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].preview, "42001");
    let malformed = driver
        .search_table_data("main", "malformed", "needle", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(malformed[0].preview, "�needle");
    let continuation = "80".repeat(4096);
    driver
        .execute_query(&format!(
            "INSERT INTO malformed VALUES (CAST(X'FF{continuation}6E6565646C65' AS TEXT))"
        ))
        .await
        .unwrap();
    let long = driver
        .search_table_data("main", "malformed", "needle", signal.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(long.len(), 2);
    assert!(long.iter().all(|hit| hit.preview.chars().count() <= 512));

    assert!(
        driver
            .search_table_data("main", "scope_view", "420", signal)
            .await
            .unwrap_err()
            .to_string()
            .contains("ordinary SQLite table")
    );
}
