use super::*;

impl Workspace {
    /// DBeaver-style database navigator tree in the left sidebar
    pub(super) fn render_navigator(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
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
    }
}
