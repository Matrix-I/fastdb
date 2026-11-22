use anyhow::{Context, Result};
use std::{fs, path::Path};

/// Keep the lock file at a stable pathname. Closing its handle releases the OS lock,
/// including when an installer process exits unexpectedly.
pub(super) fn acquire(updates: &Path) -> Result<fs::File> {
    fs::create_dir_all(updates).context("Cannot create update workspace")?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(updates.join("install.lock"))
        .context("Cannot open update installation lock")?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(error @ fs::TryLockError::WouldBlock) => {
            Err(error).context("Another FastDB process is already installing an update")
        }
        Err(error) => Err(error).context("Cannot lock update installation"),
    }
}

pub(super) fn new_stage(updates: &Path) -> Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(updates)
        .context("Cannot create private update staging directory")
}
