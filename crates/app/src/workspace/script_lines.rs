//! Line operations on local SQL text; these commands never execute SQL.
use ui_editor::buffer::RopeyBuffer;

#[derive(Clone, Copy)]
pub(super) enum LineCommand {
    Comment,
    Indent,
    Outdent,
}

pub(super) fn apply(buffer: &mut RopeyBuffer, command: LineCommand) {
    let text = buffer.text();
    let selection = buffer.selection();
    let range = selection
        .clone()
        .unwrap_or(buffer.cursor_offset()..buffer.cursor_offset());
    let start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    // An endpoint at the next line's start does not include that next line.
    let last = if range.end > range.start && text[..range.end].ends_with('\n') {
        range.end - 1
    } else {
        range.end
    };
    let end = text[last..].find('\n').map_or(text.len(), |i| last + i + 1);
    let source = &text[start..end];
    let uncomment = matches!(command, LineCommand::Comment)
        && source
            .split_inclusive('\n')
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start_matches([' ', '\t']).starts_with("--"));
    let mut replacement = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let body = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - body.len();
        match command {
            LineCommand::Comment if !line.trim().is_empty() => {
                replacement.push_str(&line[..indent]);
                if uncomment {
                    replacement.push_str(body.strip_prefix("-- ").unwrap_or(&body[2..]));
                } else {
                    replacement.push_str("-- ");
                    replacement.push_str(body);
                }
            }
            LineCommand::Indent => {
                replacement.push_str("    ");
                replacement.push_str(line);
            }
            LineCommand::Outdent => {
                let remove = if line.starts_with('\t') {
                    1
                } else {
                    line.bytes().take(4).take_while(|b| *b == b' ').count()
                };
                replacement.push_str(&line[remove..]);
            }
            _ => replacement.push_str(line),
        }
    }
    if replacement == source {
        return;
    }
    let caret = buffer.cursor_offset() - start;
    let edit_start = if matches!(command, LineCommand::Comment) {
        source.len() - source.trim_start_matches([' ', '\t']).len()
    } else {
        0
    };
    let mapped_caret = if caret <= edit_start {
        caret
    } else {
        caret
            .saturating_add_signed(replacement.len() as isize - source.len() as isize)
            .max(edit_start)
    };
    buffer.replace_range(start, end, &replacement);
    if selection.is_some() {
        buffer.set_cursor_offset(start);
        buffer.move_selection(true, |buffer| {
            buffer.set_cursor_offset(start + replacement.len())
        });
    } else {
        buffer.set_cursor_offset(
            start + mapped_caret.min(replacement.trim_end_matches(['\r', '\n']).len()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_lines_comment_and_uncomment_as_single_undo_steps() {
        let source = "  SELECT '名';\r\n\r\n\tSELECT 2;\r\nSELECT 3;";
        let mut buffer = RopeyBuffer::new(source);
        buffer.set_cursor_offset(2);
        buffer.move_selection(true, |b| {
            b.set_cursor_offset(source.find("SELECT 3").unwrap())
        });
        apply(&mut buffer, LineCommand::Comment);
        assert_eq!(
            buffer.text(),
            "  -- SELECT '名';\r\n\r\n\t-- SELECT 2;\r\nSELECT 3;"
        );
        let commented = buffer.text();
        apply(&mut buffer, LineCommand::Comment);
        assert_eq!(buffer.text(), source);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), commented);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn indent_outdent_preserve_selection_and_undo() {
        let mut buffer = RopeyBuffer::new("名\n  x\nlast");
        buffer.set_cursor_offset(0);
        buffer.move_selection(true, |b| b.set_cursor_offset("名\n  x\n".len()));
        apply(&mut buffer, LineCommand::Indent);
        assert_eq!(buffer.text(), "    名\n      x\nlast");
        assert_eq!(buffer.selected_text().unwrap(), "    名\n      x\n");
        apply(&mut buffer, LineCommand::Outdent);
        assert_eq!(buffer.text(), "名\n  x\nlast");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "    名\n      x\nlast");
    }

    #[test]
    fn caret_only_and_empty_input_are_safe() {
        let mut buffer = RopeyBuffer::new("SELECT 1;\n\t--SELECT 2;");
        apply(&mut buffer, LineCommand::Comment);
        assert_eq!(buffer.text(), "SELECT 1;\n\tSELECT 2;");
        apply(&mut buffer, LineCommand::Outdent);
        assert_eq!(buffer.text(), "SELECT 1;\nSELECT 2;");
        let mut empty = RopeyBuffer::new("");
        for command in [
            LineCommand::Comment,
            LineCommand::Indent,
            LineCommand::Outdent,
        ] {
            apply(&mut empty, command);
            assert_eq!(empty.text(), "");
            assert_eq!(empty.revision(), 0);
        }
    }

    #[test]
    fn repeated_caret_commands_stay_on_the_same_nonfinal_line() {
        let mut buffer = RopeyBuffer::new("SELECT 名;\nSELECT 2;");
        buffer.set_cursor_offset("SELECT 名".len());
        let original = buffer.text();
        let caret = buffer.cursor_offset();
        apply(&mut buffer, LineCommand::Comment);
        assert_eq!(buffer.cursor_offset(), caret + 3);
        apply(&mut buffer, LineCommand::Comment);
        assert_eq!(
            (buffer.text(), buffer.cursor_offset()),
            (original.clone(), caret)
        );
        apply(&mut buffer, LineCommand::Indent);
        apply(&mut buffer, LineCommand::Outdent);
        assert_eq!((buffer.text(), buffer.cursor_offset()), (original, caret));
    }
}
