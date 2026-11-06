//! Disk-backed app-session log. Closing drops both the page and its pending read.
use super::*;
use crate::sql_log::Entry;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_RETAINED_ENTRIES: usize = 500;

pub(super) struct Panel {
    query: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    entries: Vec<Entry>,
    next_offset: Option<u64>,
    scroll: ScrollHandle,
    message: String,
    cancel: Arc<AtomicBool>,
    task: Option<Task<()>>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            query: String::new(),
            caret: 0,
            anchor: None,
            history: LineHistory::new(),
            entries: Vec::new(),
            next_offset: None,
            scroll: ScrollHandle::new(),
            message: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            task: None,
        }
    }
}
impl Panel {
    fn handle_escape(&mut self) -> bool {
        ui_editor::line_input::clear_selection(
            &mut self.caret,
            &mut self.anchor,
            self.query.chars().count(),
        )
    }
}
impl Drop for Panel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn single_line_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Workspace {
    pub(super) fn open_transaction_log(&mut self, cx: &mut Context<Self>) {
        self.menus.tx_menu = None;
        self.transaction_log = Some(Panel::default());
        self.load_transaction_log(0, false, cx);
    }
    fn load_transaction_log(&mut self, offset: u64, append: bool, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.transaction_log else {
            return;
        };
        panel.cancel.store(true, Ordering::Relaxed);
        panel.task = None;
        panel.cancel = Arc::new(AtomicBool::new(false));
        let cancel = panel.cancel.clone();
        let query = panel.query.clone();
        let log = self.sql_log.clone();
        if !append {
            panel.entries.clear();
            panel.next_offset = None;
            panel.scroll.set_offset(point(px(0.0), px(0.0)));
            panel.message = "Loading…".into();
        }
        panel.task = Some(cx.spawn(async move |this, cx| {
            let worker_cancel = cancel.clone();
            let page = cx
                .background_executor()
                .spawn(async move {
                    let page = log.page(offset, &query, &worker_cancel);
                    (page, log.error())
                })
                .await;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                let Some(panel) = &mut this.transaction_log else {
                    return;
                };
                if !Arc::ptr_eq(&panel.cancel, &cancel) {
                    return;
                }
                panel.task = None;
                match page {
                    (Ok(page), error) => {
                        panel.next_offset = page.next;
                        if append {
                            panel.entries.extend(page.entries);
                            if panel.entries.len() > MAX_RETAINED_ENTRIES {
                                let excess = panel.entries.len() - MAX_RETAINED_ENTRIES;
                                panel.entries.drain(0..excess);
                            }
                        } else {
                            panel.entries = page.entries;
                        }
                        panel.message = error.unwrap_or_else(|| {
                            if panel.entries.is_empty() {
                                "No matching SQL in this app session.".into()
                            } else {
                                format!(
                                    "{} records · UTC · SQL previews are limited to 2,000 characters",
                                    panel.entries.len()
                                )
                            }
                        });
                    }
                    (Err(error), _) => panel.message = error,
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn maybe_load_more_transaction_log(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &self.transaction_log else {
            return;
        };
        if panel.task.is_some() {
            return;
        }
        let Some(next) = panel.next_offset else {
            return;
        };
        let max_y = f32::from(panel.scroll.max_offset().height);
        let current_y = -f32::from(panel.scroll.offset().y);
        if max_y <= 0.0 || current_y + 48.0 >= max_y {
            self.load_transaction_log(next, true, cx);
        }
    }
    pub(super) fn transaction_log_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape"
            && self
                .transaction_log
                .as_mut()
                .is_some_and(Panel::handle_escape)
        {
            // Selection cleared; keep the dialog open.
        } else if key.key == "escape"
            || ((key.modifiers.platform || key.modifiers.control)
                && key.key.eq_ignore_ascii_case("w"))
        {
            self.transaction_log = None;
        } else if matches!(key.key.as_str(), "enter" | "return") {
            self.load_transaction_log(0, false, cx);
        } else if let Some(panel) = &mut self.transaction_log {
            let before = panel.query.clone();
            handle_line_key(
                LineInput {
                    text: &mut panel.query,
                    caret: &mut panel.caret,
                    anchor: &mut panel.anchor,
                    digits_only: false,
                    history: &mut panel.history,
                },
                key,
                cx,
            );
            if before != panel.query {
                self.load_transaction_log(0, false, cx);
                return;
            }
        }
        cx.notify();
    }
    pub(super) fn render_transaction_log(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let panel = self.transaction_log.as_ref().unwrap();
        let theme = &self.theme;
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(panel) = &mut this.transaction_log {
                    apply_pick(&mut panel.caret, &mut panel.anchor, index, kind, true);
                    this.input.cursor_visible = true;
                    window.focus(&this.focus_handle);
                    cx.notify();
                }
            });
        });
        let field = ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .bg(theme.bg_editor)
            .border_2()
            .border_color(theme.border_focused)
            .child(render_field_content(
                &panel.query,
                "Type query part to search in query history",
                true,
                panel.anchor,
                panel.caret,
                self.input.cursor_visible,
                theme,
                &pick,
            ));
        let cell = |text: &str, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .px_2()
                .py_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(single_line_text(text))
        };
        let header_cell = |text: &'static str, width: f32, has_divider: bool| {
            let base = div()
                .w(px(width))
                .flex_none()
                .px_2()
                .py_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(theme.text_secondary)
                .child(text);
            if has_divider {
                base.border_l_1().border_color(theme.border)
            } else {
                base
            }
        };
        let mut rows = div().flex().flex_col().flex_shrink_0();
        for (index, entry) in panel.entries.iter().enumerate() {
            rows = rows.child(
                div()
                    .id(("transaction-log-row", index))
                    .flex()
                    .flex_shrink_0()
                    .min_h(px(26.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .hover(|style| style.bg(theme.bg_surface_hover))
                    .child(cell(&entry.time, 150.))
                    .child(cell(&entry.kind, 145.))
                    .child(cell(&entry.sql, 520.))
                    .child(cell(&entry.duration_text(), 90.))
                    .child(cell(&entry.rows_text(), 70.))
                    .child(cell(&entry.result, 190.)),
            );
        }
        let header = div()
            .flex()
            .flex_shrink_0()
            .min_h(px(24.0))
            .bg(theme.bg_surface_selected)
            .border_b_1()
            .border_color(theme.border)
            .child(header_cell("Time (UTC)", 150., false))
            .child(header_cell("Type", 145., true))
            .child(header_cell("Text", 520., true))
            .child(header_cell("Duration", 90., true))
            .child(header_cell("Rows", 70., true))
            .child(header_cell("Result", 190., true));
        let scroll = panel.scroll.clone();
        let table = div()
            .flex()
            .flex_col()
            .h(px(360.))
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_editor)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .id("transaction-log-table")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .overflow_scroll()
                            .track_scroll(&scroll)
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                this.maybe_load_more_transaction_log(cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_shrink_0()
                                    .w_full()
                                    .min_w(px(1165.))
                                    .child(header)
                                    .child(rows),
                            ),
                    )
                    .child(properties::grid::detail_scrollbar(
                        scroll.clone(),
                        true,
                        theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(scroll, false, theme));
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus_handle);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .w_full()
                    .max_w(px(1200.))
                    .rounded_md()
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border)
                    .text_color(theme.text_primary)
                    .text_size(px(12.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .child("Transaction log — current app session"),
                    )
                    .child(field)
                    .child(table)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(theme.text_secondary)
                                    .child(panel.message.clone()),
                            )
                            .child(super::script_tools::tool("Close").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.transaction_log = None;
                                    cx.notify();
                                },
                            ))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Panel, single_line_text};
    use std::sync::atomic::Ordering;
    #[test]
    fn closing_panel_cancels_pending_read_and_releases_loaded_page() {
        let panel = Panel::default();
        let cancel = panel.cancel.clone();
        assert!(!cancel.load(Ordering::Relaxed));
        drop(panel);
        assert!(cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn multiline_cell_text_collapses_newlines_and_indentation_for_gpui_ellipsis() {
        assert_eq!(
            single_line_text("SELECT a,\n       b\n  FROM items"),
            "SELECT a, b FROM items"
        );
        let error = single_line_text(
            "Error: syntax error at or near \"FRO\"\r\n\nCaused by:\n    LINE 1: SELECT * FRO items;",
        );
        assert_eq!(
            error,
            "Error: syntax error at or near \"FRO\" Caused by: LINE 1: SELECT * FRO items;"
        );
        assert!(!error.contains(['\n', '\r']));
    }

    #[test]
    fn escape_only_intercepts_non_empty_text_selection() {
        let mut panel = Panel::default();
        panel.query = "SELECT 1".into();
        panel.caret = 3;
        panel.anchor = Some(3);
        assert!(!panel.handle_escape());
        assert_eq!(panel.anchor, None);

        panel.anchor = Some(0);
        panel.caret = 6;
        assert!(panel.handle_escape());
        assert_eq!(panel.anchor, None);
        assert!(!panel.handle_escape());
    }
}
