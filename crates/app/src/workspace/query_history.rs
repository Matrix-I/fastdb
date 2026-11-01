//! A single background worker serializes history reads/writes across script tabs.
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 100;
const MAX_SQL_BYTES: usize = 64 * 1024;
#[derive(Clone)]
pub(super) struct QueryHistory {
    sender: flume::Sender<Request>,
}
enum Request {
    Record(String, String),
    List(String, flume::Sender<Result<Vec<String>, String>>),
}
fn path(root: &Path, connection: &str) -> PathBuf {
    // Connection IDs are folder names, never SQL or user-chosen script names.
    let name: String = connection.bytes().map(|b| format!("{b:02x}")).collect();
    root.join(format!("{name}.json"))
}
fn read(path: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| "Cannot read query history".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err("Cannot read query history".into()),
    }
}
fn remember(entries: &mut Vec<String>, sql: String) {
    if sql.trim().is_empty() || sql.len() > MAX_SQL_BYTES {
        return;
    }
    entries.retain(|entry| entry != &sql);
    entries.insert(0, sql);
    entries.truncate(MAX_ENTRIES);
}
impl QueryHistory {
    pub fn new(root: PathBuf) -> Self {
        let (sender, receiver) = flume::unbounded();
        std::thread::Builder::new()
            .name("fastdb-query-history".into())
            .spawn(move || {
                let mut errors = std::collections::HashMap::new();
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Record(connection, sql) => {
                            let path = path(&root, &connection);
                            // A corrupt file is retained rather than overwritten.
                            let result = (|| {
                                let mut entries = read(&path)?;
                                remember(&mut entries, sql);
                                let json = serde_json::to_string(&entries)
                                    .map_err(|_| "Cannot encode query history".to_string())?;
                                super::script_saves::atomic_write(&path, &json)
                            })();
                            match result {
                                Ok(()) => {
                                    errors.remove(&connection);
                                }
                                Err(error) => {
                                    errors.insert(connection, error);
                                }
                            }
                        }
                        Request::List(connection, reply) => {
                            let result = errors.get(&connection).map_or_else(
                                || read(&path(&root, &connection)),
                                |error| Err(format!("History could not be saved: {error}")),
                            );
                            let _ = reply.send(result);
                        }
                    }
                }
            })
            .expect("start query history worker");
        Self { sender }
    }
    pub fn record(&self, connection: String, sql: String) {
        if sql.trim().is_empty() || sql.len() > MAX_SQL_BYTES {
            return;
        }
        let _ = self.sender.send(Request::Record(connection, sql));
    }
    pub fn list(&self, connection: String) -> flume::Receiver<Result<Vec<String>, String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(Request::List(connection, reply));
        receiver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_persistent_ordered_deduplicated_and_connection_scoped() {
        let root = tempfile::tempdir().unwrap();
        let history = QueryHistory::new(root.path().into());
        history.record("a".into(), "select 1".into());
        history.record("b".into(), "select 2".into());
        history.record("a".into(), "select 3".into());
        history.record("a".into(), "select 1".into());
        assert_eq!(
            history.list("a".into()).recv().unwrap().unwrap(),
            ["select 1", "select 3"]
        );
        let reloaded = QueryHistory::new(root.path().into());
        assert_eq!(
            reloaded.list("b".into()).recv().unwrap().unwrap(),
            ["select 2"]
        );
        let mut entries = vec![];
        for n in 0..110 {
            remember(&mut entries, n.to_string());
        }
        assert_eq!(entries.len(), MAX_ENTRIES);
        remember(&mut entries, "a".repeat(MAX_SQL_BYTES + 1));
        assert_eq!(entries[0], "109");
    }
}
