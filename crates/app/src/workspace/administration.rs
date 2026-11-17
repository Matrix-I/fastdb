//! Connection-bound PostgreSQL catalog administration and reviewed SQL.
use super::script_tools::tool;
use super::*;
use db_wire::administration::{
    AdminFamily, AdminObject, AdminPage, postgres_templates, validate_admin_sql,
};
use std::time::Duration;

#[derive(Default)]
struct ReviewState {
    sql: Option<String>,
    busy: bool,
    needs_refresh: bool,
}
impl ReviewState {
    fn review(&mut self, sql: String) -> anyhow::Result<()> {
        anyhow::ensure!(!self.busy, "An operation is already running");
        anyhow::ensure!(
            !self.needs_refresh,
            "Refresh the catalog and check the outcome before retrying"
        );
        validate_admin_sql(&sql)?;
        self.sql = Some(sql);
        Ok(())
    }
    fn take(&mut self, current: &str, writable: bool, live: bool) -> anyhow::Result<String> {
        anyhow::ensure!(
            !self.busy && !self.needs_refresh && writable && live,
            "The original writable connection must be available and idle"
        );
        let sql = self
            .sql
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Review SQL first"))?;
        anyhow::ensure!(sql == current, "SQL changed; review it again");
        self.busy = true;
        Ok(self.sql.take().unwrap())
    }
    fn execution_finished(&mut self, success: bool) {
        self.busy = false;
        // A failed request can include a lost commit acknowledgment.
        self.needs_refresh = !success;
    }
    fn catalog_refreshed(&mut self) {
        self.needs_refresh = false;
    }
}

pub(super) struct AdministrationDialog {
    id: u64,
    owner: String,
    connection_label: String,
    driver: Arc<dyn DatabaseDriver>,
    family: AdminFamily,
    objects: Vec<AdminObject>,
    selected: Option<usize>,
    next: Option<u32>,
    loading: bool,
    review: ReviewState,
    editor: SqlEditorView,
    templates: Vec<(&'static str, String)>,
    message: String,
    result: String,
    cancel: Option<flume::Sender<()>>,
}
impl AdministrationDialog {
    pub(super) fn executing(&self) -> bool {
        self.review.busy
    }
    fn begin(&mut self, operations: &mut Operations, live: bool) -> anyhow::Result<(String, u64)> {
        let sql = self.review.take(
            &self.editor.buffer.text(),
            !self.driver.is_read_only(),
            live && !operations.is_running(),
        )?;
        let Some(run) = operations.begin(
            self.id,
            self.owner.clone(),
            self.driver.clone(),
            OperationKind::Administration,
        ) else {
            self.review.busy = false;
            self.review.sql = Some(sql);
            anyhow::bail!("Another database operation is running");
        };
        Ok((sql, run))
    }
    fn editable(&self) -> bool {
        !self.review.busy && self.review.sql.is_none()
    }
    fn set_templates(&mut self) {
        self.templates =
            postgres_templates(self.family, self.selected.and_then(|i| self.objects.get(i)));
    }
}

pub(super) fn allows_departure(dialog: Option<&AdministrationDialog>) -> bool {
    !dialog.is_some_and(AdministrationDialog::executing)
}

impl Workspace {
    fn administration_live(&self) -> bool {
        self.administration.as_ref().is_some_and(|dialog| {
            self.connections
                .sessions
                .get(&dialog.owner)
                .and_then(|s| s.driver.as_ref())
                .is_some_and(|d| Arc::ptr_eq(d, &dialog.driver))
        })
    }
    pub(super) fn open_administration(&mut self, owner: &str, cx: &mut Context<Self>) {
        if self.administration.is_some() || self.operations.is_running() {
            return;
        }
        let Some(session) = self.connections.sessions.get(owner) else {
            return;
        };
        let Some(driver) = session.driver.clone() else {
            return;
        };
        if !driver.supports_administration() {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let profile = self
            .connections
            .saved_connections
            .iter()
            .find(|profile| profile.id == owner)
            .map_or("PostgreSQL", |profile| profile.name.as_str());
        let connection_label = session.connection_config.as_ref().map_or_else(
            || profile.to_owned(),
            |config| {
                format!(
                    "{profile} · {} · {}:{}",
                    config.database, config.host, config.port
                )
            },
        );
        self.administration = Some(AdministrationDialog {
            id,
            owner: owner.into(),
            connection_label,
            driver,
            family: AdminFamily::Routines,
            objects: Vec::new(),
            selected: None,
            next: None,
            loading: false,
            review: ReviewState::default(),
            editor: SqlEditorView::new("", self.theme.clone(), None),
            templates: Vec::new(),
            message: String::new(),
            result: String::new(),
            cancel: None,
        });
        self.load_administration(AdminFamily::Routines, 0, cx);
    }
    fn load_administration(&mut self, family: AdminFamily, after: u32, cx: &mut Context<Self>) {
        let live = self.administration_live();
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if dialog.loading || dialog.review.busy || dialog.review.sql.is_some() {
            return;
        }
        if !live {
            dialog.message = "Original connection changed. Close and reopen Administration.".into();
            cx.notify();
            return;
        }
        dialog.family = family;
        dialog.objects.clear();
        dialog.selected = None;
        dialog.next = None;
        dialog.loading = true;
        dialog.message = "Loading catalog…".into();
        dialog.set_templates();
        let id = dialog.id;
        let driver = dialog.driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(
                            Duration::from_secs(16),
                            driver.administration_catalog(family, after),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("Catalog request timed out"))?
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let live = this.administration_live();
                let Some(dialog) = this.administration.as_mut().filter(|d| d.id == id) else {
                    return;
                };
                dialog.loading = false;
                if !live {
                    dialog.message =
                        "Original connection changed. Close and reopen Administration.".into();
                } else {
                    match result {
                        Ok(AdminPage { objects, next }) => {
                            dialog.review.catalog_refreshed();
                            dialog.message = if objects.is_empty() {
                                "No objects found.".into()
                            } else if next.is_some() {
                                "More objects available: use Next page.".into()
                            } else {
                                "Catalog loaded. Select an object or choose a creation template."
                                    .into()
                            };
                            dialog.objects = objects;
                            dialog.next = next;
                        }
                        Err(error) => dialog.message = format!("Catalog failed: {error:#}"),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn administration_select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if !dialog.editable() || dialog.loading {
            return;
        }
        let Some(object) = dialog.objects.get(index) else {
            return;
        };
        dialog.message = object.details.clone();
        if object.source.is_none() {
            dialog.message.push_str("\nNo editable native source is available for this object; choose an operation template.");
        }
        dialog.selected = Some(index);
        dialog.set_templates();
        if let Some((_, sql)) = dialog.templates.first() {
            dialog.editor = SqlEditorView::new(sql, self.theme.clone(), None);
        }
        cx.notify();
    }
    fn administration_review(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if let Err(error) = dialog.review.review(dialog.editor.buffer.text()) {
            dialog.result = error.to_string();
        }
        cx.notify();
    }
    fn administration_execute(&mut self, cx: &mut Context<Self>) {
        let live = self.administration_live();
        let Some(dialog) = &mut self.administration else {
            return;
        };
        let (sql, run) = match dialog.begin(&mut self.operations, live) {
            Ok(started) => started,
            Err(error) => {
                dialog.result = error.to_string();
                cx.notify();
                return;
            }
        };
        let (send, cancel) = flume::bounded(1);
        dialog.cancel = Some(send);
        dialog.result = "Executing reviewed SQL…".into();
        let id = dialog.id;
        let driver = dialog.driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                db_wire::get_runtime().block_on(driver.execute_administration(&sql, cancel))
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.operations.finish(run);
                let live = this.administration_live();
                let Some(dialog) = this.administration.as_mut().filter(|d| d.id == id) else { return; };
                let success = result.is_ok();
                dialog.review.execution_finished(success);
                dialog.cancel = None;
                dialog.result = match result {
                    Ok(outcome) => {
                        let mut text = format!("Completed and committed. Rows affected/returned: {}.\n", outcome.affected);
                        if !outcome.columns.is_empty() { text.push_str(&outcome.columns.join(" | ")); text.push('\n'); }
                        for row in outcome.rows { text.push_str(&row.into_iter().map(|v| match v { Some(s) => format!("{s:?}"), None => "SQL NULL".into() }).collect::<Vec<_>>().join(" | ")); text.push('\n'); }
                        if outcome.truncated { text.push_str("Preview truncated (100 rows, 32 columns, 512 characters per cell).\n"); }
                        text
                    }
                    Err(error) => format!("Administration failed: {error:#}"),
                };
                if !live { dialog.message = "Original connection changed. Outcome belongs to the original connection; close and reopen.".into(); }
                let owner = dialog.owner.clone(); let family = dialog.family;
                if success && live {
                    this.refresh_connection_for(&owner, cx);
                    this.load_administration(family, 0, cx);
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn administration_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if key.key == "escape" {
            if dialog.review.busy {
                return;
            }
            if dialog.review.sql.take().is_none() {
                self.administration = None;
            }
        } else if dialog.editable() {
            dialog.editor.handle_input_key(key, cx);
        }
        cx.notify();
    }
    pub(super) fn render_administration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let pick: ui_editor::EditorPick = std::rc::Rc::new(move |line, col, kind, _, app| {
            let _ = weak.update(app, |this, cx| {
                if let Some(dialog) = this.administration.as_mut().filter(|d| d.editable()) {
                    dialog.editor.pick(line, col, kind);
                    cx.notify();
                }
            });
        });
        let live = self.administration_live();
        let dialog = self.administration.as_mut().unwrap();
        let ready = !dialog.loading && dialog.editable();
        let mut families = div().flex().flex_wrap().gap_1();
        for family in AdminFamily::ALL {
            let button = tool(family.label());
            families = families.child(if ready {
                button.on_click(
                    cx.listener(move |this, _, _, cx| this.load_administration(family, 0, cx)),
                )
            } else {
                button.opacity(0.5)
            });
        }
        let mut rows = div()
            .id("admin-catalog")
            .max_h(px(150.))
            .overflow_y_scroll();
        for (index, object) in dialog.objects.iter().enumerate() {
            let title = format!("{}.{} {}", object.schema, object.name, object.parent);
            let row = div().id(("admin-object", index)).px_2().py_1().child(title);
            rows = rows.child(if ready {
                row.cursor_pointer().on_click(
                    cx.listener(move |this, _, _, cx| this.administration_select(index, cx)),
                )
            } else {
                row
            });
        }
        let mut templates = div().flex().flex_wrap().gap_1();
        for (index, (label, _)) in dialog.templates.iter().enumerate() {
            let button = tool(label);
            templates = templates.child(if ready {
                button.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(dialog) = this.administration.as_mut().filter(|d| d.editable()) {
                        dialog.editor = SqlEditorView::new(
                            &dialog.templates[index].1,
                            this.theme.clone(),
                            None,
                        );
                        cx.notify();
                    }
                }))
            } else {
                button.opacity(0.5)
            });
        }
        let family = dialog.family;
        let mut actions = div().flex().gap_2();
        if ready {
            actions = actions.child(tool("Refresh").on_click(
                cx.listener(move |this, _, _, cx| this.load_administration(family, 0, cx)),
            ));
            if let Some(after) = dialog.next {
                actions = actions.child(tool("Next page").on_click(
                    cx.listener(move |this, _, _, cx| this.load_administration(family, after, cx)),
                ));
            }
        }
        if dialog.review.busy {
            actions = actions.child(tool("Cancel operation").on_click(cx.listener(
                |this, _, _, cx| {
                    if let Some(dialog) = &mut this.administration
                        && let Some(cancel) = &dialog.cancel
                    {
                        match cancel.try_send(()) {
                            Ok(()) | Err(flume::TrySendError::Full(_)) => {
                                dialog.result =
                                    "Cancellation requested; waiting for final outcome…".into()
                            }
                            Err(flume::TrySendError::Disconnected(_)) => {
                                dialog.result =
                                    "Operation already finishing; waiting for final outcome…".into()
                            }
                        }
                    }
                    cx.notify();
                },
            )));
        } else if dialog.review.sql.is_some() {
            actions = actions
                .child(tool("Go back").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(d) = &mut this.administration {
                        d.review.sql = None;
                    }
                    cx.notify();
                })))
                .child(
                    tool("Confirm and execute")
                        .on_click(cx.listener(|this, _, _, cx| this.administration_execute(cx))),
                );
        } else {
            if ready && live && !dialog.driver.is_read_only() && !dialog.review.needs_refresh {
                actions =
                    actions
                        .child(tool("Review SQL…").on_click(
                            cx.listener(|this, _, _, cx| this.administration_review(cx)),
                        ));
            }
            actions = actions.child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                this.administration = None;
                cx.notify();
            })));
        }
        let warning = if dialog.driver.is_read_only() {
            "Read-only profile: browsing is available; execution is disabled."
        } else if dialog.review.needs_refresh {
            "Refresh the catalog and check the server state before retrying."
        } else if dialog.review.sql.is_some() {
            "Review the exact SQL below. Confirm executes it on this profile and may change data, permissions or object definitions. Routine effects may be irreversible."
        } else {
            "Edit native SQL below. Routine arguments start as typed NULL values; replace them before review. One statement per operation."
        };
        dialog.editor.theme = theme.clone();
        let editor =
            dialog
                .editor
                .render_editor(dialog.editable(), self.input.cursor_visible, pick, window);
        let scroll = dialog.editor.scroll_handle.clone();
        let panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .w(px(1000.))
            .max_w_full()
            .h(relative(0.92))
            .min_h_0()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .rounded_md()
            .child(format!("Administration · {}", dialog.connection_label))
            .child(families)
            .child(rows)
            .child(
                div()
                    .id("admin-details")
                    .max_h(px(90.))
                    .overflow_y_scroll()
                    .child(dialog.message.clone()),
            )
            .child(templates)
            .child(warning)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(editor),
                    )
                    .child(properties::grid::detail_scrollbar(
                        scroll.clone(),
                        true,
                        &theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(scroll, false, &theme))
            .child(
                div()
                    .id("admin-outcome")
                    .max_h(px(140.))
                    .overflow_y_scroll()
                    .child(dialog.result.clone()),
            )
            .child(actions);
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
            .child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewState;
    fn reviewed_dialog() -> super::AdministrationDialog {
        let driver = db_wire::create_driver(db_wire::ConnectionConfig {
            driver: db_wire::DbDriverKind::Sqlite,
            database: ":memory:".into(),
            ..Default::default()
        })
        .unwrap();
        let mut review = ReviewState::default();
        review.review("SELECT 1".into()).unwrap();
        super::AdministrationDialog {
            id: 99,
            owner: "original".into(),
            connection_label: "Test profile".into(),
            driver,
            family: db_wire::administration::AdminFamily::Routines,
            objects: Vec::new(),
            selected: None,
            next: None,
            loading: false,
            review,
            editor: ui_editor::SqlEditorView::new("SELECT 1", ui_core::theme::Theme::dark(), None),
            templates: Vec::new(),
            message: String::new(),
            result: String::new(),
            cancel: None,
        }
    }
    #[test]
    fn administration_owns_the_slot_when_a_deferred_save_run_completes() {
        use super::super::operations::{OperationKind, Operations};
        let mut dialog = reviewed_dialog();
        let mut operations = Operations::default();
        let (saved, completion) = flume::bounded(1);
        let (_, run) = dialog.begin(&mut operations, true).unwrap();
        saved.send(()).unwrap();
        completion.recv().unwrap();
        assert!(operations.is_running());
        assert!(
            operations
                .begin(
                    1,
                    "query".into(),
                    dialog.driver.clone(),
                    OperationKind::Query
                )
                .is_none()
        );
        // Generic cancellation must not target the profile's unrelated client.
        assert!(operations.request_cancel().is_none());
        assert!(!operations.finish(run + 1));
        assert!(operations.finish(run));
        assert!(
            operations
                .begin(
                    1,
                    "query".into(),
                    dialog.driver.clone(),
                    OperationKind::Query
                )
                .is_some()
        );
    }
    #[test]
    fn failed_administration_execution_requires_a_catalog_refresh_before_replay() {
        let mut dialog = reviewed_dialog();
        let mut operations = super::super::Operations::default();
        let (_, run) = dialog.begin(&mut operations, true).unwrap();
        assert!(operations.finish(run));
        dialog.review.execution_finished(false);
        assert!(dialog.review.review("SELECT 1".into()).is_err());
        dialog.review.catalog_refreshed();
        assert!(dialog.review.review("SELECT 1".into()).is_ok());
    }
    #[test]
    fn deferred_quit_and_restart_recheck_administration_before_departure() {
        let mut dialog = reviewed_dialog();
        let mut operations = super::super::Operations::default();
        assert!(super::allows_departure(Some(&dialog)));
        dialog.begin(&mut operations, true).unwrap();
        // A flush begun while idle is no longer permission to exit.
        assert!(!super::allows_departure(Some(&dialog)));
        dialog.review.busy = false;
        assert!(super::allows_departure(Some(&dialog)));
        assert!(super::allows_departure(None));
    }
    #[test]
    fn administration_requires_exact_review_and_live_writable_profile() {
        let mut review = ReviewState::default();
        assert!(review.take("DROP ROLE x", true, true).is_err());
        review.review("ALTER ROLE x NOLOGIN".into()).unwrap();
        assert!(review.take("DROP ROLE x", true, true).is_err());
        assert!(review.take("ALTER ROLE x NOLOGIN", false, true).is_err());
        assert!(review.take("ALTER ROLE x NOLOGIN", true, false).is_err());
        assert_eq!(
            review.take("ALTER ROLE x NOLOGIN", true, true).unwrap(),
            "ALTER ROLE x NOLOGIN"
        );
        assert!(review.busy);
        assert!(review.review("SELECT 1".into()).is_err());
    }
}
