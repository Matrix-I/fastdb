use super::script_saves::SaveQueue;
use crate::updater::{ReleaseInfo, UpdateState};
use gpui::*;
use ui_core::theme::Theme;
mod check;
mod dialog;
mod download_view;
mod state;
mod status_views;
mod tasks;
use state::{DownloadUiState, UpdateDialog, UpdateDialogStatus};

pub(super) struct UpdateController {
    theme: Theme,
    http_client: reqwest::Client,
    update_state: UpdateState,
    update_dialog: Option<UpdateDialog>,
    pending_update: Option<ReleaseInfo>,
    save_queue: SaveQueue,
    check_id: u64,
}
impl UpdateController {
    pub(super) fn new(theme: Theme, save_queue: SaveQueue) -> Self {
        Self {
            theme,
            http_client: reqwest::Client::new(),
            update_state: UpdateState::load(),
            update_dialog: None,
            pending_update: None,
            save_queue,
            check_id: 0,
        }
    }
    pub(super) fn is_open(&self) -> bool {
        self.update_dialog.is_some()
    }
    pub(super) fn pending(&self) -> Option<ReleaseInfo> {
        self.pending_update.clone()
    }
    pub(super) fn signature(&self) -> (bool, Option<String>) {
        (
            self.is_open(),
            self.pending_update
                .as_ref()
                .map(|info| info.tag_name.clone()),
        )
    }
    pub(super) fn is_time_for_auto_check(&self, now: u64) -> bool {
        self.update_state.is_time_for_auto_check(now)
    }
    fn is_busy(&self) -> bool {
        matches!(
            self.update_dialog.as_ref().map(|d| &d.status),
            Some(UpdateDialogStatus::UpdateAvailable {
                download: DownloadUiState::Downloading { .. } | DownloadUiState::Installing,
                ..
            })
        )
    }
    pub(super) fn show_release(&mut self, info: ReleaseInfo, cx: &mut Context<Self>) {
        if !self.is_busy() {
            self.check_id += 1;
            self.update_dialog = Some(UpdateDialog::update_available(info, true));
            cx.notify();
        }
    }
    pub(super) fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.is_busy() {
            self.check_id += 1;
            self.close_update_dialog();
            cx.notify();
        }
    }
    fn save_preferences(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.update_state) {
            self.save_queue.write(UpdateState::config_path(), text);
        }
    }
}
impl Render for UpdateController {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.is_open() {
            self.render_update_dialog(cx).into_any_element()
        } else {
            div().into_any_element()
        }
    }
}

pub(super) struct RestartRequested;
impl EventEmitter<RestartRequested> for UpdateController {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn progress_has_a_local_signature_and_installation_cannot_be_dismissed() {
        let mut controller = UpdateController {
            theme: Theme::dark(),
            http_client: reqwest::Client::new(),
            update_state: UpdateState::default(),
            update_dialog: Some(UpdateDialog::checking(true)),
            pending_update: None,
            save_queue: SaveQueue::default(),
            check_id: 0,
        };
        let signature = controller.signature();
        let release = ReleaseInfo {
            tag_name: "v0.2.0".into(),
            ..release_fixture()
        };
        controller.update_dialog = Some(UpdateDialog::update_available(release, true));
        assert_eq!(signature, controller.signature());
        if let Some(UpdateDialog {
            status: UpdateDialogStatus::UpdateAvailable { download, .. },
            ..
        }) = controller.update_dialog.as_mut()
        {
            *download = DownloadUiState::Installing;
        }
        controller.close_update_dialog();
        assert!(controller.is_open());
        assert!(controller.is_busy());
        controller.update_dialog = Some(UpdateDialog::up_to_date("0.1.0".into(), true));
        controller.close_update_dialog();
        assert!(!controller.is_open());
        assert_eq!(controller.check_id, 1);
    }
    fn release_fixture() -> ReleaseInfo {
        ReleaseInfo {
            tag_name: "v0.2.0".into(),
            name: String::new(),
            body: String::new(),
            html_url: String::new(),
            published_at: String::new(),
            assets: vec![],
            matching_asset: None,
        }
    }
}
