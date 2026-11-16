use super::script_tools::tool;
use super::*;
use crate::native_backup::{self, Maintenance, Operation, RestoreTarget, process::Outcome};
use std::path::PathBuf;
use std::sync::Weak;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Backup(Vec<(String, String)>),
    Restore,
    Maintain(Option<String>),
}

#[derive(Clone, Copy)]
enum FileField {
    ClientDirectory,
    Archive,
    Destination,
    ExistingDestination,
}

pub(super) struct NativeBackupModal {
    owner: String,
    driver: Weak<dyn DatabaseDriver>,
    provider: DbDriverKind,
    database: String,
    mode: Mode,
    maintenance: Maintenance,
    client_directory: Option<PathBuf>,
    archive: Option<PathBuf>,
    destination: Option<PathBuf>,
    target: String,
    schema: super::import_modal::TextField,
    schema_active: bool,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    confirmed: bool,
    picking: bool,
    cancel: Option<watch::Sender<bool>>,
    status: String,
    log: String,
}

impl NativeBackupModal {
    fn operation(&self) -> Option<Operation> {
        Some(match &self.mode {
            Mode::Backup(scope) => Operation::Backup {
                destination: self.destination.clone()?,
                scope: scope.clone(),
            },
            Mode::Restore if self.confirmed => Operation::Restore {
                archive: self.archive.clone()?,
                target: if self.provider == DbDriverKind::Sqlite {
                    RestoreTarget::SqliteFile(self.destination.clone()?)
                } else {
                    if self.target.is_empty()
                        || (self.provider == DbDriverKind::Postgres && self.target == self.database)
                    {
                        return None;
                    }
                    if self.provider == DbDriverKind::Postgres && !self.schema.text.is_empty() {
                        RestoreTarget::PostgresSchema {
                            database: self.target.clone(),
                            schema: self.schema.text.clone(),
                        }
                    } else {
                        RestoreTarget::Database(self.target.clone())
                    }
                },
            },
            Mode::Restore => return None,
            Mode::Maintain(table) => Operation::Maintain {
                operation: self.maintenance,
                table: table.clone(),
            },
        })
    }

    fn title(&self) -> &'static str {
        match self.mode {
            Mode::Backup(_) => "Backup Database",
            Mode::Restore => "Restore Database",
            Mode::Maintain(_) => "Database Maintenance",
        }
    }

    fn path(&self, field: FileField) -> Option<&PathBuf> {
        match field {
            FileField::ClientDirectory => self.client_directory.as_ref(),
            FileField::Archive => self.archive.as_ref(),
            FileField::Destination | FileField::ExistingDestination => self.destination.as_ref(),
        }
    }
}

impl Workspace {
    pub(super) fn native_backup_running(&self) -> bool {
        self.native_backup_modal
            .as_ref()
            .is_some_and(|modal| modal.cancel.is_some())
    }

    pub(super) fn native_backup_available(&self, owner: &str, writes: bool) -> bool {
        let session = self.connections.session(owner);
        let (Some(config), Some(driver)) = (&session.connection_config, &session.driver) else {
            return false;
        };
        matches!(
            config.driver,
            DbDriverKind::Postgres | DbDriverKind::MySql | DbDriverKind::Sqlite
        ) && (!writes || (!config.read_only && !driver.is_read_only()))
            && (config.driver != DbDriverKind::Sqlite
                || (!config.database.is_empty() && config.database != ":memory:"))
            && config.tls_client_identity.is_none()
            && !(config.driver == DbDriverKind::MySql
                && config.tls_enabled
                && config.ssh_tunnel.is_some())
    }

    pub(super) fn open_native_backup(&mut self, owner: &str, mode: Mode, cx: &mut Context<Self>) {
        if self.native_backup_modal.is_some()
            || !self.native_backup_available(owner, !matches!(mode, Mode::Backup(_)))
        {
            return;
        }
        let session = self.connections.session(owner);
        let (Some(config), Some(driver)) = (&session.connection_config, &session.driver) else {
            return;
        };
        self.native_backup_modal = Some(NativeBackupModal {
            owner: owner.to_owned(),
            driver: Arc::downgrade(driver),
            provider: config.driver,
            database: config.database.clone(),
            mode,
            maintenance: Maintenance::Analyze,
            client_directory: None,
            archive: None,
            destination: None,
            target: String::new(),
            schema: Default::default(),
            schema_active: false,
            caret: 0,
            anchor: None,
            history: LineHistory::new(),
            confirmed: false,
            picking: false,
            cancel: None,
            status: String::new(),
            log: String::new(),
        });
        cx.notify();
    }

    fn choose_native_backup_path(&mut self, field: FileField, cx: &mut Context<Self>) {
        let Some(modal) = self.native_backup_modal.as_mut() else {
            return;
        };
        if modal.picking || modal.cancel.is_some() {
            return;
        }
        modal.picking = true;
        let suggested = match (&modal.mode, modal.provider) {
            (Mode::Restore, _) => "restored.sqlite",
            (_, DbDriverKind::Postgres) => "backup.dump",
            (_, DbDriverKind::MySql) => "backup.sql",
            _ => "backup.sqlite",
        };
        let picker = if matches!(field, FileField::Destination) {
            Some(cx.prompt_for_new_path(&platform_hal::paths::downloads_dir(), Some(suggested)))
        } else {
            None
        };
        let existing = if picker.is_none() {
            let directories = matches!(field, FileField::ClientDirectory);
            Some(cx.prompt_for_paths(PathPromptOptions {
                files: !directories,
                directories,
                multiple: false,
                prompt: None,
            }))
        } else {
            None
        };
        cx.spawn(async move |this, cx| {
            let result = if let Some(picker) = picker {
                picker.await.map(|result| result.map_err(|e| e.to_string()))
            } else {
                existing.unwrap().await.map(|result| {
                    result
                        .map(|paths| paths.and_then(|paths| paths.into_iter().next()))
                        .map_err(|e| e.to_string())
                })
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(modal) = &mut this.native_backup_modal {
                    modal.picking = false;
                    match result {
                        Ok(Ok(Some(path))) => {
                            match field {
                                FileField::ClientDirectory => modal.client_directory = Some(path),
                                FileField::Archive => modal.archive = Some(path),
                                FileField::Destination | FileField::ExistingDestination => {
                                    modal.destination = Some(path)
                                }
                            }
                            modal.confirmed = false;
                        }
                        Ok(Ok(None)) => {}
                        Ok(Err(error)) => modal.status = error,
                        Err(_) => modal.status = "The file picker closed unexpectedly.".into(),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn start_native_backup(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.native_backup_modal.as_mut() else {
            return;
        };
        if modal.cancel.is_some() || modal.picking {
            return;
        }
        let Some(operation) = modal.operation() else {
            return;
        };
        let session = self.connections.session(&modal.owner);
        let config = session.connection_config.as_ref().filter(|_| {
            session
                .driver
                .as_ref()
                .is_some_and(|current| Weak::ptr_eq(&modal.driver, &Arc::downgrade(current)))
        });
        let Some(config) = config.cloned() else {
            modal.status = "Connection changed. Reopen this dialog.".into();
            cx.notify();
            return;
        };
        let refresh_target = match &operation {
            Operation::Restore {
                target: RestoreTarget::Database(database),
                ..
            } => config.driver == DbDriverKind::MySql && database == &config.database,
            Operation::Restore {
                target: RestoreTarget::SqliteFile(path),
                ..
            } => path == std::path::Path::new(&config.database),
            _ => false,
        }
        .then(|| (modal.owner.clone(), modal.driver.clone()));
        let tools = modal.client_directory.clone();
        let (cancel, receiver) = watch::channel(false);
        let (sender, mut log) = watch::channel(String::new());
        modal.cancel = Some(cancel);
        modal.status = "Running…".into();
        modal.log.clear();
        modal.confirmed = false;
        let task = db_wire::get_runtime().spawn(native_backup::execute(
            config, tools, operation, receiver, sender,
        ));
        cx.spawn(async move |this, cx| {
            let timer = cx.background_executor().clone();
            while !task.is_finished() {
                timer.timer(Duration::from_millis(100)).await;
                if log.has_changed().unwrap_or(false) {
                    let text = log.borrow_and_update().clone();
                    if this
                        .update(cx, |this, cx| {
                            if let Some(modal) = &mut this.native_backup_modal {
                                modal.log = text;
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
            let result = task.await;
            let latest = log.borrow().clone();
            let _ = this.update(cx, |this, cx| {
                let completed = matches!(result, Ok(Ok(Outcome::Completed)));
                if let Some(modal) = &mut this.native_backup_modal {
                    modal.cancel = None;
                    modal.log = latest;
                    modal.status = match result {
                        Ok(Ok(Outcome::Completed)) => "Completed.".into(),
                        Ok(Ok(Outcome::Canceled)) => "Canceled.".into(),
                        Ok(Err(error)) => format!("{error:#}"),
                        Err(error) => format!("Native job failed: {error}"),
                    };
                }
                if let Some((owner, driver)) = refresh_target
                    && completed
                    && this
                        .connections
                        .session(&owner)
                        .driver
                        .as_ref()
                        .is_some_and(|current| Weak::ptr_eq(&driver, &Arc::downgrade(current)))
                {
                    this.refresh_connection_for(&owner, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn native_backup_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.native_backup_modal else {
            return;
        };
        if modal.cancel.is_some() || modal.picking {
            return;
        }
        if key.key == "escape" {
            self.native_backup_modal = None;
        } else if modal.mode == Mode::Restore && modal.provider != DbDriverKind::Sqlite {
            if key.key == "tab" && modal.provider == DbDriverKind::Postgres {
                modal.schema_active = !modal.schema_active;
                cx.notify();
                return;
            }
            if modal.schema_active {
                handle_line_key(
                    LineInput {
                        text: &mut modal.schema.text,
                        caret: &mut modal.schema.caret,
                        anchor: &mut modal.schema.anchor,
                        history: &mut modal.schema.history,
                        digits_only: false,
                    },
                    key,
                    cx,
                );
                modal.confirmed = false;
                cx.notify();
                return;
            }
            let previous = modal.target.clone();
            handle_line_key(
                LineInput {
                    text: &mut modal.target,
                    caret: &mut modal.caret,
                    anchor: &mut modal.anchor,
                    history: &mut modal.history,
                    digits_only: false,
                },
                key,
                cx,
            );
            if modal.target != previous {
                modal.confirmed = false;
            }
        }
        cx.notify();
    }

    pub(super) fn render_native_backup_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let modal = self.native_backup_modal.as_ref().unwrap();
        let theme = &self.theme;
        let editable = !modal.picking && modal.cancel.is_none();
        let path_row =
            |field, label, empty, cx: &mut Context<Self>| {
                let button = tool(label);
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(if editable {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_native_backup_path(field, cx)
                        }))
                    } else {
                        button.opacity(0.5)
                    })
                    .child(
                        div().flex_1().truncate().child(
                            modal
                                .path(field)
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| String::from(empty)),
                        ),
                    )
            };
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().font_weight(FontWeight::BOLD).child(modal.title()))
            .child(modal.database.clone())
            .child(path_row(
                FileField::ClientDirectory,
                "Client Directory…",
                "Auto-detect executable",
                cx,
            ));
        match &modal.mode {
            Mode::Backup(scope) => {
                if let Some((schema, table)) = scope.first() {
                    panel = panel.child(format!("Selected object: {schema}.{table}"));
                }
                panel = panel.child(path_row(
                    FileField::Destination,
                    "Save Backup As…",
                    "Choose an output file",
                    cx,
                ));
            }
            Mode::Restore => {
                panel = panel.child(path_row(
                    FileField::Archive,
                    "Choose Backup…",
                    "Choose an archive",
                    cx,
                ));
                if modal.provider == DbDriverKind::Sqlite {
                    panel = panel
                        .child(path_row(
                            FileField::Destination,
                            "New SQLite File…",
                            "Choose a restore destination",
                            cx,
                        ))
                        .child(path_row(
                            FileField::ExistingDestination,
                            "Existing Empty SQLite File…",
                            "",
                            cx,
                        ));
                } else {
                    let weak = cx.entity().downgrade();
                    let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if let Some(modal) = &mut this.native_backup_modal
                                && modal.cancel.is_none()
                            {
                                modal.schema_active = false;
                                apply_pick(&mut modal.caret, &mut modal.anchor, index, kind, true);
                                cx.notify();
                            }
                        });
                    });
                    panel = panel.child(
                        div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(theme.border_focused)
                            .child(render_field_content(
                                &modal.target,
                                "Separate empty database name",
                                editable && !modal.schema_active,
                                modal.anchor,
                                modal.caret,
                                self.input.cursor_visible,
                                theme,
                                &pick,
                            )),
                    );
                    if modal.provider == DbDriverKind::Postgres {
                        let weak = cx.entity().downgrade();
                        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(modal) = &mut this.native_backup_modal
                                    && modal.cancel.is_none()
                                {
                                    modal.schema_active = true;
                                    apply_pick(
                                        &mut modal.schema.caret,
                                        &mut modal.schema.anchor,
                                        index,
                                        kind,
                                        true,
                                    );
                                    cx.notify();
                                }
                            });
                        });
                        panel = panel.child("For a scoped archive, optionally create its source schema before restore.")
                            .child(div().px_2().py_1().border_1().border_color(theme.border)
                                .child(render_field_content(
                                    &modal.schema.text, "Source schema for scoped archive (optional)",
                                    editable && modal.schema_active, modal.schema.anchor,
                                    modal.schema.caret, self.input.cursor_visible, theme, &pick,
                                )));
                    }
                }
                if modal.provider == DbDriverKind::MySql {
                    panel = panel.child("Use a trusted SQL dump and a profile with privileges confined to the target database. Stored-object definers become the restore account. Failure or cancellation can leave changes applied.");
                }
                let confirm = tool(if modal.confirmed {
                    "✓ Confirmed empty target and trusted backup"
                } else {
                    "Confirm empty target and trusted backup"
                });
                panel = panel.child(if editable {
                    confirm.on_click(cx.listener(|this, _, _, cx| {
                        if let Some(modal) = &mut this.native_backup_modal {
                            modal.confirmed = !modal.confirmed;
                        }
                        cx.notify();
                    }))
                } else {
                    confirm.opacity(0.5)
                });
            }
            Mode::Maintain(table) => {
                if let Some(table) = table {
                    panel = panel.child(table.clone());
                }
                let options: &[(&str, Maintenance)] = if modal.provider == DbDriverKind::MySql {
                    &[
                        ("Analyze Table", Maintenance::Analyze),
                        ("Optimize Table", Maintenance::Vacuum),
                    ]
                } else {
                    &[
                        ("Vacuum", Maintenance::Vacuum),
                        ("Analyze", Maintenance::Analyze),
                        ("Reindex", Maintenance::Reindex),
                    ]
                };
                let mut row = div().flex().gap_2();
                for &(label, operation) in options {
                    let selected = operation == modal.maintenance;
                    let button = tool(label).bg(if selected {
                        theme.bg_surface_selected
                    } else {
                        theme.bg_surface
                    });
                    row = row.child(if editable {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(modal) = &mut this.native_backup_modal {
                                modal.maintenance = operation;
                            }
                            cx.notify();
                        }))
                    } else {
                        button.opacity(0.5)
                    });
                }
                panel = panel.child(row);
            }
        }
        if !modal.status.is_empty() {
            panel = panel.child(modal.status.clone());
        }
        if !modal.log.is_empty() {
            panel = panel.child(
                div()
                    .id("native-tool-log")
                    .max_h(px(200.0))
                    .overflow_y_scroll()
                    .bg(theme.bg_app)
                    .p_2()
                    .text_size(px(12.0))
                    .child(modal.log.clone()),
            );
        }
        let mut footer = div().flex().justify_end().gap_2();
        if let Some(cancel) = &modal.cancel {
            let button = tool(if *cancel.borrow() {
                "Canceling…"
            } else {
                "Cancel Job"
            });
            footer = footer.child(if *cancel.borrow() {
                button.opacity(0.5)
            } else {
                button.on_click(cx.listener(|this, _, _, cx| {
                    if let Some(sender) = this
                        .native_backup_modal
                        .as_ref()
                        .and_then(|modal| modal.cancel.as_ref())
                    {
                        sender.send_replace(true);
                        cx.notify();
                    }
                }))
            });
        } else {
            let close = tool("Close");
            let start = tool("Run");
            footer = footer
                .child(if editable {
                    close.on_click(cx.listener(|this, _, _, cx| {
                        this.native_backup_modal = None;
                        cx.notify();
                    }))
                } else {
                    close.opacity(0.5)
                })
                .child(if editable && modal.operation().is_some() {
                    start.on_click(cx.listener(|this, _, _, cx| this.start_native_backup(cx)))
                } else {
                    start.opacity(0.5)
                });
        }
        let frame = div()
            .w(px(620.0))
            .max_w_full()
            .max_h_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .text_color(theme.text_primary)
            .child(
                div()
                    .id("native-backup-form")
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(panel),
            )
            .child(footer.flex_none());
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(frame)
    }
}
