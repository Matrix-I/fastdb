use super::*;
impl UpdateController {
    pub(super) fn render_update_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let auto_check = self.update_state.auto_check_enabled;
        let dialog_status = self
            .update_dialog
            .as_ref()
            .map(|d| d.status.clone())
            .unwrap_or(UpdateDialogStatus::Checking);
        let card_width = if matches!(dialog_status, UpdateDialogStatus::UpdateAvailable { .. }) {
            480.0
        } else {
            380.0
        };

        div()
            .id("update-dialog-backdrop")
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                // Clicking outside closes only when not downloading
                let is_downloading = matches!(
                    this.update_dialog.as_ref().map(|d| &d.status),
                    Some(UpdateDialogStatus::UpdateAvailable {
                        download: DownloadUiState::Downloading { .. },
                        ..
                    })
                );
                if !is_downloading {
                    this.close_update_dialog();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("update-dialog-card")
                    .w(px(card_width))
                    .max_w_full()
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border_focused)
                    .rounded_lg()
                    .shadow_lg()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .on_click(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    // 1. Dialog Header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(theme.border)
                            .pb_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text_primary)
                                    .child(
                                        div()
                                            .text_color(theme.text_accent)
                                            .child("⚡ FastDB Update"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("btn-close-update-dialog")
                                    .w(px(22.0))
                                    .h(px(22.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_xs()
                                    .text_size(px(13.0))
                                    .text_color(theme.text_muted)
                                    .hover(|s| {
                                        s.bg(theme.bg_surface_hover).text_color(theme.text_primary)
                                    })
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_update_dialog();
                                        cx.notify();
                                    }))
                                    .child("✕"),
                            ),
                    )
                    // 2. Dialog Content Area
                    .child(match dialog_status {
                        UpdateDialogStatus::Checking => self.render_checking_state(theme, cx),
                        UpdateDialogStatus::UpToDate {
                            ref current_version,
                        } => self.render_up_to_date_state(current_version, theme, cx),
                        UpdateDialogStatus::Error { ref message } => {
                            self.render_error_state(message, theme, cx)
                        }
                        UpdateDialogStatus::UpdateAvailable {
                            ref release,
                            ref download,
                        } => self.render_update_available_state(
                            release, download, auto_check, theme, cx,
                        ),
                    }),
            )
    }
}
