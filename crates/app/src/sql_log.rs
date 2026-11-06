//! App-session SQL records live on disk. Readers keep at most one bounded page.
use db_wire::sql_observer::{Registration, SqlObserver, SqlRecord};
use serde::{Deserialize, Serialize};
use std::fs::{File, Permissions, TryLockError};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(crate) const PAGE_SIZE: usize = 100;
const QUEUE_CAPACITY: usize = 64;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PREVIEW_CHARS: usize = 2000;
const FILE_PREFIX: &str = "fastdb-sql-session-";
const FILE_SUFFIX: &str = ".jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub time: String,
    pub kind: String,
    pub session: String,
    pub sql: String,
    pub elapsed_ms: u64,
    pub rows: Option<u64>,
    pub result: String,
}
impl Entry {
    pub fn duration_text(&self) -> String {
        format!("{}.{:03}s", self.elapsed_ms / 1000, self.elapsed_ms % 1000)
    }

    pub fn rows_text(&self) -> String {
        self.rows.map_or_else(String::new, |rows| rows.to_string())
    }
}
impl From<SqlRecord> for Entry {
    fn from(record: SqlRecord) -> Self {
        Self {
            time: record.started_at.format("%b-%d %H:%M:%S").to_string(),
            kind: record.kind.into(),
            session: record.session,
            sql: record.sql,
            elapsed_ms: record.elapsed_ms,
            rows: record.rows,
            result: record.result,
        }
    }
}

#[derive(Default)]
pub(crate) struct Page {
    pub entries: Vec<Entry>,
    pub next: Option<u64>,
}

enum Request {
    Record(Entry),
    Snapshot(flume::Sender<Result<(File, u64), String>>),
    Stop,
}
struct Sink {
    sender: flume::Sender<Request>,
    error: Arc<Mutex<Option<String>>>,
    closed: Arc<AtomicBool>,
}
impl SqlObserver for Sink {
    fn record(&self, record: SqlRecord) {
        if !self.closed.load(Ordering::Acquire)
            && self.sender.send(Request::Record(record.into())).is_err()
        {
            *self.error.lock().unwrap() = Some("SQL log writer stopped".into());
        }
    }
}

fn cleanup_stale_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(FILE_PREFIX) || !name.ends_with(FILE_SUFFIX) {
            continue;
        }
        let Ok(candidate) = File::open(&path) else {
            continue;
        };
        match candidate.try_lock() {
            Ok(()) => {
                let _ = std::fs::remove_file(&path);
            }
            Err(TryLockError::WouldBlock | TryLockError::Error(_)) => {}
        }
    }
}

pub(crate) struct SqlLog {
    sink: Arc<Sink>,
    registration: Mutex<Option<Registration>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    #[cfg(test)]
    path: Option<PathBuf>,
}
impl SqlLog {
    pub fn new() -> anyhow::Result<Arc<Self>> {
        let dir = platform_hal::paths::ensure_spill_dir().unwrap_or_else(|_| std::env::temp_dir());
        Self::new_in(&dir)
    }

    pub fn new_in(dir: &Path) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        cleanup_stale_files(dir);
        let (file, path) = tempfile::Builder::new()
            .prefix(FILE_PREFIX)
            .suffix(FILE_SUFFIX)
            .permissions(Permissions::from_mode(0o600))
            .tempfile_in(dir)?
            .keep()?;
        file.try_lock()
            .map_err(|_| anyhow::anyhow!("Cannot lock SQL session log"))?;
        Self::from_file(file, Some(path))
    }

    fn from_file(file: File, path: Option<PathBuf>) -> anyhow::Result<Arc<Self>> {
        #[cfg(test)]
        let test_path = path.clone();
        let (sender, receiver) = flume::bounded(QUEUE_CAPACITY);
        let error = Arc::new(Mutex::new(None));
        let sink = Arc::new(Sink {
            sender,
            error: error.clone(),
            closed: Arc::new(AtomicBool::new(false)),
        });
        let worker = std::thread::Builder::new()
            .name("fastdb-sql-log".into())
            .spawn(move || {
                let mut file = file;
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Record(entry) => {
                            let start = file.stream_position();
                            let result = (|| -> anyhow::Result<()> {
                                serde_json::to_writer(&mut file, &entry)?;
                                file.write_all(b"\n")?;
                                Ok(())
                            })();
                            if result.is_err() {
                                // Restore the last complete line after a partial disk write.
                                if let Ok(start) = start {
                                    let _ = file.set_len(start).and_then(|()| {
                                        file.seek(SeekFrom::Start(start)).map(|_| ())
                                    });
                                }
                                *error.lock().unwrap() = Some(
                                    "Cannot write SQL log; some statements were not recorded"
                                        .into(),
                                );
                            }
                        }
                        Request::Snapshot(reply) => {
                            let result = (|| -> std::io::Result<_> {
                                file.flush()?;
                                Ok((file.try_clone()?, file.metadata()?.len()))
                            })()
                            .map_err(|_| "Cannot read SQL log".to_string());
                            let _ = reply.send(result);
                        }
                        Request::Stop => break,
                    }
                }
                if let Some(path) = path {
                    let _ = std::fs::remove_file(path);
                }
                drop(file);
            })?;
        Ok(Arc::new(Self {
            sink,
            registration: Mutex::new(None),
            worker: Mutex::new(Some(worker)),
            #[cfg(test)]
            path: test_path,
        }))
    }
    pub fn install(&self) -> anyhow::Result<()> {
        *self.registration.lock().unwrap() =
            Some(db_wire::sql_observer::install(self.sink.clone())?);
        Ok(())
    }
    pub fn error(&self) -> Option<String> {
        self.sink.error.lock().unwrap().clone()
    }
    pub fn shutdown(&self) {
        if self.sink.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.registration.lock().unwrap().take();
        let _ = self.sink.sender.send(Request::Stop);
        if let Some(worker) = self.worker.lock().unwrap().take()
            && worker.join().is_err()
        {
            *self.sink.error.lock().unwrap() = Some("SQL log writer failed during shutdown".into());
        }
    }
    pub fn page(&self, offset: u64, query: &str, cancel: &AtomicBool) -> Result<Page, String> {
        if self.sink.closed.load(Ordering::Acquire) {
            return Err("SQL log is closed".into());
        }
        let (reply, receiver) = flume::bounded(1);
        self.sink
            .sender
            .send(Request::Snapshot(reply))
            .map_err(|_| "SQL log writer stopped")?;
        let (file, length) = receiver.recv().map_err(|_| "SQL log writer stopped")??;
        // Positional reads never change the cursor shared by cloned writer handles.
        let mut reader = BufReader::new(Snapshot {
            file,
            position: offset.min(length),
            length,
        });
        let mut page = Page::default();
        let needle = query.to_lowercase();
        let mut position = offset;
        let mut bytes = 0;
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Search cancelled".into());
            }
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|_| "Cannot read SQL log")?;
            if read == 0 {
                break;
            }
            let mut entry: Entry =
                serde_json::from_str(&line).map_err(|_| "SQL log contains an incomplete record")?;
            if entry.sql.to_lowercase().contains(&needle) {
                // Rendering remains bounded even for a multi-megabyte SQL statement.
                entry.sql = preview(&entry.sql);
                entry.result = preview(&entry.result);
                let entry_bytes = entry.sql.len() + entry.result.len();
                if page.entries.len() == PAGE_SIZE || bytes + entry_bytes > MAX_PAGE_BYTES {
                    page.next = Some(position);
                    break;
                }
                bytes += entry_bytes;
                page.entries.push(entry);
            }
            position += read as u64;
        }
        Ok(page)
    }
}
struct Snapshot {
    file: File,
    position: u64,
    length: u64,
}
impl Read for Snapshot {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = buffer
            .len()
            .min(self.length.saturating_sub(self.position) as usize);
        let read = self.file.read_at(&mut buffer[..count], self.position)?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Drop for SqlLog {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let mut preview: String = chars.by_ref().take(MAX_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        preview.push_str("… [preview]");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(sql: &str) -> SqlRecord {
        SqlRecord {
            started_at: chrono::DateTime::from_timestamp(1_793_979_058, 0).unwrap(),
            provider: "PostgreSQL",
            session: "PostgreSQL#1".into(),
            kind: db_wire::sql_observer::classify_sql(sql),
            sql: sql.into(),
            elapsed_ms: 53,
            rows: Some(19),
            result: "Success".into(),
        }
    }

    #[test]
    fn formats_demo_row_matching_transaction_log_columns() {
        let entry = Entry::from(record(r#"SELECT cs.* FROM public."componentScore" AS cs"#));
        assert_eq!(entry.time, "Nov-06 15:30:58");
        assert_eq!(entry.kind, "SQL / User");
        assert_eq!(
            entry.sql,
            r#"SELECT cs.* FROM public."componentScore" AS cs"#
        );
        assert_eq!(entry.duration_text(), "0.053s");
        assert_eq!(entry.rows_text(), "19");
        assert_eq!(entry.result, "Success");
    }

    #[test]
    fn disk_paging_search_and_shutdown_preserve_session_only_records() {
        let dir = tempfile::tempdir().unwrap();
        let log = SqlLog::new_in(dir.path()).unwrap();
        let path = log.path.clone().unwrap();
        assert!(path.exists());
        for i in 0..205 {
            log.sink.record(record(&format!("SELECT '{i} é'")));
        }
        let cancel = AtomicBool::new(false);
        let first = log.page(0, "", &cancel).unwrap();
        assert_eq!(first.entries.len(), PAGE_SIZE);
        assert_eq!(first.entries[0].sql, "SELECT '0 é'");
        let next = first.next.unwrap();
        drop(first);
        assert_eq!(
            log.page(next, "", &cancel).unwrap().entries[0].sql,
            "SELECT '100 é'"
        );
        let found = log.page(0, "204 É", &cancel).unwrap();
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].elapsed_ms, 53);
        assert_eq!(found.entries[0].duration_text(), "0.053s");
        assert_eq!(found.entries[0].kind, "SQL / User");
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(log.page(0, "", &cancel).err().unwrap(), "Search cancelled");
        log.shutdown();
        assert!(!path.exists());
        assert!(log.worker.lock().unwrap().is_none());
        assert!(log.page(0, "", &AtomicBool::new(false)).is_err());
        let fresh = SqlLog::new_in(dir.path()).unwrap();
        assert!(
            fresh
                .page(0, "", &AtomicBool::new(false))
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn startup_removes_stale_crash_files_without_touching_active_process_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let stale = dir
            .path()
            .join(format!("{FILE_PREFIX}stale-crashed{FILE_SUFFIX}"));
        std::fs::write(&stale, b"{\"corrupt\":true}\n").unwrap();
        let unrelated = dir.path().join("other-file.txt");
        std::fs::write(&unrelated, b"keep").unwrap();

        let active_one = SqlLog::new_in(dir.path()).unwrap();
        let path_one = active_one.path.clone().unwrap();
        assert!(!stale.exists(), "stale crash file should be removed");
        assert!(unrelated.exists(), "unrelated file must not be removed");
        assert!(path_one.exists());
        assert_eq!(
            std::fs::metadata(&path_one).unwrap().permissions().mode() & 0o777,
            0o600
        );
        active_one.sink.record(record("SELECT 1"));

        // A second process starting in the same directory must not remove active_one's file.
        let active_two = SqlLog::new_in(dir.path()).unwrap();
        let path_two = active_two.path.clone().unwrap();
        assert!(path_one.exists(), "running process file must remain intact");
        assert!(path_two.exists());
        assert_eq!(
            active_one
                .page(0, "", &AtomicBool::new(false))
                .unwrap()
                .entries
                .len(),
            1
        );

        active_one.shutdown();
        assert!(!path_one.exists());
        assert!(path_two.exists());
        active_two.shutdown();
        assert!(!path_two.exists());
    }

    #[test]
    fn real_sqlite_operations_reach_disk_without_retaining_skipped_statements() {
        use db_wire::DatabaseDriver;
        let dir = tempfile::tempdir().unwrap();
        let log = SqlLog::new_in(dir.path()).unwrap();
        log.install().unwrap();
        let driver = Arc::new(db_wire::SqliteDriver::new_memory());
        db_wire::get_runtime().block_on(async {
            driver
                .execute_schema_ddl("CREATE TABLE sql_log_acceptance (id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            driver
                .apply_row_changes(&["INSERT INTO sql_log_acceptance VALUES (1)".into()])
                .await
                .unwrap();
            driver
                .execute_query("SELECT * FROM sql_log_acceptance")
                .await
                .unwrap();
            driver
                .list_columns("main", "sql_log_acceptance")
                .await
                .unwrap();
            let (tx, rx) = flume::unbounded();
            db_wire::script::run_script(
                driver,
                vec![
                    "SELECT missing_column FROM sql_log_acceptance".into(),
                    "SELECT 999 FROM sql_log_acceptance /* skipped */".into(),
                ],
                Arc::new(db_wire::script::ScriptControl::default()),
                tx,
            )
            .await;
            assert!(rx.drain().any(|event| matches!(event,
                db_wire::script::ScriptEvent::Finished(outcome) if outcome.error.is_some())));
        });
        let page = log
            .page(0, "sql_log_acceptance", &AtomicBool::new(false))
            .unwrap();
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.sql.starts_with("CREATE TABLE")
                    && entry.kind == "SQL / Schema"
                    && entry.result == "Success")
        );
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.sql.starts_with("INSERT INTO")
                    && entry.kind == "SQL / User"
                    && entry.rows == Some(1))
        );
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.sql.starts_with("SELECT *")
                    && entry.kind == "SQL / User"
                    && entry.rows == Some(1))
        );
        assert!(page.entries.iter().any(
            |entry| entry.sql.contains("missing_column") && entry.result.starts_with("Error:")
        ));
        assert!(
            !page
                .entries
                .iter()
                .any(|entry| entry.sql.contains("skipped"))
        );
        let metadata = log
            .page(0, "pragma_table_info", &AtomicBool::new(false))
            .unwrap();
        assert!(!metadata.entries.is_empty());
        log.shutdown();
        assert!(log.worker.lock().unwrap().is_none());
    }

    #[test]
    fn io_failure_is_reported_without_corrupting_log() {
        let named = tempfile::NamedTempFile::new().unwrap();
        let read_only = File::open(named.path()).unwrap();
        let log = SqlLog::from_file(read_only, None).unwrap();
        log.sink.record(record("SELECT 1"));
        assert!(
            log.page(0, "", &AtomicBool::new(false))
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(log.error().unwrap().contains("Cannot write SQL log"));
        log.shutdown();
        assert!(log.worker.lock().unwrap().is_none());
    }

    #[test]
    fn display_preview_is_utf8_safe_and_does_not_truncate_disk_record() {
        let dir = tempfile::tempdir().unwrap();
        let log = SqlLog::new_in(dir.path()).unwrap();
        let sql = "é".repeat(MAX_PREVIEW_CHARS + 1);
        log.sink.record(record(&sql));
        let page = log.page(0, &sql, &AtomicBool::new(false)).unwrap();
        assert!(page.entries[0].sql.ends_with("… [preview]"));
        let (reply, receiver) = flume::bounded(1);
        log.sink.sender.send(Request::Snapshot(reply)).unwrap();
        let (file, length) = receiver.recv().unwrap().unwrap();
        let mut disk = String::new();
        Snapshot {
            file,
            position: 0,
            length,
        }
        .read_to_string(&mut disk)
        .unwrap();
        assert!(disk.contains(&sql));
    }
}
