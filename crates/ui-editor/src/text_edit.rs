//! Shared keyboard commands and clipboard dispatch for every editable text surface.
use gpui::{App, ClipboardItem, Keystroke, Modifiers};
use ui_core::history::UndoKey;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextCommand {
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Navigate { key: String, modifiers: Modifiers },
    Backspace { modifiers: Modifiers },
    Delete,
    Insert(String),
    Escape,
}

impl TextCommand {
    pub fn from_key(key: &Keystroke, multiline: bool) -> Option<Self> {
        let mods = key.modifiers;
        let command = mods.platform || mods.control;
        if let Some(undo) = UndoKey::from_keystroke(key) {
            return Some(match undo {
                UndoKey::Undo => Self::Undo,
                UndoKey::Redo => Self::Redo,
            });
        }
        let name = key.key.as_str();
        if command {
            match name.to_ascii_lowercase().as_str() {
                "a" => return Some(Self::SelectAll),
                "c" => return Some(Self::Copy),
                "x" => return Some(Self::Cut),
                "v" => return Some(Self::Paste),
                _ => {}
            }
        }
        if matches!(name, "left" | "right" | "up" | "down" | "home" | "end") {
            return Some(Self::Navigate {
                key: name.into(),
                modifiers: mods,
            });
        }
        match name {
            "backspace" => Some(Self::Backspace { modifiers: mods }),
            "delete" => Some(Self::Delete),
            "escape" => Some(Self::Escape),
            "enter" | "return" if multiline && !command => Some(Self::Insert("\n".into())),
            "tab" if multiline && !command => Some(Self::Insert("    ".into())),
            "space" if !command => Some(Self::Insert(" ".into())),
            _ if !command => key
                .key_char
                .as_deref()
                .filter(|s| s.chars().count() == 1)
                .or_else(|| (name.chars().count() == 1).then_some(name))
                .map(|s| Self::Insert(s.into())),
            _ => None,
        }
    }
}

/// Storage-specific operations; selection, commands and clipboard policy are shared.
pub trait TextTarget {
    fn select_all(&mut self);
    fn selected_text(&self) -> Option<String>;
    fn delete_selection(&mut self);
    fn insert(&mut self, text: &str);
    fn backspace(&mut self, modifiers: &Modifiers);
    fn delete_forward(&mut self);
    fn navigate(&mut self, key: &str, modifiers: &Modifiers);
    fn restore(&mut self, redo: bool);
    fn escape(&mut self);
}

pub trait TextClipboard {
    fn read(&mut self) -> Option<String>;
    fn write(&mut self, text: String);
}
impl TextClipboard for App {
    fn read(&mut self) -> Option<String> {
        self.read_from_clipboard().and_then(|item| item.text())
    }
    fn write(&mut self, text: String) {
        self.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

pub fn dispatch(
    target: &mut impl TextTarget,
    command: &TextCommand,
    clipboard: &mut impl TextClipboard,
) {
    match command {
        TextCommand::SelectAll => target.select_all(),
        TextCommand::Copy | TextCommand::Cut => {
            if let Some(text) = target.selected_text() {
                clipboard.write(text);
                if matches!(command, TextCommand::Cut) {
                    target.delete_selection();
                }
            }
        }
        TextCommand::Paste => {
            if let Some(text) = clipboard.read().filter(|text| !text.is_empty()) {
                target.insert(&text);
            }
        }
        TextCommand::Undo => target.restore(false),
        TextCommand::Redo => target.restore(true),
        TextCommand::Navigate { key, modifiers } => target.navigate(key, modifiers),
        TextCommand::Backspace { modifiers } => target.backspace(modifiers),
        TextCommand::Delete => target.delete_forward(),
        TextCommand::Insert(text) => target.insert(text),
        TextCommand::Escape => target.escape(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TextClipboard, TextCommand, dispatch};
    use crate::{
        SqlEditorView,
        line_input::{LineHistory, LineInput, apply_line_command},
    };
    use gpui::{Keystroke, Modifiers};
    use ui_core::theme::Theme;

    struct Clipboard(String);
    impl TextClipboard for Clipboard {
        fn read(&mut self) -> Option<String> {
            Some(self.0.clone())
        }
        fn write(&mut self, text: String) {
            self.0 = text;
        }
    }

    fn contract(mut execute: impl FnMut(&TextCommand, &mut Clipboard)) {
        let mut clipboard = Clipboard("untouched".into());
        execute(&TextCommand::Copy, &mut clipboard);
        execute(&TextCommand::Cut, &mut clipboard);
        assert_eq!(clipboard.0, "untouched");
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "Việt user_id");
        execute(&TextCommand::Insert("A 😀".into()), &mut clipboard);
        execute(&TextCommand::Undo, &mut clipboard);
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "Việt user_id");
        execute(&TextCommand::Redo, &mut clipboard);
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Cut, &mut clipboard);
        assert_eq!(clipboard.0, "A 😀");
        execute(&TextCommand::Paste, &mut clipboard);
        execute(
            &TextCommand::Navigate {
                key: "home".into(),
                modifiers: Modifiers::default(),
            },
            &mut clipboard,
        );
        execute(
            &TextCommand::Navigate {
                key: "right".into(),
                modifiers: Modifiers {
                    shift: true,
                    alt: true,
                    ..Default::default()
                },
            },
            &mut clipboard,
        );
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "A");
        clipboard.0 = "B".into();
        execute(&TextCommand::Paste, &mut clipboard);
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "B 😀");
        clipboard.0 = "unchanged".into();
        execute(&TextCommand::Escape, &mut clipboard);
        execute(&TextCommand::Escape, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "unchanged");
        execute(&TextCommand::Insert("!".into()), &mut clipboard);
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "B 😀!");
        execute(&TextCommand::Undo, &mut clipboard);
        execute(&TextCommand::SelectAll, &mut clipboard);
        execute(&TextCommand::Copy, &mut clipboard);
        assert_eq!(clipboard.0, "B 😀");
    }

    #[test]
    fn sql_and_single_line_inputs_share_clipboard_selection_and_undo_contract() {
        let mut sql = SqlEditorView::new("Việt user_id", Theme::dark(), None);
        contract(|command, clipboard| dispatch(&mut sql, command, clipboard));
        let mut text = "Việt user_id".to_string();
        let mut caret = text.chars().count();
        let mut anchor = None;
        let mut history = LineHistory::new();
        let mut line = LineInput {
            text: &mut text,
            caret: &mut caret,
            anchor: &mut anchor,
            history: &mut history,
            digits_only: false,
        };
        contract(|command, clipboard| apply_line_command(&mut line, command, clipboard));
    }

    #[test]
    fn shortcuts_share_command_recognition_and_preserve_application_keys() {
        let key = |name: &str, modifiers| Keystroke {
            key: name.into(),
            key_char: None,
            modifiers,
        };
        for modifiers in [
            Modifiers {
                platform: true,
                ..Default::default()
            },
            Modifiers {
                control: true,
                ..Default::default()
            },
        ] {
            for multiline in [true, false] {
                assert_eq!(
                    TextCommand::from_key(&key("A", modifiers), multiline),
                    Some(TextCommand::SelectAll)
                );
                assert_eq!(
                    TextCommand::from_key(&key("v", modifiers), multiline),
                    Some(TextCommand::Paste)
                );
                assert_eq!(TextCommand::from_key(&key("q", modifiers), multiline), None);
            }
        }
        assert_eq!(
            TextCommand::from_key(&key("enter", Modifiers::default()), false),
            None
        );
        assert_eq!(
            TextCommand::from_key(&key("enter", Modifiers::default()), true),
            Some(TextCommand::Insert("\n".into()))
        );
    }
    #[test]
    fn single_line_paste_preserves_validation_selection_and_undo() {
        let mut text = "5432".to_string();
        let mut caret = 4;
        let mut anchor = Some(0);
        let mut history = LineHistory::new();
        let mut line = LineInput {
            text: &mut text,
            caret: &mut caret,
            anchor: &mut anchor,
            history: &mut history,
            digits_only: true,
        };
        let mut clipboard = Clipboard("invalid".into());
        apply_line_command(&mut line, &TextCommand::Paste, &mut clipboard);
        assert_eq!(line.text, "5432");
        assert_eq!(*line.anchor, Some(0));
        clipboard.0 = "port 55438".into();
        apply_line_command(&mut line, &TextCommand::Paste, &mut clipboard);
        assert_eq!(line.text, "55438");
        apply_line_command(&mut line, &TextCommand::Undo, &mut clipboard);
        assert_eq!(line.text, "5432");
        line.digits_only = false;
        apply_line_command(&mut line, &TextCommand::SelectAll, &mut clipboard);
        clipboard.0 = "\r\nViệt\r\nSQL\n".into();
        apply_line_command(&mut line, &TextCommand::Paste, &mut clipboard);
        assert_eq!(line.text, "Việt SQL");
    }
}
