use super::*;
use gpui::prelude::FluentBuilder;

impl Workspace {
    /// Center area: tab strip and the active table tab (data, properties, diagram)
    pub(super) fn render_table_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let active_tab_idx = self.active_tab;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(theme.bg_grid)
            .child(self.render_document_tabs(cx))
            .child({
                if self.tabs.is_empty() || active_tab_idx >= self.tabs.len() {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .size_full()
                        .gap_3()
                        .bg(theme.bg_grid)
                        .child(
                            img("icons/table.svg")
                                .w(px(52.0))
                                .h(px(52.0))
                        )
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_secondary)
                                .child("No Table Opened")
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.text_muted)
                                .text_center()
                                .child("Click a table in the Database Navigator on the left to open and view data.")
                        )
                        .into_any_element()
                } else if self.tabs[active_tab_idx].column_editor.is_some() {
                    self.render_column_editor(active_tab_idx, window, cx).into_any_element()
                } else if self.tabs[active_tab_idx].is_script() {
                    self.render_script_view(active_tab_idx, window, cx).into_any_element()
                } else {
                    let active_subtab = self.tabs[active_tab_idx].active_subtab;
                    let show_sql_dialog = self.tabs[active_tab_idx].show_sql_dialog;
                    let filter_sql = self.tabs[active_tab_idx].filter_sql.clone();
                    let is_filter_focused = self.input.is_filter_focused;
                    let is_running_query = self.operations.is_running();
                    let history_editable= !is_running_query && !self.tabs[active_tab_idx].grid.changeset.is_dirty()
                        && self.tabs[active_tab_idx].grid.editing_cell.is_none() && self.tab_session(active_tab_idx).driver.is_some();
                    let can_back=history_editable&&self.tabs[active_tab_idx].filter_timeline.can_back();
                    let can_forward=history_editable&&self.tabs[active_tab_idx].filter_timeline.can_forward();
                    let expanded=self.layout.focus_data;

                    let mut center_col = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .overflow_hidden();

                // A. DBeaver Sub-tabs Bar: Properties | Data | Diagram
                center_col = center_col.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(28.0))
                        .bg(theme.bg_surface)
                        .border_b_1()
                        .border_color(theme.border)
                        .px_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                // 1. Properties
                                .child({
                                    let is_prop = active_subtab == TableSubTab::Properties;
                                    div()
                                        .id("subtab-properties")
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_sm()
                                        .bg(if is_prop { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .border_b_2()
                                        .border_color(if is_prop { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .text_color(if is_prop { theme.text_primary } else { theme.text_muted })
                                        .text_size(px(11.5))
                                        .font_weight(if is_prop { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.bg_surface_hover))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                t.active_subtab = TableSubTab::Properties;
                                                this.input.is_filter_focused = false;
                                                cx.notify();
                                            }
                                        }))
                                        .child(img("icons/folder_table.svg").w(px(13.0)).h(px(13.0)))
                                        .child("Properties")
                                })
                                // 2. Data
                                .child({
                                    let is_data = active_subtab == TableSubTab::Data;
                                    div()
                                        .id("subtab-data")
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_sm()
                                        .bg(if is_data { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .border_b_2()
                                        .border_color(if is_data { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .text_color(if is_data { theme.text_primary } else { theme.text_muted })
                                        .text_size(px(11.5))
                                        .font_weight(if is_data { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.bg_surface_hover))
                                        .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                                            let mut needs_load = false;
                                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                t.active_subtab = TableSubTab::Data;
                                                needs_load = t.grid.pager.is_none() || event.click_count() >= 2;
                                            }
                                            if needs_load {
                                                this.run_active_query(cx);
                                            }
                                            cx.notify();
                                        }))
                                        .child(img("icons/grid.svg").w(px(13.0)).h(px(13.0)))
                                        .child("Data")
                                })
                                // 3. Diagram
                                .child({
                                    let is_diag = active_subtab == TableSubTab::Diagram;
                                    div()
                                        .id("subtab-diagram")
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_sm()
                                        .bg(if is_diag { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .border_b_2()
                                        .border_color(if is_diag { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                        .text_color(if is_diag { theme.text_primary } else { theme.text_muted })
                                        .text_size(px(11.5))
                                        .font_weight(if is_diag { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.bg_surface_hover))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                t.active_subtab = TableSubTab::Diagram;
                                                this.input.is_filter_focused = false;
                                                cx.notify();
                                            }
                                        }))
                                        .child(img("icons/schema.svg").w(px(13.0)).h(px(13.0)))
                                        .child("Diagram")
                                })
                        )
                );

                // B. Filter Toolbar (when in Data subtab)
                if active_subtab == TableSubTab::Data {
                    center_col = center_col.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(32.0))
                            .bg(theme.bg_surface)
                            .border_b_1()
                            .border_color(theme.border)
                            .px_2()
                            // DBeaver Query Source Preview: select * from "table" alias
                            .child({
                                let table_name = self.tabs[active_tab_idx].table_name.clone().unwrap_or_default();
                                let mut alias = String::new();
                                let mut prev_lower = false;
                                for c in table_name.chars() {
                                    if c.is_alphabetic() {
                                        if alias.is_empty() {
                                            alias.push(c.to_ascii_lowercase());
                                            prev_lower = c.is_lowercase();
                                        } else if c.is_uppercase() && prev_lower {
                                            alias.push(c.to_ascii_lowercase());
                                            prev_lower = false;
                                        } else if c == '_' {
                                            prev_lower = false;
                                        }
                                    }
                                }
                                let alias = if alias.is_empty() { "t".to_string() } else { alias };
                                let sql_label = format!("select * from \"{table_name}\" {alias}");

                                div()
                                    .id("btn-show-sql")
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(if show_sql_dialog { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                    .cursor_pointer()
                                    .text_size(px(11.5))
                                    .text_color(rgb(0x3fb950))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                            t.show_sql_dialog = !t.show_sql_dialog;
                                            cx.notify();
                                        }
                                    }))
                                    .child(img("icons/sql.svg").w(px(13.0)).h(px(13.0)))
                                    .child(sql_label)
                            })
                            .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                            .child(
                                div()
                                    .id("btn-expand-data")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(22.0))
                                    .h(px(22.0))
                                    .rounded_xs()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.layout.focus_data = !this.layout.focus_data;
                                        this.layout.resizing_panel = None;
                                        cx.notify();
                                    }))
                                    .child(if expanded { "⤡" } else { "⤢" }),
                            )
                            // Filter Expression Input Box ("khung để nối thêm sql như dbeaver")
                            .child(
                                ui_editor::line_input::pick_surface(div(), usize::MAX, &self.filter_caret_pick(cx))
                                    .id("filter-sql-input-box")
                                    .relative()
                                    .flex()
                                    .flex_1()
                                    .items_center()
                                    .h(px(24.0))
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(if is_filter_focused { rgb(0x388bfd) } else { theme.border })
                                    .rounded_xs()
                                    .px_2()
                                    .cursor_text()
                                    .text_size(px(11.5))
                                    .child(render_field_content(
                                        &filter_sql,
                                        "Enter a SQL expression to filter results (use Ctrl+Space)",
                                        is_filter_focused,
                                        self.input.filter_anchor,
                                        self.input.filter_cursor,
                                        self.input.cursor_visible,
                                        theme,
                                        &self.filter_caret_pick(cx),
                                    ))
                                    .children(self.render_filter_suggest(cx))
                            )
                            // Execute Query Button ▶ (Green play arrow)
                            .child(
                                div()
                                    .id("btn-execute-filtered-query")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .rounded_xs()
                                    .bg(if is_running_query { rgb(0xda3633) } else { rgb(0x238636) })
                                    .hover(move |s| s.bg(if is_running_query { rgb(0xf85149) } else { rgb(0x2ea043) }))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        // While a query runs, the button cancels it
                                        if this.operations.is_running() {
                                            this.cancel_running_query(cx);
                                        } else {
                                            this.run_active_query(cx);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        if is_running_query {
                                            div().text_size(px(11.0)).text_color(rgb(0xffffff)).child("⏹").into_any_element()
                                        } else {
                                            img("icons/sql_exec.svg").w(px(13.0)).h(px(13.0)).into_any_element()
                                        }
                                    )
                            )
                            .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                            // Clear filter button ✕
                            .child(
                                div()
                                    .id("btn-clear-filter")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px_1p5()
                                    .py_1()
                                    .rounded_xs()
                                    .cursor_pointer()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.bg(theme.bg_surface_hover).text_color(theme.text_primary))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                            t.filter_sql.clear();
                                            this.run_active_query(cx);
                                            cx.notify();
                                        }
                                    }))
                                    .child("✕")
                            )
                            // Filter criteria funnel ∇
                            .child(
                                div().id("btn-filter-criteria").px_1().py_0p5().text_size(px(11.0))
    .text_color(theme.text_secondary).cursor_pointer().hover(|s|s.bg(theme.bg_surface_hover))
    .on_click(cx.listener(|this,_,_,cx|this.open_filter_builder(cx))).child("Criteria")
                            )
                            // History Back ◀
                            .child(
                                div().id("btn-filter-back").px_1().py_0p5().text_size(px(10.0))
    .text_color(if can_back{theme.text_primary}else{theme.text_muted})
    .when(can_back,|button|button.cursor_pointer().hover(|s|s.bg(theme.bg_surface_hover))
        .on_click(cx.listener(|this,_,_,cx|this.navigate_filter_history(false,cx))))
    .child("◀")
                            )
                            // History Forward ▶
                            .child(
                                div().id("btn-filter-forward").px_1().py_0p5().text_size(px(10.0))
    .text_color(if can_forward{theme.text_primary}else{theme.text_muted})
    .when(can_forward,|button|button.cursor_pointer().hover(|s|s.bg(theme.bg_surface_hover))
        .on_click(cx.listener(|this,_,_,cx|this.navigate_filter_history(true,cx))))
    .child("▶")
                            )
                            // Refresh ↻
                            .child(
                                div()
                                    .id("btn-refresh-data")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px_1p5()
                                    .py_1()
                                    .rounded_xs()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0x58a6ff))
                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_active_query(cx);
                                        cx.notify();
                                    }))
                                    .child("↻")
                            )
                    );
                }

                // C. Optional Show SQL preview dialog
                if show_sql_dialog {
                    let cur_sql = self.build_query_for_tab(active_tab_idx);
                    let sql_clone = cur_sql.clone();
                    center_col = center_col.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .bg(theme.bg_editor)
                            .border_b_1()
                            .border_color(theme.border)
                            .p_2p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.text_secondary)
                                            .child("GENERATED SQL QUERY")
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .id("btn-copy-generated-sql")
                                                    .px_2()
                                                    .py_0p5()
                                                    .rounded_xs()
                                                    .bg(theme.bg_surface)
                                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                                    .text_size(px(10.5))
                                                    .text_color(theme.text_primary)
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.clipboard_request_id += 1;
                                                        cx.write_to_clipboard(ClipboardItem::new_string(sql_clone.clone()));
                                                    }))
                                                    .child("📋 Copy")
                                            )
                                            .child(
                                                div()
                                                    .id("btn-close-sql-preview")
                                                    .px_1p5()
                                                    .rounded_xs()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.text_muted)
                                                    .hover(|s| s.text_color(theme.text_primary))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                            t.show_sql_dialog = false;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child("✕")
                                            )
                                    )
                            )
                            .child(
                                div()
                                    .p_2()
                                    .rounded_xs()
                                    .bg(theme.bg_surface)
                                    .text_size(px(11.5))
                                    .text_color(theme.text_accent)
                                    .child(cur_sql)
                            )
                    );
                }

                if active_subtab == TableSubTab::Data
                    && let Some(message) = self.tabs[active_tab_idx].last_error.clone() {
                        center_col = center_col.child(self.render_error_banner(&message));
                    }

                // D. Main View Body depending on active_subtab
                match active_subtab {
                    TableSubTab::Data => {
                        center_col = center_col.child(self.render_data_panel(active_tab_idx, window, cx));
                    }
                    TableSubTab::Properties => {
                        center_col = center_col.child(self.render_properties_panel(active_tab_idx, cx));
                    }
                    TableSubTab::Diagram => {
                        center_col = center_col.child(self.render_diagram_panel(active_tab_idx, window, cx));
                    }
                }

                // E. Result footer (when in Data subtab)
                if active_subtab == TableSubTab::Data {
                    center_col = center_col.child(self.render_result_footer(active_tab_idx, cx));
                }

                    center_col.into_any_element()
                }
            })
    }

    /// Status bar under a result: edit toolbar, fetch size, fetch next, row count and the
    /// summary of the last fetch, laid out like DBeaver's result panel status bar
    pub(super) fn render_result_footer(
        &mut self,
        tab_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[tab_idx];
        let has_more_rows = tab.fetch.has_more;
        let is_appending = tab.fetch.is_appending();
        let fetch_summary = tab.fetch.last.as_ref().map(|s| s.message());
        let total_rows = tab.loaded_rows();
        let fetch_size_text = tab.fetch.size_text.clone();
        let is_fetch_size_focused = self.input.is_fetch_size_focused;
        let is_running_query = self.operations.is_running();
        let can_fetch_next = has_more_rows && !is_running_query;
        let row_count = format!(
            "{}{}",
            group_thousands(total_rows),
            if has_more_rows { "+" } else { "" }
        );
        let summary = if let Some(message) = &tab.export.message {
            message.clone()
        } else if let Some(status) = &tab.script_status {
            status.clone()
        } else if is_appending {
            "Fetching next rows…".to_string()
        } else {
            fetch_summary.unwrap_or_default()
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .bg(theme.bg_surface)
            .border_t_1()
            .border_color(theme.border)
            .px_2()
            .text_size(px(11.0))
            .text_color(theme.text_muted)
            .child(self.render_edit_toolbar(cx))
            .child(self.render_export_toolbar(tab_idx, cx))
            .child(
                ui_editor::line_input::pick_surface(
                    div(),
                    usize::MAX,
                    &self.fetch_size_caret_pick(cx),
                )
                .id("fetch-size-input-box")
                .flex()
                .items_center()
                .w(px(64.0))
                .h(px(20.0))
                .px_1()
                .bg(theme.bg_editor)
                .border_1()
                .border_color(if is_fetch_size_focused {
                    rgb(0x388bfd)
                } else {
                    theme.border
                })
                .rounded_xs()
                .cursor_text()
                .text_color(theme.text_primary)
                .child(render_field_content(
                    &fetch_size_text,
                    "",
                    is_fetch_size_focused,
                    self.input.fetch_size_anchor,
                    self.input.fetch_size_cursor,
                    self.input.cursor_visible,
                    theme,
                    &self.fetch_size_caret_pick(cx),
                )),
            )
            .child({
                let fetch_next_btn = div()
                    .id("btn-fetch-next-page")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(20.0))
                    .h(px(20.0))
                    .rounded_xs();
                let fetch_next_btn = if can_fetch_next {
                    fetch_next_btn
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                } else {
                    fetch_next_btn.opacity(0.4)
                };
                fetch_next_btn
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.blur_fetch_size();
                        this.fetch_next_rows(cx);
                        cx.notify();
                    }))
                    .child(img("icons/fetch_next.svg").w(px(14.0)).h(px(14.0)))
            })
            .child(div().text_color(theme.text_secondary).child(row_count))
            .child(div().w(px(1.0)).h(px(14.0)).bg(theme.border))
            .child({
                let tab_id = tab.id;
                let has_pager = tab.grid.pager.is_some();
                div()
                    .id("btn-find-in-result")
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(20.0))
                    .px_1p5()
                    .rounded_xs()
                    .text_size(px(11.0))
                    .text_color(if has_pager {
                        theme.text_secondary
                    } else {
                        theme.text_muted
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child("⌕ Find…")
                    .when(has_pager, |btn| {
                        btn.on_click(cx.listener(move |this, _, _, cx| {
                            this.open_result_search(tab_id, cx);
                        }))
                    })
            })
            .children(
                (tab.is_script() && self.tab_provider(tab_idx) == DbDriverKind::Postgres).then(
                    || {
                        let tab_id = tab.id;
                        div()
                            .id("btn-load-editable-result")
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(20.0))
                            .px_1p5()
                            .rounded_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child("Load editable")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.load_editable_result(tab_id, cx);
                            }))
                    },
                ),
            )
            .children(tab.reference_picker.is_some().then(|| {
                div()
                    .id("btn-use-selected-key")
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(20.0))
                    .px_1p5()
                    .rounded_xs()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child("Use selected key")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.accept_grid_reference(cx);
                    }))
            }))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(summary),
            )
    }
}
