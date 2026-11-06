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

        let supports_plan = self.tab_provider(tab_idx) == DbDriverKind::Postgres;
        let supports_transactions = self
            .tab_session(tab_idx)
            .driver
            .as_ref()
            .is_some_and(|driver| driver.supports_script_session());
        let rail_separator = || div().w(px(20.0)).h(px(1.0)).my(px(2.0)).bg(theme.border);
        let is_manual = self.tabs[tab_idx].manual_transactions;
        let toolbar =
            div()
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
                .cursor_default()
                .child(
                    tool(if is_running_query {
                        "Cancel execution"
                    } else {
                        "Execute"
                    })
                    .id("btn-script-run")
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
                .when(supports_plan && !is_running_query, |toolbar| {
                    toolbar.child(tool("Explain").id("btn-explain").on_click(
                        cx.listener(|this, _, _, cx| this.explain_active_statement(false, cx)),
                    ))
                })
                .when(supports_transactions && !is_running_query, |toolbar| {
                    toolbar
                        .child(rail_separator())
                        .when(is_manual, |toolbar| {
                            toolbar
                                .child(tool("Commit").id("btn-commit").on_click(cx.listener(
                                    |this, _, _, cx| this.transaction_command("COMMIT", cx),
                                )))
                                .child(tool("Rollback").id("btn-rollback").on_click(cx.listener(
                                    |this, _, _, cx| this.transaction_command("ROLLBACK", cx),
                                )))
                        })
                        .child(
                            tool(if is_manual {
                                "Transaction: Manual"
                            } else {
                                "Transaction: Auto"
                            })
                            .id("btn-transaction-menu")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    this.open_transaction_menu(tab_idx, event.position);
                                    cx.notify();
                                }),
                            ),
                        )
                });

        let pick = self.script_editor_pick(tab_idx, cx);
        let tab = &mut self.tabs[tab_idx];
        let show_results = tab.shows_script_results();
        let editor_fraction = tab.result_split.editor_fraction;
        let split_bounds = tab.result_split.bounds.clone();
        let split_dragging = tab.result_split.is_dragging();
        let editor_focused = tab.editor_focused;
        let editor_scroll = tab.editor.scroll_handle.clone();
        let editor = div()
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(show_results, |editor| {
                editor.flex_none().h(relative(editor_fraction))
            })
            .when(!show_results, |editor| editor.flex_1())
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

        let mut shell = div().flex().flex_col().size_full().overflow_hidden();

        if !connection_name.is_empty() {
            shell = shell.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1p5()
                    .h(px(22.0))
                    .px_2()
                    .bg(theme.bg_surface)
                    .border_b_1()
                    .border_color(theme.border)
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(img("icons/database.svg").w(px(12.0)).h(px(12.0)))
                    .child(connection_name),
            );
        }
        let mut body = div()
            .id("script-split")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .track_scroll(&split_bounds)
            .when(split_dragging, |body| body.cursor_row_resize())
            .child(editor);
        if split_dragging {
            body = body.child(self.render_script_split_drag(tab_id, cx));
        }
        if show_results {
            body = body.child(
                div()
                    .id("script-result-splitter")
                    .flex_none()
                    .h(px(super::script_split::SPLITTER_HEIGHT))
                    .w_full()
                    .cursor_row_resize()
                    .bg(if split_dragging {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .hover(|style| style.bg(theme.border_focused))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                                let height =
                                    f32::from(tab.result_split.bounds.bounds().size.height);
                                tab.result_split
                                    .begin_drag(f32::from(event.position.y), height);
                                this.input.grid_drag = None;
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    ),
            );
        }
        let mut column = div()
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .when(show_results, |column| column.flex_1())
            .when(!show_results, |column| column.flex_none());

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
        if !show_results {
            return shell.child(body.child(column));
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

        shell.child(
            body.child(
                column
                    .child(result)
                    .child(self.render_result_footer(tab_idx, cx)),
            ),
        )
    }

    fn render_script_split_drag(
        &self,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let on_move = cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id)
                && tab
                    .result_split
                    .handle_mouse_move(event, DispatchPhase::Capture)
            {
                cx.stop_propagation();
                cx.notify();
            }
        });
        let on_up = cx.listener(move |this, event: &MouseUpEvent, _, cx| {
            if event.button == MouseButton::Left
                && let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id)
                && tab.result_split.end_drag()
            {
                cx.stop_propagation();
                cx.notify();
            }
        });
        // Window capture listeners keep resizing over editor text and outside the panel.
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        on_move(event, window, cx);
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        on_up(event, window, cx);
                    }
                });
            },
        )
        .absolute()
        .size_full()
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
                if this
                    .tabs
                    .get(tab_idx)
                    .is_some_and(|tab| tab.result_split.is_dragging())
                {
                    return;
                }
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
