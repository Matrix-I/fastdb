use super::*;
impl Workspace {
    pub(super) fn append_table_node(
        &self,
        mut nav_tree: Stateful<Div>,
        t: &TableSummary,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;

        let table_schema = t.schema.clone();
        let table_name = t.name.clone();
        let table_key = format!("{}.{}", table_schema, table_name);
        let is_table_exp = self
            .connections
            .navigator()
            .nav_table_expanded
            .contains(&table_key);
        let is_selected =
            self.connections.navigator().selected_table_name.as_deref() == Some(&table_name);
        let item_id = format!("nav-table-{}-{}", table_schema, table_name);

        let t_schema_for_click = table_schema.clone();
        let t_name_for_click = table_name.clone();

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
            // Single click selects; double click opens the table's data.
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.query_table(&t_schema_for_click, &t_name_for_click, cx);
                } else {
                    this.connections.navigator_mut().selected_table_name =
                        Some(t_name_for_click.clone());
                    cx.notify();
                }
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
                        .child(nav_chevron(
                            format!("nav-chev-{}-{}", table_schema, table_name),
                            if is_table_exp { "▾" } else { "▸" },
                            12.0,
                            10.0,
                            if is_selected {
                                rgb(0xffffff)
                            } else {
                                theme.text_muted
                            },
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                if this
                                    .connections
                                    .navigator_mut()
                                    .nav_table_expanded
                                    .contains(&t_key_for_chev)
                                {
                                    this.connections
                                        .navigator_mut()
                                        .nav_table_expanded
                                        .remove(&t_key_for_chev);
                                } else {
                                    this.connections
                                        .navigator_mut()
                                        .nav_table_expanded
                                        .insert(t_key_for_chev.clone());
                                    this.connections
                                        .navigator_mut()
                                        .nav_subfolder_expanded
                                        .insert(format!("{}.columns", t_key_for_chev));
                                }
                            },
                        ))
                        .child(img("icons/table.svg").w(px(16.0)).h(px(16.0)))
                        .child(
                            div()
                                .font_weight(if is_selected {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_selected {
                                    rgb(0xffffff)
                                } else {
                                    theme.text_primary
                                })
                                .child(t.name.clone()),
                        ),
                )
                .child(if let Some(rows) = t.estimated_rows {
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
                        .border_color(if is_selected {
                            rgb(0x2874ba)
                        } else {
                            theme.border
                        })
                        .rounded_xs()
                        .text_size(px(10.0))
                        .text_color(if is_selected {
                            rgb(0x89c5ff)
                        } else {
                            theme.text_muted
                        })
                        .child(count_str)
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        );

        // Level 7: Dropdown folders under table (Columns, Constraints, Foreign Keys, Indexes, Dependencies, References, Partitions, Triggers, Rules, Policies)
        if is_table_exp {
            nav_tree = self.append_table_details(nav_tree, table_schema, table_name, table_key, cx);
        }

        nav_tree
    }
}
