//! SQL script tab: a toolbar, the SQL editor, and the result of the last statement below it.

use gpui::prelude::FluentBuilder;
use std::rc::Rc;

use super::script_tools::tool;
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
        let toolbar = div()
            .id("script-toolbar")
            .overflow_x_scroll()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(32.0))
            .px_2()
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("btn-script-run")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .h(px(24.0))
                    .px_2()
                    .rounded_xs()
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
                    }))
                    .child(if is_running_query {
                        div().child("⏹").into_any_element()
                    } else {
                        img("icons/sql_exec.svg")
                            .w(px(13.0))
                            .h(px(13.0))
                            .into_any_element()
                    })
                    .child(if is_running_query {
                        "Cancel"
                    } else {
                        "Execute"
                    }),
            )
            .child(tool("Run script").on_click(cx.listener(|this, _, _, cx| this.run_script(cx))))
            .when(supports_plan && !is_running_query, |toolbar| {
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
            .child(
                tool("Rename")
                    .on_click(cx.listener(|this, _, _, cx| this.open_script_action(false, cx))),
            )
            .child(
                tool("Delete")
                    .on_click(cx.listener(|this, _, _, cx| this.open_script_action(true, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .child(img("icons/database.svg").w(px(13.0)).h(px(13.0)))
                    .child(connection_name),
            );

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

        let last_error = tab.last_error.clone();
        let no_result_set = tab.grid.pager.as_ref().is_some_and(|p| p.num_cols() == 0);
        let elapsed = tab.fetch.last.as_ref().map(|s| s.elapsed);
        let result_count = tab.result_count();
        let selected_result = tab.active_result;
        let history = tab.show_history.then(|| tab.query_history.clone());
        let has_plan = tab.query_plan.is_some();
        let show_plan = tab.show_plan && has_plan;
        let tab_id = tab.id;

        let current_rev = tab.editor.buffer.revision();
        if tab.diagnostics_revision != Some(current_rev) {
            let script_text = tab.editor.buffer.text();
            tab.diagnostics = if !script_text.trim().is_empty() {
                sql_intel::diagnose_sql(&script_text)
            } else {
                Vec::new()
            };
            tab.diagnostics_revision = Some(current_rev);
        }
        let first_diag = tab.diagnostics.first().cloned();

        let mut column = div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .child(toolbar)
            .child(editor);

        if let Some(diag) = first_diag {
            column = column.child(
                div()
                    .id("script-syntax-diagnostic")
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .bg(rgb(0x381a1a))
                    .border_b_1()
                    .border_color(rgb(0xf85149))
                    .text_size(px(11.5))
                    .text_color(rgb(0xffa198))
                    .child("⚠")
                    .child(format!(
                        "Line {}, Col {}: {}",
                        diag.line, diag.column, diag.message
                    )),
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
            let detail = elapsed
                .map(|e| format!(" ({:.3}s)", e.as_secs_f64()))
                .unwrap_or_default();
            result.child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(format!("Statement executed successfully.{detail}")),
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
