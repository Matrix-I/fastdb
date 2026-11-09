//! Bounded CSV import on a run-owned destination session.
use super::{
    batch::{Batch, MAX_BATCH_ROWS, MAX_PARAMETERS},
    csv::{CsvOptions, CsvReader, MAX_RECORD_BYTES},
    mapping::{Mapping, Value},
    preview::csv_values,
};
use crate::{DatabaseDriver, script::ScriptSession};
use anyhow::{Result, anyhow, ensure};
use std::{io::Read, sync::Arc, time::Duration};
use tokio::sync::watch;

mod source {
    pub trait Sealed {}
    impl Sealed for std::fs::File {}
    impl<T: AsRef<[u8]>> Sealed for std::io::Cursor<T> {}
}

/// CSV input is a regular file or an in-memory buffer. Socket and pipe readers
/// cannot participate because their blocking reads cannot be cancelled safely.
pub trait CsvSource: Read + Send + source::Sealed {
    fn validate(&self) -> Result<()>;
}

impl CsvSource for std::fs::File {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.metadata()?.is_file(),
            "CSV source must be a regular file"
        );
        Ok(())
    }
}

impl<T: AsRef<[u8]> + Send> CsvSource for std::io::Cursor<T> {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

type Record = Result<Option<(u64, Vec<Value>)>>;

const STEP_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Default)]
pub enum CommitPolicy {
    #[default]
    Atomic,
    PerBatch,
}

pub struct ImportSettings {
    pub schema: String,
    pub table: String,
    pub expected_object_id: Option<String>,
    pub csv: CsvOptions,
    pub mapping: Mapping,
    pub commit: CommitPolicy,
    pub skip_invalid_values: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportProgress {
    pub processed: u64,
    pub rejected: u64,
    pub committed: u64,
    pub committed_batches: u64,
}

#[derive(Debug, Default)]
pub struct ImportOutcome {
    pub progress: ImportProgress,
    pub error: Option<anyhow::Error>,
    pub cleanup_error: Option<anyhow::Error>,
    pub cancelled: bool,
    pub commit_unknown: bool,
}

pub struct ImportControl {
    cancel: watch::Sender<bool>,
    progress: watch::Sender<ImportProgress>,
}

impl Default for ImportControl {
    fn default() -> Self {
        Self {
            cancel: watch::channel(false).0,
            progress: watch::channel(ImportProgress::default()).0,
        }
    }
}

impl ImportControl {
    pub fn cancel(&self) {
        self.cancel.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    pub fn progress(&self) -> watch::Receiver<ImportProgress> {
        self.progress.subscribe()
    }

    async fn cancelled(&self) {
        let mut state = self.cancel.subscribe();
        let _ = state.wait_for(|cancelled| *cancelled).await;
    }

    async fn step<T>(&self, work: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::select! {
            biased;
            _ = self.cancelled() => Err(anyhow!("Import cancelled")),
            result = tokio::time::timeout(STEP_TIMEOUT, work) => result.map_err(|_| anyhow!("Import step timed out"))?,
        }
    }
}

/// The destination must acknowledge closure before the returned outcome is final.
pub async fn run_csv_import(
    driver: Arc<dyn DatabaseDriver>,
    input: impl CsvSource + 'static,
    settings: ImportSettings,
    control: Arc<ImportControl>,
) -> ImportOutcome {
    match crate::run_on_shared_runtime(async move {
        Ok(run_import(driver, input, settings, control).await)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => ImportOutcome {
            error: Some(error),
            ..Default::default()
        },
    }
}

/// Transfer rows from a source database query to a destination table on a run-owned session.
pub async fn run_table_transfer(
    source_driver: Arc<dyn DatabaseDriver>,
    source_sql: String,
    dest_driver: Arc<dyn DatabaseDriver>,
    settings: ImportSettings,
    control: Arc<ImportControl>,
) -> ImportOutcome {
    match crate::run_on_shared_runtime(async move {
        Ok(run_transfer(source_driver, source_sql, dest_driver, settings, control).await)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => ImportOutcome {
            error: Some(error),
            ..Default::default()
        },
    }
}

async fn drain_stream_rows(
    pager: &Option<Arc<mem_engine::QueryResultPager>>,
    total_rows: u64,
    read_rows: &mut u64,
    record_num: &mut u64,
    cancel: &ImportControl,
    sender: &flume::Sender<Record>,
) -> Result<()> {
    if let Some(p) = pager {
        let num_cols = p.num_cols() as usize;
        while *read_rows < total_rows {
            if *cancel.cancel.borrow() {
                break;
            }
            *record_num += 1;
            let mut values = Vec::with_capacity(num_cols);
            for c in 0..num_cols {
                let val = match p.get_cell(*read_rows, c) {
                    None => Value::Null,
                    Some(bytes) => match String::from_utf8(bytes) {
                        Ok(s) => Value::Text(s),
                        Err(e) => Value::Binary(e.into_bytes()),
                    },
                };
                values.push(val);
            }
            *read_rows += 1;
            if sender
                .send_async(Ok(Some((*record_num, values))))
                .await
                .is_err()
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn run_transfer(
    source_driver: Arc<dyn DatabaseDriver>,
    source_sql: String,
    dest_driver: Arc<dyn DatabaseDriver>,
    settings: ImportSettings,
    control: Arc<ImportControl>,
) -> ImportOutcome {
    let mut outcome = ImportOutcome::default();
    if dest_driver.is_read_only() || !dest_driver.supports_transfer() {
        outcome.error = Some(anyhow!("Destination does not support transfer writes"));
        return outcome;
    }
    let session = match control.step(dest_driver.open_script_session()).await {
        Ok(session) => SessionGuard(session),
        Err(error) => {
            outcome.cancelled = *control.cancel.borrow();
            outcome.error = Some(error);
            return outcome;
        }
    };
    let (sender, records) = flume::bounded(1);
    let cancel = control.clone();
    let reader = crate::get_runtime().spawn(async move {
        let (event_tx, event_rx) = flume::bounded(32);
        let query_task = tokio::spawn({
            let source_driver = source_driver.clone();
            let source_sql = source_sql.clone();
            async move { source_driver.execute_streaming(&source_sql, event_tx).await }
        });
        let mut pager: Option<Arc<mem_engine::QueryResultPager>> = None;
        let mut read_rows: u64 = 0;
        let mut record_num: u64 = 0;
        while let Ok(event) = event_rx.recv_async().await {
            if *cancel.cancel.borrow() {
                query_task.abort();
                break;
            }
            match event {
                crate::StreamEvent::SchemaReady(p) => {
                    pager = Some(p);
                }
                crate::StreamEvent::PageAppended { total_rows } => {
                    drain_stream_rows(
                        &pager,
                        total_rows,
                        &mut read_rows,
                        &mut record_num,
                        &cancel,
                        &sender,
                    )
                    .await?;
                }
                crate::StreamEvent::Completed { total_rows, .. } => {
                    drain_stream_rows(
                        &pager,
                        total_rows,
                        &mut read_rows,
                        &mut record_num,
                        &cancel,
                        &sender,
                    )
                    .await?;
                    break;
                }
                crate::StreamEvent::Error(err) => {
                    let _ = sender.send_async(Err(anyhow!(err))).await;
                    return Ok(());
                }
                _ => {}
            }
        }
        let _ = query_task.await;
        let _ = sender.send_async(Ok(None)).await;
        Ok::<(), anyhow::Error>(())
    });

    if let Err(error) = import(
        session.0.as_ref(),
        &records,
        settings,
        &control,
        &mut outcome,
    )
    .await
    {
        outcome.error = Some(error);
    }
    drop(records);
    control.progress.send_replace(outcome.progress);
    outcome.cancelled = *control.cancel.borrow();
    outcome.cleanup_error = match tokio::time::timeout(CLEANUP_TIMEOUT, session.0.finish()).await {
        Ok(result) => result.err(),
        Err(_) => Some(anyhow!("Transfer session cleanup timed out")),
    };
    let reader_error = match tokio::time::timeout(CLEANUP_TIMEOUT, reader).await {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(error))) => Some(anyhow!("Source reader error: {error}")),
        Ok(Err(join_err)) => Some(anyhow!("Source reader task failed: {join_err}")),
        Err(_) => Some(anyhow!("Source reader cleanup timed out")),
    };
    if outcome.cleanup_error.is_none() {
        outcome.cleanup_error = reader_error;
    }
    outcome
}

async fn run_import(
    driver: Arc<dyn DatabaseDriver>,
    input: impl CsvSource + 'static,
    settings: ImportSettings,
    control: Arc<ImportControl>,
) -> ImportOutcome {
    let mut outcome = ImportOutcome::default();
    if driver.is_read_only() || !driver.supports_transfer() {
        outcome.error = Some(anyhow!("Destination does not support transfer writes"));
        return outcome;
    }
    let session = match control.step(driver.open_script_session()).await {
        Ok(session) => SessionGuard(session),
        Err(error) => {
            outcome.cancelled = *control.cancel.borrow();
            outcome.error = Some(error);
            return outcome;
        }
    };
    let (sender, records) = flume::bounded(1);
    let options = settings.csv.clone();
    let reader = crate::get_runtime().spawn_blocking(move || {
        let read = || -> Result<()> {
            input.validate()?;
            let mut csv = CsvReader::new(input, options)?;
            while let Some(fields) = csv.next_record()? {
                if sender
                    .send(Ok(Some((csv.record_number(), csv_values(fields)))))
                    .is_err()
                {
                    return Ok(());
                }
            }
            let _ = sender.send(Ok(None));
            Ok(())
        };
        if let Err(error) = read() {
            let _ = sender.send(Err(error));
        }
    });
    if let Err(error) = import(
        session.0.as_ref(),
        &records,
        settings,
        &control,
        &mut outcome,
    )
    .await
    {
        outcome.error = Some(error);
    }
    drop(records);
    control.progress.send_replace(outcome.progress);
    outcome.cancelled = *control.cancel.borrow();
    outcome.cleanup_error = match tokio::time::timeout(CLEANUP_TIMEOUT, session.0.finish()).await {
        Ok(result) => result.err(),
        Err(_) => Some(anyhow!("Import session cleanup timed out")),
    };
    // Dropping the receiver releases a producer waiting on backpressure.
    let reader_error = match tokio::time::timeout(CLEANUP_TIMEOUT, reader).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(anyhow!("CSV reader worker failed: {error}")),
        Err(_) => Some(anyhow!("CSV reader cleanup timed out")),
    };
    if outcome.cleanup_error.is_none() {
        outcome.cleanup_error = reader_error;
    }
    outcome
}

struct SessionGuard(Arc<dyn ScriptSession>);
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn import(
    session: &dyn ScriptSession,
    records: &flume::Receiver<Record>,
    settings: ImportSettings,
    control: &ImportControl,
    outcome: &mut ImportOutcome,
) -> Result<()> {
    let columns = settings
        .mapping
        .columns()
        .iter()
        .map(|column| column.target.name.clone())
        .collect::<Vec<_>>();
    let row_limit = MAX_BATCH_ROWS.min(MAX_PARAMETERS / columns.len());
    let mut rows = Vec::with_capacity(row_limit);
    let mut bytes = 0;
    let mut pending = (0, 0);
    loop {
        let next = control.step(async { records.recv_async().await? }).await?;
        let finished = next.is_none();
        let converted = if let Some((record, fields)) = next {
            outcome.progress.processed += 1;
            match settings.mapping.convert(&fields) {
                Ok(row) => Some(row),
                Err(_) if settings.skip_invalid_values => {
                    outcome.progress.rejected += 1;
                    None
                }
                Err(error) => return Err(error.context(format!("CSV record {record}"))),
            }
        } else {
            None
        };
        let size = converted
            .as_ref()
            .map_or(0, |row| row.iter().map(Value::byte_len).sum::<usize>());
        if !rows.is_empty() && (finished || bytes + size > MAX_RECORD_BYTES) {
            write_batch(
                session,
                &settings,
                &columns,
                std::mem::take(&mut rows),
                control,
                outcome,
                &mut pending,
            )
            .await?;
            bytes = 0;
        }
        if let Some(row) = converted {
            rows.push(row);
            bytes += size;
            if rows.len() == row_limit {
                write_batch(
                    session,
                    &settings,
                    &columns,
                    std::mem::take(&mut rows),
                    control,
                    outcome,
                    &mut pending,
                )
                .await?;
                bytes = 0;
            }
        }
        control.progress.send_replace(outcome.progress);
        if finished {
            if pending.1 == 0 && outcome.progress.committed_batches == 0 {
                control
                    .step(
                        session.begin_transfer(
                            &settings.schema,
                            &settings.table,
                            settings.expected_object_id.as_deref(),
                            &settings
                                .mapping
                                .columns()
                                .iter()
                                .map(|mapping| mapping.target.clone())
                                .collect::<Vec<_>>(),
                        ),
                    )
                    .await?;
            }
            if pending.1 != 0 {
                commit(session, control, outcome, pending).await?;
            }
            return Ok(());
        }
    }
}

async fn write_batch(
    session: &dyn ScriptSession,
    settings: &ImportSettings,
    columns: &[String],
    rows: Vec<Vec<Value>>,
    control: &ImportControl,
    outcome: &mut ImportOutcome,
    pending: &mut (u64, u64),
) -> Result<()> {
    let batch = Batch::new(&settings.schema, &settings.table, columns.to_vec(), rows)?;
    if pending.1 == 0 {
        control
            .step(
                session.begin_transfer(
                    &settings.schema,
                    &settings.table,
                    settings.expected_object_id.as_deref(),
                    &settings
                        .mapping
                        .columns()
                        .iter()
                        .map(|mapping| mapping.target.clone())
                        .collect::<Vec<_>>(),
                ),
            )
            .await?;
    }
    let expected = batch.row_count();
    let inserted = control.step(session.insert_batch(batch)).await?;
    ensure!(
        inserted == expected,
        "Destination inserted {inserted} of {expected} rows"
    );
    pending.0 += inserted;
    pending.1 += 1;
    if matches!(settings.commit, CommitPolicy::PerBatch) {
        commit(session, control, outcome, *pending).await?;
        *pending = (0, 0);
    }
    Ok(())
}

async fn commit(
    session: &dyn ScriptSession,
    control: &ImportControl,
    outcome: &mut ImportOutcome,
    pending: (u64, u64),
) -> Result<()> {
    ensure!(!*control.cancel.borrow(), "Import cancelled before commit");
    outcome.commit_unknown = true;
    let (events, receiver) = flume::bounded(8);
    tokio::time::timeout(STEP_TIMEOUT, async {
        let (result, ()) = tokio::join!(session.execute_streaming("COMMIT", events), async {
            while receiver.recv_async().await.is_ok() {}
        });
        result
    })
    .await
    .map_err(|_| anyhow!("Commit acknowledgement timed out"))??;
    outcome.commit_unknown = false;
    outcome.progress.committed += pending.0;
    outcome.progress.committed_batches += pending.1;
    control.progress.send_replace(outcome.progress);
    Ok(())
}
