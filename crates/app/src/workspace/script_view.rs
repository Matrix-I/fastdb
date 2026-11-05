//! SQL script tab: a toolbar, the SQL editor, and the result of the last statement below it.

use gpui::prelude::FluentBuilder;
use std::rc::Rc;

use super::script_tools::rail_tool as tool;
use ui_editor::EditorPick;

use super::*;

impl Workspace {
    /// Script tab: Execute toolbar, editor on top, error banner, result grid and its footer
    pub(super) fn render_script_view(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let is_running_query = self.operations.is_running();
        let caret_visible = self.input.cursor_visible;
        let connection_name = self.tabs[tab_idx]
            .connection_id
            .as_deref()
            .and_then(|id| {
                self.connections
                    .saved_connections
                    .iter()
                    .find(|c| c.id == id)
            })
            .map(|c| c.name.clone())
            .unwrap_or_default();

        let timeout_label = match self.tabs[tab_idx].timeout_index {
            1 => "Timeout: 5s",
            2 => "Timeout: 30s",
            3 => "Timeout: 60s",
            4 => "Timeout: 300s",
            _ => "Timeout: off",
        };
        let supports_plan = self.tab_provider(tab_idx) == DbDriverKind::Postgres;
        let script_tab_id = self.tabs[tab_idx].id;
        let autocommit_label =
            if super::transaction_mode::client_managed(self.tab_provider(tab_idx)) {
                if self.tabs[tab_idx].manual_transactions {
                    "Autocommit: off…"
                } else {
                    "Autocommit: on…"
                }
            } else {
                "Autocommit…"
            };
        let supports_transactions = self
            .tab_session(tab_idx)
            .driver
            .as_ref()
            .is_some_and(|driver| driver.supports_script_session());
        let toolbar = div()
            .id("script-toolbar")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap_1()
            .w(px(36.0))
            .min_h_0()
            .py_1()
            .bg(theme.bg_surface)
            .border_r_1()
            .border_color(theme.border)
            .child(
                tool(if is_running_query {
                    "Cancel execution"
                } else {
                    "Execute"
                })
                .id("btn-script-run")
                .bg(if is_running_query {
                    rgb(0xda3633)
                } else {
                    rgb(0x238636)
                })
                .hover(move |s| {
                    s.bg(if is_running_query {
                        rgb(0xf85149)
                    } else {
                        rgb(0x2ea043)
                    })
                })
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(rgb(0xffffff))
                .on_click(cx.listener(|this, _, _, cx| {
                    // While a query runs, the button cancels it
                    if this.operations.is_running() {
                        this.cancel_running_query(cx);
                    } else {
                        this.run_active_query(cx);
                    }
                    cx.notify();
                })),
            )
            .child(tool("Run script").on_click(cx.listener(|this, _, _, cx| this.run_script(cx))))
            .child(
                tool("Execution log")
                    .id("toggle-execution-log")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tabs[tab_idx].show_execution_log =
                            !this.tabs[tab_idx].show_execution_log;
                        cx.notify();
                    })),
            )
            .child(
                tool(timeout_label)
                    .id("statement-timeout")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.execution_busy(cx) {
                            let tab = &mut this.tabs[tab_idx];
                            tab.timeout_index = (tab.timeout_index + 1)
                                % super::execution_log::TIMEOUT_SECONDS.len();
                            cx.notify();
                        }
                    })),
            )
            .when(supports_transactions && !is_running_query, |toolbar| {
                toolbar
                    .child(tool("Begin").on_click(
                        cx.listener(|this, _, _, cx| this.transaction_command("BEGIN", cx)),
                    ))
                    .child(tool("Commit").on_click(
                        cx.listener(|this, _, _, cx| this.transaction_command("COMMIT", cx)),
                    ))
                    .child(tool("Rollback").on_click(
                        cx.listener(|this, _, _, cx| this.transaction_command("ROLLBACK", cx)),
                    ))
                    .child(
                        tool("Isolation…").on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_session_isolation(script_tab_id, window, cx)
                        })),
                    )
            })
            .when(
                supports_transactions
                    && !is_running_query
                    && matches!(
                        self.tab_provider(tab_idx),
                        DbDriverKind::Postgres
                            | DbDriverKind::Sqlite
                            | DbDriverKind::MySql
                            | DbDriverKind::MariaDb
                            | DbDriverKind::SqlServer
                            | DbDriverKind::Oracle
                            | DbDriverKind::Db2
                    ),
                |toolbar| {
                    toolbar.child(tool(autocommit_label).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.choose_session_autocommit(script_tab_id, window, cx)
                        },
                    )))
                },
            )
            .when(supports_transactions && !is_running_query, |toolbar| {
                toolbar
                    .child(
                        tool("Database…").on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_sql_context(
                                script_tab_id,
                                db_wire::session_context::ContextKind::Database,
                                window,
                                cx,
                            )
                        })),
                    )
                    .child(
                        tool("Schema…").on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_sql_context(
                                script_tab_id,
                                db_wire::session_context::ContextKind::Schema,
                                window,
                                cx,
                            )
                        })),
                    )
            })
            .child(tool("Format").on_click(cx.listener(|this, _, _, cx| this.format_script(cx))))
            .child(
                tool("Snippets…")
                    .on_click(cx.listener(|this, _, _, cx| this.open_snippet_modal(cx))),
            )
            .child(
                tool("Import SQL copy…")
                    .on_click(cx.listener(|this, _, _, cx| this.import_script_file(cx))),
            )
            .child(
                tool("Save SQL copy…")
                    .on_click(cx.listener(|this, _, _, cx| this.save_script_copy(cx))),
            )
            .child(
                tool("Find / Replace")
                    .on_click(cx.listener(|this, _, _, cx| this.open_script_search(cx))),
            )
            .child(
                tool("Comment / Uncomment").on_click(cx.listener(|this, _, _, cx| {
                    this.edit_script_lines(super::script_lines::LineCommand::Comment, cx)
                })),
            )
            .child(tool("Indent").on_click(cx.listener(|this, _, _, cx| {
                this.edit_script_lines(super::script_lines::LineCommand::Indent, cx)
            })))
            .child(tool("Outdent").on_click(cx.listener(|this, _, _, cx| {
                this.edit_script_lines(super::script_lines::LineCommand::Outdent, cx)
            })))
            .child(tool("History").on_click(cx.listener(|this, _, _, cx| this.toggle_history(cx))))
            .when(supports_plan && !is_running_query, |toolbar| {
                toolbar
                    .child(tool("Explain").id("btn-explain").on_click(
                        cx.listener(|this, _, _, cx| this.explain_active_statement(false, cx)),
                    ))
                    .child(tool("Analyze (executes SQL)").id("btn-analyze").on_click(
                        cx.listener(|this, _, _, cx| this.explain_active_statement(true, cx)),
                    ))
            })
            .when(self.tabs[tab_idx].script_path.is_some(), |toolbar| {
                toolbar
                    .child(
                        tool("Rename").on_click(
                            cx.listener(|this, _, _, cx| this.open_script_action(false, cx)),
                        ),
                    )
                    .child(
                        tool("Delete").on_click(
                            cx.listener(|this, _, _, cx| this.open_script_action(true, cx)),
                        ),
                    )
            });

        let pick = self.script_editor_pick(tab_idx, cx);
        let tab = &mut self.tabs[tab_idx];
        let editor_focused = tab.editor_focused;
        let editor_scroll = tab.editor.scroll_handle.clone();
        let editor = div()
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex_none()
            .h(relative(0.45))
            .w_full()
            .border_b_1()
            .border_color(theme.border)
            .cursor_text()
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.focus_script_editor(true);
                    window.focus(&this.focus_handle);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(toolbar)
                    .child(
                        div().flex_1().min_w_0().min_h_0().overflow_hidden().child(
                            tab.editor
                                .render_editor(editor_focused, caret_visible, pick, window),
                        ),
                    )
                    .child(properties::grid::detail_scrollbar(
                        editor_scroll.clone(),
                        true,
                        theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(
                editor_scroll,
                false,
                theme,
            ));

        let log = tab
            .show_execution_log
            .then(|| tab.execution_log.messages().rev().collect::<Vec<_>>());
        let execution_status = tab.script_status.clone();
        let last_error = tab.last_error.clone();
        let no_result_set = tab.grid.pager.as_ref().is_some_and(|p| p.num_cols() == 0);
        let elapsed = tab.execution_time_ms;
        let command_count = tab.affected_rows;
        let result_failed = last_error.is_some();
        let result_count = tab.result_count();
        let selected_result = tab.active_result;
        let history = tab.show_history.then(|| tab.query_history.clone());
        let has_plan = tab.query_plan.is_some();
        let show_plan = tab.show_plan && has_plan;
        let tab_id = tab.id;

        let mut column = div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .px_2()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(connection_name),
            )
            .child(editor);

        if let Some(entries) = log {
            let mut list = div()
                .id("execution-log")
                .flex()
                .flex_col()
                .flex_none()
                .max_h(px(180.0))
                .overflow_y_scroll()
                .bg(theme.bg_surface)
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .child(div().px_2().py_1().child(
                    "Execution log · latest 100 statements · retained while this tab is open",
                ));
            if entries.is_empty() {
                list = list.child(div().p_2().child("No statements executed in this tab."));
            }
            for (index, message) in entries.into_iter().enumerate() {
                list = list.child(
                    div()
                        .id(("execution-entry", index))
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(message),
                );
            }
            column = column.child(list);
        }
        if let Some(message) = execution_status {
            column = column.child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(message),
            );
        }
        if let Some(history) = history {
            let mut list = div()
                .id("query-history")
                .max_h(px(140.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .bg(theme.bg_surface);
            if history.is_empty() {
                list = list.child(
                    div()
                        .p_2()
                        .text_size(px(12.0))
                        .child("No query history for this connection."),
                );
            }
            for (index, sql) in history.iter().enumerate() {
                let preview: String = sql.chars().take(180).collect();
                list = list.child(
                    div()
                        .id(("history", index))
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .child(preview)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.recall_history(index, cx)),
                        ),
                );
            }
            column = column.child(list);
        }
        if result_count > 0 {
            let mut strip = div()
                .id("script-results")
                .flex()
                .flex_none()
                .overflow_x_scroll()
                .bg(theme.bg_surface);
            for index in 0..result_count {
                strip = strip.child(
                    div()
                        .id(("result", index))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .bg(if index == selected_result {
                            theme.bg_surface_selected
                        } else {
                            theme.bg_surface
                        })
                        .child(format!("Result {}", index + 1))
                        .child(
                            div()
                                .id(("close-result", index))
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(16.0))
                                .h(px(16.0))
                                .rounded_xs()
                                .text_color(theme.text_muted)
                                .hover(|s| {
                                    s.text_color(theme.text_primary).bg(theme.bg_surface_hover)
                                })
                                .child("✕")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if !this.operations.is_running() {
                                        this.tabs[tab_idx].close_result(index);
                                        cx.notify();
                                    }
                                })),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.operations.is_running() {
                                this.tabs[tab_idx].select_result(index);
                                cx.notify();
                            }
                        })),
                );
            }
            column = column.child(strip);
        }
        if let Some(message) = last_error {
            column = column.child(self.render_error_banner(&message));
        }

        if has_plan {
            column = column.child(
                div()
                    .id("toggle-plan-json")
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(if show_plan {
                        "Query plan · Show raw JSON"
                    } else {
                        "Raw JSON · Show query plan"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) {
                            tab.show_plan = !tab.show_plan;
                        }
                        cx.notify();
                    })),
            );
        }
        let mut result = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            // Capture, so the grid's own handlers cannot hide the click from us
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.focus_script_editor(false);
                cx.notify();
            }));
        result = if show_plan {
            result.child(self.render_query_plan(tab_idx, cx))
        } else if no_result_set {
            let detail = elapsed.map(|ms| format!(" · {ms} ms")).unwrap_or_default();
            let message = if result_failed {
                "Statement failed".into()
            } else if elapsed.is_none() {
                "Waiting for statement completion…".into()
            } else {
                match command_count {
                    Some(Some(count)) => format!("{count} row(s) affected{detail}"),
                    Some(None) => {
                        format!("Statement completed; affected count unavailable{detail}")
                    }
                    None => format!("Statement completed{detail}"),
                }
            };
            result.child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(message),
            )
        } else {
            result.child(self.render_data_panel(tab_idx, window, cx))
        };

        column
            .child(result)
            .child(self.render_result_footer(tab_idx, cx))
    }

    /// Gives the keyboard to the active script tab's editor (`true`) or to its result grid
    pub(super) fn focus_script_editor(&mut self, focused: bool) {
        if focused {
            self.commit_cell_edit();
            self.blur_fetch_size();
            self.input.is_filter_focused = false;
            self.input.cursor_visible = true;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.editor_focused = focused;
            if !focused {
                tab.editor.hide_completions();
            }
        }
    }

    /// Click handler for the characters of a script tab's editor
    pub(super) fn script_editor_pick(&self, tab_idx: usize, cx: &mut Context<Self>) -> EditorPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |line, col, kind, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.focus_script_editor(true);
                if let Some(tab) = this.tabs.get_mut(tab_idx) {
                    tab.editor.pick(line, col, kind);
                }
                window.focus(&this.focus_handle);
                cx.notify();
            });
        })
    }

    /// Red strip with the message of the last failed run
    pub(super) fn render_error_banner(&self, message: &str) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_none()
            .items_start()
            .gap_2()
            .w_full()
            .max_h(px(96.0))
            .px_3()
            .py_2()
            .bg(rgba(0xf8514926))
            .border_b_1()
            .border_color(rgb(0xf85149))
            .text_size(px(12.0))
            .text_color(rgb(0xff7b72))
            .overflow_hidden()
            .child(div().flex_none().child("✕"))
            .child(div().flex_1().child(message.to_string()))
    }
}
