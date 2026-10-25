//! Caret-aware single-line text inputs: key handling for the connection form and the
//! filter bar, and a renderer that lets a mouse click place the caret between characters
//! and a drag select a range.
//!
//! Carets are char indices (not byte offsets) so the masked password field, which
//! renders one bullet per character, maps clicks straight onto the real value.
//! A selection runs between an anchor and the caret; no anchor (or one equal to the
//! caret) means nothing is selected.

use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use ui_core::history::{EditKind, History, UndoKey};

use super::*;

/// How the mouse reached a caret position in an input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PickKind {
    /// Mouse button pressed; Shift extends the selection from the current caret
    Press { clicks: usize, shift: bool },
    /// Mouse moved with the left button held, extending the selection
    Drag,
}

/// Called with `(char_index, kind)` when a character of an input is clicked or dragged over.
pub(super) type CaretPick = Rc<dyn Fn(usize, PickKind, &mut Window, &mut App)>;

/// Moves `caret` and `anchor` for a mouse pick. `was_active` is whether the input already
/// had the keyboard: Shift+click and drags only extend a selection inside the focused input.
pub(super) fn apply_pick(caret: &mut usize, anchor: &mut Option<usize>, index: usize, kind: PickKind, was_active: bool) {
    match kind {
        PickKind::Press { clicks, .. } if clicks >= 2 => {
            *anchor = Some(0);
            *caret = usize::MAX;
        }
        PickKind::Press { shift: true, .. } if was_active => {
            anchor.get_or_insert(*caret);
            *caret = index;
        }
        PickKind::Press { .. } => {
            *anchor = Some(index);
            *caret = index;
        }
        PickKind::Drag if was_active && anchor.is_some() => *caret = index,
        PickKind::Drag => {}
    }
}

/// Selected char range of an input with `len` chars, if any
pub(super) fn selection_range(anchor: Option<usize>, caret: usize, len: usize) -> Option<Range<usize>> {
    let (a, c) = (anchor?.min(len), caret.min(len));
    (a != c).then(|| a.min(c)..a.max(c))
}

/// Undo history of a single-line input: snapshots of its text and caret
pub type LineHistory = History<(String, usize)>;

/// A single-line input being edited by a key press.
pub(super) struct LineInput<'a> {
    pub text: &'a mut String,
    pub caret: &'a mut usize,
    /// Other end of the selection; see [`selection_range`]
    pub anchor: &'a mut Option<usize>,
    pub digits_only: bool,
    pub history: &'a mut LineHistory,
}

fn byte_at(text: &str, caret: usize) -> usize {
    text.char_indices().nth(caret).map(|(i, _)| i).unwrap_or(text.len())
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Index of the start of the word before `caret` (macOS Option+Left).
fn prev_word(text: &str, caret: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = caret.min(chars.len());
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}

/// Index of the end of the word after `caret` (macOS Option+Right).
fn next_word(text: &str, caret: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = caret.min(chars.len());
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}

fn selection(input: &LineInput) -> Option<Range<usize>> {
    selection_range(*input.anchor, *input.caret, char_len(input.text))
}

fn selected_text(input: &LineInput) -> Option<String> {
    let range = selection(input)?;
    Some(input.text.chars().skip(range.start).take(range.len()).collect())
}

/// Removes the selected chars, if any. Returns whether something was removed.
fn delete_selection(input: &mut LineInput) -> bool {
    let range = selection(input);
    *input.anchor = None;
    match range {
        Some(range) => {
            remove_range(input, range.start, range.end);
            true
        }
        None => false,
    }
}

fn insert(input: &mut LineInput, s: &str) {
    let s: String = if input.digits_only {
        s.chars().filter(|c| c.is_ascii_digit()).collect()
    } else {
        s.to_string()
    };
    delete_selection(input);
    let at = byte_at(input.text, *input.caret);
    input.text.insert_str(at, &s);
    *input.caret += char_len(&s);
}

/// Removes the chars in `[from, to)`, leaving the caret at `from`.
fn remove_range(input: &mut LineInput, from: usize, to: usize) {
    let (start, end) = (byte_at(input.text, from), byte_at(input.text, to));
    input.text.replace_range(start..end, "");
    *input.caret = from;
}

/// Applies an editing key press, Cmd+Z / Cmd+Shift+Z included. Returns `true` when the
/// key was consumed.
pub(super) fn handle_line_key(mut input: LineInput, keystroke: &Keystroke, cx: &mut App) -> bool {
    if let Some(undo_key) = UndoKey::from_keystroke(keystroke) {
        let current = (input.text.clone(), *input.caret);
        let restored = match undo_key {
            UndoKey::Undo => input.history.undo(current),
            UndoKey::Redo => input.history.redo(current),
        };
        if let Some((text, caret)) = restored {
            *input.text = text;
            *input.caret = caret;
            *input.anchor = None;
        }
        return true;
    }

    let before = (input.text.clone(), (*input.caret).min(char_len(input.text)));
    let Some(kind) = apply_line_key(&mut input, keystroke, cx) else { return false };
    if *input.text != before.0 {
        input.history.record(before, kind);
    } else {
        input.history.break_run();
    }
    true
}

/// Applies an editing key press, returning how it changed the text, or `None` when
/// the key is not an editing key
fn apply_line_key(input: &mut LineInput, keystroke: &Keystroke, cx: &mut App) -> Option<EditKind> {
    let len = char_len(input.text);
    *input.caret = (*input.caret).min(len);
    let caret = *input.caret;
    let mods = &keystroke.modifiers;
    let cmd = mods.platform || mods.control;
    let key = keystroke.key.as_str();

    if cmd && key.eq_ignore_ascii_case("a") {
        *input.anchor = Some(0);
        *input.caret = len;
        return Some(EditKind::Other);
    }
    // Copy and cut act on the selection, or on the whole text when nothing is selected
    if cmd && key.eq_ignore_ascii_case("c") {
        let copied = selected_text(input).unwrap_or_else(|| input.text.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(copied));
        return Some(EditKind::Other);
    }
    if cmd && key.eq_ignore_ascii_case("x") {
        match selected_text(input) {
            Some(cut) => {
                cx.write_to_clipboard(ClipboardItem::new_string(cut));
                delete_selection(input);
            }
            None => {
                cx.write_to_clipboard(ClipboardItem::new_string(input.text.clone()));
                input.text.clear();
                *input.caret = 0;
                *input.anchor = None;
            }
        }
        return Some(EditKind::Other);
    }
    if cmd && key.eq_ignore_ascii_case("v") {
        if let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) {
            insert(input, pasted.trim_matches(|c| c == '\r' || c == '\n'));
        }
        return Some(EditKind::Other);
    }

    if move_caret(input, key, mods) {
        return Some(EditKind::Other);
    }

    let had_selection = selection(input).is_some();
    let deleting_one = (key == "backspace" || key == "delete") && !had_selection && !mods.platform && !mods.alt;
    match key {
        "backspace" | "delete" if had_selection => {
            delete_selection(input);
        }
        "backspace" if mods.platform => remove_range(input, 0, caret),
        "backspace" if mods.alt => {
            let from = prev_word(input.text, caret);
            remove_range(input, from, caret);
        }
        "backspace" if caret > 0 => remove_range(input, caret - 1, caret),
        "delete" if caret < len => remove_range(input, caret, caret + 1),
        "backspace" | "delete" => {}
        "space" => insert(input, " "),
        _ => {
            if cmd {
                return None;
            }
            let typed = keystroke
                .key_char
                .as_deref()
                .filter(|kc| kc.chars().count() == 1)
                .or_else(|| (key.chars().count() == 1).then_some(key));
            match typed {
                Some(s) => {
                    insert(input, s);
                    let word_char = !had_selection && !s.chars().all(char::is_whitespace);
                    return Some(if word_char { EditKind::Typing } else { EditKind::Other });
                }
                None => return None,
            }
        }
    }
    *input.anchor = None;
    Some(if deleting_one { EditKind::Deleting } else { EditKind::Other })
}

/// Caret movement keys. With Shift they extend the selection (Shift+Cmd+Left selects to
/// the start, Shift+Cmd+Right to the end); without it they drop the selection, and plain
/// Left/Right collapse it to its edge. Returns `false` for any other key.
fn move_caret(input: &mut LineInput, key: &str, mods: &Modifiers) -> bool {
    let len = char_len(input.text);
    let caret = *input.caret;
    let selected = selection(input);
    let target = match key {
        "left" if mods.platform => 0,
        "right" if mods.platform => len,
        "left" if mods.alt => prev_word(input.text, caret),
        "right" if mods.alt => next_word(input.text, caret),
        "left" if !mods.shift && selected.is_some() => selected.as_ref().map_or(0, |r| r.start),
        "right" if !mods.shift && selected.is_some() => selected.as_ref().map_or(len, |r| r.end),
        "left" => caret.saturating_sub(1),
        "right" => (caret + 1).min(len),
        "up" | "home" => 0,
        "down" | "end" => len,
        _ => return false,
    };
    if mods.shift {
        input.anchor.get_or_insert(caret);
    } else {
        *input.anchor = None;
    }
    *input.caret = target;
    true
}

/// Renders the text of an input one character per cell. Clicking the left or right half
/// of a character reports the caret position before or after it through `on_pick`.
pub(super) fn render_caret_text(
    display: &str,
    caret: Option<usize>,
    caret_visible: bool,
    selection: Option<Range<usize>>,
    on_pick: &CaretPick,
) -> Vec<AnyElement> {
    // Like macOS, the caret hides while a range is selected
    let caret = if selection.is_some() { None } else { caret };
    let caret_bar = |visible: bool| {
        div()
            .relative()
            .w(px(0.0))
            .h(px(14.0))
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
    };
    let half = |index: usize, right: bool| {
        let pick = on_pick.clone();
        let el = div().absolute().top_0().bottom_0().w(relative(0.5));
        let el = if right { el.right_0() } else { el.left_0() };
        let drag_pick = on_pick.clone();
        el.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            pick(index, PickKind::Press { clicks: event.click_count, shift: event.modifiers.shift }, window, cx);
        })
        .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                drag_pick(index, PickKind::Drag, window, cx);
            }
        })
    };

    let mut cells = Vec::new();
    for (i, ch) in display.chars().enumerate() {
        if caret == Some(i) {
            cells.push(caret_bar(caret_visible));
        }
        // A lone space shapes to zero width, so render it as a non-breaking space.
        let glyph = if ch == ' ' { '\u{00a0}' } else { ch };
        let selected = selection.as_ref().is_some_and(|r| r.contains(&i));
        cells.push(
            div()
                .relative()
                .flex_none()
                .when(selected, |d| d.bg(rgb(0x264f78)).text_color(rgb(0xffffff)))
                .child(glyph.to_string())
                .child(half(i, false))
                .child(half(i + 1, true))
                .into_any_element(),
        );
    }
    if caret.is_some_and(|c| c >= char_len(display)) {
        cells.push(caret_bar(caret_visible));
    }
    cells
}

impl Workspace {
    pub(super) fn field_value_mut(&mut self, field: FormField) -> &mut String {
        match field {
            FormField::Name => &mut self.form_name,
            FormField::Host => &mut self.form_host,
            FormField::Port => &mut self.form_port,
            FormField::Database => &mut self.form_database,
            FormField::User => &mut self.form_user,
            FormField::Password => &mut self.form_password,
        }
    }

    /// Click handler for the characters of a connection form field.
    pub(super) fn field_caret_pick(&self, field: FormField, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.active_field == Some(field);
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                this.active_field = Some(field);
                apply_pick(&mut this.field_cursor, &mut this.field_anchor, index, kind, was_active);
                this.cursor_visible = true;
                window.focus(&this.focus_handle);
                cx.notify();
            });
        })
    }

    /// Click handler for the characters of the filter bar.
    pub(super) fn filter_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.is_filter_focused;
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                this.commit_cell_edit();
                this.blur_fetch_size();
                this.is_filter_focused = true;
                apply_pick(&mut this.filter_cursor, &mut this.filter_anchor, index, kind, was_active);
                this.cursor_visible = true;
                cx.notify();
            });
        })
    }
}

impl Workspace {
    /// Click handler for the characters of the fetch size box.
    pub(super) fn fetch_size_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.is_fetch_size_focused;
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                if !was_active {
                    this.focus_fetch_size(false, index);
                }
                apply_pick(&mut this.fetch_size_cursor, &mut this.fetch_size_anchor, index, kind, was_active);
                this.cursor_visible = true;
                cx.notify();
            });
        })
    }

    /// Moves keyboard input to the fetch size box, leaving the filter bar
    pub(super) fn focus_fetch_size(&mut self, select_all: bool, caret: usize) {
        self.commit_cell_edit();
        self.is_filter_focused = false;
        self.is_fetch_size_focused = true;
        self.fetch_size_anchor = select_all.then_some(0);
        self.fetch_size_cursor = if select_all { usize::MAX } else { caret };
        self.cursor_visible = true;
    }

    /// Leaves the fetch size box, keeping a valid typed size for the next fetch
    pub(super) fn blur_fetch_size(&mut self) {
        if self.is_fetch_size_focused {
            self.is_fetch_size_focused = false;
            self.fetch_size_anchor = None;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.fetch.commit_size_text();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{apply_pick, delete_selection, insert, move_caret, next_word, prev_word, remove_range, selected_text,
        selection_range, LineHistory, LineInput, PickKind};
    use gpui::Modifiers;

    struct Field {
        text: String,
        caret: usize,
        anchor: Option<usize>,
        history: LineHistory,
    }

    impl Field {
        fn new(text: &str, caret: usize, anchor: Option<usize>) -> Self {
            Self { text: text.to_string(), caret, anchor, history: LineHistory::new() }
        }

        fn input(&mut self, digits_only: bool) -> LineInput<'_> {
            LineInput { text: &mut self.text, caret: &mut self.caret, anchor: &mut self.anchor, digits_only, history: &mut self.history }
        }

        fn selection(&self) -> Option<std::ops::Range<usize>> {
            selection_range(self.anchor, self.caret, self.text.chars().count())
        }
    }

    fn edit(text: &str, caret: usize, f: impl FnOnce(&mut LineInput)) -> (String, usize) {
        let mut field = Field::new(text, caret, None);
        f(&mut field.input(false));
        (field.text, field.caret)
    }

    fn shift() -> Modifiers {
        Modifiers { shift: true, ..Default::default() }
    }

    fn shift_cmd() -> Modifiers {
        Modifiers { shift: true, platform: true, ..Default::default() }
    }

    #[test]
    fn inserts_and_removes_at_the_caret() {
        assert_eq!(edit("id = 1", 2, |i| insert(i, "x")), ("idx = 1".to_string(), 3));
        assert_eq!(edit("ngày", 2, |i| insert(i, "à")), ("ngàày".to_string(), 3));
        assert_eq!(edit("ngày", 3, |i| remove_range(i, 2, 3)), ("ngy".to_string(), 2));
    }

    #[test]
    fn selection_is_replaced_by_typing() {
        let mut field = Field::new("old", usize::MAX, Some(0));
        insert(&mut field.input(false), "n");
        assert_eq!((field.text.as_str(), field.caret, field.anchor), ("n", 1, None));

        // Only the selected part is replaced
        let mut field = Field::new("id = 10", 7, Some(5));
        insert(&mut field.input(false), "2");
        assert_eq!((field.text.as_str(), field.caret), ("id = 2", 6));
    }

    #[test]
    fn digits_only_drops_other_chars() {
        let mut field = Field::new("54", 1, None);
        insert(&mut field.input(true), "3a2");
        assert_eq!((field.text.as_str(), field.caret), ("5324", 3));
    }

    #[test]
    fn word_jumps_skip_punctuation() {
        let text = "status = 'open' AND id";
        assert_eq!(prev_word(text, text.len()), 20);
        assert_eq!(prev_word(text, 9), 0);
        assert_eq!(next_word(text, 6), 14);
        assert_eq!(next_word(text, 0), 6);
    }

    #[test]
    fn shift_cmd_arrows_select_to_the_start_or_end() {
        let mut field = Field::new("SELECT 1", 3, None);
        assert!(move_caret(&mut field.input(false), "right", &shift_cmd()));
        assert_eq!(field.selection(), Some(3..8));

        let mut field = Field::new("SELECT 1", 3, None);
        move_caret(&mut field.input(false), "left", &shift_cmd());
        assert_eq!(field.selection(), Some(0..3));
        assert_eq!(selected_text(&field.input(false)).as_deref(), Some("SEL"));
    }

    #[test]
    fn shift_arrows_grow_and_shrink_the_selection_from_the_anchor() {
        let mut field = Field::new("abcdef", 2, None);
        move_caret(&mut field.input(false), "right", &shift());
        move_caret(&mut field.input(false), "right", &shift());
        assert_eq!(field.selection(), Some(2..4));
        move_caret(&mut field.input(false), "left", &shift());
        assert_eq!(field.selection(), Some(2..3));
        // Crossing the anchor flips the range to its other side
        move_caret(&mut field.input(false), "left", &shift());
        move_caret(&mut field.input(false), "left", &shift());
        assert_eq!(field.selection(), Some(1..2));
    }

    #[test]
    fn plain_arrows_collapse_the_selection_to_its_edge() {
        let mut field = Field::new("abcdef", 4, Some(1));
        move_caret(&mut field.input(false), "left", &Modifiers::default());
        assert_eq!((field.caret, field.selection()), (1, None));

        let mut field = Field::new("abcdef", 1, Some(4));
        move_caret(&mut field.input(false), "right", &Modifiers::default());
        assert_eq!((field.caret, field.selection()), (4, None));
    }

    #[test]
    fn deleting_removes_only_the_selection() {
        let mut field = Field::new("id = 10 AND x", 7, Some(2));
        assert!(delete_selection(&mut field.input(false)));
        assert_eq!((field.text.as_str(), field.caret, field.anchor), ("id AND x", 2, None));
        assert!(!delete_selection(&mut field.input(false)));
    }

    #[test]
    fn mouse_press_and_drag_select_a_range() {
        let (mut caret, mut anchor) = (0, None);
        apply_pick(&mut caret, &mut anchor, 2, PickKind::Press { clicks: 1, shift: false }, true);
        assert_eq!(selection_range(anchor, caret, 10), None);
        apply_pick(&mut caret, &mut anchor, 6, PickKind::Drag, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(2..6));
        // Dragging back past the press point selects to its left
        apply_pick(&mut caret, &mut anchor, 0, PickKind::Drag, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(0..2));
    }

    #[test]
    fn shift_click_extends_and_double_click_selects_all() {
        let (mut caret, mut anchor) = (3, None);
        apply_pick(&mut caret, &mut anchor, 8, PickKind::Press { clicks: 1, shift: true }, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(3..8));

        // Shift+click into an input that did not have focus just places the caret
        let (mut caret, mut anchor) = (3, None);
        apply_pick(&mut caret, &mut anchor, 8, PickKind::Press { clicks: 1, shift: true }, false);
        assert_eq!((caret, selection_range(anchor, caret, 10)), (8, None));

        apply_pick(&mut caret, &mut anchor, 4, PickKind::Press { clicks: 2, shift: false }, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(0..10));
    }
}
