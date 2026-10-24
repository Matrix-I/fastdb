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
}
