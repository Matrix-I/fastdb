use db_wire::session_context::{ContextKind, choices_sql, schema_statements};
use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver};

#[test]
fn context_support_and_quoted_identifiers_are_provider_specific() {
    assert!(choices_sql(DbDriverKind::Sqlite, ContextKind::Schema).is_err());
    assert!(choices_sql(DbDriverKind::Oracle, ContextKind::Database).is_err());
    assert!(schema_statements(DbDriverKind::SqlServer, "dbo").is_err());
    assert_eq!(
        schema_statements(DbDriverKind::Postgres, "a\"b").unwrap(),
        ["SET search_path TO \"a\"\"b\"", "SELECT current_schema()"].map(String::from)
    );
    assert!(schema_statements(DbDriverKind::MySql, "other").is_err());
    assert!(schema_statements(DbDriverKind::Postgres, "$user").is_err());
    for name in ["", "a\0b", "a\nb", "a\rb"] {
        assert!(schema_statements(DbDriverKind::Postgres, name).is_err());
    }
    assert!(SqliteDriver::new_memory().with_database("other").is_err());
}

#[test]
fn postgres_context_preserves_the_original_physical_session() {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none());
        eprintln!("skipped: FASTDB_TEST_PG_PORT not configured");
        return;
    };
    db_wire::get_runtime().block_on(async {
        use db_wire::{ConnectionConfig, PostgresDriver, StreamEvent};
        let driver = PostgresDriver::new(ConnectionConfig {
            port: port.parse().unwrap(),
            use_unix_socket: false,
            host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
            password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
            ..ConnectionConfig::default()
        });
        async fn value(session: &dyn db_wire::script::ScriptSession, sql: &str) -> Option<Vec<u8>> {
            let (tx, rx) = flume::unbounded();
            session.execute_streaming(sql, tx).await.unwrap();
            rx.drain().find_map(|event| match event {
                StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
                _ => None,
            })
        }
        let original = driver.open_script_session().await.unwrap();
        let initial = value(original.as_ref(), "SELECT current_database()").await;
        let [set, read] = schema_statements(DbDriverKind::Postgres, "pg_catalog").unwrap();
        value(original.as_ref(), &set).await;
        assert_eq!(
            value(original.as_ref(), &read).await.unwrap(),
            b"pg_catalog"
        );
        value(original.as_ref(), "BEGIN").await;
        value(
            original.as_ref(),
            "CREATE TEMP TABLE fastdb_context (id int)",
        )
        .await;
        value(original.as_ref(), "INSERT INTO fastdb_context VALUES (42)").await;
        let alternate = driver
            .with_database("template1")
            .unwrap()
            .open_script_session()
            .await
            .unwrap();
        assert_eq!(
            value(alternate.as_ref(), "SELECT current_database()")
                .await
                .unwrap(),
            b"template1"
        );
        assert_eq!(
            value(
                alternate.as_ref(),
                "SELECT to_regclass('pg_temp.fastdb_context')"
            )
            .await,
            None
        );
        assert_eq!(
            value(original.as_ref(), "SELECT id FROM fastdb_context")
                .await
                .unwrap(),
            b"42"
        );
        assert_eq!(
            value(original.as_ref(), "SELECT current_database()").await,
            initial
        );
        alternate.finish().await.unwrap();
        original.finish().await.unwrap();
    });
}
