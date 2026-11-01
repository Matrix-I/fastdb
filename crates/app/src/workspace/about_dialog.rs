use super::*;

impl Workspace {
    pub(super) fn render_about_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        // Reuse the decoded image across renders, including the cursor blink timer.
        static ICON: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
        let icon = ICON.get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../../../../resources/icon.png").to_vec(),
            ))
        });

        div()
            .id("about-backdrop")
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_about_dialog = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("about-card")
                    .w(px(460.0))
                    .p_6()
                    .py_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .bg(rgb(0x242424))
                    .text_color(theme.text_primary)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_lg()
                    .shadow_lg()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(img(icon.clone()).size(px(104.0)))
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(28.0))
                            .font_weight(FontWeight::BOLD)
                            .child("FastDB"),
                    )
                    .child(div().text_size(px(20.0)).child(format!(
                        "Version {version} ({version})",
                        version = env!("CARGO_PKG_VERSION")
                    )))
                    .child(
                        div()
                            .id("about-close")
                            .mt_2()
                            .px_5()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme.bg_surface_hover)
                            .border_1()
                            .border_color(theme.border)
                            .text_size(px(13.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_selected))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.show_about_dialog = false;
                                cx.notify();
                            }))
                            .child("Close"),
                    ),
            )
    }
}
