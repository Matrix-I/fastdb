use ui_core::history::UndoKey;

use super::*;

impl Workspace {
    /// Handles every key press for the window: app shortcuts, the connection form,
    /// the filter bar and the SQL editor
    pub(super) fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && matches!(event.keystroke.key.as_str(), "c" | "x")
        {
            // A newer clipboard action supersedes any background grid copy.
            self.clipboard_request_id += 1;
        }
        let before = self
            .tabs
            .get(self.active_tab)
            .map(|t| (t.id, t.editor.buffer.revision()));
        self.handle_workspace_key_down(event, cx);
        if let Some((id, revision)) = before
            && let Some(tab) = self.tabs.iter().find(|t| t.id == id)
            && tab.editor.buffer.revision() != revision
        {
            self.schedule_autosave(id, tab.editor.buffer.revision(), cx);
        }
    }
    fn handle_workspace_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        // Intercept Cmd+Q (macOS) / Ctrl+Q (Linux/Windows) unconditionally to close the app
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("q")
        {
            self.request_quit(cx);
            return;
        }

        if self.show_about_dialog {
            if event.keystroke.key == "escape"
                || ((event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("w"))
            {
                self.show_about_dialog = false;
                cx.notify();
            }
            return;
        }

        if event.keystroke.key == "escape"
            && !self.updates.read(cx).is_open()
            && self.menus.grid_context_menu.is_none()
            && self.menus.column_menu.is_none()
            && self.menus.nav_context_menu.is_none()
            && self.cancel_focused_text_selection()
        {
            self.input.cursor_visible = true;
            cx.notify();
            return;
        }

        if self.script_action.is_some() {
            self.handle_script_action_key(&event.keystroke, cx);
            return;
        }

        // Intercept Cmd+W (macOS) / Ctrl+W (Linux/Windows) to close active tab
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("w")
        {
            if self.form.show_connection_modal {
                self.form.invalidate();
                self.form.show_connection_modal = false;
                self.form.field_anchor = None;
            } else if !self.tabs.is_empty() {
                self.close_tab(self.active_tab, cx);
            }
            cx.notify();
            return;
        }

        if self.updates.read(cx).is_open() {
            if event.keystroke.key == "escape" {
                self.updates.update(cx, |updates, cx| updates.dismiss(cx));
            }
            return;
        }

        if self.script_chooser.is_some() {
            self.handle_script_chooser_key(&event.keystroke, cx);
            return;
        }

        // Cmd+] (macOS) / Ctrl+] opens a new SQL script, as in DBeaver
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key == "]"
            && !self.form.show_connection_modal
        {
            self.open_sql_script(None, cx);
            cx.notify();
            return;
        }

        if self.menus.grid_context_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.menus.grid_context_menu = None;
                cx.notify();
            }
            return;
        }

        if self.menus.column_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.menus.column_menu = None;
                cx.notify();
            }
            return;
        }

        if self.menus.nav_context_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.menus.nav_context_menu = None;
                cx.notify();
            }
            return;
        }

        if self.form.show_connection_modal {
            if event.keystroke.key == "escape" {
                self.form.invalidate();
                self.form.show_connection_modal = false;
                self.form.field_anchor = None;
                cx.notify();
                return;
            }
            if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                self.connect_to_database(cx);
                return;
            }
            if event.keystroke.key == "tab" {
                self.form.field_anchor = None;
                self.form.field_cursor = usize::MAX;
                self.input.cursor_visible = true;
                self.form.active_field =
                    match (self.form.selected_driver_kind, self.form.active_field) {
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
                        (_, Some(FormField::Password)) if self.form.form_tls_enabled => {
                            Some(FormField::TlsCa)
                        }
                        (_, Some(FormField::Password | FormField::TlsCa)) => Some(FormField::Name),
                    };
                cx.notify();
                return;
            }
            if let Some(field) = self.form.active_field {
                let mut caret = self.form.field_cursor;
                let mut anchor = self.form.field_anchor;
                let mut history = self.form.form_history.remove(&field).unwrap_or_default();
                let input = LineInput {
                    text: self.field_value_mut(field),
                    caret: &mut caret,
                    anchor: &mut anchor,
                    digits_only: field == FormField::Port,
                    history: &mut history,
                };
                let consumed = handle_line_key(input, &event.keystroke, cx);
                self.form.form_history.insert(field, history);
                if consumed {
                    self.form.field_cursor = caret;
                    self.form.field_anchor = anchor;
                    self.input.cursor_visible = true;
                    cx.notify();
                }
            }
        } else if self.tabs.get(self.active_tab).is_some_and(|t| {
            t.active_subtab == TableSubTab::Data
                && t.grid.editing_cell.is_some()
                && (!t.is_script() || !t.editor_focused)
        }) {
            self.handle_cell_edit_key(event, cx);
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("s")
        {
            self.save_pending_edits(cx);
            cx.notify();
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && (event.keystroke.key == "enter" || event.keystroke.key == "return")
        {
            if event.keystroke.modifiers.shift {
                self.run_script(cx);
            } else {
                self.run_active_query(cx);
            }
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.modifiers.shift
            && event.keystroke.key.eq_ignore_ascii_case("f")
        {
            self.format_script(cx);
        } else if self.handle_ddl_key(&event.keystroke, cx) {
            cx.notify();
        } else if !self.tabs.is_empty() {
            let quote = if matches!(
                self.tab_provider(self.active_tab),
                DbDriverKind::MySql | DbDriverKind::MariaDb
            ) {
                '`'
            } else {
                '"'
            };
            let tab = &mut self.tabs[self.active_tab];
            if self.input.is_fetch_size_focused {
                if event.keystroke.key == "escape" {
                    tab.fetch.size_text = tab.fetch.size.to_string();
                    self.input.is_fetch_size_focused = false;
                    self.input.fetch_size_anchor = None;
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    // Like DBeaver, a new fetch size re-reads the result from the first row
                    self.run_active_query(cx);
                } else {
                    let input = LineInput {
                        text: &mut tab.fetch.size_text,
                        caret: &mut self.input.fetch_size_cursor,
                        anchor: &mut self.input.fetch_size_anchor,
                        digits_only: true,
                        history: &mut tab.fetch.size_history,
                    };
                    if handle_line_key(input, &event.keystroke, cx) {
                        self.input.cursor_visible = true;
                        cx.notify();
                    }
                }
            } else if self.input.is_filter_focused {
                if self.handle_filter_suggest_key(&event.keystroke) {
                    cx.notify();
                } else if event.keystroke.key == "escape" {
                    self.input.is_filter_focused = false;
                    self.input.filter_anchor = None;
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    self.run_active_query(cx);
                } else if event.keystroke.modifiers.control && event.keystroke.key == "space" {
                    self.refresh_filter_suggest(true);
                    cx.notify();
                } else {
                    let tab = &mut self.tabs[self.active_tab];
                    let before = tab.filter_sql.clone();
                    let input = LineInput {
                        text: &mut tab.filter_sql,
                        caret: &mut self.input.filter_cursor,
                        anchor: &mut self.input.filter_anchor,
                        digits_only: false,
                        history: &mut tab.filter_history,
                    };
                    if handle_line_key(input, &event.keystroke, cx) {
                        // Only typing opens the list; moving the caret just closes it
                        if self.tabs[self.active_tab].filter_sql != before {
                            self.refresh_filter_suggest(false);
                        } else {
                            self.input.filter_suggest = None;
                        }
                        self.input.cursor_visible = true;
                        cx.notify();
                    }
                }
            } else if tab.is_script() && tab.editor_focused {
                if tab.file_busy {
                    return;
                }
                tab.editor.identifier_quote = quote;
                if !tab.editor.handle_input_key(&event.keystroke, cx) {
                    return;
                }
                self.input.cursor_visible = true;
                cx.notify();
            } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("c")
            {
                if let Some(text) = tab.grid.selection_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    cx.notify();
                }
            } else if let Some(undo_key) = UndoKey::from_keystroke(&event.keystroke) {
                let redo = undo_key == UndoKey::Redo;
                if !tab.is_script() && tab.active_subtab == TableSubTab::Data {
                    self.step_pending_edits(redo);
                }
                cx.notify();
            }
        }
    }

    fn handle_ddl_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return false;
        };
        if tab.active_subtab != TableSubTab::Properties
            || tab.active_property_section != PropertySection::Ddl
            || self.input.is_filter_focused
            || self.input.is_fetch_size_focused
        {
            return false;
        }
        let cmd = key.modifiers.platform || key.modifiers.control;
        if cmd && key.key.eq_ignore_ascii_case("a") {
            self.tabs[self.active_tab].ddl_selected_all = true;
        } else if cmd && key.key.eq_ignore_ascii_case("c") {
            if tab.ddl_selected_all {
                let schema = tab.schema_name.clone().unwrap_or_default();
                let table = tab.table_name.clone().unwrap_or_default();
                if let Some(model) = self
                    .tab_session(self.active_tab)
                    .properties
                    .get(&(schema, table))
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(model.ddl.clone()));
                }
            }
        } else if key.key == "escape" {
            self.tabs[self.active_tab].ddl_selected_all = false;
        }
        // Read-only pane: never route its keystrokes to the hidden data grid/editor.
        true
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
                    let col = if keystroke.modifiers.shift {
                        edit.col.checked_sub(1)?
                    } else {
                        edit.col + 1
                    };
                    (col < num_cols).then_some((edit.row, col))
                });
                self.commit_cell_edit();
                if let Some((row, col)) = next {
                    self.begin_cell_edit(row, col);
                }
            }
            "s" if cmd => self.save_pending_edits(cx),
            _ => {
                let Some(edit) = self
                    .tabs
                    .get_mut(self.active_tab)
                    .and_then(|t| t.grid.editing_cell.as_mut())
                else {
                    return;
                };
                let input = LineInput {
                    text: &mut edit.text,
                    caret: &mut edit.caret,
                    anchor: &mut edit.anchor,
                    digits_only: false,
                    history: &mut self.input.cell_edit_history,
                };
                if !handle_line_key(input, keystroke, cx) {
                    return;
                }
                self.input.cursor_visible = true;
            }
        }
        cx.notify();
    }
}
