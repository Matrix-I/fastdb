use db_wire::{
    ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver, QueryCellUpdate, StreamEvent,
};
use std::sync::Arc;

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_PG_PORT not configured");
        return None;
    };
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        database: std::env::var("FASTDB_TEST_PG_DATABASE").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
        use_unix_socket: false,
        ..Default::default()
    })
}

#[test]
fn native_query_origin_edits_and_conflicts_are_verified_against_postgres() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let session = driver.open_script_session().await.unwrap();
        let table = format!("public.fastdb_query_edit_{}", uuid::Uuid::new_v4().simple());
        driver.execute_query(&format!("CREATE TABLE {table} (tenant int, id int, label text, enabled boolean, PRIMARY KEY (tenant, id))")).await.unwrap();
        let result = async {
            driver.execute_query(&format!("INSERT INTO {table} VALUES (1, 1, 'old', true), (2, 1, 'other', false)")).await?;
            let query = format!("SELECT * FROM {table}");
            async fn read(session: &dyn db_wire::script::ScriptSession, sql: &str) -> anyhow::Result<Arc<db_wire::QueryResultPager>> {
                let (tx, rx) = flume::unbounded();
                session.execute_streaming(sql, tx).await?;
                rx.drain().find_map(|event| match event {
                    StreamEvent::SchemaReady(pager) => Some(pager), _ => None
                }).ok_or_else(|| anyhow::anyhow!("No result"))
            }
            assert!(read(session.as_ref(), &query).await?.query_edit_origin.is_none());
            let pager = driver.load_editable_query(&query).await?;
            assert!(pager.query_edit_origin.is_some());
            let row = (0..pager.total_rows()).find(|&row| pager.get_cell(row, 0).as_deref() == Some(b"1")).unwrap();
            driver.save_query_edits(pager.clone(), vec![
                QueryCellUpdate { row, column: 2, value: Some("O'Reilly \\ $1".into()) },
                QueryCellUpdate { row, column: 3, value: None },
            ]).await?;
            let (stored, _, _) = driver.execute_query(&format!("SELECT label, enabled FROM {table} WHERE tenant=1")).await?;
            assert_eq!(stored.get_cell(0, 0).unwrap(), b"O'Reilly \\ $1");
            assert_eq!(stored.get_cell(0, 1), None);
            assert!(driver.save_query_edits(pager.clone(), vec![QueryCellUpdate { row, column: 2, value: Some("stale".into()) }]).await.is_err());
            assert!(driver.save_query_edits(pager.clone(), vec![QueryCellUpdate { row, column: 0, value: Some("9".into()) }]).await.is_err());

            for sql in [
                format!("SELECT label FROM {table}"),
                format!("SELECT * FROM {table} WHERE id=1"),
                format!("SELECT * FROM {table} UNION ALL SELECT * FROM {table}"),
                format!("SELECT * FROM {table} a JOIN {table} b USING (tenant, id)"),
            ] {
                assert!(driver.load_editable_query(&sql).await.is_err(), "{sql}");
            }
            let view = format!("{table}_view");
            driver.execute_query(&format!("CREATE VIEW {view} AS {query}")).await?;
            assert!(driver.load_editable_query(&format!("SELECT * FROM {view}")).await.is_err());
            driver.execute_query(&format!("DROP VIEW {view}")).await?;
            driver.execute_query(&format!("ALTER TABLE {table} ENABLE ROW LEVEL SECURITY")).await?;
            assert!(driver.load_editable_query(&query).await.is_err());
            driver.execute_query(&format!("ALTER TABLE {table} DISABLE ROW LEVEL SECURITY")).await?;
            driver.execute_query(&format!("CREATE TRIGGER query_edit_guard BEFORE UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION pg_catalog.suppress_redundant_updates_trigger()")).await?;
            assert!(driver.load_editable_query(&query).await.is_err());
            driver.execute_query(&format!("DROP TRIGGER query_edit_guard ON {table}")).await?;
            let fresh = driver.load_editable_query(&query).await?;
            let old_label = fresh.get_cell(0, 2);
            let tenant: i32 = std::str::from_utf8(&fresh.get_cell(0, 0).unwrap()).unwrap().parse().unwrap();
            // The row sort guarantees the valid first write executes before the failing write.
            assert!(driver.save_query_edits(fresh.clone(), vec![
                QueryCellUpdate { row: 0, column: 2, value: Some("must roll back".into()) },
                QueryCellUpdate { row: 1, column: 3, value: Some("not boolean".into()) },
            ]).await.is_err());
            let (stored, _, _) = driver.execute_query(&format!("SELECT label FROM {table} WHERE tenant={tenant}")).await?;
            assert_eq!(stored.get_cell(0, 0), old_label);
            let (a, b) = tokio::join!(
                driver.save_query_edits(fresh.clone(), vec![QueryCellUpdate { row: 0, column: 2, value: Some("writer a".into()) }]),
                driver.save_query_edits(fresh.clone(), vec![QueryCellUpdate { row: 0, column: 2, value: Some("writer b".into()) }])
            );
            assert_ne!(a.is_ok(), b.is_ok(), "Exactly one concurrent writer may replace the original value");
            let current = driver.load_editable_query(&query).await?;
            driver.execute_query(&format!("ALTER TABLE {table} RENAME COLUMN label TO renamed")).await?;
            assert!(driver.save_query_edits(current, vec![QueryCellUpdate { row, column: 2, value: Some("wrong".into()) }]).await.is_err());
            let fresh = driver.load_editable_query(&query).await?;
            let relation = table.strip_prefix("public.").unwrap();
            driver.execute_query(&format!("ALTER TABLE {table} DROP CONSTRAINT {relation}_pkey")).await?;
            assert!(driver.load_editable_query(&query).await.is_err());
            assert!(driver.save_query_edits(fresh, vec![QueryCellUpdate { row, column: 2, value: Some("no key".into()) }]).await.is_err());

            anyhow::Ok(())
        }.await;
        session.finish().await.unwrap();
        driver.execute_query(&format!("DROP TABLE {table}")).await.unwrap();
        result.unwrap();
    });
}
