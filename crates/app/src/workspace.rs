use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use db_wire::{create_driver, ColumnMeta, ConnectionConfig, DatabaseDriver, DbDriverKind, TableSummary};
use gpui::*;
use sql_intel::SchemaCache;
use ui_core::theme::Theme;
use ui_editor::SqlEditorView;
use ui_grid::VirtualDataGrid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableSubTab {
    Properties,
    Data,
    Diagram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataViewMode {
    Grid,
    Text,
}

pub struct TabState {
    pub title: String,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub filter_sql: String,
    pub active_subtab: TableSubTab,
    pub view_mode: DataViewMode,
    pub show_sql_dialog: bool,
    pub limit: usize,
    pub editor: SqlEditorView,
    pub grid: VirtualDataGrid,
    pub execution_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Name,
    Host,
    Port,
    Database,
    User,
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    pub use_unix_socket: bool,
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
    pub status_message: String,

    // Real Database Connection Setup State
    pub saved_connections: Vec<SavedConnection>,
    pub active_connection_id: Option<String>,
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
    pub is_filter_focused: bool,
    pub show_right_panels: bool,
    pub cursor_visible: bool,
    pub field_selected_all: bool,
    pub filter_selected_all: bool,
}

fn render_field_content(
    value: &str,
    placeholder: &str,
    is_active: bool,
    is_selected_all: bool,
    cursor_visible: bool,
    theme: &Theme,
) -> Div {
    let mut row = div().flex().items_center().h(px(18.0)).w_full().overflow_hidden();

    if value.is_empty() {
        if is_active {
            row = row
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(14.0))
                        .rounded_xs()
                        .bg(if cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                )
                .child(
                    div()
                        .pl_1()
                        .text_color(theme.text_muted)
                        .child(placeholder.to_string())
                );
        } else {
            row = row.child(
                div()
                    .text_color(theme.text_muted)
                    .child(placeholder.to_string())
            );
        }
    } else {
        if is_active && is_selected_all {
            row = row.child(
                div()
                    .px_1()
                    .py_0p5()
                    .rounded_xs()
                    .bg(rgb(0x264f78))
                    .text_color(rgb(0xffffff))
                    .child(value.to_string())
            );
        } else if is_active {
            row = row
                .child(
                    div()
                        .text_color(theme.text_primary)
                        .child(value.to_string())
                )
                .child(
                    div()
                        .pl(px(1.0))
                        .w(px(1.5))
                        .h(px(14.0))
                        .rounded_xs()
                        .bg(if cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                );
        } else {
            row = row.child(
                div()
                    .text_color(theme.text_primary)
                    .child(value.to_string())
            );
        }
    }

    row
}

fn dbeaver_driver_icon(driver: &str, is_connected: bool) -> Div {
    let icon_path = match driver {
        "MySql" | "mysql" => "icons/mysql.png",
        "Sqlite" | "sqlite" => "icons/sqlite.png",
        _ => "icons/postgresql.png",
    };

    div()
        .relative()
        .w(px(16.0))
        .h(px(16.0))
        .flex_shrink_0()
        .child(
            img(icon_path)
                .w(px(16.0))
                .h(px(16.0))
        )
        .child(
            if is_connected {
                div()
                    .absolute()
                    .bottom(px(-1.0))
                    .left(px(-1.0))
                    .w(px(7.0))
                    .h(px(8.0))
                    .child(
                        img("icons/success_ovr.png")
                            .w(px(7.0))
                            .h(px(8.0))
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        )
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let focus_handle = cx.focus_handle();
        let schema_cache = Arc::new(SchemaCache::new().expect("Failed to initialize SchemaCache"));

        let (saved_connections, active_connection_id) = Self::load_saved_connections();
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
            status_message,

            // Connection Setup Form Defaults
            saved_connections,
            active_connection_id,
            editing_connection_id: None,
            is_connected: false,
            is_connecting: false,
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
            is_filter_focused: false,
            show_right_panels: false,
            cursor_visible: true,
            field_selected_all: false,
            filter_selected_all: false,
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

        workspace
    }

    pub fn load_saved_connections_from_str(content: &str) -> (Vec<SavedConnection>, Option<String>) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
            return (Vec::new(), None);
        };

        let mut connections = Vec::new();

        if let Some(arr) = v["connections"].as_array() {
            for (idx, item) in arr.iter().enumerate() {
                let id = item["id"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("conn_{}", idx + 1));
                let d = item["database"].as_str().unwrap_or("").to_string();
                let drv_str = item["driver"].as_str().unwrap_or("Postgres").to_string();
                let default_name = if drv_str == "Sqlite" {
                    std::path::Path::new(&d)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&d)
                        .to_string()
                } else if !d.is_empty() {
                    format!("{}-local", d)
                } else {
                    format!("{}-conn", drv_str)
                };
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&default_name)
                    .to_string();
                let h = item["host"].as_str().unwrap_or("localhost").to_string();
                let p = if let Some(n) = item["port"].as_u64() {
                    n as u16
                } else if let Some(s) = item["port"].as_str() {
                    s.parse::<u16>().unwrap_or(5432)
                } else {
                    5432
                };
                let u = item["user"].as_str().unwrap_or("postgres").to_string();
                let pw = item["password"].as_str().map(|s| s.to_string());
                let sock = item["use_unix_socket"].as_bool().unwrap_or(false);

                if !d.is_empty() || drv_str == "Sqlite" {
                    connections.push(SavedConnection {
                        id,
                        name,
                        driver: drv_str,
                        host: h,
                        port: p,
                        database: d,
                        user: u,
                        password: pw,
                        use_unix_socket: sock,
                    });
                }
            }
        } else {
            // Legacy single connection format migration
            let d = v["database"].as_str().unwrap_or("").to_string();
            let has = v["has_saved_connection"].as_bool().unwrap_or(!d.is_empty());
            if has && !d.is_empty() {
                let drv_str = v["driver"].as_str().unwrap_or("Postgres").to_string();
                let h = v["host"].as_str().unwrap_or("localhost").to_string();
                let p = if let Some(n) = v["port"].as_u64() {
                    n as u16
                } else if let Some(s) = v["port"].as_str() {
                    s.parse::<u16>().unwrap_or(5432)
                } else {
                    5432
                };
                let u = v["user"].as_str().unwrap_or("postgres").to_string();
                let pw = v["password"].as_str().map(|s| s.to_string());
                let sock = v["use_unix_socket"].as_bool().unwrap_or(false);
                let name = if drv_str == "Sqlite" {
                    std::path::Path::new(&d)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&d)
                        .to_string()
                } else {
                    format!("{}-local", d)
                };

                connections.push(SavedConnection {
                    id: "conn_1".to_string(),
                    name,
                    driver: drv_str,
                    host: h,
                    port: p,
                    database: d,
                    user: u,
                    password: pw,
                    use_unix_socket: sock,
                });
            }
        }

        let active_id = v["active_connection_id"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| connections.first().map(|c| c.id.clone()));

        (connections, active_id)
    }

    pub fn load_saved_connections() -> (Vec<SavedConnection>, Option<String>) {
        if let Ok(content) = std::fs::read_to_string(".fastdb_config.json") {
            Self::load_saved_connections_from_str(&content)
        } else {
            (Vec::new(), None)
        }
    }

    pub fn serialize_connections_config(connections: &[SavedConnection], active_id: Option<&str>) -> String {
        let connections_json: Vec<serde_json::Value> = connections
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "driver": c.driver,
                    "host": c.host,
                    "port": c.port.to_string(),
                    "database": c.database,
                    "user": c.user,
                    "password": c.password.clone().unwrap_or_default(),
                    "use_unix_socket": c.use_unix_socket,
                })
            })
            .collect();

        let save_data = serde_json::json!({
            "has_saved_connection": !connections.is_empty(),
            "active_connection_id": active_id,
            "connections": connections_json,
        });

        serde_json::to_string_pretty(&save_data).unwrap_or_else(|_| save_data.to_string())
    }

    pub fn save_config(&self) {
        let content = Self::serialize_connections_config(&self.saved_connections, self.active_connection_id.as_deref());
        let _ = std::fs::write(".fastdb_config.json", content);
    }

    pub fn active_saved_connection(&self) -> Option<&SavedConnection> {
        if let Some(ref id) = self.active_connection_id {
            self.saved_connections.iter().find(|c| &c.id == id)
        } else {
            self.saved_connections.first()
        }
    }

    pub fn build_current_config(&self) -> ConnectionConfig {
        let port: u16 = self.form_port.parse().unwrap_or(match self.selected_driver_kind {
            DbDriverKind::Postgres => 5432,
            DbDriverKind::MySql => 3306,
            DbDriverKind::Sqlite => 0,
        });

        ConnectionConfig {
            driver: self.selected_driver_kind,
            host: self.form_host.clone(),
            port,
            database: self.form_database.clone(),
            user: self.form_user.clone(),
            password: if self.form_password.is_empty() {
                None
            } else {
                Some(self.form_password.clone())
            },
            use_unix_socket: self.form_use_unix_socket,
            ssh_tunnel: None,
            connect_timeout: std::time::Duration::from_millis(10000),
        }
    }

    pub fn test_connection(&mut self, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }

        self.is_connecting = true;
        self.status_message = "Testing database connection...".to_string();
        self.test_result = None;
        cx.notify();

        let config = self.build_current_config();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => return Err(format!("Failed to create runtime: {}", e)),
                };

                let timeout_duration = std::time::Duration::from_millis(10000);
                let outcome = rt.block_on(async {
                    tokio::time::timeout(timeout_duration, async {
                        let driver = create_driver(config)
                            .map_err(|e| format!("Driver initialization error: {}", e))?;
                        driver.ping().await
                            .map_err(|e| format!("Ping failed: {}", e))?;
                        Ok::<_, String>("Connection test successful! Database responded to ping.".to_string())
                    }).await
                });

                match outcome {
                    Ok(Ok(msg)) => Ok(msg),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err("Connection timed out (server unreachable or offline)".to_string()),
                }
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_connecting = false;
                match result {
                    Ok(msg) => {
                        this.test_result = Some(Ok(msg));
                        this.status_message = "✓ Database connection test passed.".to_string();
                    }
                    Err(err) => {
                        this.test_result = Some(Err(err.clone()));
                        this.status_message = format!("✕ Test failed: {}", err);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn connect_to_database(&mut self, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }

        self.is_connecting = true;
        self.status_message = "Connecting to database...".to_string();
        self.test_result = None;
        cx.notify();

        let config = self.build_current_config();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => return Err(format!("Failed to create runtime: {}", e)),
                };

                let timeout_duration = std::time::Duration::from_millis(15000);
                let outcome = rt.block_on(async {
                    tokio::time::timeout(timeout_duration, async {
                        let driver = create_driver(config.clone())
                            .map_err(|e| format!("Driver initialization error: {}", e))?;

                        driver.ping().await
                            .map_err(|e| format!("Connection refused: {}", e))?;

                        let tables = driver.list_tables().await
                            .map_err(|e| format!("Connected but failed to list tables: {}", e))?;

                        // Fast batch fetch all columns in 1 single query; fall back gracefully if schema discovery fails
                        let columns_info = driver.list_all_columns().await.unwrap_or_default();
                        let constraints_info = driver.list_all_constraints().await.unwrap_or_default();
                        let indexes_info = driver.list_all_indexes().await.unwrap_or_default();

                        Ok::<_, String>((driver, tables, columns_info, constraints_info, indexes_info, config))
                    }).await
                });

                match outcome {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err("Connection timed out (server unreachable or request took too long)".to_string()),
                }
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_connecting = false;
                match result {
                    Ok((driver, tables, columns_info, constraints_info, indexes_info, config)) => {
                        let _ = this.schema_cache.clear();
                        for t in &tables {
                            let _ = this.schema_cache.add_table(&t.schema, &t.name, &t.table_type);
                        }

                        let mut col_map = HashMap::new();
                        for (schema, table, cols) in columns_info {
                            for c in &cols {
                                let _ = this.schema_cache.add_column(&schema, &table, &c.name, c.data_type.display_name(), c.is_primary_key);
                            }
                            col_map.insert((schema, table), cols);
                        }
                        this.table_columns = col_map;

                        let mut constr_map = HashMap::new();
                        for (schema, table, cname, ctype) in constraints_info {
                            constr_map.entry((schema, table)).or_insert_with(Vec::new).push((cname, ctype));
                        }
                        this.table_constraints = constr_map;

                        let mut idx_map = HashMap::new();
                        for (schema, table, iname) in indexes_info {
                            idx_map.entry((schema, table)).or_insert_with(Vec::new).push(iname);
                        }
                        this.table_indexes = idx_map;

                        this.tables = tables;
                        this.driver = Some(driver);
                        this.is_connected = true;
                        this.show_connection_modal = false;
                        this.test_result = None;

                        let conn_id = this.editing_connection_id.clone().unwrap_or_else(|| {
                            format!("conn_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
                        });

                        let conn_name = if !this.form_name.trim().is_empty() {
                            this.form_name.trim().to_string()
                        } else {
                            match config.driver {
                                DbDriverKind::Sqlite => {
                                    std::path::Path::new(&config.database)
                                        .file_name()
                                        .and_then(|f| f.to_str())
                                        .unwrap_or(&config.database)
                                        .to_string()
                                }
                                _ => if !config.database.is_empty() {
                                    format!("{}-local", config.database)
                                } else {
                                    format!("{:?}-conn", config.driver)
                                },
                            }
                        };

                        let saved = SavedConnection {
                            id: conn_id.clone(),
                            name: conn_name.clone(),
                            driver: match config.driver {
                                DbDriverKind::Postgres => "Postgres".to_string(),
                                DbDriverKind::MySql => "MySql".to_string(),
                                DbDriverKind::Sqlite => "Sqlite".to_string(),
                            },
                            host: config.host.clone(),
                            port: config.port,
                            database: config.database.clone(),
                            user: config.user.clone(),
                            password: config.password.clone(),
                            use_unix_socket: config.use_unix_socket,
                        };

                        if let Some(pos) = this.saved_connections.iter().position(|c| c.id == conn_id) {
                            this.saved_connections[pos] = saved.clone();
                        } else {
                            this.saved_connections.push(saved.clone());
                        }
                        this.active_connection_id = Some(conn_id.clone());
                        this.editing_connection_id = Some(conn_id);
                        this.save_config();

                        this.nav_connection_expanded = true;
                        this.nav_databases_expanded = true;
                        this.nav_database_expanded = true;
                        this.nav_schemas_expanded = true;
                        this.nav_table_expanded.clear();
                        this.nav_subfolder_expanded.clear();
                        this.selected_table_name = None;

                        for t in &this.tables {
                            this.nav_schema_expanded.insert(t.schema.clone());
                            this.nav_tables_expanded.insert(t.schema.clone());
                        }

                        let db_label = format!("{:?} ({}:{})", config.driver, config.host, config.port);
                        this.status_message = format!("🟢 Connected to: {} ({}) — {} tables found", conn_name, db_label, this.tables.len());

                        for tab in &mut this.tabs {
                            tab.editor.schema_cache = Some(this.schema_cache.clone());
                        }

                        if !this.tabs.is_empty() {
                            let tab = &mut this.tabs[this.active_tab];
                            let current_text = tab.editor.buffer.text();
                            if current_text.trim().is_empty() || current_text.starts_with("-- Welcome to FastDB") {
                                let welcome_conn = format!(
                                    "-- Connected to {}.\n-- Select a table from the navigator or write a query to execute.\n",
                                    conn_name
                                );
                                tab.editor = SqlEditorView::new(&welcome_conn, this.theme.clone(), Some(this.schema_cache.clone()));
                                tab.editor.buffer.set_cursor_offset(0);
                            }
                            tab.grid = VirtualDataGrid::new(None, this.theme.clone());
                        }
                    }
                    Err(e) => {
                        this.test_result = Some(Err(e.clone()));
                        this.status_message = format!("❌ Connection failed: {}", e);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn disconnect(&mut self) {
        self.driver = None;
        self.tables.clear();
        let _ = self.schema_cache.clear();
        self.is_connected = false;
        self.selected_table_name = None;
        self.table_columns.clear();
        self.table_constraints.clear();
        self.table_indexes.clear();
        self.nav_table_expanded.clear();
        self.nav_subfolder_expanded.clear();
        self.nav_connection_expanded = false;
        self.status_message = "⚪ Disconnected from database — Click connection in navigator to reconnect.".to_string();
        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.grid = VirtualDataGrid::new(None, self.theme.clone());
            tab.execution_time_ms = None;
        }
    }

    pub fn open_new_connection_dialog(&mut self) {
        self.editing_connection_id = None;
        self.form_name = "".to_string();
        self.selected_driver_kind = DbDriverKind::Postgres;
        self.form_host = "localhost".to_string();
        self.form_port = "5432".to_string();
        self.form_database = "".to_string();
        self.form_user = "postgres".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = false;
        self.active_field = Some(FormField::Name);
        self.field_selected_all = false;
        self.cursor_visible = true;
        self.show_password = false;
        self.test_result = None;
        self.show_connection_modal = true;
    }

    pub fn edit_saved_connection(&mut self, conn_id: Option<&str>) {
        let target_conn = if let Some(id) = conn_id {
            self.saved_connections.iter().find(|c| c.id == id).cloned()
        } else if let Some(ref id) = self.active_connection_id {
            self.saved_connections.iter().find(|c| &c.id == id).cloned()
        } else {
            self.saved_connections.first().cloned()
        };

        if let Some(saved) = target_conn {
            self.selected_driver_kind = match saved.driver.as_str() {
                "MySql" => DbDriverKind::MySql,
                "Sqlite" => DbDriverKind::Sqlite,
                _ => DbDriverKind::Postgres,
            };
            self.form_name = saved.name.clone();
            self.form_host = saved.host.clone();
            self.form_port = saved.port.to_string();
            self.form_database = saved.database.clone();
            self.form_user = saved.user.clone();
            self.form_password = saved.password.clone().unwrap_or_default();
            self.form_use_unix_socket = saved.use_unix_socket;
            self.editing_connection_id = Some(saved.id.clone());
            self.test_result = None;
            self.active_field = Some(FormField::Database);
            self.field_selected_all = false;
            self.cursor_visible = true;
            self.show_password = false;
            self.show_connection_modal = true;
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn connect_saved_by_id(&mut self, conn_id: &str, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }
        let Some(saved) = self.saved_connections.iter().find(|c| c.id == conn_id).cloned() else {
            return;
        };

        self.selected_driver_kind = match saved.driver.as_str() {
            "MySql" => DbDriverKind::MySql,
            "Sqlite" => DbDriverKind::Sqlite,
            _ => DbDriverKind::Postgres,
        };
        self.form_name = saved.name.clone();
        self.form_host = saved.host.clone();
        self.form_port = saved.port.to_string();
        self.form_database = saved.database.clone();
        self.form_user = saved.user.clone();
        self.form_password = saved.password.clone().unwrap_or_default();
        self.form_use_unix_socket = saved.use_unix_socket;
        self.editing_connection_id = Some(saved.id.clone());
        self.active_connection_id = Some(saved.id.clone());
        self.nav_connection_expanded = true;
        self.connect_to_database(cx);
    }

    pub fn connect_saved(&mut self, cx: &mut Context<Self>) {
        if let Some(ref id) = self.active_connection_id.clone() {
            self.connect_saved_by_id(id, cx);
        } else if let Some(first) = self.saved_connections.first().map(|c| c.id.clone()) {
            self.connect_saved_by_id(&first, cx);
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn delete_saved_connection(&mut self, conn_id: &str) {
        self.saved_connections.retain(|c| c.id != conn_id);
        if self.active_connection_id.as_deref() == Some(conn_id) {
            if self.is_connected {
                self.disconnect();
            }
            self.active_connection_id = self.saved_connections.first().map(|c| c.id.clone());
        }
        self.save_config();
        self.status_message = "Removed connection from list.".to_string();
    }

    pub fn save_connection_form(&mut self) {
        let conn_id = self.editing_connection_id.clone().unwrap_or_else(|| {
            format!("conn_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
        });

        let config = self.build_current_config();

        let conn_name = if !self.form_name.trim().is_empty() {
            self.form_name.trim().to_string()
        } else {
            match config.driver {
                DbDriverKind::Sqlite => {
                    std::path::Path::new(&config.database)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&config.database)
                        .to_string()
                }
                _ => if !config.database.is_empty() {
                    format!("{}-local", config.database)
                } else {
                    format!("{:?}-conn", config.driver)
                },
            }
        };

        let saved = SavedConnection {
            id: conn_id.clone(),
            name: conn_name,
            driver: match config.driver {
                DbDriverKind::Postgres => "Postgres".to_string(),
                DbDriverKind::MySql => "MySql".to_string(),
                DbDriverKind::Sqlite => "Sqlite".to_string(),
            },
            host: config.host.clone(),
            port: config.port,
            database: config.database.clone(),
            user: config.user.clone(),
            password: config.password.clone(),
            use_unix_socket: config.use_unix_socket,
        };

        if let Some(pos) = self.saved_connections.iter().position(|c| c.id == conn_id) {
            self.saved_connections[pos] = saved;
        } else {
            self.saved_connections.push(saved);
        }
        self.active_connection_id = Some(conn_id);
        self.save_config();
        self.show_connection_modal = false;
        self.status_message = "✓ Saved connection to list.".to_string();
    }

    pub fn select_preset_postgres(&mut self) {
        self.selected_driver_kind = DbDriverKind::Postgres;
        self.form_host = "localhost".to_string();
        self.form_port = "5432".to_string();
        self.form_database = "postgres".to_string();
        self.form_user = "postgres".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = true;
        self.active_field = Some(FormField::Host);
        self.test_result = None;
    }

    pub fn select_preset_mysql(&mut self) {
        self.selected_driver_kind = DbDriverKind::MySql;
        self.form_host = "localhost".to_string();
        self.form_port = "3306".to_string();
        self.form_database = "mysql".to_string();
        self.form_user = "root".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = true;
        self.active_field = Some(FormField::Host);
        self.test_result = None;
    }

    pub fn select_preset_sqlite(&mut self) {
        self.selected_driver_kind = DbDriverKind::Sqlite;
        self.form_host = "localhost".to_string();
        self.form_port = "0".to_string();
        self.form_database = "fastdb.sqlite".to_string();
        self.form_user = "".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = false;
        self.active_field = Some(FormField::Database);
        self.test_result = None;
    }

    pub fn build_query_for_tab(&self, tab_idx: usize) -> String {
        let tab = match self.tabs.get(tab_idx) {
            Some(t) => t,
            None => return "SELECT 1;".to_string(),
        };

        let schema = tab.schema_name.as_deref().unwrap_or("public");
        let table = match tab.table_name.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => {
                if !tab.title.is_empty() && !tab.title.starts_with("Query ") {
                    &tab.title
                } else {
                    let full_text = tab.editor.buffer.text();
                    let non_comments: Vec<&str> = full_text
                        .lines()
                        .filter(|l| !l.trim().starts_with("--") && !l.trim().trim_matches(';').trim().is_empty())
                        .collect();
                    if !non_comments.is_empty() {
                        return non_comments.join("\n");
                    }
                    return "SELECT 1;".to_string();
                }
            }
        };

        let target = match self.selected_driver_kind {
            DbDriverKind::Postgres => format!("\"{}\".\"{}\"", schema, table),
            DbDriverKind::MySql => format!("`{}`.`{}`", schema, table),
            DbDriverKind::Sqlite => format!("\"{}\"", table),
        };

        let filter = tab.filter_sql.trim();
        let limit = tab.limit;

        if filter.is_empty() {
            format!("SELECT * FROM {} LIMIT {};", target, limit)
        } else {
            let f_upper = filter.to_uppercase();
            let has_limit = f_upper.contains("LIMIT");
            let limit_suffix = if has_limit {
                "".to_string()
            } else {
                format!(" LIMIT {}", limit)
            };

            if f_upper.starts_with("WHERE")
                || f_upper.starts_with("ORDER BY")
                || f_upper.starts_with("GROUP BY")
                || f_upper.starts_with("HAVING")
                || f_upper.starts_with("LIMIT")
            {
                format!("SELECT * FROM {} {}{};", target, filter, limit_suffix)
            } else {
                format!("SELECT * FROM {} WHERE {}{};", target, filter, limit_suffix)
            }
        }
    }

    pub fn query_table(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        self.selected_table_name = Some(table.to_string());

        let existing_tab_idx = self.tabs.iter().position(|t| {
            t.table_name.as_deref() == Some(table) && t.schema_name.as_deref() == Some(schema)
        });

        if let Some(idx) = existing_tab_idx {
            self.active_tab = idx;
        } else {
            let mut grid = VirtualDataGrid::new(None, self.theme.clone());
            grid.set_table_name(table);
            self.tabs.push(TabState {
                title: table.to_string(),
                schema_name: Some(schema.to_string()),
                table_name: Some(table.to_string()),
                filter_sql: String::new(),
                active_subtab: TableSubTab::Data,
                view_mode: DataViewMode::Grid,
                show_sql_dialog: false,
                limit: 200,
                editor: SqlEditorView::new("SELECT 1;", self.theme.clone(), Some(self.schema_cache.clone())),
                grid,
                execution_time_ms: None,
            });
            self.active_tab = self.tabs.len() - 1;
        }

        self.is_filter_focused = false;
        self.run_active_query(cx);
    }

    pub fn run_active_query(&mut self, cx: &mut Context<Self>) {
        let Some(ref driver) = self.driver else {
            self.show_connection_modal = true;
            self.status_message = "Please connect to a database first.".to_string();
            cx.notify();
            return;
        };

        if self.tabs.is_empty() {
            return;
        }

        let sql = self.build_query_for_tab(self.active_tab);

        if sql.trim().trim_matches(';').trim().is_empty() {
            self.status_message = "No SQL statement to execute.".to_string();
            cx.notify();
            return;
        }

        self.is_running_query = true;
        self.status_message = format!("Executing query: {}", sql.trim().replace('\n', " "));
        cx.notify();

        let driver = Arc::clone(driver);
        let active_tab_idx = self.active_tab;
        let query_sql = sql.clone();

        cx.spawn(async move |this, cx| {
            let exec_result = cx.background_executor().spawn(async move {
                let rt = db_wire::get_runtime();
                rt.block_on(async {
                    driver.execute_query(&query_sql).await
                })
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_running_query = false;
                match exec_result {
                    Ok((pager, total_rows, elapsed_ms)) => {
                        this.status_message = format!("✅ Fetched {} rows in {}ms (120 FPS Zero-Copy)", total_rows, elapsed_ms);
                        if active_tab_idx < this.tabs.len() {
                            let tab = &mut this.tabs[active_tab_idx];
                            tab.execution_time_ms = Some(elapsed_ms);
                            tab.grid.set_pager(pager);
                            if let Some(ref t_name) = tab.table_name {
                                tab.grid.set_table_name(t_name.clone());
                            }
                        }
                    }
                    Err(e) => {
                        this.status_message = format!("❌ SQL Error: {}", e);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    #[allow(dead_code)]
    pub fn new_tab(&mut self) {
        let count = self.tabs.len() + 1;
        let title = format!("Query {}", count);
        let editor = SqlEditorView::new(
            "SELECT 1;",
            self.theme.clone(),
            Some(self.schema_cache.clone()),
        );
        let grid = VirtualDataGrid::new(None, self.theme.clone());

        self.tabs.push(TabState {
            title,
            schema_name: None,
            table_name: None,
            filter_sql: String::new(),
            active_subtab: TableSubTab::Data,
            view_mode: DataViewMode::Grid,
            show_sql_dialog: false,
            limit: 200,
            editor,
            grid,
            execution_time_ms: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.is_filter_focused = false;
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_tab = 0;
                self.is_filter_focused = false;
            } else if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
                self.is_filter_focused = false;
            }
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let active_tab_idx = self.active_tab;

        if self.show_connection_modal {
            window.focus(&self.focus_handle);
        }

        #[cfg(target_os = "macos")]
        let top_bar_left_pad = 80.0;
        #[cfg(not(target_os = "macos"))]
        let top_bar_left_pad = 14.0;

        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                // Intercept Cmd+Q (macOS) / Ctrl+Q (Linux/Windows) unconditionally to close the app
                if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("q")
                {
                    cx.quit();
                    return;
                }

                // Intercept Cmd+W (macOS) / Ctrl+W (Linux/Windows) to close active tab
                if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("w")
                {
                    if this.show_connection_modal {
                        this.show_connection_modal = false;
                        this.field_selected_all = false;
                    } else if !this.tabs.is_empty() {
                        this.close_tab(this.active_tab);
                    }
                    cx.notify();
                    return;
                }

                if this.show_connection_modal {
                    if event.keystroke.key == "escape" {
                        this.show_connection_modal = false;
                        this.field_selected_all = false;
                        cx.notify();
                        return;
                    }
                    if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                        this.connect_to_database(cx);
                        return;
                    }
                    if event.keystroke.key == "tab" {
                        this.field_selected_all = false;
                        this.cursor_visible = true;
                        this.active_field = match (this.selected_driver_kind, this.active_field) {
                            (DbDriverKind::Sqlite, None) => Some(FormField::Name),
                            (DbDriverKind::Sqlite, Some(FormField::Name)) => Some(FormField::Database),
                            (DbDriverKind::Sqlite, Some(FormField::Database)) => Some(FormField::Name),
                            (DbDriverKind::Sqlite, _) => Some(FormField::Database),
                            (_, None) => Some(FormField::Name),
                            (_, Some(FormField::Name)) => Some(FormField::Host),
                            (_, Some(FormField::Host)) => Some(FormField::Port),
                            (_, Some(FormField::Port)) => Some(FormField::Database),
                            (_, Some(FormField::Database)) => Some(FormField::User),
                            (_, Some(FormField::User)) => Some(FormField::Password),
                            (_, Some(FormField::Password)) => Some(FormField::Name),
                        };
                        cx.notify();
                        return;
                    }
                    // Select All (Cmd+A / Ctrl+A)
                    if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                        && event.keystroke.key.eq_ignore_ascii_case("a")
                    {
                        if this.active_field.is_some() {
                            this.field_selected_all = true;
                            this.cursor_visible = true;
                            cx.notify();
                            return;
                        }
                    }
                    // Copy (Cmd+C / Ctrl+C)
                    if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                        && event.keystroke.key.eq_ignore_ascii_case("c")
                    {
                        if let Some(field) = this.active_field {
                            let text = match field {
                                FormField::Name => &this.form_name,
                                FormField::Host => &this.form_host,
                                FormField::Port => &this.form_port,
                                FormField::Database => &this.form_database,
                                FormField::User => &this.form_user,
                                FormField::Password => &this.form_password,
                            };
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            return;
                        }
                    }
                    // Cut (Cmd+X / Ctrl+X)
                    if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                        && event.keystroke.key.eq_ignore_ascii_case("x")
                    {
                        if let Some(field) = this.active_field {
                            let target = match field {
                                FormField::Name => &mut this.form_name,
                                FormField::Host => &mut this.form_host,
                                FormField::Port => &mut this.form_port,
                                FormField::Database => &mut this.form_database,
                                FormField::User => &mut this.form_user,
                                FormField::Password => &mut this.form_password,
                            };
                            cx.write_to_clipboard(ClipboardItem::new_string(target.clone()));
                            target.clear();
                            this.field_selected_all = false;
                            this.cursor_visible = true;
                            cx.notify();
                            return;
                        }
                    }
                    // Clipboard paste (Cmd+V on macOS or Ctrl+V on Linux/Windows)
                    if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                        && event.keystroke.key.eq_ignore_ascii_case("v")
                    {
                        if let Some(item) = cx.read_from_clipboard() {
                            if let Some(pasted) = item.text() {
                                let clean = pasted.trim_matches(|c| c == '\r' || c == '\n');
                                if let Some(field) = this.active_field {
                                    let target = match field {
                                        FormField::Name => &mut this.form_name,
                                        FormField::Host => &mut this.form_host,
                                        FormField::Port => &mut this.form_port,
                                        FormField::Database => &mut this.form_database,
                                        FormField::User => &mut this.form_user,
                                        FormField::Password => &mut this.form_password,
                                    };
                                    if this.field_selected_all {
                                        target.clear();
                                        this.field_selected_all = false;
                                    }
                                    if field == FormField::Port {
                                        let digits: String = clean.chars().filter(|c| c.is_ascii_digit()).collect();
                                        target.push_str(&digits);
                                    } else {
                                        target.push_str(clean);
                                    }
                                    this.cursor_visible = true;
                                    cx.notify();
                                }
                            }
                        }
                        return;
                    }
                    // Cancel selection on Left / Right / Up / Down arrow navigation
                    if event.keystroke.key == "left" || event.keystroke.key == "right" || event.keystroke.key == "up" || event.keystroke.key == "down" {
                        if this.field_selected_all {
                            this.field_selected_all = false;
                            this.cursor_visible = true;
                            cx.notify();
                            return;
                        }
                    }
                    // Cmd+Backspace = clear entire field
                    if event.keystroke.modifiers.platform && event.keystroke.key == "backspace" {
                        if let Some(field) = this.active_field {
                            let target = match field {
                                FormField::Name => &mut this.form_name,
                                FormField::Host => &mut this.form_host,
                                FormField::Port => &mut this.form_port,
                                FormField::Database => &mut this.form_database,
                                FormField::User => &mut this.form_user,
                                FormField::Password => &mut this.form_password,
                            };
                            target.clear();
                            this.field_selected_all = false;
                            this.cursor_visible = true;
                            cx.notify();
                        }
                        return;
                    }
                    // Backspace or Delete
                    if event.keystroke.key == "backspace" || event.keystroke.key == "delete" {
                        if let Some(field) = this.active_field {
                            let target = match field {
                                FormField::Name => &mut this.form_name,
                                FormField::Host => &mut this.form_host,
                                FormField::Port => &mut this.form_port,
                                FormField::Database => &mut this.form_database,
                                FormField::User => &mut this.form_user,
                                FormField::Password => &mut this.form_password,
                            };
                            if this.field_selected_all {
                                target.clear();
                                this.field_selected_all = false;
                            } else {
                                target.pop();
                            }
                            this.cursor_visible = true;
                            cx.notify();
                        }
                        return;
                    }
                    // Printable character insertion
                    let input_char = if let Some(ref kc) = event.keystroke.key_char {
                        if kc.chars().count() == 1 {
                            Some(kc.as_str())
                        } else {
                            None
                        }
                    } else if event.keystroke.key.chars().count() == 1 {
                        Some(event.keystroke.key.as_str())
                    } else {
                        None
                    };

                    if let Some(char_str) = input_char {
                        if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                            if let Some(field) = this.active_field {
                                let target = match field {
                                    FormField::Name => &mut this.form_name,
                                    FormField::Host => &mut this.form_host,
                                    FormField::Port => &mut this.form_port,
                                    FormField::Database => &mut this.form_database,
                                    FormField::User => &mut this.form_user,
                                    FormField::Password => &mut this.form_password,
                                };
                                if this.field_selected_all {
                                    target.clear();
                                    this.field_selected_all = false;
                                }
                                if field == FormField::Port {
                                    if char_str.chars().all(|c| c.is_ascii_digit()) {
                                        target.push_str(char_str);
                                    }
                                } else {
                                    target.push_str(char_str);
                                }
                                this.cursor_visible = true;
                                cx.notify();
                            }
                        }
                    }
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && (event.keystroke.key == "enter" || event.keystroke.key == "return")
                {
                    this.run_active_query(cx);
                } else if !this.tabs.is_empty() {
                    let tab = &mut this.tabs[this.active_tab];
                    if this.is_filter_focused {
                        if event.keystroke.key == "escape" {
                            this.is_filter_focused = false;
                            this.filter_selected_all = false;
                            cx.notify();
                        } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                            this.run_active_query(cx);
                        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                            && event.keystroke.key.eq_ignore_ascii_case("a")
                        {
                            this.filter_selected_all = true;
                            this.cursor_visible = true;
                            cx.notify();
                        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                            && event.keystroke.key.eq_ignore_ascii_case("c")
                        {
                            cx.write_to_clipboard(ClipboardItem::new_string(tab.filter_sql.clone()));
                        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                            && event.keystroke.key.eq_ignore_ascii_case("x")
                        {
                            cx.write_to_clipboard(ClipboardItem::new_string(tab.filter_sql.clone()));
                            tab.filter_sql.clear();
                            this.filter_selected_all = false;
                            this.cursor_visible = true;
                            cx.notify();
                        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                            && event.keystroke.key.eq_ignore_ascii_case("v")
                        {
                            if let Some(item) = cx.read_from_clipboard() {
                                if let Some(pasted) = item.text() {
                                    let clean = pasted.trim_matches(|c| c == '\r' || c == '\n');
                                    if this.filter_selected_all {
                                        tab.filter_sql.clear();
                                        this.filter_selected_all = false;
                                    }
                                    tab.filter_sql.push_str(clean);
                                    this.cursor_visible = true;
                                    cx.notify();
                                }
                            }
                        } else if event.keystroke.modifiers.platform && event.keystroke.key == "backspace" {
                            tab.filter_sql.clear();
                            this.filter_selected_all = false;
                            this.cursor_visible = true;
                            cx.notify();
                        } else if event.keystroke.key == "backspace" || event.keystroke.key == "delete" {
                            if this.filter_selected_all {
                                tab.filter_sql.clear();
                                this.filter_selected_all = false;
                            } else {
                                tab.filter_sql.pop();
                            }
                            this.cursor_visible = true;
                            cx.notify();
                        } else if event.keystroke.key == "left" || event.keystroke.key == "right" {
                            if this.filter_selected_all {
                                this.filter_selected_all = false;
                                this.cursor_visible = true;
                                cx.notify();
                            }
                        } else if event.keystroke.key == "space" {
                            if this.filter_selected_all {
                                tab.filter_sql.clear();
                                this.filter_selected_all = false;
                            }
                            tab.filter_sql.push(' ');
                            this.cursor_visible = true;
                            cx.notify();
                        } else {
                            let input_char = if let Some(ref kc) = event.keystroke.key_char {
                                if kc.chars().count() == 1 {
                                    Some(kc.as_str())
                                } else {
                                    None
                                }
                            } else if event.keystroke.key.chars().count() == 1 {
                                Some(event.keystroke.key.as_str())
                            } else {
                                None
                            };

                            if let Some(char_str) = input_char {
                                if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                                    if this.filter_selected_all {
                                        tab.filter_sql.clear();
                                        this.filter_selected_all = false;
                                    }
                                    tab.filter_sql.push_str(char_str);
                                    this.cursor_visible = true;
                                    cx.notify();
                                }
                            }
                        }
                    } else if tab.table_name.is_none() {
                        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                            && event.keystroke.key.eq_ignore_ascii_case("v")
                        {
                            if let Some(item) = cx.read_from_clipboard() {
                                if let Some(pasted) = item.text() {
                                    tab.editor.insert_text(&pasted);
                                    cx.notify();
                                }
                            }
                        } else if event.keystroke.key == "backspace" {
                            tab.editor.backspace();
                            cx.notify();
                        } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                            tab.editor.insert_text("\n");
                            cx.notify();
                        } else if event.keystroke.key == "space" {
                            tab.editor.insert_text(" ");
                            cx.notify();
                        } else {
                            let input_char = if let Some(ref kc) = event.keystroke.key_char {
                                if kc.chars().count() == 1 {
                                    Some(kc.as_str())
                                } else {
                                    None
                                }
                            } else if event.keystroke.key.chars().count() == 1 {
                                Some(event.keystroke.key.as_str())
                            } else {
                                None
                            };

                            if let Some(char_str) = input_char {
                                if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                                    tab.editor.insert_text(char_str);
                                    cx.notify();
                                }
                            }
                        }
                    }
                }
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

        // 1. Top Bar / Tabs & Toolbar
        root = root.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(38.0))
                .bg(theme.bg_surface)
                .border_b_1()
                .border_color(theme.border)
                .pl(px(top_bar_left_pad))
                .pr_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(14.0))
                                        .text_color(theme.text_accent)
                                        .child("⚡ FastDB")
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.text_muted)
                                        .child("v0.1.0 Pure Rust")
                                )
                        )
                        // Tabs Bar
                        .child({
                            let mut tabs_bar = div().flex().items_center().gap_1();
                            for (i, tab) in self.tabs.iter().enumerate() {
                                let is_active = i == active_tab_idx;
                                let bg = if is_active { theme.bg_surface_selected } else { theme.bg_surface };
                                let border_color = if is_active { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() };
                                let tab_id = format!("tab-select-{}", i);
                                let close_id = format!("tab-close-{}", i);

                                tabs_bar = tabs_bar.child(
                                    div()
                                        .id(ElementId::Name(tab_id.into()))
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_3()
                                        .py_1()
                                        .rounded_t_md()
                                        .bg(bg)
                                        .border_t_2()
                                        .border_color(border_color)
                                        .text_size(px(12.0))
                                        .text_color(if is_active { theme.text_primary } else { theme.text_secondary })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.active_tab = i;
                                            this.is_filter_focused = false;
                                            cx.notify();
                                        }))
                                        .child(img("icons/table.svg").w(px(13.0)).h(px(13.0)))
                                        .child(tab.title.clone())
                                        .child(
                                            div()
                                                .id(ElementId::Name(close_id.into()))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(16.0))
                                                .h(px(16.0))
                                                .rounded_xs()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.text_color(rgb(0xff7b72)).bg(theme.bg_surface_hover))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.close_tab(i);
                                                    cx.notify();
                                                }))
                                                .child("✕")
                                        )
                                );
                            }
                            tabs_bar
                        })
                )
                // Top Right Connection Status Badge
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("btn-topbar-connection")
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(theme.bg_surface_hover)
                                .text_color(theme.text_secondary)
                                .text_size(px(11.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.is_connected && !this.saved_connections.is_empty() {
                                        this.connect_saved(cx);
                                    } else if let Some(active_id) = this.active_connection_id.clone() {
                                        this.edit_saved_connection(Some(&active_id));
                                    } else {
                                        this.open_new_connection_dialog();
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    if self.is_connected {
                                        let drv = self.active_saved_connection().map(|s| s.driver.as_str()).unwrap_or(match self.selected_driver_kind {
                                            DbDriverKind::Postgres => "Postgres",
                                            DbDriverKind::MySql => "MySql",
                                            DbDriverKind::Sqlite => "Sqlite",
                                        });
                                        dbeaver_driver_icon(drv, true).into_any_element()
                                    } else {
                                        img("icons/disconnect.svg").w(px(13.0)).h(px(13.0)).into_any_element()
                                    }
                                )
                                .child(if self.is_connected {
                                    let name = self.active_saved_connection().map(|s| s.name.as_str()).unwrap_or(&self.form_database);
                                    format!("{} ({})", name, self.form_user)
                                } else if let Some(first) = self.saved_connections.first() {
                                    format!("Connect: {}", first.name)
                                } else {
                                    "Connect DB".to_string()
                                })
                                .child(img("icons/configuration.svg").w(px(13.0)).h(px(13.0)))
                        )
                )
        );

        // 2. Main Body: Left Sidebar + Center Workspace
        root = root.child(
            div()
                .flex()
                .flex_1()
                .w_full()
                .overflow_hidden()
                // Left Sidebar: DBeaver-style Database Navigator
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(theme.sidebar_width.max(280.0)))
                        .h_full()
                        .bg(theme.bg_sidebar)
                        .border_r_1()
                        .border_color(theme.border)
                        .p_2()
                        .gap_2()
                        // Sidebar Header
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text_secondary)
                                        .child(img("icons/database_navigator.svg").w(px(14.0)).h(px(14.0)))
                                        .child("DATABASE NAVIGATOR")
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .id("btn-open-connect-dialog")
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(theme.bg_surface)
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.open_new_connection_dialog();
                                                    cx.notify();
                                                }))
                                                .child(
                                                    div()
                                                        .text_size(px(13.0))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text_accent)
                                                        .child("+")
                                                )
                                                .child(img("icons/connect.svg").w(px(12.0)).h(px(12.0)))
                                        )
                                        .child(if self.is_connected {
                                            div()
                                                .id("btn-disconnect")
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(theme.bg_surface)
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .text_size(px(10.0))
                                                .text_color(theme.text_error)
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.disconnect();
                                                    cx.notify();
                                                }))
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(img("icons/disconnect.svg").w(px(11.0)).h(px(11.0)))
                                                .child("Disconnect")
                                                .into_any_element()
                                        } else {
                                            div().into_any_element()
                                        })
                                )
                        )
                        // Tree View Container with vertical scroll
                        .child({
                            let mut nav_tree = div()
                                .id("dbeaver-nav-tree")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .overflow_y_scroll()
                                .gap_0p5();

                            if self.saved_connections.is_empty() && !self.is_connected {
                                nav_tree = nav_tree.child(
                                    div()
                                        .p_4()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child("No database connections")
                                        .child(
                                            div()
                                                .id("btn-sidebar-connect-prompt")
                                                .px_3()
                                                .py_1p5()
                                                .rounded_md()
                                                .bg(theme.bg_surface_selected)
                                                .text_color(theme.text_accent)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.open_new_connection_dialog();
                                                    cx.notify();
                                                }))
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                                .child("Add Connection")
                                        )
                                );
                            } else {
                                for conn in &self.saved_connections {
                                    let conn_id = conn.id.clone();
                                    let is_this_active = self.active_connection_id.as_deref() == Some(&conn_id);
                                    let is_this_connected = self.is_connected && is_this_active;
                                    let is_this_connecting = self.is_connecting && is_this_active;
                                    let is_conn_exp = self.nav_connection_expanded && is_this_connected;

                                    let drv_name = conn.driver.clone();
                                    let conn_name = conn.name.clone();
                                    let conn_addr = if conn.driver == "Sqlite" {
                                        conn.database.clone()
                                    } else {
                                        format!("{}:{}", conn.host, conn.port)
                                    };

                                    let conn_id_for_click = conn_id.clone();
                                    let conn_id_for_edit = conn_id.clone();
                                    let conn_id_for_del = conn_id.clone();

                                    nav_tree = nav_tree.child(
                                        div()
                                            .id(ElementId::Name(format!("nav-conn-{}", conn_id).into()))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .h(px(24.0))
                                            .pl_1()
                                            .pr_2()
                                            .rounded_sm()
                                            .bg(if is_this_connected { theme.bg_surface_selected } else { rgba(0x00000000) })
                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_this_connected {
                                                    this.nav_connection_expanded = !this.nav_connection_expanded;
                                                } else if !this.is_connecting {
                                                    this.connect_saved_by_id(&conn_id_for_click, cx);
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .text_size(px(12.0))
                                                    .child(
                                                        div()
                                                            .w(px(12.0))
                                                            .text_size(px(10.0))
                                                            .text_color(if is_this_connecting { theme.text_warning } else { theme.text_muted })
                                                            .child(if is_this_connecting { "⏳" } else if is_conn_exp { "▾" } else { "▸" })
                                                    )
                                                    .child(dbeaver_driver_icon(&drv_name, is_this_connected))
                                                    .child(
                                                        div()
                                                            .font_weight(if is_this_connected { FontWeight::BOLD } else { FontWeight::NORMAL })
                                                            .text_color(if is_this_connected { theme.text_accent } else { theme.text_primary })
                                                            .child(conn_name)
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(10.5))
                                                            .text_color(theme.text_muted)
                                                            .child(format!(" {}", conn_addr))
                                                    )
                                                    .child(
                                                        if is_this_connecting {
                                                            div()
                                                                .text_size(px(10.0))
                                                                .text_color(theme.text_warning)
                                                                .child("(Connecting...)")
                                                        } else if !is_this_connected {
                                                            div()
                                                                .text_size(px(10.0))
                                                                .text_color(theme.text_muted)
                                                                .child("(Click to connect)")
                                                        } else {
                                                            div()
                                                        }
                                                    )
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .id(ElementId::Name(format!("btn-edit-{}", conn_id).into()))
                                                            .hover(|s| s.bg(theme.bg_surface).text_color(theme.text_accent))
                                                            .p_0p5()
                                                            .rounded_sm()
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.edit_saved_connection(Some(&conn_id_for_edit));
                                                                cx.notify();
                                                            }))
                                                            .child(img("icons/configuration.svg").w(px(13.0)).h(px(13.0)))
                                                    )
                                                    .child(
                                                        div()
                                                            .id(ElementId::Name(format!("btn-del-{}", conn_id).into()))
                                                            .hover(|s| s.bg(theme.bg_surface).text_color(theme.text_error))
                                                            .px_1()
                                                            .py_0p5()
                                                            .rounded_sm()
                                                            .text_size(px(11.0))
                                                            .text_color(theme.text_muted)
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.delete_saved_connection(&conn_id_for_del);
                                                                cx.notify();
                                                            }))
                                                            .child("✕")
                                                    )
                                            )
                                    );

                                    if is_this_connected && is_conn_exp {
                                    // Level 1: "Databases" Folder
                                    let is_dbs_exp = self.nav_databases_expanded;
                                    nav_tree = nav_tree.child(
                                        div()
                                            .id("nav-databases-folder")
                                            .flex()
                                            .items_center()
                                            .h(px(22.0))
                                            .pl(px(14.0))
                                            .rounded_sm()
                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.nav_databases_expanded = !this.nav_databases_expanded;
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .text_size(px(11.5))
                                                    .child(
                                                        div()
                                                            .w(px(12.0))
                                                            .text_size(px(10.0))
                                                            .text_color(theme.text_muted)
                                                            .child(if is_dbs_exp { "▾" } else { "▸" })
                                                    )
                                                    .child(img("icons/folder_database.svg").w(px(16.0)).h(px(16.0)))
                                                    .child("Databases")
                                            )
                                    );

                                    if is_dbs_exp {
                                        // Level 2: Target Database
                                        let is_db_exp = self.nav_database_expanded;
                                        nav_tree = nav_tree.child(
                                            div()
                                                .id("nav-database-node")
                                                .flex()
                                                .items_center()
                                                .h(px(22.0))
                                                .pl(px(26.0))
                                                .rounded_sm()
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.nav_database_expanded = !this.nav_database_expanded;
                                                    cx.notify();
                                                }))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1p5()
                                                        .text_size(px(11.5))
                                                        .child(
                                                            div()
                                                                .w(px(12.0))
                                                                .text_size(px(10.0))
                                                                .text_color(theme.text_muted)
                                                                .child(if is_db_exp { "▾" } else { "▸" })
                                                        )
                                                        .child(img("icons/database.svg").w(px(16.0)).h(px(16.0)))
                                                        .child(
                                                            div()
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .text_color(theme.text_accent)
                                                                .child(self.form_database.clone())
                                                        )
                                                )
                                        );

                                        if is_db_exp {
                                            // Level 3: "Schemas" Folder
                                            let is_schemas_exp = self.nav_schemas_expanded;
                                            nav_tree = nav_tree.child(
                                                div()
                                                    .id("nav-schemas-folder")
                                                    .flex()
                                                    .items_center()
                                                    .h(px(22.0))
                                                    .pl(px(38.0))
                                                    .rounded_sm()
                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.nav_schemas_expanded = !this.nav_schemas_expanded;
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_1p5()
                                                            .text_size(px(11.5))
                                                            .child(
                                                                div()
                                                                    .w(px(12.0))
                                                                    .text_size(px(10.0))
                                                                    .text_color(theme.text_muted)
                                                                    .child(if is_schemas_exp { "▾" } else { "▸" })
                                                            )
                                                            .child(img("icons/folder_schema.svg").w(px(16.0)).h(px(16.0)))
                                                            .child("Schemas")
                                                    )
                                            );

                                            if is_schemas_exp {
                                                // Group tables by schema
                                                let mut schema_map: BTreeMap<String, Vec<&TableSummary>> = BTreeMap::new();
                                                for t in &self.tables {
                                                    schema_map.entry(t.schema.clone()).or_default().push(t);
                                                }

                                                if schema_map.is_empty() {
                                                    nav_tree = nav_tree.child(
                                                        div()
                                                            .pl(px(50.0))
                                                            .py_1()
                                                            .text_size(px(11.0))
                                                            .text_color(theme.text_muted)
                                                            .child("(No schemas found)")
                                                    );
                                                }

                                                for (schema_name, s_tables) in schema_map {
                                                    let is_schema_exp = self.nav_schema_expanded.contains(&schema_name);
                                                    let s_name_clone = schema_name.clone();
                                                    let schema_id = format!("nav-schema-{}", schema_name);

                                                    // Level 4: Schema item (e.g. public)
                                                    nav_tree = nav_tree.child(
                                                        div()
                                                            .id(ElementId::Name(schema_id.into()))
                                                            .flex()
                                                            .items_center()
                                                            .h(px(22.0))
                                                            .pl(px(50.0))
                                                            .rounded_sm()
                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                if this.nav_schema_expanded.contains(&s_name_clone) {
                                                                    this.nav_schema_expanded.remove(&s_name_clone);
                                                                } else {
                                                                    this.nav_schema_expanded.insert(s_name_clone.clone());
                                                                }
                                                                cx.notify();
                                                            }))
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .items_center()
                                                                    .gap_1p5()
                                                                    .text_size(px(11.5))
                                                                    .child(
                                                                        div()
                                                                            .w(px(12.0))
                                                                            .text_size(px(10.0))
                                                                            .text_color(theme.text_muted)
                                                                            .child(if is_schema_exp { "▾" } else { "▸" })
                                                                    )
                                                                    .child(img("icons/schema.svg").w(px(16.0)).h(px(16.0)))
                                                                    .child(
                                                                        div()
                                                                            .font_weight(FontWeight::SEMIBOLD)
                                                                            .child(schema_name.clone())
                                                                    )
                                                            )
                                                    );

                                                    if is_schema_exp {
                                                        // Level 5: "Tables" Folder under this Schema
                                                        let is_tables_exp = self.nav_tables_expanded.contains(&schema_name);
                                                        let s_name_for_tables = schema_name.clone();
                                                        let tables_folder_id = format!("nav-tables-folder-{}", schema_name);

                                                        nav_tree = nav_tree.child(
                                                            div()
                                                                .id(ElementId::Name(tables_folder_id.into()))
                                                                .flex()
                                                                .items_center()
                                                                .h(px(22.0))
                                                                .pl(px(62.0))
                                                                .rounded_sm()
                                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                                .cursor_pointer()
                                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                                    if this.nav_tables_expanded.contains(&s_name_for_tables) {
                                                                        this.nav_tables_expanded.remove(&s_name_for_tables);
                                                                    } else {
                                                                        this.nav_tables_expanded.insert(s_name_for_tables.clone());
                                                                    }
                                                                    cx.notify();
                                                                }))
                                                                .child(
                                                                    div()
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap_1p5()
                                                                        .text_size(px(11.5))
                                                                        .child(
                                                                            div()
                                                                                .w(px(12.0))
                                                                                .text_size(px(10.0))
                                                                                .text_color(theme.text_muted)
                                                                                .child(if is_tables_exp { "▾" } else { "▸" })
                                                                        )
                                                                        .child(img("icons/folder_table.svg").w(px(16.0)).h(px(16.0)))
                                                                        .child("Tables")
                                                                )
                                                        );

                                                        if is_tables_exp {
                                                            // Level 6: Table items under this schema
                                                            for t in s_tables {
                                                                let table_schema = t.schema.clone();
                                                                let table_name = t.name.clone();
                                                                let table_key = format!("{}.{}", table_schema, table_name);
                                                                let is_table_exp = self.nav_table_expanded.contains(&table_key);
                                                                let is_selected = self.selected_table_name.as_deref() == Some(&table_name);
                                                                let item_id = format!("nav-table-{}-{}", table_schema, table_name);

                                                                let t_schema_for_click = table_schema.clone();
                                                                let t_name_for_click = table_name.clone();
                                                                let t_key_for_click = table_key.clone();

                                                                let mut row_div = div()
                                                                    .id(ElementId::Name(item_id.into()))
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_between()
                                                                    .h(px(22.0))
                                                                    .pl(px(74.0))
                                                                    .pr_2()
                                                                    .rounded_sm()
                                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                                    .cursor_pointer()
                                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                                        this.selected_table_name = Some(t_name_for_click.clone());
                                                                        if !this.nav_table_expanded.contains(&t_key_for_click) {
                                                                            this.nav_table_expanded.insert(t_key_for_click.clone());
                                                                            this.nav_subfolder_expanded.insert(format!("{}.columns", t_key_for_click));
                                                                        }
                                                                        this.query_table(&t_schema_for_click, &t_name_for_click, cx);
                                                                    }));

                                                                if is_selected {
                                                                    row_div = row_div.bg(rgb(0x135ba2));
                                                                }

                                                                let t_key_for_chev = table_key.clone();
                                                                nav_tree = nav_tree.child(
                                                                    row_div
                                                                        .child(
                                                                            div()
                                                                                .flex()
                                                                                .items_center()
                                                                                .gap_1p5()
                                                                                .text_size(px(11.5))
                                                                                .child(
                                                                                    div()
                                                                                        .id(ElementId::Name(format!("nav-chev-{}-{}", table_schema, table_name).into()))
                                                                                        .w(px(12.0))
                                                                                        .text_size(px(10.0))
                                                                                        .text_color(if is_selected { rgb(0xffffff) } else { theme.text_muted })
                                                                                        .hover(|s| s.text_color(theme.text_primary))
                                                                                        .cursor_pointer()
                                                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                                                            cx.stop_propagation();
                                                                                            if this.nav_table_expanded.contains(&t_key_for_chev) {
                                                                                                this.nav_table_expanded.remove(&t_key_for_chev);
                                                                                            } else {
                                                                                                this.nav_table_expanded.insert(t_key_for_chev.clone());
                                                                                                this.nav_subfolder_expanded.insert(format!("{}.columns", t_key_for_chev));
                                                                                            }
                                                                                            cx.notify();
                                                                                        }))
                                                                                        .child(if is_table_exp { "▾" } else { "▸" })
                                                                                )
                                                                                .child(img("icons/table.svg").w(px(16.0)).h(px(16.0)))
                                                                                .child(
                                                                                    div()
                                                                                        .font_weight(if is_selected { FontWeight::BOLD } else { FontWeight::NORMAL })
                                                                                        .text_color(if is_selected { rgb(0xffffff) } else { theme.text_primary })
                                                                                        .child(t.name.clone())
                                                                                )
                                                                        )
                                                                        .child(
                                                                            if let Some(rows) = t.estimated_rows {
                                                                                let count_str = if rows >= 1_000_000 {
                                                                                    format!("{:.1}M", rows as f64 / 1_000_000.0)
                                                                                } else if rows >= 1_000 {
                                                                                    format!("{}K", rows / 1_000)
                                                                                } else {
                                                                                    format!("{}", rows)
                                                                                };
                                                                                div()
                                                                                    .px_1()
                                                                                    .border_1()
                                                                                    .border_color(if is_selected { rgb(0x2874ba) } else { theme.border })
                                                                                    .rounded_xs()
                                                                                    .text_size(px(10.0))
                                                                                    .text_color(if is_selected { rgb(0x89c5ff) } else { theme.text_muted })
                                                                                    .child(count_str)
                                                                                    .into_any_element()
                                                                            } else {
                                                                                div().into_any_element()
                                                                            }
                                                                        )
                                                                );

                                                                // Level 7: Dropdown folders under table (Columns, Constraints, Foreign Keys, Indexes, Dependencies, References, Partitions, Triggers, Rules, Policies)
                                                                if is_table_exp {
                                                                    // 1. Columns
                                                                    let col_key = format!("{}.columns", table_key);
                                                                    let is_col_exp = self.nav_subfolder_expanded.contains(&col_key);
                                                                    let col_key_toggle = col_key.clone();
                                                                    let cols_for_table = self.table_columns.get(&(table_schema.clone(), table_name.clone()));

                                                                    nav_tree = nav_tree.child(
                                                                        div()
                                                                            .id(ElementId::Name(format!("nav-cat-cols-{}-{}", table_schema, table_name).into()))
                                                                            .flex()
                                                                            .items_center()
                                                                            .h(px(22.0))
                                                                            .pl(px(86.0))
                                                                            .pr_2()
                                                                            .rounded_sm()
                                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                                            .cursor_pointer()
                                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                                if this.nav_subfolder_expanded.contains(&col_key_toggle) {
                                                                                    this.nav_subfolder_expanded.remove(&col_key_toggle);
                                                                                } else {
                                                                                    this.nav_subfolder_expanded.insert(col_key_toggle.clone());
                                                                                }
                                                                                cx.notify();
                                                                            }))
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap_1p5()
                                                                                    .text_size(px(11.5))
                                                                                    .child(
                                                                                        div()
                                                                                            .w(px(10.0))
                                                                                            .text_size(px(9.0))
                                                                                            .text_color(theme.text_muted)
                                                                                            .child(if is_col_exp { "▾" } else { "▸" })
                                                                                    )
                                                                                    .child(img("icons/columns.svg").w(px(16.0)).h(px(16.0)))
                                                                                    .child(
                                                                                        div()
                                                                                            .text_color(theme.text_primary)
                                                                                            .child("Columns")
                                                                                    )
                                                                            )
                                                                    );

                                                                    if is_col_exp {
                                                                        if let Some(cols) = cols_for_table {
                                                                            if cols.is_empty() {
                                                                                nav_tree = nav_tree.child(
                                                                                    div()
                                                                                        .h(px(20.0))
                                                                                        .pl(px(98.0))
                                                                                        .flex()
                                                                                        .items_center()
                                                                                        .text_size(px(11.0))
                                                                                        .text_color(theme.text_muted)
                                                                                        .child("(No columns)")
                                                                                );
                                                                            } else {
                                                                                for c in cols {
                                                                                    let col_name = c.name.clone();
                                                                                    let is_pk = c.is_primary_key;
                                                                                    let type_str = c.data_type.display_name().to_lowercase();
                                                                                    let is_nullable = c.nullable;

                                                                                    nav_tree = nav_tree.child(
                                                                                        div()
                                                                                            .id(ElementId::Name(format!("nav-col-{}-{}-{}", table_schema, table_name, col_name).into()))
                                                                                            .flex()
                                                                                            .items_center()
                                                                                            .justify_between()
                                                                                            .h(px(20.0))
                                                                                            .pl(px(98.0))
                                                                                            .pr_2()
                                                                                            .rounded_sm()
                                                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                                                            .child(
                                                                                                div()
                                                                                                    .flex()
                                                                                                    .items_center()
                                                                                                    .gap_1p5()
                                                                                                    .text_size(px(11.0))
                                                                                                    .child(
                                                                                                        if is_pk {
                                                                                                            img("icons/key.svg").w(px(16.0)).h(px(16.0)).into_any_element()
                                                                                                        } else {
                                                                                                            img("icons/column.svg").w(px(16.0)).h(px(16.0)).into_any_element()
                                                                                                        }
                                                                                                    )
                                                                                                    .child(
                                                                                                        div()
                                                                                                            .font_weight(if is_pk { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                                                                                            .text_color(if is_pk { theme.text_accent } else { theme.text_primary })
                                                                                                            .child(col_name)
                                                                                                    )
                                                                                                    .child(
                                                                                                        div()
                                                                                                            .text_color(theme.text_muted)
                                                                                                            .child(format!(": {}", type_str))
                                                                                                    )
                                                                                            )
                                                                                            .child(
                                                                                                if !is_nullable {
                                                                                                    div()
                                                                                                        .text_size(px(9.0))
                                                                                                        .text_color(theme.text_warning)
                                                                                                        .child("NOT NULL")
                                                                                                } else {
                                                                                                    div()
                                                                                                }
                                                                                            )
                                                                                    );
                                                                                }
                                                                            }
                                                                        } else {
                                                                            nav_tree = nav_tree.child(
                                                                                div()
                                                                                    .h(px(20.0))
                                                                                    .pl(px(98.0))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .text_size(px(11.0))
                                                                                    .text_color(theme.text_muted)
                                                                                    .child("(No columns)")
                                                                            );
                                                                        }
                                                                    }

                                                                    // 2. Constraints
                                                                    let constr_key = format!("{}.constraints", table_key);
                                                                    let is_constr_exp = self.nav_subfolder_expanded.contains(&constr_key);
                                                                    let constr_key_toggle = constr_key.clone();
                                                                    let constraints_for_table = self.table_constraints.get(&(table_schema.clone(), table_name.clone()));

                                                                    nav_tree = nav_tree.child(
                                                                        div()
                                                                            .id(ElementId::Name(format!("nav-cat-constr-{}-{}", table_schema, table_name).into()))
                                                                            .flex()
                                                                            .items_center()
                                                                            .h(px(22.0))
                                                                            .pl(px(86.0))
                                                                            .pr_2()
                                                                            .rounded_sm()
                                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                                            .cursor_pointer()
                                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                                if this.nav_subfolder_expanded.contains(&constr_key_toggle) {
                                                                                    this.nav_subfolder_expanded.remove(&constr_key_toggle);
                                                                                } else {
                                                                                    this.nav_subfolder_expanded.insert(constr_key_toggle.clone());
                                                                                }
                                                                                cx.notify();
                                                                            }))
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap_1p5()
                                                                                    .text_size(px(11.5))
                                                                                    .child(
                                                                                        div()
                                                                                            .w(px(10.0))
                                                                                            .text_size(px(9.0))
                                                                                            .text_color(theme.text_muted)
                                                                                            .child(if is_constr_exp { "▾" } else { "▸" })
                                                                                    )
                                                                                    .child(img("icons/folder_constraint.svg").w(px(16.0)).h(px(16.0)))
                                                                                    .child(
                                                                                        div()
                                                                                            .text_color(theme.text_primary)
                                                                                            .child("Constraints")
                                                                                    )
                                                                            )
                                                                    );

                                                                    if is_constr_exp {
                                                                        let mut rendered_any = false;
                                                                        if let Some(constrs) = constraints_for_table {
                                                                            for (cname, ctype) in constrs {
                                                                                rendered_any = true;
                                                                                nav_tree = nav_tree.child(
                                                                                    div()
                                                                                        .flex()
                                                                                        .items_center()
                                                                                        .gap_1p5()
                                                                                        .h(px(20.0))
                                                                                        .pl(px(98.0))
                                                                                        .text_size(px(11.0))
                                                                                        .child(img("icons/constraint.svg").w(px(16.0)).h(px(16.0)))
                                                                                        .child(div().text_color(theme.text_primary).child(cname.clone()))
                                                                                        .child(div().text_color(theme.text_muted).child(format!("({})", ctype)))
                                                                                );
                                                                            }
                                                                        }
                                                                        if !rendered_any {
                                                                            nav_tree = nav_tree.child(
                                                                                div()
                                                                                    .h(px(20.0))
                                                                                    .pl(px(98.0))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .text_size(px(11.0))
                                                                                    .text_color(theme.text_muted)
                                                                                    .child("(No constraints)")
                                                                            );
                                                                        }
                                                                    }

                                                                    // 3. Foreign Keys
                                                                    let fk_key = format!("{}.foreign_keys", table_key);
                                                                    let is_fk_exp = self.nav_subfolder_expanded.contains(&fk_key);
                                                                    let fk_key_toggle = fk_key.clone();
                                                                    let fks_for_table: Vec<(String, String)> = constraints_for_table
                                                                        .map(|v| v.iter().filter(|(_, ctype)| ctype.to_uppercase().contains("FOREIGN")).cloned().collect())
                                                                        .unwrap_or_default();

                                                                    nav_tree = nav_tree.child(
                                                                        div()
                                                                            .id(ElementId::Name(format!("nav-cat-fk-{}-{}", table_schema, table_name).into()))
                                                                            .flex()
                                                                            .items_center()
                                                                            .h(px(22.0))
                                                                            .pl(px(86.0))
                                                                            .pr_2()
                                                                            .rounded_sm()
                                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                                            .cursor_pointer()
                                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                                if this.nav_subfolder_expanded.contains(&fk_key_toggle) {
                                                                                    this.nav_subfolder_expanded.remove(&fk_key_toggle);
                                                                                } else {
                                                                                    this.nav_subfolder_expanded.insert(fk_key_toggle.clone());
                                                                                }
                                                                                cx.notify();
                                                                            }))
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap_1p5()
                                                                                    .text_size(px(11.5))
                                                                                    .child(
                                                                                        div()
                                                                                            .w(px(10.0))
                                                                                            .text_size(px(9.0))
                                                                                            .text_color(theme.text_muted)
                                                                                            .child(if is_fk_exp { "▾" } else { "▸" })
                                                                                    )
                                                                                    .child(img("icons/folder.svg").w(px(16.0)).h(px(16.0)))
                                                                                    .child(
                                                                                        div()
                                                                                            .text_color(theme.text_primary)
                                                                                            .child("Foreign Keys")
                                                                                    )
                                                                            )
                                                                    );

                                                                    if is_fk_exp {
                                                                        if fks_for_table.is_empty() {
                                                                            nav_tree = nav_tree.child(
                                                                                div()
                                                                                    .h(px(20.0))
                                                                                    .pl(px(98.0))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .text_size(px(11.0))
                                                                                    .text_color(theme.text_muted)
                                                                                    .child("(No foreign keys)")
                                                                            );
                                                                        } else {
                                                                            for (fk_name, _) in &fks_for_table {
                                                                                nav_tree = nav_tree.child(
                                                                                    div()
                                                                                        .flex()
                                                                                        .items_center()
                                                                                        .gap_1p5()
                                                                                        .h(px(20.0))
                                                                                        .pl(px(98.0))
                                                                                        .text_size(px(11.0))
                                                                                        .child(img("icons/foreign_key.svg").w(px(16.0)).h(px(16.0)))
                                                                                        .child(div().text_color(theme.text_primary).child(fk_name.clone()))
                                                                                );
                                                                            }
                                                                        }
                                                                    }

                                                                    // 4. Indexes
                                                                    let idx_key = format!("{}.indexes", table_key);
                                                                    let is_idx_exp = self.nav_subfolder_expanded.contains(&idx_key);
                                                                    let idx_key_toggle = idx_key.clone();
                                                                    let idxs_for_table = self.table_indexes.get(&(table_schema.clone(), table_name.clone()));

                                                                    nav_tree = nav_tree.child(
                                                                        div()
                                                                            .id(ElementId::Name(format!("nav-cat-idx-{}-{}", table_schema, table_name).into()))
                                                                            .flex()
                                                                            .items_center()
                                                                            .h(px(22.0))
                                                                            .pl(px(86.0))
                                                                            .pr_2()
                                                                            .rounded_sm()
                                                                            .hover(|s| s.bg(theme.bg_surface_hover))
                                                                            .cursor_pointer()
                                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                                if this.nav_subfolder_expanded.contains(&idx_key_toggle) {
                                                                                    this.nav_subfolder_expanded.remove(&idx_key_toggle);
                                                                                } else {
                                                                                    this.nav_subfolder_expanded.insert(idx_key_toggle.clone());
                                                                                }
                                                                                cx.notify();
                                                                            }))
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap_1p5()
                                                                                    .text_size(px(11.5))
                                                                                    .child(
                                                                                        div()
                                                                                            .w(px(10.0))
                                                                                            .text_size(px(9.0))
                                                                                            .text_color(theme.text_muted)
                                                                                            .child(if is_idx_exp { "▾" } else { "▸" })
                                                                                    )
                                                                                    .child(img("icons/folder.svg").w(px(16.0)).h(px(16.0)))
                                                                                    .child(
                                                                                        div()
                                                                                            .text_color(theme.text_primary)
                                                                                            .child("Indexes")
                                                                                    )
                                                                            )
                                                                    );

                                                                    if is_idx_exp {
                                                                        let mut rendered_any = false;
                                                                        if let Some(idxs) = idxs_for_table {
                                                                            for iname in idxs {
                                                                                rendered_any = true;
                                                                                nav_tree = nav_tree.child(
                                                                                    div()
                                                                                        .flex()
                                                                                        .items_center()
                                                                                        .gap_1p5()
                                                                                        .h(px(20.0))
                                                                                        .pl(px(98.0))
                                                                                        .text_size(px(11.0))
                                                                                        .child(img("icons/table_index.svg").w(px(16.0)).h(px(16.0)))
                                                                                        .child(div().text_color(theme.text_primary).child(iname.clone()))
                                                                                );
                                                                            }
                                                                        }
                                                                        if !rendered_any {
                                                                            nav_tree = nav_tree.child(
                                                                                div()
                                                                                    .h(px(20.0))
                                                                                    .pl(px(98.0))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .text_size(px(11.0))
                                                                                    .text_color(theme.text_muted)
                                                                                    .child("(No indexes)")
                                                                            );
                                                                        }
                                                                    }

                                                                    // Remaining categories: Dependencies, References, Partitions, Triggers, Rules, Policies
                                                                    let other_folders: [(&str, &str, &str, bool); 6] = [
                                                                        ("dependencies", "Dependencies", "(No dependencies)", false),
                                                                        ("references", "References", "(No references)", false),
                                                                        ("partitions", "Partitions", "(No partitions)", true),
                                                                        ("triggers", "Triggers", "(No triggers)", false),
                                                                        ("rules", "Rules", "(No rules)", false),
                                                                        ("policies", "Policies", "(No policies)", false),
                                                                    ];

                                                                    for (f_id, f_label, f_empty, is_partition) in other_folders {
                                                                        let cat_key = format!("{}.{}", table_key, f_id);
                                                                        let is_cat_exp = self.nav_subfolder_expanded.contains(&cat_key);
                                                                        let cat_key_toggle = cat_key.clone();

                                                                        nav_tree = nav_tree.child(
                                                                            div()
                                                                                .id(ElementId::Name(format!("nav-cat-{}-{}-{}", f_id, table_schema, table_name).into()))
                                                                                .flex()
                                                                                .items_center()
                                                                                .h(px(22.0))
                                                                                .pl(px(86.0))
                                                                                .pr_2()
                                                                                .rounded_sm()
                                                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                                                .cursor_pointer()
                                                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                                                    if this.nav_subfolder_expanded.contains(&cat_key_toggle) {
                                                                                        this.nav_subfolder_expanded.remove(&cat_key_toggle);
                                                                                    } else {
                                                                                        this.nav_subfolder_expanded.insert(cat_key_toggle.clone());
                                                                                    }
                                                                                    cx.notify();
                                                                                }))
                                                                                .child(
                                                                                    div()
                                                                                        .flex()
                                                                                        .items_center()
                                                                                        .gap_1p5()
                                                                                        .text_size(px(11.5))
                                                                                        .child(
                                                                                            div()
                                                                                                .w(px(10.0))
                                                                                                .text_size(px(9.0))
                                                                                                .text_color(theme.text_muted)
                                                                                                .child(if is_cat_exp { "▾" } else { "▸" })
                                                                                        )
                                                                                        .child(
                                                                                            if is_partition {
                                                                                                img("icons/folder_table.svg").w(px(16.0)).h(px(16.0))
                                                                                            } else {
                                                                                                img("icons/folder.svg").w(px(16.0)).h(px(16.0))
                                                                                            }
                                                                                        )
                                                                                        .child(
                                                                                            div()
                                                                                                .text_color(theme.text_primary)
                                                                                                .child(f_label)
                                                                                        )
                                                                                )
                                                                        );

                                                                        if is_cat_exp {
                                                                            nav_tree = nav_tree.child(
                                                                                div()
                                                                                    .h(px(20.0))
                                                                                    .pl(px(98.0))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .text_size(px(11.0))
                                                                                    .text_color(theme.text_muted)
                                                                                    .child(f_empty)
                                                                            );
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        nav_tree
                    })
                )
                // Main Center Workspace: DBeaver-style Data & Table Viewer
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .overflow_hidden()
                        .bg(theme.bg_grid)
                        .child({
                            if self.tabs.is_empty() || active_tab_idx >= self.tabs.len() {
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .size_full()
                                    .gap_3()
                                    .bg(theme.bg_grid)
                                    .child(
                                        img("icons/table.svg")
                                            .w(px(52.0))
                                            .h(px(52.0))
                                    )
                                    .child(
                                        div()
                                            .text_size(px(15.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text_secondary)
                                            .child("No Table Opened")
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(theme.text_muted)
                                            .child("Click a table in the Database Navigator on the left to open and view data.")
                                    )
                                    .into_any_element()
                            } else {
                                let active_subtab = self.tabs[active_tab_idx].active_subtab;
                                let view_mode = self.tabs[active_tab_idx].view_mode;
                                let show_sql_dialog = self.tabs[active_tab_idx].show_sql_dialog;
                                let filter_sql = self.tabs[active_tab_idx].filter_sql.clone();
                                let table_name_opt = self.tabs[active_tab_idx].table_name.clone();
                                let schema_name_opt = self.tabs[active_tab_idx].schema_name.clone();
                                let current_limit = self.tabs[active_tab_idx].limit;
                                let total_rows = self.tabs[active_tab_idx].grid.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
                                let exec_ms = self.tabs[active_tab_idx].execution_time_ms;
                                let is_filter_focused = self.is_filter_focused;
                                let is_running_query = self.is_running_query;
                                let show_right_panels = self.show_right_panels;

                                let mut center_col = div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .h_full()
                                    .overflow_hidden();

                            // A. DBeaver Sub-tabs Bar: Properties | Data | Diagram
                            center_col = center_col.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .h(px(28.0))
                                    .bg(theme.bg_surface)
                                    .border_b_1()
                                    .border_color(theme.border)
                                    .px_2()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            // 1. Properties
                                            .child({
                                                let is_prop = active_subtab == TableSubTab::Properties;
                                                div()
                                                    .id("subtab-properties")
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .px_2p5()
                                                    .py_1()
                                                    .rounded_sm()
                                                    .bg(if is_prop { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .border_b_2()
                                                    .border_color(if is_prop { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .text_color(if is_prop { theme.text_primary } else { theme.text_muted })
                                                    .text_size(px(11.5))
                                                    .font_weight(if is_prop { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.active_subtab = TableSubTab::Properties;
                                                            this.is_filter_focused = false;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(img("icons/folder_table.svg").w(px(13.0)).h(px(13.0)))
                                                    .child("Properties")
                                            })
                                            // 2. Data
                                            .child({
                                                let is_data = active_subtab == TableSubTab::Data;
                                                div()
                                                    .id("subtab-data")
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .px_2p5()
                                                    .py_1()
                                                    .rounded_sm()
                                                    .bg(if is_data { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .border_b_2()
                                                    .border_color(if is_data { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .text_color(if is_data { theme.text_primary } else { theme.text_muted })
                                                    .text_size(px(11.5))
                                                    .font_weight(if is_data { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.active_subtab = TableSubTab::Data;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(img("icons/grid.svg").w(px(13.0)).h(px(13.0)))
                                                    .child("Data")
                                            })
                                            // 3. Diagram
                                            .child({
                                                let is_diag = active_subtab == TableSubTab::Diagram;
                                                div()
                                                    .id("subtab-diagram")
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .px_2p5()
                                                    .py_1()
                                                    .rounded_sm()
                                                    .bg(if is_diag { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .border_b_2()
                                                    .border_color(if is_diag { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .text_color(if is_diag { theme.text_primary } else { theme.text_muted })
                                                    .text_size(px(11.5))
                                                    .font_weight(if is_diag { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.active_subtab = TableSubTab::Diagram;
                                                            this.is_filter_focused = false;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(img("icons/schema.svg").w(px(13.0)).h(px(13.0)))
                                                    .child("Diagram")
                                            })
                                    )
                            );

                            // B. Filter Toolbar (when in Data subtab)
                            if active_subtab == TableSubTab::Data {
                                center_col = center_col.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .h(px(32.0))
                                        .bg(theme.bg_surface)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .px_2()
                                        // Show SQL Button
                                        .child(
                                            div()
                                                .id("btn-show-sql")
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .bg(if show_sql_dialog { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(rgb(0x3fb950))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                        t.show_sql_dialog = !t.show_sql_dialog;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(img("icons/sql.svg").w(px(12.0)).h(px(12.0)))
                                                .child("Show SQL")
                                        )
                                        .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                                        .child(
                                            div()
                                                .px_1()
                                                .py_0p5()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.text_color(theme.text_primary))
                                                .cursor_pointer()
                                                .child("⛶")
                                        )
                                        // Filter Expression Input Box ("khung để nối thêm sql như dbeaver")
                                        .child(
                                            div()
                                                .id("filter-sql-input-box")
                                                .flex()
                                                .flex_1()
                                                .items_center()
                                                .h(px(24.0))
                                                .bg(theme.bg_editor)
                                                .border_1()
                                                .border_color(if is_filter_focused { rgb(0x388bfd) } else { theme.border })
                                                .rounded_xs()
                                                .px_2()
                                                .cursor_text()
                                                .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                                                    this.is_filter_focused = true;
                                                    this.filter_selected_all = event.click_count() >= 2;
                                                    this.cursor_visible = true;
                                                    cx.notify();
                                                }))
                                                .child({
                                                    let mut filter_row = div().flex().items_center().h(px(18.0)).w_full().overflow_hidden();
                                                    if filter_sql.is_empty() {
                                                        if is_filter_focused {
                                                            filter_row = filter_row
                                                                .child(
                                                                    div()
                                                                        .w(px(1.5))
                                                                        .h(px(13.0))
                                                                        .rounded_xs()
                                                                        .bg(if self.cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                                )
                                                                .child(
                                                                    div()
                                                                        .pl_1()
                                                                        .text_size(px(11.0))
                                                                        .text_color(theme.text_muted)
                                                                        .child("Enter a SQL expression to filter results (use Ctrl+Space)")
                                                                );
                                                        } else {
                                                            filter_row = filter_row.child(
                                                                div()
                                                                    .text_size(px(11.0))
                                                                    .text_color(theme.text_muted)
                                                                    .child("Enter a SQL expression to filter results (use Ctrl+Space)")
                                                            );
                                                        }
                                                    } else {
                                                        if is_filter_focused && self.filter_selected_all {
                                                            filter_row = filter_row.child(
                                                                div()
                                                                    .px_1()
                                                                    .py_0p5()
                                                                    .rounded_xs()
                                                                    .bg(rgb(0x264f78))
                                                                    .text_color(rgb(0xffffff))
                                                                    .text_size(px(11.5))
                                                                    .child(filter_sql.clone())
                                                            );
                                                        } else if is_filter_focused {
                                                            filter_row = filter_row
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.5))
                                                                        .text_color(theme.text_primary)
                                                                        .child(filter_sql.clone())
                                                                )
                                                                .child(
                                                                    div()
                                                                        .pl(px(1.0))
                                                                        .w(px(1.5))
                                                                        .h(px(13.0))
                                                                        .rounded_xs()
                                                                        .bg(if self.cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                                );
                                                        } else {
                                                            filter_row = filter_row.child(
                                                                div()
                                                                    .text_size(px(11.5))
                                                                    .text_color(theme.text_primary)
                                                                    .child(filter_sql.clone())
                                                            );
                                                        }
                                                    }
                                                    filter_row
                                                })
                                        )
                                        // Execute Query Button ▶ (Green play arrow)
                                        .child(
                                            div()
                                                .id("btn-execute-filtered-query")
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(24.0))
                                                .h(px(24.0))
                                                .rounded_xs()
                                                .bg(if is_running_query { theme.bg_surface_hover } else { rgb(0x238636) })
                                                .hover(|s| s.bg(rgb(0x2ea043)))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.run_active_query(cx);
                                                    cx.notify();
                                                }))
                                                .child(
                                                    if is_running_query {
                                                        div().text_size(px(11.0)).child("⏳").into_any_element()
                                                    } else {
                                                        img("icons/sql_exec.svg").w(px(13.0)).h(px(13.0)).into_any_element()
                                                    }
                                                )
                                        )
                                        .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                                        // Clear filter button ✕
                                        .child(
                                            div()
                                                .id("btn-clear-filter")
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .px_1p5()
                                                .py_1()
                                                .rounded_xs()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.bg(theme.bg_surface_hover).text_color(theme.text_primary))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                        t.filter_sql.clear();
                                                        this.run_active_query(cx);
                                                        cx.notify();
                                                    }
                                                }))
                                                .child("✕")
                                        )
                                        // Filter criteria funnel ∇
                                        .child(
                                            div()
                                                .px_1()
                                                .py_0p5()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.text_color(theme.text_primary))
                                                .cursor_pointer()
                                                .child("∇")
                                        )
                                        // History Back ◀
                                        .child(
                                            div()
                                                .px_1()
                                                .py_0p5()
                                                .text_size(px(10.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.text_color(theme.text_primary))
                                                .cursor_pointer()
                                                .child("◀")
                                        )
                                        // History Forward ▶
                                        .child(
                                            div()
                                                .px_1()
                                                .py_0p5()
                                                .text_size(px(10.0))
                                                .text_color(theme.text_muted)
                                                .hover(|s| s.text_color(theme.text_primary))
                                                .cursor_pointer()
                                                .child("▶")
                                        )
                                        // Refresh ↻
                                        .child(
                                            div()
                                                .id("btn-refresh-data")
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .px_1p5()
                                                .py_1()
                                                .rounded_xs()
                                                .text_size(px(12.0))
                                                .text_color(rgb(0x58a6ff))
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.run_active_query(cx);
                                                    cx.notify();
                                                }))
                                                .child("↻")
                                        )
                                );
                            }

                            // C. Optional Show SQL preview dialog
                            if show_sql_dialog {
                                let cur_sql = self.build_query_for_tab(active_tab_idx);
                                let sql_clone = cur_sql.clone();
                                center_col = center_col.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .bg(theme.bg_editor)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .p_2p5()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text_secondary)
                                                        .child("GENERATED SQL QUERY")
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            div()
                                                                .id("btn-copy-generated-sql")
                                                                .px_2()
                                                                .py_0p5()
                                                                .rounded_xs()
                                                                .bg(theme.bg_surface)
                                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                                .text_size(px(10.5))
                                                                .text_color(theme.text_primary)
                                                                .cursor_pointer()
                                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                                    cx.write_to_clipboard(ClipboardItem::new_string(sql_clone.clone()));
                                                                }))
                                                                .child("📋 Copy")
                                                        )
                                                        .child(
                                                            div()
                                                                .id("btn-close-sql-preview")
                                                                .px_1p5()
                                                                .rounded_xs()
                                                                .text_size(px(11.0))
                                                                .text_color(theme.text_muted)
                                                                .hover(|s| s.text_color(theme.text_primary))
                                                                .cursor_pointer()
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                                        t.show_sql_dialog = false;
                                                                        cx.notify();
                                                                    }
                                                                }))
                                                                .child("✕")
                                                        )
                                                )
                                        )
                                        .child(
                                            div()
                                                .p_2()
                                                .rounded_xs()
                                                .bg(theme.bg_surface)
                                                .text_size(px(11.5))
                                                .text_color(theme.text_accent)
                                                .child(cur_sql)
                                        )
                                );
                            }

                            // D. Main View Body depending on active_subtab
                            match active_subtab {
                                TableSubTab::Data => {
                                    let mut data_row = div()
                                        .flex()
                                        .flex_row()
                                        .flex_1()
                                        .w_full()
                                        .overflow_hidden();

                                    // Left vertical toolstrip (Grid / Text)
                                    data_row = data_row.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .w(px(26.0))
                                            .h_full()
                                            .bg(theme.bg_surface)
                                            .border_r_1()
                                            .border_color(theme.border)
                                            .py_2()
                                            .gap_2()
                                            .child({
                                                let is_grid = view_mode == DataViewMode::Grid;
                                                div()
                                                    .id("toolstrip-grid")
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .justify_center()
                                                    .w(px(22.0))
                                                    .py_2()
                                                    .rounded_xs()
                                                    .bg(if is_grid { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .border_l_2()
                                                    .border_color(if is_grid { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.view_mode = DataViewMode::Grid;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(img("icons/grid.svg").w(px(14.0)).h(px(14.0)))
                                            })
                                            .child({
                                                let is_text = view_mode == DataViewMode::Text;
                                                div()
                                                    .id("toolstrip-text")
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .justify_center()
                                                    .w(px(22.0))
                                                    .py_2()
                                                    .rounded_xs()
                                                    .bg(if is_text { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .border_l_2()
                                                    .border_color(if is_text { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.view_mode = DataViewMode::Text;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(img("icons/sql_script.svg").w(px(14.0)).h(px(14.0)))
                                            })
                                    );

                                    // Center Grid / Text
                                    data_row = data_row.child(
                                        div()
                                            .id("center-data-container")
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .h_full()
                                            .overflow_hidden()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.is_filter_focused = false;
                                                cx.notify();
                                            }))
                                            .child({
                                                match view_mode {
                                                    DataViewMode::Grid => {
                                                        let sidebar_w = theme.sidebar_width.max(280.0);
                                                        let panels_w = if show_right_panels { 240.0 } else { 0.0 };
                                                        let available_viewport_w = (f32::from(window.viewport_size().width) - sidebar_w - 26.0 - 22.0 - panels_w).max(200.0);
                                                        let tab = &mut self.tabs[active_tab_idx];
                                                        tab.grid.update_viewport_width(available_viewport_w);

                                                        let active_resizing_col = self.resizing_column.and_then(|(t, c, _, _)| {
                                                            if t == active_tab_idx { Some(c) } else { None }
                                                        });

                                                        let theme_border_focused = theme.border_focused;
                                                        let theme_border = theme.border;
                                                        let view = cx.entity().downgrade();

                                                        tab.grid.render_grid_interactive(active_resizing_col, move |col_idx, cur_width| {
                                                            let is_active = active_resizing_col == Some(col_idx);
                                                            let bg_color = if is_active {
                                                                Rgba { a: 0.85, ..theme_border_focused }
                                                            } else {
                                                                Rgba { a: 0.0, ..theme_border_focused }
                                                            };
                                                            let hover_bg = Rgba { a: 0.35, ..theme_border_focused };
                                                            let grip_color = if is_active {
                                                                theme_border_focused
                                                            } else {
                                                                Rgba { a: 0.7, ..theme_border }
                                                            };

                                                            let view_clone = view.clone();

                                                            div()
                                                                .id(("col-resizer", col_idx))
                                                                .w(px(8.0))
                                                                .h_full()
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .cursor_col_resize()
                                                                .bg(bg_color)
                                                                .hover(move |s| s.bg(hover_bg))
                                                                .child(
                                                                    div()
                                                                        .w(px(1.5))
                                                                        .h(px(14.0))
                                                                        .rounded_xs()
                                                                        .bg(grip_color)
                                                                )
                                                                .on_mouse_down(
                                                                    MouseButton::Left,
                                                                    move |event: &MouseDownEvent, _window: &mut Window, app: &mut App| {
                                                                        let _ = view_clone.update(app, |this, cx| {
                                                                            if event.button == MouseButton::Left {
                                                                                if event.click_count >= 2 {
                                                                                    if let Some(tab) = this.tabs.get_mut(active_tab_idx) {
                                                                                        tab.grid.auto_fit_single_column(col_idx);
                                                                                        this.resizing_column = None;
                                                                                        cx.notify();
                                                                                    }
                                                                                } else {
                                                                                    this.resizing_column = Some((
                                                                                        active_tab_idx,
                                                                                        col_idx,
                                                                                        f32::from(event.position.x),
                                                                                        cur_width,
                                                                                    ));
                                                                                    cx.notify();
                                                                                }
                                                                            }
                                                                        });
                                                                    },
                                                                )
                                                                .into_any_element()
                                                        }).into_any_element()
                                                    }
                                                    DataViewMode::Text => {
                                                        let tab = &self.tabs[active_tab_idx];
                                                        let mut text_lines = Vec::new();
                                                        if let Some(ref pager) = tab.grid.pager {
                                                            let col_names: Vec<String> = pager.columns.iter().map(|c| c.name.clone()).collect();
                                                            text_lines.push(col_names.join(" | "));
                                                            text_lines.push(col_names.iter().map(|c| "-".repeat(c.len().max(6))).collect::<Vec<_>>().join("-+-"));
                                                            let limit_render = pager.total_rows().min(200);
                                                            for r in 0..limit_render {
                                                                let mut row_vals = Vec::new();
                                                                for c in 0..pager.num_cols() as usize {
                                                                    let val = if let Some(cell) = pager.get_cell(r, c) {
                                                                        String::from_utf8_lossy(&cell).to_string()
                                                                    } else {
                                                                        "NULL".to_string()
                                                                    };
                                                                    row_vals.push(val);
                                                                }
                                                                text_lines.push(row_vals.join(" | "));
                                                            }
                                                        }

                                                        div()
                                                            .id("text-view-scroll")
                                                            .flex()
                                                            .flex_col()
                                                            .flex_1()
                                                            .p_3()
                                                            .overflow_y_scroll()
                                                            .bg(theme.bg_editor)
                                                            .text_size(px(12.0))
                                                            .text_color(theme.text_primary)
                                                            .child(
                                                                if text_lines.is_empty() {
                                                                    "(No query result data to display in text mode)".to_string()
                                                                } else {
                                                                    text_lines.join("\n")
                                                                }
                                                            )
                                                            .into_any_element()
                                                    }
                                                }
                                            })
                                    );

                                    // Right vertical toolstrip (Panels)
                                    data_row = data_row.child(
                                        div()
                                            .id("btn-toggle-panels-strip")
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .w(px(22.0))
                                            .h_full()
                                            .bg(theme.bg_surface)
                                            .border_l_1()
                                            .border_color(theme.border)
                                            .py_3()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.show_right_panels = !this.show_right_panels;
                                                cx.notify();
                                            }))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("P"))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("A"))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("N"))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("E"))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("L"))
                                            .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("S"))
                                    );

                                    if show_right_panels {
                                        data_row = data_row.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .w(px(240.0))
                                                .h_full()
                                                .bg(theme.bg_surface)
                                                .border_l_1()
                                                .border_color(theme.border)
                                                .p_3()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .child(div().font_weight(FontWeight::BOLD).text_size(px(11.0)).text_color(theme.text_secondary).child("RECORD VALUE"))
                                                        .child(
                                                            div()
                                                                .id("btn-close-right-panels")
                                                                .px_1()
                                                                .text_size(px(11.0))
                                                                .text_color(theme.text_muted)
                                                                .cursor_pointer()
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    this.show_right_panels = false;
                                                                    cx.notify();
                                                                }))
                                                                .child("✕")
                                                        )
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .p_2()
                                                        .rounded_xs()
                                                        .bg(theme.bg_editor)
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .text_size(px(11.0))
                                                        .text_color(theme.text_muted)
                                                        .child("Select a cell or record in the data grid to view full content, formatting, and metadata.")
                                                )
                                        );
                                    }

                                    center_col = center_col.child(data_row);
                                }
                                TableSubTab::Properties => {
                                    // Properties View
                                    let mut prop_view = div()
                                        .id("prop-view-scroll")
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .p_4()
                                        .gap_3()
                                        .overflow_y_scroll();

                                    let t_name = table_name_opt.as_deref().unwrap_or("Table");
                                    let s_name = schema_name_opt.as_deref().unwrap_or("public");

                                    prop_view = prop_view.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(img("icons/table.svg").w(px(18.0)).h(px(18.0)))
                                            .child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme.text_primary)
                                                    .child(format!("{}.{}", s_name, t_name))
                                            )
                                    );

                                    // Columns Table
                                    let columns_opt = self.table_columns.get(&(s_name.to_string(), t_name.to_string()));
                                    prop_view = prop_view.child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(12.0))
                                            .text_color(theme.text_secondary)
                                            .child("COLUMNS")
                                    );

                                    let mut col_table = div()
                                        .flex()
                                        .flex_col()
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                        .bg(theme.bg_surface);

                                    // Header
                                    col_table = col_table.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .h(px(24.0))
                                            .bg(theme.bg_grid_header)
                                            .border_b_1()
                                            .border_color(theme.border)
                                            .px_2()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text_secondary)
                                            .child(div().w(px(40.0)).child("#"))
                                            .child(div().w(px(180.0)).child("Name"))
                                            .child(div().w(px(140.0)).child("Data Type"))
                                            .child(div().w(px(90.0)).child("Nullable"))
                                            .child(div().w(px(80.0)).child("Key"))
                                    );

                                    if let Some(cols) = columns_opt {
                                        for (idx, c) in cols.iter().enumerate() {
                                            col_table = col_table.child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .h(px(24.0))
                                                    .border_b_1()
                                                    .border_color(theme.border)
                                                    .px_2()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.text_primary)
                                                    .child(div().w(px(40.0)).text_color(theme.text_muted).child(format!("{}", idx + 1)))
                                                    .child(div().w(px(180.0)).child(c.name.clone()))
                                                    .child(div().w(px(140.0)).text_color(theme.text_accent).child(c.data_type.to_string()))
                                                    .child(div().w(px(90.0)).text_color(if c.nullable { theme.text_muted } else { theme.text_warning }).child(if c.nullable { "YES" } else { "NO" }))
                                                    .child(div().w(px(80.0)).child(if c.is_primary_key { "PK 🗝" } else if c.is_foreign_key { "FK" } else { "-" }))
                                            );
                                        }
                                    } else {
                                        col_table = col_table.child(
                                            div()
                                                .p_2()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child("No column schema details cached yet. Expand table in navigator to fetch.")
                                        );
                                    }
                                    prop_view = prop_view.child(col_table);

                                    center_col = center_col.child(prop_view);
                                }
                                TableSubTab::Diagram => {
                                    // Diagram View
                                    let t_name = table_name_opt.as_deref().unwrap_or("Table");
                                    let s_name = schema_name_opt.as_deref().unwrap_or("public");
                                    let columns_opt = self.table_columns.get(&(s_name.to_string(), t_name.to_string()));

                                    let diag_view = div()
                                        .id("diag-view-scroll")
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .p_6()
                                        .bg(theme.bg_editor)
                                        .overflow_y_scroll()
                                        .child(
                                            div()
                                                .w(px(280.0))
                                                .rounded_md()
                                                .bg(theme.bg_surface)
                                                .border_1()
                                                .border_color(rgb(0x388bfd))
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1p5()
                                                        .px_3()
                                                        .py_2()
                                                        .bg(rgb(0x135ba2))
                                                        .text_color(rgb(0xffffff))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_size(px(12.0))
                                                        .child(img("icons/table.svg").w(px(14.0)).h(px(14.0)))
                                                        .child(format!("{}.{}", s_name, t_name))
                                                )
                                                .child({
                                                    let mut col_list = div().flex().flex_col().p_2().gap_1();
                                                    if let Some(cols) = columns_opt {
                                                        for c in cols {
                                                            col_list = col_list.child(
                                                                div()
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_between()
                                                                    .text_size(px(11.0))
                                                                    .child(
                                                                        div()
                                                                            .flex()
                                                                            .items_center()
                                                                            .gap_1()
                                                                            .child(if c.is_primary_key { "🗝" } else { "•" })
                                                                            .child(div().text_color(theme.text_primary).child(c.name.clone()))
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_color(theme.text_muted)
                                                                            .text_size(px(9.5))
                                                                            .child(c.data_type.to_string())
                                                                    )
                                                            );
                                                        }
                                                    } else {
                                                        col_list = col_list.child(
                                                            div().text_size(px(11.0)).text_color(theme.text_muted).child("Expand table in navigator to display diagram columns.")
                                                        );
                                                    }
                                                    col_list
                                                })
                                        );

                                    center_col = center_col.child(diag_view);
                                }
                            }

                            // E. Table Bottom Pagination & Status Bar (when in Data subtab)
                            if active_subtab == TableSubTab::Data {
                                center_col = center_col.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(24.0))
                                        .bg(theme.bg_surface)
                                        .border_t_1()
                                        .border_color(theme.border)
                                        .px_3()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(img("icons/grid.svg").w(px(12.0)).h(px(12.0)))
                                                .child({
                                                    let exec_str = if let Some(ms) = exec_ms {
                                                        format!(" | {}ms", ms)
                                                    } else {
                                                        "".to_string()
                                                    };
                                                    format!("{} rows{}", total_rows, exec_str)
                                                })
                                                .child(div().text_color(theme.text_muted).child(" | Zero-Copy 120 FPS Culled Grid"))
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_3()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("|◀"))
                                                        .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("◀"))
                                                        .child(div().px_1p5().text_size(px(10.5)).text_color(theme.text_secondary).child(format!("1 - {}", current_limit)))
                                                        .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("▶"))
                                                        .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("▶|"))
                                                )
                                                .child(
                                                    div()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded_xs()
                                                        .bg(theme.bg_editor)
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .text_size(px(10.5))
                                                        .text_color(theme.text_secondary)
                                                        .child(format!("Limit: {}", current_limit))
                                                )
                                        )
                                );
                            }

                                center_col.into_any_element()
                            }
                        })
                )
        );

        // 3. Bottom Status Bar
        root = root.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(theme.status_bar_height))
                .bg(theme.bg_surface)
                .border_t_1()
                .border_color(theme.border)
                .px_3()
                .text_size(px(11.0))
                .text_color(theme.text_muted)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    if self.is_connected {
                                        let drv = match self.selected_driver_kind {
                                            DbDriverKind::Postgres => "postgres",
                                            DbDriverKind::MySql => "mysql",
                                            DbDriverKind::Sqlite => "sqlite",
                                        };
                                        dbeaver_driver_icon(drv, true)
                                    } else {
                                        div().child(img("icons/disconnect.svg").w(px(14.0)).h(px(14.0)))
                                    }
                                )
                                .child(if self.is_connected {
                                    let name = self.active_saved_connection().map(|s| s.name.as_str()).unwrap_or(&self.form_database);
                                    format!("Connected: {:?} ({})", self.selected_driver_kind, name)
                                } else {
                                    "Disconnected".to_string()
                                })
                        )
                        .child(self.status_message.clone())
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child("Zero-Copy SlottedPage (64KB)")
                        .child("120 FPS Metal/Vulkan")
                )
        );

        // 4. Modal Dialog: Database Connection Setup (When show_connection_modal is true)
        if self.show_connection_modal {
            let is_pg = self.selected_driver_kind == DbDriverKind::Postgres;
            let is_mysql = self.selected_driver_kind == DbDriverKind::MySql;
            let is_sqlite = self.selected_driver_kind == DbDriverKind::Sqlite;

            let is_name_active = self.active_field == Some(FormField::Name);
            let is_host_active = self.active_field == Some(FormField::Host);
            let is_port_active = self.active_field == Some(FormField::Port);
            let is_db_active = self.active_field == Some(FormField::Database);
            let is_user_active = self.active_field == Some(FormField::User);
            let is_pass_active = self.active_field == Some(FormField::Password);

            let modal_overlay = div()
                .absolute()
                .inset_0()
                .bg(rgba(0x00000088))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(540.0))
                        .bg(theme.bg_surface)
                        .border_1()
                        .border_color(theme.border_focused)
                        .rounded_lg()
                        .shadow_lg()
                        .p_5()
                        .flex()
                        .flex_col()
                        .gap_4()
                        // Modal Header
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .border_b_1()
                                .border_color(theme.border)
                                .pb_3()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .text_size(px(16.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child(img("icons/connect.svg").w(px(18.0)).h(px(18.0)))
                                                .child(if self.editing_connection_id.is_some() {
                                                    "Edit Database Connection"
                                                } else {
                                                    "New Database Connection"
                                                })
                                        )
                                )
                                .child(
                                    div()
                                        .id("btn-close-modal-x")
                                        .text_size(px(14.0))
                                        .text_color(theme.text_muted)
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_connection_modal = false;
                                            cx.notify();
                                        }))
                                        .child("✕")
                                )
                        )
                        // Driver Selection Tabs
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("btn-select-driver-pg")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .bg(if is_pg { theme.bg_surface_selected } else { theme.bg_app })
                                        .border_1()
                                        .border_color(if is_pg { theme.text_accent } else { theme.border })
                                        .text_size(px(12.0))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_preset_postgres();
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(img("icons/postgresql.png").w(px(16.0)).h(px(16.0)))
                                                .child("PostgreSQL")
                                        )
                                )
                                .child(
                                    div()
                                        .id("btn-select-driver-mysql")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .bg(if is_mysql { theme.bg_surface_selected } else { theme.bg_app })
                                        .border_1()
                                        .border_color(if is_mysql { theme.text_accent } else { theme.border })
                                        .text_size(px(12.0))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_preset_mysql();
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(img("icons/mysql.png").w(px(16.0)).h(px(16.0)))
                                                .child("MySQL")
                                        )
                                )
                                .child(
                                    div()
                                        .id("btn-select-driver-sqlite")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .bg(if is_sqlite { theme.bg_surface_selected } else { theme.bg_app })
                                        .border_1()
                                        .border_color(if is_sqlite { theme.text_accent } else { theme.border })
                                        .text_size(px(12.0))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_preset_sqlite();
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(img("icons/sqlite.png").w(px(16.0)).h(px(16.0)))
                                                .child("SQLite")
                                        )
                                )
                        )
                        // Form Input Fields
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                // Connection Name (Optional)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Connection Name (Optional)"))
                                        .child(
                                            div()
                                                .id("input-field-name")
                                                .p_2()
                                                .rounded_md()
                                                .bg(theme.bg_app)
                                                .border_1()
                                                .border_color(if is_name_active { theme.text_accent } else { theme.border })
                                                .text_size(px(13.0))
                                                .cursor_text()
                                                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                    this.active_field = Some(FormField::Name);
                                                    this.field_selected_all = event.click_count() >= 2;
                                                    this.cursor_visible = true;
                                                    window.focus(&this.focus_handle);
                                                    cx.notify();
                                                }))
                                                .child(render_field_content(
                                                    &self.form_name,
                                                    "(optional - defaults to database name)",
                                                    is_name_active,
                                                    if is_name_active { self.field_selected_all } else { false },
                                                    self.cursor_visible,
                                                    theme,
                                                ))
                                        )
                                )
                                // Host & Port row (hidden for SQLite)
                                .child(if !is_sqlite {
                                    div()
                                        .flex()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .flex_1()
                                                .gap_1()
                                                .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Host"))
                                                .child(
                                                    div()
                                                        .id("input-field-host")
                                                        .p_2()
                                                        .rounded_md()
                                                        .bg(theme.bg_app)
                                                        .border_1()
                                                        .border_color(if is_host_active { theme.text_accent } else { theme.border })
                                                        .text_size(px(13.0))
                                                        .cursor_text()
                                                        .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                            this.active_field = Some(FormField::Host);
                                                            this.field_selected_all = event.click_count() >= 2;
                                                            this.cursor_visible = true;
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(render_field_content(
                                                            &self.form_host,
                                                            "localhost",
                                                            is_host_active,
                                                            if is_host_active { self.field_selected_all } else { false },
                                                            self.cursor_visible,
                                                            theme,
                                                        ))
                                                )
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .w(px(120.0))
                                                .gap_1()
                                                .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Port"))
                                                .child(
                                                    div()
                                                        .id("input-field-port")
                                                        .p_2()
                                                        .rounded_md()
                                                        .bg(theme.bg_app)
                                                        .border_1()
                                                        .border_color(if is_port_active { theme.text_accent } else { theme.border })
                                                        .text_size(px(13.0))
                                                        .cursor_text()
                                                        .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                            this.active_field = Some(FormField::Port);
                                                            this.field_selected_all = event.click_count() >= 2;
                                                            this.cursor_visible = true;
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(render_field_content(
                                                            &self.form_port,
                                                            "5432",
                                                            is_port_active,
                                                            if is_port_active { self.field_selected_all } else { false },
                                                            self.cursor_visible,
                                                            theme,
                                                        ))
                                                )
                                        )
                                } else {
                                    div()
                                })
                                // Database Name / SQLite File row
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .flex_1()
                                                .gap_1()
                                                .child(div().text_size(px(11.0)).text_color(theme.text_muted).child(
                                                    if is_sqlite { "SQLite Database File / Path" } else { "Database Name" }
                                                ))
                                                .child(
                                                    div()
                                                        .id("input-field-db")
                                                        .p_2()
                                                        .rounded_md()
                                                        .bg(theme.bg_app)
                                                        .border_1()
                                                        .border_color(if is_db_active { theme.text_accent } else { theme.border })
                                                        .text_size(px(13.0))
                                                        .cursor_text()
                                                        .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                            this.active_field = Some(FormField::Database);
                                                            this.field_selected_all = event.click_count() >= 2;
                                                            this.cursor_visible = true;
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(render_field_content(
                                                            &self.form_database,
                                                            if is_sqlite { "e.g. ./test.db or :memory:" } else { "database name" },
                                                            is_db_active,
                                                            if is_db_active { self.field_selected_all } else { false },
                                                            self.cursor_visible,
                                                            theme,
                                                        ))
                                                )
                                        )
                                        .child(if !is_sqlite {
                                            div()
                                                .flex()
                                                .flex_col()
                                                .flex_1()
                                                .gap_1()
                                                .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Username"))
                                                .child(
                                                    div()
                                                        .id("input-field-user")
                                                        .p_2()
                                                        .rounded_md()
                                                        .bg(theme.bg_app)
                                                        .border_1()
                                                        .border_color(if is_user_active { theme.text_accent } else { theme.border })
                                                        .text_size(px(13.0))
                                                        .cursor_text()
                                                        .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                            this.active_field = Some(FormField::User);
                                                            this.field_selected_all = event.click_count() >= 2;
                                                            this.cursor_visible = true;
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(render_field_content(
                                                            &self.form_user,
                                                            "username",
                                                            is_user_active,
                                                            if is_user_active { self.field_selected_all } else { false },
                                                            self.cursor_visible,
                                                            theme,
                                                        ))
                                                )
                                        } else {
                                            div()
                                        })
                                )
                                // Password row (hidden for SQLite)
                                .child(if !is_sqlite {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Password"))
                                                .child(
                                                    div()
                                                        .id("btn-toggle-show-pass")
                                                        .text_size(px(11.0))
                                                        .text_color(theme.text_accent)
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.show_password = !this.show_password;
                                                            cx.notify();
                                                        }))
                                                        .child(if self.show_password { "🙈 Hide" } else { "👁 Show" })
                                                )
                                        )
                                        .child({
                                            let display_pass = if self.show_password {
                                                self.form_password.clone()
                                            } else {
                                                "•".repeat(self.form_password.chars().count())
                                            };
                                            div()
                                                .id("input-field-pass")
                                                .p_2()
                                                .rounded_md()
                                                .bg(theme.bg_app)
                                                .border_1()
                                                .border_color(if is_pass_active { theme.text_accent } else { theme.border })
                                                .text_size(px(13.0))
                                                .cursor_text()
                                                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                    this.active_field = Some(FormField::Password);
                                                    this.field_selected_all = event.click_count() >= 2;
                                                    this.cursor_visible = true;
                                                    window.focus(&this.focus_handle);
                                                    cx.notify();
                                                }))
                                                .child(render_field_content(
                                                    &display_pass,
                                                    "(optional password)",
                                                    is_pass_active,
                                                    if is_pass_active { self.field_selected_all } else { false },
                                                    self.cursor_visible,
                                                    theme,
                                                ))
                                        })
                                } else {
                                    div()
                                })
                        )
                        // Test Result Notification Banner
                        .child({
                            match &self.test_result {
                                Some(Ok(msg)) => div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(theme.bg_surface_selected)
                                    .text_color(theme.text_success)
                                    .text_size(px(12.0))
                                    .child(format!("✓ {}", msg)),
                                Some(Err(err)) => div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .text_color(theme.text_error)
                                    .text_size(px(12.0))
                                    .child(format!("✕ {}", err)),
                                None => div(),
                            }
                        })
                        // Modal Action Buttons
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .pt_2()
                                .border_t_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .id("btn-test-connection")
                                        .flex()
                                        .items_center()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .bg(if self.is_connecting { theme.bg_app } else { theme.bg_surface_hover })
                                        .border_1()
                                        .border_color(theme.border)
                                        .text_size(px(12.0))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if !this.is_connecting {
                                                this.test_connection(cx);
                                            }
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(
                                                    if self.is_connecting {
                                                        div().child("⏳")
                                                    } else {
                                                        div().child(img("icons/sql_validate.svg").w(px(14.0)).h(px(14.0)))
                                                    }
                                                )
                                                .child(if self.is_connecting { "Testing..." } else { "Test Connection" })
                                        )
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .id("btn-cancel-modal")
                                                .flex()
                                                .items_center()
                                                .px_3()
                                                .py_1p5()
                                                .rounded_md()
                                                .bg(theme.bg_surface_hover)
                                                .border_1()
                                                .border_color(theme.border)
                                                .text_size(px(12.0))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_connection_modal = false;
                                                    cx.notify();
                                                }))
                                                .child("Cancel")
                                        )
                                        .child(
                                            div()
                                                .id("btn-save-conn-only")
                                                .flex()
                                                .items_center()
                                                .px_3()
                                                .py_1p5()
                                                .rounded_md()
                                                .bg(theme.bg_surface_hover)
                                                .border_1()
                                                .border_color(theme.border)
                                                .text_size(px(12.0))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.save_connection_form();
                                                    cx.notify();
                                                }))
                                                .child("Save")
                                        )
                                        .child(
                                            div()
                                                .id("btn-connect-database")
                                                .flex()
                                                .items_center()
                                                .px_4()
                                                .py_1p5()
                                                .rounded_md()
                                                .bg(if self.is_connecting { theme.bg_surface_hover } else { theme.text_accent })
                                                .text_color(if self.is_connecting { theme.text_muted } else { theme.bg_app })
                                                .font_weight(FontWeight::BOLD)
                                                .text_size(px(12.0))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if !this.is_connecting {
                                                        this.connect_to_database(cx);
                                                    }
                                                }))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1p5()
                                                        .child(
                                                            if self.is_connecting {
                                                                div().child("⏳")
                                                            } else {
                                                                div().child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                                            }
                                                        )
                                                        .child(if self.is_connecting { "Connecting..." } else { "Connect Database" })
                                                )
                                        )
                                )
                        )
                );

            root = root.child(modal_overlay);
        }

        root
    }
}
