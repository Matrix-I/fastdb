use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One writer serializes immutable snapshots in enqueue order, including config writes.
#[derive(Clone)]
pub(super) struct SaveQueue {
    sender: flume::Sender<WriteRequest>,
    pub connection_error: Arc<std::sync::Mutex<Option<String>>>,
}
struct WriteRequest {
    job: Option<Box<dyn FnOnce() -> Result<(), String> + Send>>,
    path: Option<PathBuf>,
    target: Option<PathBuf>,
    text: String,
    reply: flume::Sender<Result<(), String>>,
}
type WriteFile = dyn Fn(&Path, &str) -> Result<(), String> + Send + Sync;

pub(super) fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("fastdb-tmp");
        let result = (|| {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    };
    write().map_err(|error| format!("Cannot save {}: {error}", path.display()))
}
fn move_file(path: &Path, target: &Path) -> Result<(), String> {
    let move_it = || -> std::io::Result<()> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // hard_link fails if the destination exists, unlike rename on Unix.
        std::fs::hard_link(path, target)?;
        if let Err(error) = std::fs::remove_file(path) {
            let _ = std::fs::remove_file(target);
            return Err(error);
        }
        Ok(())
    };
    move_it().map_err(|error| format!("Cannot move script: {error}"))
}
impl Default for SaveQueue {
    fn default() -> Self {
        Self::with_writer(Arc::new(atomic_write))
    }
}
impl SaveQueue {
    fn with_writer(writer: Arc<WriteFile>) -> Self {
        let (sender, receiver) = flume::unbounded::<WriteRequest>();
        let connection_error = Arc::new(std::sync::Mutex::new(None));
        let errors = connection_error.clone();
        std::thread::Builder::new()
            .name("fastdb-file-writer".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let is_connection = request.job.is_some();
                    let result = if let Some(job) = request.job {
                        job()
                    } else {
                        match (request.path.as_ref(), request.target.as_ref()) {
                            (Some(path), Some(target)) => {
                                writer(path, &request.text).and_then(|()| move_file(path, target))
                            }
                            (Some(path), None) => writer(path, &request.text),
                            _ => errors.lock().unwrap().clone().map_or(Ok(()), Err),
                        }
                    };
                    if is_connection {
                        *errors.lock().unwrap() = result.clone().err();
                    }
                    if let Err(error) = &result {
                        tracing::error!("{error}");
                    }
                    let _ = request.reply.send(result);
                }
            })
            .expect("start file writer");
        Self {
            sender,
            connection_error,
        }
    }
    pub fn connection_job(&self, job: impl FnOnce() -> Result<(), String> + Send + 'static) {
        let (reply, _) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            job: Some(Box::new(job)),
            path: None,
            target: None,
            text: String::new(),
            reply,
        });
    }
    pub fn move_script(
        &self,
        path: PathBuf,
        target: PathBuf,
        text: String,
    ) -> flume::Receiver<Result<(), String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            job: None,
            path: Some(path),
            target: Some(target),
            text,
            reply,
        });
        receiver
    }
    pub fn flush(&self) -> flume::Receiver<Result<(), String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            job: None,
            path: None,
            target: None,
            text: String::new(),
            reply,
        });
        receiver
    }
    pub fn write(&self, path: PathBuf, text: String) -> flume::Receiver<Result<(), String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            job: None,
            path: Some(path),
            target: None,
            text,
            reply,
        });
        receiver
    }
}

#[derive(Clone)]
pub(super) enum AfterSave {
    None,
    Run(u64),
    RunScript(u64),
    Explain(u64, bool),
    Close(Vec<u64>),
    Quit,
    Restart,
    RefreshColumn(u64),
    RefreshConnection(String),
    Disconnect(String),
    DeleteConnection(String),
}

use super::{Context, Workspace};
impl Workspace {
    pub(super) fn schedule_autosave(&mut self, id: u64, revision: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.tabs.iter().any(|t| {
                    t.id == id
                        && !t.file_busy
                        && t.editor.buffer.revision() == revision
                        && t.is_dirty()
                }) {
                    this.save_then(vec![id], AfterSave::None, cx);
                }
            });
        })
        .detach();
    }
    pub(super) fn save_then(&mut self, ids: Vec<u64>, action: AfterSave, cx: &mut Context<Self>) {
        if self.guard_column_action(&ids, &action, cx) {
            return;
        }
        let requests: Vec<_> = self
            .tabs
            .iter()
            .filter(|t| ids.contains(&t.id) && !t.file_busy && t.is_dirty())
            .filter_map(|tab| {
                let path = tab.script_path.clone()?;
                let text = tab.editor.buffer.text();
                let receiver = self.save_queue.write(path, text.clone());
                Some((tab.id, tab.editor.buffer.revision(), text, receiver))
            })
            .collect();
        if requests.is_empty() {
            self.after_save(action, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let mut outcomes = Vec::new();
            for (id, revision, text, receiver) in requests {
                let result = receiver
                    .recv_async()
                    .await
                    .unwrap_or_else(|_| Err("File writer stopped".into()));
                outcomes.push((id, revision, text, result));
            }
            let _ = this.update(cx, |this, cx| {
                let mut failed = false;
                for (id, revision, text, result) in outcomes {
                    failed |= result.is_err();
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        match result {
                            Ok(()) => {
                                tab.mark_saved(revision, text);
                            }
                            Err(error) => {
                                tab.last_error = Some(error);
                            }
                        }
                    }
                }
                cx.notify();
                if failed {
                    return;
                }
                if !matches!(action, AfterSave::None)
                    && this
                        .tabs
                        .iter()
                        .any(|t| ids.contains(&t.id) && t.is_dirty())
                {
                    this.save_then(ids, action, cx);
                } else {
                    this.after_save(action, cx);
                }
            });
        })
        .detach();
    }
    fn after_save(&mut self, action: AfterSave, cx: &mut Context<Self>) {
        match action {
            AfterSave::None => {}
            AfterSave::RefreshColumn(id) => self.load_column_tab(id, cx),
            AfterSave::RefreshConnection(owner) => self.refresh_connection_for(&owner, cx),
            AfterSave::Disconnect(owner) => self.disconnect_by_id_now(&owner),
            AfterSave::DeleteConnection(owner) => self.delete_saved_connection_now(&owner),
            AfterSave::Run(id) => self.run_saved_query(id, cx),
            AfterSave::RunScript(id) => self.run_saved_script(id, cx),
            AfterSave::Explain(id, analyze) => self.explain_saved_statement(id, analyze, cx),
            AfterSave::Close(ids) => self.close_saved_tabs(&ids, cx),
            AfterSave::Quit | AfterSave::Restart => {
                let restart = matches!(action, AfterSave::Restart);
                let script = self
                    .operations
                    .active()
                    .and_then(|op| op.script_control.clone());
                if script.is_some() {
                    self.cancel_running_query(cx);
                }
                let flushed = self.save_queue.flush();
                cx.spawn(async move |this, cx| {
                    if !matches!(flushed.recv_async().await, Ok(Ok(()))) {
                        return;
                    }
                    if let Some(script) = script {
                        let _ = db_wire::run_on_shared_runtime(async move {
                            tokio::time::timeout(
                                std::time::Duration::from_secs(15),
                                script.wait_finished(),
                            )
                            .await?;
                            Ok(())
                        })
                        .await;
                    }
                    let _ = this.update(cx, |this, cx| {
                        if this.tabs.iter().any(|t| t.is_dirty()) {
                            this.save_then(
                                this.tabs.iter().map(|t| t.id).collect(),
                                if restart {
                                    AfterSave::Restart
                                } else {
                                    AfterSave::Quit
                                },
                                cx,
                            );
                        } else {
                            this.allow_quit = true;
                            if restart {
                                crate::updater::restart_application();
                            } else {
                                cx.quit();
                            }
                        }
                    });
                })
                .detach();
            }
        }
    }
    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
        self.script_search = None;
        self.filter_builder = None;
        self.script_chooser = None;
        self.script_action = None;
        self.save_then(
            self.tabs.iter().map(|t| t.id).collect(),
            AfterSave::Quit,
            cx,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::{SaveQueue, atomic_write};
    use std::sync::{Arc, Mutex};
    #[test]
    fn snapshots_are_serialized_and_errors_allow_retry() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let output = recorded.clone();
        let (entered_tx, entered_rx) = flume::bounded(1);
        let (release_tx, release_rx) = flume::bounded(1);
        let queue = SaveQueue::with_writer(Arc::new(move |_, text| {
            if text == "old" {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            if text == "failure" {
                return Err("injected write failure".into());
            }
            output.lock().unwrap().push(text.to_owned());
            Ok(())
        }));
        let first = queue.write("script.sql".into(), "old".into());
        entered_rx.recv().unwrap();
        let latest = queue.write("script.sql".into(), "new".into());
        assert!(latest.is_empty());
        release_tx.send(()).unwrap();
        first.recv().unwrap().unwrap();
        latest.recv().unwrap().unwrap();
        assert_eq!(*recorded.lock().unwrap(), ["old", "new"]);
        assert!(
            queue
                .write("script.sql".into(), "failure".into())
                .recv()
                .unwrap()
                .is_err()
        );
        queue
            .write("script.sql".into(), "retry".into())
            .recv()
            .unwrap()
            .unwrap();
    }
    #[test]
    fn credential_failures_block_flush_until_a_successful_retry() {
        let queue = SaveQueue::default();
        queue.connection_job(|| Err("keychain locked".into()));
        assert!(queue.flush().recv().unwrap().is_err());
        assert_eq!(
            queue.connection_error.lock().unwrap().as_deref(),
            Some("keychain locked")
        );
        queue.connection_job(|| Ok(()));
        queue.flush().recv().unwrap().unwrap();
        assert!(queue.connection_error.lock().unwrap().is_none());
    }
    #[test]
    fn atomic_replacement_preserves_script_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.sql");
        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "latest").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "latest");
        assert!(!path.with_extension("fastdb-tmp").exists());
    }
}
