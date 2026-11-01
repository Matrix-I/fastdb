use super::*;
use crate::updater::{
    DownloadProgressEvent, apply_update_file, clean_version_str, download_asset_file,
};
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
}
