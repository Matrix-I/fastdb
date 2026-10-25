use ui_core::history::UndoKey;

use super::*;

impl Workspace {
    /// Handles every key press for the window: app shortcuts, the connection form,
    /// the filter bar and the SQL editor
    pub(super) fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        // Intercept Cmd+Q (macOS) / Ctrl+Q (Linux/Windows) unconditionally to close the app
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("q")
        {
            cx.quit();
            return;
        }

        // Intercept Cmd+W (macOS) / Ctrl+W (Linux/Windows) to close active tab
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("w")
        {
            if self.show_connection_modal {
                self.show_connection_modal = false;
                self.field_anchor = None;
            } else if !self.tabs.is_empty() {
                self.close_tab(self.active_tab);
            }
            cx.notify();
            return;
        }

        if self.column_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.column_menu = None;
                cx.notify();
            }
            return;
        }

        if self.nav_context_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.nav_context_menu = None;
                cx.notify();
            }
            return;
        }

        if self.show_connection_modal {
            if event.keystroke.key == "escape" {
                self.show_connection_modal = false;
                self.field_anchor = None;
                cx.notify();
                return;
            }
            if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                self.connect_to_database(cx);
                return;
            }
            if event.keystroke.key == "tab" {
                self.field_anchor = None;
                self.field_cursor = usize::MAX;
                self.cursor_visible = true;
                self.active_field = match (self.selected_driver_kind, self.active_field) {
                    (DbDriverKind::Sqlite, None) => Some(FormField::Name),
                    (DbDriverKind::Sqlite, Some(FormField::Name)) => Some(FormField::Database),
                    (DbDriverKind::Sqlite, Some(FormField::Database)) => Some(FormField::Name),
                    (DbDriverKind::Sqlite, _) => Some(FormField::Database),
                    (_, None) => Some(FormField::Name),
                    (_, Some(FormField::Name)) => Some(FormField::Host),
                    (_, Some(FormField::Host)) => Some(FormField::Port),
                    (_, Some(FormField::Port)) => Some(FormField::Database),
                    (_, Some(FormField::Database)) => Some(FormField::User),
                    (_, Some(FormField::User)) => Some(FormField::Password),
                    (_, Some(FormField::Password)) => Some(FormField::Name),
                };
                cx.notify();
                return;
            }
            if let Some(field) = self.active_field {
                let mut caret = self.field_cursor;
                let mut anchor = self.field_anchor;
                let mut history = self.form_history.remove(&field).unwrap_or_default();
                let input = LineInput {
                    text: self.field_value_mut(field),
                    caret: &mut caret,
                    anchor: &mut anchor,
                    digits_only: field == FormField::Port,
                    history: &mut history,
                };
                let consumed = handle_line_key(input, &event.keystroke, cx);
                self.form_history.insert(field, history);
                if consumed {
                    self.field_cursor = caret;
                    self.field_anchor = anchor;
                    self.cursor_visible = true;
                    cx.notify();
                }
            }
        } else if self.tabs.get(self.active_tab).is_some_and(|t| t.grid.editing_cell.is_some()) {
            self.handle_cell_edit_key(event, cx);
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("s")
        {
            self.save_pending_edits(cx);
            cx.notify();
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && (event.keystroke.key == "enter" || event.keystroke.key == "return")
        {
            self.run_active_query(cx);
        } else if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            if self.is_fetch_size_focused {
                if event.keystroke.key == "escape" {
                    tab.fetch.size_text = tab.fetch.size.to_string();
                    self.is_fetch_size_focused = false;
                    self.fetch_size_anchor = None;
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    // Like DBeaver, a new fetch size re-reads the result from the first row
                    self.run_active_query(cx);
                } else {
                    let input = LineInput {
                        text: &mut tab.fetch.size_text,
                        caret: &mut self.fetch_size_cursor,
                        anchor: &mut self.fetch_size_anchor,
                        digits_only: true,
                        history: &mut tab.fetch.size_history,
                    };
                    if handle_line_key(input, &event.keystroke, cx) {
                        self.cursor_visible = true;
                        cx.notify();
                    }
                }
            } else if self.is_filter_focused {
                if event.keystroke.key == "escape" {
                    self.is_filter_focused = false;
                    self.filter_anchor = None;
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    self.run_active_query(cx);
                } else {
                    let input = LineInput {
                        text: &mut tab.filter_sql,
                        caret: &mut self.filter_cursor,
                        anchor: &mut self.filter_anchor,
                        digits_only: false,
                        history: &mut tab.filter_history,
                    };
                    if handle_line_key(input, &event.keystroke, cx) {
                        self.cursor_visible = true;
                        cx.notify();
                    }
                }
            } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("c")
            {
                if let Some(text) = tab.grid.selection_text() {
                    let cells = text.split('\n').map(|l| l.split('\t').count()).sum::<usize>();
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.status_message = format!("📋 Copied {} cell(s) to clipboard", cells);
                    cx.notify();
                }
            } else if let Some(undo_key) = UndoKey::from_keystroke(&event.keystroke) {
                let redo = undo_key == UndoKey::Redo;
                if tab.table_name.is_none() {
                    if redo { tab.editor.redo() } else { tab.editor.undo() };
                } else if tab.active_subtab == TableSubTab::Data {
                    self.step_pending_edits(redo);
                }
                cx.notify();
            } else if tab.table_name.is_none() {
                if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("v")
                {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(pasted) = item.text() {
                            tab.editor.insert_text(&pasted);
                            cx.notify();
                        }
                    }
                } else if event.keystroke.key == "backspace" {
                    tab.editor.backspace();
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    tab.editor.insert_text("\n");
                    cx.notify();
                } else if event.keystroke.key == "space" {
                    tab.editor.insert_text(" ");
                    cx.notify();
                } else {
                    let input_char = if let Some(ref kc) = event.keystroke.key_char {
                        if kc.chars().count() == 1 {
                            Some(kc.as_str())
                        } else {
                            None
                        }
                    } else if event.keystroke.key.chars().count() == 1 {
                        Some(event.keystroke.key.as_str())
                    } else {
                        None
                    };

                    if let Some(char_str) = input_char {
                        if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                            tab.editor.insert_text(char_str);
                            cx.notify();
                        }
                    }
                }
            }
        }
    }

    /// Keys while a grid cell is edited in place: Enter keeps the text, Escape drops it,
    /// Tab moves on to the next column, Cmd+S keeps it and saves; the rest edit the text
    fn handle_cell_edit_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let cmd = keystroke.modifiers.platform || keystroke.modifiers.control;
        match keystroke.key.as_str() {
            "enter" | "return" => self.commit_cell_edit(),
            "escape" => self.cancel_cell_edit(),
            "tab" => {
                let next = self.tabs.get(self.active_tab).and_then(|t| {
                    let edit = t.grid.editing_cell.as_ref()?;
                    let num_cols = t.grid.pager.as_ref()?.num_cols() as usize;
                    let col = if keystroke.modifiers.shift { edit.col.checked_sub(1)? } else { edit.col + 1 };
                    (col < num_cols).then_some((edit.row, col))
                });
                self.commit_cell_edit();
                if let Some((row, col)) = next {
                    self.begin_cell_edit(row, col);
                }
            }
            "s" if cmd => self.save_pending_edits(cx),
            _ => {
                let Some(edit) = self.tabs.get_mut(self.active_tab).and_then(|t| t.grid.editing_cell.as_mut()) else { return };
                let input = LineInput {
                    text: &mut edit.text,
                    caret: &mut edit.caret,
                    anchor: &mut edit.anchor,
                    digits_only: false,
                    history: &mut self.cell_edit_history,
                };
                if !handle_line_key(input, keystroke, cx) {
                    return;
                }
                self.cursor_visible = true;
            }
        }
        cx.notify();
    }
}
