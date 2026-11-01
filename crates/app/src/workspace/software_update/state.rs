use crate::updater::ReleaseInfo;
use std::path::PathBuf;
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
