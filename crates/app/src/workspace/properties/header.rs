use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_header(&self, theme: &Theme) -> AnyElement {
        let t_name = self.table.as_str();
        let details = self.details.as_ref();
        let owner_name = details
            .and_then(|d| d.owner.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let table_oid = details
            .and_then(|d| d.object_id.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let tablespace = details
            .and_then(|d| d.tablespace.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let comment = details
            .map(|d| d.comment.clone().unwrap_or_else(|| "None".into()))
            .unwrap_or_else(|| "Unavailable".into());
        let partition_key = details
            .and_then(|d| d.partition_key.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let flags = self.metadata_error.clone().unwrap_or_else(|| {
            let flag = |v| match v {
                Some(true) => "Yes",
                Some(false) => "No",
                None => "Unavailable",
            };
            format!(
                "RLS: {} / Partitioned: {}",
                flag(details.and_then(|d| d.row_security)),
                flag(details.and_then(|d| d.partitioned))
            )
        });
        let top_form = div()
            .id("props-top-form")
            .flex()
            .flex_col()
            .px_4()
            .py_2p5()
            .gap_2()
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            // Row 1
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    // Table Name
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(75.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Table Name:"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .w(px(250.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded_xs()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_primary)
                                            .child(t_name.to_string()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(theme.text_muted)
                                            .child("⤢"),
                                    ),
                            ),
                    )
                    // Tablespace
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(70.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Tablespace:"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .w(px(130.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(rgb(0x19324d))
                                    .border_1()
                                    .border_color(rgb(0x388bfd))
                                    .rounded_xs()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(0xffffff))
                                            .child(tablespace),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(rgb(0x58a6ff))
                                            .child("▾"),
                                    ),
                            ),
                    )
                    // Object ID
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(65.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Object ID:"),
                            )
                            .child(
                                div()
                                    .w(px(130.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded_xs()
                                    .text_size(px(11.5))
                                    .text_color(theme.text_primary)
                                    .child(table_oid),
                            ),
                    ),
            )
            // Row 2
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    // Comment
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(75.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Comment:"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .w(px(250.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded_xs()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_primary)
                                            .child(comment),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(theme.text_muted)
                                            .child("⤡"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child(flags),
                    )
                    // Owner
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(65.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Owner:"),
                            )
                            .child(
                                div()
                                    .w(px(130.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .text_size(px(11.5))
                                    .text_color(rgb(0x58a6ff))
                                    .cursor_pointer()
                                    .child(owner_name),
                            ),
                    ),
            )
            // Row 3
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    .child(div().w(px(331.0)))
                    // Partition by
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w(px(240.0))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Partition by:"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded_xs()
                                    .text_size(px(11.5))
                                    .text_color(theme.text_primary)
                                    .child(partition_key),
                            ),
                    )
                    // Extra Options
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(80.0))
                                    .text_size(px(11.5))
                                    .text_color(theme.text_secondary)
                                    .child("Extra Options:"),
                            )
                            .child(
                                div()
                                    .w(px(115.0))
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(theme.bg_editor)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded_xs(),
                            ),
                    ),
            );

        top_form.into_any_element()
    }
}
