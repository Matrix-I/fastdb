use super::*;

impl Workspace {
    /// Diagram sub-tab: entity box listing the table columns
    pub(super) fn render_diagram_panel(&self, active_tab_idx: usize) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[active_tab_idx];
        let table_name_opt = tab.table_name.clone();
        let schema_name_opt = tab.schema_name.clone();
        // Diagram View
        let t_name = table_name_opt.as_deref().unwrap_or("Table");
        let s_name = schema_name_opt.as_deref().unwrap_or("public");
        let columns_opt = self.table_columns.get(&(s_name.to_string(), t_name.to_string()));

        let diag_view = div()
            .id("diag-view-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .p_6()
            .bg(theme.bg_editor)
            .overflow_y_scroll()
            .child(
                div()
                    .w(px(280.0))
                    .rounded_md()
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(rgb(0x388bfd))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x135ba2))
                            .text_color(rgb(0xffffff))
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.0))
                            .child(img("icons/table.svg").w(px(14.0)).h(px(14.0)))
                            .child(format!("{}.{}", s_name, t_name))
                    )
                    .child({
                        let mut col_list = div().flex().flex_col().p_2().gap_1();
                        if let Some(cols) = columns_opt {
                            for c in cols {
                                col_list = col_list.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .text_size(px(11.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(if c.is_primary_key { "🗝" } else { "•" })
                                                .child(div().text_color(theme.text_primary).child(c.name.clone()))
                                        )
                                        .child(
                                            div()
                                                .text_color(theme.text_muted)
                                                .text_size(px(9.5))
                                                .child(c.data_type.to_string())
                                        )
                                );
                            }
                        } else {
                            col_list = col_list.child(
                                div().text_size(px(11.0)).text_color(theme.text_muted).child("Expand table in navigator to display diagram columns.")
                            );
                        }
                        col_list
                    })
            );


        diag_view
    }
}
