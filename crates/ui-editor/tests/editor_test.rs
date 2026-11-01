use ui_core::theme::Theme;
use ui_editor::{RopeyBuffer, SqlEditorView};

#[test]
fn test_buffer_ropey_operations() {
    let mut buf = RopeyBuffer::new("SELECT *");
    buf.insert_str(" FROM orders;");
    assert_eq!(buf.text(), "SELECT * FROM orders;");
    buf.undo();
    assert_eq!(buf.text(), "SELECT *");
    buf.redo();
    assert_eq!(buf.text(), "SELECT * FROM orders;");
}

#[test]
fn test_editor_active_statement_parsing() {
    let code = "SELECT 1;\nSELECT 2 FROM users;";
    let mut editor = SqlEditorView::new(code, Theme::dark(), None);

    editor.buffer.set_cursor_offset(3);
    let stmt1 = editor.active_statement();
    assert!(stmt1.contains("SELECT 1"));

    editor.buffer.set_cursor_offset(15);
    let stmt2 = editor.active_statement();
    assert!(stmt2.contains("SELECT 2 FROM users"));

    let code3 = "SELECT * FROM \"public\".\"additionalInfo\" LIMIT 100;";
    let mut editor3 = SqlEditorView::new(code3, Theme::dark(), None);
    // Default cursor is at code3.len()
    let stmt3 = editor3.active_statement();
    println!(
        "stmt3 at offset {}: {:?}",
        editor3.buffer.cursor_offset(),
        stmt3
    );
    assert_eq!(stmt3, code3);
}

#[test]
fn unicode_selections_replace_cut_delete_and_undo_as_one_edit() {
    let mut buffer = RopeyBuffer::new("select 'ngày';\nselect 2;");
    buffer.set_cursor_offset(8);
    buffer.move_selection(true, |b| b.move_word_right());
    assert_eq!(buffer.selected_text().as_deref(), Some("ngày"));
    buffer.insert_str("đêm");
    assert_eq!(buffer.text(), "select 'đêm';\nselect 2;");
    buffer.undo();
    assert_eq!(buffer.text(), "select 'ngày';\nselect 2;");
    buffer.select_all();
    assert!(buffer.delete_selection());
    assert!(buffer.is_empty());
    buffer.undo();
    assert_eq!(buffer.text(), "select 'ngày';\nselect 2;");
    buffer.select_all();
    buffer.delete_forward();
    assert!(buffer.is_empty());
    buffer.undo();
    buffer.set_cursor_offset(10); // inside a UTF-8 code point: clamp to its start
    assert!(buffer.text().is_char_boundary(buffer.cursor_offset()));
}

#[test]
fn shift_navigation_extends_and_plain_arrow_collapses_selection() {
    use gpui::{Keystroke, Modifiers};
    let mut editor = SqlEditorView::new("select 1;\nselect 2;", Theme::dark(), None);
    let key = |name: &str, shift| Keystroke {
        key: name.into(),
        key_char: None,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
    };
    editor.handle_key(&key("home", false));
    editor.handle_key(&key("up", true));
    assert_eq!(
        editor.buffer.selected_text().as_deref(),
        Some("select 1;\n")
    );
    assert_eq!(editor.active_statement(), "select 1;\n");
    editor.handle_key(&key("right", false));
    assert!(editor.buffer.selection().is_none());
    assert_eq!(editor.buffer.cursor_line_col(), (1, 0));
    editor.handle_key(&key("end", true));
    editor.handle_key(&key("backspace", false));
    assert_eq!(editor.buffer.text(), "select 1;\n");
    editor.undo();
    assert_eq!(editor.buffer.text(), "select 1;\nselect 2;");
}
