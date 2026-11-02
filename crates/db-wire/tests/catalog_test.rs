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
