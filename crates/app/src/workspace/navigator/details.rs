use super::*;
impl Workspace {
    pub(super) fn append_table_details(
        &self,
        mut nav_tree: Stateful<Div>,
        table_schema: String,
        table_name: String,
        table_key: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;

        // 1. Columns
        let col_key = format!("{}.columns", table_key);
        let is_col_exp = self
            .connections
            .navigator()
            .nav_subfolder_expanded
            .contains(&col_key);
        let col_key_toggle = col_key.clone();
        let cols_for_table = self
            .connections
            .active()
            .table_columns
            .get(&(table_schema.clone(), table_name.clone()));

        nav_tree = nav_tree.child(
            div()
                .id(ElementId::Name(
                    format!("nav-cat-cols-{}-{}", table_schema, table_name).into(),
                ))
                .flex()
                .items_center()
                .h(px(22.0))
                .pl(px(86.0))
                .pr_2()
                .rounded_sm()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.5))
                        .child(nav_chevron(
                            format!("nav-chev-{}", col_key_toggle),
                            if is_col_exp { "▾" } else { "▸" },
                            10.0,
                            9.0,
                            theme.text_muted,
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                toggle_key(
                                    &mut this.connections.navigator_mut().nav_subfolder_expanded,
                                    &col_key_toggle,
                                )
                            },
                        ))
                        .child(img("icons/columns.svg").w(px(16.0)).h(px(16.0)))
                        .child(div().text_color(theme.text_primary).child("Columns")),
                ),
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
                            .child("(No columns)"),
                    );
                } else {
                    for c in cols {
                        let col_name = c.name.clone();
                        let pick = self.column_open_pick(
                            self.connections
                                .active_connection_id
                                .clone()
                                .unwrap_or_default(),
                            table_schema.clone(),
                            table_name.clone(),
                            cx,
                        );
                        let picked_name = col_name.clone();
                        let is_pk = c.is_primary_key;
                        let type_str = c.data_type.display_name().to_lowercase();
                        let is_nullable = c.nullable;

                        nav_tree = nav_tree.child(
                            div()
                                .id(ElementId::Name(
                                    format!("nav-col-{}-{}-{}", table_schema, table_name, col_name)
                                        .into(),
                                ))
                                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                    pick(picked_name.clone(), event.click_count, window, cx);
                                    cx.stop_propagation();
                                })
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
                                        .child(if is_pk {
                                            img("icons/key.svg")
                                                .w(px(16.0))
                                                .h(px(16.0))
                                                .into_any_element()
                                        } else {
                                            img("icons/column.svg")
                                                .w(px(16.0))
                                                .h(px(16.0))
                                                .into_any_element()
                                        })
                                        .child(
                                            div()
                                                .font_weight(if is_pk {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .text_color(if is_pk {
                                                    theme.text_accent
                                                } else {
                                                    theme.text_primary
                                                })
                                                .child(col_name),
                                        )
                                        .child(
                                            div()
                                                .text_color(theme.text_muted)
                                                .child(format!(": {}", type_str)),
                                        ),
                                )
                                .child(if !is_nullable {
                                    div()
                                        .text_size(px(9.0))
                                        .text_color(theme.text_warning)
                                        .child("NOT NULL")
                                } else {
                                    div()
                                }),
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
                        .child("(No columns)"),
                );
            }
        }

        // 2. Constraints
        let constr_key = format!("{}.constraints", table_key);
        let is_constr_exp = self
            .connections
            .navigator()
            .nav_subfolder_expanded
            .contains(&constr_key);
        let constr_key_toggle = constr_key.clone();
        let constraints_for_table = self
            .connections
            .active()
            .table_constraints
            .get(&(table_schema.clone(), table_name.clone()));

        nav_tree = nav_tree.child(
            div()
                .id(ElementId::Name(
                    format!("nav-cat-constr-{}-{}", table_schema, table_name).into(),
                ))
                .flex()
                .items_center()
                .h(px(22.0))
                .pl(px(86.0))
                .pr_2()
                .rounded_sm()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.5))
                        .child(nav_chevron(
                            format!("nav-chev-{}", constr_key_toggle),
                            if is_constr_exp { "▾" } else { "▸" },
                            10.0,
                            9.0,
                            theme.text_muted,
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                toggle_key(
                                    &mut this.connections.navigator_mut().nav_subfolder_expanded,
                                    &constr_key_toggle,
                                )
                            },
                        ))
                        .child(img("icons/folder_constraint.svg").w(px(16.0)).h(px(16.0)))
                        .child(div().text_color(theme.text_primary).child("Constraints")),
                ),
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
                            .child(
                                div()
                                    .text_color(theme.text_muted)
                                    .child(format!("({})", ctype)),
                            ),
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
                        .child("(No constraints)"),
                );
            }
        }

        // 3. Foreign Keys
        let fk_key = format!("{}.foreign_keys", table_key);
        let is_fk_exp = self
            .connections
            .navigator()
            .nav_subfolder_expanded
            .contains(&fk_key);
        let fk_key_toggle = fk_key.clone();
        let fks_for_table: Vec<(String, String)> = constraints_for_table
            .map(|v| {
                v.iter()
                    .filter(|(_, ctype)| ctype.to_uppercase().contains("FOREIGN"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        nav_tree = nav_tree.child(
            div()
                .id(ElementId::Name(
                    format!("nav-cat-fk-{}-{}", table_schema, table_name).into(),
                ))
                .flex()
                .items_center()
                .h(px(22.0))
                .pl(px(86.0))
                .pr_2()
                .rounded_sm()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.5))
                        .child(nav_chevron(
                            format!("nav-chev-{}", fk_key_toggle),
                            if is_fk_exp { "▾" } else { "▸" },
                            10.0,
                            9.0,
                            theme.text_muted,
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                toggle_key(
                                    &mut this.connections.navigator_mut().nav_subfolder_expanded,
                                    &fk_key_toggle,
                                )
                            },
                        ))
                        .child(img("icons/folder.svg").w(px(16.0)).h(px(16.0)))
                        .child(div().text_color(theme.text_primary).child("Foreign Keys")),
                ),
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
                        .child("(No foreign keys)"),
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
                            .child(div().text_color(theme.text_primary).child(fk_name.clone())),
                    );
                }
            }
        }

        // 4. Indexes
        let idx_key = format!("{}.indexes", table_key);
        let is_idx_exp = self
            .connections
            .navigator()
            .nav_subfolder_expanded
            .contains(&idx_key);
        let idx_key_toggle = idx_key.clone();
        let idxs_for_table = self
            .connections
            .active()
            .table_indexes
            .get(&(table_schema.clone(), table_name.clone()));

        nav_tree = nav_tree.child(
            div()
                .id(ElementId::Name(
                    format!("nav-cat-idx-{}-{}", table_schema, table_name).into(),
                ))
                .flex()
                .items_center()
                .h(px(22.0))
                .pl(px(86.0))
                .pr_2()
                .rounded_sm()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.5))
                        .child(nav_chevron(
                            format!("nav-chev-{}", idx_key_toggle),
                            if is_idx_exp { "▾" } else { "▸" },
                            10.0,
                            9.0,
                            theme.text_muted,
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                toggle_key(
                                    &mut this.connections.navigator_mut().nav_subfolder_expanded,
                                    &idx_key_toggle,
                                )
                            },
                        ))
                        .child(img("icons/folder.svg").w(px(16.0)).h(px(16.0)))
                        .child(div().text_color(theme.text_primary).child("Indexes")),
                ),
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
                            .child(div().text_color(theme.text_primary).child(iname.clone())),
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
                        .child("(No indexes)"),
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
            let is_cat_exp = self
                .connections
                .navigator()
                .nav_subfolder_expanded
                .contains(&cat_key);
            let cat_key_toggle = cat_key.clone();

            nav_tree = nav_tree.child(
                div()
                    .id(ElementId::Name(
                        format!("nav-cat-{}-{}-{}", f_id, table_schema, table_name).into(),
                    ))
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .pl(px(86.0))
                    .pr_2()
                    .rounded_sm()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.5))
                            .child(nav_chevron(
                                format!("nav-chev-{}", cat_key_toggle),
                                if is_cat_exp { "▾" } else { "▸" },
                                10.0,
                                9.0,
                                theme.text_muted,
                                theme.text_primary,
                                cx,
                                move |this, _| {
                                    toggle_key(
                                        &mut this
                                            .connections
                                            .navigator_mut()
                                            .nav_subfolder_expanded,
                                        &cat_key_toggle,
                                    )
                                },
                            ))
                            .child(if is_partition {
                                img("icons/folder_table.svg").w(px(16.0)).h(px(16.0))
                            } else {
                                img("icons/folder.svg").w(px(16.0)).h(px(16.0))
                            })
                            .child(div().text_color(theme.text_primary).child(f_label)),
                    ),
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
                        .child(f_empty),
                );
            }
        }

        nav_tree
    }
}
