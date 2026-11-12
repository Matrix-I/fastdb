use db_wire::catalog::CatalogSection;
use db_wire::metadata::{MetadataFeature as F, MetadataSupport as S};
use db_wire::{
    ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, PostgresDriver, SqliteDriver,
};

#[test]
fn declarations_match_implemented_provider_readers() {
    let pg = PostgresDriver::new(ConnectionConfig::default()).metadata_capabilities();
    for feature in [
        F::Columns,
        F::Constraints,
        F::Indexes,
        F::ForeignKeys,
        F::TableDetails,
    ] {
        assert_eq!(pg.support(feature), S::Available);
    }
    for section in [
        CatalogSection::Dependencies,
        CatalogSection::References,
        CatalogSection::Partitions,
        CatalogSection::Triggers,
        CatalogSection::Rules,
        CatalogSection::Policies,
        CatalogSection::Permissions,
    ] {
        assert_eq!(pg.support(F::Catalog(section)), S::Available);
    }
    for kind in [DbDriverKind::MySql, DbDriverKind::MariaDb] {
        let mysql = MySqlDriver::new(ConnectionConfig {
            driver: kind,
            ..Default::default()
        })
        .metadata_capabilities();
        for feature in [F::Columns, F::Constraints, F::Indexes, F::ForeignKeys] {
            assert_eq!(mysql.support(feature), S::Available);
        }
        assert_eq!(
            mysql.support(F::TableDetails),
            if kind == DbDriverKind::MySql {
                S::Available
            } else {
                S::NotImplemented
            }
        );
        assert_eq!(
            mysql.support(F::Catalog(CatalogSection::Triggers)),
            S::NotImplemented
        );
        assert_eq!(
            mysql.support(F::Catalog(CatalogSection::Rules)),
            S::Unsupported
        );
    }
    let sqlite = SqliteDriver::new_memory().metadata_capabilities();
    assert_eq!(sqlite.support(F::Columns), S::Available);
    assert_eq!(sqlite.support(F::Indexes), S::Available);
    assert_eq!(sqlite.support(F::ForeignKeys), S::Available);
    assert_eq!(sqlite.support(F::Constraints), S::Available);
    assert_eq!(
        sqlite.support(F::Catalog(CatalogSection::Permissions)),
        S::Unsupported
    );
    for kind in [
        DbDriverKind::SqlServer,
        DbDriverKind::Oracle,
        DbDriverKind::Db2,
    ] {
        let odbc = db_wire::odbc::OdbcDriver::new(ConnectionConfig {
            driver: kind,
            ..Default::default()
        })
        .metadata_capabilities();
        assert_eq!(odbc.support(F::Columns), S::Available);
        assert_eq!(odbc.support(F::Constraints), S::NotImplemented);
        assert_eq!(odbc.support(F::ForeignKeys), S::NotImplemented);
    }
}

#[tokio::test]
async fn available_empty_is_distinct_from_unimplemented() {
    let sqlite = SqliteDriver::new_memory();
    assert_eq!(sqlite.list_foreign_keys().await.unwrap(), Some(vec![]));
    assert!(sqlite.list_all_constraints().await.unwrap().is_empty());
    assert!(sqlite.list_all_indexes().await.unwrap().is_empty());
    assert_eq!(
        sqlite.metadata_capabilities().support(F::ForeignKeys),
        S::Available
    );
    assert_eq!(
        sqlite.metadata_capabilities().support(F::Constraints),
        S::Available
    );
    assert!(
        sqlite
            .load_catalog_section("main", "absent", CatalogSection::Permissions)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn default_batch_reader_does_not_hide_failed_column_metadata() {
    let sqlite = SqliteDriver::new_memory();
    sqlite
        .execute_query("CREATE VIEW broken AS SELECT id FROM missing_relation")
        .await
        .unwrap();
    let result = sqlite.list_all_columns().await;
    assert!(
        result.is_err(),
        "a failed view must not become an empty successful column list"
    );
}

#[test]
fn native_postgres_privilege_errors_are_classified_through_query_boundary() {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return;
    };
    let driver = PostgresDriver::new(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: "postgres".into(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
        use_unix_socket: false,
        ..Default::default()
    });
    db_wire::get_runtime().block_on(async {
        let error=driver.execute_query("DO $$ BEGIN RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'localized fixture'; END $$").await.err().expect("expected native permission error");
        let failure=db_wire::metadata::MetadataFailure::from_error(&error);
        assert!(failure.permission_denied,"{error:#}");
        assert!(failure.message.contains("localized fixture"));
    });
}
