use super::*;

impl Workspace {
    /// Bottom status bar
    pub(super) fn render_status_bar(&self) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
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
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                if self.is_connected {
                                    let drv = match self.selected_driver_kind {
                                        DbDriverKind::Postgres => "postgres",
                                        DbDriverKind::MySql => "mysql",
                                        DbDriverKind::Sqlite => "sqlite",
                                    };
                                    dbeaver_driver_icon(drv, true)
                                } else {
                                    div().child(img("icons/disconnect.svg").w(px(14.0)).h(px(14.0)))
                                }
                            )
                            .child(if self.is_connected {
                                let name = self.active_saved_connection().map(|s| s.name.as_str()).unwrap_or(&self.form_database);
                                format!("Connected: {:?} ({})", self.selected_driver_kind, name)
                            } else {
                                "Disconnected".to_string()
                            })
                    )
                    .child({
                        let failed = ["❌", "✕", "⚠"].iter().any(|m| self.status_message.starts_with(m));
                        div()
                            .text_color(if failed { theme.text_error } else { theme.text_muted })
                            .child(self.status_message.clone())
                    })
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child("Zero-Copy SlottedPage (64KB)")
                    .child("120 FPS Metal/Vulkan")
            )
    }
}
