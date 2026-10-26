//! SQL script tab: a toolbar, the SQL editor, and the result of the last statement below it.

use std::rc::Rc;

use ui_editor::EditorPick;

use super::*;

impl Workspace {
    /// Script tab: Execute toolbar, editor on top, error banner, result grid and its footer
    pub(super) fn render_script_view(&mut self, tab_idx: usize, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let is_running_query = self.is_running_query;
        let caret_visible = self.cursor_visible;

        let toolbar = div()
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
                    .bg(if is_running_query { rgb(0xda3633) } else { rgb(0x238636) })
                    .hover(move |s| s.bg(if is_running_query { rgb(0xf85149) } else { rgb(0x2ea043) }))
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .text_color(rgb(0xffffff))
                    .on_click(cx.listener(|this, _, _, cx| {
                        // While a query runs, the button cancels it
                        if this.is_running_query {
                            this.cancel_running_query(cx);
                        } else {
                            this.run_active_query(cx);
                        }
                        cx.notify();
                    }))
                    .child(if is_running_query {
                        div().child("⏹").into_any_element()
                    } else {
                        img("icons/sql_exec.svg").w(px(13.0)).h(px(13.0)).into_any_element()
                    })
                    .child(if is_running_query { "Cancel" } else { "Execute (⌘↵)" }),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child("Runs the statement under the caret"),
            );

        let pick = self.script_editor_pick(tab_idx, cx);
        let tab = &mut self.tabs[tab_idx];
        let editor_focused = tab.editor_focused;
        let editor = div()
            .flex_none()
            .h(relative(0.45))
            .w_full()
            .border_b_1()
            .border_color(theme.border)
            .cursor_text()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus_script_editor(true);
                window.focus(&this.focus_handle);
                cx.notify();
            }))
            .child(tab.editor.render_editor(editor_focused, caret_visible, pick));

        let last_error = tab.last_error.clone();
        let no_result_set = tab.grid.pager.as_ref().is_some_and(|p| p.num_cols() == 0);
        let elapsed = tab.fetch.last.as_ref().map(|s| s.elapsed);

        let mut column = div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .child(toolbar)
            .child(editor);
        if let Some(message) = last_error {
            column = column.child(self.render_error_banner(&message));
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
        result = if no_result_set {
            let detail = elapsed.map(|e| format!(" ({:.3}s)", e.as_secs_f64())).unwrap_or_default();
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
    fn focus_script_editor(&mut self, focused: bool) {
        if focused {
            self.commit_cell_edit();
            self.blur_fetch_size();
            self.is_filter_focused = false;
            self.cursor_visible = true;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.editor_focused = focused;
            if !focused {
                tab.editor.hide_completions();
            }
        }
    }

    /// Click handler for the characters of a script tab's editor
    fn script_editor_pick(&self, tab_idx: usize, cx: &mut Context<Self>) -> EditorPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |line, col, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.focus_script_editor(true);
                if let Some(tab) = this.tabs.get_mut(tab_idx) {
                    tab.editor.buffer.set_cursor_line_col(line, col);
                    tab.editor.hide_completions();
                }
                window.focus(&this.focus_handle);
                cx.notify();
            });
        })
    }

    /// Red strip with the message of the last failed run
    pub(super) fn render_error_banner(&self, message: &str) -> impl IntoElement {
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
