use super::*;
impl Workspace {
    pub(super) fn append_connection_node(
        &self,
        mut nav_tree: Stateful<Div>,
        conn: &SavedConnection,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;

        let conn_id = conn.id.clone();
        let is_this_active = self.connections.active_connection_id.as_deref() == Some(&conn_id);
        let is_this_connected = self.is_connection_live(&conn_id);
        let read_only = self
            .connections
            .session(&conn_id)
            .driver
            .as_ref()
            .map_or(conn.read_only, |driver| driver.is_read_only());
        let is_this_connecting = self.form.is_connecting && is_this_active;
        // Only the active connection's tree is loaded into the navigator
        let filtering = self.navigator_relation_filter_active();
        let is_conn_exp = (filtering || self.connections.navigator().nav_connection_expanded)
            && self.connections.active().driver.is_some()
            && is_this_active;

        let drv_name = conn.driver.clone();
        let conn_name = conn.name.clone();
        let conn_addr = if conn.driver == "Sqlite" {
            conn.database.clone()
        } else {
            format!("{}:{}", conn.host, conn.port)
        };

        let conn_id_for_click = conn_id.clone();
        let conn_id_for_chev = conn_id.clone();
        let conn_id_for_menu = conn_id.clone();

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
                .bg(if is_this_connected && is_this_active {
                    theme.bg_surface_selected
                } else {
                    rgba(0x00000000)
                })
                .hover(|s| s.bg(theme.bg_surface_hover))
                .cursor_pointer()
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 {
                        this.toggle_or_connect(&conn_id_for_click, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_nav_context_menu(&conn_id_for_menu, event.position);
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(12.0))
                        .child(nav_chevron(
                            format!("nav-chev-conn-{}", conn_id),
                            if is_this_connecting {
                                "⏳"
                            } else if is_conn_exp {
                                "▾"
                            } else {
                                "▸"
                            },
                            12.0,
                            10.0,
                            if is_this_connecting {
                                theme.text_warning
                            } else {
                                theme.text_muted
                            },
                            theme.text_primary,
                            cx,
                            move |this, cx| this.toggle_or_connect(&conn_id_for_chev, cx),
                        ))
                        .child(dbeaver_driver_icon(&drv_name, is_this_connected))
                        .when(read_only, |row| {
                            row.child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_warning)
                                    .child("Read-only"),
                            )
                        })
                        .children(conn.appearance.color.map(|color| {
                            div()
                                .size(px(8.0))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(color))
                        }))
                        .child(
                            div()
                                .font_weight(if is_this_connected {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_this_connected {
                                    theme.text_accent
                                } else {
                                    theme.text_primary
                                })
                                .child(conn_name),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(theme.text_muted)
                                .child(format!(" {}", conn_addr)),
                        )
                        .child(if is_this_connecting {
                            div()
                                .text_size(px(10.0))
                                .text_color(theme.text_warning)
                                .child("(Connecting...)")
                        } else if !is_this_connected {
                            div()
                                .text_size(px(10.0))
                                .text_color(theme.text_muted)
                                .child("(Double-click to connect)")
                        } else {
                            div()
                        }),
                ),
        );

        if is_conn_exp {
            // Level 1: "Databases" Folder
            let is_dbs_exp = filtering || self.connections.navigator().nav_databases_expanded;
            nav_tree = nav_tree.child(
                div()
                    .id("nav-databases-folder")
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .pl(px(14.0))
                    .rounded_sm()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.5))
                            .child(nav_chevron(
                                "nav-chev-databases",
                                if is_dbs_exp { "▾" } else { "▸" },
                                12.0,
                                10.0,
                                theme.text_muted,
                                theme.text_primary,
                                cx,
                                |this, _| {
                                    this.connections.navigator_mut().nav_databases_expanded =
                                        !this.connections.navigator_mut().nav_databases_expanded
                                },
                            ))
                            .child(img("icons/folder_database.svg").w(px(16.0)).h(px(16.0)))
                            .child("Databases"),
                    ),
            );

            if is_dbs_exp {
                // Level 2: Target Database
                let is_db_exp = filtering || self.connections.navigator().nav_database_expanded;
                nav_tree = nav_tree.child(
                    div()
                        .id("nav-database-node")
                        .flex()
                        .items_center()
                        .h(px(22.0))
                        .pl(px(26.0))
                        .rounded_sm()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .text_size(px(11.5))
                                .child(nav_chevron(
                                    "nav-chev-database",
                                    if is_db_exp { "▾" } else { "▸" },
                                    12.0,
                                    10.0,
                                    theme.text_muted,
                                    theme.text_primary,
                                    cx,
                                    |this, _| {
                                        this.connections.navigator_mut().nav_database_expanded =
                                            !this.connections.navigator_mut().nav_database_expanded
                                    },
                                ))
                                .child(img("icons/database.svg").w(px(16.0)).h(px(16.0)))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_accent)
                                        .child(conn.database.clone()),
                                ),
                        ),
                );

                if is_db_exp {
                    // Level 3: "Schemas" Folder
                    let is_schemas_exp =
                        filtering || self.connections.navigator().nav_schemas_expanded;
                    nav_tree = nav_tree.child(
                        div()
                            .id("nav-schemas-folder")
                            .flex()
                            .items_center()
                            .h(px(22.0))
                            .pl(px(38.0))
                            .rounded_sm()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .text_size(px(11.5))
                                    .child(nav_chevron(
                                        "nav-chev-schemas",
                                        if is_schemas_exp { "▾" } else { "▸" },
                                        12.0,
                                        10.0,
                                        theme.text_muted,
                                        theme.text_primary,
                                        cx,
                                        |this, _| {
                                            this.connections.navigator_mut().nav_schemas_expanded =
                                                !this
                                                    .connections
                                                    .navigator_mut()
                                                    .nav_schemas_expanded
                                        },
                                    ))
                                    .child(img("icons/folder_schema.svg").w(px(16.0)).h(px(16.0)))
                                    .child("Schemas"),
                            ),
                    );

                    if is_schemas_exp {
                        let schema_map = self.connections.active().schema_tree.clone();

                        if schema_map.is_empty() {
                            nav_tree = nav_tree.child(
                                div()
                                    .pl(px(50.0))
                                    .py_1()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("(No schemas found)"),
                            );
                        }

                        for (schema_name, s_tables) in schema_map.iter() {
                            if filtering
                                && !s_tables.iter().any(|table| {
                                    self.navigator_relation_matches(schema_name, &table.name)
                                })
                            {
                                continue;
                            }
                            nav_tree = self.append_schema_node(nav_tree, schema_name, s_tables, cx);
                        }
                    }
                }
            }
        }

        nav_tree
    }
}
