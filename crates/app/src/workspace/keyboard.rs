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
            && tab.is_script()
            && tab.editor.buffer.revision() != revision
        {
            self.schedule_autosave(id, tab.editor.buffer.revision(), cx);
        }
    }
    fn handle_workspace_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.value_editor.is_some() {
            self.value_editor_key(&event.keystroke, cx);
            return;
        }
        // Intercept Cmd+Q (macOS) / Ctrl+Q (Linux/Windows) unconditionally to close the app
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key.eq_ignore_ascii_case("q")
        {
            self.filter_builder = None;
            self.params_modal = None;
            self.snippet_modal = None;
            self.script_search = None;
            self.close_result_search();
            self.request_quit(cx);
            return;
        }

        // Toggle Theme shortcut: Cmd+Shift+T / Ctrl+Shift+T
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.modifiers.shift
            && event.keystroke.key.eq_ignore_ascii_case("t")
        {
            self.toggle_theme(cx);
            return;
        }

        // Editor Zoom shortcuts: Cmd+= / Cmd+- / Cmd+0
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && matches!(event.keystroke.key.as_str(), "=" | "+")
        {
            self.zoom_in_editor(cx);
            return;
        }
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key == "-"
        {
            self.zoom_out_editor(cx);
            return;
        }
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key == "0"
        {
            self.reset_editor_zoom(cx);
            return;
        }

        if self.navigator_filter_key(&event.keystroke, cx) {
            return;
        }

        if self.show_about_dialog {
            if self
                .column_pending
                .as_ref()
                .is_some_and(|p| !p.awaiting_save)
            {
                if event.keystroke.key == "escape" {
                    self.column_pending = None;
                    cx.notify();
                }
                return;
            }

            if event.keystroke.key == "escape"
                || ((event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("w"))
            {
                self.show_about_dialog = false;
                cx.notify();
            }
            return;
        }

        if self.object_browser.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            if event.keystroke.key == "escape"
                || ((event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("w"))
            {
                self.object_browser = None;
                cx.notify();
            }
            return;
        }
        if self.object_search.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            self.object_search_key(&event.keystroke, cx);
            return;
        }
        if self.schema_action_modal.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            self.schema_action_modal_key(&event.keystroke, cx);
            return;
        }
        if self.import_modal.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            if event.keystroke.key == "escape" {
                if let Some(m) = &mut self.import_modal
                    && let Some(ctrl) = &m.control
                {
                    ctrl.cancel();
                }
                self.import_modal = None;
                cx.notify();
            }
            return;
        }
        if self.params_modal.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            self.handle_params_modal_key(&event.keystroke, cx);
            return;
        }
        if self.snippet_modal.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            self.handle_snippet_modal_key(&event.keystroke, cx);
            return;
        }
        if self.script_search.is_some() {
            self.search_key(&event.keystroke, cx);
            return;
        }
        if self.result_search.is_some()
            && !self.updates.read(cx).is_open()
            && self.column_pending.is_none()
        {
            self.result_search_key(&event.keystroke, cx);
            return;
        }
        if self.filter_builder.is_some() {
            if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key == "w"
            {
                self.filter_builder = None;
                cx.notify();
                return;
            }
            if self.handle_filter_builder_key(event, cx) {
                return;
            }
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

        if event.keystroke.key == "escape"
            && self.menus.grid_context_menu.is_none()
            && self.menus.column_menu.is_none()
            && self.menus.nav_context_menu.is_none()
            && self.layout.focus_data
            && !self.form.show_connection_modal
            && self.script_action.is_none()
            && self.script_chooser.is_none()
            && !self.updates.read(cx).is_open()
        {
            self.layout.focus_data = false;
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

        if !self.form.show_connection_modal
            && self.menus.grid_context_menu.is_none()
            && self.menus.column_menu.is_none()
            && self.menus.nav_context_menu.is_none()
            && (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && let Some(factor) = super::diagram_model::zoom_shortcut(&event.keystroke.key)
            && let Some(tab) = self.tabs.get_mut(self.active_tab)
            && tab.active_subtab == TableSubTab::Diagram
        {
            tab.diagram.zoom_to(if factor == 0.0 {
                1.0
            } else {
                tab.diagram.zoom * factor
            });
            cx.stop_propagation();
            cx.notify();
            return;
        }

        if !self.form.show_connection_modal
            && self.column_pending.is_none()
            && !event.keystroke.modifiers.platform
            && !event.keystroke.modifiers.control
            && matches!(event.keystroke.key.as_str(), "enter" | "return")
            && let Some((owner, schema, table, column)) = self.column_pick.take()
        {
            self.open_column(owner, schema, table, column, cx);
            return;
        }
        if !self.form.show_connection_modal
            && self.column_pending.is_none()
            && self.menus.grid_context_menu.is_none()
            && self.menus.column_menu.is_none()
            && self.menus.nav_context_menu.is_none()
            && self.handle_column_key(event, cx)
        {
            return;
        }
        if self.column_pending.is_some() {
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

        if self.menus.tx_menu.is_some() {
            if event.keystroke.key == "escape" {
                self.menus.tx_menu = None;
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
            if self.form.choosing_provider {
                if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    self.form.show_configuration();
                    cx.notify();
                }
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
                self.form
                    .cycle_settings_field(event.keystroke.modifiers.shift);
                cx.notify();
                return;
            }
            if let Some(field) = self.form.active_field {
                if matches!(
                    field,
                    FormField::SshSecret | FormField::ClientArchivePassword
                ) && (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && (event.keystroke.key.eq_ignore_ascii_case("c")
                        || event.keystroke.key.eq_ignore_ascii_case("x"))
                {
                    return;
                }
                let mut caret = self.form.field_cursor;
                let mut anchor = self.form.field_anchor;
                let mut history = self.form.form_history.remove(&field).unwrap_or_default();
                let text = self.field_value_mut(field);
                let before = text.clone();
                let input = LineInput {
                    text,
                    caret: &mut caret,
                    anchor: &mut anchor,
                    digits_only: matches!(
                        field,
                        FormField::Port | FormField::SshPort | FormField::ConnectTimeout
                    ),
                    history: &mut history,
                };
                let consumed = handle_line_key(input, &event.keystroke, cx);
                self.form.form_history.insert(field, history);
                if consumed {
                    if *self.field_value_mut(field) != before {
                        self.form.test_result = None;
                        self.form.invalidate();
                    }
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
            } else if tab.record_view.is_some() {
                // The read-only view does not route shortcuts to the hidden grid.
                if event.keystroke.key == "escape" {
                    tab.leave_record_view();
                    cx.notify();
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("c")
                    && let Some(text) = tab.record_text()
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("v")
            {
                self.paste_grid_cells(cx);
            } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("c")
            {
                if let Some(text) = tab.grid.selection_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    cx.notify();
                }
            } else if let Some(undo_key) = UndoKey::from_keystroke(&event.keystroke) {
                let redo = undo_key == UndoKey::Redo;
                if tab.active_subtab == TableSubTab::Data {
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
                    t.grid
                        .adjacent_column(edit.col, !keystroke.modifiers.shift)
                        .map(|col| (edit.row, col))
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
