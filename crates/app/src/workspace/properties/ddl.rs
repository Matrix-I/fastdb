use super::model::TableProperties;
use crate::workspace::Workspace;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_ddl(
        &self,
        theme: &Theme,
        cx: &mut Context<Workspace>,
    ) -> AnyElement {
        let ddl_code = self.ddl.clone();
        let ddl_copy = ddl_code.clone();
        // Reserve the intrinsic width of the monospaced lines inside the shared viewport.
        let code_width = ddl_code
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f32
            * 7.3
            + 76.0;

        let ddl_view = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_full()
            .w(px(code_width))
            .whitespace_nowrap()
            .bg(theme.bg_editor)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(28.0))
                    .flex_shrink_0()
                    .px_3()
                    .bg(theme.bg_surface)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(img("icons/sql_script.svg").w(px(13.0)).h(px(13.0)))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_secondary)
                                    .child("DDL Metadata Preview"),
                            ),
                    )
                    .child(
                        div()
                            .id("copy-ddl-btn")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded_xs()
                            .bg(theme.bg_editor)
                            .border_1()
                            .border_color(theme.border)
                            .text_size(px(11.0))
                            .text_color(theme.text_primary)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(ddl_copy.clone()));
                                cx.notify();
                            }))
                            .child("📋 Copy DDL"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .p_3()
                    .text_size(px(12.0))
                    .line_height(px(18.0))
                    .font_family(".AppleSystemUIFontMonospaced, Menlo, monospace")
                    .text_color(rgb(0x9cdcfe))
                    .id("props-ddl-code-scroll")
                    .child(div().flex().flex_col().gap_0p5().children(
                        ddl_code.lines().enumerate().map(|(idx, line)| {
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .w(px(28.0))
                                        .flex_shrink_0()
                                        .text_color(theme.text_muted)
                                        .text_size(px(11.0))
                                        .child(format!("{}", idx + 1)),
                                )
                                .child(
                                    div()
                                        .text_color(if line.starts_with("--") {
                                            theme.text_muted
                                        } else if line.starts_with("CREATE")
                                            || line.starts_with(");")
                                        {
                                            rgb(0xc586c0)
                                        } else if line.contains("CONSTRAINT")
                                            || line.contains("PRIMARY KEY")
                                        {
                                            rgb(0x4ec9b0)
                                        } else {
                                            rgb(0x9cdcfe)
                                        })
                                        .child(line.to_string()),
                                )
                        }),
                    )),
            );

        ddl_view.into_any_element()
    }
}
