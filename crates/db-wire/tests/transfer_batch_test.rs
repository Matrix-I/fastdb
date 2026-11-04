//! Native parameter binding and transaction read-back for transfer batches.
use anyhow::{Result, ensure};
use db_wire::{
    ConnectionConfig, DatabaseDriver, DbDriverKind, QueryResultPager, StreamEvent,
    script::ScriptSession,
    transfer::{batch::Batch, mapping::Value},
};
use std::{sync::Arc, time::Duration};

async fn execute(session: &dyn ScriptSession, sql: &str) -> Result<Option<Arc<QueryResultPager>>> {
    let (tx, rx) = flume::unbounded();
    tokio::time::timeout(Duration::from_secs(15), session.execute_streaming(sql, tx)).await??;
    Ok(rx.drain().find_map(|event| match event {
        StreamEvent::SchemaReady(pager) => Some(pager),
        _ => None,
    }))
}
fn input(schema: &str, table: &str) -> Batch {
    use db_wire::{
        ColumnMeta, DataType,
        transfer::mapping::{ColumnMapping, Conversion, Mapping},
    };
    let columns = [
        ("id", DataType::Int4),
        ("value", DataType::Text),
        ("payload", DataType::Bytea),
        ("flag", DataType::Bool),
    ];
    let mapping = Mapping::new(
        4,
        columns
            .iter()
            .enumerate()
            .map(|(source, (name, kind))| ColumnMapping {
                source,
                target: ColumnMeta::new(*name, *kind),
                conversion: Conversion::Auto,
            })
            .collect(),
    )
    .unwrap();
    let rows = [
        vec![
            Value::Text("1".into()),
            Value::Text("quote'\\ 名; DROP TABLE unrelated;--".into()),
            Value::Binary((0..=255).collect()),
            Value::Text("true".into()),
        ],
        vec![
            Value::Text("2".into()),
            Value::Text("".into()),
            Value::Binary(vec![]),
            Value::Text("false".into()),
        ],
        vec![
            Value::Text("3".into()),
            Value::Null,
            Value::Null,
            Value::Null,
        ],
    ]
    .iter()
    .map(|row| mapping.convert(row).unwrap())
    .collect();
    Batch::new(
        schema,
        table,
        columns.iter().map(|(name, _)| (*name).into()).collect(),
        rows,
    )
    .unwrap()
}

async fn roundtrip(
    driver: Arc<dyn DatabaseDriver>,
    kind: DbDriverKind,
    schema: &str,
) -> Result<()> {
    ensure!(
        driver.supports_transfer(),
        "Native transfer capability missing"
    );
    let table = format!("fastdb_batch_{}", uuid::Uuid::new_v4().simple());
    let dialect = db_wire::sql::SqlDialect(kind);
    let target = format!(
        "{}.{}",
        dialect.quote_ident(schema),
        dialect.quote_ident(&table)
    );
    let binary = if kind == DbDriverKind::Postgres {
        "BYTEA"
    } else {
        "BLOB"
    };
    let boolean = if kind == DbDriverKind::MySql {
        "BIT(1)"
    } else {
        "BOOLEAN"
    };
    let engine = if kind == DbDriverKind::MySql {
        " ENGINE=InnoDB"
    } else {
        ""
    };
    driver.execute_query(&format!("CREATE TABLE {target} (id INTEGER PRIMARY KEY, value TEXT, payload {binary}, flag {boolean}, generated_value VARCHAR(30) DEFAULT 'generated'){engine}")).await?;
    let session = driver.open_script_session().await?;
    let result: Result<()> = async {
        execute(session.as_ref(), "BEGIN").await?;
        ensure!(
            session.insert_batch(input(schema, &table)).await? == 3,
            "Batch row count differs"
        );
        let stored = execute(
            session.as_ref(),
            &format!("SELECT id, value, payload, CASE WHEN flag THEN 1 WHEN flag IS NULL THEN NULL ELSE 0 END, generated_value FROM {target} ORDER BY id"),
        )
        .await?
        .unwrap();
        ensure!(
            stored.get_cell(0, 1).as_deref()
                == Some("quote'\\ 名; DROP TABLE unrelated;--".as_bytes()),
            "Text changed"
        );
        ensure!(
            db_wire::value::source_binary(&stored.get_cell(0, 2).unwrap(), kind)?
                == (0..=255).collect::<Vec<_>>(),
            "Binary changed"
        );
        ensure!(
            stored.get_cell(1, 1).as_deref() == Some(b""),
            "Empty text changed"
        );
        ensure!(
            db_wire::value::source_binary(&stored.get_cell(1, 2).unwrap(), kind)?.is_empty(),
            "Empty bytes changed"
        );
        ensure!(
            stored.get_cell(2, 1).is_none() && stored.get_cell(2, 2).is_none(),
            "NULL changed"
        );
        for row in 0..3 {
            ensure!(
                stored.get_cell(row, 4).as_deref() == Some(b"generated"),
                "Omitted default changed"
            );
        }
        ensure!(stored.get_cell(0, 3).as_deref() == Some(b"1"), "True changed");
        ensure!(stored.get_cell(1, 3).as_deref() == Some(b"0"), "False changed");
        ensure!(stored.get_cell(2, 3).is_none(), "NULL boolean changed");
        let bad = Batch::new(
            schema,
            &table,
            vec!["id".into()],
            vec![vec![Value::Text("4".into())], vec![Value::Text("1".into())]],
        )?;
        ensure!(
            session.insert_batch(bad).await.is_err(),
            "Duplicate key must fail"
        );
        execute(session.as_ref(), "ROLLBACK").await?;
        let (rows, _, _) = driver
            .execute_query(&format!("SELECT COUNT(*) FROM {target}"))
            .await?;
        ensure!(
            rows.get_cell(0, 0).as_deref() == Some(b"0"),
            "Failed transaction retained rows"
        );
        execute(session.as_ref(), "BEGIN").await?;
        ensure!(
            session.insert_batch(input(schema, &table)).await? == 3,
            "Batch row count differs"
        );
        execute(session.as_ref(), "COMMIT").await?;
        let (rows, _, _) = driver
            .execute_query(&format!("SELECT COUNT(*) FROM {target}"))
            .await?;
        ensure!(
            rows.get_cell(0, 0).as_deref() == Some(b"3"),
            "Commit lost rows"
        );
        Ok(())
    }
    .await;
    session.finish().await?;
    ensure!(
        session.insert_batch(input(schema, &table)).await.is_err(),
        "Closed session accepted another batch"
    );
    driver
        .execute_query(&format!("DROP TABLE {target}"))
        .await?;
    result
}

#[test]
fn sqlite_transfer_batch_roundtrips_and_rolls_back() {
    db_wire::get_runtime().block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            roundtrip(
                Arc::new(db_wire::SqliteDriver::new_memory()),
                DbDriverKind::Sqlite,
                "main",
            ),
        )
        .await
        .unwrap()
        .unwrap();
    });
}

fn config(
    provider: DbDriverKind,
    prefix: &str,
    database: &str,
    user: &str,
) -> Option<ConnectionConfig> {
    let port = match std::env::var(format!("FASTDB_TEST_{prefix}_PORT")) {
        Ok(port) => port.parse().unwrap(),
        Err(_) => {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "Missing {prefix} test port"
            );
            return None;
        }
    };
    Some(ConnectionConfig {
        driver: provider,
        host: std::env::var(format!("FASTDB_TEST_{prefix}_HOST"))
            .unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        database: std::env::var(format!("FASTDB_TEST_{prefix}_DATABASE"))
            .unwrap_or_else(|_| database.into()),
        user: std::env::var(format!("FASTDB_TEST_{prefix}_USER")).unwrap_or_else(|_| user.into()),
        password: std::env::var(format!("FASTDB_TEST_{prefix}_PASSWORD")).ok(),
        use_unix_socket: false,
        ..Default::default()
    })
}
#[test]
fn postgres_transfer_batch_roundtrips_and_rolls_back() {
    let Some(config) = config(DbDriverKind::Postgres, "PG", "postgres", "postgres") else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            roundtrip(
                Arc::new(db_wire::PostgresDriver::new(config)),
                DbDriverKind::Postgres,
                "public",
            ),
        )
        .await
        .unwrap()
        .unwrap();
    });
}
#[test]
fn mysql_transfer_batch_roundtrips_and_rolls_back() {
    let Some(config) = config(DbDriverKind::MySql, "MYSQL", "fastdb_test", "root") else {
        return;
    };
    let schema = config.database.clone();
    db_wire::get_runtime().block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            roundtrip(
                Arc::new(db_wire::MySqlDriver::new(config)),
                DbDriverKind::MySql,
                &schema,
            ),
        )
        .await
        .unwrap()
        .unwrap();
    });
}

#[test]
fn postgres_transfer_preserves_native_types_and_rejects_nul_without_truncating() {
    let Some(config) = config(DbDriverKind::Postgres, "PG", "postgres", "postgres") else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = db_wire::PostgresDriver::new(config);
        let session = driver.open_script_session().await.unwrap();
        let result: Result<()> = async {
            use db_wire::{ColumnMeta, DataType, transfer::mapping::{Mapping, ColumnMapping, Conversion}};
            execute(session.as_ref(), "CREATE TEMP TABLE native_transfer_values (amount NUMERIC(50,20), document JSON, instant TIMESTAMPTZ, flag BOOLEAN, uid UUID, note TEXT)").await?;
            execute(session.as_ref(), "BEGIN").await?;
            let columns = [("amount", DataType::Numeric), ("document", DataType::Json), ("instant", DataType::TimestampTz), ("flag", DataType::Bool), ("uid", DataType::Uuid)];
            let mapping = Mapping::new(columns.len(), columns.iter().enumerate().map(|(source, (name, kind))| ColumnMapping {
                source, target: ColumnMeta::new(*name, *kind), conversion: Conversion::Auto,
            }).collect())?;
            let amount = "123456789012345678901234567890.12345678901234567890";
            let json = r#"{"huge":1e999,"k":1,"k":2}"#;
            let instant = "2026-11-04T07:08:09+07:00";
            let uid = "00000000-0000-4000-8000-000000000001";
            let row = mapping.convert(&[amount, json, instant, "false", uid].map(|value| Value::Text(value.into())))?;
            let batch = Batch::new("pg_temp", "native_transfer_values", columns.iter().map(|(name, _)| (*name).into()).collect(), vec![row])?;
            ensure!(session.insert_batch(batch).await? == 1, "Wrong inserted count");
            let stored = execute(session.as_ref(), "SELECT amount, document, instant, flag, uid FROM native_transfer_values").await?.unwrap();
            ensure!(stored.get_cell(0, 0).as_deref() == Some(amount.as_bytes()), "Exact decimal changed");
            ensure!(stored.get_cell(0, 1).as_deref() == Some(json.as_bytes()), "JSON source changed");
            let timestamp = String::from_utf8(stored.get_cell(0, 2).unwrap())?;
            ensure!(db_wire::value::parse_timestamp_with_offset(&timestamp)? == chrono::DateTime::parse_from_rfc3339(instant)?, "Timestamp instant changed");
            ensure!(stored.get_cell(0, 3).as_deref() == Some(b"false"), "Boolean changed");
            ensure!(stored.get_cell(0, 4).as_deref() == Some(uid.as_bytes()), "UUID changed");
            let invalid = Batch::new("pg_temp", "native_transfer_values", vec!["note".into()], vec![vec![Value::Text("prefix\0suffix".into())]])?;
            let error = session.insert_batch(invalid).await.unwrap_err();
            ensure!(format!("{error:#}").contains("cannot contain NUL"), "NUL rejection reason missing");
            let count = execute(session.as_ref(), "SELECT COUNT(*) FROM native_transfer_values").await?.unwrap();
            ensure!(count.get_cell(0, 0).as_deref() == Some(b"1"), "Rejected NUL was inserted or truncated");
            execute(session.as_ref(), "ROLLBACK").await?;
            Ok(())
        }.await;
        session.finish().await.unwrap();
        result.unwrap();
    });
}

#[test]
fn mysql_transfer_preserves_exact_decimal_and_unsigned_integer() {
    let Some(config) = config(DbDriverKind::MySql, "MYSQL", "fastdb_test", "root") else {
        return;
    };
    let schema = config.database.clone();
    db_wire::get_runtime().block_on(async {
        let driver = db_wire::MySqlDriver::new(config);
        let session = driver.open_script_session().await.unwrap();
        let result: Result<()> = async {
            use db_wire::{ColumnMeta, DataType, transfer::mapping::{Mapping, ColumnMapping, Conversion}};
            execute(session.as_ref(), "CREATE TEMPORARY TABLE native_transfer_values (id BIGINT UNSIGNED PRIMARY KEY, amount DECIMAL(50,20)) ENGINE=InnoDB").await?;
            let mapping = Mapping::new(2, vec![
                ColumnMapping { source: 0, target: ColumnMeta::new("id", DataType::Int8).with_raw_type("bigint unsigned"), conversion: Conversion::Auto },
                ColumnMapping { source: 1, target: ColumnMeta::new("amount", DataType::Numeric), conversion: Conversion::Auto },
            ])?;
            let amount = "123456789012345678901234567890.12345678901234567890";
            let row = mapping.convert(&[Value::Text(u64::MAX.to_string()), Value::Text(amount.into())])?;
            execute(session.as_ref(), "BEGIN").await?;
            ensure!(session.insert_batch(Batch::new(&schema, "native_transfer_values", vec!["id".into(), "amount".into()], vec![row])?).await? == 1, "Inserted count differs");
            let stored = execute(session.as_ref(), "SELECT id, amount FROM native_transfer_values").await?.unwrap();
            ensure!(stored.get_cell(0, 0).as_deref() == Some(u64::MAX.to_string().as_bytes()), "Unsigned integer changed");
            ensure!(stored.get_cell(0, 1).as_deref() == Some(amount.as_bytes()), "Exact decimal changed");
            execute(session.as_ref(), "COMMIT").await?;
            Ok(())
        }.await;
        session.finish().await.unwrap();
        result.unwrap();
    });
}
