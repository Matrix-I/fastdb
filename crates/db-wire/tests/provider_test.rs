//! Opt-in tests against real vendor servers and installed ODBC drivers.
//! FASTDB_TEST_{SQLSERVER,ORACLE,DB2}_{PORT,HOST,DATABASE,USER,PASSWORD,TLS,CA}
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, StreamEvent, create_driver};

fn config(kind: DbDriverKind, prefix: &str) -> Option<ConnectionConfig> {
    let port = match std::env::var(format!("FASTDB_TEST_{prefix}_PORT")) {
        Ok(port) => port.parse().expect("provider test port must be numeric"),
        Err(_) => {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_PROVIDER_TESTS").is_none(),
                "missing FASTDB_TEST_{prefix}_PORT"
            );
            eprintln!("skipped: FASTDB_TEST_{prefix}_PORT not configured");
            return None;
        }
    };
    let env = |field: &str| std::env::var(format!("FASTDB_TEST_{prefix}_{field}"));
    Some(ConnectionConfig {
        driver: kind,
        port,
        host: env("HOST").unwrap_or_else(|_| "localhost".into()),
        database: env("DATABASE").expect("provider DATABASE (Oracle service) required"),
        user: env("USER").expect("provider USER required"),
        password: env("PASSWORD").ok(),
        tls_enabled: env("TLS").is_ok_and(|v| v == "1"),
        tls_ca_cert: env("CA").ok(),
        use_unix_socket: false,
        ..ConnectionConfig::default()
    })
}

async fn check_writes(driver: &dyn DatabaseDriver, kind: DbDriverKind) -> anyhow::Result<()> {
    let table = format!(
        "\"FASTDB_T_{}\"",
        uuid::Uuid::new_v4().simple().to_string()[..12].to_uppercase()
    );
    let text_type = if kind == DbDriverKind::Oracle {
        "VARCHAR2(40)"
    } else {
        "VARCHAR(40)"
    };
    driver
        .execute_query(&format!(
            "CREATE TABLE {table} (ID INTEGER PRIMARY KEY, VAL {text_type})"
        ))
        .await?;
    let result = async {
        driver
            .execute_query(&format!(
                "INSERT INTO {table} (ID, VAL) VALUES (1, 'before')"
            ))
            .await?;
        let update = format!("UPDATE {table} SET VAL='after' WHERE ID=1");
        let missing = format!("UPDATE {table} SET VAL='missing' WHERE ID=9999");
        assert!(
            driver
                .apply_row_changes(&[update.clone(), missing])
                .await
                .is_err()
        );
        let (pager, _, _) = driver
            .execute_query(&format!("SELECT VAL FROM {table} WHERE ID=1"))
            .await?;
        assert_eq!(
            pager.get_cell(0, 0).unwrap(),
            b"before",
            "failed transaction must roll back"
        );
        assert_eq!(driver.apply_row_changes(&[update]).await?, 1);
        let (pager, _, _) = driver
            .execute_query(&format!("SELECT VAL FROM {table} WHERE ID=1"))
            .await?;
        assert_eq!(pager.get_cell(0, 0).unwrap(), b"after");
        driver
            .execute_query(&format!("DELETE FROM {table} WHERE ID=1"))
            .await?;
        anyhow::Ok(())
    }
    .await;
    let cleanup = driver.execute_query(&format!("DROP TABLE {table}")).await;
    result?;
    cleanup?;
    Ok(())
}

async fn check_script_session(
    driver: &dyn DatabaseDriver,
    kind: DbDriverKind,
) -> anyhow::Result<()> {
    let session = driver.open_script_session().await?;
    let table = format!(
        "FASTDB_P105_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    );
    let setup = format!("CREATE TABLE {table} (ID INTEGER PRIMARY KEY)");
    driver.execute_query(&setup).await?;
    let result = async {
        let execute = |sql: String| {
            let session = session.clone();
            async move {
                let (tx, _rx) = flume::unbounded();
                session.execute_streaming(&sql, tx).await
            }
        };
        if kind == DbDriverKind::SqlServer {
            execute("SET XACT_ABORT ON; SET IMPLICIT_TRANSACTIONS ON".into()).await?;
            assert!(execute("BEGIN".into()).await.is_err());
            assert_eq!(session.in_transaction(), Some(false));
            execute(format!("INSERT INTO {table} VALUES (9)")).await?;
            assert!(execute("BEGIN".into()).await.is_err());
            assert_eq!(session.in_transaction(), Some(true));
            assert!(!session.transaction_failed());
            execute("ROLLBACK".into()).await?;
            execute("SET IMPLICIT_TRANSACTIONS OFF; SET XACT_ABORT OFF".into()).await?;
        }
        execute("BEGIN".into()).await?;
        assert_eq!(session.in_transaction(), Some(true));
        execute(format!("INSERT INTO {table} VALUES (1)")).await?;
        let savepoint = match kind {
            DbDriverKind::SqlServer => "SAVE TRANSACTION fastdb_point",
            DbDriverKind::Db2 => "SAVEPOINT fastdb_point ON ROLLBACK RETAIN CURSORS",
            _ => "SAVEPOINT fastdb_point",
        };
        execute(savepoint.into()).await?;
        execute(format!("INSERT INTO {table} VALUES (2)")).await?;
        let rollback = match kind {
            DbDriverKind::SqlServer => "ROLLBACK TRANSACTION fastdb_point",
            DbDriverKind::Db2 => "ROLLBACK TO SAVEPOINT fastdb_point",
            _ => "ROLLBACK TO fastdb_point",
        };
        execute(rollback.into()).await?;
        execute("COMMIT".into()).await?;
        assert_eq!(session.in_transaction(), Some(false));
        execute("BEGIN".into()).await?;
        execute(format!("INSERT INTO {table} VALUES (3)")).await?;
        session.finish().await?;
        assert!(session.is_closed());
        let (pager, _, _) = driver
            .execute_query(&format!("SELECT ID FROM {table} ORDER BY ID"))
            .await?;
        assert_eq!(pager.total_rows(), 1);
        assert_eq!(pager.get_cell(0, 0).as_deref(), Some(b"1".as_slice()));
        anyhow::Ok(())
    }
    .await;
    if result.is_err() {
        let cleanup = session.finish().await;
        if let Err(error) = cleanup {
            eprintln!("Session cleanup failed: {error}");
        }
    }
    let drop_result = driver.execute_query(&format!("DROP TABLE {table}")).await;
    result?;
    drop_result?;
    Ok(())
}

fn check(kind: DbDriverKind, prefix: &str) {
    let Some(config) = config(kind, prefix) else {
        return;
    };
    let driver = create_driver(config).unwrap();
    db_wire::get_runtime().block_on(async {
        driver.ping().await.unwrap();
        let sql = match kind {
            DbDriverKind::Oracle => "SELECT 42 AS ID, CAST(NULL AS VARCHAR2(10)) AS EMPTY_VALUE, CAST(12345.67 AS NUMBER(10,2)) AS AMOUNT FROM DUAL;",
            DbDriverKind::Db2 => "SELECT 42 AS ID, CAST(NULL AS VARCHAR(10)) AS EMPTY_VALUE, CAST(12345.67 AS DECIMAL(10,2)) AS AMOUNT FROM SYSIBM.SYSDUMMY1",
            _ => "SELECT 42 AS ID, CAST(NULL AS VARCHAR(10)) AS EMPTY_VALUE, CAST(12345.67 AS DECIMAL(10,2)) AS AMOUNT",
        };
        let (tx, rx) = flume::unbounded();
        driver.execute_streaming(sql, tx).await.unwrap();
        let events: Vec<_> = rx.drain().collect();
        let StreamEvent::SchemaReady(pager) = &events[0] else { panic!("missing columns") };
        assert_eq!(pager.get_cell(0, 0).unwrap(), b"42");
        assert_eq!(pager.get_cell(0, 1), None);
        assert_eq!(pager.get_cell(0, 2).unwrap(), b"12345.67");
        assert!(matches!(events.last(), Some(StreamEvent::Completed { total_rows: 1, .. })));
        // Metadata and a second statement must reuse a valid session after fetching.
        driver.list_tables().await.unwrap();
        driver.list_all_columns().await.unwrap();
        let (tx, rx) = flume::unbounded();
        assert!(driver.execute_streaming("SELECT * FROM fastdb_missing_object_9d08cbe1", tx).await.is_err());
        assert!(rx.drain().any(|e| matches!(e, StreamEvent::Error(_))));
        driver.ping().await.unwrap();
        if std::env::var("FASTDB_TEST_PROVIDER_WRITES").is_ok_and(|v| v == "1") { check_writes(driver.as_ref(), kind).await.unwrap(); check_script_session(driver.as_ref(), kind).await.unwrap(); }
    });
}

#[test]
fn sql_server_connects_and_runs_sql() {
    check(DbDriverKind::SqlServer, "SQLSERVER");
}
#[test]
fn oracle_connects_and_runs_sql() {
    check(DbDriverKind::Oracle, "ORACLE");
}
#[test]
fn db2_connects_and_runs_sql() {
    check(DbDriverKind::Db2, "DB2");
}
