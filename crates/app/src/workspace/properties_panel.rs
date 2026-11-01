use super::*;

impl Workspace {
    /// Properties sub-tab: table header form plus columns, constraints, indexes and DDL
    pub(super) fn render_properties_panel(
        &mut self,
        active_tab_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[active_tab_idx];
        let table_name_opt = tab.table_name.clone();
        let schema_name_opt = tab.schema_name.clone();
        let active_property_sec = tab.active_property_section;
        let is_header_collapsed = tab.properties_header_collapsed;
        let scroll = tab.properties_scroll.clone();
        let schema = schema_name_opt.unwrap_or_default();
        let table = table_name_opt.unwrap_or_default();
        let model = self
            .tab_session(active_tab_idx)
            .properties
            .get(&(schema.clone(), table.clone()))
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(properties::model::TableProperties::new(
                    self.tab_provider(active_tab_idx),
                    schema,
                    table,
                    vec![],
                    vec![],
                    vec![],
                ))
            });
        let t_name = model.table.as_str();
        let s_name = model.schema.as_str();
        let mut prop_container = div()
            .id("properties-main-container")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .overflow_hidden()
            .bg(theme.bg_editor);

        // 1. Top Table Properties Form Header
        if !is_header_collapsed {
            prop_container = prop_container.child(model.render_header(theme));
        }

        // 2. Horizontal Splitter Bar with Toggle Chevron
        let splitter_bar = div()
            .id("props-splitter-bar")
            .flex()
            .items_center()
            .justify_center()
            .h(px(9.0))
            .flex_shrink_0()
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
                    .child(if is_header_collapsed { "▼" } else { "▲" }),
            );

        prop_container = prop_container.child(splitter_bar);

        // 3. Bottom Master-Detail Layout
        let mut master_detail = div().flex().flex_1().min_h_0().min_w_0().overflow_hidden();

        // Left Navigation Categories
        let categories = [
            (
                PropertySection::Columns,
                "Columns",
                "icons/folder_table.svg",
            ),
            (
                PropertySection::Constraints,
                "Constraints",
                "icons/folder_constraint.svg",
            ),
            (
                PropertySection::ForeignKeys,
                "Foreign Keys",
                "icons/foreign_key.svg",
            ),
            (PropertySection::Indexes, "Indexes", "icons/table_index.svg"),
            (
                PropertySection::Dependencies,
                "Dependencies",
                "icons/folder.svg",
            ),
            (
                PropertySection::References,
                "References",
                "icons/folder.svg",
            ),
            (
                PropertySection::Partitions,
                "Partitions",
                "icons/partition.svg",
            ),
            (PropertySection::Triggers, "Triggers", "icons/trigger.svg"),
            (PropertySection::Rules, "Rules", "icons/folder.svg"),
            (PropertySection::Policies, "Policies", "icons/folder.svg"),
            (
                PropertySection::Statistics,
                "Statistics",
                "icons/configuration.svg",
            ),
            (
                PropertySection::Permissions,
                "Permissions",
                "icons/folder.svg",
            ),
            (PropertySection::Ddl, "DDL", "icons/sql_script.svg"),
            (
                PropertySection::Virtual,
                "Virtual",
                "icons/configuration.svg",
            ),
        ];

        let mut left_nav = div()
            .w(px(145.0))
            .flex_shrink_0()
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
                    .flex_shrink_0()
                    .px_2()
                    .cursor_pointer()
                    .bg(if is_selected {
                        theme.bg_surface_selected
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0).into()
                    })
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                            t.active_property_section = sec;
                            t.properties_scroll.set_offset(point(px(0.0), px(0.0)));
                            cx.notify();
                        }
                    }))
                    .child(img(icon).w(px(13.0)).h(px(13.0)))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(if is_selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_selected {
                                theme.text_primary
                            } else {
                                theme.text_secondary
                            })
                            .child(label),
                    ),
            );
        }

        master_detail = master_detail.child(left_nav);

        // Right Detail Content
        let mut right_content = div()
            .id(("props-detail-scroll", tab.id))
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_scroll()
            .track_scroll(&scroll)
            .bg(theme.bg_editor);

        match active_property_sec {
            PropertySection::Columns => {
                right_content = right_content.child(model.render_columns(theme));
            }
            PropertySection::Constraints => {
                right_content = right_content.child(model.render_constraints(theme));
            }
            PropertySection::ForeignKeys => {
                right_content = right_content.child(model.render_foreignkeys(theme));
            }
            PropertySection::Indexes => {
                right_content = right_content.child(model.render_indexes(theme));
            }
            PropertySection::Ddl => {
                right_content = right_content.child(model.render_ddl(theme, cx));
            }
            PropertySection::Statistics => {
                right_content = right_content.child(model.render_statistics(theme));
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
                            .child(format!("{} metadata", sec_title)),
                    )
                    .child(
                        div()
                            .p_6()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child(format!(
                                "{} metadata is unavailable for table {}.{}.",
                                sec_title.to_lowercase(),
                                s_name,
                                t_name
                            )),
                    );

                right_content = right_content.child(empty_view);
            }
        }

        master_detail = master_detail.child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .child(right_content)
                        .child(properties::grid::detail_scrollbar(
                            scroll.clone(),
                            true,
                            theme,
                        )),
                )
                .child(properties::grid::detail_scrollbar(scroll, false, theme)),
        );
        prop_container = prop_container.child(master_detail);

        prop_container
    }
}
