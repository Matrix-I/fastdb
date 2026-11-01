use super::*;
use crate::updater::{DEFAULT_REPO, UpdateCheckResult, check_github_latest_release};
impl UpdateController {
    /// Triggers checking GitHub for a new release
    pub(crate) fn trigger_update_check(&mut self, is_manual: bool, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        self.check_id += 1;
        let check_id = self.check_id;
        self.update_state.last_check_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.save_preferences();
        if is_manual {
            self.update_dialog = Some(UpdateDialog::checking(true));
            cx.notify();
        }

        let http_client = self.http_client.clone();
        let state = self.update_state.clone();
        let repo = DEFAULT_REPO.to_string();
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let (tx, rx) = flume::bounded::<UpdateCheckResult>(1);

        let executor = cx.background_executor().clone();
        cx.background_executor()
            .spawn(async move {
                let start = std::time::Instant::now();
                let res = match check_github_latest_release(
                    &http_client,
                    &repo,
                    &current_version,
                    &state,
                    is_manual,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => UpdateCheckResult::Error(e.to_string()),
                };

                // Keep dialog in loading state for at least 800ms so the user sees the check in progress
                if is_manual {
                    let elapsed = start.elapsed();
                    let min_dur = std::time::Duration::from_millis(800);
                    if elapsed < min_dur {
                        executor.timer(min_dur - elapsed).await;
                    }
                }

                let _ = tx.send(res);
            })
            .detach();

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.recv_async().await {
                let _ = this.update(cx, |workspace, cx| {
                    if workspace.check_id != check_id {
                        return;
                    }
                    match res {
                        UpdateCheckResult::UpdateAvailable(info) => {
                            workspace.pending_update = Some(*info.clone());
                            workspace.update_dialog =
                                Some(UpdateDialog::update_available(*info, is_manual));
                            cx.notify();
                        }
                        UpdateCheckResult::UpToDate { current_version } => {
                            if is_manual {
                                workspace.update_dialog =
                                    Some(UpdateDialog::up_to_date(current_version, true));
                                cx.notify();
                            }
                        }
                        UpdateCheckResult::Skipped(info) => {
                            if is_manual {
                                workspace.pending_update = Some(*info.clone());
                                workspace.update_dialog =
                                    Some(UpdateDialog::update_available(*info, true));
                                cx.notify();
                            }
                        }
                        UpdateCheckResult::Error(err) => {
                            if is_manual {
                                workspace.update_dialog = Some(UpdateDialog::error(err, true));
                                cx.notify();
                            } else {
                                tracing::warn!("Auto update background check failed: {}", err);
                            }
                        }
                    }
                });
            }
        })
        .detach();
    }
}
