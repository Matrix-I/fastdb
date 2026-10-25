//! Caret-aware single-line text inputs: key handling for the connection form and the
//! filter bar, and a renderer that lets a mouse click place the caret between characters.
//!
//! Carets are char indices (not byte offsets) so the masked password field, which
//! renders one bullet per character, maps clicks straight onto the real value.

use std::rc::Rc;

use super::*;

/// Called with `(char_index, click_count)` when a character of an input is clicked.
pub(super) type CaretPick = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

/// A single-line input being edited by a key press.
pub(super) struct LineInput<'a> {
    pub text: &'a mut String,
    pub caret: &'a mut usize,
    pub selected_all: &'a mut bool,
    pub digits_only: bool,
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

fn insert(input: &mut LineInput, s: &str) {
    let s: String = if input.digits_only {
        s.chars().filter(|c| c.is_ascii_digit()).collect()
    } else {
        s.to_string()
    };
    if *input.selected_all {
        input.text.clear();
        *input.caret = 0;
        *input.selected_all = false;
    }
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

/// Applies an editing key press. Returns `true` when the key was consumed.
pub(super) fn handle_line_key(mut input: LineInput, keystroke: &Keystroke, cx: &mut App) -> bool {
    let len = char_len(input.text);
    *input.caret = (*input.caret).min(len);
    let caret = *input.caret;
    let mods = &keystroke.modifiers;
    let cmd = mods.platform || mods.control;
    let key = keystroke.key.as_str();

    if cmd && key.eq_ignore_ascii_case("a") {
        *input.selected_all = true;
        return true;
    }
    if cmd && key.eq_ignore_ascii_case("c") {
        cx.write_to_clipboard(ClipboardItem::new_string(input.text.clone()));
        return true;
    }
    if cmd && key.eq_ignore_ascii_case("x") {
        cx.write_to_clipboard(ClipboardItem::new_string(input.text.clone()));
        input.text.clear();
        *input.caret = 0;
        *input.selected_all = false;
        return true;
    }
    if cmd && key.eq_ignore_ascii_case("v") {
        if let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) {
            insert(&mut input, pasted.trim_matches(|c| c == '\r' || c == '\n'));
        }
        return true;
    }

    // Caret movement. With everything selected, Left/Right collapse to that edge.
    let target = match key {
        "left" if *input.selected_all => Some(0),
        "right" if *input.selected_all => Some(len),
        "left" if mods.platform => Some(0),
        "right" if mods.platform => Some(len),
        "left" if mods.alt => Some(prev_word(input.text, caret)),
        "right" if mods.alt => Some(next_word(input.text, caret)),
        "left" => Some(caret.saturating_sub(1)),
        "right" => Some((caret + 1).min(len)),
        "up" | "home" => Some(0),
        "down" | "end" => Some(len),
        _ => None,
    };
    if let Some(target) = target {
        *input.caret = target;
        *input.selected_all = false;
        return true;
    }

    match key {
        "backspace" | "delete" if *input.selected_all => {
            input.text.clear();
            *input.caret = 0;
            *input.selected_all = false;
        }
        "backspace" if mods.platform => remove_range(&mut input, 0, caret),
        "backspace" if mods.alt => {
            let from = prev_word(input.text, caret);
            remove_range(&mut input, from, caret);
        }
        "backspace" if caret > 0 => remove_range(&mut input, caret - 1, caret),
        "delete" if caret < len => remove_range(&mut input, caret, caret + 1),
        "backspace" | "delete" => {}
        "space" => insert(&mut input, " "),
        _ => {
            if cmd {
                return false;
            }
            let typed = keystroke
                .key_char
                .as_deref()
                .filter(|kc| kc.chars().count() == 1)
                .or_else(|| (key.chars().count() == 1).then_some(key));
            match typed {
                Some(s) => insert(&mut input, s),
                None => return false,
            }
        }
    }
    true
}

/// Renders the text of an input one character per cell. Clicking the left or right half
/// of a character reports the caret position before or after it through `on_pick`.
pub(super) fn render_caret_text(
    display: &str,
    caret: Option<usize>,
    caret_visible: bool,
    on_pick: &CaretPick,
) -> Vec<AnyElement> {
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
        el.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            pick(index, event.click_count, window, cx);
        })
    };

    let mut cells = Vec::new();
    for (i, ch) in display.chars().enumerate() {
        if caret == Some(i) {
            cells.push(caret_bar(caret_visible));
        }
        // A lone space shapes to zero width, so render it as a non-breaking space.
        let glyph = if ch == ' ' { '\u{00a0}' } else { ch };
        cells.push(
            div()
                .relative()
                .flex_none()
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
        Rc::new(move |index, clicks, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.active_field = Some(field);
                this.field_selected_all = clicks >= 2;
                this.field_cursor = index;
                this.cursor_visible = true;
                window.focus(&this.focus_handle);
                cx.notify();
            });
        })
    }

    /// Click handler for the characters of the filter bar.
    pub(super) fn filter_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, clicks, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.commit_cell_edit();
                this.blur_fetch_size();
                this.is_filter_focused = true;
                this.filter_selected_all = clicks >= 2;
                this.filter_cursor = index;
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
        Rc::new(move |index, clicks, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.focus_fetch_size(clicks >= 2, index);
                cx.notify();
            });
        })
    }

    /// Moves keyboard input to the fetch size box, leaving the filter bar
    pub(super) fn focus_fetch_size(&mut self, select_all: bool, caret: usize) {
        self.commit_cell_edit();
        self.is_filter_focused = false;
        self.is_fetch_size_focused = true;
        self.fetch_size_selected_all = select_all;
        self.fetch_size_cursor = caret;
        self.cursor_visible = true;
    }

    /// Leaves the fetch size box, keeping a valid typed size for the next fetch
    pub(super) fn blur_fetch_size(&mut self) {
        if self.is_fetch_size_focused {
            self.is_fetch_size_focused = false;
            self.fetch_size_selected_all = false;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.fetch.commit_size_text();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{insert, next_word, prev_word, remove_range, LineInput};

    fn edit(text: &str, caret: usize, f: impl FnOnce(&mut LineInput)) -> (String, usize) {
        let (mut text, mut caret, mut sel) = (text.to_string(), caret, false);
        f(&mut LineInput { text: &mut text, caret: &mut caret, selected_all: &mut sel, digits_only: false });
        (text, caret)
    }

    #[test]
    fn inserts_and_removes_at_the_caret() {
        assert_eq!(edit("id = 1", 2, |i| insert(i, "x")), ("idx = 1".to_string(), 3));
        assert_eq!(edit("ngày", 2, |i| insert(i, "à")), ("ngàày".to_string(), 3));
        assert_eq!(edit("ngày", 3, |i| remove_range(i, 2, 3)), ("ngy".to_string(), 2));
    }

    #[test]
    fn selection_is_replaced_by_typing() {
        let (mut text, mut caret, mut sel) = ("old".to_string(), 1, true);
        insert(&mut LineInput { text: &mut text, caret: &mut caret, selected_all: &mut sel, digits_only: false }, "n");
        assert_eq!((text.as_str(), caret, sel), ("n", 1, false));
    }

    #[test]
    fn digits_only_drops_other_chars() {
        let (mut text, mut caret, mut sel) = ("54".to_string(), 1, false);
        insert(&mut LineInput { text: &mut text, caret: &mut caret, selected_all: &mut sel, digits_only: true }, "3a2");
        assert_eq!((text.as_str(), caret), ("5324", 3));
    }

    #[test]
    fn word_jumps_skip_punctuation() {
        let text = "status = 'open' AND id";
        assert_eq!(prev_word(text, text.len()), 20);
        assert_eq!(prev_word(text, 9), 0);
        assert_eq!(next_word(text, 6), 14);
        assert_eq!(next_word(text, 0), 6);
    }
}
