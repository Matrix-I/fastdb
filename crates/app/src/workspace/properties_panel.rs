use super::*;
use db_wire::metadata::MetadataFeature as F;
use gpui::prelude::FluentBuilder;

impl Workspace {
    /// Properties sub-tab: table header form plus columns, constraints, indexes and DDL
    pub(super) fn render_properties_panel(
        &mut self,
        active_tab_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        self.ensure_catalog_section(active_tab_idx, cx);
        self.ensure_relation_ddl(active_tab_idx, cx);
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
        let model = if active_property_sec == PropertySection::Ddl {
            tab.catalog
                .relation_ddl
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .cloned()
                .unwrap_or(model)
        } else {
            model
        };
        let mut prop_container = div()
            .id("properties-main-container")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .w_full()
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
                    .bg(if is_selected {
                        theme.bg_surface_selected
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0).into()
                    })
                    .when(sec != PropertySection::Virtual, |row| {
                        row.cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                    t.active_property_section = sec;
                                    t.ddl_selected_all = false;
                                    this.input.is_filter_focused = false;
                                    this.input.is_fetch_size_focused = false;
                                    t.properties_scroll.set_offset(point(px(0.0), px(0.0)));
                                    cx.notify();
                                }
                            }))
                    })
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
                            .child(if sec == PropertySection::Virtual {
                                "Virtual · pending"
                            } else {
                                label
                            }),
                    ),
            );
        }

        left_nav = left_nav.child(
            div()
                .p_2()
                .text_size(px(10.0))
                .text_color(theme.text_muted)
                .child("Virtual keys/relationships are not implemented yet."),
        );
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

        let feature = match active_property_sec {
            PropertySection::Columns | PropertySection::Ddl => Some(F::Columns),
            PropertySection::Constraints => Some(F::Constraints),
            PropertySection::ForeignKeys => Some(F::ForeignKeys),
            PropertySection::Indexes => Some(F::Indexes),
            PropertySection::Statistics => Some(F::TableDetails),
            _ => None,
        };
        let notice = if self.tab_session(active_tab_idx).driver.is_none() {
            Some("Disconnected. Connect and refresh to load metadata.".to_string())
        } else {
            if active_property_sec == PropertySection::Ddl {
                if tab.catalog.ddl_loading {
                    Some("Loading native relation DDL…".into())
                } else if let Some(Err(error)) = &tab.catalog.relation_ddl {
                    Some(error.clone())
                } else {
                    model.ddl_notice()
                }
            } else if active_property_sec == PropertySection::Statistics {
                model.statistics_notice()
            } else {
                feature.and_then(|feature| model.section_notice(feature))
            }
        };
        if let Some(notice) = notice {
            right_content = right_content.child(div().p_4().child(notice)).child(
                div()
                    .p_4()
                    .text_color(theme.text_muted)
                    .child("Refresh the owning connection to retry metadata loading."),
            );
        } else {
            if let Some(feature) = feature
                && let Some(state) = model.states.get(&feature)
                && let Some(message) = state.message()
            {
                right_content =
                    right_content.child(div().p_3().text_color(theme.text_muted).child(format!(
                        "Summary reader: {message} Displaying independently loaded native details."
                    )));
            }
            if active_property_sec == PropertySection::Ddl
                && !matches!(tab.catalog.relation_ddl, Some(Ok(_)))
                && let Some(notice) = model.section_notice(F::TableDetails)
            {
                right_content =
                    right_content.child(div().p_3().text_color(theme.text_muted).child(format!(
                        "Native DDL unavailable. {notice} Showing a partial metadata preview."
                    )));
            }
            match active_property_sec {
                PropertySection::Columns => {
                    let pick = self.column_open_pick(
                        tab.connection_id.clone().unwrap_or_default(),
                        model.schema.clone(),
                        model.table.clone(),
                        cx,
                    );
                    // Details follow the grid so selection cannot shift double-click targets.
                    right_content = right_content.child(model.render_columns(theme, pick));
                    let selected_details =
                        self.column_pick
                            .as_ref()
                            .and_then(|(owner, schema, table, column)| {
                                (tab.connection_id.as_ref() == Some(owner)
                                    && schema == &model.schema
                                    && table == &model.table)
                                    .then(|| model.selected_column_details(column))
                                    .flatten()
                            });
                    if let Some(details) = selected_details {
                        let copy = details.clone();
                        right_content = right_content.child(
                            div()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    // Keep the selected column while using its inspector.
                                    cx.stop_propagation();
                                })
                                .p_3()
                                .flex()
                                .flex_col()
                                .flex_shrink_0()
                                .gap_2()
                                .text_size(px(12.0))
                                .child(super::script_tools::tool("Copy column details").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.clipboard_request_id += 1;
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                    }),
                                ))
                                .child(details),
                        );
                    } else {
                        right_content = right_content.child(
                            div().p_2().text_color(theme.text_muted).child(
                                "Select a column to inspect and copy its full values. Double-click or press Enter to open its editor.",
                            ),
                        );
                    }
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
                    right_content = right_content
                        .cursor_text()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.input.is_filter_focused = false;
                                this.input.is_fetch_size_focused = false;
                                if let Some(tab) = this.tabs.get_mut(this.active_tab) {
                                    tab.ddl_selected_all = false;
                                }
                                window.focus(&this.focus_handle);
                                cx.notify();
                            }),
                        )
                        .child(model.render_ddl(theme, tab.ddl_selected_all));
                }
                PropertySection::Statistics => {
                    right_content = right_content.child(model.render_statistics(theme));
                }
                section if section.catalog().is_some() => {
                    right_content = right_content.child(self.render_catalog_section(
                        active_tab_idx,
                        section.catalog().unwrap(),
                        cx,
                    ));
                }
                _ => {
                    right_content = right_content.child(
                        div()
                            .p_4()
                            .text_color(theme.text_muted)
                            .child("Virtual keys/relationships are not implemented yet."),
                    );
                }
            }
        }
        let mut detail = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden();
        if active_property_sec == PropertySection::Ddl
            && !tab.catalog.ddl_loading
            && !matches!(tab.catalog.relation_ddl, Some(Err(_)))
        {
            detail = detail.child(model.render_ddl_toolbar(theme, cx));
        }
        master_detail = master_detail.child(
            detail
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
