//! "Software Update" dialog for FastDB.
//!
//! Renders the modal dialog for checking updates, notifying when an update is available,
//! viewing release notes, downloading release packages with progress indication, and
//! launching the installer.

use std::path::PathBuf;

use super::*;
use crate::updater::{
    DownloadProgressEvent, ReleaseInfo, apply_update_file, clean_version_str, download_asset_file,
    format_bytes, open_installer_file, restart_application,
};

/// State of the active download and installation in the update dialog
#[derive(Debug, Clone)]
pub enum DownloadUiState {
    Idle,
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        percent: f32,
    },
    Installing,
    Complete {
        new_version: String,
    },
    Finished {
        path: PathBuf,
    },
    Failed {
        error: String,
    },
}

/// Current status shown inside the update dialog
#[derive(Debug, Clone)]
// The modal owns one status; boxing would complicate every UI transition.
#[allow(clippy::large_enum_variant)]
pub enum UpdateDialogStatus {
    Checking,
    UpToDate {
        current_version: String,
    },
    UpdateAvailable {
        release: ReleaseInfo,
        download: DownloadUiState,
    },
    Error {
        message: String,
    },
}

/// The state of the Software Update modal dialog
#[derive(Debug, Clone)]
pub struct UpdateDialog {
    #[allow(dead_code)]
    pub is_manual: bool,
    pub status: UpdateDialogStatus,
}

impl UpdateDialog {
    pub fn checking(is_manual: bool) -> Self {
        Self {
            is_manual,
            status: UpdateDialogStatus::Checking,
        }
    }

    pub fn up_to_date(current_version: String, is_manual: bool) -> Self {
        Self {
            is_manual,
            status: UpdateDialogStatus::UpToDate { current_version },
        }
    }

    pub fn update_available(release: ReleaseInfo, is_manual: bool) -> Self {
        Self {
            is_manual,
            status: UpdateDialogStatus::UpdateAvailable {
                release,
                download: DownloadUiState::Idle,
            },
        }
    }

    pub fn error(message: String, is_manual: bool) -> Self {
        Self {
            is_manual,
            status: UpdateDialogStatus::Error { message },
        }
    }
}

impl Workspace {
    /// Closes the software update dialog
    pub(super) fn close_update_dialog(&mut self) {
        self.update_dialog = None;
    }

    /// Skips the current release version so automatic checks will ignore it
    pub(super) fn skip_current_version(&mut self) {
        if let Some(ref dialog) = self.update_dialog
            && let UpdateDialogStatus::UpdateAvailable { ref release, .. } = dialog.status
        {
            self.update_state.skipped_version = Some(release.tag_name.clone());
            let _ = self.update_state.save();
        }
        self.update_dialog = None;
        self.pending_update = None;
    }

    /// Toggles whether FastDB automatically checks for updates on startup
    pub(super) fn toggle_auto_check_updates(&mut self) {
        self.update_state.auto_check_enabled = !self.update_state.auto_check_enabled;
        let _ = self.update_state.save();
    }

    /// Initiates downloading the platform-matched release asset
    pub(super) fn start_update_download(&mut self, cx: &mut Context<Self>) {
        let (asset, tag_name) = match self.update_dialog.as_ref().map(|d| &d.status) {
            Some(UpdateDialogStatus::UpdateAvailable { release, .. }) => {
                if let Some(ref asset) = release.matching_asset {
                    (asset.clone(), release.tag_name.clone())
                } else {
                    return;
                }
            }
            _ => return,
        };

        // Transition state to Downloading
        if let Some(ref mut dialog) = self.update_dialog
            && let UpdateDialogStatus::UpdateAvailable {
                ref mut download, ..
            } = dialog.status
        {
            *download = DownloadUiState::Downloading {
                downloaded: 0,
                total: if asset.size > 0 {
                    Some(asset.size)
                } else {
                    None
                },
                percent: 0.0,
            };
        }

        // Determine destination file path in user's Downloads or updates directory
        let dest_dir = platform_hal::paths::downloads_dir();
        let filename = if asset.name.is_empty() {
            format!("FastDB-{}.dmg", clean_version_str(&tag_name))
        } else {
            asset.name.clone()
        };
        let dest_path = dest_dir.join(filename);

        let (tx, rx) = flume::unbounded::<DownloadProgressEvent>();
        let client = self.http_client.clone();
        let download_url = asset.browser_download_url.clone();
        let file_path = dest_path.clone();

        // Spawn background download task
        cx.background_executor()
            .spawn(async move {
                download_asset_file(client, download_url, file_path, tx).await;
            })
            .detach();

        // Spawn foreground progress receiver task
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv_async().await {
                let event_clone = event.clone();
                let _ = this.update(cx, |workspace, cx| {
                    if let Some(ref mut dialog) = workspace.update_dialog
                        && let UpdateDialogStatus::UpdateAvailable {
                            ref mut download, ..
                        } = dialog.status
                    {
                        match event_clone {
                            DownloadProgressEvent::Progress { downloaded, total } => {
                                let percent = if let Some(tot) = total {
                                    if tot > 0 {
                                        (downloaded as f32 / tot as f32).clamp(0.0, 1.0)
                                    } else {
                                        0.0
                                    }
                                } else {
                                    0.0
                                };
                                *download = DownloadUiState::Downloading {
                                    downloaded,
                                    total,
                                    percent,
                                };
                            }
                            DownloadProgressEvent::Finished { path } => {
                                *download = DownloadUiState::Installing;
                                cx.notify();

                                let target_path = path.clone();
                                let tag = tag_name.clone();
                                let (install_tx, install_rx) =
                                    flume::bounded::<Result<(), String>>(1);

                                cx.background_executor()
                                    .spawn(async move {
                                        let res = apply_update_file(&target_path)
                                            .map_err(|e| e.to_string());
                                        let _ = install_tx.send(res);
                                    })
                                    .detach();

                                cx.spawn(async move |this, cx| {
                                    if let Ok(res) = install_rx.recv_async().await {
                                        let _ = this.update(cx, |workspace, cx| {
                                            if let Some(ref mut dialog) = workspace.update_dialog
                                                && let UpdateDialogStatus::UpdateAvailable {
                                                    ref mut download,
                                                    ..
                                                } = dialog.status
                                            {
                                                match res {
                                                    Ok(()) => {
                                                        *download = DownloadUiState::Complete {
                                                            new_version: tag,
                                                        };
                                                    }
                                                    Err(_) => {
                                                        *download =
                                                            DownloadUiState::Finished { path };
                                                    }
                                                }
                                                cx.notify();
                                            }
                                        });
                                    }
                                })
                                .detach();
                            }
                            DownloadProgressEvent::Failed { error } => {
                                *download = DownloadUiState::Failed { error };
                            }
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Renders the complete Software Update modal dialog
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
                    .w(px(560.0))
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border_focused)
                    .rounded_lg()
                    .shadow_lg()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
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
                            .pb_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(16.0))
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

    /// Renders the "Checking for updates..." state
    fn render_checking_state(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_8()
            .gap_3()
            .child(div().text_size(px(28.0)).child("🔄"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.0))
                    .text_color(theme.text_primary)
                    .child("Checking for updates..."),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child("Connecting to GitHub Releases to check for newer versions."),
            )
            .child(
                div().pt_3().child(
                    div()
                        .id("btn-cancel-checking")
                        .px_4()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.bg_surface_hover)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .text_color(theme.text_primary)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_selected))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_update_dialog();
                            cx.notify();
                        }))
                        .child("Cancel"),
                ),
            )
            .into_any_element()
    }

    /// Renders the "You're up to date!" state
    fn render_up_to_date_state(
        &self,
        current_version: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cur_ver = current_version.to_string();
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_6()
            .gap_3()
            .child(
                div()
                    .text_size(px(32.0))
                    .text_color(rgb(0x3fb950))
                    .child("✓"),
            )
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.0))
                    .text_color(theme.text_primary)
                    .child("FastDB is up to date!"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(format!(
                        "FastDB v{} is currently the newest version available.",
                        cur_ver
                    )),
            )
            .child(
                div().pt_4().child(
                    div()
                        .id("btn-uptodate-ok")
                        .px_5()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.text_accent)
                        .text_color(theme.bg_app)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(12.0))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_update_dialog();
                            cx.notify();
                        }))
                        .child("OK"),
                ),
            )
            .into_any_element()
    }

    /// Renders the "Update Check Failed" error state
    fn render_error_state(
        &self,
        message: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let err_msg = message.to_string();
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_6()
            .gap_3()
            .child(
                div()
                    .text_size(px(28.0))
                    .text_color(rgb(0xf85149))
                    .child("⚠"),
            )
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.0))
                    .text_color(theme.text_primary)
                    .child("Could Not Check for Updates"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .px_4()
                    .child(err_msg),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .pt_4()
                    .child(
                        div()
                            .id("btn-error-close")
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
                            .child("Close"),
                    )
                    .child(
                        div()
                            .id("btn-error-retry")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme.text_accent)
                            .text_color(theme.bg_app)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.trigger_update_check(true, cx);
                                cx.notify();
                            }))
                            .child("Try Again"),
                    ),
            )
            .into_any_element()
    }

    /// Renders the "Update Available" state with release notes and download actions
    fn render_update_available_state(
        &self,
        release: &ReleaseInfo,
        download: &DownloadUiState,
        auto_check: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let release_clone = release.clone();
        let html_url = release.html_url.clone();
        let pub_date = if release.published_at.len() >= 10 {
            &release.published_at[..10]
        } else {
            &release.published_at
        };
        let current_pkg_version = env!("CARGO_PKG_VERSION");

        div()
            .flex()
            .flex_col()
            .gap_3()
            // Title & Versions
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(15.0))
                            .text_color(theme.text_primary)
                            .child("A new version of FastDB is available!".to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_secondary)
                            .child(format!(
                                "FastDB {} is now available (you have v{}). Released on {}.",
                                release.tag_name, current_pkg_version, pub_date
                            )),
                    ),
            )
            // Release Notes container
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .child("Release Notes:"),
                    )
                    .child(
                        div()
                            .id("release-notes-scroll")
                            .h(px(150.0))
                            .overflow_y_scroll()
                            .p_3()
                            .rounded_md()
                            .bg(theme.bg_editor)
                            .border_1()
                            .border_color(theme.border)
                            .text_size(px(12.0))
                            .text_color(theme.text_secondary)
                            .child(if release.body.trim().is_empty() {
                                "No release notes provided for this version.".to_string()
                            } else {
                                release.body.clone()
                            }),
                    ),
            )
            // Download progress or matching package info
            .child(self.render_download_section(release, download, theme, cx))
            // Footer: Checkbox + Actions
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_3()
                    // Startup check toggle
                    .child(
                        div()
                            .id("chk-auto-update-toggle")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_auto_check_updates();
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(14.0))
                                    .h(px(14.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_xs()
                                    .border_1()
                                    .border_color(if auto_check {
                                        theme.text_accent
                                    } else {
                                        theme.border
                                    })
                                    .bg(if auto_check {
                                        theme.text_accent
                                    } else {
                                        theme.bg_surface_hover
                                    })
                                    .text_size(px(10.0))
                                    .text_color(theme.bg_app)
                                    .child(if auto_check { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("Automatically check for updates"),
                            ),
                    )
                    // Action Buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(if matches!(download, DownloadUiState::Complete { .. }) {
                                div()
                                    .id("btn-close-later")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_update_dialog();
                                        cx.notify();
                                    }))
                                    .child("Later")
                            } else {
                                div()
                                    .id("btn-skip-version")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip_current_version();
                                        cx.notify();
                                    }))
                                    .child("Skip This Version")
                            })
                            .child(
                                div()
                                    .id("btn-view-on-github")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_primary)
                                    .hover(|s| s.bg(theme.bg_surface_selected))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(&html_url);
                                    }))
                                    .child("View on GitHub"),
                            )
                            .child(self.render_primary_download_button(
                                &release_clone,
                                download,
                                theme,
                                cx,
                            )),
                    ),
            )
            .into_any_element()
    }

    /// Renders the download section (file info, progress bar, or completed state)
    fn render_download_section(
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
    fn render_primary_download_button(
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
                .on_click(cx.listener(|_, _, _, _| {
                    restart_application();
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
