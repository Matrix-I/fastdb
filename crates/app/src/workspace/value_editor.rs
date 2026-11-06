use super::script_tools::tool;
use super::value_files::MAX_VALUE_BYTES;
use super::value_format::{ValueFormat, detect_format};
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use ui_grid::RowRef;

struct Job(Arc<AtomicBool>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) struct ValueEditor {
    tab: u64,
    pager: Arc<db_wire::QueryResultPager>,
    driver: std::sync::Weak<dyn DatabaseDriver>,
    row: u64,
    column: usize,
    revision: u64,
    request: Arc<()>,
    editor: SqlEditorView,
    format: ValueFormat,
    original: Option<Vec<u8>>,
    initial: Option<String>,
    initial_default: bool,
    null: bool,
    default: bool,
    busy: bool,
    loaded: bool,
    writable: bool,
    error: Option<String>,
}

pub(super) struct ValueRead {
    pub(super) pager: Arc<db_wire::QueryResultPager>,
    pub(super) row: RowRef,
    pub(super) column: usize,
    pub(super) staged: Option<Option<String>>,
    pub(super) provider: DbDriverKind,
}

pub(super) fn read_value(read: ValueRead) -> Result<(Option<String>, Option<Vec<u8>>), String> {
    let binary = read.pager.columns[read.column].data_type == DataType::Bytea;
    let original = match read.row {
        RowRef::New(_) => None,
        RowRef::Fetched(row) => {
            let (page, local) = read
                .pager
                .locate_row(row)
                .ok_or("The selected row is unavailable")?;
            let page = read
                .pager
                .get_page(page)
                .map_err(|error| error.to_string())?;
            let bytes = page.get_cell(local, read.column);
            let limit = if binary
                && (read.provider == DbDriverKind::Postgres || read.provider.uses_odbc())
            {
                MAX_VALUE_BYTES * 2 + 2
            } else {
                MAX_VALUE_BYTES
            };
            if bytes.is_some_and(|bytes| bytes.len() > limit) {
                return Err("This value exceeds the 256 KiB editing limit".into());
            }
            bytes.map(<[u8]>::to_vec)
        }
    };
    let value = match read.staged {
        Some(value) => value,
        None => match &original {
            None => None,
            Some(bytes) if binary => {
                let bytes = db_wire::value::source_binary(bytes, read.provider)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > MAX_VALUE_BYTES {
                    return Err("Binary value exceeds 256 KiB".into());
                }
                Some(db_wire::value::hex_text(&bytes))
            }
            Some(bytes) => {
                Some(String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?)
            }
        },
    };
    if value.as_ref().is_some_and(|text| {
        text.len()
            > if binary {
                MAX_VALUE_BYTES * 2 + 2
            } else {
                MAX_VALUE_BYTES
            }
    }) {
        return Err("The staged value exceeds the editing limit".into());
    }
    if binary && let Some(text) = &value {
        let bytes = db_wire::value::parse_hex(text).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_VALUE_BYTES {
            return Err("Binary value exceeds 256 KiB".into());
        }
    }
    Ok((value, original))
}

fn editor_value(editor: &ValueEditor) -> Result<Option<String>, String> {
    if editor.null && !editor.pager.columns[editor.column].nullable {
        return Err("This column does not accept SQL NULL".into());
    }
    if editor.null || editor.default {
        return Ok(None);
    }
    let text = editor.editor.buffer.text();
    if editor.format == ValueFormat::Binary {
        let bytes = db_wire::value::parse_hex(&text).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_VALUE_BYTES {
            return Err("Binary value exceeds 256 KiB".into());
        }
    } else {
        if text.len() > MAX_VALUE_BYTES {
            return Err("Text value exceeds 256 KiB".into());
        }
        if text.contains('\0') {
            return Err(
                "Text values cannot contain NUL. Use a binary column for raw bytes.".into(),
            );
        }
        if editor.format == ValueFormat::Json {
            db_wire::value::validate_json_source(&text)
                .map_err(|error| format!("Invalid JSON: {error}"))?;
        }
    }
    Ok(Some(text))
}

impl ValueEditor {
    fn source_is_current(
        &self,
        tab: &TabState,
        driver: Option<&Arc<dyn DatabaseDriver>>,
        eligible: bool,
    ) -> bool {
        eligible
            && tab.id == self.tab
            && !tab.schema_stale
            && tab.grid.editing_cell.is_none()
            && tab.grid.changeset_revision() == self.revision
            && tab
                .grid
                .pager
                .as_ref()
                .is_some_and(|pager| Arc::ptr_eq(pager, &self.pager))
            && driver.is_some_and(|driver| {
                self.driver
                    .upgrade()
                    .is_some_and(|source| Arc::ptr_eq(driver, &source))
            })
            && !matches!(tab.grid.changeset.resolve_row(self.row), RowRef::Fetched(row) if tab.grid.changeset.is_deleted(row))
    }
}

fn stage_value(editor: ValueEditor, value: Option<String>, grid: &mut ui_grid::VirtualDataGrid) {
    if value != editor.initial || editor.default != editor.initial_default {
        grid.stage(|changes| match changes.resolve_row(editor.row) {
            RowRef::Fetched(row) => changes.set_cell(row, editor.column, editor.original, value),
            RowRef::New(index) => {
                let inserted = &mut changes.inserted_rows[index];
                inserted.values[editor.column] = value;
                if editor.null {
                    inserted.explicit_nulls.insert(editor.column);
                } else {
                    inserted.explicit_nulls.remove(&editor.column);
                }
            }
        });
    }
}

impl Workspace {
    #[allow(dead_code)]
    pub(super) fn open_value_editor(&mut self, cx: &mut Context<Self>) {
        if self.execution_busy(cx) || self.value_editor.is_some() {
            return;
        }
        let index = self.active_tab;
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (Some(focus), Some(pager), Some(driver)) = (
            tab.grid.selection.focus,
            tab.grid.pager.clone(),
            self.tab_session(index).driver.clone(),
        ) else {
            return;
        };
        if tab.grid.editing_cell.is_some() || focus.row >= tab.grid.row_count() {
            return;
        }
        let Some(column) = pager.columns.get(focus.col) else {
            return;
        };
        let data_type = column.data_type;
        let writable = self.can_edit_active_rows()
            && (!tab.is_script()
                || pager
                    .query_edit_origin
                    .as_ref()
                    .and_then(|origin| origin.columns.get(focus.col))
                    .is_some_and(|column| column.writable && !column.key));
        let row = tab.grid.changeset.resolve_row(focus.row);
        if matches!(row, RowRef::Fetched(row) if tab.grid.changeset.is_deleted(row)) {
            return;
        }
        let (staged, default) = match row {
            RowRef::Fetched(row) => (tab.grid.changeset.edited_value(row, focus.col), false),
            RowRef::New(index) => {
                let inserted = &tab.grid.changeset.inserted_rows[index];
                (
                    Some(inserted.values[focus.col].as_deref()),
                    inserted.values[focus.col].is_none()
                        && !inserted.explicit_nulls.contains(&focus.col),
                )
            }
        };
        if staged.flatten().is_some_and(|text| {
            text.len()
                > if data_type == DataType::Bytea {
                    MAX_VALUE_BYTES * 2 + 2
                } else {
                    MAX_VALUE_BYTES
                }
        }) {
            self.tabs[index].last_error = Some("Staged value exceeds the editor size limit".into());
            cx.notify();
            return;
        }
        let staged = staged.map(|value| value.map(str::to_owned));
        if self
            .value_job
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let job = Job(self.value_job.clone());
        let request = Arc::new(());
        self.value_editor = Some(ValueEditor {
            tab: tab.id,
            pager: pager.clone(),
            driver: Arc::downgrade(&driver),
            row: focus.row,
            column: focus.col,
            revision: tab.grid.changeset_revision(),
            request: request.clone(),
            editor: SqlEditorView::new("", self.theme.clone(), None),
            format: if data_type == DataType::Bytea {
                ValueFormat::Binary
            } else {
                ValueFormat::Text
            },
            original: None,
            initial: None,
            initial_default: default,
            null: false,
            default,
            busy: true,
            loaded: false,
            writable,
            error: None,
        });
        let read = ValueRead {
            pager,
            row,
            column: focus.col,
            staged,
            provider: self.tab_provider(index),
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _job = job;
                    read_value(read)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(editor) = this
                    .value_editor
                    .as_mut()
                    .filter(|editor| Arc::ptr_eq(&editor.request, &request))
                else {
                    return;
                };
                editor.busy = false;
                match result {
                    Ok((value, original)) => {
                        editor.null = value.is_none() && !editor.default;
                        if editor.format != ValueFormat::Binary {
                            editor.format = value
                                .as_ref()
                                .map(|text| detect_format(text.as_bytes(), data_type))
                                .unwrap_or(ValueFormat::Text);
                        }
                        editor.editor = SqlEditorView::new(
                            value.as_deref().unwrap_or(""),
                            this.theme.clone(),
                            None,
                        );
                        editor.initial = value;
                        editor.original = original;
                        editor.loaded = true;
                    }
                    Err(error) => editor.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn value_editor_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape" {
            self.value_editor = None;
            cx.notify();
            return;
        }
        if (key.modifiers.platform || key.modifiers.control) && key.key.eq_ignore_ascii_case("s") {
            self.apply_value_editor(cx);
            return;
        }
        let Some(editor) = self
            .value_editor
            .as_mut()
            .filter(|editor| editor.loaded && !editor.busy)
        else {
            return;
        };
        use ui_editor::text_edit::{TextCommand, dispatch};
        let Some(mut command) = TextCommand::from_key(key, true) else {
            return;
        };
        if command == TextCommand::Paste {
            let Some(text) = cx
                .read_from_clipboard()
                .and_then(|clipboard| clipboard.text())
            else {
                return;
            };
            command = TextCommand::Insert(text);
        }
        if let TextCommand::Insert(text) = &command {
            let remaining = editor.editor.buffer.len().saturating_sub(
                editor
                    .editor
                    .buffer
                    .selected_text()
                    .map_or(0, |text| text.len()),
            );
            let limit = if editor.format == ValueFormat::Binary {
                MAX_VALUE_BYTES * 2 + 2
            } else {
                MAX_VALUE_BYTES
            };
            if text.len() > limit.saturating_sub(remaining) {
                editor.error = Some("Value would exceed the editor size limit".into());
                cx.notify();
                return;
            }
        }
        let revision = editor.editor.buffer.revision();
        dispatch(&mut editor.editor, &command, &mut **cx);
        editor.editor.hide_completions();
        if editor.editor.buffer.revision() != revision {
            editor.null = false;
            editor.default = false;
            editor.error = None;
        }
        cx.notify();
    }

    fn apply_value_editor(&mut self, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        let Some(editor) = self
            .value_editor
            .as_ref()
            .filter(|editor| editor.loaded && !editor.busy && editor.writable)
        else {
            return;
        };
        let result = editor_value(editor);
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.value_editor.as_mut().unwrap().error = Some(error);
                cx.notify();
                return;
            }
        };
        let Some(index) = self.tabs.iter().position(|tab| tab.id == editor.tab) else {
            self.value_editor = None;
            cx.notify();
            return;
        };
        let tab = &self.tabs[index];
        let valid = editor.source_is_current(
            tab,
            self.tab_session(index).driver.as_ref(),
            index == self.active_tab && self.can_edit_active_rows(),
        );
        if !valid {
            self.value_editor.as_mut().unwrap().error = Some(
                "Source result, connection or staged edits changed. Reopen the value editor."
                    .into(),
            );
            cx.notify();
            return;
        }
        let editor = self.value_editor.take().unwrap();
        stage_value(editor, value, &mut self.tabs[index].grid);
        cx.notify();
    }

    fn load_value_file(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.value_editor.as_mut().filter(|e| e.loaded && !e.busy) else {
            return;
        };
        if self
            .value_job
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let job = Job(self.value_job.clone());
        let request = editor.request.clone();
        let binary = editor.format == ValueFormat::Binary;
        let encoding = self.value_panel.file_encoding;
        editor.busy = true;
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load value file".into()),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let _job = job;
            let result = match dialog.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => {
                        cx.background_executor()
                            .spawn(async move {
                                super::value_files::read_file(&path, binary, encoding).map(Some)
                            })
                            .await
                    }
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("Value file dialog closed unexpectedly".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(editor) = this
                    .value_editor
                    .as_mut()
                    .filter(|e| Arc::ptr_eq(&e.request, &request))
                {
                    editor.busy = false;
                    match result {
                        Ok(Some(text)) => {
                            editor.editor.buffer.select_all();
                            editor.editor.buffer.insert_str(&text);
                            editor.null = false;
                            editor.default = false;
                            editor.error = None;
                        }
                        Ok(None) => {}
                        Err(error) => editor.error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn save_value_file(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.value_editor.as_mut().filter(|e| e.loaded && !e.busy) else {
            return;
        };
        let text = match editor_value(editor) {
            Ok(Some(text)) => text,
            Ok(None) => {
                editor.error =
                    Some("SQL NULL and unset defaults have no file representation".into());
                cx.notify();
                return;
            }
            Err(error) => {
                editor.error = Some(error);
                cx.notify();
                return;
            }
        };
        if self
            .value_job
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let job = Job(self.value_job.clone());
        let request = editor.request.clone();
        let binary = editor.format == ValueFormat::Binary;
        let encoding = self.value_panel.file_encoding;
        editor.busy = true;
        let dialog = cx.prompt_for_new_path(
            &platform_hal::paths::downloads_dir(),
            Some(if binary { "value.bin" } else { "value.txt" }),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let _job = job;
            let result = match dialog.await {
                Ok(Ok(Some(path))) => {
                    cx.background_executor()
                        .spawn(async move {
                            super::value_files::write_file(&path, &text, binary, encoding)
                        })
                        .await
                }
                Ok(Ok(None)) => Ok(()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("Value save dialog closed unexpectedly".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(editor) = this
                    .value_editor
                    .as_mut()
                    .filter(|e| Arc::ptr_eq(&e.request, &request))
                {
                    editor.busy = false;
                    editor.error = result.err();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn format_edited_value(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self
            .value_editor
            .as_mut()
            .filter(|editor| editor.loaded && !editor.busy)
        else {
            return;
        };
        let text = editor.editor.buffer.text();
        let formatted = match editor.format {
            ValueFormat::Json => super::value_files::format_json(&text),
            ValueFormat::Xml => {
                Err("XML whitespace can be significant. Edit the XML source directly.".into())
            }
            _ => Ok(text),
        };
        match formatted {
            Ok(text) if text.len() <= MAX_VALUE_BYTES => {
                editor.editor.buffer.select_all();
                editor.editor.buffer.insert_str(&text);
                editor.null = false;
                editor.default = false;
                editor.error = None;
            }
            Ok(_) => editor.error = Some("Formatted value exceeds 256 KiB".into()),
            Err(error) => editor.error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn render_value_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let pick: ui_editor::EditorPick = std::rc::Rc::new(move |line, column, kind, _, app| {
            let _ = weak.update(app, |this, cx| {
                if let Some(editor) = this
                    .value_editor
                    .as_mut()
                    .filter(|editor| editor.loaded && !editor.busy)
                {
                    editor.editor.pick(line, column, kind);
                    cx.notify();
                }
            });
        });
        let editor = self.value_editor.as_mut().unwrap();
        let binary = editor.pager.columns[editor.column].data_type == DataType::Bytea;
        let ready = editor.loaded && !editor.busy;
        let can_default = matches!(
            self.tabs
                .iter()
                .find(|tab| tab.id == editor.tab)
                .map(|tab| tab.grid.changeset.resolve_row(editor.row)),
            Some(RowRef::New(_))
        );
        let mut tools = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                this.value_editor = None;
                cx.notify();
            })));
        if ready {
            if editor.writable {
                tools = tools.child(
                    tool("Apply to grid")
                        .on_click(cx.listener(|this, _, _, cx| this.apply_value_editor(cx))),
                );
            }
            tools = tools
                .child(tool("SQL NULL").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(editor) = &mut this.value_editor {
                        editor.null = true;
                        editor.default = false;
                    }
                    cx.notify();
                })))
                .child(
                    tool("Empty value").on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(editor) = &mut this.value_editor {
                            editor.editor.buffer.select_all();
                            editor
                                .editor
                                .buffer
                                .insert_str(if binary { "\\x" } else { "" });
                            editor.null = false;
                            editor.default = false;
                        }
                        cx.notify();
                    })),
                );
            if can_default {
                tools = tools.child(tool("Use default").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(editor) = &mut this.value_editor {
                        editor.default = true;
                        editor.null = false;
                    }
                    cx.notify();
                })));
            }
            for format in if binary {
                vec![ValueFormat::Binary]
            } else {
                vec![ValueFormat::Text, ValueFormat::Json, ValueFormat::Xml]
            } {
                tools = tools.child(tool(format.label()).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(editor) = &mut this.value_editor {
                            editor.format = format;
                        }
                        cx.notify();
                    },
                )));
            }
            tools = tools
                .child(
                    tool("Format value")
                        .on_click(cx.listener(|this, _, _, cx| this.format_edited_value(cx))),
                )
                .child(
                    tool("Load file")
                        .on_click(cx.listener(|this, _, _, cx| this.load_value_file(cx))),
                )
                .child(
                    tool("Save file")
                        .on_click(cx.listener(|this, _, _, cx| this.save_value_file(cx))),
                );
            let encoding = self.value_panel.file_encoding;
            tools = tools.child(
                tool(encoding.label()).on_click(cx.listener(|this, _, _, cx| {
                    this.value_panel.file_encoding = this.value_panel.file_encoding.next();
                    cx.notify();
                })),
            );
        }
        if ready && editor.pager.columns[editor.column].data_type == DataType::TimestampTz {
            tools = tools.child(
                tool(if self.value_panel.utc_timestamps {
                    "Time: UTC"
                } else {
                    "Time: source offset"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.value_panel.utc_timestamps = !this.value_panel.utc_timestamps;
                    cx.notify();
                })),
            );
        }
        let editor = self.value_editor.as_mut().unwrap();
        let timestamp = if ready
            && !editor.null
            && !editor.default
            && editor.pager.columns[editor.column].data_type == DataType::TimestampTz
        {
            Some(
                super::value_files::timestamp_display(
                    &editor.editor.buffer.text(),
                    self.value_panel.utc_timestamps,
                )
                .unwrap_or_else(|error| error),
            )
        } else {
            None
        };
        let status = if editor.busy {
            "Loading…"
        } else if editor.default {
            "Unset · database default"
        } else if editor.null {
            "SQL NULL"
        } else if binary {
            "Binary · hexadecimal bytes"
        } else {
            "Text · original bytes preserved until Apply"
        };
        let content = editor.editor.render_editor(ready, true, pick, window);
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(820.0))
                    .h(px(600.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(theme.bg_surface)
                    .text_color(theme.text_primary)
                    .text_size(px(12.0))
                    .child(format!(
                        "Value · {} · row {} · 256 KiB limit",
                        editor.pager.columns[editor.column].name,
                        editor.row + 1
                    ))
                    .child(tools)
                    .child(status)
                    .children(
                        timestamp.map(|text| div().child(format!("Timestamp display: {text}"))),
                    )
                    .child(div().flex_1().min_h_0().overflow_hidden().child(content))
                    .children(
                        editor
                            .error
                            .as_ref()
                            .map(|error| div().text_color(theme.text_error).child(error.clone())),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_VALUE_BYTES, ValueRead, read_value};
    use db_wire::{DataType, DbDriverKind};
    use std::sync::Arc;
    use ui_grid::RowRef;

    fn read(bytes: Option<&[u8]>, kind: DataType, provider: DbDriverKind) -> ValueRead {
        let pager =
            db_wire::QueryResultPager::new(vec![db_wire::ColumnMeta::new("value", kind)]).unwrap();
        pager.append_row([bytes]).unwrap();
        ValueRead {
            pager: Arc::new(pager),
            row: RowRef::Fetched(0),
            column: 0,
            staged: None,
            provider,
        }
    }

    fn editor(pager: Arc<db_wire::QueryResultPager>) -> super::ValueEditor {
        let driver: Arc<dyn db_wire::DatabaseDriver> =
            Arc::new(db_wire::SqliteDriver::new_memory());
        super::ValueEditor {
            tab: 1,
            pager,
            driver: Arc::downgrade(&driver),
            row: 0,
            column: 0,
            revision: 0,
            request: Arc::new(()),
            editor: ui_editor::SqlEditorView::new("old", ui_core::theme::Theme::dark(), None),
            format: super::ValueFormat::Text,
            original: Some(b"old".to_vec()),
            initial: Some("old".into()),
            initial_default: false,
            null: false,
            default: false,
            busy: false,
            loaded: true,
            writable: true,
            error: None,
        }
    }

    #[test]
    fn native_json_editor_keeps_numbers_larger_than_binary_float_range() {
        let json = r#"{"huge":1e999,"precise":0.12345678901234567890123456789}"#;
        let mut edit = editor(
            read(
                Some(json.as_bytes()),
                DataType::Json,
                DbDriverKind::Postgres,
            )
            .pager,
        );
        edit.format = super::ValueFormat::Json;
        edit.editor.buffer.select_all();
        edit.editor.buffer.insert_str(json);
        assert_eq!(super::editor_value(&edit).unwrap(), Some(json.into()));
    }

    #[test]
    fn apply_source_guard_rejects_replaced_results_drivers_tabs_and_edits() {
        let request = read(Some(b"old"), DataType::Text, DbDriverKind::Sqlite);
        let mut edit = editor(request.pager.clone());
        let driver: Arc<dyn db_wire::DatabaseDriver> =
            Arc::new(db_wire::SqliteDriver::new_memory());
        edit.driver = Arc::downgrade(&driver);
        let mut tab = crate::workspace::TabState::table(
            1,
            "main",
            "items",
            ui_core::theme::Theme::dark(),
            Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        tab.grid.pager = Some(request.pager.clone());
        assert!(edit.source_is_current(&tab, Some(&driver), true));
        assert!(!edit.source_is_current(&tab, Some(&driver), false));
        assert!(!edit.source_is_current(&tab, None, true));
        let other: Arc<dyn db_wire::DatabaseDriver> = Arc::new(db_wire::SqliteDriver::new_memory());
        assert!(!edit.source_is_current(&tab, Some(&other), true));
        tab.id = 2;
        assert!(!edit.source_is_current(&tab, Some(&driver), true));
        tab.id = 1;
        tab.schema_stale = true;
        assert!(!edit.source_is_current(&tab, Some(&driver), true));
        tab.schema_stale = false;
        tab.grid.pager = Some(read(Some(b"old"), DataType::Text, DbDriverKind::Sqlite).pager);
        assert!(!edit.source_is_current(&tab, Some(&driver), true));
        tab.grid.pager = Some(request.pager);
        tab.grid
            .stage(|changes| changes.set_cell(0, 0, Some(b"old".to_vec()), Some("new".into())));
        assert!(!edit.source_is_current(&tab, Some(&driver), true));
        assert!(tab.grid.step_staged_edits(false));
        assert!(!edit.source_is_current(&tab, Some(&driver), true));
        let pager = db_wire::QueryResultPager::new(vec![
            db_wire::ColumnMeta::new("required", DataType::Text).with_nullable(false),
        ])
        .unwrap();
        let mut required = editor(Arc::new(pager));
        required.null = true;
        assert!(super::editor_value(&required).is_err());
    }

    #[test]
    fn apply_stages_one_undo_and_distinguishes_insert_default_null_empty() {
        let request = read(Some(b"old"), DataType::Text, DbDriverKind::Sqlite);
        let mut grid = ui_grid::VirtualDataGrid::new(
            Some(request.pager.clone()),
            ui_core::theme::Theme::dark(),
        );
        let mut edit = editor(request.pager.clone());
        edit.null = true;
        let value = super::editor_value(&edit).unwrap();
        super::stage_value(edit, value, &mut grid);
        assert_eq!(grid.changeset.edited_value(0, 0), Some(None));
        assert!(grid.step_staged_edits(false));
        assert!(!grid.changeset.is_dirty());
        grid.stage(|changes| changes.insert_row_at(0, vec![None]));
        let mut edit = editor(request.pager.clone());
        edit.initial = None;
        edit.initial_default = true;
        edit.null = true;
        super::stage_value(edit, None, &mut grid);
        assert!(grid.changeset.inserted_rows[0].explicit_nulls.contains(&0));
        let mut edit = editor(request.pager.clone());
        edit.initial = None;
        edit.editor.buffer.select_all();
        edit.editor.buffer.insert_str("");
        let value = super::editor_value(&edit).unwrap();
        assert_eq!(value, Some(String::new()));
        super::stage_value(edit, value, &mut grid);
        assert_eq!(
            grid.changeset.inserted_rows[0].values[0],
            Some(String::new())
        );
        assert!(!grid.changeset.inserted_rows[0].explicit_nulls.contains(&0));
        let mut edit = editor(request.pager);
        edit.initial = Some(String::new());
        edit.default = true;
        super::stage_value(edit, None, &mut grid);
        assert_eq!(grid.changeset.inserted_rows[0].values[0], None);
        assert!(!grid.changeset.inserted_rows[0].explicit_nulls.contains(&0));
    }

    #[test]
    fn reads_distinguish_null_empty_binary_and_staged_values() {
        assert_eq!(
            read_value(read(None, DataType::Text, DbDriverKind::Sqlite)).unwrap(),
            (None, None)
        );
        assert_eq!(
            read_value(read(Some(b""), DataType::Text, DbDriverKind::Sqlite))
                .unwrap()
                .0,
            Some(String::new())
        );
        for provider in [DbDriverKind::Sqlite, DbDriverKind::MySql] {
            assert_eq!(
                read_value(read(Some(b"\\xdead"), DataType::Bytea, provider))
                    .unwrap()
                    .0,
                Some(db_wire::value::hex_text(b"\\xdead"))
            );
        }
        assert_eq!(
            read_value(read(
                Some(b"\\x00ff"),
                DataType::Bytea,
                DbDriverKind::Postgres
            ))
            .unwrap()
            .0,
            Some("\\x00ff".into())
        );
        let mut request = read(Some(b"old"), DataType::Text, DbDriverKind::Sqlite);
        request.staged = Some(None);
        assert_eq!(read_value(request).unwrap(), (None, Some(b"old".to_vec())));
        assert!(
            read_value(read(
                Some(&vec![b'a'; MAX_VALUE_BYTES + 1]),
                DataType::Text,
                DbDriverKind::Sqlite
            ))
            .is_err()
        );
        assert!(read_value(read(Some(&[0xff]), DataType::Text, DbDriverKind::Sqlite)).is_err());
    }
}
