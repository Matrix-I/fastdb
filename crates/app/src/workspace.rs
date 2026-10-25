use std::sync::Arc;
use std::time::Instant;
use db_wire::{create_driver, ConnectionConfig, DatabaseDriver, DbDriverKind, StreamEvent, TableSummary};
use gpui::*;
use mem_engine::QueryResultPager;
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
            connect_timeout: std::time::Duration::from_millis(2500),
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

                let timeout_duration = std::time::Duration::from_millis(2500);
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

                let timeout_duration = std::time::Duration::from_millis(2500);
                let outcome = rt.block_on(async {
                    tokio::time::timeout(timeout_duration, async {
                        let driver = create_driver(config.clone())
                            .map_err(|e| format!("Driver initialization error: {}", e))?;

                        driver.ping().await
                            .map_err(|e| format!("Connection refused: {}", e))?;

                        let tables = driver.list_tables().await
                            .map_err(|e| format!("Connected but failed to list tables: {}", e))?;

                        let mut columns_info = Vec::new();
                        for t in &tables {
                            if let Ok(cols) = driver.list_columns(&t.schema, &t.name).await {
                                columns_info.push((t.schema.clone(), t.name.clone(), cols));
                            }
                        }

                        Ok::<_, String>((driver, tables, columns_info, config))
                    }).await
                });

                match outcome {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err("Connection timed out (server unreachable or offline)".to_string()),
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

                        let db_label = format!("{:?} ({}:{})", config.driver, config.host, config.port);
                        this.status_message = format!("🟢 Connected to real database: {} — {} tables found", db_label, this.tables.len());

                        let starter_sql = if let Some(first_table) = this.tables.first() {
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
                            this.run_active_query();
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

    pub fn query_table(&mut self, schema: &str, table: &str) {
        let sql = match self.selected_driver_kind {
            DbDriverKind::Postgres => format!("SELECT * FROM \"{}\".\"{}\" LIMIT 100;", schema, table),
            DbDriverKind::MySql => format!("SELECT * FROM `{}`.`{}` LIMIT 100;", schema, table),
            DbDriverKind::Sqlite => format!("SELECT * FROM \"{}\" LIMIT 100;", table),
        };

        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.editor = SqlEditorView::new(&sql, self.theme.clone(), Some(self.schema_cache.clone()));
        }

        self.run_active_query();
    }

    pub fn run_active_query(&mut self) {
        let Some(ref driver) = self.driver else {
            self.show_connection_modal = true;
            self.status_message = "Please connect to a database first.".to_string();
            return;
        };

        if self.tabs.is_empty() {
            return;
        }

        let tab = &mut self.tabs[self.active_tab];
        let sql = tab.editor.active_statement();
        if sql.is_empty() {
            return;
        }

        self.is_running_query = true;
        self.status_message = format!("Executing query on database: {}", sql);

        let driver = Arc::clone(driver);
        let (tx, rx) = flume::unbounded();
        let start_time = Instant::now();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // 1. Initial schema discovery
        let dummy_pager = Arc::new(QueryResultPager::new(vec![]).unwrap());
        let mut final_pager = dummy_pager.clone();

        let exec_sql = sql.clone();
        rt.block_on(async {
            let p = dummy_pager.clone();
            let _ = driver.execute_streaming(&exec_sql, p, tx).await;
        });

        while let Ok(event) = rx.try_recv() {
            match event {
                StreamEvent::SchemaReady(columns) => {
                    if let Ok(new_p) = QueryResultPager::new(columns) {
                        final_pager = Arc::new(new_p);
                    }
                }
                StreamEvent::Completed { total_rows, elapsed_ms } => {
                    self.status_message = format!("Fetched {} rows in {}ms (120 FPS Zero-Copy)", total_rows, elapsed_ms);
                }
                StreamEvent::Error(err) => {
                    self.status_message = format!("SQL Error: {}", err);
                }
                _ => {}
            }
        }

        // 2. Stream rows into typed pager
        let p_final = final_pager.clone();
        let (tx2, _rx2) = flume::unbounded();
        rt.block_on(async {
            let _ = driver.execute_streaming(&sql, p_final, tx2).await;
        });

        let elapsed = start_time.elapsed().as_millis() as u64;
        let tab = &mut self.tabs[self.active_tab];
        tab.execution_time_ms = Some(elapsed);
        tab.grid.set_pager(final_pager);
        tab.grid.set_table_name("result");

        self.is_running_query = false;
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
                    this.run_active_query();
                    cx.notify();
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
                                    this.run_active_query();
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
                // Left Sidebar
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(theme.sidebar_width))
                        .h_full()
                        .bg(theme.bg_sidebar)
                        .border_r_1()
                        .border_color(theme.border)
                        .p_3()
                        .gap_3()
                        // "New Connection" Action Button
                        .child(
                            div()
                                .id("btn-open-connect-dialog")
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .py_1p5()
                                .rounded_md()
                                .bg(theme.bg_surface_selected)
                                .border_1()
                                .border_color(theme.border)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_accent)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_connection_modal = true;
                                    cx.notify();
                                }))
                                .child("➕ Connect to Database")
                        )
                        // Active Connection Status Card
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .p_2()
                                .rounded_md()
                                .bg(theme.bg_surface)
                                .border_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(10.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text_muted)
                                                .child("CURRENT DATABASE")
                                        )
                                        .child(if self.is_connected {
                                            div()
                                                .id("btn-disconnect")
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
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .text_size(px(12.0))
                                        .child(if self.is_connected {
                                            let icon = match self.selected_driver_kind {
                                                DbDriverKind::Postgres => "🐘 PostgreSQL",
                                                DbDriverKind::MySql => "🐬 MySQL",
                                                DbDriverKind::Sqlite => "📁 SQLite",
                                            };
                                            format!("{} ({})", icon, self.form_database)
                                        } else {
                                            "⚪ No active connection".to_string()
                                        })
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.text_muted)
                                        .child(if self.is_connected {
                                            if self.selected_driver_kind == DbDriverKind::Sqlite {
                                                format!("File: {}", self.form_database)
                                            } else {
                                                format!("{}:{}", self.form_host, self.form_port)
                                            }
                                        } else {
                                            "Click Connect above to configure".to_string()
                                        })
                                )
                        )
                        // Tables List (from real DB)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .gap_1()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(10.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text_muted)
                                                .child("TABLES & VIEWS")
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.0))
                                                .text_color(theme.text_muted)
                                                .child(format!("{} items", self.tables.len()))
                                        )
                                )
                                .child({
                                    let mut tables_scroll = div().flex().flex_col().gap_1().overflow_hidden();
                                    if self.tables.is_empty() {
                                        tables_scroll = tables_scroll.child(
                                            div()
                                                .p_3()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child("No tables found. Connect to a database to load schema.")
                                        );
                                    } else {
                                        for (i, t) in self.tables.iter().enumerate() {
                                            let table_schema = t.schema.clone();
                                            let table_name = t.name.clone();
                                            let item_id = format!("sidebar-table-item-{}", i);
                                            tables_scroll = tables_scroll.child(
                                                div()
                                                    .id(ElementId::Name(item_id.into()))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .p_1()
                                                    .px_2()
                                                    .rounded_sm()
                                                    .bg(theme.bg_surface)
                                                    .text_size(px(12.0))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.query_table(&table_schema, &table_name);
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .child("📋")
                                                            .child(t.name.clone())
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(9.0))
                                                            .text_color(theme.text_muted)
                                                            .child(t.table_type.clone())
                                                    )
                                            );
                                        }
                                    }
                                    tables_scroll
                                })
                        )
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
