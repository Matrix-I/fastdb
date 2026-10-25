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
                self.field_selected_all = false;
            } else if !self.tabs.is_empty() {
                self.close_tab(self.active_tab);
            }
            cx.notify();
            return;
        }

        if self.show_connection_modal {
            if event.keystroke.key == "escape" {
                self.show_connection_modal = false;
                self.field_selected_all = false;
                cx.notify();
                return;
            }
            if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                self.connect_to_database(cx);
                return;
            }
            if event.keystroke.key == "tab" {
                self.field_selected_all = false;
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
            // Select All (Cmd+A / Ctrl+A)
            if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("a")
            {
                if self.active_field.is_some() {
                    self.field_selected_all = true;
                    self.cursor_visible = true;
                    cx.notify();
                    return;
                }
            }
            // Copy (Cmd+C / Ctrl+C)
            if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("c")
            {
                if let Some(field) = self.active_field {
                    let text = match field {
                        FormField::Name => &self.form_name,
                        FormField::Host => &self.form_host,
                        FormField::Port => &self.form_port,
                        FormField::Database => &self.form_database,
                        FormField::User => &self.form_user,
                        FormField::Password => &self.form_password,
                    };
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    return;
                }
            }
            // Cut (Cmd+X / Ctrl+X)
            if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("x")
            {
                if let Some(field) = self.active_field {
                    let target = match field {
                        FormField::Name => &mut self.form_name,
                        FormField::Host => &mut self.form_host,
                        FormField::Port => &mut self.form_port,
                        FormField::Database => &mut self.form_database,
                        FormField::User => &mut self.form_user,
                        FormField::Password => &mut self.form_password,
                    };
                    cx.write_to_clipboard(ClipboardItem::new_string(target.clone()));
                    target.clear();
                    self.field_selected_all = false;
                    self.cursor_visible = true;
                    cx.notify();
                    return;
                }
            }
            // Clipboard paste (Cmd+V on macOS or Ctrl+V on Linux/Windows)
            if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                && event.keystroke.key.eq_ignore_ascii_case("v")
            {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(pasted) = item.text() {
                        let clean = pasted.trim_matches(|c| c == '\r' || c == '\n');
                        if let Some(field) = self.active_field {
                            let target = match field {
                                FormField::Name => &mut self.form_name,
                                FormField::Host => &mut self.form_host,
                                FormField::Port => &mut self.form_port,
                                FormField::Database => &mut self.form_database,
                                FormField::User => &mut self.form_user,
                                FormField::Password => &mut self.form_password,
                            };
                            if self.field_selected_all {
                                target.clear();
                                self.field_selected_all = false;
                            }
                            if field == FormField::Port {
                                let digits: String = clean.chars().filter(|c| c.is_ascii_digit()).collect();
                                target.push_str(&digits);
                            } else {
                                target.push_str(clean);
                            }
                            self.cursor_visible = true;
                            cx.notify();
                        }
                    }
                }
                return;
            }
            // Cancel selection on Left / Right / Up / Down arrow navigation
            if event.keystroke.key == "left" || event.keystroke.key == "right" || event.keystroke.key == "up" || event.keystroke.key == "down" {
                if self.field_selected_all {
                    self.field_selected_all = false;
                    self.cursor_visible = true;
                    cx.notify();
                    return;
                }
            }
            // Cmd+Backspace = clear entire field
            if event.keystroke.modifiers.platform && event.keystroke.key == "backspace" {
                if let Some(field) = self.active_field {
                    let target = match field {
                        FormField::Name => &mut self.form_name,
                        FormField::Host => &mut self.form_host,
                        FormField::Port => &mut self.form_port,
                        FormField::Database => &mut self.form_database,
                        FormField::User => &mut self.form_user,
                        FormField::Password => &mut self.form_password,
                    };
                    target.clear();
                    self.field_selected_all = false;
                    self.cursor_visible = true;
                    cx.notify();
                }
                return;
            }
            // Backspace or Delete
            if event.keystroke.key == "backspace" || event.keystroke.key == "delete" {
                if let Some(field) = self.active_field {
                    let target = match field {
                        FormField::Name => &mut self.form_name,
                        FormField::Host => &mut self.form_host,
                        FormField::Port => &mut self.form_port,
                        FormField::Database => &mut self.form_database,
                        FormField::User => &mut self.form_user,
                        FormField::Password => &mut self.form_password,
                    };
                    if self.field_selected_all {
                        target.clear();
                        self.field_selected_all = false;
                    } else {
                        target.pop();
                    }
                    self.cursor_visible = true;
                    cx.notify();
                }
                return;
            }
            // Printable character insertion
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
                    if let Some(field) = self.active_field {
                        let target = match field {
                            FormField::Name => &mut self.form_name,
                            FormField::Host => &mut self.form_host,
                            FormField::Port => &mut self.form_port,
                            FormField::Database => &mut self.form_database,
                            FormField::User => &mut self.form_user,
                            FormField::Password => &mut self.form_password,
                        };
                        if self.field_selected_all {
                            target.clear();
                            self.field_selected_all = false;
                        }
                        if field == FormField::Port {
                            if char_str.chars().all(|c| c.is_ascii_digit()) {
                                target.push_str(char_str);
                            }
                        } else {
                            target.push_str(char_str);
                        }
                        self.cursor_visible = true;
                        cx.notify();
                    }
                }
            }
        } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && (event.keystroke.key == "enter" || event.keystroke.key == "return")
        {
            self.run_active_query(cx);
        } else if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            if self.is_filter_focused {
                if event.keystroke.key == "escape" {
                    self.is_filter_focused = false;
                    self.filter_selected_all = false;
                    cx.notify();
                } else if event.keystroke.key == "enter" || event.keystroke.key == "return" {
                    self.run_active_query(cx);
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("a")
                {
                    self.filter_selected_all = true;
                    self.cursor_visible = true;
                    cx.notify();
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("c")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(tab.filter_sql.clone()));
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("x")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(tab.filter_sql.clone()));
                    tab.filter_sql.clear();
                    self.filter_selected_all = false;
                    self.cursor_visible = true;
                    cx.notify();
                } else if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
                    && event.keystroke.key.eq_ignore_ascii_case("v")
                {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(pasted) = item.text() {
                            let clean = pasted.trim_matches(|c| c == '\r' || c == '\n');
                            if self.filter_selected_all {
                                tab.filter_sql.clear();
                                self.filter_selected_all = false;
                            }
                            tab.filter_sql.push_str(clean);
                            self.cursor_visible = true;
                            cx.notify();
                        }
                    }
                } else if event.keystroke.modifiers.platform && event.keystroke.key == "backspace" {
                    tab.filter_sql.clear();
                    self.filter_selected_all = false;
                    self.cursor_visible = true;
                    cx.notify();
                } else if event.keystroke.key == "backspace" || event.keystroke.key == "delete" {
                    if self.filter_selected_all {
                        tab.filter_sql.clear();
                        self.filter_selected_all = false;
                    } else {
                        tab.filter_sql.pop();
                    }
                    self.cursor_visible = true;
                    cx.notify();
                } else if event.keystroke.key == "left" || event.keystroke.key == "right" {
                    if self.filter_selected_all {
                        self.filter_selected_all = false;
                        self.cursor_visible = true;
                        cx.notify();
                    }
                } else if event.keystroke.key == "space" {
                    if self.filter_selected_all {
                        tab.filter_sql.clear();
                        self.filter_selected_all = false;
                    }
                    tab.filter_sql.push(' ');
                    self.cursor_visible = true;
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
                            if self.filter_selected_all {
                                tab.filter_sql.clear();
                                self.filter_selected_all = false;
                            }
                            tab.filter_sql.push_str(char_str);
                            self.cursor_visible = true;
                            cx.notify();
                        }
                    }
                }
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
}
