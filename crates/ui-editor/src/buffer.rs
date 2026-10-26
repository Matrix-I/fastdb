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

    /// Char index of the caret
    fn cursor_char(&self) -> usize {
        self.rope.byte_to_char(self.cursor_offset)
    }

    /// Moves the caret to a char index, ending the current undo run
    fn set_cursor_char(&mut self, char_idx: usize) {
        let char_idx = char_idx.min(self.rope.len_chars());
        self.set_cursor_offset(self.rope.char_to_byte(char_idx));
    }

    /// Chars of `line_idx` without its trailing line break
    fn line_len_chars(&self, line_idx: usize) -> usize {
        let line = self.rope.line(line_idx);
        let mut len = line.len_chars();
        if len > 0 && line.char(len - 1) == '\n' {
            len -= 1;
            if len > 0 && line.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        len
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    pub fn move_left(&mut self) {
        self.set_cursor_char(self.cursor_char().saturating_sub(1));
    }

    pub fn move_right(&mut self) {
        self.set_cursor_char(self.cursor_char() + 1);
    }

    /// Alt+Left: to the start of the word before the caret
    pub fn move_word_left(&mut self) {
        let mut idx = self.cursor_char();
        while idx > 0 && !Self::is_word_char(self.rope.char(idx - 1)) {
            idx -= 1;
        }
        while idx > 0 && Self::is_word_char(self.rope.char(idx - 1)) {
            idx -= 1;
        }
        self.set_cursor_char(idx);
    }

    /// Alt+Right: to the end of the word after the caret
    pub fn move_word_right(&mut self) {
        let len = self.rope.len_chars();
        let mut idx = self.cursor_char();
        while idx < len && !Self::is_word_char(self.rope.char(idx)) {
            idx += 1;
        }
        while idx < len && Self::is_word_char(self.rope.char(idx)) {
            idx += 1;
        }
        self.set_cursor_char(idx);
    }

    /// Moves to the same column one line up, or to the start of the text on the first line
    pub fn move_up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            self.set_cursor_offset(0);
        } else {
            self.set_cursor_line_col(line - 1, col);
        }
    }

    /// Moves to the same column one line down, or to the end of the text on the last line
    pub fn move_down(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line + 1 >= self.rope.len_lines() {
            self.set_cursor_offset(self.len());
        } else {
            self.set_cursor_line_col(line + 1, col);
        }
    }

    pub fn move_line_start(&mut self) {
        let (line, _) = self.cursor_line_col();
        self.set_cursor_line_col(line, 0);
    }

    pub fn move_line_end(&mut self) {
        let (line, _) = self.cursor_line_col();
        self.set_cursor_line_col(line, usize::MAX);
    }

    pub fn move_to_start(&mut self) {
        self.set_cursor_offset(0);
    }

    pub fn move_to_end(&mut self) {
        self.set_cursor_offset(self.len());
    }

    /// Places the caret at `line` and char column `col`, both clamped to the text
    pub fn set_cursor_line_col(&mut self, line: usize, col: usize) {
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        let col = col.min(self.line_len_chars(line));
        self.set_cursor_char(self.rope.line_to_char(line) + col);
    }

    /// Replaces the bytes `start..end` with `text` as one undo step and puts the caret after it
    pub fn replace_range(&mut self, start: usize, end: usize, text: &str) {
        let end = end.min(self.len());
        let start = start.min(end);
        let char_start = self.rope.byte_to_char(start);
        let char_end = self.rope.byte_to_char(end);
        let old_text = self.rope.slice(char_start..char_end).to_string();
        self.rope.remove(char_start..char_end);
        self.rope.insert(char_start, text);
        self.record(TextEdit { offset: start, old_text, new_text: text.to_string() }, EditKind::Other);
        self.cursor_offset = start + text.len();
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

    #[test]
    fn arrows_move_by_char_and_stay_on_char_boundaries() {
        let mut buf = RopeyBuffer::new("ngày");
        assert_eq!(buf.cursor_offset(), "ngày".len());
        buf.move_left();
        assert_eq!(buf.cursor_offset(), "ngà".len());
        buf.move_left();
        assert_eq!(buf.cursor_offset(), "ng".len());
        buf.move_right();
        assert_eq!(buf.cursor_offset(), "ngà".len());
        buf.move_to_start();
        buf.move_left();
        assert_eq!(buf.cursor_offset(), 0);
        buf.move_to_end();
        buf.move_right();
        assert_eq!(buf.cursor_offset(), buf.len());
    }

    #[test]
    fn up_and_down_keep_the_column_clamped_to_the_line() {
        let mut buf = RopeyBuffer::new("SELECT id\nFROM t\nWHERE id = 1");
        buf.set_cursor_line_col(0, 8);
        buf.move_down();
        assert_eq!(buf.cursor_line_col(), (1, 6));
        buf.move_down();
        assert_eq!(buf.cursor_line_col(), (2, 6));
        buf.move_up();
        buf.move_up();
        assert_eq!(buf.cursor_line_col(), (0, 6));
        buf.move_up();
        assert_eq!(buf.cursor_line_col(), (0, 0));
        buf.move_to_end();
        buf.move_down();
        assert_eq!(buf.cursor_offset(), buf.len());
    }

    #[test]
    fn home_and_end_stop_at_the_line_break() {
        let mut buf = RopeyBuffer::new("SELECT 1;\nSELECT 2;");
        buf.set_cursor_line_col(0, 3);
        buf.move_line_end();
        assert_eq!(buf.cursor_line_col(), (0, 9));
        buf.move_line_start();
        assert_eq!(buf.cursor_line_col(), (0, 0));
        buf.set_cursor_line_col(1, 99);
        assert_eq!(buf.cursor_line_col(), (1, 9));
        buf.set_cursor_line_col(99, 0);
        assert_eq!(buf.cursor_line_col(), (1, 0));
    }

    #[test]
    fn word_jumps_skip_to_word_edges() {
        let mut buf = RopeyBuffer::new("SELECT user_id FROM t");
        buf.move_to_start();
        buf.move_word_right();
        assert_eq!(buf.cursor_offset(), 6);
        buf.move_word_right();
        assert_eq!(buf.cursor_offset(), 14);
        buf.move_word_left();
        assert_eq!(buf.cursor_offset(), 7);
    }

    #[test]
    fn replace_range_is_one_undo_step() {
        let mut buf = RopeyBuffer::new("SELECT * FROM cus");
        buf.replace_range(14, 17, "customers");
        assert_eq!(buf.text(), "SELECT * FROM customers");
        assert_eq!(buf.cursor_offset(), 23);
        buf.undo();
        assert_eq!(buf.text(), "SELECT * FROM cus");
        buf.redo();
        assert_eq!(buf.text(), "SELECT * FROM customers");
    }
}
