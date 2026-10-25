use super::*;

impl Workspace {
    /// Center area: tab strip and the active table tab (data, properties, diagram)
    pub(super) fn render_table_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let active_tab_idx = self.active_tab;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .bg(theme.bg_grid)
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
                                .child("Click a table in the Database Navigator on the left to open and view data.")
                        )
                        .into_any_element()
                } else {
                    let active_subtab = self.tabs[active_tab_idx].active_subtab;
                    let show_sql_dialog = self.tabs[active_tab_idx].show_sql_dialog;
                    let filter_sql = self.tabs[active_tab_idx].filter_sql.clone();
                    let current_limit = self.tabs[active_tab_idx].limit;
                    let total_rows = self.tabs[active_tab_idx].grid.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
                    let exec_ms = self.tabs[active_tab_idx].execution_time_ms;
                    let is_filter_focused = self.is_filter_focused;
                    let is_running_query = self.is_running_query;

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
                                                this.is_filter_focused = false;
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
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                                t.active_subtab = TableSubTab::Data;
                                                cx.notify();
                                            }
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
                                                this.is_filter_focused = false;
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
                            // Show SQL Button
                            .child(
                                div()
                                    .id("btn-show-sql")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(if show_sql_dialog { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                    .hover(|s| s.bg(theme.bg_surface_hover))
                                    .cursor_pointer()
                                    .text_size(px(11.0))
                                    .text_color(rgb(0x3fb950))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                            t.show_sql_dialog = !t.show_sql_dialog;
                                            cx.notify();
                                        }
                                    }))
                                    .child(img("icons/sql.svg").w(px(12.0)).h(px(12.0)))
                                    .child("Show SQL")
                            )
                            .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .child("⛶")
                            )
                            // Filter Expression Input Box ("khung để nối thêm sql như dbeaver")
                            .child(
                                div()
                                    .id("filter-sql-input-box")
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
                                    .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                                        this.is_filter_focused = true;
                                        this.filter_selected_all = event.click_count() >= 2;
                                        this.cursor_visible = true;
                                        cx.notify();
                                    }))
                                    .child({
                                        let mut filter_row = div().flex().items_center().h(px(18.0)).w_full().overflow_hidden();
                                        if filter_sql.is_empty() {
                                            if is_filter_focused {
                                                filter_row = filter_row
                                                    .child(
                                                        div()
                                                            .w(px(1.5))
                                                            .h(px(13.0))
                                                            .rounded_xs()
                                                            .bg(if self.cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    )
                                                    .child(
                                                        div()
                                                            .pl_1()
                                                            .text_size(px(11.0))
                                                            .text_color(theme.text_muted)
                                                            .child("Enter a SQL expression to filter results (use Ctrl+Space)")
                                                    );
                                            } else {
                                                filter_row = filter_row.child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.text_muted)
                                                        .child("Enter a SQL expression to filter results (use Ctrl+Space)")
                                                );
                                            }
                                        } else {
                                            if is_filter_focused && self.filter_selected_all {
                                                filter_row = filter_row.child(
                                                    div()
                                                        .px_1()
                                                        .py_0p5()
                                                        .rounded_xs()
                                                        .bg(rgb(0x264f78))
                                                        .text_color(rgb(0xffffff))
                                                        .text_size(px(11.5))
                                                        .child(filter_sql.clone())
                                                );
                                            } else if is_filter_focused {
                                                filter_row = filter_row
                                                    .child(
                                                        div()
                                                            .text_size(px(11.5))
                                                            .text_color(theme.text_primary)
                                                            .child(filter_sql.clone())
                                                    )
                                                    .child(
                                                        div()
                                                            .pl(px(1.0))
                                                            .w(px(1.5))
                                                            .h(px(13.0))
                                                            .rounded_xs()
                                                            .bg(if self.cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                                                    );
                                            } else {
                                                filter_row = filter_row.child(
                                                    div()
                                                        .text_size(px(11.5))
                                                        .text_color(theme.text_primary)
                                                        .child(filter_sql.clone())
                                                );
                                            }
                                        }
                                        filter_row
                                    })
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
                                        if this.is_running_query {
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
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .child("∇")
                            )
                            // History Back ◀
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .child("◀")
                            )
                            // History Forward ▶
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
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
                                                    .on_click(cx.listener(move |_, _, _, cx| {
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

                // D. Main View Body depending on active_subtab
                match active_subtab {
                    TableSubTab::Data => {
                        center_col = center_col.child(self.render_data_panel(active_tab_idx, window, cx));
                    }
                    TableSubTab::Properties => {
                        center_col = center_col.child(self.render_properties_panel(active_tab_idx, cx));
                    }
                    TableSubTab::Diagram => {
                        center_col = center_col.child(self.render_diagram_panel(active_tab_idx));
                    }
                }

                // E. Table Bottom Pagination & Status Bar (when in Data subtab)
                if active_subtab == TableSubTab::Data {
                    center_col = center_col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(24.0))
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
                                    .gap_1p5()
                                    .child(img("icons/grid.svg").w(px(12.0)).h(px(12.0)))
                                    .child({
                                        let exec_str = if let Some(ms) = exec_ms {
                                            format!(" | {}ms", ms)
                                        } else {
                                            "".to_string()
                                        };
                                        format!("{} rows{}", total_rows, exec_str)
                                    })
                                    .child(div().text_color(theme.text_muted).child(" | Zero-Copy 120 FPS Culled Grid"))
                            )
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
                                            .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("|◀"))
                                            .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("◀"))
                                            .child(div().px_1p5().text_size(px(10.5)).text_color(theme.text_secondary).child(format!("1 - {}", current_limit)))
                                            .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("▶"))
                                            .child(div().px_1().text_size(px(10.0)).cursor_pointer().hover(|s| s.text_color(theme.text_primary)).child("▶|"))
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_xs()
                                            .bg(theme.bg_editor)
                                            .border_1()
                                            .border_color(theme.border)
                                            .text_size(px(10.5))
                                            .text_color(theme.text_secondary)
                                            .child(format!("Limit: {}", current_limit))
                                    )
                            )
                    );
                }

                    center_col.into_any_element()
                }
            })
    }
}
