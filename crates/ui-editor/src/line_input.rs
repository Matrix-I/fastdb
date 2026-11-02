//! Reusable single-line editing, selection and mouse hit targets for text inputs.
//! Application code owns focus and validation; shared commands own editing behavior.
//!
//! Carets are char indices (not byte offsets) so the masked password field, which
//! renders one bullet per character, maps clicks straight onto the real value.
//! A selection runs between an anchor and the caret; no anchor (or one equal to the
//! caret) means nothing is selected.

use std::ops::Range;
use std::rc::Rc;

use crate::text_edit::{TextClipboard, TextCommand, TextTarget, dispatch};
use gpui::prelude::FluentBuilder;
use ui_core::history::{EditKind, History};

use gpui::*;

/// How the mouse reached a caret position in an input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    /// Mouse button pressed; Shift extends the selection from the current caret
    Press { clicks: usize, shift: bool },
    /// Mouse moved with the left button held, extending the selection
    Drag,
}

/// Called with `(char_index, kind)` when a character of an input is clicked or dragged over.
pub type CaretPick = Rc<dyn Fn(usize, PickKind, &mut Window, &mut App)>;

/// Moves `caret` and `anchor` for a mouse pick. `was_active` is whether the input already
/// had the keyboard: Shift+click and drags only extend a selection inside the focused input.
pub fn apply_pick(
    caret: &mut usize,
    anchor: &mut Option<usize>,
    index: usize,
    kind: PickKind,
    was_active: bool,
) {
    match pick_action(kind, was_active, *anchor == Some(0) && *caret == usize::MAX) {
        PickAction::SelectUnit => {
            *anchor = Some(0);
            *caret = usize::MAX;
        }
        PickAction::Move { extend } => {
            move_anchor(anchor, *caret, extend);
            *caret = index;
        }
        PickAction::ExtendUnit | PickAction::Ignore => {}
    }
}

/// A common mouse policy for single-line values and SQL line selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickAction {
    Move { extend: bool },
    SelectUnit,
    ExtendUnit,
    Ignore,
}

pub fn pick_action(kind: PickKind, active: bool, unit_selected: bool) -> PickAction {
    match kind {
        PickKind::Press { shift: true, .. } if active => PickAction::Move { extend: true },
        PickKind::Press { clicks, .. } if clicks >= 2 => PickAction::SelectUnit,
        PickKind::Press { .. } => PickAction::Move { extend: false },
        PickKind::Drag if active && unit_selected => PickAction::ExtendUnit,
        PickKind::Drag if active => PickAction::Move { extend: true },
        PickKind::Drag => PickAction::Ignore,
    }
}

/// Keep the original anchor through repeated Shift navigation and mouse presses.
pub fn move_anchor(anchor: &mut Option<usize>, caret: usize, extend: bool) {
    *anchor = extend.then_some(anchor.unwrap_or(caret));
}

/// Selected char range of an input with `len` chars, if any
pub fn selection_range(anchor: Option<usize>, caret: usize, len: usize) -> Option<Range<usize>> {
    let (a, c) = (anchor?.min(len), caret.min(len));
    (a != c).then(|| a.min(c)..a.max(c))
}

/// Clear a visible selection without moving its caret or editing text.
/// Returns whether a selection was cancelled, for Escape-before-dismiss routing.
pub fn clear_selection(caret: &mut usize, anchor: &mut Option<usize>, len: usize) -> bool {
    let selected = selection_range(*anchor, *caret, len).is_some();
    *caret = (*caret).min(len);
    *anchor = None;
    selected
}

/// Undo history of a single-line input: snapshots of its text and caret
pub type LineHistory = History<(String, usize)>;

/// A single-line input being edited by a key press.
pub struct LineInput<'a> {
    pub text: &'a mut String,
    pub caret: &'a mut usize,
    /// Other end of the selection; see [`selection_range`]
    pub anchor: &'a mut Option<usize>,
    pub digits_only: bool,
    pub history: &'a mut LineHistory,
}

fn byte_at(text: &str, caret: usize) -> usize {
    text.char_indices()
        .nth(caret)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Index of the start of the word before `caret` (macOS Option+Left).
fn prev_word(text: &str, caret: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = caret.min(chars.len());
    while i > 0 && !is_word_char(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    i
}

/// Index of the end of the word after `caret` (macOS Option+Right).
fn next_word(text: &str, caret: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = caret.min(chars.len());
    while i < chars.len() && !is_word_char(chars[i]) {
        i += 1;
    }
    while i < chars.len() && is_word_char(chars[i]) {
        i += 1;
    }
    i
}

fn selection(input: &LineInput) -> Option<Range<usize>> {
    selection_range(*input.anchor, *input.caret, char_len(input.text))
}

fn selected_text(input: &LineInput) -> Option<String> {
    let range = selection(input)?;
    Some(
        input
            .text
            .chars()
            .skip(range.start)
            .take(range.len())
            .collect(),
    )
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
    if s.is_empty() {
        return;
    }
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
pub fn handle_line_key(mut input: LineInput, keystroke: &Keystroke, cx: &mut App) -> bool {
    let Some(command) = TextCommand::from_key(keystroke, false) else {
        return false;
    };
    apply_line_command(&mut input, &command, cx);
    true
}

/// Execute a shared command while recording short-field undo snapshots.
pub fn apply_line_command(
    input: &mut LineInput,
    command: &TextCommand,
    clipboard: &mut impl TextClipboard,
) {
    let before = (input.text.clone(), (*input.caret).min(char_len(input.text)));
    *input.caret = before.1;
    let had_selection = selection(input).is_some();
    let kind = match command {
        TextCommand::Insert(text)
            if !had_selection
                && text.chars().count() == 1
                && !text.chars().all(char::is_whitespace) =>
        {
            EditKind::Typing
        }
        TextCommand::Backspace { modifiers }
            if !had_selection && !modifiers.platform && !modifiers.control && !modifiers.alt =>
        {
            EditKind::Deleting
        }
        _ => EditKind::Other,
    };
    dispatch(input, command, clipboard);
    if !matches!(command, TextCommand::Undo | TextCommand::Redo) {
        if *input.text != before.0 {
            input.history.record(before, kind);
        } else {
            input.history.break_run();
        }
    }
}

impl TextTarget for LineInput<'_> {
    fn select_all(&mut self) {
        *self.anchor = Some(0);
        *self.caret = char_len(self.text);
    }
    fn selected_text(&self) -> Option<String> {
        selected_text(self)
    }
    fn delete_selection(&mut self) {
        delete_selection(self);
    }
    fn insert(&mut self, text: &str) {
        let text = text
            .trim_matches(['\r', '\n'])
            .replace("\r\n", " ")
            .replace(['\r', '\n'], " ");
        insert(self, &text);
    }
    fn backspace(&mut self, modifiers: &Modifiers) {
        if delete_selection(self) {
            return;
        }
        let caret = *self.caret;
        let start = if modifiers.platform || modifiers.control {
            0
        } else if modifiers.alt {
            prev_word(self.text, caret)
        } else {
            caret.saturating_sub(1)
        };
        remove_range(self, start, caret);
    }
    fn delete_forward(&mut self) {
        if !delete_selection(self) {
            let caret = *self.caret;
            remove_range(self, caret, (caret + 1).min(char_len(self.text)));
        }
    }
    fn navigate(&mut self, key: &str, modifiers: &Modifiers) {
        move_caret(self, key, modifiers);
    }
    fn escape(&mut self) {
        clear_selection(self.caret, self.anchor, char_len(self.text));
    }
    fn restore(&mut self, redo: bool) {
        let current = (self.text.clone(), *self.caret);
        let restored = if redo {
            self.history.redo(current)
        } else {
            self.history.undo(current)
        };
        if let Some((text, caret)) = restored {
            *self.text = text;
            *self.caret = caret;
            *self.anchor = None;
        }
    }
}

/// Caret movement keys. With Shift they extend the selection (Shift+Cmd+Left selects to
/// the start, Shift+Cmd+Right to the end); without it they drop the selection, and plain
/// Left/Right collapse it to its edge. Returns `false` for any other key.
fn move_caret(input: &mut LineInput, key: &str, mods: &Modifiers) -> bool {
    let len = char_len(input.text);
    let caret = *input.caret;
    let selected = selection(input);
    let target = match key {
        "left" if mods.platform || mods.control => 0,
        "right" if mods.platform || mods.control => len,
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
    move_anchor(input.anchor, caret, mods.shift);
    *input.caret = target;
    true
}

pub fn selection_color() -> Rgba {
    rgb(0x264f78)
}
pub fn selection_text_color() -> Rgba {
    rgb(0xffffff)
}

/// Attach the common press/drag events to a character hit target or input background.
/// Consuming both events prevents an outer field from replacing the picked position.
pub fn pick_surface(element: Div, index: usize, on_pick: &CaretPick) -> Div {
    let press = on_pick.clone();
    let drag = on_pick.clone();
    element
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            press(
                index,
                PickKind::Press {
                    clicks: event.click_count,
                    shift: event.modifiers.shift,
                },
                window,
                cx,
            );
        })
        .on_mouse_move(move |event, window, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                cx.stop_propagation();
                drag(index, PickKind::Drag, window, cx);
            }
        })
}

/// Renders the text of an input one character per cell. Clicking the left or right half
/// of a character reports the caret position before or after it through `on_pick`.
pub fn render_caret_text(
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
                    .bg(if visible {
                        rgb(0x58a6ff)
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0).into()
                    }),
            )
            .into_any_element()
    };
    let half = |index: usize, right: bool| {
        let el = div().absolute().top_0().bottom_0().w(relative(0.5));
        let el = if right { el.right_0() } else { el.left_0() };
        pick_surface(el, index, on_pick)
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
                .when(selected, |d| {
                    d.bg(selection_color()).text_color(selection_text_color())
                })
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

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{
        LineHistory, LineInput, PickKind, apply_pick, delete_selection, insert, move_caret,
        next_word, prev_word, remove_range, selected_text, selection_range,
    };
    use gpui::Modifiers;

    struct Field {
        text: String,
        caret: usize,
        anchor: Option<usize>,
        history: LineHistory,
    }

    impl Field {
        fn new(text: &str, caret: usize, anchor: Option<usize>) -> Self {
            Self {
                text: text.to_string(),
                caret,
                anchor,
                history: LineHistory::new(),
            }
        }

        fn input(&mut self, digits_only: bool) -> LineInput<'_> {
            LineInput {
                text: &mut self.text,
                caret: &mut self.caret,
                anchor: &mut self.anchor,
                digits_only,
                history: &mut self.history,
            }
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
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    fn shift_cmd() -> Modifiers {
        Modifiers {
            shift: true,
            platform: true,
            ..Default::default()
        }
    }

    #[test]
    fn inserts_and_removes_at_the_caret() {
        assert_eq!(
            edit("id = 1", 2, |i| insert(i, "x")),
            ("idx = 1".to_string(), 3)
        );
        assert_eq!(
            edit("ngày", 2, |i| insert(i, "à")),
            ("ngàày".to_string(), 3)
        );
        assert_eq!(
            edit("ngày", 3, |i| remove_range(i, 2, 3)),
            ("ngy".to_string(), 2)
        );
    }

    #[test]
    fn selection_is_replaced_by_typing() {
        let mut field = Field::new("old", usize::MAX, Some(0));
        insert(&mut field.input(false), "n");
        assert_eq!(
            (field.text.as_str(), field.caret, field.anchor),
            ("n", 1, None)
        );

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
        assert_eq!(
            (field.text.as_str(), field.caret, field.anchor),
            ("id AND x", 2, None)
        );
        assert!(!delete_selection(&mut field.input(false)));
    }

    #[test]
    fn mouse_press_and_drag_select_a_range() {
        let (mut caret, mut anchor) = (0, None);
        apply_pick(
            &mut caret,
            &mut anchor,
            2,
            PickKind::Press {
                clicks: 1,
                shift: false,
            },
            true,
        );
        assert_eq!(selection_range(anchor, caret, 10), None);
        apply_pick(&mut caret, &mut anchor, 6, PickKind::Drag, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(2..6));
        // Dragging back past the press point selects to its left
        apply_pick(&mut caret, &mut anchor, 0, PickKind::Drag, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(0..2));
    }

    #[test]
    fn cancel_selection_retains_the_caret_and_reports_when_escape_should_dismiss() {
        let mut caret = 2;
        let mut anchor = Some(7);
        assert!(super::clear_selection(&mut caret, &mut anchor, 10));
        assert_eq!((caret, anchor), (2, None));
        assert!(!super::clear_selection(&mut caret, &mut anchor, 10));
        caret = usize::MAX;
        anchor = Some(0);
        assert!(super::clear_selection(&mut caret, &mut anchor, 5));
        assert_eq!((caret, anchor), (5, None));
    }

    #[test]
    fn rapid_shift_presses_extend_and_double_click_drag_keeps_the_whole_value() {
        let (mut caret, mut anchor) = (3, None);
        apply_pick(
            &mut caret,
            &mut anchor,
            8,
            PickKind::Press {
                clicks: 2,
                shift: true,
            },
            true,
        );
        assert_eq!(selection_range(anchor, caret, 10), Some(3..8));
        apply_pick(
            &mut caret,
            &mut anchor,
            1,
            PickKind::Press {
                clicks: 3,
                shift: true,
            },
            true,
        );
        assert_eq!(selection_range(anchor, caret, 10), Some(1..3));
        apply_pick(
            &mut caret,
            &mut anchor,
            5,
            PickKind::Press {
                clicks: 2,
                shift: false,
            },
            true,
        );
        apply_pick(&mut caret, &mut anchor, 4, PickKind::Drag, true);
        assert_eq!(selection_range(anchor, caret, 10), Some(0..10));
    }

    #[test]
    fn shift_click_extends_and_double_click_selects_all() {
        let (mut caret, mut anchor) = (3, None);
        apply_pick(
            &mut caret,
            &mut anchor,
            8,
            PickKind::Press {
                clicks: 1,
                shift: true,
            },
            true,
        );
        assert_eq!(selection_range(anchor, caret, 10), Some(3..8));

        // Shift+click into an input that did not have focus just places the caret
        let (mut caret, mut anchor) = (3, None);
        apply_pick(
            &mut caret,
            &mut anchor,
            8,
            PickKind::Press {
                clicks: 1,
                shift: true,
            },
            false,
        );
        assert_eq!((caret, selection_range(anchor, caret, 10)), (8, None));

        apply_pick(
            &mut caret,
            &mut anchor,
            4,
            PickKind::Press {
                clicks: 2,
                shift: false,
            },
            true,
        );
        assert_eq!(selection_range(anchor, caret, 10), Some(0..10));
    }
}
