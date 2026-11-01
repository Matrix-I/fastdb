use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One writer serializes immutable snapshots in enqueue order, including config writes.
#[derive(Clone)]
pub(super) struct SaveQueue {
    sender: flume::Sender<WriteRequest>,
}
struct WriteRequest {
    path: Option<PathBuf>,
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
impl Default for SaveQueue {
    fn default() -> Self {
        Self::with_writer(Arc::new(atomic_write))
    }
}
impl SaveQueue {
    fn with_writer(writer: Arc<WriteFile>) -> Self {
        let (sender, receiver) = flume::unbounded::<WriteRequest>();
        std::thread::Builder::new()
            .name("fastdb-file-writer".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let result = request
                        .path
                        .as_ref()
                        .map_or(Ok(()), |path| writer(path, &request.text));
                    if let Err(error) = &result {
                        tracing::error!("{error}");
                    }
                    let _ = request.reply.send(result);
                }
            })
            .expect("start file writer");
        Self { sender }
    }
    pub fn flush(&self) -> flume::Receiver<Result<(), String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            path: None,
            text: String::new(),
            reply,
        });
        receiver
    }
    pub fn write(&self, path: PathBuf, text: String) -> flume::Receiver<Result<(), String>> {
        let (reply, receiver) = flume::bounded(1);
        let _ = self.sender.send(WriteRequest {
            path: Some(path),
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
    Close(Vec<u64>),
    Quit,
}

use super::{Context, Workspace};
impl Workspace {
    pub(super) fn schedule_autosave(&mut self, id: u64, revision: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
            let _ =
                this.update(cx, |this, cx| {
                    if this.tabs.iter().any(|t| {
                        t.id == id && t.editor.buffer.revision() == revision && t.is_dirty()
                    }) {
                        this.save_then(vec![id], AfterSave::None, cx);
                    }
                });
        })
        .detach();
    }
    pub(super) fn save_then(&mut self, ids: Vec<u64>, action: AfterSave, cx: &mut Context<Self>) {
        let requests: Vec<_> = self
            .tabs
            .iter()
            .filter(|t| ids.contains(&t.id) && t.is_dirty())
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
            AfterSave::Run(id) => self.run_saved_query(id, cx),
            AfterSave::Close(ids) => self.close_saved_tabs(&ids, cx),
            AfterSave::Quit => {
                let flushed = self.save_queue.flush();
                cx.spawn(async move |this, cx| {
                    let _ = flushed.recv_async().await;
                    let _ = this.update(cx, |this, cx| {
                        if this.tabs.iter().any(|t| t.is_dirty()) {
                            this.request_quit(cx);
                        } else {
                            this.allow_quit = true;
                            cx.quit();
                        }
                    });
                })
                .detach();
            }
        }
    }
    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
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
    fn atomic_replacement_preserves_script_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.sql");
        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "latest").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "latest");
        assert!(!path.with_extension("fastdb-tmp").exists());
    }
}
