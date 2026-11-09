use super::*;
use db_wire::sql::schema_ddl::SchemaAction;

#[derive(Debug, Clone)]
pub enum NavContextTarget {
    Connection {
        conn_id: String,
    },
    Table {
        conn_id: String,
        schema: String,
        table: String,
    },
    View {
        conn_id: String,
        schema: String,
        view: String,
    },
    Folder {
        conn_id: String,
        schema: String,
        is_views: bool,
    },
    Column {
        conn_id: String,
        schema: String,
        table: String,
        column: String,
    },
    Constraint {
        conn_id: String,
        schema: String,
        table: String,
        constraint: String,
        constraint_type: Option<String>,
    },
    Index {
        conn_id: String,
        schema: String,
        table: String,
        index: String,
    },
}

/// Right-click menu of a navigator node (connection, relation, folder, or detail item)
#[derive(Debug, Clone)]
pub struct NavContextMenu {
    pub target: NavContextTarget,
    /// Window position of the right click; the menu opens there
    pub position: Point<Pixels>,
}

impl NavContextMenu {
    pub fn conn_id(&self) -> &str {
        match &self.target {
            NavContextTarget::Connection { conn_id }
            | NavContextTarget::Table { conn_id, .. }
            | NavContextTarget::View { conn_id, .. }
            | NavContextTarget::Folder { conn_id, .. }
            | NavContextTarget::Column { conn_id, .. }
            | NavContextTarget::Constraint { conn_id, .. }
            | NavContextTarget::Index { conn_id, .. } => conn_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NavMenuAction {
    EditConnection,
    Connect,
    Disconnect,
    Delete,
    Refresh,
    OpenSqlEditor,

    // Schema Actions
    OpenData {
        schema: String,
        table: String,
    },
    ImportCsv {
        schema: String,
        table: String,
    },
    TransferTable {
        schema: String,
        table: String,
    },
    RenameTable {
        schema: String,
        table: String,
    },
    TruncateTable {
        schema: String,
        table: String,
    },
    DropTable {
        schema: String,
        table: String,
    },
    AddColumn {
        schema: String,
        table: String,
    },
    RenameView {
        schema: String,
        view: String,
    },
    DropView {
        schema: String,
        view: String,
    },
    CreateTable {
        schema: String,
    },
    CreateView {
        schema: String,
    },
    RenameColumn {
        schema: String,
        table: String,
        column: String,
    },
    DropColumn {
        schema: String,
        table: String,
        column: String,
    },
    DropConstraint {
        schema: String,
        table: String,
        constraint: String,
        constraint_type: Option<String>,
    },
    DropIndex {
        schema: String,
        table: String,
        index: String,
    },
}

impl Workspace {
    pub(super) fn open_nav_context_menu(&mut self, conn_id: &str, position: Point<Pixels>) {
        self.menus.nav_context_menu = Some(NavContextMenu {
            target: NavContextTarget::Connection {
                conn_id: conn_id.to_string(),
            },
            position,
        });
    }

    pub(super) fn open_nav_target_context_menu(
        &mut self,
        target: NavContextTarget,
        position: Point<Pixels>,
    ) {
        self.menus.nav_context_menu = Some(NavContextMenu { target, position });
    }

    pub(super) fn render_nav_context_menu(
        &mut self,
        menu: NavContextMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let conn_id = menu.conn_id().to_string();
        let is_connected = self.is_connection_live(&conn_id);

        let item = |id: &'static str,
                    icon: &'static str,
                    label: &'static str,
                    enabled: bool,
                    action: NavMenuAction,
                    cx: &mut Context<Self>| {
            let conn_id = conn_id.clone();
            let mut row = div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(24.0))
                .px_2()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if enabled {
                    theme.text_primary
                } else {
                    theme.text_muted
                })
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .opacity(if enabled { 1.0 } else { 0.4 })
                        .child(img(icon).w(px(14.0)).h(px(14.0))),
                )
                .child(label);
            if enabled {
                let action = action.clone();
                row = row
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.nav_context_menu = None;
                        this.run_nav_menu_action(action.clone(), &conn_id, cx);
                        cx.notify();
                    }));
            }
            row
        };
        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);

        let mut panel = div()
            .id("nav-context-menu")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(200.0))
            .p_1()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menus.nav_context_menu = None;
                cx.notify();
            }));

        match menu.target {
            NavContextTarget::Connection { .. } => {
                panel = panel
                    .child(item(
                        "nav-menu-sql-editor",
                        "icons/sql_script.svg",
                        "SQL Editor",
                        is_connected && !self.form.is_connecting,
                        NavMenuAction::OpenSqlEditor,
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-edit",
                        "icons/edit.svg",
                        "Edit Connection",
                        true,
                        NavMenuAction::EditConnection,
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-connect",
                        "icons/connect.svg",
                        "Connect",
                        !is_connected && !self.form.is_connecting,
                        NavMenuAction::Connect,
                        cx,
                    ))
                    .child(item(
                        "nav-menu-disconnect",
                        "icons/disconnect.svg",
                        "Disconnect",
                        is_connected,
                        NavMenuAction::Disconnect,
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-delete",
                        "icons/delete.svg",
                        "Delete",
                        true,
                        NavMenuAction::Delete,
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-refresh",
                        "icons/refresh.svg",
                        "Refresh",
                        is_connected && !self.form.is_connecting,
                        NavMenuAction::Refresh,
                        cx,
                    ));
            }
            NavContextTarget::Table { schema, table, .. } => {
                panel = panel
                    .child(item(
                        "nav-menu-table-open",
                        "icons/table.svg",
                        "View Table Data",
                        is_connected,
                        NavMenuAction::OpenData {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(item(
                        "nav-menu-table-import",
                        "icons/table.svg",
                        "Import CSV Data…",
                        is_connected,
                        NavMenuAction::ImportCsv {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(item(
                        "nav-menu-table-transfer",
                        "icons/table.svg",
                        "Transfer to Table…",
                        is_connected && self.can_transfer_table(&conn_id, &schema, &table),
                        NavMenuAction::TransferTable {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-table-add-col",
                        "icons/columns.svg",
                        "Add Column…",
                        is_connected,
                        NavMenuAction::AddColumn {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(item(
                        "nav-menu-table-rename",
                        "icons/edit.svg",
                        "Rename Table…",
                        is_connected,
                        NavMenuAction::RenameTable {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-table-truncate",
                        "icons/delete.svg",
                        "Truncate Table…",
                        is_connected,
                        NavMenuAction::TruncateTable {
                            schema: schema.clone(),
                            table: table.clone(),
                        },
                        cx,
                    ))
                    .child(item(
                        "nav-menu-table-drop",
                        "icons/delete.svg",
                        "Drop Table…",
                        is_connected,
                        NavMenuAction::DropTable { schema, table },
                        cx,
                    ));
            }
            NavContextTarget::View { schema, view, .. } => {
                panel = panel
                    .child(item(
                        "nav-menu-view-open",
                        "icons/table.svg",
                        "View Data",
                        is_connected,
                        NavMenuAction::OpenData {
                            schema: schema.clone(),
                            table: view.clone(),
                        },
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-view-rename",
                        "icons/edit.svg",
                        "Rename View…",
                        is_connected,
                        NavMenuAction::RenameView {
                            schema: schema.clone(),
                            view: view.clone(),
                        },
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-view-drop",
                        "icons/delete.svg",
                        "Drop View…",
                        is_connected,
                        NavMenuAction::DropView { schema, view },
                        cx,
                    ));
            }
            NavContextTarget::Folder {
                schema, is_views, ..
            } => {
                if !is_views {
                    panel = panel.child(item(
                        "nav-menu-folder-create-table",
                        "icons/table.svg",
                        "Create Table…",
                        is_connected,
                        NavMenuAction::CreateTable { schema },
                        cx,
                    ));
                } else {
                    panel = panel.child(item(
                        "nav-menu-folder-create-view",
                        "icons/table.svg",
                        "Create View…",
                        is_connected,
                        NavMenuAction::CreateView { schema },
                        cx,
                    ));
                }
            }
            NavContextTarget::Column {
                schema,
                table,
                column,
                ..
            } => {
                panel = panel
                    .child(item(
                        "nav-menu-col-rename",
                        "icons/edit.svg",
                        "Rename Column…",
                        is_connected,
                        NavMenuAction::RenameColumn {
                            schema: schema.clone(),
                            table: table.clone(),
                            column: column.clone(),
                        },
                        cx,
                    ))
                    .child(separator())
                    .child(item(
                        "nav-menu-col-drop",
                        "icons/delete.svg",
                        "Drop Column…",
                        is_connected,
                        NavMenuAction::DropColumn {
                            schema,
                            table,
                            column,
                        },
                        cx,
                    ));
            }
            NavContextTarget::Constraint {
                schema,
                table,
                constraint,
                constraint_type,
                ..
            } => {
                panel = panel.child(item(
                    "nav-menu-constraint-drop",
                    "icons/delete.svg",
                    "Drop Constraint…",
                    is_connected,
                    NavMenuAction::DropConstraint {
                        schema,
                        table,
                        constraint,
                        constraint_type,
                    },
                    cx,
                ));
            }
            NavContextTarget::Index {
                schema,
                table,
                index,
                ..
            } => {
                panel = panel.child(item(
                    "nav-menu-index-drop",
                    "icons/delete.svg",
                    "Drop Index…",
                    is_connected,
                    NavMenuAction::DropIndex {
                        schema,
                        table,
                        index,
                    },
                    cx,
                ));
            }
        }

        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(1)
    }

    fn run_nav_menu_action(
        &mut self,
        action: NavMenuAction,
        conn_id: &str,
        cx: &mut Context<Self>,
    ) {
        match action {
            NavMenuAction::EditConnection => self.edit_saved_connection(Some(conn_id)),
            NavMenuAction::Connect => self.connect_saved_by_id(conn_id, cx),
            NavMenuAction::Disconnect => self.disconnect_by_id(conn_id, cx),
            NavMenuAction::Delete => self.delete_saved_connection(conn_id, cx),
            NavMenuAction::Refresh => {
                // Refresh acts on the active connection, so switch to a parked one first
                self.connect_saved_by_id(conn_id, cx);
                self.refresh_connection(cx);
            }
            // Opening the script switches to its connection
            NavMenuAction::OpenSqlEditor => self.open_sql_script(Some(conn_id), cx),

            NavMenuAction::OpenData { schema, table } => {
                self.query_table(conn_id, &schema, &table, cx);
            }
            NavMenuAction::ImportCsv { schema, table } => {
                self.open_import_modal(conn_id, &schema, &table, cx);
            }
            NavMenuAction::TransferTable { schema, table } => {
                // The menu item is disabled when the source cannot be built.
                let _ = self.open_table_transfer(conn_id, &schema, &table, "", cx);
            }
            NavMenuAction::RenameTable { schema, table } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::RenameTable {
                        schema,
                        table: table.clone(),
                        new_name: format!("{}_new", table),
                    },
                    cx,
                );
            }
            NavMenuAction::TruncateTable { schema, table } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::TruncateTable {
                        schema,
                        table,
                        cascade: false,
                    },
                    cx,
                );
            }
            NavMenuAction::DropTable { schema, table } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::DropTable {
                        schema,
                        table,
                        cascade: false,
                    },
                    cx,
                );
            }
            NavMenuAction::AddColumn { schema, table } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::AddColumn {
                        schema,
                        table,
                        column: "new_column".into(),
                        data_type: "text".into(),
                        nullable: true,
                        default_val: None,
                    },
                    cx,
                );
            }
            NavMenuAction::RenameView { schema, view } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::RenameView {
                        schema,
                        view: view.clone(),
                        new_name: format!("{}_new", view),
                    },
                    cx,
                );
            }
            NavMenuAction::DropView { schema, view } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::DropView {
                        schema,
                        view,
                        cascade: false,
                    },
                    cx,
                );
            }
            NavMenuAction::CreateTable { schema } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::CreateTable {
                        schema,
                        table: "new_table".into(),
                        columns: Vec::new(),
                    },
                    cx,
                );
            }
            NavMenuAction::CreateView { schema } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::CreateView {
                        schema,
                        view: "new_view".into(),
                        definition: "SELECT 1 AS col".into(),
                    },
                    cx,
                );
            }
            NavMenuAction::RenameColumn {
                schema,
                table,
                column,
            } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::RenameColumn {
                        schema,
                        table,
                        old_name: column.clone(),
                        new_name: format!("{}_new", column),
                    },
                    cx,
                );
            }
            NavMenuAction::DropColumn {
                schema,
                table,
                column,
            } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::DropColumn {
                        schema,
                        table,
                        column,
                        cascade: false,
                    },
                    cx,
                );
            }
            NavMenuAction::DropConstraint {
                schema,
                table,
                constraint,
                constraint_type,
            } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::DropConstraint {
                        schema,
                        table,
                        constraint,
                        constraint_type,
                        cascade: false,
                    },
                    cx,
                );
            }
            NavMenuAction::DropIndex {
                schema,
                table,
                index,
            } => {
                self.open_schema_action(
                    conn_id,
                    SchemaAction::DropIndex {
                        schema,
                        table,
                        index,
                        cascade: false,
                    },
                    cx,
                );
            }
        }
    }
}
