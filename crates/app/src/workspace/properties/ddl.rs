use super::model::TableProperties;
use crate::workspace::Workspace;
use gpui::prelude::FluentBuilder;
use gpui::*;
use sql_intel::TokenKind;
use std::ops::Range;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_ddl_toolbar(
        &self,
        theme: &Theme,
        cx: &mut Context<Workspace>,
    ) -> AnyElement {
        let ddl_copy = self.ddl.clone();
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
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .items_center()
                    .gap_1p5()
                    .child(img("icons/sql_script.svg").w(px(13.0)).h(px(13.0)))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_secondary)
                            .child(
                                if self.details.as_ref().and_then(|d| d.ddl.as_ref()).is_some() {
                                    "Native DDL"
                                } else {
                                    "DDL Metadata Preview"
                                },
                            ),
                    ),
            )
            .child(
                div()
                    .id("copy-ddl-btn")
                    .flex_shrink_0()
                    .whitespace_nowrap()
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clipboard_request_id += 1;
                        cx.write_to_clipboard(ClipboardItem::new_string(ddl_copy.clone()));
                        cx.notify();
                    }))
                    .child("📋 Copy DDL"),
            )
            .w_full()
            .min_w_0()
            .into_any_element()
    }

    pub(in crate::workspace) fn render_ddl(&self, theme: &Theme, selected: bool) -> AnyElement {
        // Native DDL is prepared by the background loader. An unprepared preview
        // retains plain rendering while that request is pending.
        let characters = self.ddl_document.as_ref().map_or_else(
            || {
                self.ddl
                    .lines()
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(0)
            },
            |document| document.max_chars,
        );
        let lines: Box<dyn Iterator<Item = AnyElement> + '_> = match &self.ddl_document {
            Some(document) => Box::new(document.lines.iter().enumerate().map(|(idx, line)| {
                render_line(
                    idx,
                    &self.ddl[line.range.clone()],
                    &line.highlights,
                    selected,
                    theme,
                )
            })),
            None => Box::new(
                self.ddl
                    .lines()
                    .enumerate()
                    .map(|(idx, text)| render_line(idx, text, &[], selected, theme)),
            ),
        };
        const MONO_CHARACTER_WIDTH: f32 = 7.3;
        const GUTTER_AND_PADDING: f32 = 76.0;
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_full()
            .w(px(
                characters as f32 * MONO_CHARACTER_WIDTH + GUTTER_AND_PADDING
            ))
            .whitespace_nowrap()
            .bg(theme.bg_editor)
            .text_size(px(12.0))
            .line_height(px(18.0))
            .font_family(".AppleSystemUIFontMonospaced, Menlo, monospace")
            .text_color(theme.text_primary)
            .id("props-ddl-code-scroll")
            .py_2()
            .children(lines)
            .into_any_element()
    }
}

fn render_line(
    index: usize,
    text: &str,
    highlights: &[(Range<usize>, TokenKind)],
    selected: bool,
    theme: &Theme,
) -> AnyElement {
    let code = StyledText::new(text.to_owned()).with_highlights(
        highlights
            .iter()
            .map(|(range, kind)| (range.clone(), token_style(*kind, theme))),
    );
    div()
        .flex()
        .h(px(18.0))
        .flex_shrink_0()
        .min_w_full()
        .child(
            div()
                .w(px(36.0))
                .h_full()
                .flex_shrink_0()
                .pr_2()
                .text_right()
                .border_r_1()
                .border_color(theme.border)
                .text_color(theme.text_muted)
                .child((index + 1).to_string()),
        )
        .child(
            div()
                .pl_3()
                .when(selected, |line| line.bg(theme.bg_surface_selected))
                .child(code),
        )
        .into_any_element()
}

fn token_style(kind: TokenKind, theme: &Theme) -> HighlightStyle {
    let color = match kind {
        TokenKind::Keyword => theme.text_accent,
        TokenKind::Comment | TokenKind::StringLiteral => theme.text_success,
        TokenKind::NumberLiteral => theme.text_warning,
        TokenKind::Identifier => theme.text_primary,
        _ => theme.text_secondary,
    };
    HighlightStyle {
        color: Some(color.into()),
        font_weight: (kind == TokenKind::Keyword).then_some(FontWeight::SEMIBOLD),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::token_style;
    use sql_intel::TokenKind;
    use ui_core::theme::Theme;
    #[test]
    fn ddl_token_colors_follow_the_active_theme() {
        for theme in [Theme::dark(), Theme::light()] {
            assert_eq!(
                token_style(TokenKind::Keyword, &theme).color,
                Some(theme.text_accent.into())
            );
            assert_eq!(
                token_style(TokenKind::Comment, &theme).color,
                Some(theme.text_success.into())
            );
            assert_eq!(
                token_style(TokenKind::StringLiteral, &theme).color,
                Some(theme.text_success.into())
            );
            assert_eq!(
                token_style(TokenKind::NumberLiteral, &theme).color,
                Some(theme.text_warning.into())
            );
        }
    }
}
