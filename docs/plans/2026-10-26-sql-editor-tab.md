# SQL Editor Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user open a DBeaver-style "SQL Script" tab, type free-form SQL with a real caret and autocompletion, run the statement under the caret with Cmd+Enter, and see its rows (or its error) in the result grid below the editor.

**Architecture:** The editor already exists as library code (`ui-editor::SqlEditorView` over a Ropey buffer, `sql-intel` for statement-at-cursor and completion) but is never rendered and has no caret movement. We (1) finish the editor inside `ui-editor` (caret navigation, key handling, completion that replaces the typed word, caret/scroll/click rendering), then (2) wire it into the app as a second kind of tab: a `TabState` with `table_name == None` renders an editor on top and the existing data panel + result footer below. Running reuses the existing streaming pipeline (`start_query`); `SELECT`/`WITH` statements without their own `LIMIT` are wrapped in a sub-select so the existing fetch-size / fetch-next paging keeps working.

**Tech Stack:** Rust (stable), GPUI 0.2.2, Ropey, tree-sitter (`tree_sitter_sequel`), flume, existing `db-wire` drivers.

## Global Constraints

- Code, comments, identifiers, commit messages and docs in English; match the surrounding style (doc comments on every pub item, short sentences, no Vietnamese in code).
- Buffer offsets in `RopeyBuffer` are **byte** offsets; every movement must land on a char boundary (Vietnamese and other multi-byte text must work).
- In `#[cfg(test)]` modules of files that `use gpui::*` or `use super::*`, do **not** glob-import `super::*` (it brings in `gpui::test`, which shadows `#[test]`); import the needed names explicitly, as `tab_context_menu.rs` does.
- One query runs at a time per window (`Workspace::is_running_query`); do not change that.
- Row editing stays table-tab only (`can_edit_active_rows` already requires `table_name.is_some()`); query tab results are read-only.
- Shortcuts: `Cmd+Enter` / `Ctrl+Enter` runs, `Cmd+]` / `Ctrl+]` opens a new SQL script (DBeaver's shortcut).
- No new crate dependencies.
- Commit after each task, conventional-commit style (`feat(ui-editor): …`, `feat(app): …`), no attribution lines.

## Out of scope (follow-up plans)

Execute whole script (Alt+X), multiple result tabs, text selection/copy inside the editor, query history, SQL formatting, EXPLAIN, affected-row counts for DML, persisting scripts to disk, a draggable editor/result splitter.

---

## File Structure

| File | Change | Responsibility |
| :--- | :--- | :--- |
| `crates/ui-editor/src/buffer.rs` | Modify | Caret navigation (left/right/up/down/home/end/doc start/end, line+col placement) and `replace_range` with undo |
| `crates/ui-editor/src/editor_view.rs` | Modify | `handle_key` for editing/navigation/completion keys; completion replaces the typed word and only opens on a word or after `.`; caret, monospace, scrolling and click-to-place rendering |
| `crates/app/src/workspace/sql_script.rs` | Create | Pure helpers: `paged_select` (wrap a SELECT in LIMIT/OFFSET) and `script_title` |
| `crates/app/src/workspace/query.rs` | Modify | `TabState::table` / `TabState::script` constructors, `script_sql` / `last_error` / `editor_focused` fields, `open_sql_script`, query-tab branch of `build_fetch_query`, error capture |
| `crates/app/src/workspace/script_view.rs` | Create | Render a query tab: toolbar (Run/Cancel), editor, error banner, data panel and result footer |
| `crates/app/src/workspace/table_view.rs` | Modify | Dispatch query tabs to `render_script_view`; extract the result footer into `render_result_footer` so both tab kinds share it |
| `crates/app/src/workspace/keyboard.rs` | Modify | Route keys to the editor when a query tab's editor has focus; `Cmd+]` |
| `crates/app/src/workspace/top_bar.rs` | Modify | SQL script icon for query tabs and a "+ SQL" button |
| `crates/app/src/workspace/nav_context_menu.rs` | Modify | "SQL Editor" item on a connection |
| `crates/app/src/workspace/mod.rs` | Modify | Register new modules, `next_script_number` field |
| `crates/app/src/assets.rs` | Modify | Test fixture uses `TabState::table` |
| `README.md` | Modify | Move the SQL editor from "Not built yet" to "What works today", add shortcuts |

---

### Task 1: Caret navigation and range replacement in `RopeyBuffer`

**Files:**
- Modify: `crates/ui-editor/src/buffer.rs`
- Test: `crates/ui-editor/src/buffer.rs` (existing `mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces (all on `RopeyBuffer`, all byte-offset based, all end the current undo run):
  - `pub fn move_left(&mut self)` / `pub fn move_right(&mut self)` — one char
  - `pub fn move_word_left(&mut self)` / `pub fn move_word_right(&mut self)` — Alt+arrows
  - `pub fn move_up(&mut self)` / `pub fn move_down(&mut self)` — keep the column, clamped to the target line
  - `pub fn move_line_start(&mut self)` / `pub fn move_line_end(&mut self)` — end excludes the line break
  - `pub fn move_to_start(&mut self)` / `pub fn move_to_end(&mut self)`
  - `pub fn set_cursor_line_col(&mut self, line: usize, col: usize)` — col in chars, both clamped
  - `pub fn replace_range(&mut self, start: usize, end: usize, text: &str)` — one undo step, caret after `text`

- [ ] **Step 1: Write the failing tests** (append inside the existing `mod tests`)

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui-editor buffer`
Expected: compile errors `no method named move_left found for struct RopeyBuffer` (and the other new methods).

- [ ] **Step 3: Implement** (add to `impl RopeyBuffer`, after `set_cursor_offset`)

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui-editor buffer`
Expected: all buffer tests PASS (3 existing + 5 new).

- [ ] **Step 5: Commit**

```bash
git add crates/ui-editor/src/buffer.rs
git commit -m "feat(ui-editor): move the caret by char, word and line, and replace ranges"
```

---

### Task 2: Editor key handling and word-replacing completion

**Files:**
- Modify: `crates/ui-editor/src/editor_view.rs`
- Test: `crates/ui-editor/src/editor_view.rs` (new `#[cfg(test)] mod tests` at the bottom)

**Interfaces:**
- Consumes: Task 1's `move_*`, `set_cursor_line_col`, `replace_range`.
- Produces:
  - `pub const MAX_VISIBLE_COMPLETIONS: usize = 6;`
  - `pub fn handle_key(&mut self, keystroke: &gpui::Keystroke) -> bool` — handles arrows (with Alt = word, Cmd = line start/end, Cmd+Up/Down = text start/end), Home/End, Backspace, Delete, Enter, Tab, Space, Escape and printable chars without Cmd/Ctrl; when the completion popup is open, Up/Down move the selection, Enter/Tab accept, Escape closes. Returns `false` for keys it does not handle (Cmd+V, Cmd+Z, Cmd+Enter … stay with the app).
  - `pub fn apply_selected_completion(&mut self) -> bool` — now replaces the word before the caret
  - `pub fn hide_completions(&mut self)`
  - `pub fn select_next_completion(&mut self)` / `pub fn select_prev_completion(&mut self)`
  - `pub caret_moved: bool` field — set by every change of caret position, read and cleared by rendering (Task 3) to scroll the caret into view.

- [ ] **Step 1: Write the failing tests** (new module at the end of `editor_view.rs`)

```rust
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
        assert!(ed.completion_items.iter().any(|i| i.label == "email"));
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
        let cmd_right = Keystroke { modifiers: Modifiers { platform: true, ..Default::default() }, key: "right".into(), key_char: None };
        assert!(ed.handle_key(&cmd_right));
        assert_eq!(ed.buffer.cursor_line_col(), (0, 9));
    }

    #[test]
    fn app_shortcuts_are_left_to_the_caller() {
        let mut ed = editor("SELECT 1");
        let cmd = |k: &str| Keystroke { modifiers: Modifiers { platform: true, ..Default::default() }, key: k.into(), key_char: Some(k.into()) };
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui-editor editor_view`
Expected: compile errors (`handle_key`, `caret_moved`, `MAX_VISIBLE_COMPLETIONS` not found).

- [ ] **Step 3: Implement**

Add the constant and field:

```rust
/// Completion entries shown in the popup; the selection wraps within them
pub const MAX_VISIBLE_COMPLETIONS: usize = 6;
```

Add `pub caret_moved: bool,` to `SqlEditorView` (doc comment: "Set when the caret moved since the last render, which scrolls it into view") and initialise it to `true` in `new`.

Replace `insert_text`, `backspace`, `update_completions` and `apply_selected_completion`, and add the new methods:

```rust
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
                "up" => { self.select_prev_completion(); return true; }
                "down" => { self.select_next_completion(); return true; }
                "enter" | "return" | "tab" if !cmd => return self.apply_selected_completion(),
                "escape" => { self.hide_completions(); return true; }
                _ => {}
            }
        }

        let moved = match key {
            "left" if cmd => { self.buffer.move_line_start(); true }
            "right" if cmd => { self.buffer.move_line_end(); true }
            "up" if cmd => { self.buffer.move_to_start(); true }
            "down" if cmd => { self.buffer.move_to_end(); true }
            "left" if mods.alt => { self.buffer.move_word_left(); true }
            "right" if mods.alt => { self.buffer.move_word_right(); true }
            "left" => { self.buffer.move_left(); true }
            "right" => { self.buffer.move_right(); true }
            "up" => { self.buffer.move_up(); true }
            "down" => { self.buffer.move_down(); true }
            "home" => { self.buffer.move_line_start(); true }
            "end" => { self.buffer.move_line_end(); true }
            _ => false,
        };
        if moved {
            self.caret_moved = true;
            self.hide_completions();
            return true;
        }

        if cmd {
            return false;
        }
        match key {
            "backspace" => self.backspace(),
            "delete" => {
                self.buffer.delete_forward();
                self.caret_moved = true;
                self.hide_completions();
            }
            "enter" | "return" => {
                self.insert_text("\n");
            }
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
```

Note: `undo` / `redo` must also set `self.caret_moved = true;`. `use gpui::*;` at the top already brings `Keystroke` into scope.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui-editor`
Expected: all ui-editor tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ui-editor/src/editor_view.rs
git commit -m "feat(ui-editor): handle editing keys and complete the word under the caret"
```

---

### Task 3: Render the caret, scroll to it and place it by mouse

**Files:**
- Modify: `crates/ui-editor/src/editor_view.rs`

**Interfaces:**
- Consumes: Task 2's `caret_moved`, Task 1's `set_cursor_line_col`.
- Produces:
  - `pub type EditorPick = std::rc::Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;` — called with `(line, col)` when the user clicks a char (left half → before it, right half → after it) or the empty space right of a line (→ line end).
  - `pub fn render_editor(&mut self, focused: bool, caret_visible: bool, on_pick: EditorPick) -> impl IntoElement`
  - `pub scroll_handle: ScrollHandle` field.

There is no GPUI render test harness in this repo, so this task is verified by running the app after Task 7. Keep it a pure rendering change.

- [ ] **Step 1: Add the field and constants**

```rust
const LINE_HEIGHT: f32 = 20.0;
const GUTTER_WIDTH: f32 = 48.0;
const MONO_FONT: &str = ".AppleSystemUIFontMonospaced, Menlo, monospace";
/// Approximate advance of one monospace char at 13 px, used to place the completion popup
const CHAR_WIDTH: f32 = 7.8;
```

Add `pub scroll_handle: ScrollHandle,` (doc: "Scroll position of the line list; the caret line is scrolled into view after it moves") initialised with `ScrollHandle::new()`.

- [ ] **Step 2: Rewrite `render_editor`**

Structure (replace the current body; keep the completion popup code but change its position):

```rust
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
            lines = lines.child(self.render_line(l, content, caret_col, caret_visible, &on_pick, &theme));
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

        if self.show_completions && !self.completion_items.is_empty() {
            let scroll_y = f32::from(self.scroll_handle.offset().y);
            let top = (cursor_line as f32 + 1.0) * LINE_HEIGHT + 4.0 + scroll_y;
            let left = GUTTER_WIDTH + 12.0 + cursor_col as f32 * CHAR_WIDTH;
            root = root.child(self.render_completion_popup(top, left, &theme));
        }
        root
    }
```

`render_line` (new private method): a `div().flex().h(px(LINE_HEIGHT)).w_full()` row containing
1. the gutter cell: `div().flex_none().w(px(GUTTER_WIDTH)).pr_2().flex().justify_end().items_center().bg(theme.bg_surface).border_r_1().border_color(theme.border).text_size(px(12.0))` with the 1-based line number, coloured `text_accent` on the caret line, `text_muted` otherwise;
2. the code cell: `div().flex().flex_1().items_center().pl_3()`, with `bg(theme.bg_surface_hover)` on the caret line, holding one cell per char. Colour each char by the word it belongs to using the existing keyword/string/number/comment `match` (split with `split_inclusive(' ')` as today, then emit a cell per char of the word). Each char cell copies `render_caret_text` from `crates/app/src/workspace/text_input.rs`: `relative().flex_none()`, a space rendered as `'\u{00a0}'`, and two absolutely positioned halves whose `on_mouse_down(MouseButton::Left, …)` calls `cx.stop_propagation()` then `on_pick(line, col, window, cx)` (left half → `col`, right half → `col + 1`). Insert a 1.5 px caret bar (`rgb(0x58a6ff)`, transparent while `!caret_visible`) before the char at `caret_col`, or after the last char when `caret_col == line len`;
3. after the chars, a `div().flex_1().h_full()` filler whose mouse-down calls `on_pick(line, usize::MAX, …)` so clicking right of the text puts the caret at the line end.

Move the existing popup code into `fn render_completion_popup(&self, top: f32, left: f32, theme: &Theme) -> impl IntoElement`, using `.top(px(top)).left(px(left))` and `.take(MAX_VISIBLE_COMPLETIONS)`.

Update `impl Render for SqlEditorView` to call `self.render_editor(true, true, std::rc::Rc::new(|_, _, _, _| {}))`.

- [ ] **Step 3: Build**

Run: `cargo build -p ui-editor && cargo test -p ui-editor`
Expected: builds without warnings; all tests still PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ui-editor/src/editor_view.rs
git commit -m "feat(ui-editor): draw the caret, keep it in view and place it by mouse"
```

---

### Task 4: Paging helper for script statements

**Files:**
- Create: `crates/app/src/workspace/sql_script.rs`
- Modify: `crates/app/src/workspace/mod.rs` (add `mod sql_script;`)

**Interfaces:**
- Produces:
  - `pub(super) fn paged_select(statement: &str, size: usize, offset: u64) -> Option<String>` — for a statement whose first keyword is `SELECT` or `WITH` and that contains no `LIMIT`/`FETCH FIRST`/`OFFSET`/`FOR UPDATE`, returns `SELECT * FROM (<statement without trailing ;>\n) fastdb_page LIMIT <size>[ OFFSET <offset>]`; otherwise `None` (the statement runs as written and is not paged). Leading `--` comment lines are skipped when finding the first keyword.
  - `pub(super) fn script_title(number: usize) -> String` — `"SQL Script 1"`, …

- [ ] **Step 1: Write the file with failing tests**

```rust
//! Helpers for SQL script tabs: paging a free-form SELECT like a table and naming the tabs.

/// Wraps a plain SELECT in a sub-select reading `size` rows from `offset`, so a script's
/// result is fetched in chunks like a table's. Returns `None` for any other statement, or
/// one that already limits or locks its rows, which then runs exactly as written.
pub(super) fn paged_select(statement: &str, size: usize, offset: u64) -> Option<String> {
    todo!()
}

/// Title of the `number`-th SQL script tab opened in this window
pub(super) fn script_title(number: usize) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::{paged_select, script_title};

    #[test]
    fn plain_selects_are_paged() {
        assert_eq!(
            paged_select("SELECT * FROM users;", 200, 0).as_deref(),
            Some("SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 200")
        );
        assert_eq!(
            paged_select("with t as (select 1) select * from t", 50, 100).as_deref(),
            Some("SELECT * FROM (with t as (select 1) select * from t\n) fastdb_page LIMIT 50 OFFSET 100")
        );
        assert_eq!(
            paged_select("-- latest users\nSELECT id FROM users ;; ", 10, 0).as_deref(),
            Some("SELECT * FROM (-- latest users\nSELECT id FROM users\n) fastdb_page LIMIT 10")
        );
    }

    #[test]
    fn statements_that_limit_lock_or_change_rows_run_as_written() {
        assert_eq!(paged_select("SELECT * FROM users LIMIT 5", 200, 0), None);
        assert_eq!(paged_select("select * from users fetch first 5 rows only", 200, 0), None);
        assert_eq!(paged_select("SELECT * FROM users FOR UPDATE", 200, 0), None);
        assert_eq!(paged_select("UPDATE users SET a = 1", 200, 0), None);
        assert_eq!(paged_select("SHOW TABLES", 200, 0), None);
        assert_eq!(paged_select("   ", 200, 0), None);
    }

    #[test]
    fn a_trailing_comment_does_not_swallow_the_closing_parenthesis() {
        assert_eq!(
            paged_select("SELECT 1 -- one", 10, 0).as_deref(),
            Some("SELECT * FROM (SELECT 1 -- one\n) fastdb_page LIMIT 10")
        );
    }

    #[test]
    fn script_titles_count_up() {
        assert_eq!(script_title(1), "SQL Script 1");
        assert_eq!(script_title(12), "SQL Script 12");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastdb-app sql_script`
Expected: FAIL with `not yet implemented` panics.

- [ ] **Step 3: Implement**

```rust
pub(super) fn paged_select(statement: &str, size: usize, offset: u64) -> Option<String> {
    let body = statement.trim().trim_end_matches(|c: char| c == ';' || c.is_whitespace());
    let first_keyword = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("--"))?
        .split(|c: char| !c.is_alphanumeric())
        .next()?
        .to_uppercase();
    if first_keyword != "SELECT" && first_keyword != "WITH" {
        return None;
    }
    let upper = body.to_uppercase();
    let words: Vec<&str> = upper.split(|c: char| !c.is_alphanumeric() && c != '_').collect();
    let has = |w: &str| words.contains(&w);
    if has("LIMIT") || has("OFFSET") || upper.contains("FETCH FIRST") || upper.contains("FETCH NEXT") || upper.contains("FOR UPDATE") {
        return None;
    }
    // The closing parenthesis goes on its own line so a trailing `--` comment cannot swallow it
    let mut sql = format!("SELECT * FROM ({body}\n) fastdb_page LIMIT {size}");
    if offset > 0 {
        sql.push_str(&format!(" OFFSET {offset}"));
    }
    Some(sql)
}

pub(super) fn script_title(number: usize) -> String {
    format!("SQL Script {number}")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastdb-app sql_script`
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/workspace/sql_script.rs crates/app/src/workspace/mod.rs
git commit -m "feat(app): page free-form SELECT statements like table data"
```

---

### Task 5: Query tab state, constructors and error capture

**Files:**
- Modify: `crates/app/src/workspace/query.rs`
- Modify: `crates/app/src/workspace/mod.rs` (`next_script_number: usize` field on `Workspace`, initialised to `1`)
- Modify: `crates/app/src/assets.rs` (fixture + tests)

**Interfaces:**
- Consumes: Task 4's `script_title`.
- Produces on `TabState`:
  - fields `pub script_sql: Option<String>` (statement of the current result, reused by fetch-next), `pub last_error: Option<String>` (message of the last failed run, cleared when a run starts), `pub editor_focused: bool` (query tabs: keys go to the editor rather than the grid)
  - `pub fn table(id: u64, schema: &str, table: &str, theme: Theme, cache: Arc<SchemaCache>) -> Self`
  - `pub fn script(id: u64, title: String, theme: Theme, cache: Arc<SchemaCache>) -> Self` — empty editor, `editor_focused: true`
  - `pub fn is_script(&self) -> bool` (= `table_name.is_none()`)
- Produces on `Workspace`: `pub fn open_sql_script(&mut self)` replacing the dead `new_tab`.

- [ ] **Step 1: Write the failing tests** (in `crates/app/src/assets.rs` tests module; replace the body of `sample_table_tab` with the constructor)

```rust
    fn sample_table_tab() -> crate::workspace::TabState {
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        crate::workspace::TabState::table(7, "public", "users", Theme::dark(), cache)
    }

    #[test]
    fn test_failed_run_keeps_its_message_until_the_next_run() {
        use db_wire::StreamEvent;

        let mut tab = sample_table_tab();
        tab.begin_fetch(true, false);
        tab.apply_stream_event(StreamEvent::Error("relation \"users\" does not exist".to_string()));
        assert_eq!(tab.last_error.as_deref(), Some("relation \"users\" does not exist"));

        tab.begin_fetch(true, false);
        assert_eq!(tab.last_error, None);
    }

    #[test]
    fn test_script_tabs_start_empty_with_the_editor_focused() {
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let tab = crate::workspace::TabState::script(3, "SQL Script 1".to_string(), Theme::dark(), cache);
        assert!(tab.is_script());
        assert!(tab.editor_focused);
        assert_eq!(tab.editor.buffer.text(), "");
        assert!(!sample_table_tab().is_script());
    }
```

Check that `sql-intel` is reachable from the app's tests (`sql-intel` is already an app dependency).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastdb-app test_`
Expected: compile errors (`no function or associated item named table`, `no field last_error`).

- [ ] **Step 3: Implement**

In `query.rs`, add the three fields to `TabState` with doc comments, then:

```rust
impl TabState {
    /// Tab showing the rows of `schema.table`, opened on its Data sub-tab
    pub fn table(id: u64, schema: &str, table: &str, theme: Theme, cache: Arc<SchemaCache>) -> Self {
        let mut grid = VirtualDataGrid::new(None, theme.clone());
        grid.set_table_name(table);
        Self {
            id,
            title: table.to_string(),
            schema_name: Some(schema.to_string()),
            table_name: Some(table.to_string()),
            editor: SqlEditorView::new("", theme, Some(cache)),
            grid,
            editor_focused: false,
            ..Self::blank(id)
        }
    }

    /// Free-form SQL script tab with an empty editor that has the keyboard
    pub fn script(id: u64, title: String, theme: Theme, cache: Arc<SchemaCache>) -> Self {
        Self {
            title,
            editor: SqlEditorView::new("", theme.clone(), Some(cache)),
            grid: VirtualDataGrid::new(None, theme),
            editor_focused: true,
            ..Self::blank(id)
        }
    }

    /// Whether this tab runs SQL typed in its editor rather than reading a table
    pub fn is_script(&self) -> bool {
        self.table_name.is_none()
    }

    fn blank(id: u64) -> Self {
        Self {
            id,
            title: String::new(),
            schema_name: None,
            table_name: None,
            filter_sql: String::new(),
            filter_history: Default::default(),
            active_subtab: TableSubTab::Data,
            active_property_section: PropertySection::Columns,
            properties_header_collapsed: false,
            view_mode: DataViewMode::Grid,
            show_sql_dialog: false,
            editor: SqlEditorView::new("", Theme::dark(), None),
            grid: VirtualDataGrid::new(None, Theme::dark()),
            execution_time_ms: None,
            fetch: FetchState::default(),
            script_sql: None,
            last_error: None,
            editor_focused: false,
        }
    }
}
```

In `begin_fetch` add `self.last_error = None;`. In `fail_fetch` add `self.last_error = Some(error.clone());` before returning.

In `query_table`, replace the `TabState { … }` literal with `self.tabs.push(TabState::table(id, schema, table, self.theme.clone(), self.schema_cache.clone()));`.

Replace `new_tab` (and its `#[allow(dead_code)]`) with:

```rust
    /// Opens an empty SQL script tab for the active connection (Cmd+], "+ SQL")
    pub fn open_sql_script(&mut self) {
        let title = script_title(self.next_script_number);
        self.next_script_number += 1;
        let id = self.allocate_tab_id();
        self.tabs.push(TabState::script(id, title, self.theme.clone(), self.schema_cache.clone()));
        self.active_tab = self.tabs.len() - 1;
        self.is_filter_focused = false;
        self.blur_fetch_size();
    }
```

with `use super::sql_script::{paged_select, script_title};` at the top of `query.rs`. Also remove the `#[allow(dead_code)]` on `Workspace::schema_cache` in `mod.rs`.

In `connections.rs:400-412` the code resets every query tab's editor to a "welcome" text on connect; delete the reset (keep only `tab.editor.schema_cache = Some(...)`) so a user's script survives reconnects.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastdb-app`
Expected: all tests PASS, including the existing stream/fetch tests that now use the constructor.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/workspace/query.rs crates/app/src/workspace/mod.rs crates/app/src/workspace/connections.rs crates/app/src/assets.rs
git commit -m "feat(app): add SQL script tabs and remember why the last run failed"
```

---

### Task 6: Run the statement under the caret

**Files:**
- Modify: `crates/app/src/workspace/query.rs`
- Test: `crates/app/src/assets.rs`

**Interfaces:**
- Consumes: Task 4's `paged_select`, Task 5's `script_sql`, `is_script`.
- Produces:
  - `pub fn script_fetch_query(tab: &TabState, offset: u64) -> (String, bool)` (free function in `query.rs`) — `(sql, paged)` for a script tab's `script_sql`
  - `run_active_query` records `editor.active_statement()` into `script_sql` before building the query for script tabs; `fetch_next_rows` keeps using the stored `script_sql`.

- [ ] **Step 1: Write the failing test** (in `assets.rs` tests)

```rust
    #[test]
    fn test_script_fetch_query_pages_selects_and_runs_other_statements_as_written() {
        use crate::workspace::{script_fetch_query, TabState};
        use ui_core::theme::Theme;

        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::script(1, "SQL Script 1".to_string(), Theme::dark(), cache);
        tab.fetch.size = 100;

        tab.script_sql = Some("SELECT * FROM users;".to_string());
        assert_eq!(
            script_fetch_query(&tab, 0),
            ("SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100".to_string(), true)
        );
        assert_eq!(
            script_fetch_query(&tab, 100).0,
            "SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100 OFFSET 100"
        );

        tab.script_sql = Some("DELETE FROM users WHERE id = 1;".to_string());
        assert_eq!(script_fetch_query(&tab, 0), ("DELETE FROM users WHERE id = 1;".to_string(), false));

        tab.script_sql = None;
        assert_eq!(script_fetch_query(&tab, 0), (String::new(), false));
    }
```

Export it from `mod.rs`: `pub use query::{group_thousands, script_fetch_query, TabState};`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p fastdb-app test_script_fetch_query`
Expected: compile error `unresolved import crate::workspace::script_fetch_query`.

- [ ] **Step 3: Implement**

```rust
/// SQL reading one chunk of a script tab's statement from `offset`, and whether it is paged.
/// Statements that are not plain SELECTs run as written, in one go.
pub fn script_fetch_query(tab: &TabState, offset: u64) -> (String, bool) {
    let Some(ref statement) = tab.script_sql else {
        return (String::new(), false);
    };
    match paged_select(statement, tab.fetch.size, offset) {
        Some(sql) => (sql, true),
        None => (statement.clone(), false),
    }
}
```

In `build_fetch_query`, replace the whole `_ => { … }` arm that reads the editor text with:

```rust
            _ => return script_fetch_query(tab, offset),
```

(the `!tab.title.starts_with("Query ")` fallback goes away: table tabs always have `table_name`).

In `run_active_query`, before `let (sql, paged) = …`:

```rust
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.is_script() {
                let statement = tab.editor.active_statement();
                tab.script_sql = (!statement.is_empty()).then_some(statement);
            }
        }
```

`start_query` already ignores an empty SQL string, so an empty editor does nothing.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastdb-app`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/workspace/query.rs crates/app/src/workspace/mod.rs crates/app/src/assets.rs
git commit -m "feat(app): run the SQL statement under the caret and fetch its rows in chunks"
```

---

### Task 7: Script tab layout, focus and keyboard routing

**Files:**
- Create: `crates/app/src/workspace/script_view.rs`
- Modify: `crates/app/src/workspace/table_view.rs`, `keyboard.rs`, `top_bar.rs`, `nav_context_menu.rs`, `mod.rs` (`mod script_view;`)

**Interfaces:**
- Consumes: Task 3's `render_editor(focused, caret_visible, on_pick)` and `EditorPick`; Task 5's `editor_focused`, `last_error`, `open_sql_script`; existing `render_data_panel`, `run_active_query`, `cancel_running_query`, `render_edit_toolbar`.
- Produces:
  - `pub(super) fn render_script_view(&mut self, tab_idx: usize, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - `pub(super) fn render_result_footer(&mut self, tab_idx: usize, cx: &mut Context<Self>) -> impl IntoElement` (extracted from `render_table_view` section E, unchanged behaviour)
  - `pub(super) fn render_error_banner(&self, message: &str) -> impl IntoElement`

UI has no automated tests; verify by running the app (Step 6).

- [ ] **Step 1: Extract the result footer**

Move section E of `render_table_view` (`table_view.rs`, the `if active_subtab == TableSubTab::Data { … }` block building the footer) verbatim into `fn render_result_footer(&mut self, tab_idx: usize, cx: &mut Context<Self>) -> impl IntoElement` in `table_view.rs`, reading `has_more`, `is_appending`, `fetch.last`, `loaded_rows`, `fetch.size_text` from `self.tabs[tab_idx]` inside the function. The call site becomes `center_col = center_col.child(self.render_result_footer(active_tab_idx, cx));`. Run `cargo build -p fastdb-app` — must compile with identical behaviour.

- [ ] **Step 2: Error banner for both tab kinds**

In `script_view.rs`:

```rust
    /// Red strip under the toolbar with the message of the last failed run
    pub(super) fn render_error_banner(&self, message: &str) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .gap_2()
            .w_full()
            .max_h(px(96.0))
            .px_3()
            .py_2()
            .bg(rgba(0xf8514926))
            .border_b_1()
            .border_color(rgb(0xf85149))
            .text_size(px(12.0))
            .text_color(rgb(0xff7b72))
            .overflow_hidden()
            .child(div().flex_none().child("✕"))
            .child(div().flex_1().child(message.to_string()))
    }
```

In `render_table_view`, insert it above the data panel when `self.tabs[active_tab_idx].last_error` is `Some` and the Data sub-tab is active.

- [ ] **Step 3: `render_script_view`**

In `render_table_view`, right after the empty-state branch, dispatch: `else if self.tabs[active_tab_idx].is_script() { self.render_script_view(active_tab_idx, window, cx).into_any_element() }`.

`render_script_view` builds a `flex_col size_full` column:
1. **Toolbar** (`h(px(28.0))`, `bg_surface`, `border_b_1`): a Run button (`id("btn-script-run")`, icon `icons/sql_exec.svg`, label "Execute (⌘↵)") that calls `this.run_active_query(cx)`; while `self.is_running_query` it becomes "Cancel" calling `this.cancel_running_query(cx)` — mirror the existing table toolbar button styles in `table_view.rs`.
2. **Editor** (`div().h(relative(0.45)).w_full().border_b_1().border_color(theme.border)`): `on_mouse_down(MouseButton::Left, …)` sets `tab.editor_focused = true`, commits any cell edit, blurs the fetch size and filter, `window.focus(&this.focus_handle)`; child is `tab.editor.render_editor(tab.editor_focused, self.cursor_visible, pick)`, where `pick` is:

```rust
        let workspace = cx.entity().downgrade();
        let pick: EditorPick = Rc::new(move |line, col, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.get_mut(tab_idx) {
                    tab.editor_focused = true;
                    tab.editor.buffer.set_cursor_line_col(line, col);
                    tab.editor.hide_completions();
                }
                this.cursor_visible = true;
                window.focus(&this.focus_handle);
                cx.notify();
            });
        });
```

3. **Error banner** when `last_error` is set.
4. **Result area** (`flex_1`, `on_mouse_down` sets `tab.editor_focused = false`): `self.render_data_panel(tab_idx, window, cx)`; when the result has zero columns (a statement without a result set, `grid.pager.as_ref().is_some_and(|p| p.num_cols() == 0)`) render instead a centred muted line "Statement executed successfully." with the fetch summary's elapsed time.
5. **Footer**: `self.render_result_footer(tab_idx, cx)`.

- [ ] **Step 4: Keyboard routing** (`keyboard.rs`)

1. Next to the Cmd+W intercept, add `Cmd+]` / `Ctrl+]` → `self.open_sql_script(); cx.notify(); return;` (skip when `show_connection_modal`).
2. In the final `else if !self.tabs.is_empty()` branch, replace the whole `tab.table_name.is_none()` editor block **and** the undo branch's `if tab.table_name.is_none()` arm with one routing step placed before the grid's Cmd+C handling:

```rust
            } else if tab.is_script() && tab.editor_focused {
                let cmd = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if let Some(undo_key) = UndoKey::from_keystroke(&event.keystroke) {
                    if undo_key == UndoKey::Redo { tab.editor.redo(); } else { tab.editor.undo(); }
                } else if cmd && event.keystroke.key.eq_ignore_ascii_case("v") {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        tab.editor.insert_text(&text);
                    }
                } else if !tab.editor.handle_key(&event.keystroke) {
                    return;
                }
                self.cursor_visible = true;
                cx.notify();
            }
```

   Cmd+Enter is already handled earlier and calls `run_active_query`, which now reads the statement at the caret.
3. Escape with the editor focused and no completion popup open does nothing (handled by `handle_key`).

- [ ] **Step 5: Entry points**

- `top_bar.rs`: tab icon is `if tab.is_script() { "icons/sql_script.svg" } else { "icons/table.svg" }`; after the tab loop add a `div().id("btn-new-sql-script")` "+ SQL" button (`text_size(px(11.0))`, `text_muted`, hover `bg_surface_hover`, `cursor_pointer`) that calls `this.open_sql_script(); cx.notify();`.
- `nav_context_menu.rs`: add `NavMenuAction::OpenSqlEditor` with item label "SQL Editor" and icon `icons/sql_script.svg`, enabled only when the connection is live; the action switches to that connection if needed (reuse the `Connect` path when not active) and calls `self.open_sql_script()`.

- [ ] **Step 6: Verify in the app**

Run: `cargo run -p fastdb-app` (connect to a local PostgreSQL or a SQLite file).
Check, one by one:
1. Cmd+] opens "SQL Script 1" with a blinking caret in the editor.
2. Type `SELECT * FROM cu` → popup lists matching tables; ↓/↑ move, Tab inserts the name in place of `cu`.
3. Type two statements on separate lines; put the caret on the second; Cmd+Enter runs only it and the grid shows its rows.
4. For a table with more rows than the fetch size, scrolling to the end of the grid fetches the next chunk (footer shows `200+`).
5. `SELECT * FROM missing_table` shows the red banner with the driver error; fixing the text and re-running clears it.
6. `UPDATE …` / `CREATE TABLE …` shows "Statement executed successfully.".
7. Clicking a character places the caret there; clicking right of a line puts it at the line end; a long script scrolls and the caret stays in view while typing.
8. Clicking the grid then pressing Cmd+C copies cells; clicking the editor again sends keys back to the editor.
9. Table tabs still behave as before (filter bar, edit/save rows, fetch next, Cmd+Z on staged edits).

- [ ] **Step 7: Commit**

```bash
git add crates/app/src/workspace/
git commit -m "feat(app): open SQL script tabs with an editor above the result grid"
```

---

### Task 8: Documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README**

- Status paragraph: drop "The free-form SQL editor" from the list of missing features.
- "What works today": add a **SQL editor** subsection — script tabs (Cmd+] / "+ SQL" / navigator "SQL Editor"), run statement at caret, schema-aware completion, SELECTs fetched in chunks, errors shown above the result.
- "Not built yet": replace the "Free-form SQL editor tabs" bullet with "**SQL editor extras.** Running a whole script, several result tabs, selection and copy inside the editor, query history and formatting are not built yet."
- Shortcut table: add `Cmd + ]` / `Ctrl + ]` "Open a new SQL script", change `Cmd + Enter` description to "Run the active tab's query (in a SQL script: the statement under the caret)", and add editor keys (arrows, Alt+arrows, Cmd+arrows, Tab / Enter accept a completion, Esc closes it).
- Workspace crates table: `sql-intel` → "Used"; `ui-editor` → "Used".

- [ ] **Step 2: Run the whole test suite**

Run: `cargo test --workspace`
Expected: all PASS (driver tests that need live PostgreSQL/MySQL are skipped or pass as on `main`).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: describe SQL script tabs and their shortcuts"
```

---

## Self-review notes

- Spec coverage: open tab (T5, T7), caret + typing + navigation (T1–T3), completion (T2), run statement at caret (T6), paging (T4, T6), error display (T5, T7), docs (T8).
- Known limitations kept on purpose: the editor renders every line and every char as its own element (fine for scripts of a few hundred lines; virtualise later), the completion popup's horizontal position uses an approximate monospace char width, and there is no text selection in the editor yet.
