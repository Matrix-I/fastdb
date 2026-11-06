//! Literal search and replacement of local script text.
#![allow(dead_code)]
use super::script_tools::tool;
use super::*;

pub(super) struct Search {
    tab: u64,
    fields: [String; 2],
    active: usize,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    message: String,
}

fn find_next(buffer: &mut ui_editor::RopeyBuffer, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let text = buffer.text();
    let from = buffer.selection().map_or(buffer.cursor_offset(), |r| r.end);
    let found = text[from..]
        .find(needle)
        .map(|i| from + i)
        .or_else(|| text.find(needle));
    if let Some(start) = found {
        buffer.set_cursor_offset(start);
        buffer.move_selection(true, |b| b.set_cursor_offset(start + needle.len()));
        true
    } else {
        false
    }
}

fn replace_all(buffer: &mut ui_editor::RopeyBuffer, needle: &str, replacement: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let text = buffer.text();
    let count = text.matches(needle).count();
    if count > 0 && needle != replacement {
        buffer.replace_range(0, buffer.len(), &text.replace(needle, replacement));
    }
    count
}

impl Workspace {
    pub(super) fn open_script_search(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        self.script_search = Some(Search {
            tab: tab.id,
            fields: [
                tab.editor
                    .buffer
                    .selected_text()
                    .filter(|s| !s.contains(['\n', '\r']))
                    .unwrap_or_default(),
                String::new(),
            ],
            active: 0,
            caret: usize::MAX,
            anchor: Some(0),
            history: LineHistory::new(),
            message: String::new(),
        });
        cx.notify();
    }

    pub(super) fn search_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape" {
            let cleared = self.script_search.as_mut().is_some_and(|s| {
                ui_editor::line_input::clear_selection(
                    &mut s.caret,
                    &mut s.anchor,
                    s.fields[s.active].chars().count(),
                )
            });
            if !cleared {
                self.script_search = None;
            }
        } else if (key.modifiers.platform || key.modifiers.control) && key.key == "w" {
            self.script_search = None;
        } else if key.key == "enter" || key.key == "return" {
            self.search_command(0, cx);
        } else if let Some(search) = self.script_search.as_mut() {
            if key.key == "tab" {
                search.active = 1 - search.active;
                search.caret = usize::MAX;
                search.anchor = Some(0);
                search.history = LineHistory::new();
            } else {
                handle_line_key(
                    LineInput {
                        text: &mut search.fields[search.active],
                        caret: &mut search.caret,
                        anchor: &mut search.anchor,
                        digits_only: false,
                        history: &mut search.history,
                    },
                    key,
                    cx,
                );
                search.message.clear();
            }
        }
        cx.notify();
    }

    fn search_command(&mut self, command: usize, cx: &mut Context<Self>) {
        let Some(search) = self.script_search.as_mut() else {
            return;
        };
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|t| t.id == search.tab && t.is_script() && !t.file_busy)
        else {
            search.message = "The original script is unavailable or busy.".into();
            cx.notify();
            return;
        };
        let needle = &search.fields[0];
        let replacement = &search.fields[1];
        let before = tab.editor.buffer.revision();
        search.message = if needle.is_empty() {
            "Enter text to find.".into()
        } else if command == 2 {
            format!(
                "{} occurrence(s) replaced.",
                replace_all(&mut tab.editor.buffer, needle, replacement)
            )
        } else {
            if command == 1 && tab.editor.buffer.selected_text().as_ref() == Some(needle) {
                let range = tab.editor.buffer.selection().unwrap();
                tab.editor
                    .buffer
                    .replace_range(range.start, range.end, replacement);
            }
            if find_next(&mut tab.editor.buffer, needle) {
                "Match selected in the editor."
            } else {
                "No match."
            }
            .into()
        };
        tab.editor.hide_completions();
        tab.editor.caret_moved = true;
        if before != tab.editor.buffer.revision() {
            let (id, revision) = (tab.id, tab.editor.buffer.revision());
            self.schedule_autosave(id, revision, cx);
        }
        cx.notify();
    }

    pub(super) fn render_script_search(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search = self.script_search.as_ref().unwrap();
        let theme = &self.theme;
        let mut panel = div()
            .w(px(500.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_md()
            .bg(theme.bg_surface)
            .text_color(theme.text_primary)
            .child("Find / Replace — literal, case-sensitive");
        for field in 0..2 {
            let weak = cx.entity().downgrade();
            let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if let Some(search) = &mut this.script_search {
                        let was_active = search.active == field;
                        if !was_active && kind == ui_editor::line_input::PickKind::Drag {
                            return;
                        }
                        if !was_active {
                            search.history = LineHistory::new();
                        }
                        search.active = field;
                        apply_pick(
                            &mut search.caret,
                            &mut search.anchor,
                            index,
                            kind,
                            was_active,
                        );
                        cx.notify();
                    }
                });
            });
            let active = search.active == field;
            panel = panel
                .child(div().child(if field == 0 {
                    "Find"
                } else {
                    "Replace with (empty removes)"
                }))
                .child(
                    ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(if active {
                            theme.border_focused
                        } else {
                            theme.border
                        })
                        .child(render_field_content(
                            &search.fields[field],
                            "",
                            active,
                            if active { search.anchor } else { None },
                            search.caret,
                            true,
                            theme,
                            &pick,
                        )),
                );
        }
        panel = panel.child(search.message.clone()).child(
            div()
                .flex()
                .gap_2()
                .child(
                    tool("Find next")
                        .on_click(cx.listener(|this, _, _, cx| this.search_command(0, cx))),
                )
                .child(
                    tool("Replace")
                        .on_click(cx.listener(|this, _, _, cx| this.search_command(1, cx))),
                )
                .child(
                    tool("Replace all")
                        .on_click(cx.listener(|this, _, _, cx| this.search_command(2, cx))),
                )
                .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                    this.script_search = None;
                    cx.notify();
                }))),
        );
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::{find_next, replace_all};
    use ui_editor::RopeyBuffer;
    #[test]
    fn literal_unicode_search_wraps_without_changing_text() {
        let mut b = RopeyBuffer::new("名 SELECT 名 select");
        assert!(find_next(&mut b, "名"));
        assert_eq!(b.selection(), Some(0..3));
        assert!(find_next(&mut b, "名"));
        assert_eq!(b.selection(), Some(11..14));
        assert!(find_next(&mut b, "名"));
        assert_eq!(b.selection(), Some(0..3));
        assert!(!find_next(&mut b, "Select"));
        assert!(!find_next(&mut b, ""));
        assert_eq!(b.revision(), 0);
    }
    #[test]
    fn replacement_is_literal_single_undo_and_empty_removes() {
        let mut b = RopeyBuffer::new("名.*\r\n名.*");
        assert_eq!(replace_all(&mut b, ".*", "$1"), 2);
        assert_eq!(b.text(), "名$1\r\n名$1");
        b.undo();
        assert_eq!(b.text(), "名.*\r\n名.*");
        assert_eq!(replace_all(&mut b, "名", ""), 2);
        assert_eq!(b.text(), ".*\r\n.*");
        let revision = b.revision();
        assert_eq!(replace_all(&mut b, "", "x"), 0);
        assert_eq!(replace_all(&mut b, "absent", "x"), 0);
        assert_eq!(b.revision(), revision);
    }
    #[test]
    fn wrap_finds_a_match_spanning_the_initial_caret() {
        let mut b = RopeyBuffer::new("SELECT");
        b.set_cursor_offset(3);
        assert!(find_next(&mut b, "SELECT"));
        assert_eq!(b.selected_text().as_deref(), Some("SELECT"));
    }
}
