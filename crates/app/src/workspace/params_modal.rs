//! Parameter binding modal dialog.
//!
//! Intercepts queries containing unbound named (`:param`, `@param`) or positional (`$1`, `?1`)
//! parameters, prompts the user with typed input fields, provides live SQL preview with
//! safe escaping, remembers entered values for the tab session, and executes upon confirmation.

use super::script_tools::tool;
use super::*;
use sql_intel::{
    ParamBinding, ParamType, distinct_parameter_names, extract_parameters, substitute_parameters,
};

#[derive(Debug, Clone)]
pub struct ParamInputState {
    pub name: String,
    pub param_type: ParamType,
    pub value_text: String,
    pub value_caret: usize,
    pub value_anchor: Option<usize>,
    pub value_history: LineHistory,
}

pub struct ParameterBindingModal {
    pub tab_id: u64,
    pub original_sql: String,
    pub is_explain: bool,
    pub explain_analyze: bool,
    pub active_field: usize,
    pub bindings: Vec<ParamInputState>,
    pub preview_sql: String,
    pub error_message: Option<String>,
}

impl ParameterBindingModal {
    pub fn new(
        tab_id: u64,
        sql: String,
        is_explain: bool,
        explain_analyze: bool,
        saved_params: &std::collections::HashMap<String, (ParamType, String)>,
    ) -> Self {
        let params = extract_parameters(&sql);
        let names = distinct_parameter_names(&params);

        let mut bindings = Vec::new();
        for name in names {
            let (pt, val) = if let Some((saved_type, saved_val)) = saved_params.get(&name) {
                (*saved_type, saved_val.clone())
            } else {
                (ParamType::String, String::new())
            };

            bindings.push(ParamInputState {
                name,
                param_type: pt,
                value_text: val.clone(),
                value_caret: val.len(),
                value_anchor: None,
                value_history: LineHistory::new(),
            });
        }

        let mut modal = Self {
            tab_id,
            original_sql: sql,
            is_explain,
            explain_analyze,
            active_field: 0,
            bindings,
            preview_sql: String::new(),
            error_message: None,
        };

        modal.update_preview();
        modal
    }

    pub fn update_preview(&mut self) {
        let param_bindings: Vec<ParamBinding> = self
            .bindings
            .iter()
            .map(|b| ParamBinding {
                name: b.name.clone(),
                param_type: b.param_type,
                value: b.value_text.clone(),
            })
            .collect();

        match substitute_parameters(&self.original_sql, &param_bindings) {
            Ok(sql) => {
                self.preview_sql = sql;
                self.error_message = None;
            }
            Err(err) => {
                self.preview_sql.clear();
                self.error_message = Some(err);
            }
        }
    }

    pub fn next_field(&mut self) {
        if !self.bindings.is_empty() {
            self.active_field = (self.active_field + 1) % self.bindings.len();
        }
    }

    pub fn prev_field(&mut self) {
        if !self.bindings.is_empty() {
            if self.active_field == 0 {
                self.active_field = self.bindings.len() - 1;
            } else {
                self.active_field -= 1;
            }
        }
    }
}

impl Workspace {
    pub(super) fn open_parameter_binding_modal(
        &mut self,
        tab_id: u64,
        sql: String,
        is_explain: bool,
        explain_analyze: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let modal = ParameterBindingModal::new(
            tab_id,
            sql,
            is_explain,
            explain_analyze,
            &self.tabs[tab_idx].saved_params,
        );
        self.params_modal = Some(modal);
        cx.notify();
    }

    pub(super) fn execute_parameter_modal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.params_modal else {
            return;
        };
        modal.update_preview();
        if let Some(err) = &modal.error_message {
            let msg = err.clone();
            if let Some(tab_idx) = self.tabs.iter().position(|t| t.id == modal.tab_id) {
                self.tabs[tab_idx].last_error = Some(msg);
            }
            cx.notify();
            return;
        }

        let tab_id = modal.tab_id;
        let is_explain = modal.is_explain;
        let explain_analyze = modal.explain_analyze;
        let substituted_sql = modal.preview_sql.clone();

        // Save parameter bindings in tab session
        if let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            for b in &modal.bindings {
                self.tabs[tab_idx]
                    .saved_params
                    .insert(b.name.clone(), (b.param_type, b.value_text.clone()));
            }
        }

        self.params_modal = None;

        let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };

        if is_explain {
            let kind = self.tab_provider(tab_idx);
            match db_wire::query_plan::explain_sql(kind, &substituted_sql, explain_analyze) {
                Ok(explain_stmt) => {
                    let Some(driver) = self.tab_session(tab_idx).driver.clone() else {
                        let owner = self.tabs[tab_idx].connection_id.clone();
                        self.edit_saved_connection(owner.as_deref());
                        cx.notify();
                        return;
                    };
                    self.start_pinned_plan(tab_idx, driver, explain_stmt, cx);
                }
                Err(error) => self.tabs[tab_idx].last_error = Some(error.to_string()),
            }
        } else if self.tabs[tab_idx].is_script()
            && let Some(driver) = self.tab_session(tab_idx).driver.clone()
            && driver.supports_script_session()
        {
            match sql_intel::split_session_script(&substituted_sql) {
                Ok(statements) if !statements.is_empty() => {
                    self.start_pinned_script(tab_idx, driver, statements, cx)
                }
                Ok(_) => {}
                Err(error) => {
                    self.tabs[tab_idx].last_error = Some(error);
                    cx.notify();
                }
            }
        } else {
            let tab = &mut self.tabs[tab_idx];
            tab.pending_statements.clear();
            tab.script_status = None;
            tab.new_script_result(substituted_sql);

            let (paged_sql, paged) = self.build_fetch_query(tab_idx, 0);
            self.start_query(tab_idx, paged_sql, paged, false, cx);
        }

        cx.notify();
    }

    pub(super) fn handle_params_modal_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.params_modal else {
            return;
        };

        if key.key == "escape" {
            self.params_modal = None;
            self.focus_script_editor(true);
            cx.notify();
            return;
        }

        if key.key == "enter" || key.key == "return" {
            self.execute_parameter_modal(cx);
            return;
        }

        if key.key == "tab" {
            if key.modifiers.shift {
                modal.prev_field();
            } else {
                modal.next_field();
            }
            cx.notify();
            return;
        }

        let field_idx = modal.active_field;
        if let Some(field) = modal.bindings.get_mut(field_idx) {
            let input = LineInput {
                text: &mut field.value_text,
                caret: &mut field.value_caret,
                anchor: &mut field.value_anchor,
                digits_only: false,
                history: &mut field.value_history,
            };
            if handle_line_key(input, key, cx) {
                modal.update_preview();
                cx.notify();
            }
        }
    }

    pub(super) fn render_params_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let modal = self.params_modal.as_ref().unwrap();
        let theme = &self.theme;

        let mut bindings_list = div()
            .id("param-bindings-list")
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(260.0))
            .overflow_y_scroll()
            .p_1();

        for (idx, b) in modal.bindings.iter().enumerate() {
            let is_active = idx == modal.active_field;
            let weak = cx.entity().downgrade();
            let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
                if let Some(app) = weak.upgrade() {
                    app.update(cx, |this, cx| {
                        if let Some(m) = &mut this.params_modal {
                            m.active_field = idx;
                            if let Some(field) = m.bindings.get_mut(idx) {
                                apply_pick(
                                    &mut field.value_caret,
                                    &mut field.value_anchor,
                                    index,
                                    kind,
                                    true,
                                );
                                cx.notify();
                            }
                        }
                    });
                }
            });

            let current_type = b.param_type;
            let type_pills = div().flex().items_center().gap_1().children(
                ParamType::all().iter().enumerate().map(|(p_idx, &pt)| {
                    let is_sel = pt == current_type;
                    div()
                        .id(ElementId::NamedInteger(
                            "param-type-pill".into(),
                            (idx * 100 + p_idx) as u64,
                        ))
                        .px_1p5()
                        .py_0p5()
                        .rounded_xs()
                        .cursor_pointer()
                        .text_size(px(10.5))
                        .bg(if is_sel {
                            theme.bg_surface_selected
                        } else {
                            theme.bg_app
                        })
                        .text_color(if is_sel {
                            rgb(0xffffff)
                        } else {
                            theme.text_secondary
                        })
                        .hover(move |s| {
                            if !is_sel {
                                s.bg(theme.bg_surface_hover)
                            } else {
                                s
                            }
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = &mut this.params_modal
                                && let Some(field) = m.bindings.get_mut(idx)
                            {
                                field.param_type = pt;
                                m.update_preview();
                                cx.notify();
                            }
                        }))
                        .child(pt.label())
                }),
            );

            bindings_list = bindings_list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_xs()
                    .border_1()
                    .border_color(if is_active {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .bg(if is_active {
                        theme.bg_surface_hover
                    } else {
                        theme.bg_surface
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(12.5))
                                    .text_color(theme.text_warning)
                                    .child(b.name.clone()),
                            )
                            .child(type_pills),
                    )
                    .child(
                        ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(if is_active {
                                theme.border_focused
                            } else {
                                theme.border
                            })
                            .rounded_xs()
                            .bg(theme.bg_app)
                            .child(render_field_content(
                                &b.value_text,
                                if b.param_type == ParamType::Null {
                                    "NULL (value ignored)"
                                } else {
                                    "Enter parameter value..."
                                },
                                is_active,
                                if is_active { b.value_anchor } else { None },
                                b.value_caret,
                                true,
                                theme,
                                &pick,
                            )),
                    ),
            );
        }

        let preview_box = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_secondary)
                    .child("Substituted SQL Preview:"),
            )
            .child(
                div()
                    .id("param-sql-preview-text")
                    .p_2p5()
                    .rounded_xs()
                    .bg(theme.bg_app)
                    .border_1()
                    .border_color(if modal.error_message.is_some() {
                        rgb(0xf85149)
                    } else {
                        theme.border
                    })
                    .max_h(px(120.0))
                    .overflow_y_scroll()
                    .text_size(px(11.5))
                    .font_family(".AppleSystemUIFontMonospaced")
                    .text_color(if modal.error_message.is_some() {
                        rgb(0xf85149)
                    } else {
                        theme.text_primary
                    })
                    .child(
                        modal
                            .error_message
                            .as_deref()
                            .unwrap_or(&modal.preview_sql)
                            .to_string(),
                    ),
            );

        let dialog = div()
            .w(px(640.0))
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
                            .child("Bind Query Parameters"),
                    )
                    .child(
                        tool("✕ Close").on_click(cx.listener(|this, _, _, cx| {
                            this.params_modal = None;
                            this.focus_script_editor(true);
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .child(
                        "The SQL query contains parameter placeholders. Provide typed values to safely substitute before execution.",
                    ),
            )
            .child(bindings_list)
            .child(preview_box)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("Tab moves between fields • Cmd+Enter executes"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(tool("Cancel (Esc)").on_click(cx.listener(|this, _, _, cx| {
                                this.params_modal = None;
                                this.focus_script_editor(true);
                                cx.notify();
                            })))
                            .child(
                                tool(if modal.is_explain {
                                    "Execute Explain"
                                } else {
                                    "Execute Query"
                                })
                                .bg(if modal.error_message.is_some() {
                                    theme.bg_surface
                                } else {
                                    rgb(0x238636)
                                })
                                .text_color(if modal.error_message.is_some() {
                                    theme.text_muted
                                } else {
                                    rgb(0xffffff)
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.execute_parameter_modal(cx);
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
            .child(dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::ParameterBindingModal;
    use sql_intel::ParamType;
    use std::collections::HashMap;

    #[test]
    fn parameter_modal_init_and_preview() {
        let sql = "SELECT * FROM users WHERE id = :id AND active = :status;".to_string();
        let saved = HashMap::new();
        let mut modal = ParameterBindingModal::new(1, sql, false, false, &saved);

        assert_eq!(modal.bindings.len(), 2);
        assert_eq!(modal.bindings[0].name, ":id");
        assert_eq!(modal.bindings[1].name, ":status");

        // Set values and types
        modal.bindings[0].param_type = ParamType::Integer;
        modal.bindings[0].value_text = "42".into();

        modal.bindings[1].param_type = ParamType::Boolean;
        modal.bindings[1].value_text = "true".into();

        modal.update_preview();
        assert!(modal.error_message.is_none());
        assert_eq!(
            modal.preview_sql,
            "SELECT * FROM users WHERE id = 42 AND active = TRUE;"
        );

        // Field navigation
        assert_eq!(modal.active_field, 0);
        modal.next_field();
        assert_eq!(modal.active_field, 1);
        modal.next_field();
        assert_eq!(modal.active_field, 0);
        modal.prev_field();
        assert_eq!(modal.active_field, 1);
    }

    #[test]
    fn parameter_modal_catches_invalid_types() {
        let sql = "SELECT * FROM items WHERE count > :count;".to_string();
        let saved = HashMap::new();
        let mut modal = ParameterBindingModal::new(1, sql, false, false, &saved);

        modal.bindings[0].param_type = ParamType::Integer;
        modal.bindings[0].value_text = "not_a_number".into();

        modal.update_preview();
        assert!(modal.error_message.is_some());
        assert!(
            modal
                .error_message
                .as_ref()
                .unwrap()
                .contains("Invalid integer format")
        );
    }

    #[test]
    fn parameter_modal_uses_saved_values() {
        let sql = "SELECT * FROM users WHERE dept_id = :dept_id;".to_string();
        let mut saved = HashMap::new();
        saved.insert(
            ":dept_id".to_string(),
            (ParamType::Integer, "100".to_string()),
        );

        let modal = ParameterBindingModal::new(1, sql, false, false, &saved);
        assert_eq!(modal.bindings.len(), 1);
        assert_eq!(modal.bindings[0].param_type, ParamType::Integer);
        assert_eq!(modal.bindings[0].value_text, "100");
        assert_eq!(
            modal.preview_sql,
            "SELECT * FROM users WHERE dept_id = 100;"
        );
    }
}
