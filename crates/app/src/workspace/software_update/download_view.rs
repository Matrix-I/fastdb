use super::*;
use crate::updater::{format_bytes, open_installer_file};
impl UpdateController {
    pub(super) fn render_download_section(
        &self,
        release: &ReleaseInfo,
        download: &DownloadUiState,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match download {
            DownloadUiState::Idle => {
                if let Some(ref asset) = release.matching_asset {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .rounded_md()
                        .bg(theme.bg_surface_hover)
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(div().child("📦"))
                        .child(div().child(format!(
                            "Package: {} ({})",
                            asset.name,
                            format_bytes(asset.size)
                        )))
                        .into_any_element()
                } else {
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_muted)
                        .child("No prebuilt installer matching your platform was found. You can view the release on GitHub.")
                        .into_any_element()
                }
            }
            DownloadUiState::Downloading {
                downloaded,
                total,
                percent,
            } => {
                let pct_int = (*percent * 100.0) as u32;
                let downloaded_str = format_bytes(*downloaded);
                let total_str = total
                    .map(|t| format!(" / {}", format_bytes(t)))
                    .unwrap_or_default();

                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .p_2()
                    .rounded_md()
                    .bg(theme.bg_surface_hover)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.0))
                            .child(
                                div()
                                    .text_color(theme.text_primary)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Downloading update..."),
                            )
                            .child(
                                div()
                                    .text_color(theme.text_secondary)
                                    .child(format!("{}{} ({}%)", downloaded_str, total_str, pct_int)),
                            ),
                    )
                    // Progress Bar Bar
                    .child(
                        div()
                            .w_full()
                            .h(px(8.0))
                            .rounded_full()
                            .bg(theme.bg_editor)
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(*percent))
                                    .bg(rgb(0x388bfd))
                                    .rounded_full(),
                            ),
                    )
                    .into_any_element()
            }
            DownloadUiState::Installing => div()
                .flex()
                .flex_col()
                .gap_1p5()
                .p_2()
                .rounded_md()
                .bg(theme.bg_surface_hover)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.0))
                        .text_color(theme.text_primary)
                        .child(div().child("⚙️"))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Installing update..."),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child("Applying update to application bundle and verifying signature..."),
                )
                .into_any_element(),
            DownloadUiState::Complete { new_version } => div()
                .flex()
                .flex_col()
                .gap_1p5()
                .p_2p5()
                .rounded_md()
                .bg(rgba(0x23863633))
                .border_1()
                .border_color(rgb(0x238636))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x3fb950))
                        .child(div().child("🎉"))
                        .child(div().child("Update Installed Successfully!")),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(format!(
                            "FastDB has been upgraded to {}. Restart the application to apply changes.",
                            new_version
                        )),
                )
                .into_any_element(),
            DownloadUiState::Finished { path } => {
                let p = path.clone();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_2()
                    .rounded_md()
                    .bg(rgba(0x23863633))
                    .border_1()
                    .border_color(rgb(0x238636))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.0))
                            .text_color(rgb(0x3fb950))
                            .child(div().child("✓"))
                            .child(div().child(format!("Downloaded: {}", p.display()))),
                    )
                    .child(
                        div()
                            .id("btn-reveal-finder")
                            .px_2()
                            .py_0p5()
                            .rounded_xs()
                            .bg(theme.bg_surface_hover)
                            .text_size(px(11.0))
                            .text_color(theme.text_primary)
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, _| {
                                let _ = open_installer_file(&p);
                            }))
                            .child("Show in Folder"),
                    )
                    .into_any_element()
            }
            DownloadUiState::Failed { error } => {
                let err = error.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .bg(rgba(0xda363333))
                    .border_1()
                    .border_color(rgb(0xf85149))
                    .text_size(px(11.0))
                    .text_color(rgb(0xf85149))
                    .child(div().child("⚠"))
                    .child(div().child(format!("Download failed: {}", err)))
                    .into_any_element()
            }
        }
    }

    /// Renders the primary action button (Download / Open Installer)
    pub(super) fn render_primary_download_button(
        &self,
        release: &ReleaseInfo,
        download: &DownloadUiState,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match download {
            DownloadUiState::Idle => {
                if release.matching_asset.is_some() {
                    div()
                        .id("btn-start-download")
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_4()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.text_accent)
                        .text_color(theme.bg_app)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(12.0))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_update_download(cx);
                            cx.notify();
                        }))
                        .child(div().child("⚡"))
                        .child("Upgrade")
                        .into_any_element()
                } else {
                    div()
                        .id("btn-close-no-asset")
                        .px_4()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.bg_surface_hover)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .text_color(theme.text_primary)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_update_dialog();
                            cx.notify();
                        }))
                        .child("Close")
                        .into_any_element()
                }
            }
            DownloadUiState::Downloading { .. } => div()
                .flex()
                .items_center()
                .gap_1p5()
                .px_4()
                .py_1p5()
                .rounded_md()
                .bg(theme.bg_surface_hover)
                .text_color(theme.text_muted)
                .text_size(px(12.0))
                .child(div().child("⏳"))
                .child("Downloading...")
                .into_any_element(),
            DownloadUiState::Installing => div()
                .flex()
                .items_center()
                .gap_1p5()
                .px_4()
                .py_1p5()
                .rounded_md()
                .bg(theme.bg_surface_hover)
                .text_color(theme.text_muted)
                .text_size(px(12.0))
                .child(div().child("⚙️"))
                .child("Installing...")
                .into_any_element(),
            DownloadUiState::Complete { .. } => div()
                .id("btn-restart-app")
                .flex()
                .items_center()
                .gap_1p5()
                .px_4()
                .py_1p5()
                .rounded_md()
                .bg(rgb(0x238636))
                .text_color(rgb(0xffffff))
                .font_weight(FontWeight::BOLD)
                .text_size(px(12.0))
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(RestartRequested);
                }))
                .child(div().child("🚀"))
                .child("Restart Now")
                .into_any_element(),
            DownloadUiState::Finished { path } => {
                let p = path.clone();
                div()
                    .id("btn-open-installer")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .bg(rgb(0x238636))
                    .text_color(rgb(0xffffff))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let _ = open_installer_file(&p);
                        this.close_update_dialog();
                        cx.notify();
                    }))
                    .child(div().child("🚀"))
                    .child("Open Installer")
                    .into_any_element()
            }
            DownloadUiState::Failed { .. } => div()
                .id("btn-retry-download")
                .px_4()
                .py_1p5()
                .rounded_md()
                .bg(theme.text_accent)
                .text_color(theme.bg_app)
                .font_weight(FontWeight::BOLD)
                .text_size(px(12.0))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.start_update_download(cx);
                    cx.notify();
                }))
                .child("Retry Download")
                .into_any_element(),
        }
    }
}
