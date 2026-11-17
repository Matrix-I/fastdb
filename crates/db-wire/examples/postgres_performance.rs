//! Release measurements against the disposable P2-10 fixture.
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use db_wire::{ConnectionConfig, DatabaseDriver, PostgresDriver, StreamEvent};
use serde_json::{Value, json};

const MIXED_SQL: &str = include_str!("../../../benchmarks/postgres/mixed.sql");
const LARGE_LOB_SQL: &str = include_str!("../../../benchmarks/postgres/large-lob.sql");

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

async fn measure(config: ConnectionConfig) -> Result<Value> {
    let driver = PostgresDriver::new(config);
    let start = Instant::now();
    driver.ping().await?;
    let connect_ms = milliseconds(start);

    let start = Instant::now();
    let tables = driver.list_tables().await?;
    let (columns, constraints, indexes, details, foreign_keys) = tokio::try_join!(
        driver.list_all_columns(),
        driver.list_all_constraints(),
        driver.list_all_indexes(),
        driver.list_table_details(),
        driver.list_foreign_keys(),
    )?;
    let catalog_ms = milliseconds(start);
    ensure!(
        tables.len() == 2_000
            && columns.len() == 2_000
            && columns.iter().all(|(_, _, columns)| columns.len() == 5)
            && constraints
                .iter()
                .filter(|(_, _, _, kind)| kind == "PRIMARY KEY")
                .count()
                == 2_000
            && indexes.len() == 2_000
            && details.len() == 2_000
            && foreign_keys.is_some_and(|keys| keys.is_empty()),
        "unexpected catalog fixture shape"
    );

    let (send, receive) = flume::unbounded();
    let start = Instant::now();
    let collect = async {
        let mut first_batch_ms = None;
        let mut pager = None;
        let mut rows = None;
        while let Ok(event) = receive.recv_async().await {
            match event {
                StreamEvent::SchemaReady(result) => pager = Some(result),
                StreamEvent::PageAppended { .. } => {
                    first_batch_ms.get_or_insert_with(|| milliseconds(start));
                }
                StreamEvent::Completed { total_rows, .. } => rows = Some(total_rows),
                StreamEvent::Error(error) => anyhow::bail!("{error}"),
                StreamEvent::CommandComplete { .. } | StreamEvent::Notice(_) => {}
            }
        }
        Ok::<_, anyhow::Error>((first_batch_ms, pager, rows))
    };
    let (_, (first_batch_ms, pager, rows)) =
        tokio::try_join!(driver.execute_streaming(MIXED_SQL, send), collect)?;
    let stream_ms = milliseconds(start);
    let pager = pager.context("missing result pager")?;
    ensure!(
        rows == Some(20_000) && pager.total_rows() == 20_000 && pager.num_cols() == 3,
        "unexpected result shape"
    );
    let start = Instant::now();
    let expected_blob = format!("\\x{}", "deadbeef".repeat(5_000));
    ensure!(
        pager.get_cell(0, 0).as_deref() == Some(b"1")
            && pager.get_cell(0, 1).as_deref() == Some(vec![b'L'; 20_000].as_slice())
            && pager.get_cell(0, 2).as_deref() == Some(expected_blob.as_bytes())
            && pager.get_cell(12, 1).is_none()
            && pager.get_cell(19_999, 0).as_deref() == Some(b"20000"),
        "unexpected mixed-width result values"
    );
    Ok(json!({
        "connect_ms": connect_ms,
        "catalog_ms": catalog_ms,
        "first_batch_ms": first_batch_ms.context("missing first batch")?,
        "stream_ms": stream_ms,
        "read_back_ms": milliseconds(start),
    }))
}

async fn oversized_lob(config: ConnectionConfig) -> Result<Value> {
    let driver = PostgresDriver::new(config);
    match driver.execute_query(LARGE_LOB_SQL).await {
        Ok((pager, _, _)) => {
            let expected_blob = format!("\\x{}", "deadbeef".repeat(75_000));
            ensure!(
                pager.total_rows() == 1
                    && pager.get_cell(0, 0).as_deref() == Some(vec![b'L'; 300_000].as_slice())
                    && pager.get_cell(0, 1).as_deref() == Some(expected_blob.as_bytes()),
                "oversized LOB returned incomplete values"
            );
            Ok(json!({"status": "passed"}))
        }
        Err(error) if error.to_string().contains("maximum page capacity") => {
            Ok(json!({"status": "rejected", "error": error.to_string()}))
        }
        Err(error) => Err(error),
    }
}

fn main() -> Result<()> {
    let config = ConnectionConfig {
        host: "127.0.0.1".into(),
        port: std::env::var("PGPORT")
            .context("set PGPORT to the fixture port")?
            .parse()?,
        database: "fastdb_perf".into(),
        password: Some(std::env::var("PGPASSWORD").context("set the fixture password")?),
        use_unix_socket: false,
        ..Default::default()
    };
    db_wire::get_runtime().block_on(async {
        let mut samples = Vec::with_capacity(5);
        for run in 0..=5 {
            let sample = tokio::time::timeout(Duration::from_secs(60), measure(config.clone()))
                .await
                .with_context(|| format!("measurement {run} timed out"))??;
            if run > 0 {
                samples.push(sample);
            }
        }
        let oversized = tokio::time::timeout(Duration::from_secs(15), oversized_lob(config))
            .await
            .context("oversized LOB probe timed out")??;
        let printout = json!({ "samples": samples, "oversized_lob": oversized });
        println!("{}", serde_json::to_string_pretty(&printout)?);
        ensure!(
            oversized["status"] == "passed",
            "oversized LOB probe rejected"
        );
        Ok(())
    })
}
