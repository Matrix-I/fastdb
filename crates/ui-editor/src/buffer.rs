use ropey::Rope;
use ui_core::history::EditKind;

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub offset: usize,
    pub old_text: String,
    pub new_text: String,
}

pub struct RopeyBuffer {
    rope: Rope,
    cursor_offset: usize,
    undo_stack: Vec<TextEdit>,
    redo_stack: Vec<TextEdit>,
    /// Kind of the last edit while the next one of the same kind joins its undo step
    open_run: Option<EditKind>,
}

impl Default for RopeyBuffer {
    fn default() -> Self {
        Self::new("")
    }
}

impl RopeyBuffer {
    pub fn new(initial_text: &str) -> Self {
        Self {
            rope: Rope::from_str(initial_text),
            cursor_offset: initial_text.len(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            open_run: None,
        }
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }

    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.cursor_offset = offset.min(self.len());
        self.open_run = None;
    }

    /// Records `edit` as an undo step, or folds it into the last step when it continues
    /// a run of typing or of backspaces, so Cmd+Z takes back a word rather than a letter
    fn record(&mut self, edit: TextEdit, kind: EditKind) {
        self.redo_stack.clear();
        let joins = kind != EditKind::Other && self.open_run == Some(kind);
        self.open_run = (kind != EditKind::Other).then_some(kind);
        if joins {
            if let Some(last) = self.undo_stack.last_mut() {
                match kind {
                    EditKind::Typing if last.offset + last.new_text.len() == edit.offset => {
                        last.new_text.push_str(&edit.new_text);
                        return;
                    }
                    EditKind::Deleting if edit.offset + edit.old_text.len() == last.offset => {
                        last.old_text.insert_str(0, &edit.old_text);
                        last.offset = edit.offset;
                        return;
                    }
                    _ => {}
                }
            }
        }
        self.undo_stack.push(edit);
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let char_idx = self.rope.byte_to_char(self.cursor_offset);
        let line_idx = self.rope.char_to_line(char_idx);
        let line_start_char = self.rope.line_to_char(line_idx);
        let col_idx = char_idx - line_start_char;
        (line_idx, col_idx)
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, line_idx: usize) -> Option<String> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line(line_idx).to_string())
    }

    pub fn insert_char(&mut self, ch: char) {
        let mut s = String::new();
        s.push(ch);
        self.insert_str(&s);
    }

    pub fn insert_str(&mut self, s: &str) {
        let offset = self.cursor_offset;
        let char_idx = self.rope.byte_to_char(offset);
        self.rope.insert(char_idx, s);

        let mut chars = s.chars();
        let word_char = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_whitespace());
        let kind = if word_char { EditKind::Typing } else { EditKind::Other };
        self.record(TextEdit { offset, old_text: String::new(), new_text: s.to_string() }, kind);

        self.cursor_offset += s.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor_offset == 0 {
            return;
        }

        let char_idx = self.rope.byte_to_char(self.cursor_offset);
        let prev_char_idx = char_idx - 1;
        let prev_byte = self.rope.char_to_byte(prev_char_idx);
        let removed_str = self.rope.slice(prev_char_idx..char_idx).to_string();

        self.rope.remove(prev_char_idx..char_idx);

        self.record(TextEdit { offset: prev_byte, old_text: removed_str, new_text: String::new() }, EditKind::Deleting);

        self.cursor_offset = prev_byte;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor_offset >= self.len() {
            return;
        }

        let char_idx = self.rope.byte_to_char(self.cursor_offset);
        if char_idx >= self.rope.len_chars() {
            return;
        }

        let next_char_idx = char_idx + 1;
        let removed_str = self.rope.slice(char_idx..next_char_idx).to_string();
        self.rope.remove(char_idx..next_char_idx);

        self.record(TextEdit { offset: self.cursor_offset, old_text: removed_str, new_text: String::new() }, EditKind::Other);
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo_stack.pop() else {
            return false;
        };

        let char_start = self.rope.byte_to_char(edit.offset);
        let new_char_len = edit.new_text.chars().count();
        self.rope.remove(char_start..char_start + new_char_len);

        if !edit.old_text.is_empty() {
            self.rope.insert(char_start, &edit.old_text);
        }

        self.cursor_offset = edit.offset + edit.old_text.len();
        self.open_run = None;
        self.redo_stack.push(edit);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo_stack.pop() else {
            return false;
        };

        let char_start = self.rope.byte_to_char(edit.offset);
        let old_char_len = edit.old_text.chars().count();
        self.rope.remove(char_start..char_start + old_char_len);

        if !edit.new_text.is_empty() {
            self.rope.insert(char_start, &edit.new_text);
        }

        self.cursor_offset = edit.offset + edit.new_text.len();
        self.open_run = None;
        self.undo_stack.push(edit);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ropey_buffer_edit_and_undo() {
        let mut buf = RopeyBuffer::new("SELECT *");
        buf.insert_str(" FROM users;");
        assert_eq!(buf.text(), "SELECT * FROM users;");

        buf.undo();
        assert_eq!(buf.text(), "SELECT *");

        buf.redo();
        assert_eq!(buf.text(), "SELECT * FROM users;");

        buf.backspace();
        assert_eq!(buf.text(), "SELECT * FROM users");
    }

    #[test]
    fn typed_words_and_backspace_runs_undo_as_one_step() {
        let mut buf = RopeyBuffer::new("");
        for ch in "SELECT 1".chars() {
            buf.insert_char(ch);
        }
        buf.undo();
        assert_eq!(buf.text(), "SELECT ");
        buf.undo();
        assert_eq!(buf.text(), "SELECT");
        buf.undo();
        assert_eq!(buf.text(), "");
        buf.redo();
        assert_eq!(buf.text(), "SELECT");

        buf.backspace();
        buf.backspace();
        buf.backspace();
        assert_eq!(buf.text(), "SEL");
        buf.undo();
        assert_eq!((buf.text().as_str(), buf.cursor_offset()), ("SELECT", 6));
    }

    #[test]
    fn moving_the_cursor_starts_a_new_step() {
        let mut buf = RopeyBuffer::new("");
        buf.insert_str("ab");
        buf.insert_char('c');
        buf.set_cursor_offset(0);
        buf.insert_char('x');
        buf.insert_char('y');
        assert_eq!(buf.text(), "xyabc");
        buf.undo();
        assert_eq!(buf.text(), "abc");
    }
}
