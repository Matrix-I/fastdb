use super::*;

pub(super) const TOP_BAR_HEIGHT: f32 = 38.0;

impl Workspace {
    /// Window title bar with connection actions and toolbar
    pub(super) fn render_top_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let active_tab_idx = self.active_tab;
        #[cfg(target_os = "macos")]
        let top_bar_left_pad = 80.0;
        #[cfg(not(target_os = "macos"))]
        let top_bar_left_pad = 14.0;
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(TOP_BAR_HEIGHT))
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            .pl(px(top_bar_left_pad))
            .pr_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(14.0))
                                    .text_color(theme.text_accent)
                                    .child("⚡ FastDB")
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_muted)
                                    .child("v0.1.0 Pure Rust")
                            )
                    )
                    // Tabs Bar
                    .child({
                        let mut tabs_bar = div().flex().items_center().gap_1();
                        for (i, tab) in self.tabs.iter().enumerate() {
                            let is_active = i == active_tab_idx;
                            let bg = if is_active { theme.bg_surface_selected } else { theme.bg_surface };
                            let border_color = if is_active { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() };
                            let tab_id = format!("tab-select-{}", i);
                            let close_id = format!("tab-close-{}", i);

                            tabs_bar = tabs_bar.child(
                                div()
                                    .id(ElementId::Name(tab_id.into()))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .rounded_t_md()
                                    .bg(bg)
                                    .border_t_2()
                                    .border_color(border_color)
                                    .text_size(px(12.0))
                                    .text_color(if is_active { theme.text_primary } else { theme.text_secondary })
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.active_tab = i;
                                        this.is_filter_focused = false;
                                        cx.notify();
                                    }))
                                    .on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.open_tab_context_menu(i, event.position);
                                        cx.notify();
                                    }))
                                    .child(img("icons/table.svg").w(px(13.0)).h(px(13.0)))
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
                                            .hover(|s| s.text_color(rgb(0xff7b72)).bg(theme.bg_surface_hover))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.close_tab(i);
                                                cx.notify();
                                            }))
                                            .child("✕")
                                    )
                            );
                        }
                        tabs_bar
                    })
            )
            // Top Right Connection Status Badge
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("btn-topbar-connection")
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(theme.bg_surface_hover)
                            .text_color(theme.text_secondary)
                            .text_size(px(11.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.is_connected && !this.saved_connections.is_empty() {
                                    this.connect_saved(cx);
                                } else if let Some(active_id) = this.active_connection_id.clone() {
                                    this.edit_saved_connection(Some(&active_id));
                                } else {
                                    this.open_new_connection_dialog();
                                }
                                cx.notify();
                            }))
                            .child(
                                if self.is_connected {
                                    let drv = self.active_saved_connection().map(|s| s.driver.as_str()).unwrap_or(match self.selected_driver_kind {
                                        DbDriverKind::Postgres => "Postgres",
                                        DbDriverKind::MySql => "MySql",
                                        DbDriverKind::Sqlite => "Sqlite",
                                    });
                                    dbeaver_driver_icon(drv, true).into_any_element()
                                } else {
                                    img("icons/disconnect.svg").w(px(13.0)).h(px(13.0)).into_any_element()
                                }
                            )
                            .child(if self.is_connected {
                                let name = self.active_saved_connection().map(|s| s.name.as_str()).unwrap_or(&self.form_database);
                                format!("{} ({})", name, self.form_user)
                            } else if let Some(first) = self.saved_connections.first() {
                                format!("Connect: {}", first.name)
                            } else {
                                "Connect DB".to_string()
                            })
                            .child(img("icons/configuration.svg").w(px(13.0)).h(px(13.0)))
                    )
            )
    }
}
