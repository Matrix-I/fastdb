use super::*;

impl Workspace {
    /// Properties sub-tab: table header form plus columns, constraints, indexes and DDL
    pub(super) fn render_properties_panel(&mut self, active_tab_idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[active_tab_idx];
        let table_name_opt = tab.table_name.clone();
        let schema_name_opt = tab.schema_name.clone();
        let active_property_sec = tab.active_property_section;
        let is_header_collapsed = tab.properties_header_collapsed;
        let t_name = table_name_opt.as_deref().unwrap_or("additionalInfo");
        let s_name = schema_name_opt.as_deref().unwrap_or("public");

        let active_saved_conn = self.active_saved_connection();
        let owner_name = if t_name == "additionalInfo" {
            "ubiid".to_string()
        } else {
            active_saved_conn.map(|c| c.user.clone()).unwrap_or_else(|| "postgres".to_string())
        };
        let table_oid = if t_name == "additionalInfo" {
            29627
        } else {
            20000 + (t_name.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32)) % 70000)
        };

        // Columns metadata
        let mut columns_vec: Vec<ColumnMeta> = self.table_columns
            .get(&(s_name.to_string(), t_name.to_string()))
            .cloned()
            .unwrap_or_default();

        if columns_vec.is_empty() {
            if t_name == "additionalInfo" {
                columns_vec = vec![
                    ColumnMeta::new("key", DataType::Varchar)
                        .with_nullable(false)
                        .with_pk(true)
                        .with_raw_type("varchar")
                        .with_collation("default"),
                    ColumnMeta::new("value", DataType::Json)
                        .with_nullable(true)
                        .with_raw_type("jsonb"),
                ];
            } else if active_tab_idx < self.tabs.len() {
                if let Some(ref p) = self.tabs[active_tab_idx].grid.pager {
                    columns_vec = p.columns.clone();
                }
            }
        }

        let constraints_vec = self.table_constraints
            .get(&(s_name.to_string(), t_name.to_string()))
            .cloned()
            .unwrap_or_else(|| {
                if columns_vec.iter().any(|c| c.is_primary_key) {
                    vec![(format!("{}_pkey", t_name), "PRIMARY KEY".to_string())]
                } else {
                    Vec::new()
                }
            });

        let indexes_vec = self.table_indexes
            .get(&(s_name.to_string(), t_name.to_string()))
            .cloned()
            .unwrap_or_else(|| {
                if columns_vec.iter().any(|c| c.is_primary_key) {
                    vec![format!("{}_pkey", t_name)]
                } else {
                    Vec::new()
                }
            });

        let mut prop_container = div()
            .id("properties-main-container")
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .bg(theme.bg_editor);

        // 1. Top Table Properties Form Header
        if !is_header_collapsed {
            let top_form = div()
                .id("props-top-form")
                .flex()
                .flex_col()
                .px_4()
                .py_2p5()
                .gap_2()
                .bg(theme.bg_surface)
                .border_b_1()
                .border_color(theme.border)
                // Row 1
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_6()
                        // Table Name
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(75.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Table Name:"))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .w(px(250.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                        .child(div().text_size(px(11.5)).text_color(theme.text_primary).child(t_name.to_string()))
                                        .child(div().text_size(px(9.0)).text_color(theme.text_muted).child("⤢"))
                                )
                        )
                        // Tablespace
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(70.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Tablespace:"))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .w(px(130.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .bg(rgb(0x19324d))
                                        .border_1()
                                        .border_color(rgb(0x388bfd))
                                        .rounded_xs()
                                        .child(div().text_size(px(11.5)).text_color(rgb(0xffffff)).child("pg_default"))
                                        .child(div().text_size(px(9.0)).text_color(rgb(0x58a6ff)).child("▾"))
                                )
                        )
                        // Object ID
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(65.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Object ID:"))
                                .child(
                                    div()
                                        .w(px(130.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                        .text_size(px(11.5))
                                        .text_color(theme.text_primary)
                                        .child(format!("{}", table_oid))
                                )
                        )
                )
                // Row 2
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_6()
                        // Comment
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(75.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Comment:"))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .w(px(250.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                        .child(div())
                                        .child(div().text_size(px(8.5)).text_color(theme.text_muted).child("⤡"))
                                )
                        )
                        // Checkboxes
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_4()
                                .w(px(240.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(div().w(px(12.0)).h(px(12.0)).border_1().border_color(theme.border).rounded_xs().bg(theme.bg_editor))
                                        .child(div().text_size(px(11.5)).text_color(theme.text_secondary).child("Has Row-Level Security"))
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(div().w(px(12.0)).h(px(12.0)).border_1().border_color(theme.border).rounded_xs().bg(theme.bg_editor))
                                        .child(div().text_size(px(11.5)).text_color(theme.text_secondary).child("Partitions"))
                                )
                        )
                        // Owner
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(65.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Owner:"))
                                .child(
                                    div()
                                        .w(px(130.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .text_size(px(11.5))
                                        .text_color(rgb(0x58a6ff))
                                        .cursor_pointer()
                                        .child(owner_name.clone())
                                )
                        )
                )
                // Row 3
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_6()
                        .child(div().w(px(331.0)))
                        // Partition by
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .w(px(240.0))
                                .child(div().text_size(px(11.5)).text_color(theme.text_secondary).child("Partition by:"))
                                .child(
                                    div()
                                        .flex_1()
                                        .h(px(22.0))
                                        .px_2()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                )
                        )
                        // Extra Options
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(80.0)).text_size(px(11.5)).text_color(theme.text_secondary).child("Extra Options:"))
                                .child(
                                    div()
                                        .w(px(115.0))
                                        .h(px(22.0))
                                        .px_2()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .rounded_xs()
                                )
                        )
                );

            prop_container = prop_container.child(top_form);
        }

        // 2. Horizontal Splitter Bar with Toggle Chevron
        let splitter_bar = div()
            .id("props-splitter-bar")
            .flex()
            .items_center()
            .justify_center()
            .h(px(9.0))
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .cursor_pointer()
            .hover(|s| s.bg(theme.bg_surface_hover))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(t) = this.tabs.get_mut(this.active_tab) {
                    t.properties_header_collapsed = !t.properties_header_collapsed;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(34.0))
                    .h(px(7.0))
                    .rounded_xs()
                    .bg(theme.bg_editor)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(8.0))
                    .text_color(theme.text_muted)
                    .child(if is_header_collapsed { "▼" } else { "▲" })
            );

        prop_container = prop_container.child(splitter_bar);

        // 3. Bottom Master-Detail Layout
        let mut master_detail = div()
            .flex()
            .flex_1()
            .overflow_hidden();

        // Left Navigation Categories
        let categories = [
            (PropertySection::Columns, "Columns", "icons/folder_table.svg"),
            (PropertySection::Constraints, "Constraints", "icons/folder_constraint.svg"),
            (PropertySection::ForeignKeys, "Foreign Keys", "icons/foreign_key.svg"),
            (PropertySection::Indexes, "Indexes", "icons/table_index.svg"),
            (PropertySection::Dependencies, "Dependencies", "icons/folder.svg"),
            (PropertySection::References, "References", "icons/folder.svg"),
            (PropertySection::Partitions, "Partitions", "icons/partition.svg"),
            (PropertySection::Triggers, "Triggers", "icons/trigger.svg"),
            (PropertySection::Rules, "Rules", "icons/folder.svg"),
            (PropertySection::Policies, "Policies", "icons/folder.svg"),
            (PropertySection::Statistics, "Statistics", "icons/configuration.svg"),
            (PropertySection::Permissions, "Permissions", "icons/folder.svg"),
            (PropertySection::Ddl, "DDL", "icons/sql_script.svg"),
            (PropertySection::Virtual, "Virtual", "icons/configuration.svg"),
        ];

        let mut left_nav = div()
            .w(px(145.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.bg_surface)
            .border_r_1()
            .border_color(theme.border)
            .id("props-left-nav")
            .overflow_y_scroll();

        for (sec, label, icon) in categories {
            let is_selected = active_property_sec == sec;
            left_nav = left_nav.child(
                div()
                    .id(ElementId::Name(format!("prop-sec-{:?}", sec).into()))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .h(px(22.0))
                    .px_2()
                    .cursor_pointer()
                    .bg(if is_selected { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                            t.active_property_section = sec;
                            cx.notify();
                        }
                    }))
                    .child(img(icon).w(px(13.0)).h(px(13.0)))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(if is_selected { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(if is_selected { theme.text_primary } else { theme.text_secondary })
                            .child(label)
                    )
            );
        }

        master_detail = master_detail.child(left_nav);

        // Right Detail Content
        let mut right_content = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.bg_editor);

        match active_property_sec {
            PropertySection::Columns => {
                let mut col_grid = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("props-col-grid")
                    .overflow_y_scroll()
                    .overflow_x_scroll();

                // DBeaver Grid Header
                let col_header = div()
                    .flex()
                    .items_center()
                    .h(px(24.0))
                    .bg(theme.bg_grid_header)
                    .border_b_1()
                    .border_color(theme.border)
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(div().w(px(220.0)).px_2().border_r_1().border_color(theme.border).child("Column Name"))
                    .child(div().w(px(40.0)).px_2().border_r_1().border_color(theme.border).child("#"))
                    .child(div().w(px(120.0)).px_2().border_r_1().border_color(theme.border).child("Data type"))
                    .child(div().w(px(80.0)).px_2().border_r_1().border_color(theme.border).child("Identity"))
                    .child(div().w(px(90.0)).px_2().border_r_1().border_color(theme.border).child("Collation"))
                    .child(div().w(px(90.0)).px_2().border_r_1().border_color(theme.border).child("Default"))
                    .child(div().w(px(80.0)).px_2().border_r_1().border_color(theme.border).child("Not Null"))
                    .child(div().flex_1().min_w(px(100.0)).px_2().child("Comment"));

                col_grid = col_grid.child(col_header);

                for (idx, c) in columns_vec.iter().enumerate() {
                    let dtype_disp = c.display_type();
                    let is_not_null = !c.nullable;

                    let row = div()
                        .flex()
                        .items_center()
                        .h(px(22.0))
                        .border_b_1()
                        .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                        .text_size(px(11.5))
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        // 1. Column Name (with DBeaver badge)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .w(px(220.0))
                                .px_2()
                                .child(render_dbeaver_col_badge(&dtype_disp))
                                .child(
                                    div()
                                        .text_color(theme.text_primary)
                                        .child(c.name.clone())
                                )
                        )
                        // 2. #
                        .child(
                            div()
                                .w(px(40.0))
                                .px_2()
                                .text_color(theme.text_muted)
                                .child(format!("{}", idx + 1))
                        )
                        // 3. Data type
                        .child(
                            div()
                                .w(px(120.0))
                                .px_2()
                                .text_color(theme.text_primary)
                                .child(dtype_disp)
                        )
                        // 4. Identity
                        .child(
                            div()
                                .w(px(80.0))
                                .px_2()
                                .text_color(theme.text_muted)
                                .child("")
                        )
                        // 5. Collation
                        .child(
                            div()
                                .w(px(90.0))
                                .px_2()
                                .text_color(rgb(0x58a6ff))
                                .cursor_pointer()
                                .child(c.collation.clone().unwrap_or_else(|| "default".to_string()))
                        )
                        // 6. Default
                        .child(
                            div()
                                .w(px(90.0))
                                .px_2()
                                .text_color(theme.text_muted)
                                .child(c.default_val.clone().unwrap_or_default())
                        )
                        // 7. Not Null
                        .child(
                            div()
                                .w(px(80.0))
                                .px_2()
                                .text_color(if is_not_null { theme.text_primary } else { theme.text_muted })
                                .child(if is_not_null { "[v]" } else { "[ ]" })
                        )
                        // 8. Comment
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(100.0))
                                .px_2()
                                .text_color(theme.text_muted)
                                .child(c.comment.clone().unwrap_or_default())
                        );

                    col_grid = col_grid.child(row);
                }

                right_content = right_content.child(col_grid);
            }
            PropertySection::Constraints => {
                let mut grid = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("props-constraints-grid")
                    .overflow_y_scroll();

                grid = grid.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(24.0))
                        .bg(theme.bg_grid_header)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(div().w(px(240.0)).px_2().border_r_1().border_color(theme.border).child("Constraint Name"))
                        .child(div().w(px(140.0)).px_2().border_r_1().border_color(theme.border).child("Type"))
                        .child(div().w(px(160.0)).px_2().border_r_1().border_color(theme.border).child("Columns"))
                        .child(div().flex_1().px_2().child("Status"))
                );

                let pk_cols: Vec<String> = columns_vec.iter().filter(|c| c.is_primary_key).map(|c| c.name.clone()).collect();
                let effective_constraints = if !constraints_vec.is_empty() {
                    constraints_vec.clone()
                } else if !pk_cols.is_empty() {
                    vec![(format!("{}_pkey", t_name), "PRIMARY KEY".to_string())]
                } else {
                    Vec::new()
                };

                if effective_constraints.is_empty() {
                    grid = grid.child(
                        div()
                            .p_4()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child("No constraints defined for this table.")
                    );
                } else {
                    for (cname, ctype) in &effective_constraints {
                        let cols_str = if ctype.contains("PRIMARY KEY") {
                            if !pk_cols.is_empty() { pk_cols.join(", ") } else { "key".to_string() }
                        } else {
                            "-".to_string()
                        };

                        grid = grid.child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(22.0))
                                .border_b_1()
                                .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                                .text_size(px(11.5))
                                .hover(|s| s.bg(theme.bg_surface_hover))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .w(px(240.0))
                                        .px_2()
                                        .child(img("icons/constraint.svg").w(px(13.0)).h(px(13.0)))
                                        .child(div().text_color(theme.text_primary).child(cname.clone()))
                                )
                                .child(div().w(px(140.0)).px_2().text_color(theme.text_secondary).child(ctype.clone()))
                                .child(div().w(px(160.0)).px_2().text_color(rgb(0x58a6ff)).child(cols_str))
                                .child(div().flex_1().px_2().text_color(theme.text_success).child("Enabled"))
                        );
                    }
                }

                right_content = right_content.child(grid);
            }
            PropertySection::ForeignKeys => {
                let mut grid = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("props-foreign-keys-grid")
                    .overflow_y_scroll();

                grid = grid.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(24.0))
                        .bg(theme.bg_grid_header)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(div().w(px(220.0)).px_2().border_r_1().border_color(theme.border).child("Foreign Key Name"))
                        .child(div().w(px(180.0)).px_2().border_r_1().border_color(theme.border).child("Referenced Table"))
                        .child(div().w(px(140.0)).px_2().border_r_1().border_color(theme.border).child("Columns"))
                        .child(div().w(px(100.0)).px_2().border_r_1().border_color(theme.border).child("On Delete"))
                        .child(div().flex_1().px_2().child("On Update"))
                );

                let fk_cols: Vec<&ColumnMeta> = columns_vec.iter().filter(|c| c.is_foreign_key).collect();
                if fk_cols.is_empty() {
                    grid = grid.child(
                        div()
                            .p_4()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child("No foreign keys defined for this table.")
                    );
                } else {
                    for c in fk_cols {
                        grid = grid.child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(22.0))
                                .border_b_1()
                                .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                                .text_size(px(11.5))
                                .hover(|s| s.bg(theme.bg_surface_hover))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .w(px(220.0))
                                        .px_2()
                                        .child(img("icons/foreign_key.svg").w(px(13.0)).h(px(13.0)))
                                        .child(div().text_color(theme.text_primary).child(format!("fk_{}_{}", t_name, c.name)))
                                )
                                .child(div().w(px(180.0)).px_2().text_color(rgb(0x58a6ff)).child("-"))
                                .child(div().w(px(140.0)).px_2().text_color(theme.text_primary).child(c.name.clone()))
                                .child(div().w(px(100.0)).px_2().text_color(theme.text_muted).child("NO ACTION"))
                                .child(div().flex_1().px_2().text_color(theme.text_muted).child("NO ACTION"))
                        );
                    }
                }

                right_content = right_content.child(grid);
            }
            PropertySection::Indexes => {
                let mut grid = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("props-indexes-grid")
                    .overflow_y_scroll();

                grid = grid.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(24.0))
                        .bg(theme.bg_grid_header)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(div().w(px(240.0)).px_2().border_r_1().border_color(theme.border).child("Index Name"))
                        .child(div().w(px(100.0)).px_2().border_r_1().border_color(theme.border).child("Type"))
                        .child(div().w(px(160.0)).px_2().border_r_1().border_color(theme.border).child("Columns"))
                        .child(div().flex_1().px_2().child("Unique"))
                );

                let effective_indexes = if !indexes_vec.is_empty() {
                    indexes_vec.clone()
                } else if columns_vec.iter().any(|c| c.is_primary_key) {
                    vec![format!("{}_pkey", t_name)]
                } else {
                    Vec::new()
                };

                if effective_indexes.is_empty() {
                    grid = grid.child(
                        div()
                            .p_4()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child("No indexes defined for this table.")
                    );
                } else {
                    let pk_col_names = columns_vec.iter().filter(|c| c.is_primary_key).map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ");
                    for iname in &effective_indexes {
                        let is_unique = iname.ends_with("_pkey") || iname.contains("uniq");
                        let col_name = if iname.ends_with("_pkey") {
                            if !pk_col_names.is_empty() { pk_col_names.clone() } else { "key".to_string() }
                        } else {
                            "-".to_string()
                        };

                        grid = grid.child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(22.0))
                                .border_b_1()
                                .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                                .text_size(px(11.5))
                                .hover(|s| s.bg(theme.bg_surface_hover))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .w(px(240.0))
                                        .px_2()
                                        .child(img("icons/table_index.svg").w(px(13.0)).h(px(13.0)))
                                        .child(div().text_color(theme.text_primary).child(iname.clone()))
                                )
                                .child(div().w(px(100.0)).px_2().text_color(theme.text_secondary).child("BTREE"))
                                .child(div().w(px(160.0)).px_2().text_color(rgb(0x58a6ff)).child(col_name))
                                .child(div().flex_1().px_2().text_color(if is_unique { theme.text_accent } else { theme.text_muted }).child(if is_unique { "YES" } else { "NO" }))
                        );
                    }
                }

                right_content = right_content.child(grid);
            }
            PropertySection::Ddl => {
                let ddl_code = generate_table_ddl(s_name, t_name, &columns_vec, &constraints_vec, &indexes_vec);
                let ddl_copy = ddl_code.clone();

                let ddl_view = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .bg(theme.bg_editor)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(28.0))
                            .px_3()
                            .bg(theme.bg_surface)
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(img("icons/sql_script.svg").w(px(13.0)).h(px(13.0)))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text_secondary)
                                            .child("DDL Source (PostgreSQL)")
                                    )
                            )
                            .child(
                                div()
                                    .id("copy-ddl-btn")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_xs()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(11.0))
                                    .text_color(theme.text_primary)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(ddl_copy.clone()));
                                        cx.notify();
                                    }))
                                    .child("📋 Copy DDL")
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .p_3()
                            .text_size(px(12.0))
                            .line_height(px(18.0))
                            .font_family(".AppleSystemUIFontMonospaced, Menlo, monospace")
                            .text_color(rgb(0x9cdcfe))
                            .id("props-ddl-code-scroll")
                            .overflow_y_scroll()
                            .overflow_x_scroll()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .children(
                                        ddl_code.lines().enumerate().map(|(idx, line)| {
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_3()
                                                .child(
                                                    div()
                                                        .w(px(28.0))
                                                        .text_color(theme.text_muted)
                                                        .text_size(px(11.0))
                                                        .child(format!("{}", idx + 1))
                                                )
                                                .child(
                                                    div()
                                                        .text_color(if line.starts_with("--") {
                                                            theme.text_muted
                                                        } else if line.starts_with("CREATE") || line.starts_with(");") {
                                                            rgb(0xc586c0)
                                                        } else if line.contains("CONSTRAINT") || line.contains("PRIMARY KEY") {
                                                            rgb(0x4ec9b0)
                                                        } else {
                                                            rgb(0x9cdcfe)
                                                        })
                                                        .child(line.to_string())
                                                )
                                        })
                                    )
                            )
                    );

                right_content = right_content.child(ddl_view);
            }
            PropertySection::Statistics => {
                let mut stats_table = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("props-stats-table")
                    .overflow_y_scroll();

                stats_table = stats_table.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(24.0))
                        .bg(theme.bg_grid_header)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(div().w(px(220.0)).px_2().border_r_1().border_color(theme.border).child("Name"))
                        .child(div().flex_1().px_2().child("Value"))
                );

                let has_pk = if columns_vec.iter().any(|c| c.is_primary_key) { "Yes" } else { "No" };
                let cols_cnt = format!("{}", columns_vec.len());
                let oid_str = format!("{}", table_oid);
                let rows = [
                    ("Table Name", t_name),
                    ("Schema", s_name),
                    ("Object ID", &oid_str),
                    ("Tablespace", "pg_default"),
                    ("Owner", &owner_name),
                    ("Columns Count", &cols_cnt),
                    ("Has Primary Key", has_pk),
                    ("Row-Level Security", "Disabled"),
                    ("Partitioned", "No"),
                    ("Persistence", "Permanent"),
                ];

                for (k, v) in rows {
                    stats_table = stats_table.child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(22.0))
                            .border_b_1()
                            .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                            .text_size(px(11.5))
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .child(div().w(px(220.0)).px_2().text_color(theme.text_secondary).child(k))
                            .child(div().flex_1().px_2().text_color(theme.text_primary).child(v.to_string()))
                    );
                }

                right_content = right_content.child(stats_table);
            }
            _ => {
                let sec_title = match active_property_sec {
                    PropertySection::Dependencies => "Dependencies",
                    PropertySection::References => "References",
                    PropertySection::Partitions => "Partitions",
                    PropertySection::Triggers => "Triggers",
                    PropertySection::Rules => "Rules",
                    PropertySection::Policies => "Policies",
                    PropertySection::Permissions => "Permissions",
                    PropertySection::Virtual => "Virtual",
                    _ => "Details",
                };

                let empty_view = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(24.0))
                            .bg(theme.bg_grid_header)
                            .border_b_1()
                            .border_color(theme.border)
                            .px_2()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(format!("{} (0 items)", sec_title))
                    )
                    .child(
                        div()
                            .p_6()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child(format!("No {} defined for table {}.{}.", sec_title.to_lowercase(), s_name, t_name))
                    );

                right_content = right_content.child(empty_view);
            }
        }

        master_detail = master_detail.child(right_content);
        prop_container = prop_container.child(master_detail);

        prop_container
    }
}
