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
            .w_full()
            .h(px(TOP_BAR_HEIGHT))
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border)
            // Left section spans the navigator so the tab strip starts where the center area does
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .w(px(self.navigator_width))
                    .h_full()
                    .pl(px(top_bar_left_pad))
                    .pr_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(14.0))
                            .text_color(theme.text_accent)
                            .child("⚡ FastDB")
                    )
            )
            // Tabs Bar: only open tables, the first one flush with the navigator edge
            .child({
                let mut tabs_bar = div().flex().flex_1().min_w_0().items_center().gap_1().pr_2().overflow_hidden();
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
                            .text_color(if is_active { theme.text_primary } else { theme.text_secondary })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.active_tab = i;
                                this.is_filter_focused = false;
                                this.activate_script_connection(cx);
                                cx.notify();
                            }))
                            .on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.open_tab_context_menu(i, event.position);
                                cx.notify();
                            }))
                            .child(img(if tab.is_script() { "icons/sql_script.svg" } else { "icons/table.svg" }).w(px(13.0)).h(px(13.0)))
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
                            this.open_sql_script(None, cx);
                            cx.notify();
                        }))
                        .child(img("icons/sql_script.svg").w(px(12.0)).h(px(12.0)))
                        .child("+ SQL")
                )
            })
    }
}
