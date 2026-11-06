//! SQL Snippets & Templates modal dialog.
//!
//! Provides searchable standard SQL templates that can be inserted into the editor buffer
//! at the current cursor offset or replacing selected text, with complete Undo support.
#![allow(dead_code)]

use super::script_tools::tool;
use super::*;
use sql_intel::{SqlSnippet, default_snippets, filter_snippets};

pub struct SnippetModal {
    pub tab_id: u64,
    pub filter_text: String,
    pub filter_caret: usize,
    pub filter_anchor: Option<usize>,
    pub filter_history: LineHistory,
    pub selected_index: usize,
    pub snippets: Vec<SqlSnippet>,
}

impl SnippetModal {
    pub fn new(tab_id: u64) -> Self {
        let snippets = default_snippets();
        Self {
            tab_id,
            filter_text: String::new(),
            filter_caret: 0,
            filter_anchor: None,
            filter_history: LineHistory::new(),
            selected_index: 0,
            snippets,
        }
    }

    pub fn update_filter(&mut self) {
        let all = default_snippets();
        self.snippets = filter_snippets(&all, &self.filter_text);
        if self.selected_index >= self.snippets.len() {
            self.selected_index = 0;
        }
    }

    pub fn selected_snippet(&self) -> Option<&SqlSnippet> {
        self.snippets.get(self.selected_index)
    }

    pub fn select_prev(&mut self) {
        if !self.snippets.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if !self.snippets.is_empty() && self.selected_index + 1 < self.snippets.len() {
            self.selected_index += 1;
        }
    }
}

impl Workspace {
    pub(super) fn open_snippet_modal(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        self.snippet_modal = Some(SnippetModal::new(tab.id));
        cx.notify();
    }

    pub(super) fn insert_selected_snippet(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &self.snippet_modal else {
            return;
        };
        let Some(snippet) = modal.selected_snippet().cloned() else {
            return;
        };
        let tab_id = modal.tab_id;
        self.snippet_modal = None;

        let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let tab = &mut self.tabs[tab_idx];
        if tab.file_busy {
            return;
        }

        let before_rev = tab.editor.buffer.revision();
        if let Some(range) = tab.editor.buffer.selection() {
            tab.editor
                .buffer
                .replace_range(range.start, range.end, snippet.template);
        } else {
            tab.editor.buffer.insert_str(snippet.template);
        }
        tab.editor.hide_completions();
        tab.editor.caret_moved = true;

        if before_rev != tab.editor.buffer.revision() {
            let (id, rev) = (tab.id, tab.editor.buffer.revision());
            self.schedule_autosave(id, rev, cx);
        }

        self.focus_script_editor(true);
        cx.notify();
    }

    pub(super) fn handle_snippet_modal_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.snippet_modal else {
            return;
        };

        if key.key == "escape" {
            self.snippet_modal = None;
            self.focus_script_editor(true);
            cx.notify();
            return;
        }

        if key.key == "enter" || key.key == "return" {
            self.insert_selected_snippet(cx);
            return;
        }

        if key.key == "up" {
            modal.select_prev();
            cx.notify();
            return;
        }

        if key.key == "down" {
            modal.select_next();
            cx.notify();
            return;
        }

        // Forward other keystrokes to the filter search field
        let input = LineInput {
            text: &mut modal.filter_text,
            caret: &mut modal.filter_caret,
            anchor: &mut modal.filter_anchor,
            digits_only: false,
            history: &mut modal.filter_history,
        };
        if handle_line_key(input, key, cx) {
            modal.update_filter();
            cx.notify();
        }
    }

    pub(super) fn render_snippet_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let modal = self.snippet_modal.as_ref().unwrap();
        let theme = &self.theme;

        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            if let Some(app) = weak.upgrade() {
                app.update(cx, |this, cx| {
                    if let Some(m) = &mut this.snippet_modal {
                        apply_pick(&mut m.filter_caret, &mut m.filter_anchor, index, kind, true);
                        cx.notify();
                    }
                });
            }
        });

        let mut list_pane = div()
            .id("snippet-list-pane")
            .flex()
            .flex_col()
            .gap_1()
            .w(px(320.0))
            .h(px(380.0))
            .overflow_y_scroll()
            .p_1()
            .border_r_1()
            .border_color(theme.border);

        if modal.snippets.is_empty() {
            list_pane = list_pane.child(
                div()
                    .p_3()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child("No matching snippets found"),
            );
        } else {
            for (idx, snip) in modal.snippets.iter().enumerate() {
                let is_sel = idx == modal.selected_index;
                let cat_color = match snip.category {
                    "Query" => rgb(0x58a6ff),
                    "DML" => rgb(0x3fb950),
                    "DDL" => rgb(0xbc8cff),
                    _ => rgb(0xd29922),
                };

                list_pane = list_pane.child(
                    div()
                        .id(ElementId::NamedInteger("snippet-item".into(), idx as u64))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .p_2()
                        .rounded_xs()
                        .cursor_pointer()
                        .bg(if is_sel {
                            theme.bg_surface_selected
                        } else {
                            theme.bg_surface
                        })
                        .hover(move |s| {
                            if !is_sel {
                                s.bg(theme.bg_surface_hover)
                            } else {
                                s
                            }
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = &mut this.snippet_modal {
                                m.selected_index = idx;
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(if is_sel {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(theme.text_primary)
                                        .child(snip.title),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_xs()
                                        .bg(theme.bg_app)
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(cat_color)
                                        .child(snip.category),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_secondary)
                                .child(snip.description),
                        ),
                );
            }
        }

        let selected = modal.selected_snippet();
        let preview_pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .h(px(380.0))
            .p_3()
            .gap_2()
            .bg(theme.bg_surface)
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text_primary)
                    .child(selected.map(|s| s.title).unwrap_or("No snippet selected")),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .child(selected.map(|s| s.description).unwrap_or("")),
            )
            .child(
                div()
                    .id("snippet-preview-template")
                    .flex_1()
                    .p_2p5()
                    .rounded_xs()
                    .bg(theme.bg_app)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_y_scroll()
                    .text_size(px(12.0))
                    .font_family(".AppleSystemUIFontMonospaced")
                    .text_color(theme.text_primary)
                    .child(selected.map(|s| s.template).unwrap_or("")),
            );

        let dialog = div()
            .w(px(740.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .shadow_lg()
            .text_color(theme.text_primary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .child("SQL Snippets & Templates"),
                    )
                    .child(tool("✕ Close").on_click(cx.listener(|this, _, _, cx| {
                        this.snippet_modal = None;
                        this.focus_script_editor(true);
                        cx.notify();
                    }))),
            )
            .child(
                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                    .px_2()
                    .py_1p5()
                    .border_1()
                    .border_color(theme.border_focused)
                    .rounded_xs()
                    .bg(theme.bg_app)
                    .child(render_field_content(
                        &modal.filter_text,
                        "Type to filter templates (e.g. select, cte, insert, ddl)...",
                        true,
                        modal.filter_anchor,
                        modal.filter_caret,
                        true,
                        theme,
                        &pick,
                    )),
            )
            .child(
                div()
                    .flex()
                    .w_full()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_xs()
                    .overflow_hidden()
                    .child(list_pane)
                    .child(preview_pane),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child("Navigate with ↑ / ↓ • Press Enter to insert at cursor"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(tool("Cancel (Esc)").on_click(cx.listener(|this, _, _, cx| {
                                this.snippet_modal = None;
                                this.focus_script_editor(true);
                                cx.notify();
                            })))
                            .child(
                                tool("Insert Snippet (Enter)")
                                    .bg(theme.bg_surface_selected)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.insert_selected_snippet(cx);
                                    })),
                            ),
                    ),
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
            .child(dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::SnippetModal;
    use ui_editor::RopeyBuffer;

    #[test]
    fn snippet_modal_navigation_and_filtering() {
        let mut modal = SnippetModal::new(1);
        assert!(!modal.snippets.is_empty());
        assert_eq!(modal.selected_index, 0);

        modal.select_next();
        assert_eq!(modal.selected_index, 1);
        modal.select_prev();
        assert_eq!(modal.selected_index, 0);

        modal.filter_text = "insert".into();
        modal.update_filter();
        assert!(!modal.snippets.is_empty());
        for s in &modal.snippets {
            assert!(
                s.title.to_lowercase().contains("insert")
                    || s.category.to_lowercase().contains("insert")
                    || s.id.to_lowercase().contains("insert")
                    || s.description.to_lowercase().contains("insert")
            );
        }
    }

    #[test]
    fn snippet_insertion_is_undoable() {
        let initial_text = "-- existing query\nSELECT 1;";
        let mut buffer = RopeyBuffer::new(initial_text);
        let rev0 = buffer.revision();

        // Select a snippet template and insert
        let modal = SnippetModal::new(1);
        let snippet = modal.selected_snippet().unwrap();

        buffer.insert_str(snippet.template);
        assert!(buffer.text().contains(snippet.template));
        assert!(buffer.revision() > rev0);

        // Undo reverts insertion
        assert!(buffer.undo());
        assert_eq!(buffer.text(), initial_text);
    }
}
