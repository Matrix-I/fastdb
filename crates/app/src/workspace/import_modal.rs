use super::script_tools::tool;
use super::*;
use db_wire::{
    ColumnMeta, DatabaseDriver,
    transfer::{
        csv::{CsvOptions, CsvReader},
        import::{CommitPolicy, ImportControl, ImportProgress, ImportSettings, run_csv_import},
        mapping::{ColumnMapping, Conversion, Mapping},
    },
};
use std::sync::Arc;

pub struct ImportModal {
    pub conn_id: String,
    pub provider: db_wire::DbDriverKind,
    pub schema: String,
    pub table: String,
    pub driver: Arc<dyn DatabaseDriver>,
    pub file_path: String,
    pub delimiter: u8,
    pub has_header: bool,
    pub commit_policy: CommitPolicy,
    pub skip_invalid: bool,
    pub target_columns: Vec<ColumnMeta>,
    pub source_headers: Vec<String>,
    pub preview_rows: Vec<Vec<String>>,
    pub is_importing: bool,
    pub progress: ImportProgress,
    pub status: Option<String>,
    pub error: Option<String>,
    pub control: Option<Arc<ImportControl>>,
}

impl ImportModal {
    pub fn new(
        conn_id: String,
        provider: db_wire::DbDriverKind,
        schema: String,
        table: String,
        driver: Arc<dyn DatabaseDriver>,
        target_columns: Vec<ColumnMeta>,
    ) -> Self {
        Self {
            conn_id,
            provider,
            schema,
            table,
            driver,
            file_path: String::new(),
            delimiter: b',',
            has_header: true,
            commit_policy: CommitPolicy::Atomic,
            skip_invalid: false,
            target_columns,
            source_headers: Vec::new(),
            preview_rows: Vec::new(),
            is_importing: false,
            progress: ImportProgress::default(),
            status: None,
            error: None,
            control: None,
        }
    }

    pub fn load_file(&mut self, path: String) {
        self.file_path = path.clone();
        self.error = None;
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
                return;
            }
        };
        let options = CsvOptions {
            delimiter: self.delimiter,
            header: self.has_header,
            ..Default::default()
        };
        let mut reader = match CsvReader::new(file, options) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("Failed to parse CSV: {e}"));
                return;
            }
        };
        let headers = if let Some(h) = reader.headers() {
            h.to_vec()
        } else {
            Vec::new()
        };
        let mut preview_rows: Vec<Vec<String>> = Vec::new();
        for _ in 0..10 {
            match reader.next_record() {
                Ok(Some(row)) => {
                    preview_rows.push(
                        row.into_iter()
                            .map(|val| val.unwrap_or_else(|| "NULL".to_string()))
                            .collect(),
                    );
                }
                Ok(None) => break,
                Err(e) => {
                    self.error = Some(format!("Error reading CSV preview: {e}"));
                    break;
                }
            }
        }
        if headers.is_empty() && !preview_rows.is_empty() {
            self.source_headers = (0..preview_rows[0].len())
                .map(|i| format!("Col {}", i + 1))
                .collect();
        } else {
            self.source_headers = headers;
        }
        self.preview_rows = preview_rows;
        self.status = Some(format!(
            "Loaded preview with {} columns, {} sample rows",
            self.source_headers.len(),
            self.preview_rows.len()
        ));
    }
}

impl Workspace {
    pub(super) fn open_import_modal(
        &mut self,
        conn_id: &str,
        schema: &str,
        table: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.connections.sessions.get(conn_id) else {
            return;
        };
        let Some(driver) = session.driver.clone() else {
            return;
        };
        let target_columns = session
            .table_columns
            .get(&(schema.to_string(), table.to_string()))
            .cloned()
            .unwrap_or_default();

        let modal = ImportModal::new(
            conn_id.to_string(),
            session.kind,
            schema.to_string(),
            table.to_string(),
            driver,
            target_columns,
        );
        self.import_modal = Some(modal);
        cx.notify();
    }

    pub(super) fn execute_csv_import(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.import_modal else {
            return;
        };
        if modal.is_importing || modal.file_path.is_empty() {
            return;
        }
        let file_path = modal.file_path.clone();
        let file = match std::fs::File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                modal.error = Some(format!("Cannot open source file: {e}"));
                cx.notify();
                return;
            }
        };

        // Construct mapping: map each target column to matching source header
        let mut column_mappings = Vec::new();
        for target in &modal.target_columns {
            if let Some(src_idx) = modal
                .source_headers
                .iter()
                .position(|h| h.eq_ignore_ascii_case(&target.name))
            {
                let conversion = Conversion::Auto;
                column_mappings.push(ColumnMapping {
                    source: src_idx,
                    target: target.clone(),
                    conversion,
                });
            }
        }

        if column_mappings.is_empty() {
            modal.error =
                Some("No CSV headers matched target table columns. Verify CSV header row.".into());
            cx.notify();
            return;
        }

        let mapping = match Mapping::new(modal.source_headers.len(), column_mappings) {
            Ok(m) => m,
            Err(e) => {
                modal.error = Some(format!("Mapping error: {e}"));
                cx.notify();
                return;
            }
        };

        let modal_schema = modal.schema.clone();
        let modal_table = modal.table.clone();
        let commit_policy = modal.commit_policy;
        let skip_invalid = modal.skip_invalid;
        let delimiter = modal.delimiter;
        let has_header = modal.has_header;

        let control = Arc::new(ImportControl::default());
        modal.control = Some(control.clone());
        modal.is_importing = true;
        modal.status = Some("Importing in progress…".into());
        modal.error = None;
        cx.notify();

        let driver = modal.driver.clone();
        let provider = modal.provider;
        let mut progress_rx = control.progress();

        cx.spawn(async move |this, cx| {
            let mut expected_object_id = None;
            if provider == db_wire::DbDriverKind::Postgres {
                let query = format!("SELECT '{modal_schema}.{modal_table}'::regclass::oid::text");
                if let Ok((res, _, _)) = driver.execute_query(&query).await
                    && let Some(cell) = res.get_cell(0, 0)
                {
                    expected_object_id = String::from_utf8(cell).ok();
                }
            }

            let settings = ImportSettings {
                schema: modal_schema,
                table: modal_table,
                expected_object_id,
                csv: CsvOptions {
                    delimiter,
                    header: has_header,
                    ..Default::default()
                },
                mapping,
                commit: commit_policy,
                skip_invalid_values: skip_invalid,
            };

            let progress_task = cx.spawn({
                let this = this.clone();
                async move |cx| {
                    while progress_rx.changed().await.is_ok() {
                        let prog = *progress_rx.borrow();
                        let _ = this.update(cx, |this, cx| {
                            if let Some(modal) = &mut this.import_modal {
                                modal.progress = prog;
                                modal.status = Some(format!(
                                    "Processed: {}, Committed: {}, Rejected: {}",
                                    prog.processed, prog.committed, prog.rejected
                                ));
                                cx.notify();
                            }
                        });
                    }
                }
            });

            let outcome = run_csv_import(driver, file, settings, control).await;
            drop(progress_task);

            let _ = this.update(cx, |this, cx| {
                if let Some(modal) = &mut this.import_modal {
                    modal.is_importing = false;
                    modal.control = None;
                    if let Some(err) = outcome.error {
                        modal.error = Some(format!("Import failed: {err}"));
                    } else if outcome.cancelled {
                        modal.status = Some("Import was cancelled.".into());
                    } else {
                        modal.status = Some(format!(
                            "Successfully imported {} row(s) (committed: {}, rejected: {}).",
                            outcome.progress.committed,
                            outcome.progress.committed,
                            outcome.progress.rejected
                        ));
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn render_import_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let modal = self.import_modal.as_ref().unwrap();
        let theme = &self.theme;

        let mut content = div().flex().flex_col().gap_3();

        // 1. File picker row
        let file_path_display = if modal.file_path.is_empty() {
            "No CSV file selected"
        } else {
            &modal.file_path
        };
        let file_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .bg(theme.bg_editor)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(11.5))
                    .text_color(if modal.file_path.is_empty() {
                        theme.text_muted
                    } else {
                        theme.text_primary
                    })
                    .child(file_path_display.to_string()),
            )
            .child(tool("Browse…").on_click(cx.listener(|_this, _, _, cx| {
                let dialog = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Select CSV File".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = dialog.await
                        && let Some(path) = paths.into_iter().next()
                    {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(modal) = &mut this.import_modal {
                                modal.load_file(path.display().to_string());
                                cx.notify();
                            }
                        });
                    }
                })
                .detach();
            })));
        content = content.child(file_row);

        // 2. Options row
        let mut options_row = div().flex().items_center().gap_3().text_size(px(11.5));

        // Delimiter picker
        for (label, delim) in [
            ("Comma (,)", b','),
            ("Semicolon (;)", b';'),
            ("Tab (\\t)", b'\t'),
            ("Pipe (|)", b'|'),
        ] {
            let is_selected = modal.delimiter == delim;
            let mut delim_btn = div()
                .id(label)
                .px_1p5()
                .py_0p5()
                .rounded_xs()
                .border_1()
                .border_color(if is_selected {
                    rgb(0x388bfd)
                } else {
                    theme.border
                })
                .bg(if is_selected {
                    rgb(0x1d4ed8)
                } else {
                    theme.bg_surface
                })
                .text_color(if is_selected {
                    rgb(0xffffff)
                } else {
                    theme.text_muted
                })
                .child(label);
            if !modal.is_importing {
                delim_btn = delim_btn
                    .cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = &mut this.import_modal {
                            m.delimiter = delim;
                            if !m.file_path.is_empty() {
                                let path = m.file_path.clone();
                                m.load_file(path);
                            }
                            cx.notify();
                        }
                    }));
            }
            options_row = options_row.child(delim_btn);
        }

        // Header checkbox
        let has_hdr = modal.has_header;
        let mut hdr_btn = div()
            .id("toggle-has-header")
            .px_1p5()
            .py_0p5()
            .rounded_xs()
            .border_1()
            .border_color(if has_hdr { rgb(0x388bfd) } else { theme.border })
            .bg(if has_hdr {
                rgb(0x1d4ed8)
            } else {
                theme.bg_surface
            })
            .text_color(if has_hdr {
                rgb(0xffffff)
            } else {
                theme.text_muted
            })
            .child("Has Header");
        if !modal.is_importing {
            hdr_btn = hdr_btn
                .cursor_pointer()
                .hover(move |s| s.bg(theme.bg_surface_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal {
                        m.has_header = !m.has_header;
                        if !m.file_path.is_empty() {
                            let path = m.file_path.clone();
                            m.load_file(path);
                        }
                        cx.notify();
                    }
                }));
        }
        options_row = options_row.child(hdr_btn);

        // Commit policy toggle
        for (label, policy) in [
            ("Atomic Commit", CommitPolicy::Atomic),
            ("Per-Batch Commit", CommitPolicy::PerBatch),
        ] {
            let is_selected = matches!(modal.commit_policy, p if std::mem::discriminant(&p) == std::mem::discriminant(&policy));
            let mut pol_btn = div()
                .id(label)
                .px_1p5()
                .py_0p5()
                .rounded_xs()
                .border_1()
                .border_color(if is_selected {
                    rgb(0x388bfd)
                } else {
                    theme.border
                })
                .bg(if is_selected {
                    rgb(0x1d4ed8)
                } else {
                    theme.bg_surface
                })
                .text_color(if is_selected {
                    rgb(0xffffff)
                } else {
                    theme.text_muted
                })
                .child(label);
            if !modal.is_importing {
                pol_btn = pol_btn
                    .cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = &mut this.import_modal {
                            m.commit_policy = policy;
                            cx.notify();
                        }
                    }));
            }
            options_row = options_row.child(pol_btn);
        }

        content = content.child(options_row);

        // 3. Status or Error banners
        if let Some(ref err) = modal.error {
            content = content.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .bg(rgba(0xef444422))
                    .border_1()
                    .border_color(rgb(0xef4444))
                    .text_size(px(11.5))
                    .text_color(rgb(0xfca5a5))
                    .child(err.clone()),
            );
        } else if let Some(ref status) = modal.status {
            content = content.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .bg(rgba(0x3b82f622))
                    .border_1()
                    .border_color(rgb(0x3b82f6))
                    .text_size(px(11.5))
                    .text_color(rgb(0x93c5fd))
                    .child(status.clone()),
            );
        }

        // 4. Preview table
        if !modal.preview_rows.is_empty() {
            let mut preview_box = div()
                .id("import-preview-box")
                .flex()
                .flex_col()
                .max_h(px(180.0))
                .overflow_y_scroll()
                .rounded_xs()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg_editor);

            let mut header_row = div()
                .flex()
                .bg(theme.bg_surface)
                .border_b_1()
                .border_color(theme.border);
            for h in &modal.source_headers {
                header_row = header_row.child(
                    div()
                        .w(px(100.0))
                        .px_1()
                        .py_0p5()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.text_primary)
                        .child(h.clone()),
                );
            }
            preview_box = preview_box.child(header_row);

            for row in modal.preview_rows.iter().take(6) {
                let mut data_row = div().flex().border_b_1().border_color(theme.border);
                for cell in row {
                    data_row = data_row.child(
                        div()
                            .w(px(100.0))
                            .px_1()
                            .py_0p5()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(cell.clone()),
                    );
                }
                preview_box = preview_box.child(data_row);
            }
            content = content.child(preview_box);
        }

        // 5. Action buttons
        let mut buttons_row = div().flex().items_center().justify_between().mt_2();
        if modal.is_importing {
            buttons_row =
                buttons_row
                    .child(div())
                    .child(
                        tool("Cancel Import")
                            .bg(rgb(0x991b1b))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = &mut this.import_modal {
                                    if let Some(ctrl) = &m.control {
                                        ctrl.cancel();
                                    }
                                    cx.notify();
                                }
                            })),
                    );
        } else {
            buttons_row = buttons_row
                .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                    this.import_modal = None;
                    cx.notify();
                })))
                .child(tool("Start Import").bg(rgb(0x1d4ed8)).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.execute_csv_import(cx);
                    },
                )));
        }
        content = content.child(buttons_row);

        // Modal overlay frame
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("import-csv-modal")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .w(px(640.0))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_focused)
                    .bg(theme.bg_surface)
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(14.0))
                                    .text_color(theme.text_primary)
                                    .child(format!(
                                        "Import CSV Data into {}.{} ({})",
                                        modal.schema, modal.table, modal.conn_id
                                    )),
                            )
                            .child(
                                div()
                                    .id("import-modal-close")
                                    .cursor_pointer()
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .child("✕")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.import_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::ImportModal;
    use db_wire::{
        ColumnMeta, DataType,
        transfer::import::{CommitPolicy, ImportProgress},
    };
    use std::io::Write;

    #[test]
    fn import_modal_loads_csv_preview_and_handles_missing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let csv_path = temp_dir.path().join("users.csv");
        let mut file = std::fs::File::create(&csv_path).unwrap();
        writeln!(file, "id,name,score").unwrap();
        writeln!(file, "1,alice,95.5").unwrap();
        writeln!(file, "2,bob,80.0").unwrap();

        let mut modal = ImportModal {
            conn_id: "test-conn".into(),
            provider: db_wire::DbDriverKind::Sqlite,
            schema: "public".into(),
            table: "users".into(),
            driver: std::sync::Arc::new(db_wire::sqlite::SqliteDriver::new_memory()),
            file_path: String::new(),
            delimiter: b',',
            has_header: true,
            commit_policy: CommitPolicy::Atomic,
            skip_invalid: false,
            target_columns: vec![
                ColumnMeta::new("id", DataType::Int4),
                ColumnMeta::new("name", DataType::Text),
            ],
            source_headers: Vec::new(),
            preview_rows: Vec::new(),
            is_importing: false,
            progress: ImportProgress::default(),
            status: None,
            error: None,
            control: None,
        };

        modal.load_file(csv_path.display().to_string());
        assert!(modal.error.is_none());
        assert_eq!(modal.source_headers, vec!["id", "name", "score"]);
        assert_eq!(modal.preview_rows.len(), 2);
        assert_eq!(modal.preview_rows[0], vec!["1", "alice", "95.5"]);

        modal.load_file("/nonexistent/path/never_there.csv".into());
        assert!(modal.error.is_some());
    }
}
