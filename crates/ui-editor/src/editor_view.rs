use std::sync::Arc;
use gpui::*;
use sql_intel::{get_completions, CompletionItem, CompletionKind, SchemaCache, SqlParser};
use ui_core::theme::Theme;

use crate::buffer::RopeyBuffer;

pub struct SqlEditorView {
    pub buffer: RopeyBuffer,
    pub parser: Option<SqlParser>,
    pub theme: Theme,
    pub schema_cache: Option<Arc<SchemaCache>>,
    pub completion_items: Vec<CompletionItem>,
    pub selected_completion: usize,
    pub show_completions: bool,
}

impl SqlEditorView {
    pub fn new(initial_code: &str, theme: Theme, schema_cache: Option<Arc<SchemaCache>>) -> Self {
        Self {
            buffer: RopeyBuffer::new(initial_code),
            parser: SqlParser::new(),
            theme,
            schema_cache,
            completion_items: Vec::new(),
            selected_completion: 0,
            show_completions: false,
        }
    }

    /// Returns the SQL statement at the current cursor offset
    pub fn active_statement(&mut self) -> String {
        let text = self.buffer.text();
        let offset = self.buffer.cursor_offset();

        if let Some(ref mut parser) = self.parser {
            if let Some((_, _, stmt)) = parser.statement_at_offset(&text, offset) {
                let trimmed = stmt.trim();
                if !trimmed.trim_matches(';').trim().is_empty() {
                    return trimmed.to_string();
                }
            }
        }

        let trimmed_text = text.trim();
        if !trimmed_text.trim_matches(';').trim().is_empty() {
            trimmed_text.to_string()
        } else {
            String::new()
        }
    }

    pub fn insert_text(&mut self, s: &str) {
        self.buffer.insert_str(s);
        self.update_completions();
    }

    pub fn backspace(&mut self) {
        self.buffer.backspace();
        self.update_completions();
    }

    pub fn update_completions(&mut self) {
        if let Some(ref cache) = self.schema_cache {
            let text = self.buffer.text();
            let offset = self.buffer.cursor_offset();
            let items = get_completions(&text, offset, cache);

            if !items.is_empty() {
                self.completion_items = items;
                self.selected_completion = 0;
                self.show_completions = true;
                return;
            }
        }
        self.show_completions = false;
    }

    pub fn apply_selected_completion(&mut self) {
        if !self.show_completions || self.completion_items.is_empty() {
            return;
        }

        let item = &self.completion_items[self.selected_completion];
        self.buffer.insert_str(&item.label);
        self.show_completions = false;
    }

    pub fn render_editor(&mut self) -> impl IntoElement {
        let theme = &self.theme;
        let (cursor_line, _cursor_col) = self.buffer.cursor_line_col();

        let line_count = self.buffer.line_count().max(1);
        let gutter_width = 48.0;

        let mut editor_container = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_editor)
            .text_color(theme.text_primary)
            .overflow_hidden();

        // Editor Body: Gutter + Code Lines
        let mut body = div()
            .flex()
            .flex_1()
            .size_full()
            .overflow_hidden();

        // 1. Line Number Gutter
        let mut gutter = div()
            .flex()
            .flex_col()
            .w(px(gutter_width))
            .h_full()
            .bg(theme.bg_surface)
            .border_r_1()
            .border_color(theme.border)
            .py_2();

        for l in 0..line_count {
            let is_current = l == cursor_line;
            gutter = gutter.child(
                div()
                    .h(px(22.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_end()
                    .text_size(px(12.0))
                    .text_color(if is_current { theme.text_accent } else { theme.text_muted })
                    .child(format!("{}", l + 1))
            );
        }

        body = body.child(gutter);

        // 2. Code Area with Syntax Highlighting
        let mut code_area = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .px_3()
            .py_2()
            .overflow_hidden();

        // Line-by-line render
        for l in 0..line_count {
            let line_content = self.buffer.line(l).unwrap_or_default();
            let is_current = l == cursor_line;

            let mut line_div = div()
                .flex()
                .items_center()
                .h(px(22.0))
                .w_full()
                .text_size(px(13.0));

            if is_current {
                line_div = line_div.bg(theme.bg_surface_hover);
            }

            // Simple fast keyword coloring for display
            let words: Vec<&str> = line_content.split_inclusive(' ').collect();
            for word in words {
                let trimmed = word.trim().to_uppercase();
                let color = match trimmed.as_str() {
                    "SELECT" | "FROM" | "WHERE" | "JOIN" | "INNER" | "LEFT" | "RIGHT"
                    | "ON" | "GROUP" | "BY" | "ORDER" | "LIMIT" | "OFFSET" | "INSERT"
                    | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE" | "AS" | "AND"
                    | "OR" | "NOT" | "IN" | "IS" | "NULL" => theme.text_accent,
                    _ if word.starts_with('\'') || word.ends_with('\'') => theme.text_success,
                    _ if word.parse::<f64>().is_ok() => theme.text_warning,
                    _ if word.starts_with("--") => theme.text_muted,
                    _ => theme.text_primary,
                };

                line_div = line_div.child(
                    div()
                        .text_color(color)
                        .child(word.to_string())
                );
            }

            code_area = code_area.child(line_div);
        }

        body = body.child(code_area);
        editor_container = editor_container.child(body);

        // 3. Autocomplete Popup
        if self.show_completions && !self.completion_items.is_empty() {
            let mut popup = div()
                .absolute()
                .top(px(100.0))
                .left(px(gutter_width + 80.0))
                .w(px(280.0))
                .max_h(px(200.0))
                .bg(theme.bg_surface)
                .border_1()
                .border_color(theme.border_focused)
                .rounded_md()
                .shadow_lg()
                .p_1()
                .flex()
                .flex_col()
                .gap_1();

            for (idx, item) in self.completion_items.iter().take(6).enumerate() {
                let is_sel = idx == self.selected_completion;
                let bg = if is_sel { theme.bg_surface_selected } else { theme.bg_surface };

                let icon = match item.kind {
                    CompletionKind::Keyword => "🔑",
                    CompletionKind::Table => "📋",
                    CompletionKind::Column => "🔹",
                    CompletionKind::Function => "⚡",
                };

                popup = popup.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(bg)
                        .text_size(px(12.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icon)
                                .child(
                                    div()
                                        .font_weight(if is_sel { FontWeight::BOLD } else { FontWeight::NORMAL })
                                        .child(item.label.clone())
                                )
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(theme.text_muted)
                                .child(item.detail.clone())
                        )
                );
            }

            editor_container = editor_container.child(popup);
        }

        editor_container
    }
}

impl Render for SqlEditorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_editor()
    }
}
