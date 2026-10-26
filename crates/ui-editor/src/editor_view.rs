use std::sync::Arc;
use gpui::*;
use sql_intel::{get_completions, CompletionItem, CompletionKind, SchemaCache, SqlParser};
use ui_core::theme::Theme;

use crate::buffer::RopeyBuffer;

/// Completion entries shown in the popup; the selection wraps within them
pub const MAX_VISIBLE_COMPLETIONS: usize = 6;

pub struct SqlEditorView {
    pub buffer: RopeyBuffer,
    pub parser: Option<SqlParser>,
    pub theme: Theme,
    pub schema_cache: Option<Arc<SchemaCache>>,
    pub completion_items: Vec<CompletionItem>,
    pub selected_completion: usize,
    pub show_completions: bool,
    /// Set when the caret moved since the last render, which scrolls it into view
    pub caret_moved: bool,
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
            caret_moved: true,
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
        self.caret_moved = true;
        self.update_completions();
    }

    pub fn backspace(&mut self) {
        self.buffer.backspace();
        self.caret_moved = true;
        self.update_completions();
    }

    /// Cmd+Z: takes back the last edit. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.hide_completions();
        self.caret_moved = true;
        self.buffer.undo()
    }

    /// Cmd+Shift+Z: puts back the last undone edit. Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        self.hide_completions();
        self.caret_moved = true;
        self.buffer.redo()
    }

    /// Byte offset where the identifier ending at the caret starts
    fn word_start(&self) -> usize {
        let text = self.buffer.text();
        let offset = self.buffer.cursor_offset();
        text[..offset]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(offset, |(i, _)| i)
    }

    /// Refreshes the popup; it only opens while a word is typed or right after a `.`
    pub fn update_completions(&mut self) {
        let text = self.buffer.text();
        let offset = self.buffer.cursor_offset();
        let after_dot = text[..offset].ends_with('.');
        if self.word_start() == offset && !after_dot {
            self.hide_completions();
            return;
        }
        if let Some(ref cache) = self.schema_cache {
            let items = get_completions(&text, offset, cache);
            if !items.is_empty() {
                self.completion_items = items;
                self.selected_completion = 0;
                self.show_completions = true;
                return;
            }
        }
        self.hide_completions();
    }

    pub fn hide_completions(&mut self) {
        self.show_completions = false;
    }

    fn visible_completions(&self) -> usize {
        self.completion_items.len().min(MAX_VISIBLE_COMPLETIONS)
    }

    pub fn select_next_completion(&mut self) {
        let n = self.visible_completions();
        if n > 0 {
            self.selected_completion = (self.selected_completion + 1) % n;
        }
    }

    pub fn select_prev_completion(&mut self) {
        let n = self.visible_completions();
        if n > 0 {
            self.selected_completion = (self.selected_completion + n - 1) % n;
        }
    }

    /// Replaces the word before the caret with the selected entry. Returns `false` when
    /// the popup is closed.
    pub fn apply_selected_completion(&mut self) -> bool {
        if !self.show_completions {
            return false;
        }
        let Some(item) = self.completion_items.get(self.selected_completion).cloned() else {
            return false;
        };
        let start = self.word_start();
        let end = self.buffer.cursor_offset();
        self.buffer.replace_range(start, end, &item.label);
        self.caret_moved = true;
        self.hide_completions();
        true
    }

    /// Handles an editing or navigation key. Returns `false` for keys the caller owns
    /// (clipboard, undo, run, app shortcuts).
    pub fn handle_key(&mut self, keystroke: &Keystroke) -> bool {
        let mods = &keystroke.modifiers;
        let cmd = mods.platform || mods.control;
        let key = keystroke.key.as_str();

        if self.show_completions {
            match key {
                "up" => {
                    self.select_prev_completion();
                    return true;
                }
                "down" => {
                    self.select_next_completion();
                    return true;
                }
                "enter" | "return" | "tab" if !cmd => return self.apply_selected_completion(),
                "escape" => {
                    self.hide_completions();
                    return true;
                }
                _ => {}
            }
        }

        match key {
            "left" if cmd => self.buffer.move_line_start(),
            "right" if cmd => self.buffer.move_line_end(),
            "up" if cmd => self.buffer.move_to_start(),
            "down" if cmd => self.buffer.move_to_end(),
            "left" if mods.alt => self.buffer.move_word_left(),
            "right" if mods.alt => self.buffer.move_word_right(),
            "left" => self.buffer.move_left(),
            "right" => self.buffer.move_right(),
            "up" => self.buffer.move_up(),
            "down" => self.buffer.move_down(),
            "home" => self.buffer.move_line_start(),
            "end" => self.buffer.move_line_end(),
            _ => return self.handle_edit_key(keystroke, cmd),
        }
        self.caret_moved = true;
        self.hide_completions();
        true
    }

    /// Keys that change the text; see [`Self::handle_key`]
    fn handle_edit_key(&mut self, keystroke: &Keystroke, cmd: bool) -> bool {
        if cmd {
            return false;
        }
        let key = keystroke.key.as_str();
        match key {
            "backspace" => self.backspace(),
            "delete" => {
                self.buffer.delete_forward();
                self.caret_moved = true;
                self.hide_completions();
            }
            "enter" | "return" => self.insert_text("\n"),
            "tab" => self.insert_text("    "),
            "space" => self.insert_text(" "),
            "escape" => self.hide_completions(),
            _ => {
                let typed = keystroke.key_char.as_deref().or((key.chars().count() == 1).then_some(key));
                match typed {
                    Some(s) if s.chars().count() == 1 => self.insert_text(s),
                    _ => return false,
                }
            }
        }
        true
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

                let icon_path = match item.kind {
                    CompletionKind::Keyword => "icons/sql.svg",
                    CompletionKind::Table => "icons/table.svg",
                    CompletionKind::Column => "icons/column.svg",
                    CompletionKind::Function => "icons/function.svg",
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
                                .child(img(icon_path).w(px(14.0)).h(px(14.0)))
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

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{SqlEditorView, MAX_VISIBLE_COMPLETIONS};
    use gpui::{Keystroke, Modifiers};
    use sql_intel::SchemaCache;
    use std::sync::Arc;
    use ui_core::theme::Theme;

    fn key(key: &str) -> Keystroke {
        Keystroke { modifiers: Modifiers::default(), key: key.into(), key_char: None }
    }

    fn typed(ch: &str) -> Keystroke {
        Keystroke { modifiers: Modifiers::default(), key: ch.into(), key_char: Some(ch.into()) }
    }

    fn cmd(k: &str) -> Keystroke {
        Keystroke { modifiers: Modifiers { platform: true, ..Default::default() }, key: k.into(), key_char: None }
    }

    fn editor(text: &str) -> SqlEditorView {
        let cache = SchemaCache::new().unwrap();
        cache.add_table("public", "customers", "BASE TABLE").unwrap();
        cache.add_column("public", "customers", "email", "varchar", false).unwrap();
        SqlEditorView::new(text, Theme::dark(), Some(Arc::new(cache)))
    }

    fn type_str(ed: &mut SqlEditorView, s: &str) {
        for ch in s.chars() {
            let k = if ch == ' ' { key("space") } else { typed(&ch.to_string()) };
            assert!(ed.handle_key(&k));
        }
    }

    #[test]
    fn accepting_a_completion_replaces_the_typed_word() {
        let mut ed = editor("");
        type_str(&mut ed, "SELECT * FROM cust");
        assert!(ed.show_completions);
        let idx = ed.completion_items.iter().position(|i| i.label == "customers").unwrap();
        ed.selected_completion = idx;
        assert!(ed.handle_key(&key("tab")));
        assert_eq!(ed.buffer.text(), "SELECT * FROM customers");
        assert!(!ed.show_completions);
    }

    #[test]
    fn whitespace_does_not_open_the_completion_list() {
        let mut ed = editor("");
        type_str(&mut ed, "SELECT ");
        assert!(!ed.show_completions);
        assert!(ed.handle_key(&key("enter")));
        assert!(!ed.show_completions);
        assert_eq!(ed.buffer.text(), "SELECT \n");
    }

    #[test]
    fn a_dot_opens_column_completion() {
        let mut ed = editor("");
        type_str(&mut ed, "SELECT customers.");
        assert!(ed.show_completions);
        ed.selected_completion = ed.completion_items.iter().position(|i| i.label == "email").unwrap();
        assert!(ed.handle_key(&key("enter")));
        assert_eq!(ed.buffer.text(), "SELECT customers.email");
    }

    #[test]
    fn arrows_navigate_the_open_list_and_escape_closes_it() {
        let mut ed = editor("");
        type_str(&mut ed, "S");
        assert!(ed.show_completions);
        assert!(ed.handle_key(&key("down")));
        assert_eq!(ed.selected_completion, 1);
        assert!(ed.handle_key(&key("up")));
        assert!(ed.handle_key(&key("up")));
        let visible = ed.completion_items.len().min(MAX_VISIBLE_COMPLETIONS);
        assert_eq!(ed.selected_completion, visible - 1);
        assert!(ed.handle_key(&key("escape")));
        assert!(!ed.show_completions);
        assert_eq!(ed.buffer.text(), "S");
    }

    #[test]
    fn arrows_move_the_caret_when_the_list_is_closed() {
        let mut ed = editor("SELECT 1;\nSELECT 2;");
        ed.buffer.set_cursor_line_col(1, 3);
        ed.caret_moved = false;
        assert!(ed.handle_key(&key("up")));
        assert_eq!(ed.buffer.cursor_line_col(), (0, 3));
        assert!(ed.caret_moved);
        assert!(ed.handle_key(&cmd("right")));
        assert_eq!(ed.buffer.cursor_line_col(), (0, 9));
    }

    #[test]
    fn app_shortcuts_are_left_to_the_caller() {
        let mut ed = editor("SELECT 1");
        assert!(!ed.handle_key(&cmd("v")));
        assert!(!ed.handle_key(&cmd("enter")));
        assert_eq!(ed.buffer.text(), "SELECT 1");
    }

    #[test]
    fn active_statement_follows_the_caret() {
        let mut ed = editor("SELECT 1;\nSELECT 2;");
        ed.buffer.set_cursor_line_col(0, 2);
        assert_eq!(ed.active_statement(), "SELECT 1;");
        ed.buffer.set_cursor_line_col(1, 2);
        assert_eq!(ed.active_statement(), "SELECT 2;");
    }
}
