//! Bounded UTF-8 SQL file I/O, executed on the background executor.
#![allow(dead_code)]
use super::*;
use std::io::{Read, Write};
use std::path::Path;

const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;

fn validate_path(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("sql")),
        "Choose a .sql file."
    );
    Ok(())
}

fn read_sql(path: &Path) -> anyhow::Result<String> {
    validate_path(path)?;
    anyhow::ensure!(
        std::fs::metadata(path)?.is_file(),
        "Choose a regular SQL file."
    );
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "Choose a regular SQL file.");
    let mut bytes = Vec::new();
    file.take(MAX_SCRIPT_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_SCRIPT_BYTES,
        "SQL files are limited to 16 MiB."
    );
    Ok(String::from_utf8(bytes)?)
}

fn write_sql(path: &Path, text: &str) -> anyhow::Result<()> {
    validate_path(path)?;
    anyhow::ensure!(
        text.len() as u64 <= MAX_SCRIPT_BYTES,
        "SQL files are limited to 16 MiB."
    );
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

fn import_sql(
    store: &ScriptStore,
    owner: &str,
    path: &Path,
) -> anyhow::Result<(ScriptFile, String)> {
    let text = read_sql(path)?;
    let script = store.create(owner)?;
    if let Err(error) = write_sql(&script.path, &text) {
        // The empty destination was created by this operation only.
        std::fs::remove_file(&script.path).map_err(|cleanup| {
            anyhow::anyhow!("Import failed: {error}; cannot remove empty copy: {cleanup}")
        })?;
        return Err(error);
    }
    Ok((script, text))
}

impl Workspace {
    pub(super) fn import_script_file(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy && !t.script_file_dialog_busy)
        else {
            return;
        };
        let Some(owner) = tab.connection_id.clone() else {
            return;
        };
        let id = tab.id;
        let store = self.script_store.clone();
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import SQL as an autosaved copy".into()),
        });
        tab.script_file_dialog_busy = true;
        tab.script_status = Some("Choose a UTF-8 .sql file to import as a copy…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match dialog.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => {
                        cx.background_executor()
                            .spawn(async move {
                                import_sql(&store, &owner, &path)
                                    .map(Some)
                                    .map_err(|e| format!("Cannot import SQL: {e}"))
                            })
                            .await
                    }
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                Ok(Err(e)) => Err(format!("Cannot open SQL dialog: {e}")),
                Err(_) => Err("SQL dialog closed unexpectedly.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) else {
                    return;
                };
                tab.script_file_dialog_busy = false;
                tab.script_status = None;
                let revision = tab.editor.buffer.revision();
                this.schedule_autosave(id, revision, cx);
                match result {
                    Ok(Some((script, text))) => this.install_script_tab(&script, &text, cx),
                    Ok(None) => {}
                    Err(error) => {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                            tab.last_error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn save_script_copy(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy && !t.script_file_dialog_busy)
        else {
            return;
        };
        let id = tab.id;
        let text = tab.editor.buffer.text();
        let name = format!("{}.sql", tab.title);
        let source = tab.script_path.clone();
        let dialog = cx.prompt_for_new_path(&platform_hal::paths::downloads_dir(), Some(&name));
        tab.script_file_dialog_busy = true;
        tab.script_status = Some("Choose where to save a SQL copy…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match dialog.await {
                Ok(Ok(Some(path))) => cx
                    .background_executor()
                    .spawn(async move {
                        if source.as_ref().is_some_and(|source| {
                            source == &path
                                || std::fs::canonicalize(source)
                                    .ok()
                                    .zip(std::fs::canonicalize(&path).ok())
                                    .is_some_and(|(a, b)| a == b)
                        }) {
                            return Err(
                                "Choose a different destination from the autosaved source script."
                                    .to_string(),
                            );
                        }
                        write_sql(&path, &text)
                            .map(|_| format!("Saved SQL copy to {}", path.display()))
                            .map_err(|e| format!("Cannot save SQL copy: {e}"))
                    })
                    .await
                    .map(Some),
                Ok(Ok(None)) => Ok(None),
                Ok(Err(e)) => Err(format!("Cannot open save dialog: {e}")),
                Err(_) => Err("Save dialog closed unexpectedly.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                    tab.script_file_dialog_busy = false;
                    match result {
                        Ok(message) => tab.script_status = message,
                        Err(error) => tab.last_error = Some(error),
                    }
                    let revision = tab.editor.buffer.revision();
                    this.schedule_autosave(id, revision, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SCRIPT_BYTES, ScriptStore, import_sql, read_sql, write_sql};

    #[test]
    fn sql_files_preserve_unicode_line_endings_and_empty_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quoted name.SQL");
        let text = "-- 名\r\nSELECT 'ngày';\n";
        write_sql(&path, text).unwrap();
        assert_eq!(read_sql(&path).unwrap(), text);
        write_sql(&path, "").unwrap();
        assert_eq!(read_sql(&path).unwrap(), "");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_input_and_failed_replacement_preserve_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.sql");
        std::fs::write(&path, [0xff]).unwrap();
        assert!(read_sql(&path).is_err());
        assert!(read_sql(&dir.path().join("missing.sql")).is_err());
        assert!(write_sql(&dir.path().join("wrong.txt"), "x").is_err());
        let target = dir.path().join("directory.sql");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), "original").unwrap();
        assert!(write_sql(&target, "new").is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("keep")).unwrap(),
            "original"
        );
    }

    #[test]
    fn oversized_files_are_rejected_without_replacing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.sql");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SCRIPT_BYTES + 1).unwrap();
        assert!(read_sql(&path).is_err());
        std::fs::write(&path, "original").unwrap();
        assert!(write_sql(&path, &"x".repeat(MAX_SCRIPT_BYTES as usize + 1)).is_err());
        assert_eq!(read_sql(&path).unwrap(), "original");
    }

    #[test]
    fn import_creates_independent_numbered_copies_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.sql");
        let store = ScriptStore::new(dir.path().join("scripts"));
        write_sql(&source, "SELECT '名';\r\n").unwrap();
        let (first, text) = import_sql(&store, "owner", &source).unwrap();
        let (second, _) = import_sql(&store, "owner", &source).unwrap();
        assert_ne!(first.path, second.path);
        assert_eq!(store.read(&first).unwrap(), text);
        assert_eq!(first.connection_id, "owner");
        write_sql(&first.path, "changed").unwrap();
        assert_eq!(read_sql(&source).unwrap(), "SELECT '名';\r\n");
        assert_eq!(store.read(&second).unwrap(), text);
        std::fs::write(&source, [0xff]).unwrap();
        assert!(import_sql(&store, "owner", &source).is_err());
        assert_eq!(store.list(Some("owner")).len(), 2);
    }
}
