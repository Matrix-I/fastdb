use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_statistics(&self, theme: &Theme) -> AnyElement {
        let columns_vec = &self.columns;
        let t_name = self.table.as_str();
        let s_name = self.schema.as_str();
        let owner_name = "Unavailable";
        let table_oid = "Unavailable";

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
                .child(
                    div()
                        .w(px(220.0))
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child("Name"),
                )
                .child(div().flex_1().px_2().child("Value")),
        );

        let has_pk = if columns_vec.iter().any(|c| c.is_primary_key) {
            "Yes"
        } else {
            "No"
        };
        let cols_cnt = format!("{}", columns_vec.len());
        let oid_str = table_oid.to_string();
        let rows = [
            ("Table Name", t_name),
            ("Schema", s_name),
            ("Object ID", &oid_str),
            ("Tablespace", "Unavailable"),
            ("Owner", owner_name),
            ("Columns Count", &cols_cnt),
            ("Has Primary Key", has_pk),
            ("Row-Level Security", "Unavailable"),
            ("Partitioned", "Unavailable"),
            ("Persistence", "Unavailable"),
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
                    .child(
                        div()
                            .w(px(220.0))
                            .px_2()
                            .text_color(theme.text_secondary)
                            .child(k),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .text_color(theme.text_primary)
                            .child(v.to_string()),
                    ),
            );
        }

        stats_table.into_any_element()
    }
}
