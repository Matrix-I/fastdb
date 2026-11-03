use super::model::TableProperties;
use crate::workspace::ddl::render_dbeaver_col_badge;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn selected_column_details(&self, name: &str) -> Option<String> {
        self.columns
            .iter()
            .find(|column| column.name == name)
            .map(column_details)
    }

    pub(in crate::workspace) fn render_columns(
        &self,
        theme: &Theme,
        pick: crate::workspace::column_editor::ColumnPick,
    ) -> AnyElement {
        let columns_vec = &self.columns;

        let mut col_grid = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .min_w(px(1040.0))
            .id("props-col-grid");

        // DBeaver Grid Header
        let col_header = div()
            .flex()
            .items_center()
            .h(px(24.0))
            .flex_shrink_0()
            .bg(theme.bg_grid_header)
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(11.0))
            .text_color(theme.text_secondary)
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(280.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Column Name"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(40.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("#"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(180.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Data type"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(80.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Identity"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(90.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Collation"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(90.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Default"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(80.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(theme.border)
                    .child("Not Null"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(100.0))
                    .px_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child("Comment"),
            );

        col_grid = col_grid.child(col_header);

        for (idx, c) in columns_vec.iter().enumerate() {
            let dtype_disp = c.display_type();
            let is_not_null = !c.nullable;

            let pick = pick.clone();
            let name = c.name.clone();
            let row = div()
                .id(("column-property", idx))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    pick(name.clone(), event.click_count, window, cx);
                    cx.stop_propagation();
                })
                .flex()
                .items_center()
                .min_h(px(26.0))
                .py_1()
                .line_height(px(18.0))
                .flex_shrink_0()
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
                        .flex_shrink_0()
                        .w(px(280.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(render_dbeaver_col_badge(&dtype_disp))
                        .child(
                            div()
                                .min_w_0()
                                .text_ellipsis()
                                .text_color(theme.text_primary)
                                .child(c.name.clone()),
                        ),
                )
                // 2. #
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(40.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_muted)
                        .child(format!("{}", idx + 1)),
                )
                // 3. Data type
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(180.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_primary)
                        .child(dtype_disp),
                )
                // 4. Identity
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(80.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_muted)
                        .child(""),
                )
                // 5. Collation
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(90.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_secondary)
                        .child(c.collation.clone().unwrap_or_else(|| "default".to_string())),
                )
                // 6. Default
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(90.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_muted)
                        .child(c.default_val.clone().unwrap_or_default()),
                )
                // 7. Not Null
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(80.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(if is_not_null {
                            theme.text_primary
                        } else {
                            theme.text_muted
                        })
                        .child(if is_not_null { "[v]" } else { "[ ]" }),
                )
                // 8. Comment
                .child(
                    div()
                        .flex_1()
                        .min_w(px(100.0))
                        .px_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.text_muted)
                        .child(c.comment.clone().unwrap_or_default()),
                );

            col_grid = col_grid.child(row);
        }

        col_grid.into_any_element()
    }
}

fn column_details(column: &db_wire::ColumnMeta) -> String {
    // Summary metadata cannot distinguish absent values from unsupported fields.
    // Preserve native spelling rather than the normalized grid display type.
    format!(
        "Name: {}\nNative type: {}\nNullable: {}\nCollation: {}\nDefault: {}\nComment: {}",
        column.name,
        column.raw_type.as_deref().unwrap_or("Unavailable"),
        if column.nullable { "Yes" } else { "No" },
        column.collation.as_deref().unwrap_or("Not reported"),
        column.default_val.as_deref().unwrap_or("Not reported"),
        column.comment.as_deref().unwrap_or("Not reported"),
    )
}

#[cfg(test)]
mod tests {
    use super::column_details;
    use db_wire::{ColumnMeta, DataType};

    #[test]
    fn details_preserve_full_native_values_and_unicode() {
        let column = ColumnMeta::new("Long column 名", DataType::Text)
            .with_raw_type("\"MixedCase\".\"Domain\"[]")
            .with_default("'first\nsecond'::text")
            .with_collation("\"MixedCase\".\"Collation\"")
            .with_comment("First line\nSecond line 名")
            .with_nullable(false);
        let text = column_details(&column);
        assert!(text.contains("Name: Long column 名\n"));
        assert!(text.contains("Native type: \"MixedCase\".\"Domain\"[]\n"));
        assert!(text.contains("Default: 'first\nsecond'::text\n"));
        assert!(text.contains("Collation: \"MixedCase\".\"Collation\"\n"));
        assert!(text.contains("Nullable: No\n"));
        assert!(text.ends_with("Comment: First line\nSecond line 名"));
    }

    #[test]
    fn missing_values_are_distinct_from_empty_and_sql_null() {
        let column = ColumnMeta::new("value", DataType::Text);
        let text = column_details(&column);
        assert!(text.contains("Native type: Unavailable\n"));
        assert!(text.contains("Collation: Not reported\n"));
        assert!(text.contains("Default: Not reported\n"));
        assert!(text.ends_with("Comment: Not reported"));
        let text = column_details(&column.with_default("NULL").with_comment(""));
        assert!(text.contains("Default: NULL\n"));
        assert!(text.ends_with("Comment: "));
    }

    #[test]
    fn selection_resolves_exact_column_and_disappears_after_refresh() {
        use super::TableProperties;
        let mut model = TableProperties::new(
            db_wire::DbDriverKind::Sqlite,
            "main".into(),
            "items".into(),
            vec![
                ColumnMeta::new("Name", DataType::Text).with_comment("first"),
                ColumnMeta::new("name", DataType::Text).with_comment("second"),
            ],
            vec![],
            vec![],
        );
        assert!(
            model
                .selected_column_details("Name")
                .unwrap()
                .ends_with("first")
        );
        assert!(
            model
                .selected_column_details("name")
                .unwrap()
                .ends_with("second")
        );
        assert_eq!(model.selected_column_details("missing"), None);
        model.columns.clear();
        assert_eq!(model.selected_column_details("Name"), None);
    }
}
