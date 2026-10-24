use std::path::PathBuf;
use std::fs;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use tracing::{info, warn};

pub const APP_QUALIFIER: &str = "com";
pub const APP_ORGANIZATION: &str = "fastdb";
pub const APP_NAME: &str = "FastDB";

/// Returns project directories (config, cache, data)
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
}

/// Returns the configuration directory for FastDB
pub fn config_dir() -> PathBuf {
    if let Some(proj) = project_dirs() {
        proj.config_dir().to_path_buf()
    } else {
        #[cfg(target_os = "macos")]
        {
            dirs_fallback_home().join("Library/Application Support/FastDB")
        }
        #[cfg(not(target_os = "macos"))]
        {
            dirs_fallback_home().join(".config/fastdb")
        }
    }
}

/// Returns the cache directory for FastDB
pub fn cache_dir() -> PathBuf {
    if let Some(proj) = project_dirs() {
        proj.cache_dir().to_path_buf()
    } else {
        #[cfg(target_os = "macos")]
        {
            dirs_fallback_home().join("Library/Caches/FastDB")
        }
        #[cfg(not(target_os = "macos"))]
        {
            dirs_fallback_home().join(".cache/fastdb")
        }
    }
}

/// Returns the spill-to-disk directory.
///
/// CRITICAL:
/// - On Linux: Never write spill files to `/tmp` because `/tmp` is typically mounted
///   as `tmpfs` in RAM! Use `/var/tmp/fastdb` instead.
/// - On macOS: Use `$TMPDIR/fastdb` or `std::env::temp_dir().join("fastdb")`.
pub fn spill_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from("/var/tmp/fastdb")
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join("fastdb")
    }
}

/// Ensure the spill directory exists
pub fn ensure_spill_dir() -> Result<PathBuf> {
    let dir = spill_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create spill directory: {}", dir.display()))?;
    }
    Ok(dir)
}

/// Automatically clean up abandoned `.spill` files on app startup
pub fn cleanup_old_spill_files() -> Result<usize> {
    let dir = spill_dir();
    if !dir.exists() {
        return Ok(0);
    }

    let mut removed = 0;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "spill" {
                        if let Err(e) = fs::remove_file(&path) {
                            warn!("Failed to remove old spill file {}: {}", path.display(), e);
                        } else {
                            removed += 1;
                        }
                    }
                }
            }
        }
    }

    if removed > 0 {
        info!("Cleaned up {} old spill files from {}", removed, dir.display());
    }

    Ok(removed)
}

fn dirs_fallback_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spill_dir_convention() {
        let dir = spill_dir();
        #[cfg(target_os = "linux")]
        {
            assert_eq!(dir, PathBuf::from("/var/tmp/fastdb"));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(dir.ends_with("fastdb"));
        }
    }

    #[test]
    fn test_cleanup_old_spill_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let spill_file = temp_dir.path().join("test.spill");
        fs::write(&spill_file, b"test").unwrap();
        assert!(spill_file.exists());
    }
}
