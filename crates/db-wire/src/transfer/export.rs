//! Full-source streaming export to CSV or SQL.
use super::csv::{CsvOptions, CsvWriter};
use crate::{DatabaseDriver, StreamEvent, sql::SqlDialect};
use anyhow::{Result, ensure};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullExportFormat {
    Csv,
    Sql,
}

pub struct FullExportSettings {
    pub format: FullExportFormat,
    pub csv_options: CsvOptions,
    pub target_table: String,
    pub dialect: SqlDialect,
}

#[allow(clippy::too_many_arguments)]
fn drain_rows(
    pager: &Option<Arc<mem_engine::QueryResultPager>>,
    total_rows: u64,
    read_rows: &mut u64,
    settings: &FullExportSettings,
    col_list: &str,
    csv_writer: &mut Option<CsvWriter<std::fs::File>>,
    sql_writer: &mut BufWriter<std::fs::File>,
    cancel: &watch::Receiver<bool>,
    progress: &watch::Sender<u64>,
) -> Result<()> {
    if let Some(p) = pager {
        let num_cols = p.num_cols() as usize;
        while *read_rows < total_rows {
            if *cancel.borrow() {
                anyhow::bail!("Export cancelled");
            }
            match settings.format {
                FullExportFormat::Csv => {
                    let mut strings = Vec::with_capacity(num_cols);
                    for c in 0..num_cols {
                        if let Some(cell) = p.get_cell(*read_rows, c) {
                            let s = match String::from_utf8(cell) {
                                Ok(s) => s,
                                Err(e) => crate::value::hex_text(&e.into_bytes()),
                            };
                            strings.push(Some(s));
                        } else {
                            strings.push(None);
                        }
                    }
                    let record_cells: Vec<_> = strings.iter().map(|s| s.as_deref()).collect();
                    if let Some(cw) = csv_writer {
                        cw.write_record(&record_cells)?;
                    }
                }
                FullExportFormat::Sql => {
                    writeln!(
                        sql_writer,
                        "INSERT INTO {} ({}) VALUES (",
                        settings.target_table, col_list
                    )?;
                    for c in 0..num_cols {
                        if c > 0 {
                            sql_writer.write_all(b", ")?;
                        }
                        if let Some(cell) = p.get_cell(*read_rows, c) {
                            if let Ok(s) = std::str::from_utf8(&cell) {
                                write!(sql_writer, "{}", settings.dialect.literal(Some(s)))?;
                            } else {
                                let bin = crate::value::binary_literal(&cell, settings.dialect.0);
                                write!(sql_writer, "{bin}")?;
                            }
                        } else {
                            sql_writer.write_all(b"NULL")?;
                        }
                    }
                    writeln!(sql_writer, ");")?;
                }
            }
            *read_rows += 1;
            progress.send_replace(*read_rows);
        }
    }
    Ok(())
}

struct AbortOnDrop(Option<tokio::task::JoinHandle<anyhow::Result<()>>>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Streams all rows produced by an explicit query into an atomically replaced CSV or SQL file.
pub async fn run_full_export(
    driver: Arc<dyn DatabaseDriver>,
    sql: String,
    output_path: &Path,
    settings: FullExportSettings,
    cancel: watch::Receiver<bool>,
    progress: watch::Sender<u64>,
) -> Result<u64> {
    let output_path = output_path.to_path_buf();
    crate::run_on_shared_runtime(async move {
        ensure!(!*cancel.borrow(), "Export cancelled before start");
        let parent = output_path.parent().unwrap_or(Path::new("."));
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        let file_clone = temporary.as_file().try_clone()?;
        let mut sql_writer = BufWriter::new(file_clone);
        let (event_tx, event_rx) = flume::bounded(32);

        let query_task = tokio::spawn({
            let driver = driver.clone();
            let sql = sql.clone();
            async move { driver.execute_streaming(&sql, event_tx).await }
        });
        let mut query_task_guard = AbortOnDrop(Some(query_task));

        let mut pager: Option<Arc<mem_engine::QueryResultPager>> = None;
        let mut read_rows: u64 = 0;
        let mut csv_writer: Option<CsvWriter<std::fs::File>> = None;
        let mut col_list: String = String::new();

        while let Ok(event) = event_rx.recv_async().await {
            if *cancel.borrow() {
                anyhow::bail!("Export cancelled");
            }
            match event {
                StreamEvent::SchemaReady(p) => {
                    let cols = p.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
                    if settings.format == FullExportFormat::Csv {
                        let file = temporary.as_file().try_clone()?;
                        csv_writer =
                            Some(CsvWriter::new(file, settings.csv_options.clone(), &cols)?);
                    } else {
                        let col_names: Vec<String> = cols
                            .iter()
                            .map(|n| settings.dialect.quote_ident(n))
                            .collect();
                        col_list = col_names.join(", ");
                    }
                    pager = Some(p);
                }
                StreamEvent::PageAppended { total_rows } => {
                    drain_rows(
                        &pager,
                        total_rows,
                        &mut read_rows,
                        &settings,
                        &col_list,
                        &mut csv_writer,
                        &mut sql_writer,
                        &cancel,
                        &progress,
                    )?;
                }
                StreamEvent::Completed { total_rows, .. } => {
                    drain_rows(
                        &pager,
                        total_rows,
                        &mut read_rows,
                        &settings,
                        &col_list,
                        &mut csv_writer,
                        &mut sql_writer,
                        &cancel,
                        &progress,
                    )?;
                    break;
                }
                StreamEvent::Error(err) => {
                    anyhow::bail!("{err}");
                }
                _ => {}
            }
        }

        let query_res = query_task_guard.0.take().unwrap().await;
        if let Ok(Err(err)) = query_res {
            anyhow::bail!("Source query failed: {err}");
        }

        if let Some(cw) = csv_writer {
            cw.finish()?;
        } else {
            sql_writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        temporary.persist(output_path)?;
        Ok(read_rows)
    })
    .await
}
