use super::*;
use crate::updater::{DownloadProgressEvent, NATIVE_INSTALLER, download_asset_file};
impl UpdateController {
    pub(super) fn close_update_dialog(&mut self) {
        if self.is_busy() {
            return;
        }
        self.check_id += 1;
        self.update_dialog = None;
    }

    /// Skips the current release version so automatic checks will ignore it
    pub(super) fn skip_current_version(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(ref dialog) = self.update_dialog
            && let UpdateDialogStatus::UpdateAvailable { ref release, .. } = dialog.status
        {
            self.update_state.skipped_version = Some(release.tag_name.clone());
            self.save_preferences();
        }
        self.check_id += 1;
        self.update_dialog = None;
        self.pending_update = None;
    }

    /// Toggles whether FastDB automatically checks for updates on startup
    pub(super) fn toggle_auto_check_updates(&mut self) {
        self.update_state.auto_check_enabled = !self.update_state.auto_check_enabled;
        self.save_preferences();
    }

    /// Initiates downloading the platform-matched release asset
    pub(super) fn start_update_download(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let (assets, asset, tag_name) = match self.update_dialog.as_ref().map(|d| &d.status) {
            Some(UpdateDialogStatus::UpdateAvailable { release, .. }) => {
                if let Some(ref asset) = release.matching_asset {
                    (
                        release.assets.clone(),
                        asset.clone(),
                        release.tag_name.clone(),
                    )
                } else {
                    return;
                }
            }
            _ => return,
        };

        // Ignore an older background version check while this dialog owns the installation.
        self.check_id += 1;

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
        let (tx, rx) = flume::bounded::<DownloadProgressEvent>(8);
        let client = self.http_client.clone();

        // Spawn background download task
        cx.background_executor()
            .spawn(async move {
                download_asset_file(client, assets, asset, dest_dir, tx).await;
            })
            .detach();

        // Spawn foreground progress receiver task
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv_async().await {
                let _ = this.update(cx, |workspace, cx| {
                    if let Some(ref mut dialog) = workspace.update_dialog
                        && let UpdateDialogStatus::UpdateAvailable {
                            ref mut download, ..
                        } = dialog.status
                    {
                        match event {
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
                            DownloadProgressEvent::Finished { installer } => {
                                *download = DownloadUiState::Installing;
                                cx.notify();

                                let target = installer.clone();
                                let tag = tag_name.clone();
                                let (install_tx, install_rx) = flume::bounded(1);

                                cx.background_executor()
                                    .spawn(async move {
                                        let res = NATIVE_INSTALLER.apply(&target);
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
                                                *download = DownloadUiState::after_install(
                                                    res, installer, tag,
                                                );
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

    pub(super) fn open_verified_installer(
        &mut self,
        close_on_success: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(UpdateDialog {
            status: UpdateDialogStatus::UpdateAvailable { download, .. },
            ..
        }) = self.update_dialog.as_mut()
        else {
            return;
        };
        let DownloadUiState::Finished { installer, reason } = download else {
            return;
        };
        let installer = installer.clone();
        let reason = reason.clone();
        *download = DownloadUiState::Installing;
        cx.notify();
        let opening = cx.background_executor().spawn({
            let target = installer.clone();
            async move { NATIVE_INSTALLER.open(&target) }
        });
        cx.spawn(async move |this, cx| {
            let result = opening.await;
            let _ = this.update(cx, |controller, cx| {
                if let Some(UpdateDialog {
                    status: UpdateDialogStatus::UpdateAvailable { download, .. },
                    ..
                }) = controller.update_dialog.as_mut()
                {
                    match result {
                        Ok(()) if close_on_success => {
                            controller.update_dialog = None;
                        }
                        Ok(()) => *download = DownloadUiState::Finished { installer, reason },
                        Err(error) => {
                            *download = DownloadUiState::Failed {
                                error: format!("{error:#}"),
                            }
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
