use super::script_tools::tool;
use super::*;
use db_wire::DbDriverKind;
use db_wire::sql::schema_ddl::{
    ColumnDef, SchemaAction, generate_schema_ddl, is_ddl_transactional,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaModalField {
    Name,
    ColName,
    ColType,
    ColDefault,
    ViewDef,
}

pub struct SchemaActionModal {
    pub conn_id: String,
    pub driver_kind: DbDriverKind,
    pub initial_action: SchemaAction,
    pub active_field: SchemaModalField,

    // Input line states
    pub name_text: String,
    pub name_caret: usize,
    pub name_anchor: Option<usize>,
    pub name_history: LineHistory,

    pub col_name_text: String,
    pub col_name_caret: usize,
    pub col_name_anchor: Option<usize>,
    pub col_name_history: LineHistory,

    pub col_type_text: String,
    pub col_type_caret: usize,
    pub col_type_anchor: Option<usize>,
    pub col_type_history: LineHistory,

    pub col_default_text: String,
    pub col_default_caret: usize,
    pub col_default_anchor: Option<usize>,
    pub col_default_history: LineHistory,

    pub view_def_text: String,
    pub view_def_caret: usize,
    pub view_def_anchor: Option<usize>,
    pub view_def_history: LineHistory,

    // Toggles
    pub cascade: bool,
    pub col_nullable: bool,
    pub col_is_pk: bool,

    // For CreateTable
    pub created_columns: Vec<ColumnDef>,

    // Generated state
    pub preview_sql: String,
    pub preview_error: Option<String>,
    pub error_message: Option<String>,
    pub is_executing: bool,
}

impl SchemaActionModal {
    pub fn new(conn_id: String, driver_kind: DbDriverKind, initial_action: SchemaAction) -> Self {
        let mut modal = Self {
            conn_id,
            driver_kind,
            initial_action: initial_action.clone(),
            active_field: SchemaModalField::Name,

            name_text: String::new(),
            name_caret: 0,
            name_anchor: None,
            name_history: LineHistory::default(),

            col_name_text: String::new(),
            col_name_caret: 0,
            col_name_anchor: None,
            col_name_history: LineHistory::default(),

            col_type_text: String::new(),
            col_type_caret: 0,
            col_type_anchor: None,
            col_type_history: LineHistory::default(),

            col_default_text: String::new(),
            col_default_caret: 0,
            col_default_anchor: None,
            col_default_history: LineHistory::default(),

            view_def_text: String::new(),
            view_def_caret: 0,
            view_def_anchor: None,
            view_def_history: LineHistory::default(),

            cascade: false,
            col_nullable: true,
            col_is_pk: false,
            created_columns: Vec::new(),

            preview_sql: String::new(),
            preview_error: None,
            error_message: None,
            is_executing: false,
        };

        match &initial_action {
            SchemaAction::TruncateTable { cascade, .. } => {
                modal.cascade = *cascade;
            }
            SchemaAction::DropTable { cascade, .. } => {
                modal.cascade = *cascade;
            }
            SchemaAction::RenameTable {
                table, new_name, ..
            } => {
                modal.name_text = if new_name.is_empty() {
                    format!("{}_new", table)
                } else {
                    new_name.clone()
                };
                modal.name_caret = modal.name_text.len();
                modal.active_field = SchemaModalField::Name;
            }
            SchemaAction::CreateTable { table, columns, .. } => {
                modal.name_text = if table.is_empty() {
                    "new_table".into()
                } else {
                    table.clone()
                };
                modal.name_caret = modal.name_text.len();
                modal.created_columns = if columns.is_empty() {
                    vec![ColumnDef {
                        name: "id".into(),
                        data_type: if driver_kind == DbDriverKind::Sqlite {
                            "integer".into()
                        } else {
                            "bigint".into()
                        },
                        nullable: false,
                        is_pk: true,
                    }]
                } else {
                    columns.clone()
                };
                modal.col_name_text = "title".into();
                modal.col_name_caret = modal.col_name_text.len();
                modal.col_type_text = if driver_kind == DbDriverKind::Sqlite {
                    "text".into()
                } else {
                    "varchar(255)".into()
                };
                modal.col_type_caret = modal.col_type_text.len();
                modal.col_nullable = true;
                modal.col_is_pk = false;
                modal.active_field = SchemaModalField::Name;
            }
            SchemaAction::DropView { cascade, .. } => {
                modal.cascade = *cascade;
            }
            SchemaAction::RenameView { view, new_name, .. } => {
                modal.name_text = if new_name.is_empty() {
                    format!("{}_new", view)
                } else {
                    new_name.clone()
                };
                modal.name_caret = modal.name_text.len();
                modal.active_field = SchemaModalField::Name;
            }
            SchemaAction::CreateView {
                view, definition, ..
            } => {
                modal.name_text = if view.is_empty() {
                    "new_view".into()
                } else {
                    view.clone()
                };
                modal.name_caret = modal.name_text.len();
                modal.view_def_text = if definition.is_empty() {
                    "SELECT 1 AS id".into()
                } else {
                    definition.clone()
                };
                modal.view_def_caret = modal.view_def_text.len();
                modal.active_field = SchemaModalField::Name;
            }
            SchemaAction::AddColumn {
                column,
                data_type,
                nullable,
                default_val,
                ..
            } => {
                modal.col_name_text = if column.is_empty() {
                    "new_column".into()
                } else {
                    column.clone()
                };
                modal.col_name_caret = modal.col_name_text.len();
                modal.col_type_text = if data_type.is_empty() {
                    "text".into()
                } else {
                    data_type.clone()
                };
                modal.col_type_caret = modal.col_type_text.len();
                modal.col_nullable = *nullable;
                modal.col_default_text = default_val.clone().unwrap_or_default();
                modal.col_default_caret = modal.col_default_text.len();
                modal.active_field = SchemaModalField::ColName;
            }
            SchemaAction::DropColumn { cascade, .. } => {
                modal.cascade = *cascade;
            }
            SchemaAction::RenameColumn {
                old_name, new_name, ..
            } => {
                modal.name_text = if new_name.is_empty() {
                    format!("{}_new", old_name)
                } else {
                    new_name.clone()
                };
                modal.name_caret = modal.name_text.len();
                modal.active_field = SchemaModalField::Name;
            }
            SchemaAction::DropConstraint { cascade, .. } => {
                modal.cascade = *cascade;
            }
            SchemaAction::DropIndex { cascade, .. } => {
                modal.cascade = *cascade;
            }
        }

        modal.sync_preview();
        modal
    }

    pub fn is_destructive(&self) -> bool {
        matches!(
            self.initial_action,
            SchemaAction::TruncateTable { .. }
                | SchemaAction::DropTable { .. }
                | SchemaAction::DropView { .. }
                | SchemaAction::DropColumn { .. }
                | SchemaAction::DropConstraint { .. }
                | SchemaAction::DropIndex { .. }
        )
    }

    pub fn modal_title(&self) -> String {
        match &self.initial_action {
            SchemaAction::TruncateTable { schema, table, .. } => {
                format!("Truncate Table \"{}\".\"{}\"", schema, table)
            }
            SchemaAction::DropTable { schema, table, .. } => {
                format!("Drop Table \"{}\".\"{}\"", schema, table)
            }
            SchemaAction::RenameTable { schema, table, .. } => {
                format!("Rename Table \"{}\".\"{}\"", schema, table)
            }
            SchemaAction::CreateTable { schema, .. } => {
                format!("Create Table in Schema \"{}\"", schema)
            }
            SchemaAction::DropView { schema, view, .. } => {
                format!("Drop View \"{}\".\"{}\"", schema, view)
            }
            SchemaAction::RenameView { schema, view, .. } => {
                format!("Rename View \"{}\".\"{}\"", schema, view)
            }
            SchemaAction::CreateView { schema, .. } => {
                format!("Create View in Schema \"{}\"", schema)
            }
            SchemaAction::AddColumn { schema, table, .. } => {
                format!("Add Column to \"{}\".\"{}\"", schema, table)
            }
            SchemaAction::DropColumn {
                schema,
                table,
                column,
                ..
            } => {
                format!(
                    "Drop Column \"{}\" from \"{}\".\"{}\"",
                    column, schema, table
                )
            }
            SchemaAction::RenameColumn {
                schema,
                table,
                old_name,
                ..
            } => {
                format!(
                    "Rename Column \"{}\" in \"{}\".\"{}\"",
                    old_name, schema, table
                )
            }
            SchemaAction::DropConstraint {
                schema,
                table,
                constraint,
                ..
            } => {
                format!(
                    "Drop Constraint \"{}\" from \"{}\".\"{}\"",
                    constraint, schema, table
                )
            }
            SchemaAction::DropIndex { schema, index, .. } => {
                format!("Drop Index \"{}\".\"{}\"", schema, index)
            }
        }
    }

    pub fn current_action(&self) -> SchemaAction {
        match &self.initial_action {
            SchemaAction::TruncateTable { schema, table, .. } => SchemaAction::TruncateTable {
                schema: schema.clone(),
                table: table.clone(),
                cascade: self.cascade,
            },
            SchemaAction::DropTable { schema, table, .. } => SchemaAction::DropTable {
                schema: schema.clone(),
                table: table.clone(),
                cascade: self.cascade,
            },
            SchemaAction::RenameTable { schema, table, .. } => SchemaAction::RenameTable {
                schema: schema.clone(),
                table: table.clone(),
                new_name: self.name_text.trim().to_string(),
            },
            SchemaAction::CreateTable { schema, .. } => SchemaAction::CreateTable {
                schema: schema.clone(),
                table: self.name_text.trim().to_string(),
                columns: self.created_columns.clone(),
            },
            SchemaAction::DropView { schema, view, .. } => SchemaAction::DropView {
                schema: schema.clone(),
                view: view.clone(),
                cascade: self.cascade,
            },
            SchemaAction::RenameView { schema, view, .. } => SchemaAction::RenameView {
                schema: schema.clone(),
                view: view.clone(),
                new_name: self.name_text.trim().to_string(),
            },
            SchemaAction::CreateView { schema, .. } => SchemaAction::CreateView {
                schema: schema.clone(),
                view: self.name_text.trim().to_string(),
                definition: self.view_def_text.trim().to_string(),
            },
            SchemaAction::AddColumn { schema, table, .. } => SchemaAction::AddColumn {
                schema: schema.clone(),
                table: table.clone(),
                column: self.col_name_text.trim().to_string(),
                data_type: self.col_type_text.trim().to_string(),
                nullable: self.col_nullable,
                default_val: if self.col_default_text.trim().is_empty() {
                    None
                } else {
                    Some(self.col_default_text.trim().to_string())
                },
            },
            SchemaAction::DropColumn {
                schema,
                table,
                column,
                ..
            } => SchemaAction::DropColumn {
                schema: schema.clone(),
                table: table.clone(),
                column: column.clone(),
                cascade: self.cascade,
            },
            SchemaAction::RenameColumn {
                schema,
                table,
                old_name,
                ..
            } => SchemaAction::RenameColumn {
                schema: schema.clone(),
                table: table.clone(),
                old_name: old_name.clone(),
                new_name: self.name_text.trim().to_string(),
            },
            SchemaAction::DropConstraint {
                schema,
                table,
                constraint,
                constraint_type,
                ..
            } => SchemaAction::DropConstraint {
                schema: schema.clone(),
                table: table.clone(),
                constraint: constraint.clone(),
                constraint_type: constraint_type.clone(),
                cascade: self.cascade,
            },
            SchemaAction::DropIndex {
                schema,
                table,
                index,
                ..
            } => SchemaAction::DropIndex {
                schema: schema.clone(),
                table: table.clone(),
                index: index.clone(),
                cascade: self.cascade,
            },
        }
    }

    pub fn sync_preview(&mut self) {
        let action = self.current_action();
        match generate_schema_ddl(self.driver_kind, &action) {
            Ok(sql) => {
                self.preview_sql = sql;
                self.preview_error = None;
            }
            Err(e) => {
                self.preview_sql.clear();
                self.preview_error = Some(e);
            }
        }
    }

    pub fn add_created_column(&mut self) {
        let name = self.col_name_text.trim().to_string();
        let data_type = self.col_type_text.trim().to_string();
        if name.is_empty() || data_type.is_empty() {
            return;
        }
        self.created_columns.push(ColumnDef {
            name,
            data_type,
            nullable: self.col_nullable,
            is_pk: self.col_is_pk,
        });
        self.col_name_text.clear();
        self.col_name_caret = 0;
        self.col_name_anchor = None;
        self.col_is_pk = false;
        self.col_nullable = true;
        self.sync_preview();
    }

    pub fn remove_created_column(&mut self, index: usize) {
        if self.created_columns.len() > 1 && index < self.created_columns.len() {
            self.created_columns.remove(index);
            self.sync_preview();
        }
    }

    pub fn handle_key(&mut self, key: &Keystroke, cx: &mut App) -> bool {
        if key.key == "tab" {
            self.cycle_field(key.modifiers.shift);
            return true;
        }

        let before_text = self.active_field_text().to_string();
        let consumed = match self.active_field {
            SchemaModalField::Name => handle_line_key(
                LineInput {
                    text: &mut self.name_text,
                    caret: &mut self.name_caret,
                    anchor: &mut self.name_anchor,
                    history: &mut self.name_history,
                    digits_only: false,
                },
                key,
                cx,
            ),
            SchemaModalField::ColName => handle_line_key(
                LineInput {
                    text: &mut self.col_name_text,
                    caret: &mut self.col_name_caret,
                    anchor: &mut self.col_name_anchor,
                    history: &mut self.col_name_history,
                    digits_only: false,
                },
                key,
                cx,
            ),
            SchemaModalField::ColType => handle_line_key(
                LineInput {
                    text: &mut self.col_type_text,
                    caret: &mut self.col_type_caret,
                    anchor: &mut self.col_type_anchor,
                    history: &mut self.col_type_history,
                    digits_only: false,
                },
                key,
                cx,
            ),
            SchemaModalField::ColDefault => handle_line_key(
                LineInput {
                    text: &mut self.col_default_text,
                    caret: &mut self.col_default_caret,
                    anchor: &mut self.col_default_anchor,
                    history: &mut self.col_default_history,
                    digits_only: false,
                },
                key,
                cx,
            ),
            SchemaModalField::ViewDef => handle_line_key(
                LineInput {
                    text: &mut self.view_def_text,
                    caret: &mut self.view_def_caret,
                    anchor: &mut self.view_def_anchor,
                    history: &mut self.view_def_history,
                    digits_only: false,
                },
                key,
                cx,
            ),
        };

        if self.active_field_text() != before_text {
            self.sync_preview();
        }

        consumed
    }

    fn active_field_text(&self) -> &str {
        match self.active_field {
            SchemaModalField::Name => &self.name_text,
            SchemaModalField::ColName => &self.col_name_text,
            SchemaModalField::ColType => &self.col_type_text,
            SchemaModalField::ColDefault => &self.col_default_text,
            SchemaModalField::ViewDef => &self.view_def_text,
        }
    }

    fn cycle_field(&mut self, backward: bool) {
        match &self.initial_action {
            SchemaAction::CreateTable { .. } => {
                let fields = [
                    SchemaModalField::Name,
                    SchemaModalField::ColName,
                    SchemaModalField::ColType,
                ];
                let idx = fields
                    .iter()
                    .position(|f| *f == self.active_field)
                    .unwrap_or(0);
                let next_idx = if backward {
                    (idx + fields.len() - 1) % fields.len()
                } else {
                    (idx + 1) % fields.len()
                };
                self.active_field = fields[next_idx];
            }
            SchemaAction::CreateView { .. } => {
                let fields = [SchemaModalField::Name, SchemaModalField::ViewDef];
                let idx = fields
                    .iter()
                    .position(|f| *f == self.active_field)
                    .unwrap_or(0);
                let next_idx = if backward {
                    (idx + fields.len() - 1) % fields.len()
                } else {
                    (idx + 1) % fields.len()
                };
                self.active_field = fields[next_idx];
            }
            SchemaAction::AddColumn { .. } => {
                let fields = [
                    SchemaModalField::ColName,
                    SchemaModalField::ColType,
                    SchemaModalField::ColDefault,
                ];
                let idx = fields
                    .iter()
                    .position(|f| *f == self.active_field)
                    .unwrap_or(0);
                let next_idx = if backward {
                    (idx + fields.len() - 1) % fields.len()
                } else {
                    (idx + 1) % fields.len()
                };
                self.active_field = fields[next_idx];
            }
            _ => {
                self.active_field = SchemaModalField::Name;
            }
        }
    }
}

impl Workspace {
    pub(super) fn open_schema_action(
        &mut self,
        conn_id: &str,
        initial_action: SchemaAction,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.connections.sessions.get(conn_id) else {
            return;
        };
        let driver_kind = session.kind;
        let modal = SchemaActionModal::new(conn_id.to_string(), driver_kind, initial_action);
        self.schema_action_modal = Some(modal);
        cx.notify();
    }

    pub(super) fn execute_schema_action(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.schema_action_modal else {
            return;
        };
        if modal.is_executing
            || modal.preview_error.is_some()
            || modal.preview_sql.trim().is_empty()
        {
            return;
        }

        let sql = modal.preview_sql.clone();
        let conn_id = modal.conn_id.clone();
        let Some(session) = self.connections.sessions.get(&conn_id) else {
            modal.error_message = Some("Connection session is no longer available".into());
            cx.notify();
            return;
        };
        let Some(driver) = session.driver.clone() else {
            modal.error_message = Some("Connection is disconnected. Reconnect first.".into());
            cx.notify();
            return;
        };

        modal.is_executing = true;
        modal.error_message = None;
        cx.notify();

        let conn_id_clone = conn_id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            driver.execute_schema_ddl(&sql),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("DDL execution timed out after 15 seconds"))?
                    })
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.schema_action_modal = None;
                        this.refresh_connection_for(&conn_id_clone, cx);
                    }
                    Err(err) => {
                        if let Some(modal) = &mut this.schema_action_modal {
                            modal.is_executing = false;
                            modal.error_message = Some(err.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn schema_action_modal_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.schema_action_modal else {
            return;
        };
        match key.key.as_str() {
            "escape" => {
                self.schema_action_modal = None;
                cx.notify();
            }
            "w" if key.modifiers.platform || key.modifiers.control => {
                self.schema_action_modal = None;
                cx.notify();
            }
            "enter" | "return" => {
                if !modal.is_executing && modal.preview_error.is_none() {
                    self.execute_schema_action(cx);
                }
            }
            _ => {
                modal.handle_key(key, cx);
                cx.notify();
            }
        }
    }

    pub(super) fn render_schema_action_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let modal = self.schema_action_modal.as_ref().unwrap();
        let theme = &self.theme;

        let weak = cx.entity().downgrade();
        let make_pick = |field: SchemaModalField| -> CaretPick {
            let weak = weak.clone();
            std::rc::Rc::new(move |index, kind, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if let Some(m) = &mut this.schema_action_modal {
                        m.active_field = field;
                        match field {
                            SchemaModalField::Name => {
                                apply_pick(&mut m.name_caret, &mut m.name_anchor, index, kind, true)
                            }
                            SchemaModalField::ColName => apply_pick(
                                &mut m.col_name_caret,
                                &mut m.col_name_anchor,
                                index,
                                kind,
                                true,
                            ),
                            SchemaModalField::ColType => apply_pick(
                                &mut m.col_type_caret,
                                &mut m.col_type_anchor,
                                index,
                                kind,
                                true,
                            ),
                            SchemaModalField::ColDefault => apply_pick(
                                &mut m.col_default_caret,
                                &mut m.col_default_anchor,
                                index,
                                kind,
                                true,
                            ),
                            SchemaModalField::ViewDef => apply_pick(
                                &mut m.view_def_caret,
                                &mut m.view_def_anchor,
                                index,
                                kind,
                                true,
                            ),
                        }
                        cx.notify();
                    }
                });
            })
        };

        let mut form_content = div().flex().flex_col().gap_2();

        // 1. Danger banner for destructive actions
        if modal.is_destructive() {
            let warn_text = match &modal.initial_action {
                SchemaAction::TruncateTable { table, .. } => {
                    format!(
                        "⚠️ TRUNCATE TABLE will delete all rows from \"{table}\". This cannot be undone."
                    )
                }
                SchemaAction::DropTable { table, .. } => {
                    format!(
                        "⚠️ DROP TABLE will permanently drop table \"{table}\" and all its data."
                    )
                }
                SchemaAction::DropView { view, .. } => {
                    format!("⚠️ DROP VIEW will permanently drop view \"{view}\".")
                }
                SchemaAction::DropColumn { column, table, .. } => {
                    format!(
                        "⚠️ DROP COLUMN will remove column \"{column}\" from \"{table}\" and delete its values."
                    )
                }
                SchemaAction::DropConstraint { constraint, .. } => {
                    format!("⚠️ DROP CONSTRAINT will remove constraint \"{constraint}\".")
                }
                SchemaAction::DropIndex { index, .. } => {
                    format!("⚠️ DROP INDEX will drop index \"{index}\".")
                }
                _ => "⚠️ Warning: destructive database operation.".into(),
            };
            form_content = form_content.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .bg(rgba(0xef444422))
                    .border_1()
                    .border_color(rgb(0xef4444))
                    .text_size(px(12.0))
                    .text_color(rgb(0xfca5a5))
                    .child(warn_text),
            );
        }

        // 2. Transactionality Indicator
        let transactional = is_ddl_transactional(modal.driver_kind);
        if transactional {
            form_content = form_content.child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child("✓ Engine supports transactional DDL. Changes will roll back cleanly on failure."),
            );
        } else {
            form_content = form_content.child(
                div()
                    .px_3()
                    .py_1p5()
                    .rounded_sm()
                    .bg(rgba(0xf59e0b22))
                    .border_1()
                    .border_color(rgb(0xf59e0b))
                    .text_size(px(11.5))
                    .text_color(rgb(0xfcd34d))
                    .child("⚠️ Non-transactional DDL: This database engine automatically commits DDL. Any partial modification cannot be rolled back."),
            );
        }

        // 3. Action-specific inputs
        match &modal.initial_action {
            SchemaAction::RenameTable { .. }
            | SchemaAction::RenameView { .. }
            | SchemaAction::RenameColumn { .. } => {
                let pick = make_pick(SchemaModalField::Name);
                form_content = form_content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_secondary)
                                .child("New name:"),
                        )
                        .child(
                            ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                                .p_2()
                                .rounded_sm()
                                .bg(theme.bg_app)
                                .border_1()
                                .border_color(if modal.active_field == SchemaModalField::Name {
                                    theme.border_focused
                                } else {
                                    theme.border
                                })
                                .child(render_field_content(
                                    &modal.name_text,
                                    "Enter new name…",
                                    modal.active_field == SchemaModalField::Name,
                                    modal.name_anchor,
                                    modal.name_caret,
                                    true,
                                    theme,
                                    &pick,
                                )),
                        ),
                );
            }
            SchemaAction::CreateTable { .. } => {
                let name_pick = make_pick(SchemaModalField::Name);
                let col_name_pick = make_pick(SchemaModalField::ColName);
                let col_type_pick = make_pick(SchemaModalField::ColType);

                form_content = form_content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_secondary)
                                .child("Table name:"),
                        )
                        .child(
                            ui_editor::line_input::pick_surface(div(), usize::MAX, &name_pick)
                                .p_2()
                                .rounded_sm()
                                .bg(theme.bg_app)
                                .border_1()
                                .border_color(if modal.active_field == SchemaModalField::Name {
                                    theme.border_focused
                                } else {
                                    theme.border
                                })
                                .child(render_field_content(
                                    &modal.name_text,
                                    "Table name…",
                                    modal.active_field == SchemaModalField::Name,
                                    modal.name_anchor,
                                    modal.name_caret,
                                    true,
                                    theme,
                                    &name_pick,
                                )),
                        ),
                );

                // Columns list
                let mut cols_container = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_sm()
                    .bg(theme.bg_app)
                    .border_1()
                    .border_color(theme.border);
                cols_container = cols_container.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child("Columns:"),
                );

                for (idx, col) in modal.created_columns.iter().enumerate() {
                    let mut col_row = div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded_xs()
                        .bg(theme.bg_surface)
                        .text_size(px(11.5))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().font_weight(FontWeight::BOLD).child(col.name.clone()))
                                .child(
                                    div()
                                        .text_color(theme.text_secondary)
                                        .child(col.data_type.clone()),
                                )
                                .child(if col.is_pk {
                                    div()
                                        .px_1()
                                        .rounded_xs()
                                        .bg(rgb(0x1e3a8a))
                                        .text_size(px(9.0))
                                        .text_color(rgb(0x93c5fd))
                                        .child("PK")
                                } else {
                                    div()
                                })
                                .child(if !col.nullable {
                                    div()
                                        .text_size(px(9.0))
                                        .text_color(theme.text_warning)
                                        .child("NOT NULL")
                                } else {
                                    div()
                                }),
                        );

                    if modal.created_columns.len() > 1 {
                        col_row = col_row.child(
                            div()
                                .id(("remove-col", idx))
                                .cursor_pointer()
                                .text_color(theme.text_muted)
                                .hover(|s| s.text_color(rgb(0xef4444)))
                                .child("✕")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = &mut this.schema_action_modal {
                                        m.remove_created_column(idx);
                                        cx.notify();
                                    }
                                })),
                        );
                    }
                    cols_container = cols_container.child(col_row);
                }

                // Add column row
                let is_col_name_active = modal.active_field == SchemaModalField::ColName;
                let is_col_type_active = modal.active_field == SchemaModalField::ColType;
                let is_pk = modal.col_is_pk;
                let is_null = modal.col_nullable;

                let add_col_row =
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .mt_1()
                        .child(
                            ui_editor::line_input::pick_surface(div(), usize::MAX, &col_name_pick)
                                .flex_1()
                                .p_1p5()
                                .rounded_sm()
                                .bg(theme.bg_surface)
                                .border_1()
                                .border_color(if is_col_name_active {
                                    theme.border_focused
                                } else {
                                    theme.border
                                })
                                .child(render_field_content(
                                    &modal.col_name_text,
                                    "Column name",
                                    is_col_name_active,
                                    modal.col_name_anchor,
                                    modal.col_name_caret,
                                    true,
                                    theme,
                                    &col_name_pick,
                                )),
                        )
                        .child(
                            ui_editor::line_input::pick_surface(div(), usize::MAX, &col_type_pick)
                                .w(px(140.0))
                                .p_1p5()
                                .rounded_sm()
                                .bg(theme.bg_surface)
                                .border_1()
                                .border_color(if is_col_type_active {
                                    theme.border_focused
                                } else {
                                    theme.border
                                })
                                .child(render_field_content(
                                    &modal.col_type_text,
                                    "Type (e.g. text)",
                                    is_col_type_active,
                                    modal.col_type_anchor,
                                    modal.col_type_caret,
                                    true,
                                    theme,
                                    &col_type_pick,
                                )),
                        )
                        .child(tool(if is_pk { "PK: Yes" } else { "PK: No" }).on_click(
                            cx.listener(|this, _, _, cx| {
                                if let Some(m) = &mut this.schema_action_modal {
                                    m.col_is_pk = !m.col_is_pk;
                                    cx.notify();
                                }
                            }),
                        ))
                        .child(
                            tool(if is_null { "Null: Yes" } else { "Null: No" }).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(m) = &mut this.schema_action_modal {
                                        m.col_nullable = !m.col_nullable;
                                        cx.notify();
                                    }
                                }),
                            ),
                        )
                        .child(tool("+ Add").on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = &mut this.schema_action_modal {
                                m.add_created_column();
                                cx.notify();
                            }
                        })));

                cols_container = cols_container.child(add_col_row);
                form_content = form_content.child(cols_container);
            }
            SchemaAction::CreateView { .. } => {
                let name_pick = make_pick(SchemaModalField::Name);
                let def_pick = make_pick(SchemaModalField::ViewDef);

                form_content = form_content
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_secondary)
                                    .child("View name:"),
                            )
                            .child(
                                ui_editor::line_input::pick_surface(div(), usize::MAX, &name_pick)
                                    .p_2()
                                    .rounded_sm()
                                    .bg(theme.bg_app)
                                    .border_1()
                                    .border_color(if modal.active_field == SchemaModalField::Name {
                                        theme.border_focused
                                    } else {
                                        theme.border
                                    })
                                    .child(render_field_content(
                                        &modal.name_text,
                                        "View name…",
                                        modal.active_field == SchemaModalField::Name,
                                        modal.name_anchor,
                                        modal.name_caret,
                                        true,
                                        theme,
                                        &name_pick,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_secondary)
                                    .child("Query definition (SELECT expression):"),
                            )
                            .child(
                                ui_editor::line_input::pick_surface(div(), usize::MAX, &def_pick)
                                    .p_2()
                                    .rounded_sm()
                                    .bg(theme.bg_app)
                                    .border_1()
                                    .border_color(
                                        if modal.active_field == SchemaModalField::ViewDef {
                                            theme.border_focused
                                        } else {
                                            theme.border
                                        },
                                    )
                                    .child(render_field_content(
                                        &modal.view_def_text,
                                        "SELECT ...",
                                        modal.active_field == SchemaModalField::ViewDef,
                                        modal.view_def_anchor,
                                        modal.view_def_caret,
                                        true,
                                        theme,
                                        &def_pick,
                                    )),
                            ),
                    );
            }
            SchemaAction::AddColumn { .. } => {
                let col_name_pick = make_pick(SchemaModalField::ColName);
                let col_type_pick = make_pick(SchemaModalField::ColType);
                let col_default_pick = make_pick(SchemaModalField::ColDefault);

                let is_name_active = modal.active_field == SchemaModalField::ColName;
                let is_type_active = modal.active_field == SchemaModalField::ColType;
                let is_def_active = modal.active_field == SchemaModalField::ColDefault;
                let is_null = modal.col_nullable;

                form_content = form_content
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_secondary)
                                            .child("Column name:"),
                                    )
                                    .child(
                                        ui_editor::line_input::pick_surface(
                                            div(),
                                            usize::MAX,
                                            &col_name_pick,
                                        )
                                        .p_2()
                                        .rounded_sm()
                                        .bg(theme.bg_app)
                                        .border_1()
                                        .border_color(if is_name_active {
                                            theme.border_focused
                                        } else {
                                            theme.border
                                        })
                                        .child(
                                            render_field_content(
                                                &modal.col_name_text,
                                                "Column name…",
                                                is_name_active,
                                                modal.col_name_anchor,
                                                modal.col_name_caret,
                                                true,
                                                theme,
                                                &col_name_pick,
                                            ),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(160.0))
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_secondary)
                                            .child("Data type:"),
                                    )
                                    .child(
                                        ui_editor::line_input::pick_surface(
                                            div(),
                                            usize::MAX,
                                            &col_type_pick,
                                        )
                                        .p_2()
                                        .rounded_sm()
                                        .bg(theme.bg_app)
                                        .border_1()
                                        .border_color(if is_type_active {
                                            theme.border_focused
                                        } else {
                                            theme.border
                                        })
                                        .child(
                                            render_field_content(
                                                &modal.col_type_text,
                                                "Type…",
                                                is_type_active,
                                                modal.col_type_anchor,
                                                modal.col_type_caret,
                                                true,
                                                theme,
                                                &col_type_pick,
                                            ),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_secondary)
                                            .child("Default SQL expression (optional):"),
                                    )
                                    .child(
                                        ui_editor::line_input::pick_surface(
                                            div(),
                                            usize::MAX,
                                            &col_default_pick,
                                        )
                                        .p_2()
                                        .rounded_sm()
                                        .bg(theme.bg_app)
                                        .border_1()
                                        .border_color(if is_def_active {
                                            theme.border_focused
                                        } else {
                                            theme.border
                                        })
                                        .child(
                                            render_field_content(
                                                &modal.col_default_text,
                                                "e.g. 0 or 'active'",
                                                is_def_active,
                                                modal.col_default_anchor,
                                                modal.col_default_caret,
                                                true,
                                                theme,
                                                &col_default_pick,
                                            ),
                                        ),
                                    ),
                            )
                            .child(
                                div().pt(px(18.0)).child(
                                    tool(if is_null {
                                        "Nullable: Yes"
                                    } else {
                                        "Nullable: No"
                                    })
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            if let Some(m) = &mut this.schema_action_modal {
                                                m.col_nullable = !m.col_nullable;
                                                m.sync_preview();
                                                cx.notify();
                                            }
                                        },
                                    )),
                                ),
                            ),
                    );
            }
            _ => {}
        }

        // 4. Cascade Checkbox
        if matches!(
            modal.initial_action,
            SchemaAction::TruncateTable { .. }
                | SchemaAction::DropTable { .. }
                | SchemaAction::DropView { .. }
                | SchemaAction::DropColumn { .. }
                | SchemaAction::DropConstraint { .. }
                | SchemaAction::DropIndex { .. }
        ) {
            let cascade = modal.cascade;
            form_content = form_content.child(
                div().flex().items_center().gap_2().child(
                    tool(if cascade {
                        "[✓] CASCADE dependent objects"
                    } else {
                        "[  ] CASCADE dependent objects"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(m) = &mut this.schema_action_modal {
                            m.cascade = !m.cascade;
                            m.sync_preview();
                            cx.notify();
                        }
                    })),
                ),
            );
        }

        // 5. Error banner (if execution failed)
        if let Some(err) = &modal.error_message {
            form_content = form_content.child(
                div()
                    .p_3()
                    .rounded_sm()
                    .bg(rgba(0xef444422))
                    .border_1()
                    .border_color(rgb(0xef4444))
                    .text_size(px(12.0))
                    .text_color(rgb(0xfca5a5))
                    .child(format!("Execution Error: {}", err)),
            );
        }

        // 6. SQL Preview Box
        let mut preview_container = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_sm()
            .bg(theme.bg_app)
            .border_1()
            .border_color(theme.border);
        preview_container = preview_container.child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text_secondary)
                .child("SQL Preview:"),
        );

        if let Some(err) = &modal.preview_error {
            preview_container = preview_container.child(
                div()
                    .text_size(px(11.5))
                    .text_color(rgb(0xf87171))
                    .child(format!("Validation: {}", err)),
            );
        } else {
            preview_container = preview_container.child(
                div()
                    .id("schema-sql-preview")
                    .max_h(px(140.0))
                    .overflow_y_scroll()
                    .font_family(".AppleSystemUIFontMonospaced, Menlo, monospace")
                    .text_size(px(11.5))
                    .text_color(theme.text_accent)
                    .child(modal.preview_sql.clone()),
            );
        }
        form_content = form_content.child(preview_container);

        // 7. Action buttons
        let is_valid = modal.preview_error.is_none() && !modal.preview_sql.trim().is_empty();
        let is_exec = modal.is_executing;
        let is_destructive = modal.is_destructive();

        let execute_label = if is_exec {
            "Executing…"
        } else if is_destructive {
            "Execute Destructive Action"
        } else {
            "Execute"
        };

        let copy_sql = modal.preview_sql.clone();
        let mut exec_btn = tool(execute_label);
        if is_destructive && is_valid && !is_exec {
            exec_btn = exec_btn.bg(rgb(0x991b1b));
        } else if is_valid && !is_exec {
            exec_btn = exec_btn.bg(rgb(0x1d4ed8));
        }

        let buttons_row = div()
            .flex()
            .items_center()
            .justify_between()
            .mt_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(tool("Copy SQL").on_click(cx.listener(move |_, _, _, cx| {
                        if !copy_sql.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_sql.clone()));
                        }
                    }))),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(tool("Cancel").on_click(cx.listener(|this, _, _, cx| {
                        this.schema_action_modal = None;
                        cx.notify();
                    })))
                    .child(if is_valid && !is_exec {
                        exec_btn
                            .on_click(cx.listener(|this, _, _, cx| this.execute_schema_action(cx)))
                    } else {
                        exec_btn.opacity(0.5)
                    }),
            );

        form_content = form_content.child(buttons_row);

        // Modal outer overlay
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("schema-action-modal")
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
                                    .child(modal.modal_title()),
                            )
                            .child(
                                div()
                                    .id("schema-modal-close")
                                    .cursor_pointer()
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .child("✕")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.schema_action_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(form_content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaActionModal;
    use db_wire::DbDriverKind;
    use db_wire::sql::schema_ddl::{ColumnDef, SchemaAction};

    #[test]
    fn schema_action_modal_initializes_and_updates_preview() {
        let action = SchemaAction::RenameTable {
            schema: "public".into(),
            table: "customers".into(),
            new_name: "clients".into(),
        };
        let mut modal =
            SchemaActionModal::new("conn1".into(), DbDriverKind::Postgres, action.clone());
        assert_eq!(modal.name_text, "clients");
        assert_eq!(
            modal.preview_sql,
            "ALTER TABLE \"public\".\"customers\" RENAME TO \"clients\";"
        );
        assert!(!modal.is_destructive());

        modal.name_text = "new_clients".into();
        modal.sync_preview();
        assert_eq!(
            modal.preview_sql,
            "ALTER TABLE \"public\".\"customers\" RENAME TO \"new_clients\";"
        );
    }

    #[test]
    fn schema_action_modal_destructive_flags_and_truncate_sqlite() {
        let action = SchemaAction::TruncateTable {
            schema: "main".into(),
            table: "logs".into(),
            cascade: false,
        };
        let modal = SchemaActionModal::new("conn1".into(), DbDriverKind::Sqlite, action);
        assert!(modal.is_destructive());
        assert_eq!(modal.preview_sql, "DELETE FROM \"logs\";");
    }

    #[test]
    fn schema_action_modal_add_column_preview() {
        let action = SchemaAction::AddColumn {
            schema: "public".into(),
            table: "users".into(),
            column: "bio".into(),
            data_type: "text".into(),
            nullable: true,
            default_val: None,
        };
        let mut modal = SchemaActionModal::new("conn1".into(), DbDriverKind::Postgres, action);
        assert_eq!(
            modal.preview_sql,
            "ALTER TABLE \"public\".\"users\" ADD COLUMN \"bio\" text NULL;"
        );

        modal.col_nullable = false;
        modal.col_default_text = "'none'".into();
        modal.sync_preview();
        assert_eq!(
            modal.preview_sql,
            "ALTER TABLE \"public\".\"users\" ADD COLUMN \"bio\" text DEFAULT 'none' NOT NULL;"
        );
    }

    #[test]
    fn schema_action_modal_create_table_columns_management() {
        let action = SchemaAction::CreateTable {
            schema: "public".into(),
            table: "products".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                data_type: "bigint".into(),
                nullable: false,
                is_pk: true,
            }],
        };
        let mut modal = SchemaActionModal::new("conn1".into(), DbDriverKind::Postgres, action);
        assert_eq!(modal.created_columns.len(), 1);

        modal.col_name_text = "sku".into();
        modal.col_type_text = "varchar(50)".into();
        modal.col_is_pk = false;
        modal.col_nullable = false;
        modal.add_created_column();

        assert_eq!(modal.created_columns.len(), 2);
        assert!(modal.preview_sql.contains("\"sku\" varchar(50) NOT NULL"));

        modal.remove_created_column(1);
        assert_eq!(modal.created_columns.len(), 1);
        assert!(!modal.preview_sql.contains("\"sku\""));
    }
}
