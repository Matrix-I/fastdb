use gpui::*;
use ui_core::theme::Theme;

pub struct Workspace {
    pub theme: Theme,
    pub active_tab: usize,
    pub status_message: String,
}

impl Workspace {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            theme: Theme::dark(),
            active_tab: 0,
            status_message: "Ready. Pure Rust FastDB Client.".to_string(),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            // Top Bar / Tab Bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.0))
                    .bg(theme.bg_surface)
                    .border_b_1()
                    .border_color(theme.border)
                    .px_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text_accent)
                                    .child("⚡ FastDB")
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("Query 1")
                            )
                    )
            )
            // Main Body: Sidebar + Main Dock
            .child(
                div()
                    .flex()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    // Sidebar
                    .child(
                        div()
                            .w(px(theme.sidebar_width))
                            .h_full()
                            .bg(theme.bg_sidebar)
                            .border_r_1()
                            .border_color(theme.border)
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_muted)
                                    .child("CONNECTIONS")
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .p_1()
                                    .rounded_md()
                                    .bg(theme.bg_surface_selected)
                                    .text_size(px(13.0))
                                    .child("🐘 PostgreSQL (localhost:5432)")
                            )
                    )
                    // Main Dock
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .bg(theme.bg_editor)
                            .p_4()
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(theme.text_secondary)
                                    .child("SELECT * FROM information_schema.tables LIMIT 100;")
                            )
                    )
            )
            // Bottom Status Bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(theme.status_bar_height))
                    .bg(theme.bg_surface)
                    .border_t_1()
                    .border_color(theme.border)
                    .px_3()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child("🟢 Connected: PostgreSQL 16.2")
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(self.status_message.clone())
                            .child("120 FPS Metal/Vulkan")
                    )
            )
    }
}
