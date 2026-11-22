use crate::updater::{ReleaseInfo, VerificationFailure, VerifiedInstaller};
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
        installer: VerifiedInstaller,
        reason: String,
    },
    Failed {
        error: String,
    },
}

impl DownloadUiState {
    pub(super) fn after_install(
        result: anyhow::Result<()>,
        installer: VerifiedInstaller,
        new_version: String,
    ) -> Self {
        match result {
            Ok(()) => Self::Complete { new_version },
            Err(error)
                if error.is::<VerificationFailure>() || error.is::<std::fs::TryLockError>() =>
            {
                Self::Failed {
                    error: error.to_string(),
                }
            }
            Err(error) => Self::Finished {
                installer,
                reason: format!("{error:#}"),
            },
        }
    }
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
