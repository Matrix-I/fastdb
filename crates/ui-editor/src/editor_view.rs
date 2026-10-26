use std::rc::Rc;
use std::sync::Arc;
use gpui::*;
use sql_intel::{expects_table_alias, get_completions, pick_table_alias, CompletionItem, CompletionKind, SchemaCache, SqlParser};
use ui_core::theme::Theme;

use crate::buffer::RopeyBuffer;

const LINE_HEIGHT: f32 = 20.0;
const GUTTER_WIDTH: f32 = 48.0;
const MONO_FONT: &str = ".AppleSystemUIFontMonospaced, Menlo, monospace";
/// Approximate advance of one monospace char at 13 px, used to place the completion popup
const CHAR_WIDTH: f32 = 7.8;

/// Called with `(line, col)` when a char of the editor is clicked; `col` may be past the
/// end of the line (`usize::MAX` for a click right of the text)
pub type EditorPick = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

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
    /// Scroll position of the line list; the caret line is scrolled into view after it moves
    pub scroll_handle: ScrollHandle,
    /// Quote put around completed names that are not plain lowercase identifiers:
    /// `"` for PostgreSQL and SQLite, a backtick for MySQL
    pub identifier_quote: char,
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
            scroll_handle: ScrollHandle::new(),
            identifier_quote: '"',
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
        let mut start = self.word_start();
        let end = self.buffer.cursor_offset();
        let text = match item.kind {
            CompletionKind::Table | CompletionKind::Column if needs_quotes(&item.label) => {
                // An opening quote typed before the word is replaced, not doubled
                if self.buffer.text()[..start].ends_with(self.identifier_quote) {
                    start -= self.identifier_quote.len_utf8();
                }
                let q = self.identifier_quote;
                format!("{q}{}{q}", item.label.replace(q, &format!("{q}{q}")))
            }
            _ => item.label.clone(),
        };
        // Like DBeaver, a table picked after FROM or JOIN gets an alias for its columns
        let source = self.buffer.text();
        let text = if item.kind == CompletionKind::Table && expects_table_alias(&source, start) {
            format!("{text} {}", pick_table_alias(&source, &item.label))
        } else {
            text
        };
        self.buffer.replace_range(start, end, &text);
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

    pub fn render_editor(&mut self, focused: bool, caret_visible: bool, on_pick: EditorPick) -> impl IntoElement {
        let theme = self.theme.clone();
        let (cursor_line, cursor_col) = self.buffer.cursor_line_col();
        if std::mem::take(&mut self.caret_moved) {
            self.scroll_handle.scroll_to_item(cursor_line);
        }

        let mut lines = div()
            .id("sql-editor-lines")
            .flex()
            .flex_col()
            .size_full()
            .py_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle);

        for l in 0..self.buffer.line_count().max(1) {
            let content = self.buffer.line(l).unwrap_or_default();
            let content = content.trim_end_matches(['\n', '\r']);
            let caret_col = (focused && l == cursor_line).then_some(cursor_col);
            lines = lines.child(render_line(l, content, caret_col, caret_visible, &on_pick, &theme));
        }

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_editor)
            .text_color(theme.text_primary)
            .font_family(MONO_FONT)
            .text_size(px(13.0))
            .overflow_hidden()
            .child(lines);

        if focused && self.show_completions && !self.completion_items.is_empty() {
            let scroll_y = f32::from(self.scroll_handle.offset().y);
            let top = (cursor_line as f32 + 1.0) * LINE_HEIGHT + 4.0 + scroll_y;
            let left = GUTTER_WIDTH + 12.0 + cursor_col as f32 * CHAR_WIDTH;
            root = root.child(self.render_completion_popup(top, left, &theme));
        }
        root
    }

    fn render_completion_popup(&self, top: f32, left: f32, theme: &Theme) -> impl IntoElement {
        let mut popup = div()
            .occlude()
            .w(px(280.0))
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .rounded_md()
            .shadow_lg()
            .p_1()
            .flex()
            .flex_col()
            .gap_1();

        for (idx, item) in self.completion_items.iter().take(MAX_VISIBLE_COMPLETIONS).enumerate() {
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
        // Painted after everything else (like the filter bar's list), so the rest of the
        // window cannot cover it
        div()
            .absolute()
            .top(px(top))
            .left(px(left))
            .child(deferred(anchored().snap_to_window().child(popup)).with_priority(1))
    }
}

/// Whether a table or column name must be quoted to keep its case and characters:
/// anything but a lowercase letter or `_` followed by lowercase letters, digits and `_`
fn needs_quotes(name: &str) -> bool {
    let mut chars = name.chars();
    let starts_plain = chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    !(starts_plain && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// Colour of a word in the simple keyword highlighter
fn word_color(word: &str, theme: &Theme) -> Hsla {
    let trimmed = word.trim().to_uppercase();
    match trimmed.as_str() {
        "SELECT" | "FROM" | "WHERE" | "JOIN" | "INNER" | "LEFT" | "RIGHT"
        | "ON" | "GROUP" | "BY" | "ORDER" | "LIMIT" | "OFFSET" | "INSERT"
        | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE" | "AS" | "AND"
        | "OR" | "NOT" | "IN" | "IS" | "NULL" | "WITH" | "CREATE" | "TABLE"
        | "DROP" | "ALTER" | "HAVING" | "DISTINCT" | "UNION" | "CASE" | "WHEN"
        | "THEN" | "ELSE" | "END" => theme.text_accent.into(),
        _ if word.starts_with('\'') || word.ends_with('\'') => theme.text_success.into(),
        _ if trimmed.trim_end_matches([',', ';', ')']).parse::<f64>().is_ok() => theme.text_warning.into(),
        _ if word.trim_start().starts_with("--") => theme.text_muted.into(),
        _ => theme.text_primary.into(),
    }
}

/// Thin caret bar drawn between two chars; transparent while the caret blinks off
fn caret_bar(visible: bool) -> AnyElement {
    div()
        .relative()
        .w(px(0.0))
        .h(px(16.0))
        .child(
            div()
                .absolute()
                .top_0()
                .left(px(-0.75))
                .w(px(1.5))
                .h_full()
                .rounded_xs()
                .bg(if visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() }),
        )
        .into_any_element()
}

/// One editor line: its number in the gutter, then one cell per char. Clicking the left or
/// right half of a char reports the caret position before or after it through `on_pick`.
fn render_line(
    line: usize,
    content: &str,
    caret_col: Option<usize>,
    caret_visible: bool,
    on_pick: &EditorPick,
    theme: &Theme,
) -> AnyElement {
    let half = |col: usize, right: bool| {
        let pick = on_pick.clone();
        let el = div().absolute().top_0().bottom_0().w(relative(0.5));
        let el = if right { el.right_0() } else { el.left_0() };
        el.on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            pick(line, col, window, cx);
        })
    };

    let mut code = div().flex().flex_1().h_full().items_center().pl_3().overflow_hidden();
    if caret_col.is_some() {
        code = code.bg(theme.bg_surface_hover);
    }

    // A `--` comment colours the rest of the line
    let mut col = 0;
    let mut in_comment = false;
    for word in content.split_inclusive(' ') {
        in_comment = in_comment || word.trim_start().starts_with("--");
        let color: Hsla = if in_comment { theme.text_muted.into() } else { word_color(word, theme) };
        for ch in word.chars() {
            if caret_col == Some(col) {
                code = code.child(caret_bar(caret_visible));
            }
            // A lone space shapes to zero width, so render it as a non-breaking space
            let glyph = if ch == ' ' { '\u{00a0}' } else { ch };
            code = code.child(
                div()
                    .relative()
                    .flex_none()
                    .text_color(color)
                    .child(glyph.to_string())
                    .child(half(col, false))
                    .child(half(col + 1, true)),
            );
            col += 1;
        }
    }
    if caret_col.is_some_and(|c| c >= col) {
        code = code.child(caret_bar(caret_visible));
    }
    // Clicking right of the text puts the caret at the end of the line
    let pick = on_pick.clone();
    code = code.child(div().flex_1().h_full().on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        pick(line, usize::MAX, window, cx);
    }));

    div()
        .flex()
        .flex_none()
        .w_full()
        .h(px(LINE_HEIGHT))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .w(px(GUTTER_WIDTH))
                .h_full()
                .pr_2()
                .bg(theme.bg_surface)
                .border_r_1()
                .border_color(theme.border)
                .text_size(px(12.0))
                .text_color(if caret_col.is_some() { theme.text_accent } else { theme.text_muted })
                .child(format!("{}", line + 1)),
        )
        .child(code)
        .into_any_element()
}

impl Render for SqlEditorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_editor(true, true, Rc::new(|_, _, _, _| {}))
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
        assert_eq!(ed.buffer.text(), "SELECT * FROM customers c");
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

    fn mixed_case_editor() -> SqlEditorView {
        let cache = SchemaCache::new().unwrap();
        cache.add_table("public", "apiClient", "BASE TABLE").unwrap();
        cache.add_table("public", "users", "BASE TABLE").unwrap();
        cache.add_column("public", "apiClient", "clientId", "varchar", false).unwrap();
        SqlEditorView::new("", Theme::dark(), Some(Arc::new(cache)))
    }

    fn accept(ed: &mut SqlEditorView, label: &str) {
        ed.selected_completion = ed.completion_items.iter().position(|i| i.label == label).unwrap();
        assert!(ed.handle_key(&key("tab")));
    }

    #[test]
    fn mixed_case_names_are_quoted_when_completed() {
        let mut ed = mixed_case_editor();
        type_str(&mut ed, "SELECT \"api");
        accept(&mut ed, "apiClient");
        assert_eq!(ed.buffer.text(), "SELECT \"apiClient\"");

        type_str(&mut ed, ".");
        accept(&mut ed, "clientId");
        assert_eq!(ed.buffer.text(), "SELECT \"apiClient\".\"clientId\"");
    }

    #[test]
    fn tables_after_from_or_join_get_an_alias_used_by_column_completion() {
        let mut ed = mixed_case_editor();
        type_str(&mut ed, "SELECT * FROM api");
        accept(&mut ed, "apiClient");
        assert_eq!(ed.buffer.text(), "SELECT * FROM \"apiClient\" a");

        type_str(&mut ed, " JOIN us");
        accept(&mut ed, "users");
        assert_eq!(ed.buffer.text(), "SELECT * FROM \"apiClient\" a JOIN users u");

        type_str(&mut ed, " WHERE a.");
        accept(&mut ed, "clientId");
        assert_eq!(ed.buffer.text(), "SELECT * FROM \"apiClient\" a JOIN users u WHERE a.\"clientId\"");
    }

    #[test]
    fn tables_outside_from_or_join_get_no_alias() {
        let mut ed = mixed_case_editor();
        type_str(&mut ed, "UPDATE us");
        accept(&mut ed, "users");
        assert_eq!(ed.buffer.text(), "UPDATE users");
    }

    #[test]
    fn plain_lowercase_names_and_keywords_stay_unquoted() {
        let mut ed = mixed_case_editor();
        type_str(&mut ed, "SEL");
        accept(&mut ed, "SELECT");
        type_str(&mut ed, " * FROM us");
        accept(&mut ed, "users");
        assert_eq!(ed.buffer.text(), "SELECT * FROM users u");
    }

    #[test]
    fn an_opening_quote_already_typed_is_not_doubled() {
        let mut ed = mixed_case_editor();
        ed.identifier_quote = '`';
        type_str(&mut ed, "SELECT * FROM `api");
        accept(&mut ed, "apiClient");
        assert_eq!(ed.buffer.text(), "SELECT * FROM `apiClient` a");
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
