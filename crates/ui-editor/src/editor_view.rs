use gpui::*;
use sql_intel::{
    CompletionItem, CompletionKind, SchemaCache, SqlParser, expects_table_alias, get_completions,
    pick_table_alias,
};
use std::rc::Rc;
use std::sync::Arc;
use ui_core::theme::Theme;

use crate::buffer::RopeyBuffer;
pub use crate::line_input::PickKind as EditorPickKind;
use crate::line_input::{PickAction, pick_action};
use crate::text_edit::{TextClipboard, TextCommand, TextTarget, dispatch};

use crate::layout::{
    CODE_PADDING, EditorLayout, GUTTER_WIDTH, LINE_HEIGHT, MONO_FONT, VERTICAL_PADDING, reveal_axis,
};

/// Reports mouse presses and drags at `(line, col)`; `col` may be past the
/// end of the line (`usize::MAX` for a click right of the text).
pub type EditorPick = Rc<dyn Fn(usize, usize, EditorPickKind, &mut Window, &mut App)>;

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
    layout: EditorLayout,
    line_drag_anchor: Option<usize>,
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
            layout: EditorLayout::default(),
            line_drag_anchor: None,
        }
    }

    pub fn select_all(&mut self) {
        self.line_drag_anchor = None;
        self.buffer.select_all();
        self.hide_completions();
        // Keep the selected text under the user's eyes rather than jumping to EOF.
        self.caret_moved = false;
    }

    pub fn pick(&mut self, line: usize, col: usize, kind: EditorPickKind) {
        match pick_action(kind, true, self.line_drag_anchor.is_some()) {
            PickAction::SelectUnit => {
                let line = line.min(self.buffer.line_count().saturating_sub(1));
                self.line_drag_anchor = Some(line);
                self.select_lines(line, line);
                self.caret_moved = false;
            }
            PickAction::ExtendUnit => {
                if let Some(anchor) = self.line_drag_anchor {
                    self.select_lines(anchor, line);
                }
                self.caret_moved = false;
            }
            PickAction::Move { extend } => {
                self.line_drag_anchor = None;
                self.buffer
                    .move_selection(extend, |buffer| buffer.set_cursor_line_col(line, col));
                self.caret_moved = true;
            }
            PickAction::Ignore => {}
        }
        self.hide_completions();
    }

    fn select_lines(&mut self, anchor: usize, focus: usize) {
        let last_line = self.buffer.line_count().saturating_sub(1);
        let first = anchor.min(focus).min(last_line);
        let last = anchor.max(focus).min(last_line);
        let start = self.buffer.line_start_offset(first);
        let end = if last < last_line {
            self.buffer.line_start_offset(last + 1)
        } else {
            self.buffer.len()
        };
        self.buffer
            .move_selection(false, |buffer| buffer.set_cursor_offset(start));
        self.buffer
            .move_selection(true, |buffer| buffer.set_cursor_offset(end));
    }

    /// Returns the SQL statement at the current cursor offset
    pub fn active_statement(&mut self) -> String {
        let text = self.buffer.text();
        let offset = self.buffer.cursor_offset();
        if let Some(selected) = self.buffer.selected_text() {
            return selected;
        }

        if let Some(ref mut parser) = self.parser
            && let Some((_, _, stmt)) = parser.statement_at_offset(&text, offset)
        {
            let trimmed = stmt.trim();
            if !trimmed.trim_matches(';').trim().is_empty() {
                return trimmed.to_string();
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
        self.line_drag_anchor = None;
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
        self.line_drag_anchor = None;
        let mods = &keystroke.modifiers;
        let cmd = mods.platform || mods.control;
        let key = keystroke.key.as_str();
        if self.show_completions && !mods.shift && !cmd && !mods.alt {
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
                _ => {}
            }
        }

        let Some(command) = TextCommand::from_key(keystroke, true) else {
            return false;
        };
        if matches!(
            command,
            TextCommand::Copy
                | TextCommand::Cut
                | TextCommand::Paste
                | TextCommand::Undo
                | TextCommand::Redo
        ) {
            return false;
        }
        struct NoClipboard;
        impl TextClipboard for NoClipboard {
            fn read(&mut self) -> Option<String> {
                None
            }
            fn write(&mut self, _: String) {}
        }
        dispatch(self, &command, &mut NoClipboard);
        true
    }

    /// Shared editing entry point with access to the system clipboard.
    pub fn handle_input_key(&mut self, key: &Keystroke, cx: &mut App) -> bool {
        if self.handle_key(key) {
            return true;
        }
        let Some(command) = TextCommand::from_key(key, true) else {
            return false;
        };
        dispatch(self, &command, cx);
        true
    }

    fn navigate(&mut self, key: &str, mods: &Modifiers) {
        let cmd = mods.platform || mods.control;
        if !mods.shift
            && matches!(key, "left" | "right")
            && !cmd
            && !mods.alt
            && let Some(range) = self.buffer.selection()
        {
            self.buffer.set_cursor_offset(if key == "left" {
                range.start
            } else {
                range.end
            });
            self.caret_moved = true;
            self.hide_completions();
            return;
        }
        self.buffer.move_selection(mods.shift, |buffer| match key {
            "left" if cmd => buffer.move_line_start(),
            "right" if cmd => buffer.move_line_end(),
            "up" if cmd => buffer.move_to_start(),
            "down" if cmd => buffer.move_to_end(),
            "left" if mods.alt => buffer.move_word_left(),
            "right" if mods.alt => buffer.move_word_right(),
            "left" => buffer.move_left(),
            "right" => buffer.move_right(),
            "up" => buffer.move_up(),
            "down" => buffer.move_down(),
            "home" => buffer.move_line_start(),
            "end" => buffer.move_line_end(),
            _ => unreachable!(),
        });
        self.caret_moved = true;
        self.hide_completions();
    }

    pub fn render_editor(
        &mut self,
        focused: bool,
        caret_visible: bool,
        on_pick: EditorPick,
        window: &mut Window,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let (cursor_line, cursor_col) = self.buffer.cursor_line_col();
        self.layout.update(&self.buffer, window);
        let content_width = self.layout.content_width();
        if std::mem::take(&mut self.caret_moved) {
            let viewport = self.scroll_handle.bounds().size;
            let offset = self.scroll_handle.offset();
            let x = GUTTER_WIDTH
                + CODE_PADDING
                + self
                    .layout
                    .prefix_width(&self.buffer, cursor_line, cursor_col);
            let y = VERTICAL_PADDING + cursor_line as f32 * LINE_HEIGHT;
            self.scroll_handle.set_offset(point(
                px(reveal_axis(
                    offset.x.into(),
                    x,
                    x + 2.0,
                    viewport.width.into(),
                    content_width,
                )),
                px(reveal_axis(
                    offset.y.into(),
                    y,
                    y + LINE_HEIGHT,
                    viewport.height.into(),
                    self.buffer.line_count() as f32 * LINE_HEIGHT + VERTICAL_PADDING * 2.0,
                )),
            ));
        }

        let mut lines = div()
            .id("sql-editor-lines")
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .py_1()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle);

        for l in 0..self.buffer.line_count().max(1) {
            let content = self.buffer.line(l).unwrap_or_default();
            let content = content.trim_end_matches(['\n', '\r']);
            let caret_col = (focused && l == cursor_line && self.buffer.selection().is_none())
                .then_some(cursor_col);
            lines = lines.child(render_line(
                l,
                content,
                caret_col.map(|col| (col, caret_visible)),
                self.buffer.selection().map(|range| {
                    let start = self.buffer.line_start_offset(l);
                    range.start.saturating_sub(start)..range.end.saturating_sub(start)
                }),
                &on_pick,
                &theme,
                content_width,
            ));
        }

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(theme.bg_editor)
            .text_color(theme.text_primary)
            .font_family(MONO_FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(13.0))
            .overflow_hidden()
            .child(lines);

        if focused && self.show_completions && !self.completion_items.is_empty() {
            let scroll_y = f32::from(self.scroll_handle.offset().y);
            let top = (cursor_line as f32 + 1.0) * LINE_HEIGHT + 4.0 + scroll_y;
            let left = GUTTER_WIDTH
                + CODE_PADDING
                + self
                    .layout
                    .prefix_width(&self.buffer, cursor_line, cursor_col)
                + f32::from(self.scroll_handle.offset().x);
            root = root.child(self.render_completion_popup(top, left, &theme));
        }
        root
    }

    fn render_completion_popup(
        &self,
        top: f32,
        left: f32,
        theme: &Theme,
    ) -> impl IntoElement + use<> {
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

        for (idx, item) in self
            .completion_items
            .iter()
            .take(MAX_VISIBLE_COMPLETIONS)
            .enumerate()
        {
            let is_sel = idx == self.selected_completion;
            let bg = if is_sel {
                theme.bg_surface_selected
            } else {
                theme.bg_surface
            };

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
                                    .font_weight(if is_sel {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(item.label.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_muted)
                            .child(item.detail.clone()),
                    ),
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
    let starts_plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    !(starts_plain && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// Colour of a word in the dialect-aware SQL syntax highlighter
fn word_color(word: &str, theme: &Theme) -> Hsla {
    let trimmed = word.trim();
    if trimmed.starts_with("--") {
        return theme.text_muted.into();
    }
    if trimmed.starts_with('\'') || trimmed.ends_with('\'') {
        return theme.text_success.into();
    }

    // Parameters: :name, @name, $1, ?
    let stripped_punct = trimmed
        .trim_end_matches([',', ';', ')', ']'])
        .trim_start_matches(['(', '[']);
    if (stripped_punct.starts_with(':')
        && !stripped_punct.starts_with("::")
        && stripped_punct.len() > 1
        && stripped_punct[1..]
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_'))
        || (stripped_punct.starts_with('@')
            && stripped_punct.len() > 1
            && stripped_punct[1..]
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_'))
        || (stripped_punct.starts_with('$')
            && stripped_punct.len() > 1
            && stripped_punct[1..].chars().all(|c| c.is_ascii_digit()))
        || stripped_punct == "?"
    {
        return theme.text_warning.into();
    }

    let core_token = stripped_punct.to_uppercase();
    match core_token.as_str() {
        // ANSI / Standard SQL keywords
        "SELECT" | "FROM" | "WHERE" | "JOIN" | "INNER" | "LEFT" | "RIGHT" | "FULL" | "CROSS"
        | "ON" | "GROUP" | "BY" | "ORDER" | "LIMIT" | "OFFSET" | "INSERT" | "INTO" | "VALUES"
        | "UPDATE" | "SET" | "DELETE" | "AS" | "AND" | "OR" | "NOT" | "IN" | "IS" | "NULL"
        | "WITH" | "RECURSIVE" | "CREATE" | "TABLE" | "DROP" | "ALTER" | "VIEW" | "INDEX"
        | "TRIGGER" | "FUNCTION" | "PROCEDURE" | "SCHEMA" | "DATABASE" | "HAVING" | "DISTINCT"
        | "UNION" | "ALL" | "EXCEPT" | "INTERSECT" | "CASE" | "WHEN" | "THEN" | "ELSE" | "END"
        | "CAST" | "COALESCE" | "BETWEEN" | "LIKE" | "EXISTS" | "PRIMARY" | "KEY" | "FOREIGN"
        | "REFERENCES" | "CHECK" | "UNIQUE" | "DEFAULT" | "CASCADE" | "RESTRICT"
        // Transactions & Sessions
        | "BEGIN" | "COMMIT" | "ROLLBACK" | "TRANSACTION" | "SAVEPOINT" | "RELEASE"
        // Dialect-specific (PostgreSQL / MySQL / SQLite)
        | "RETURNING" | "CONFLICT" | "DO" | "NOTHING" | "VACUUM" | "ANALYZE" | "EXPLAIN"
        | "OVER" | "PARTITION" | "WINDOW" | "FILTER" | "ILIKE" | "SIMILAR" | "TRUE" | "FALSE"
        | "BOOLEAN" | "SERIAL" | "BIGSERIAL" | "INTERVAL" | "REPLACE" | "AUTO_INCREMENT"
        | "SHOW" | "DESCRIBE" | "USE" | "MODIFY" | "CHANGE" | "PRAGMA" | "ATTACH" | "DETACH"
        | "AUTOINCREMENT" | "ROWID"
        // Common aggregate & scalar functions
        | "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "NOW" => theme.text_accent.into(),

        _ if core_token.parse::<f64>().is_ok() => theme.text_warning.into(),
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
                .bg(if visible {
                    rgb(0x58a6ff)
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0).into()
                }),
        )
        .into_any_element()
}

/// One editor line: its number in the gutter, then one cell per char. Clicking the left or
/// right half of a char reports the caret position before or after it through `on_pick`.
fn render_line(
    line: usize,
    content: &str,
    caret: Option<(usize, bool)>,
    selection: Option<std::ops::Range<usize>>,
    on_pick: &EditorPick,
    theme: &Theme,
    content_width: f32,
) -> AnyElement {
    let caret_col = caret.map(|(col, _)| col);
    let caret_visible = caret.is_some_and(|(_, visible)| visible);
    let pick = on_pick.clone();
    let line_pick: crate::line_input::CaretPick =
        Rc::new(move |col, kind, window, cx| pick(line, col, kind, window, cx));
    let half = |col: usize, right: bool| {
        let el = div().absolute().top_0().bottom_0().w(relative(0.5));
        let el = if right { el.right_0() } else { el.left_0() };
        crate::line_input::pick_surface(el, col, &line_pick)
    };

    let mut code = div().flex().flex_1().h_full().items_center().pl_3();
    if caret_col.is_some() {
        code = code.bg(theme.bg_surface_hover);
    }

    // A `--` comment colours the rest of the line
    let mut col = 0;
    let mut byte = 0;
    let mut in_comment = false;
    for word in content.split_inclusive(' ') {
        in_comment = in_comment || word.trim_start().starts_with("--");
        let color: Hsla = if in_comment {
            theme.text_muted.into()
        } else {
            word_color(word, theme)
        };
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
                    .text_color(if selection.as_ref().is_some_and(|r| r.contains(&byte)) {
                        crate::line_input::selection_text_color().into()
                    } else {
                        color
                    })
                    .bg(if selection.as_ref().is_some_and(|r| r.contains(&byte)) {
                        crate::line_input::selection_color()
                    } else {
                        rgba(0x00000000)
                    })
                    .child(glyph.to_string())
                    .child(half(col, false))
                    .child(half(col + 1, true)),
            );
            col += 1;
            byte += ch.len_utf8();
        }
    }
    if caret_col.is_some_and(|c| c >= col) {
        code = code.child(caret_bar(caret_visible));
    }
    let line_break_selected = selection
        .as_ref()
        .is_some_and(|range| range.start <= content.len() && range.end > content.len());
    // Clicking right of the text puts the caret at the end of the line
    code = code.child(crate::line_input::pick_surface(
        div().flex_1().h_full().bg(if line_break_selected {
            crate::line_input::selection_color()
        } else {
            rgba(0x00000000)
        }),
        usize::MAX,
        &line_pick,
    ));

    div()
        .flex()
        .flex_none()
        .min_w_full()
        .w(px(content_width))
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
                .text_color(if caret_col.is_some() {
                    theme.text_accent
                } else {
                    theme.text_muted
                })
                .child(format!("{}", line + 1)),
        )
        .child(code)
        .into_any_element()
}

impl TextTarget for SqlEditorView {
    fn select_all(&mut self) {
        SqlEditorView::select_all(self);
    }
    fn selected_text(&self) -> Option<String> {
        self.buffer.selected_text()
    }
    fn delete_selection(&mut self) {
        self.line_drag_anchor = None;
        self.buffer.delete_selection();
        self.hide_completions();
        self.caret_moved = true;
    }
    fn insert(&mut self, text: &str) {
        self.insert_text(text);
    }
    fn backspace(&mut self, modifiers: &Modifiers) {
        if self.buffer.selection().is_some() {
            TextTarget::delete_selection(self);
            return;
        }
        if modifiers.platform || modifiers.control || modifiers.alt {
            self.buffer.move_selection(true, |buffer| {
                if modifiers.platform || modifiers.control {
                    buffer.move_line_start();
                } else {
                    buffer.move_word_left();
                }
            });
            self.buffer.delete_selection();
            self.caret_moved = true;
            self.update_completions();
        } else {
            self.backspace();
        }
    }
    fn delete_forward(&mut self) {
        self.buffer.delete_forward();
        self.caret_moved = true;
        self.hide_completions();
    }
    fn navigate(&mut self, key: &str, modifiers: &Modifiers) {
        SqlEditorView::navigate(self, key, modifiers);
    }
    fn restore(&mut self, redo: bool) {
        if redo {
            self.redo();
        } else {
            self.undo();
        }
    }
    fn escape(&mut self) {
        self.buffer.set_cursor_offset(self.buffer.cursor_offset());
        self.line_drag_anchor = None;
        self.hide_completions();
        self.caret_moved = false;
    }
}

impl Render for SqlEditorView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_editor(true, true, Rc::new(|_, _, _, _, _| {}), window)
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{EditorPickKind, MAX_VISIBLE_COMPLETIONS, SqlEditorView, word_color};
    use gpui::{Hsla, Keystroke, Modifiers};
    use sql_intel::SchemaCache;
    use std::sync::Arc;
    use ui_core::theme::Theme;

    fn key(key: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: key.into(),
            key_char: None,
        }
    }

    fn typed(ch: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: ch.into(),
            key_char: Some(ch.into()),
        }
    }

    fn cmd(k: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                platform: true,
                ..Default::default()
            },
            key: k.into(),
            key_char: None,
        }
    }

    fn editor(text: &str) -> SqlEditorView {
        let cache = SchemaCache::new().unwrap();
        cache
            .add_table("public", "customers", "BASE TABLE")
            .unwrap();
        cache
            .add_column("public", "customers", "email", "varchar", false)
            .unwrap();
        SqlEditorView::new(text, Theme::dark(), Some(Arc::new(cache)))
    }

    fn type_str(ed: &mut SqlEditorView, s: &str) {
        for ch in s.chars() {
            let k = if ch == ' ' {
                key("space")
            } else {
                typed(&ch.to_string())
            };
            assert!(ed.handle_key(&k));
        }
    }

    #[test]
    fn accepting_a_completion_replaces_the_typed_word() {
        let mut ed = editor("");
        type_str(&mut ed, "SELECT * FROM cust");
        assert!(ed.show_completions);
        let idx = ed
            .completion_items
            .iter()
            .position(|i| i.label == "customers")
            .unwrap();
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
        ed.selected_completion = ed
            .completion_items
            .iter()
            .position(|i| i.label == "email")
            .unwrap();
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
    fn select_all_covers_long_unicode_scripts_without_moving_the_viewport() {
        let original = format!(
            "SELECT 'Tiếng Việt 😀';\n{}\nSELECT 2;",
            "long_column_name, ".repeat(100)
        );
        let mut ed = editor(&original);
        ed.scroll_handle
            .set_offset(gpui::point(gpui::px(-250.0), gpui::px(-40.0)));
        ed.show_completions = true;
        assert!(ed.handle_key(&cmd("a")));
        assert_eq!(
            ed.buffer.selected_text().as_deref(),
            Some(original.as_str())
        );
        assert_eq!(ed.buffer.selection(), Some(0..original.len()));
        assert_eq!(
            ed.scroll_handle.offset(),
            gpui::point(gpui::px(-250.0), gpui::px(-40.0))
        );
        assert!(!ed.caret_moved);
        assert!(!ed.show_completions);
        ed.insert_text("SELECT 42;");
        assert_eq!(ed.buffer.text(), "SELECT 42;");
        ed.undo();
        assert_eq!(ed.buffer.text(), original);
        let mut control_a = key("a");
        control_a.modifiers.control = true;
        assert!(ed.handle_key(&control_a));
        assert_eq!(
            ed.buffer.selected_text().as_deref(),
            Some(original.as_str())
        );
    }

    #[test]
    fn double_click_selects_complete_unicode_lines_and_line_endings() {
        let original = "SELECT 1;\r\nSELECT 'Việt 😀';\r\nSELECT 3;";
        let mut ed = editor(original);
        let double_click = EditorPickKind::Press {
            shift: false,
            clicks: 2,
        };
        ed.pick(1, 4, double_click);
        assert_eq!(
            ed.buffer.selected_text().as_deref(),
            Some("SELECT 'Việt 😀';\r\n")
        );
        assert!(!ed.caret_moved);
        ed.insert_text("SELECT 42;\n");
        assert_eq!(ed.buffer.text(), "SELECT 1;\r\nSELECT 42;\nSELECT 3;");
        ed.undo();
        assert_eq!(ed.buffer.text(), original);
        ed.pick(2, usize::MAX, double_click);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("SELECT 3;"));
        ed.pick(0, 0, double_click);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("SELECT 1;\r\n"));
    }

    #[test]
    fn double_click_drag_extends_whole_lines_and_single_click_resets_it() {
        let mut ed = editor("one\n\nthree\nfour");
        ed.pick(
            2,
            2,
            EditorPickKind::Press {
                shift: false,
                clicks: 2,
            },
        );
        ed.pick(2, 1, EditorPickKind::Drag);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("three\n"));
        ed.pick(0, 1, EditorPickKind::Drag);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("one\n\nthree\n"));
        ed.pick(3, 1, EditorPickKind::Drag);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("three\nfour"));
        ed.pick(
            0,
            1,
            EditorPickKind::Press {
                shift: false,
                clicks: 1,
            },
        );
        assert!(ed.buffer.selection().is_none());
        ed.pick(0, 2, EditorPickKind::Drag);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("n"));
        ed.pick(
            0,
            3,
            EditorPickKind::Press {
                shift: true,
                clicks: 1,
            },
        );
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("ne"));
        ed.pick(
            1,
            0,
            EditorPickKind::Press {
                shift: false,
                clicks: 2,
            },
        );
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("\n"));
    }

    #[test]
    fn shift_click_extends_the_original_anchor_even_on_rapid_clicks() {
        let mut ed = editor("one\nViệt 😀\nlast");
        let press = |shift, clicks| EditorPickKind::Press { shift, clicks };
        ed.pick(1, 1, press(false, 1));
        ed.pick(2, 2, press(true, 2));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("iệt 😀\nla"));
        ed.pick(1, 3, press(true, 1));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("iệ"));
        ed.pick(0, 1, press(true, 3));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("ne\nV"));
        ed.pick(0, 2, EditorPickKind::Drag);
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("e\nV"));

        ed.pick(1, 1, press(false, 2));
        ed.pick(2, 2, press(true, 2));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("Việt 😀\nla"));
    }

    #[test]
    fn shift_navigation_and_mouse_clicks_share_the_selection_anchor() {
        let mut ed = editor("one\ntwo\nlast");
        ed.pick(
            1,
            1,
            EditorPickKind::Press {
                shift: false,
                clicks: 1,
            },
        );
        let mut right = key("right");
        right.modifiers.shift = true;
        assert!(ed.handle_key(&right));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("w"));
        ed.pick(
            2,
            2,
            EditorPickKind::Press {
                shift: true,
                clicks: 1,
            },
        );
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("wo\nla"));
        let mut home = key("home");
        home.modifiers.shift = true;
        assert!(ed.handle_key(&home));
        assert_eq!(ed.buffer.selected_text().as_deref(), Some("wo\n"));
    }

    #[test]
    fn escape_clears_reverse_selection_and_completions_without_moving_the_caret() {
        let mut ed = editor("SELECT 'Việt 😀';");
        let mut left = key("left");
        left.modifiers.shift = true;
        assert!(ed.handle_key(&left));
        assert!(ed.buffer.selection().is_some());
        let caret = ed.buffer.cursor_offset();
        let original = ed.buffer.text();
        ed.show_completions = true;
        ed.scroll_handle
            .set_offset(gpui::point(gpui::px(-20.0), gpui::px(0.0)));
        assert!(ed.handle_key(&key("escape")));
        assert!(ed.buffer.selection().is_none());
        assert_eq!(ed.buffer.cursor_offset(), caret);
        assert_eq!(ed.buffer.text(), original);
        assert!(!ed.show_completions);
        assert!(!ed.caret_moved);
        assert_eq!(ed.scroll_handle.offset().x, gpui::px(-20.0));
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
        cache
            .add_table("public", "apiClient", "BASE TABLE")
            .unwrap();
        cache.add_table("public", "users", "BASE TABLE").unwrap();
        cache
            .add_column("public", "apiClient", "clientId", "varchar", false)
            .unwrap();
        SqlEditorView::new("", Theme::dark(), Some(Arc::new(cache)))
    }

    fn accept(ed: &mut SqlEditorView, label: &str) {
        ed.selected_completion = ed
            .completion_items
            .iter()
            .position(|i| i.label == label)
            .unwrap();
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
        assert_eq!(
            ed.buffer.text(),
            "SELECT * FROM \"apiClient\" a JOIN users u"
        );

        type_str(&mut ed, " WHERE a.");
        accept(&mut ed, "clientId");
        assert_eq!(
            ed.buffer.text(),
            "SELECT * FROM \"apiClient\" a JOIN users u WHERE a.\"clientId\""
        );
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

    #[test]
    fn word_color_recognizes_dialects_and_parameters() {
        let theme = Theme::dark();
        // Dialect keywords
        assert_eq!(
            word_color("RETURNING", &theme),
            Hsla::from(theme.text_accent)
        );
        assert_eq!(
            word_color("CONFLICT", &theme),
            Hsla::from(theme.text_accent)
        );
        assert_eq!(word_color("PRAGMA", &theme), Hsla::from(theme.text_accent));
        assert_eq!(
            word_color("AUTO_INCREMENT", &theme),
            Hsla::from(theme.text_accent)
        );

        // Parameters
        assert_eq!(
            word_color(":user_id", &theme),
            Hsla::from(theme.text_warning)
        );
        assert_eq!(
            word_color("(:user_id,", &theme),
            Hsla::from(theme.text_warning)
        );
        assert_eq!(
            word_color("@tenant", &theme),
            Hsla::from(theme.text_warning)
        );
        assert_eq!(word_color("$1,", &theme), Hsla::from(theme.text_warning));
        assert_eq!(word_color("?", &theme), Hsla::from(theme.text_warning));

        // String literals & numbers
        assert_eq!(
            word_color("'hello'", &theme),
            Hsla::from(theme.text_success)
        );
        assert_eq!(word_color("42.5", &theme), Hsla::from(theme.text_warning));
    }
}
