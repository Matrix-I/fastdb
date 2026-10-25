use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use db_wire::{create_driver, ColumnMeta, ConnectionConfig, DatabaseDriver, DbDriverKind, StreamEvent, TableSummary};
use mem_engine::DataType;
use gpui::*;
use platform_hal::keychain::{CredentialStore, KeychainStore};
use sql_intel::SchemaCache;
use ui_core::theme::Theme;
use ui_editor::SqlEditorView;
use ui_grid::VirtualDataGrid;

mod connection_dialog;
mod connections;
mod data_edit;
mod data_panel;
mod ddl;
mod diagram_panel;
mod keyboard;
mod nav_context_menu;
mod navigator;
mod panel_resize;
mod properties_panel;
mod query;
mod status_bar;
mod tab_context_menu;
mod table_view;
mod text_input;
mod top_bar;
mod value_format;
mod value_panel;
mod widgets;

pub use connections::{LiveConnection, SavedConnection};
pub use nav_context_menu::NavContextMenu;
pub use tab_context_menu::TabContextMenu;
pub use ddl::generate_table_ddl;
pub use query::{group_thousands, TabState};
#[cfg(test)]
pub use query::QueryStatus;
use ddl::render_dbeaver_col_badge;
use text_input::{handle_line_key, render_caret_text, CaretPick, LineHistory, LineInput};
use panel_resize::{PanelDrag, PanelSplitter, CENTER_CHROME_WIDTH, NAVIGATOR_DEFAULT_WIDTH};
use value_panel::{ValuePanelState, VALUE_PANEL_WIDTH};
use widgets::{dbeaver_driver_icon, render_field_content};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableSubTab {
    Properties,
    Data,
    Diagram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySection {
    Columns,
    Constraints,
    ForeignKeys,
    Indexes,
    Dependencies,
    References,
    Partitions,
    Triggers,
    Rules,
    Policies,
    Statistics,
    Permissions,
    Ddl,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataViewMode {
    Grid,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormField {
    Name,
    Host,
    Port,
    Database,
    User,
    Password,
}

pub struct Workspace {
    pub theme: Theme,
    pub focus_handle: FocusHandle,
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    pub driver: Option<Arc<dyn DatabaseDriver>>,
    #[allow(dead_code)]
    pub schema_cache: Arc<SchemaCache>,
    pub tables: Vec<TableSummary>,
    pub is_running_query: bool,
    /// Incremented per query run so events from a superseded run are ignored
    pub query_generation: u64,
    pub cancel_requested: bool,
    pub next_tab_id: u64,
    pub status_message: String,

    // Real Database Connection Setup State
    pub saved_connections: Vec<SavedConnection>,
    pub active_connection_id: Option<String>,
    /// Open connections other than the active one, kept alive for instant switching
    pub live_connections: HashMap<String, LiveConnection>,
    pub credentials: Arc<dyn CredentialStore>,
    pub editing_connection_id: Option<String>,
    pub is_connected: bool,
    pub is_connecting: bool,
    pub show_connection_modal: bool,
    pub selected_driver_kind: DbDriverKind,
    pub form_name: String,
    pub form_host: String,
    pub form_port: String,
    pub form_database: String,
    pub form_user: String,
    pub form_password: String,
    pub form_use_unix_socket: bool,
    pub active_field: Option<FormField>,
    /// Undo history of each connection form field, reset whenever the form is opened
    pub form_history: HashMap<FormField, LineHistory>,
    pub show_password: bool,
    pub test_result: Option<Result<String, String>>,

    // DBeaver-style Tree Navigator State
    pub nav_connection_expanded: bool,
    pub nav_databases_expanded: bool,
    pub nav_database_expanded: bool,
    pub nav_schemas_expanded: bool,
    pub nav_schema_expanded: HashSet<String>,
    pub nav_tables_expanded: HashSet<String>,
    pub nav_table_expanded: HashSet<String>,
    pub nav_subfolder_expanded: HashSet<String>,
    pub selected_table_name: Option<String>,
    pub table_columns: HashMap<(String, String), Vec<ColumnMeta>>,
    pub table_constraints: HashMap<(String, String), Vec<(String, String)>>,
    pub table_indexes: HashMap<(String, String), Vec<String>>,
    pub resizing_column: Option<(usize, usize, f32, f32)>, // (tab_idx, col_idx, start_mouse_x, start_col_width)
    pub resizing_panel: Option<PanelDrag>,
    /// Live widths of the side panels, changed by dragging their splitters
    pub navigator_width: f32,
    pub value_panel_width: f32,
    pub is_filter_focused: bool,
    pub show_right_panels: bool,
    pub value_panel: ValuePanelState,
    pub cursor_visible: bool,
    pub field_selected_all: bool,
    pub filter_selected_all: bool,
    /// Caret of the active form field, as a char index (clamped; `usize::MAX` = end)
    pub field_cursor: usize,
    /// Caret of the filter bar, as a char index (clamped; `usize::MAX` = end)
    pub filter_cursor: usize,
    pub is_fetch_size_focused: bool,
    pub fetch_size_selected_all: bool,
    /// Caret of the fetch size box, as a char index (clamped; `usize::MAX` = end)
    pub fetch_size_cursor: usize,
    /// Undo history of the text of the grid cell being edited, reset for each cell
    pub cell_edit_history: LineHistory,
    /// Right-click menu of a navigator connection, if open
    pub nav_context_menu: Option<NavContextMenu>,
    /// Right-click menu of a top bar tab, if open
    pub tab_context_menu: Option<TabContextMenu>,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let focus_handle = cx.focus_handle();
        let schema_cache = Arc::new(SchemaCache::new().expect("Failed to initialize SchemaCache"));

        let credentials: Arc<dyn CredentialStore> = Arc::new(KeychainStore::default());
        let (mut saved_connections, active_connection_id) = Self::load_saved_connections();
        let migrated_passwords = Self::migrate_plaintext_passwords(&*credentials, &mut saved_connections);
        let initial_active_conn = if let Some(ref id) = active_connection_id {
            saved_connections.iter().find(|c| &c.id == id).cloned()
        } else {
            saved_connections.first().cloned()
        };

        let status_message = if saved_connections.is_empty() {
            "⚪ Disconnected — Click ➕ to connect to a database.".to_string()
        } else if let Some(ref saved) = initial_active_conn {
            format!("⚪ Saved connection: {} — Click in navigator to connect.", saved.name)
        } else {
            format!("⚪ {} saved connection(s) — Click in navigator to connect.", saved_connections.len())
        };

        let (form_name, form_driver, form_host, form_port, form_database, form_user, form_password, form_sock) =
            if let Some(ref saved) = initial_active_conn {
                let drv = match saved.driver.as_str() {
                    "MySql" => DbDriverKind::MySql,
                    "Sqlite" => DbDriverKind::Sqlite,
                    _ => DbDriverKind::Postgres,
                };
                (
                    saved.name.clone(),
                    drv,
                    saved.host.clone(),
                    saved.port.to_string(),
                    saved.database.clone(),
                    saved.user.clone(),
                    saved.password.clone().unwrap_or_default(),
                    saved.use_unix_socket,
                )
            } else {
                (
                    "".to_string(),
                    DbDriverKind::Postgres,
                    "localhost".to_string(),
                    "5432".to_string(),
                    "".to_string(),
                    "postgres".to_string(),
                    "".to_string(),
                    false,
                )
            };

        let workspace = Self {
            theme,
            focus_handle,
            active_tab: 0,
            tabs: Vec::new(),
            driver: None,
            schema_cache,
            tables: Vec::new(),
            is_running_query: false,
            query_generation: 0,
            cancel_requested: false,
            next_tab_id: 1,
            status_message,

            // Connection Setup Form Defaults
            saved_connections,
            active_connection_id,
            credentials,
            editing_connection_id: None,
            is_connected: false,
            is_connecting: false,
            live_connections: HashMap::new(),
            show_connection_modal: false,
            selected_driver_kind: form_driver,
            form_name,
            form_host,
            form_port,
            form_database,
            form_user,
            form_password,
            form_use_unix_socket: form_sock,
            active_field: Some(FormField::Name),
            form_history: HashMap::new(),
            show_password: false,
            test_result: None,

            // DBeaver Navigation Tree Defaults
            nav_connection_expanded: false,
            nav_databases_expanded: true,
            nav_database_expanded: true,
            nav_schemas_expanded: true,
            nav_schema_expanded: {
                let mut s = HashSet::new();
                s.insert("public".to_string());
                s
            },
            nav_tables_expanded: {
                let mut s = HashSet::new();
                s.insert("public".to_string());
                s
            },
            nav_table_expanded: HashSet::new(),
            nav_subfolder_expanded: HashSet::new(),
            selected_table_name: None,
            table_columns: HashMap::new(),
            table_constraints: HashMap::new(),
            table_indexes: HashMap::new(),
            resizing_column: None,
            resizing_panel: None,
            navigator_width: NAVIGATOR_DEFAULT_WIDTH,
            value_panel_width: VALUE_PANEL_WIDTH,
            is_filter_focused: false,
            show_right_panels: false,
            value_panel: ValuePanelState::default(),
            cursor_visible: true,
            field_selected_all: false,
            filter_selected_all: false,
            field_cursor: usize::MAX,
            filter_cursor: usize::MAX,
            is_fetch_size_focused: false,
            fetch_size_selected_all: false,
            fetch_size_cursor: usize::MAX,
            cell_edit_history: LineHistory::new(),
            nav_context_menu: None,
            tab_context_menu: None,
        };

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(530)).await;
                if this.update(cx, |this, cx| {
                    this.cursor_visible = !this.cursor_visible;
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        }).detach();

        if migrated_passwords {
            // Rewrite the config so the migrated plaintext passwords leave the disk
            workspace.save_config();
        }

        workspace
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;

        if self.show_connection_modal {
            window.focus(&self.focus_handle);
        }

        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key_down(event, cx);
            }));

        if self.resizing_column.is_some() {
            root = root
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                    if let Some((tab_idx, col_idx, start_x, start_w)) = this.resizing_column {
                        let current_x = f32::from(event.position.x);
                        let delta = current_x - start_x;
                        let new_w = (start_w + delta).max(ui_grid::MIN_COL_WIDTH);
                        if let Some(tab) = this.tabs.get_mut(tab_idx) {
                            tab.grid.set_column_width(col_idx, new_w);
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.resizing_column.is_some() {
                        this.resizing_column = None;
                        cx.notify();
                    }
                }))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.resizing_column.is_some() {
                        this.resizing_column = None;
                        cx.notify();
                    }
                }));
        }

        if self.resizing_panel.is_some() {
            root = root
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                    this.drag_panel_splitter(f32::from(event.position.x), f32::from(window.viewport_size().width));
                    cx.notify();
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.resizing_panel = None;
                    cx.notify();
                }))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.resizing_panel = None;
                    cx.notify();
                }));
        }

        // 1. Top Bar / Tabs & Toolbar
        root = root.child(self.render_top_bar(cx));

        // 2. Main Body: Left Sidebar + Center Workspace
        root = root.child(
            div()
                .flex()
                .flex_1()
                .w_full()
                .overflow_hidden()
                .child(self.render_navigator(cx))
                .child(self.render_table_view(window, cx)),
        );

        // 3. Bottom Status Bar
        root = root.child(self.render_status_bar());

        if let Some(menu) = self.nav_context_menu.clone() {
            root = root.child(self.render_nav_context_menu(menu, cx));
        }
        if let Some(menu) = self.tab_context_menu.clone() {
            root = root.child(self.render_tab_context_menu(menu, cx));
        }

        // 4. Modal Dialog: Database Connection Setup
        if self.show_connection_modal {
            root = root.child(self.render_connection_dialog(cx));
        }

        root
    }
}
