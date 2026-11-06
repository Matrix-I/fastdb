use super::*;
use gpui::prelude::FluentBuilder;

const TITLE_BAR_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 32.0;
const TAB_BAR_HEIGHT: f32 = 30.0;
pub(super) const TOP_BAR_HEIGHT: f32 = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;

fn workspace_title(connection: &str, document: &str) -> String {
    match (connection.is_empty(), document.is_empty()) {
        (true, true) => "FastDB".into(),
        (true, false) => format!("FastDB — {document}"),
        (false, true) => format!("FastDB — {connection}"),
        (false, false) => format!("FastDB — <{connection}> {document}"),
    }
}

fn handle_title_bar_click(event: &ClickEvent, zoom: impl FnOnce()) {
    if let ClickEvent::Mouse(event) = event
        && event.down.button == MouseButton::Left
        && event.up.button == MouseButton::Left
        && event.up.click_count == 2
    {
        zoom();
    }
}

impl Workspace {
    /// Native title, workspace actions, and document tabs occupy separate rows.
    pub(super) fn render_top_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let active_tab_idx = self.active_tab;
        let selected = self
            .tabs
            .get(active_tab_idx)
            .map(|tab| (tab.id, active_tab_idx, self.tabs.len()));
        // Reveal after activation or closure, without undoing the user's manual scrolling
        // on unrelated workspace updates. GPUI resolves the child after layout.
        if selected != self.revealed_tab {
            if selected.is_some() {
                self.tab_scroll.scroll_to_item(active_tab_idx);
            }
            self.revealed_tab = selected;
        }
        let connection = self.header_connection_name();
        let title = workspace_title(
            &connection,
            self.tabs
                .get(active_tab_idx)
                .map_or("", |tab| tab.title.as_str()),
        );
        window.set_window_title(&title);
        let title_bar = div()
            .id("workspace-title-bar")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w_full()
            .h(px(TITLE_BAR_HEIGHT))
            .px(px(80.0))
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .window_control_area(WindowControlArea::Drag)
            .on_click(|event, window, _| handle_title_bar_click(event, || window.zoom_window()))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(title),
            );
        let toolbar = self.render_workspace_toolbar(cx);
        let tabs = div()
            .id("workspace-tab-row")
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(px(TAB_BAR_HEIGHT))
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .w(px(if self.layout.focus_data {
                        0.0
                    } else {
                        self.layout.navigator_width
                    }))
                    .h_full()
                    .px_2()
                    .when(self.layout.focus_data, |header| header.hidden())
                    .overflow_hidden()
                    .child(img("icons/database_navigator.svg").w(px(14.0)).h(px(14.0)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child("Connections"),
                    )
                    .child(
                        div()
                            .id("btn-open-connect-dialog")
                            .flex_none()
                            .px_1()
                            .rounded_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_new_connection_dialog();
                                cx.notify();
                            }))
                            .child(img("icons/connect.svg").w(px(14.0)).h(px(14.0))),
                    ),
            )
            // Open table and script tabs, the first one flush with the navigator edge
            .child({
                let mut tabs_bar = div()
                    .id("workspace-tabs")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .pr_2()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll);
                for (i, tab) in self.tabs.iter().enumerate() {
                    let is_active = i == active_tab_idx;
                    let bg = if is_active {
                        theme.bg_surface_selected
                    } else {
                        theme.bg_surface
                    };
                    let border_color = if is_active {
                        rgb(0x388bfd)
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0).into()
                    };
                    let tab_id = format!("tab-select-{}", i);
                    let close_id = format!("tab-close-{}", i);

                    tabs_bar = tabs_bar.child(
                        div()
                            .id(ElementId::Name(tab_id.into()))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1()
                            .rounded_t_md()
                            .bg(bg)
                            .border_t_2()
                            .border_color(border_color)
                            .text_size(px(12.0))
                            .text_color(if is_active {
                                theme.text_primary
                            } else {
                                theme.text_secondary
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.active_tab = i;
                                this.tab_scroll.scroll_to_item(i);
                                this.input.is_filter_focused = false;
                                this.activate_script_connection(cx);
                                cx.notify();
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.open_tab_context_menu(i, event.position);
                                    cx.notify();
                                }),
                            )
                            .child(
                                img(if tab.is_script() {
                                    "icons/sql_script.svg"
                                } else {
                                    "icons/table.svg"
                                })
                                .w(px(13.0))
                                .h(px(13.0)),
                            )
                            .child(tab.title.clone())
                            .child(
                                div()
                                    .id(ElementId::Name(close_id.into()))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(16.0))
                                    .h(px(16.0))
                                    .rounded_xs()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| {
                                        s.text_color(rgb(0xff7b72)).bg(theme.bg_surface_hover)
                                    })
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(i, cx);
                                        cx.notify();
                                    }))
                                    .child("✕"),
                            ),
                    );
                }
                tabs_bar.child(
                    div()
                        .id("btn-new-sql-script")
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_size(px(11.0))
                        .text_color(theme.text_muted)
                        .hover(|s| s.bg(theme.bg_surface_hover).text_color(theme.text_primary))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.open_sql_script(None, cx);
                            cx.notify();
                        }))
                        .child(img("icons/sql_script.svg").w(px(12.0)).h(px(12.0)))
                        .child("+ SQL"),
                )
            })
            // Update notification badge shown when a new release is available
            .when_some(self.updates.read(cx).pending(), |el, info| {
                let info_for_click = info.clone();
                let tag = info.tag_name.clone();
                el.child(
                    div()
                        .id("btn-topbar-update-badge")
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1p5()
                        .mr_3()
                        .px_2p5()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(0x1f6feb))
                        .text_color(rgb(0xffffff))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(0x388bfd)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.updates.update(cx, |updates, cx| {
                                updates.show_release(info_for_click.clone(), cx)
                            });
                            cx.notify();
                        }))
                        .child(div().child("⚡"))
                        .child(format!("Update ({})", tag)),
                )
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(title_bar)
            .child(toolbar)
            .child(tabs)
    }

    fn header_connection_name(&self) -> String {
        let connection_id = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| tab.connection_id.as_ref())
            .or(self.connections.active_connection_id.as_ref());
        connection_id
            .and_then(|id| {
                self.connections
                    .saved_connections
                    .iter()
                    .find(|connection| &connection.id == id)
            })
            .map_or_else(String::new, |connection| connection.name.clone())
    }

    fn render_workspace_toolbar(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = &self.theme;
        let index = self.active_tab;
        let tab = self.tabs.get(index);
        let supports_script_context = tab.is_some_and(|tab| tab.is_script())
            && self
                .tab_session(index)
                .driver
                .as_ref()
                .is_some_and(|driver| driver.supports_script_session());
        let tab_id = tab.map(|tab| tab.id);
        let connection = self.header_connection_name();
        let schema = tab
            .and_then(|tab| tab.schema_name.as_deref())
            .unwrap_or("Schema…")
            .to_owned();
        let mut toolbar = div()
            .id("workspace-toolbar")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(TOOLBAR_HEIGHT))
            .w_full()
            .px_2()
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .overflow_x_scroll()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                if this.menus.tx_menu.take().is_some() {
                    cx.notify();
                }
            }))
            .text_size(px(11.0))
            .text_color(theme.text_secondary)
            .child(
                div()
                    .id("toolbar-new-connection")
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_new_connection_dialog();
                        cx.notify();
                    }))
                    .child(img("icons/connect.svg").w(px(16.0)).h(px(16.0))),
            )
            .child(
                super::script_tools::tool("+ SQL").on_click(cx.listener(|this, _, _, cx| {
                    this.open_sql_script(None, cx);
                    cx.notify();
                })),
            );
        toolbar = toolbar
            .child(self.render_transaction_controls(index, cx))
            .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .child(img("icons/database.svg").w(px(14.0)).h(px(14.0)))
                    .child(if connection.is_empty() {
                        "No connection".into()
                    } else {
                        connection
                    }),
            );
        if supports_script_context && let Some(id) = tab_id {
            toolbar = toolbar
                .child(super::script_tools::tool("Database…").on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.choose_sql_context(
                            id,
                            db_wire::session_context::ContextKind::Database,
                            window,
                            cx,
                        )
                    },
                )))
                .child(
                    div()
                        .id("toolbar-schema")
                        .flex_none()
                        .px_2()
                        .py_1()
                        .rounded_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .child(schema)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_sql_context(
                                id,
                                db_wire::session_context::ContextKind::Schema,
                                window,
                                cx,
                            )
                        })),
                );
        }
        toolbar
    }
}

#[cfg(test)]
mod tests {
    use super::handle_title_bar_click;
    use gpui::{ClickEvent, MouseButton, MouseClickEvent, MouseDownEvent, MouseUpEvent};

    fn mouse_click(button: MouseButton, click_count: usize) -> ClickEvent {
        ClickEvent::Mouse(MouseClickEvent {
            down: MouseDownEvent {
                button,
                click_count,
                ..Default::default()
            },
            up: MouseUpEvent {
                button,
                click_count,
                ..Default::default()
            },
        })
    }

    #[test]
    fn native_title_tracks_active_document_without_empty_context_markers() {
        assert_eq!(super::workspace_title("", ""), "FastDB");
        assert_eq!(super::workspace_title("", "Script-1"), "FastDB — Script-1");
        assert_eq!(super::workspace_title("local", ""), "FastDB — local");
        assert_eq!(
            super::workspace_title("local", "Script-1"),
            "FastDB — <local> Script-1"
        );
        assert_eq!(
            super::workspace_title("開発", "customers"),
            "FastDB — <開発> customers"
        );
    }

    #[test]
    fn title_bar_double_click_toggles_zoom_and_restores() {
        let mut zoomed = false;
        let event = mouse_click(MouseButton::Left, 2);
        handle_title_bar_click(&event, || zoomed = !zoomed);
        assert!(zoomed);
        handle_title_bar_click(&event, || zoomed = !zoomed);
        assert!(!zoomed);
    }

    #[test]
    fn title_bar_ignores_other_clicks() {
        for event in [
            mouse_click(MouseButton::Left, 0),
            mouse_click(MouseButton::Left, 1),
            mouse_click(MouseButton::Left, 3),
            mouse_click(MouseButton::Right, 2),
            mouse_click(MouseButton::Middle, 2),
            ClickEvent::default(),
        ] {
            let mut zoomed = false;
            handle_title_bar_click(&event, || zoomed = true);
            assert!(!zoomed, "unexpected zoom for {event:?}");
        }
    }
}
