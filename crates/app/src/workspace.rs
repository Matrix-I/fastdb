use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use db_wire::{create_driver, ConnectionConfig, DatabaseDriver, DbDriverKind, TableSummary};
use gpui::*;
use sql_intel::SchemaCache;
use ui_core::theme::Theme;
use ui_editor::SqlEditorView;
use ui_grid::VirtualDataGrid;

pub struct TabState {
    pub title: String,
    pub editor: SqlEditorView,
    pub grid: VirtualDataGrid,
    pub execution_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
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
    pub status_message: String,

    // Real Database Connection Setup State
    pub is_connected: bool,
    pub is_connecting: bool,
    pub show_connection_modal: bool,
    pub selected_driver_kind: DbDriverKind,
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
    pub selected_table_name: Option<String>,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let focus_handle = cx.focus_handle();
        let schema_cache = Arc::new(SchemaCache::new().expect("Failed to initialize SchemaCache"));

        let initial_sql = "-- Welcome to FastDB — Pure Rust Database Client\n-- Connect to a real database (PostgreSQL, MySQL, SQLite) to start querying.\n";
        let initial_editor = SqlEditorView::new(initial_sql, theme.clone(), Some(schema_cache.clone()));
        let initial_grid = VirtualDataGrid::new(None, theme.clone());

        let initial_tab = TabState {
            title: "Query 1".to_string(),
            editor: initial_editor,
            grid: initial_grid,
            execution_time_ms: None,
        };

        Self {
            theme,
            focus_handle,
            active_tab: 0,
            tabs: vec![initial_tab],
            driver: None,
            schema_cache,
            tables: Vec::new(),
            is_running_query: false,
            status_message: "⚪ Disconnected — Please configure and connect to a database.".to_string(),

            // Connection Setup Form Defaults (PostgreSQL)
            is_connected: false,
            is_connecting: false,
            show_connection_modal: true,
            selected_driver_kind: DbDriverKind::Postgres,
            form_host: "localhost".to_string(),
            form_port: "5432".to_string(),
            form_database: "postgres".to_string(),
            form_user: "postgres".to_string(),
            form_password: "".to_string(),
            form_use_unix_socket: true,
            active_field: Some(FormField::Host),
            show_password: false,
            test_result: None,

            // DBeaver Navigation Tree Defaults
            nav_connection_expanded: true,
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
            selected_table_name: None,
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
        let selected_driver_kind = self.selected_driver_kind;

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

                        Ok::<_, String>((driver, tables, columns_info, config))
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
                    Ok((driver, tables, columns_info, config)) => {
                        let _ = this.schema_cache.clear();
                        for t in &tables {
                            let _ = this.schema_cache.add_table(&t.schema, &t.name, &t.table_type);
                        }
                        for (schema, table, cols) in columns_info {
                            for c in cols {
                                let _ = this.schema_cache.add_column(&schema, &table, &c.name, c.data_type.display_name(), c.is_primary_key);
                            }
                        }

                        this.tables = tables;
                        this.driver = Some(driver);
                        this.is_connected = true;
                        this.show_connection_modal = false;
                        this.test_result = None;

                        for t in &this.tables {
                            this.nav_schema_expanded.insert(t.schema.clone());
                            this.nav_tables_expanded.insert(t.schema.clone());
                        }

                        let db_label = format!("{:?} ({}:{})", config.driver, config.host, config.port);
                        this.status_message = format!("🟢 Connected to real database: {} — {} tables found", db_label, this.tables.len());

                        let starter_sql = if let Some(first_table) = this.tables.first() {
                            this.selected_table_name = Some(first_table.name.clone());
                            match selected_driver_kind {
                                DbDriverKind::Postgres => format!("SELECT * FROM \"{}\".\"{}\" LIMIT 100;", first_table.schema, first_table.name),
                                DbDriverKind::MySql => format!("SELECT * FROM `{}`.`{}` LIMIT 100;", first_table.schema, first_table.name),
                                DbDriverKind::Sqlite => format!("SELECT * FROM \"{}\" LIMIT 100;", first_table.name),
                            }
                        } else if selected_driver_kind == DbDriverKind::Sqlite {
                            "-- Connected to real SQLite database.\n-- You can create tables and run queries:\nCREATE TABLE IF NOT EXISTS notes (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    title TEXT NOT NULL,\n    content TEXT,\n    created_at DATETIME DEFAULT CURRENT_TIMESTAMP\n);\nINSERT INTO notes (title, content) VALUES ('Welcome', 'Connected to real SQLite database via FastDB pure Rust engine');\nSELECT * FROM notes;".to_string()
                        } else {
                            "SELECT 1;".to_string()
                        };

                        if !this.tabs.is_empty() {
                            let tab = &mut this.tabs[this.active_tab];
                            tab.editor = SqlEditorView::new(&starter_sql, this.theme.clone(), Some(this.schema_cache.clone()));
                            tab.grid = VirtualDataGrid::new(None, this.theme.clone());
                        }

                        if !this.tables.is_empty() {
                            this.run_active_query(cx);
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
        self.status_message = "⚪ Disconnected from database.".to_string();
        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.grid = VirtualDataGrid::new(None, self.theme.clone());
            tab.execution_time_ms = None;
        }
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

    pub fn query_table(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        self.selected_table_name = Some(table.to_string());
        let sql = match self.selected_driver_kind {
            DbDriverKind::Postgres => format!("SELECT * FROM \"{}\".\"{}\" LIMIT 100;", schema, table),
            DbDriverKind::MySql => format!("SELECT * FROM `{}`.`{}` LIMIT 100;", schema, table),
            DbDriverKind::Sqlite => format!("SELECT * FROM \"{}\" LIMIT 100;", table),
        };

        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.editor = SqlEditorView::new(&sql, self.theme.clone(), Some(self.schema_cache.clone()));
        }

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

        let tab = &mut self.tabs[self.active_tab];
        let mut sql = tab.editor.active_statement();
        if sql.trim().is_empty() || sql.trim().starts_with("--") {
            let full_text = tab.editor.buffer.text();
            let non_comments: Vec<&str> = full_text
                .lines()
                .filter(|l| !l.trim().starts_with("--") && !l.trim().is_empty())
                .collect();
            if !non_comments.is_empty() {
                sql = non_comments.join("\n");
            }
        }

        if sql.trim().is_empty() {
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
                            tab.grid.set_table_name("result");
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
            editor,
            grid,
            execution_time_ms: None,
        });
        self.active_tab = self.tabs.len() - 1;
    }

    #[allow(dead_code)]
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
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
                if this.show_connection_modal {
                    if event.keystroke.key == "escape" {
                        this.show_connection_modal = false;
                        cx.notify();
                        return;
                    }
                    if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                        this.connect_to_database(cx);
                        return;
                    }
                    if event.keystroke.key == "tab" {
                        this.active_field = match this.active_field {
                            None => Some(FormField::Host),
                            Some(FormField::Host) => Some(FormField::Port),
                            Some(FormField::Port) => Some(FormField::Database),
                            Some(FormField::Database) => Some(FormField::User),
                            Some(FormField::User) => Some(FormField::Password),
                            Some(FormField::Password) => Some(FormField::Host),
                        };
                        cx.notify();
                        return;
                    }
                    // Clipboard paste (Cmd+V on macOS or Ctrl+V on Linux/Windows)
                    if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                        && event.keystroke.key.eq_ignore_ascii_case("v")
                    {
                        if let Some(item) = cx.read_from_clipboard() {
                            if let Some(pasted) = item.text() {
                                let clean = pasted.trim_matches(|c| c == '\r' || c == '\n');
                                if let Some(field) = this.active_field {
                                    match field {
                                        FormField::Host => this.form_host.push_str(clean),
                                        FormField::Port => {
                                            let digits: String = clean.chars().filter(|c| c.is_ascii_digit()).collect();
                                            this.form_port.push_str(&digits);
                                        }
                                        FormField::Database => this.form_database.push_str(clean),
                                        FormField::User => this.form_user.push_str(clean),
                                        FormField::Password => this.form_password.push_str(clean),
                                    }
                                    cx.notify();
                                }
                            }
                        }
                        return;
                    }
                    // Cmd+Backspace = clear entire field
                    if event.keystroke.modifiers.platform && event.keystroke.key == "backspace" {
                        if let Some(field) = this.active_field {
                            match field {
                                FormField::Host => this.form_host.clear(),
                                FormField::Port => this.form_port.clear(),
                                FormField::Database => this.form_database.clear(),
                                FormField::User => this.form_user.clear(),
                                FormField::Password => this.form_password.clear(),
                            }
                            cx.notify();
                        }
                        return;
                    }
                    // Backspace = delete single character
                    if event.keystroke.key == "backspace" {
                        if let Some(field) = this.active_field {
                            match field {
                                FormField::Host => { this.form_host.pop(); }
                                FormField::Port => { this.form_port.pop(); }
                                FormField::Database => { this.form_database.pop(); }
                                FormField::User => { this.form_user.pop(); }
                                FormField::Password => { this.form_password.pop(); }
                            }
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
                                match field {
                                    FormField::Host => this.form_host.push_str(char_str),
                                    FormField::Port => {
                                        if char_str.chars().all(|c| c.is_ascii_digit()) {
                                            this.form_port.push_str(char_str);
                                        }
                                    }
                                    FormField::Database => this.form_database.push_str(char_str),
                                    FormField::User => this.form_user.push_str(char_str),
                                    FormField::Password => this.form_password.push_str(char_str),
                                }
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
            }));

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
                                let tab_id = format!("tab-select-{}", i);
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
                                        .text_size(px(12.0))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.active_tab = i;
                                            cx.notify();
                                        }))
                                        .child(tab.title.clone())
                                );
                            }
                            tabs_bar.child(
                                div()
                                    .id("btn-new-tab")
                                    .flex()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.new_tab();
                                        cx.notify();
                                    }))
                                    .child("+ Tab")
                            )
                        })
                )
                // Toolbar Buttons
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("btn-run-query")
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(if self.is_running_query { theme.bg_surface_hover } else { theme.text_accent })
                                .text_color(if self.is_running_query { theme.text_muted } else { theme.bg_app })
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(12.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.run_active_query(cx);
                                    cx.notify();
                                }))
                                .child(if self.is_running_query { "⏳ Executing..." } else { "▶ Run Query (⌘↵)" })
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
                                        .child("🗄 DATABASE NAVIGATOR")
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .id("btn-open-connect-dialog")
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(theme.bg_surface)
                                                .hover(|s| s.bg(theme.bg_surface_hover))
                                                .text_size(px(11.0))
                                                .text_color(theme.text_accent)
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_connection_modal = true;
                                                    cx.notify();
                                                }))
                                                .child("➕")
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

                            if !self.is_connected {
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
                                        .child("No database connected")
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
                                                    this.show_connection_modal = true;
                                                    cx.notify();
                                                }))
                                                .child("➕ Connect to Database")
                                        )
                                );
                            } else {
                                // Level 0: Connection Node
                                let is_conn_exp = self.nav_connection_expanded;
                                let db_icon = match self.selected_driver_kind {
                                    DbDriverKind::Postgres => "🐘",
                                    DbDriverKind::MySql => "🐬",
                                    DbDriverKind::Sqlite => "📁",
                                };
                                let conn_name = format!("{}-local", self.form_database);
                                let conn_addr = if self.selected_driver_kind == DbDriverKind::Sqlite {
                                    self.form_database.clone()
                                } else {
                                    format!("{}:{}", self.form_host, self.form_port)
                                };

                                nav_tree = nav_tree.child(
                                    div()
                                        .id("nav-connection-node")
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(24.0))
                                        .pl_1()
                                        .pr_2()
                                        .rounded_sm()
                                        .hover(|s| s.bg(theme.bg_surface_hover))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.nav_connection_expanded = !this.nav_connection_expanded;
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
                                                        .text_color(theme.text_muted)
                                                        .child(if is_conn_exp { "▾" } else { "▸" })
                                                )
                                                .child(db_icon)
                                                .child(
                                                    div()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text_primary)
                                                        .child(conn_name)
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.0))
                                                        .text_color(theme.text_muted)
                                                        .child(conn_addr)
                                                )
                                        )
                                );

                                if is_conn_exp {
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
                                                    .child("🗄️")
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
                                                        .child("🛢️")
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
                                                            .child("🗂️")
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
                                                                    .child("📑")
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
                                                                        .child("📁")
                                                                        .child(format!("Tables ({})", s_tables.len()))
                                                                )
                                                        );

                                                        if is_tables_exp {
                                                            // Level 6: Table items under this schema
                                                            for t in s_tables {
                                                                let table_schema = t.schema.clone();
                                                                let table_name = t.name.clone();
                                                                let is_selected = self.selected_table_name.as_deref() == Some(&table_name);
                                                                let item_id = format!("nav-table-{}-{}", table_schema, table_name);

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
                                                                        this.query_table(&table_schema, &table_name, cx);
                                                                    }));

                                                                if is_selected {
                                                                    row_div = row_div.bg(theme.bg_surface_selected);
                                                                }

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
                                                                                        .w(px(8.0))
                                                                                        .text_size(px(9.0))
                                                                                        .text_color(theme.text_muted)
                                                                                        .child("›")
                                                                                )
                                                                                .child(
                                                                                    div()
                                                                                        .text_color(theme.text_accent)
                                                                                        .child("⊞")
                                                                                )
                                                                                .child(
                                                                                    div()
                                                                                        .font_weight(if is_selected { FontWeight::BOLD } else { FontWeight::NORMAL })
                                                                                        .text_color(if is_selected { theme.text_accent } else { theme.text_primary })
                                                                                        .child(t.name.clone())
                                                                                )
                                                                        )
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

                            nav_tree
                        })
                )
                // Main Split: Top SQL Editor + Bottom 2D Virtual Data Grid
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .overflow_hidden()
                        // Top: SQL Editor View (Takes 40% height)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .h(px(240.0))
                                .w_full()
                                .border_b_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(24.0))
                                        .bg(theme.bg_surface)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .px_3()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child("SQL Editor — Tree-sitter Syntax Highlighting")
                                        .child("Press ⌘↵ to Execute Statement")
                                )
                                .child({
                                    let tab = &mut self.tabs[active_tab_idx];
                                    tab.editor.render_editor()
                                })
                        )
                        // Bottom: 2D Virtual Data Grid (Takes remaining height)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_full()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(24.0))
                                        .bg(theme.bg_surface)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .px_3()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child("Result Set — 120 FPS 2D Viewport Culled Grid")
                                        .child({
                                            let tab = &self.tabs[active_tab_idx];
                                            let total = tab.grid.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
                                            let exec_str = if let Some(ms) = tab.execution_time_ms {
                                                format!(" | {}ms", ms)
                                            } else {
                                                "".to_string()
                                            };
                                            format!("{} rows{}", total, exec_str)
                                        })
                                )
                                .child({
                                    let tab = &mut self.tabs[active_tab_idx];
                                    tab.grid.render_grid()
                                })
                        )
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
                                .gap_1()
                                .child(if self.is_connected { "🟢" } else { "⚪" })
                                .child(if self.is_connected {
                                    format!("Connected: {:?} ({})", self.selected_driver_kind, self.form_database)
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
                                                .text_size(px(16.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child("🔌 Connect to Real Database")
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
                                        .child("🐘 PostgreSQL")
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
                                        .child("🐬 MySQL")
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
                                        .child("📁 SQLite")
                                )
                        )
                        // Form Input Fields
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
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
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.active_field = Some(FormField::Host);
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(format!("{}{}", self.form_host, if is_host_active { "|" } else { "" }))
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
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.active_field = Some(FormField::Port);
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(format!("{}{}", self.form_port, if is_port_active { "|" } else { "" }))
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
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.active_field = Some(FormField::Database);
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(format!("{}{}", self.form_database, if is_db_active { "|" } else { "" }))
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
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.active_field = Some(FormField::User);
                                                            window.focus(&this.focus_handle);
                                                            cx.notify();
                                                        }))
                                                        .child(format!("{}{}", self.form_user, if is_user_active { "|" } else { "" }))
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
                                        .child(
                                            div()
                                                .id("input-field-pass")
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .p_2()
                                                .rounded_md()
                                                .bg(theme.bg_app)
                                                .border_1()
                                                .border_color(if is_pass_active { theme.text_accent } else { theme.border })
                                                .text_size(px(13.0))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.active_field = Some(FormField::Password);
                                                    window.focus(&this.focus_handle);
                                                    cx.notify();
                                                }))
                                                .child(if self.form_password.is_empty() {
                                                    if is_pass_active { "| (type or paste password)".to_string() } else { "(empty / no password)".to_string() }
                                                } else if self.show_password {
                                                    format!("{}{}", self.form_password, if is_pass_active { "|" } else { "" })
                                                } else {
                                                    format!("{}{}", "•".repeat(self.form_password.chars().count()), if is_pass_active { "|" } else { "" })
                                                })
                                        )
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
                                        .child(if self.is_connecting { "⏳ Testing..." } else { "🔍 Test Connection" })
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
                                                .child(if self.is_connecting { "⏳ Connecting..." } else { "🔌 Connect Database" })
                                        )
                                )
                        )
                );

            root = root.child(modal_overlay);
        }

        root
    }
}
