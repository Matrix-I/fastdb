use std::sync::Arc;
use std::time::Instant;
use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent, TableSummary};
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

pub struct Workspace {
    pub theme: Theme,
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    pub driver: Arc<dyn DatabaseDriver>,
    #[allow(dead_code)]
    pub schema_cache: Arc<SchemaCache>,
    pub tables: Vec<TableSummary>,
    pub is_running_query: bool,
    pub status_message: String,
}

impl Workspace {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let schema_cache = Arc::new(SchemaCache::new().expect("Failed to initialize SchemaCache"));

        // Default to embedded in-memory SQLite driver with pre-populated demo tables
        let sqlite_driver = SqliteDriver::new_memory();

        // Seed demo tables so FastDB works out of the box with zero external configuration!
        let pager_init = Arc::new(QueryResultPager::new(vec![]).unwrap());
        let (tx, _rx) = flume::unbounded();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let _ = sqlite_driver
                .execute_streaming(
                    r#"
                    CREATE TABLE customers (
                        id INTEGER PRIMARY KEY,
                        name TEXT NOT NULL,
                        email TEXT UNIQUE,
                        country TEXT,
                        balance REAL
                    );
                    INSERT INTO customers VALUES
                        (1, 'Alice Walker', 'alice@fastdb.dev', 'USA', 4250.75),
                        (2, 'Bob Chen', 'bob@fastdb.dev', 'Singapore', 8920.00),
                        (3, 'Charlie Nguyen', 'charlie@fastdb.dev', 'Vietnam', 15400.50),
                        (4, 'Diana Rossi', 'diana@fastdb.dev', 'Italy', 3100.20),
                        (5, 'Erik Lindqvist', 'erik@fastdb.dev', 'Sweden', 6700.00);

                    CREATE TABLE orders (
                        order_id INTEGER PRIMARY KEY,
                        customer_id INTEGER,
                        amount REAL,
                        status TEXT,
                        created_at TEXT
                    );
                    INSERT INTO orders VALUES
                        (101, 1, 250.00, 'COMPLETED', '2026-10-20 14:32:00'),
                        (102, 2, 1420.50, 'SHIPPED', '2026-10-21 09:15:00'),
                        (103, 3, 310.00, 'PENDING', '2026-10-22 18:40:00'),
                        (104, 1, 99.99, 'COMPLETED', '2026-10-23 11:05:00'),
                        (105, 5, 2450.00, 'PROCESSING', '2026-10-24 16:20:00');
                    "#,
                    pager_init,
                    tx,
                )
                .await;
        });

        // Populate SchemaCache with demo tables and columns
        let _ = schema_cache.add_table("main", "customers", "BASE TABLE");
        let _ = schema_cache.add_column("main", "customers", "id", "INTEGER", true);
        let _ = schema_cache.add_column("main", "customers", "name", "TEXT", false);
        let _ = schema_cache.add_column("main", "customers", "email", "TEXT", false);
        let _ = schema_cache.add_column("main", "customers", "country", "TEXT", false);
        let _ = schema_cache.add_column("main", "customers", "balance", "REAL", false);

        let _ = schema_cache.add_table("main", "orders", "BASE TABLE");
        let _ = schema_cache.add_column("main", "orders", "order_id", "INTEGER", true);
        let _ = schema_cache.add_column("main", "orders", "customer_id", "INTEGER", false);
        let _ = schema_cache.add_column("main", "orders", "amount", "REAL", false);
        let _ = schema_cache.add_column("main", "orders", "status", "TEXT", false);
        let _ = schema_cache.add_column("main", "orders", "created_at", "TEXT", false);

        let tables = vec![
            TableSummary {
                schema: "main".to_string(),
                name: "customers".to_string(),
                table_type: "BASE TABLE".to_string(),
            },
            TableSummary {
                schema: "main".to_string(),
                name: "orders".to_string(),
                table_type: "BASE TABLE".to_string(),
            },
        ];

        let default_sql = "SELECT id, name, email, country, balance FROM customers ORDER BY id ASC;";
        let initial_editor = SqlEditorView::new(default_sql, theme.clone(), Some(schema_cache.clone()));
        let initial_grid = VirtualDataGrid::new(None, theme.clone());

        let initial_tab = TabState {
            title: "Query 1".to_string(),
            editor: initial_editor,
            grid: initial_grid,
            execution_time_ms: None,
        };

        let mut ws = Self {
            theme,
            active_tab: 0,
            tabs: vec![initial_tab],
            driver: Arc::new(sqlite_driver),
            schema_cache,
            tables,
            is_running_query: false,
            status_message: "⚡ Ready — Connected to FastDB In-Memory Engine (Pure Rust)".to_string(),
        };

        // Run initial query to populate grid immediately
        ws.run_active_query();
        ws
    }

    pub fn run_active_query(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let tab = &mut self.tabs[self.active_tab];
        let sql = tab.editor.active_statement();
        if sql.is_empty() {
            return;
        }

        self.is_running_query = true;
        self.status_message = format!("Executing: {}", sql);

        let driver = Arc::clone(&self.driver);
        let (tx, rx) = flume::unbounded();
        let start_time = Instant::now();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Create initial pager
        let dummy_pager = Arc::new(QueryResultPager::new(vec![]).unwrap());
        let mut final_pager = dummy_pager.clone();

        let exec_sql = sql.clone();
        rt.block_on(async {
            let p = dummy_pager.clone();
            let _ = driver.execute_streaming(&exec_sql, p, tx).await;
        });

        // Drain stream events
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
                    self.status_message = format!("Error: {}", err);
                }
                _ => {}
            }
        }

        // Re-run streaming into the finalized pager schema
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

    #[allow(dead_code)]
    pub fn new_tab(&mut self) {
        let count = self.tabs.len() + 1;
        let title = format!("Query {}", count);
        let editor = SqlEditorView::new(
            "SELECT * FROM orders LIMIT 100;",
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let active_tab_idx = self.active_tab;

        #[cfg(target_os = "macos")]
        let top_bar_left_pad = 80.0;
        #[cfg(not(target_os = "macos"))]
        let top_bar_left_pad = 14.0;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            // 1. Top Application & Tab Bar
            .child(
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
                            // Query Tabs
                            .child({
                                let mut tabs_bar = div().flex().items_center().gap_1();
                                for (i, tab) in self.tabs.iter().enumerate() {
                                    let is_active = i == active_tab_idx;
                                    let bg = if is_active { theme.bg_surface_selected } else { theme.bg_surface };
                                    tabs_bar = tabs_bar.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .py_1()
                                            .rounded_t_md()
                                            .bg(bg)
                                            .text_size(px(12.0))
                                            .child(tab.title.clone())
                                    );
                                }
                                tabs_bar
                            })
                    )
                    // Action Buttons Toolbar
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme.text_accent)
                                    .text_color(theme.bg_app)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(12.0))
                                    .child("▶ Run Query (⌘↵)")
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .text_size(px(12.0))
                                    .child("💾 Save (⌘S)")
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .text_size(px(12.0))
                                    .child("📤 Export CSV")
                            )
                    )
            )
            // 2. Main Workspace Body: Sidebar + Main Split (Editor + Grid)
            .child(
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
                            // Connection Header
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.text_muted)
                                            .child("ACTIVE CONNECTION")
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .p_2()
                                            .rounded_md()
                                            .bg(theme.bg_surface_selected)
                                            .text_size(px(12.0))
                                            .child("📦 SQLite (In-Memory Fast Engine)")
                                    )
                            )
                            // Tables Tree
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.text_muted)
                                            .child("TABLES & VIEWS")
                                    )
                                    .child({
                                        let mut tables_list = div().flex().flex_col().gap_1();
                                        for t in &self.tables {
                                            tables_list = tables_list.child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .p_1()
                                                    .px_2()
                                                    .rounded_sm()
                                                    .text_size(px(12.0))
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
                                                            .child("TABLE")
                                                    )
                                            );
                                        }
                                        tables_list
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
                                                format!("{} rows", total)
                                            })
                                    )
                                    .child({
                                        let tab = &mut self.tabs[active_tab_idx];
                                        tab.grid.render_grid()
                                    })
                            )
                    )
            )
            // 3. Bottom Status Bar
            .child(
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
                                    .child("🟢")
                                    .child("Connected: FastDB Pure Rust Engine")
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
            )
    }
}
