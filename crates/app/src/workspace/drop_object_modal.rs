use super::script_tools::tool;
use super::*;
use db_wire::{ConnectionConfig, sql::SqlDialect};
use std::{sync::Weak, time::Duration};

enum DropTarget {
    Schema(String),
    Database(String),
}

impl DropTarget {
    fn name(&self) -> &str {
        match self {
            Self::Schema(name) | Self::Database(name) => name,
        }
    }

    fn sql(&self, cascade: bool) -> String {
        let name = SqlDialect(DbDriverKind::Postgres).quote_ident(self.name());
        match self {
            Self::Schema(_) => {
                let mode = if cascade { "CASCADE" } else { "RESTRICT" };
                format!("DROP SCHEMA {name} {mode}")
            }
            Self::Database(_) => format!("DROP DATABASE {name}"),
        }
    }

    fn confirmed(&self, typed: &str) -> bool {
        match self {
            Self::Schema(_) => true,
            Self::Database(name) => !name.is_empty() && typed == name,
        }
    }

    fn maintenance_database(&self) -> Option<&'static str> {
        match self {
            Self::Database(name) if name == "postgres" => Some("template1"),
            Self::Database(_) => Some("postgres"),
            Self::Schema(_) => None,
        }
    }
}

pub(super) struct DropObjectModal {
    owner: String,
    source: Weak<dyn DatabaseDriver>,
    target: DropTarget,
    connection_label: String,
    cascade: bool,
    typed: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    pub(super) busy: bool,
    error: Option<String>,
}

async fn prepare_maintenance_driver(
    mut config: ConnectionConfig,
    database: &str,
) -> anyhow::Result<Arc<dyn DatabaseDriver>> {
    config.database = database.to_owned();
    let timeout = config.connect_timeout;
    let driver = create_driver(config)?;
    tokio::time::timeout(timeout, driver.ping())
        .await
        .map_err(|_| anyhow::anyhow!("Maintenance connection timed out"))??;
    Ok(driver)
}

async fn execute_drop(driver: Arc<dyn DatabaseDriver>, sql: String) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(30), driver.execute_schema_ddl(&sql))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Deletion timed out. Check whether the object still exists before retrying."
            )
        })?
}

impl Workspace {
    pub(super) fn can_drop_postgres_object(&self, owner: &str) -> bool {
        let session = self.connections.session(owner);
        !self.form.is_connecting
            && session.kind == DbDriverKind::Postgres
            && session
                .driver
                .as_ref()
                .is_some_and(|driver| !driver.is_read_only())
    }

    pub(super) fn open_drop_schema(&mut self, owner: &str, schema: String, cx: &mut Context<Self>) {
        self.open_drop_object(owner, DropTarget::Schema(schema), cx);
    }

    pub(super) fn open_drop_database(&mut self, owner: &str, cx: &mut Context<Self>) {
        let Some(config) = &self.connections.session(owner).connection_config else {
            return;
        };
        self.open_drop_object(owner, DropTarget::Database(config.database.clone()), cx);
    }

    fn open_drop_object(&mut self, owner: &str, target: DropTarget, cx: &mut Context<Self>) {
        if self.drop_object_modal.is_some() || !self.can_drop_postgres_object(owner) {
            return;
        }
        let session = self.connections.session(owner);
        let (Some(driver), Some(config)) = (&session.driver, &session.connection_config) else {
            return;
        };
        let connection_label = format!("{} · {}:{}", config.database, config.host, config.port);
        self.drop_object_modal = Some(DropObjectModal {
            owner: owner.to_owned(),
            source: Arc::downgrade(driver),
            target,
            connection_label,
            cascade: false,
            typed: String::new(),
            caret: 0,
            anchor: None,
            history: LineHistory::new(),
            busy: false,
            error: None,
        });
        cx.notify();
    }

    fn drop_object_blocker(&self, modal: &DropObjectModal) -> Option<&'static str> {
        let session = self.connections.session(&modal.owner);
        if !self.can_drop_postgres_object(&modal.owner)
            || !session
                .driver
                .as_ref()
                .is_some_and(|driver| Weak::ptr_eq(&modal.source, &Arc::downgrade(driver)))
        {
            return Some(
                "The original connection is no longer available. Close this dialog and reconnect.",
            );
        }
        if self
            .operations
            .active()
            .is_some_and(|op| op.connection_id == modal.owner)
        {
            return Some("Wait for the current database operation to finish.");
        }
        if self.tabs.iter().any(|tab| {
            tab.connection_id.as_deref() == Some(&modal.owner)
                && (tab.grid.changeset.is_dirty()
                    || tab.grid.editing_cell.is_some()
                    || tab
                        .column_editor
                        .as_ref()
                        .is_some_and(|column| column.dirty() || column.busy)
                    || !matches!(tab.transaction_state.as_str(), "Idle" | "Session closed"))
        }) {
            return Some(
                "Close this dialog and finish pending edits or transactions on this connection first.",
            );
        }
        None
    }

    fn submit_drop_object(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &self.drop_object_modal else {
            return;
        };
        if modal.busy || !modal.target.confirmed(&modal.typed) {
            return;
        }
        if let Some(error) = self.drop_object_blocker(modal) {
            self.drop_object_modal.as_mut().unwrap().error = Some(error.into());
            cx.notify();
            return;
        }

        let owner = modal.owner.clone();
        let sql = modal.target.sql(modal.cascade);
        let session = self.connections.session(&owner);
        let maintenance = modal.target.maintenance_database();
        let config = session.connection_config.clone();
        let schema_driver = if maintenance.is_none() {
            session.driver.clone()
        } else {
            None
        };
        let modal = self.drop_object_modal.as_mut().unwrap();
        modal.busy = true;
        modal.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let driver = if let Some(database) = maintenance {
                let prepared = cx
                    .background_executor()
                    .spawn(async move {
                        let config = config.ok_or_else(|| {
                            anyhow::anyhow!("Live connection settings are unavailable")
                        })?;
                        db_wire::get_runtime()
                            .block_on(prepare_maintenance_driver(config, database))
                    })
                    .await;
                let driver = match prepared {
                    Ok(driver) => driver,
                    Err(error) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_drop_object(
                                Err(error.context("Could not open the maintenance connection")),
                                cx,
                            )
                        });
                        return;
                    }
                };
                let released = this
                    .update(cx, |this, cx| {
                        let Some(modal) = &this.drop_object_modal else {
                            return false;
                        };
                        if let Some(error) = this.drop_object_blocker(modal) {
                            this.finish_drop_object(Err(anyhow::anyhow!(error)), cx);
                            return false;
                        }
                        this.disconnect_by_id_now(&owner);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !released {
                    return;
                }
                driver
            } else {
                let Some(driver) = schema_driver else {
                    return;
                };
                driver
            };

            let result = cx
                .background_executor()
                .spawn(async move { db_wire::get_runtime().block_on(execute_drop(driver, sql)) })
                .await;
            let result = if maintenance.is_some() {
                result.map_err(|error| {
                    error.context(
                    "FastDB disconnected this profile. Reconnect it if the database still exists",
                )
                })
            } else {
                result
            };
            let _ = this.update(cx, |this, cx| this.finish_drop_object(result, cx));
        })
        .detach();
    }

    fn finish_drop_object(&mut self, result: anyhow::Result<()>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                let Some(modal) = self.drop_object_modal.take() else {
                    return;
                };
                for tab in &mut self.tabs {
                    if tab.connection_id.as_deref() == Some(&modal.owner)
                        && tab.column_editor.is_none()
                    {
                        tab.invalidate_dropped_object_results();
                    }
                }
                if matches!(modal.target, DropTarget::Schema(_)) {
                    self.refresh_connection_for(&modal.owner, cx);
                }
            }
            Err(error) => {
                if let Some(modal) = &mut self.drop_object_modal {
                    modal.busy = false;
                    modal.error = Some(format!("{error:#}"));
                }
            }
        }
        cx.notify();
    }

    pub(super) fn drop_object_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.drop_object_modal else {
            return;
        };
        if modal.busy {
            return;
        }
        if key.key == "escape" {
            self.drop_object_modal = None;
        } else if matches!(modal.target, DropTarget::Database(_)) {
            handle_line_key(
                LineInput {
                    text: &mut modal.typed,
                    caret: &mut modal.caret,
                    anchor: &mut modal.anchor,
                    history: &mut modal.history,
                    digits_only: false,
                },
                key,
                cx,
            );
        }
        cx.notify();
    }

    pub(super) fn render_drop_object_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let modal = self.drop_object_modal.as_ref().unwrap();
        let theme = &self.theme;
        let is_database = matches!(modal.target, DropTarget::Database(_));
        let kind = if is_database { "database" } else { "schema" };
        let mut panel = div()
            .w(px(520.0))
            .max_w_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .text_color(theme.text_primary)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(format!("Delete {kind} \"{}\"?", modal.target.name())),
            );
        panel = panel.child(
            div()
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .child(modal.connection_label.clone()),
        );
        if is_database {
            let weak = cx.entity().downgrade();
            let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if let Some(modal) = &mut this.drop_object_modal {
                        apply_pick(&mut modal.caret, &mut modal.anchor, index, kind, true);
                        cx.notify();
                    }
                });
            });
            panel = panel
                .child("This permanently deletes the database and its data. Type its exact name to confirm.")
                .child(div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(theme.border_focused)
                    .child(render_field_content(
                        &modal.typed, "Database name", !modal.busy, modal.anchor,
                        modal.caret, self.input.cursor_visible, theme, &pick,
                    )));
        } else {
            let mut cascade = div()
                .id("drop-schema-cascade")
                .flex()
                .items_center()
                .gap_3()
                .w_full()
                .min_h(px(44.0))
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(if modal.cascade {
                    theme.border_focused
                } else {
                    theme.border
                })
                .bg(if modal.cascade {
                    theme.bg_surface_selected
                } else {
                    theme.bg_surface
                })
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(22.0))
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.text_primary)
                        .text_size(px(17.0))
                        .font_weight(FontWeight::BOLD)
                        .child(if modal.cascade { "✓" } else { "" }),
                )
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("CASCADE · Delete dependent objects"),
                );
            if !modal.busy {
                cascade = cascade
                    .cursor_pointer()
                    .hover(|row| row.border_color(theme.border_focused))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(modal) = &mut this.drop_object_modal {
                            modal.cascade = !modal.cascade;
                            cx.notify();
                        }
                    }));
            } else {
                cascade = cascade.opacity(0.6);
            }
            panel = panel
                .child("CASCADE permanently removes objects in this schema and may remove dependent objects in other schemas.")
                .child(cascade);
        }
        if let Some(error) = &modal.error {
            panel = panel.child(div().text_color(rgb(0xf85149)).child(error.clone()));
        }
        let delete = tool(if modal.busy { "Deleting…" } else { "Delete" });
        let cancel = tool("Cancel");
        panel = panel
            .child(
                div()
                    .text_color(theme.text_accent)
                    .child(modal.target.sql(modal.cascade)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(if modal.busy {
                        cancel.opacity(0.5)
                    } else {
                        cancel.on_click(cx.listener(|this, _, _, cx| {
                            this.drop_object_modal = None;
                            cx.notify();
                        }))
                    })
                    .child(if modal.busy || !modal.target.confirmed(&modal.typed) {
                        delete.opacity(0.5)
                    } else {
                        delete
                            .bg(rgb(0x991b1b))
                            .on_click(cx.listener(|this, _, _, cx| this.submit_drop_object(cx)))
                    }),
            );
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
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(panel)
    }
}
#[cfg(test)]
mod tests {
    use super::DropTarget;

    #[test]
    fn schema_drop_quotes_the_name_and_requires_explicit_cascade() {
        let target = DropTarget::Schema("my\"schema".into());
        assert_eq!(target.sql(false), "DROP SCHEMA \"my\"\"schema\" RESTRICT");
        assert_eq!(target.sql(true), "DROP SCHEMA \"my\"\"schema\" CASCADE");
        assert!(target.confirmed(""));
    }

    #[test]
    fn database_drop_requires_the_exact_name_and_never_forces_other_sessions() {
        let target = DropTarget::Database("Case\"Sensitive".into());
        assert!(!DropTarget::Database(String::new()).confirmed(""));
        assert!(!target.confirmed("case\"sensitive"));
        assert!(!target.confirmed("Case\"Sensitive "));
        assert!(target.confirmed("Case\"Sensitive"));
        assert_eq!(target.sql(true), "DROP DATABASE \"Case\"\"Sensitive\"");
    }

    #[test]
    fn maintenance_database_is_different_from_the_target() {
        assert_eq!(
            DropTarget::Database("app".into()).maintenance_database(),
            Some("postgres")
        );
        assert_eq!(
            DropTarget::Database("postgres".into()).maintenance_database(),
            Some("template1")
        );
        assert_eq!(
            DropTarget::Schema("public".into()).maintenance_database(),
            None
        );
    }
    #[test]
    fn postgres_live_drop_schema_and_database() {
        let Ok(port) = std::env::var("FASTDB_TEST_NAV_DROP_PORT") else {
            return;
        };
        let mut form = super::super::connection_state::ConnectionFormState::from_saved(None);
        form.form_host = "127.0.0.1".into();
        form.form_port = port;
        form.form_user = "fastdb_drop_test".into();
        form.form_use_unix_socket = false;
        form.form_database = "fastdb_drop_fixture".into();
        let config = form.build_config().unwrap();
        let runtime = db_wire::get_runtime();
        runtime.block_on(async {
            let database = DropTarget::Database(config.database.clone());
            let admin = super::prepare_maintenance_driver(
                config.clone(),
                database.maintenance_database().unwrap(),
            )
            .await
            .unwrap();
            admin
                .execute_schema_ddl("CREATE DATABASE fastdb_drop_fixture")
                .await
                .unwrap();
            let target_driver = db_wire::create_driver(config.clone()).unwrap();
            target_driver.ping().await.unwrap();
            target_driver.execute_schema_ddl(
                "CREATE SCHEMA \"Mixed Case\"; CREATE TABLE \"Mixed Case\".fixture (id integer)"
            ).await.unwrap();

            let schema = DropTarget::Schema("Mixed Case".into());
            let error = super::execute_drop(target_driver.clone(), schema.sql(false))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("depend on it"), "{error:#}");
            super::execute_drop(target_driver.clone(), schema.sql(true))
                .await
                .unwrap();
            assert!(
                target_driver
                    .list_tables()
                    .await
                    .unwrap()
                    .iter()
                    .all(|table| table.schema != "Mixed Case")
            );

            let error = super::execute_drop(admin.clone(), database.sql(false))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("being accessed by other users"),
                "{error:#}"
            );
            target_driver.ping().await.unwrap();
            drop(target_driver);
            super::execute_drop(admin, database.sql(false))
                .await
                .unwrap();
            let missing = db_wire::create_driver(config)
                .unwrap()
                .ping()
                .await
                .unwrap_err();
            assert!(
                format!("{missing:#}").contains("does not exist"),
                "{missing:#}"
            );
        });
    }
}
