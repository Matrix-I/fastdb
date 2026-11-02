//! Application focus routing for the shared text input primitives.
use super::*;
use std::rc::Rc;
use ui_editor::line_input::PickKind;

impl Workspace {
    /// Escape cancels selected text before the owning dialog or cell edit is dismissed.
    pub(super) fn cancel_focused_text_selection(&mut self) -> bool {
        use ui_editor::line_input::clear_selection;
        if let Some(action) = self.script_action.as_mut() {
            return action.cancel_selection();
        }
        if let Some(chooser) = self.script_chooser.as_mut() {
            return clear_selection(
                &mut chooser.caret,
                &mut chooser.anchor,
                chooser.query.chars().count(),
            );
        }
        if self.form.show_connection_modal {
            let Some(field) = self.form.active_field else {
                return false;
            };
            let len = self.field_value_mut(field).chars().count();
            return clear_selection(
                &mut self.form.field_cursor,
                &mut self.form.field_anchor,
                len,
            );
        }
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return false;
        };
        if (!tab.is_script() || !tab.editor_focused)
            && let Some(edit) = tab.grid.editing_cell.as_mut()
        {
            return clear_selection(&mut edit.caret, &mut edit.anchor, edit.text.chars().count());
        }
        if self.input.is_fetch_size_focused {
            return clear_selection(
                &mut self.input.fetch_size_cursor,
                &mut self.input.fetch_size_anchor,
                tab.fetch.size_text.chars().count(),
            );
        }
        if self.input.is_filter_focused {
            let cleared = clear_selection(
                &mut self.input.filter_cursor,
                &mut self.input.filter_anchor,
                tab.filter_sql.chars().count(),
            );
            if cleared {
                self.input.filter_suggest = None;
            }
            return cleared;
        }
        if tab.is_script()
            && tab.editor_focused
            && !tab.file_busy
            && tab.editor.buffer.selection().is_some()
        {
            use ui_editor::text_edit::TextTarget;
            tab.editor.escape();
            return true;
        }
        false
    }

    pub(super) fn field_value_mut(&mut self, field: FormField) -> &mut String {
        match field {
            FormField::Name => &mut self.form.form_name,
            FormField::Host => &mut self.form.form_host,
            FormField::Port => &mut self.form.form_port,
            FormField::Database => &mut self.form.form_database,
            FormField::User => &mut self.form.form_user,
            FormField::Password => &mut self.form.form_password,
            FormField::TlsCa => &mut self.form.form_tls_ca_cert,
        }
    }

    /// Click handler for the characters of a connection form field.
    pub(super) fn field_caret_pick(&self, field: FormField, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.form.active_field == Some(field);
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                this.form.active_field = Some(field);
                apply_pick(
                    &mut this.form.field_cursor,
                    &mut this.form.field_anchor,
                    index,
                    kind,
                    was_active,
                );
                this.input.cursor_visible = true;
                window.focus(&this.focus_handle);
                cx.notify();
            });
        })
    }

    /// Click handler for the characters of the filter bar.
    pub(super) fn filter_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.input.is_filter_focused;
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                this.commit_cell_edit();
                this.blur_fetch_size();
                this.input.is_filter_focused = true;
                this.input.filter_suggest = None;
                apply_pick(
                    &mut this.input.filter_cursor,
                    &mut this.input.filter_anchor,
                    index,
                    kind,
                    was_active,
                );
                this.input.cursor_visible = true;
                cx.notify();
            });
        })
    }
}

impl Workspace {
    /// Click handler for the characters of the fetch size box.
    pub(super) fn fetch_size_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                let was_active = this.input.is_fetch_size_focused;
                if !was_active && kind == PickKind::Drag {
                    return;
                }
                if !was_active {
                    this.focus_fetch_size(false, index);
                }
                apply_pick(
                    &mut this.input.fetch_size_cursor,
                    &mut this.input.fetch_size_anchor,
                    index,
                    kind,
                    was_active,
                );
                this.input.cursor_visible = true;
                cx.notify();
            });
        })
    }

    /// Moves keyboard input to the fetch size box, leaving the filter bar
    pub(super) fn focus_fetch_size(&mut self, select_all: bool, caret: usize) {
        self.commit_cell_edit();
        self.input.is_filter_focused = false;
        self.input.is_fetch_size_focused = true;
        self.input.fetch_size_anchor = select_all.then_some(0);
        self.input.fetch_size_cursor = if select_all { usize::MAX } else { caret };
        self.input.cursor_visible = true;
    }

    /// Leaves the fetch size box, keeping a valid typed size for the next fetch
    pub(super) fn blur_fetch_size(&mut self) {
        if self.input.is_fetch_size_focused {
            self.input.is_fetch_size_focused = false;
            self.input.fetch_size_anchor = None;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.fetch.commit_size_text();
            }
        }
    }
}
