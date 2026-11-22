//! Auto Update core logic for FastDB.
//!
//! Handles querying GitHub Releases API, parsing semver, selecting the platform-appropriate
//! release asset (.dmg / .zip for macOS, .AppImage / .deb for Linux), streaming the download
//! with progress reporting, persisting user update preferences, and launching the installer.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
#[cfg(any(target_os = "macos", test))]
mod install_workspace;
mod verification;
pub use verification::{VerificationFailure, VerifiedInstaller};
const RELEASE_KEY: &str = include_str!("../../../resources/minisign.pub");
use serde::{Deserialize, Serialize};
use tracing::info;

pub const DEFAULT_REPO: &str = "Matrix-I/fastdb";
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

/// A single release asset attached to a GitHub release
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// Information about a GitHub release
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: String,
    pub assets: Vec<ReleaseAsset>,
    pub matching_asset: Option<ReleaseAsset>,
}

/// Internal GitHub API response structure
#[derive(Deserialize, Debug)]
struct GithubReleaseDto {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAssetDto>,
}

#[derive(Deserialize, Debug)]
struct GithubAssetDto {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Persistent state of the update checker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateState {
    pub last_check_timestamp: u64,
    pub auto_check_enabled: bool,
    pub skipped_version: Option<String>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            last_check_timestamp: 0,
            auto_check_enabled: true,
            skipped_version: None,
        }
    }
}

impl UpdateState {
    pub fn config_path() -> PathBuf {
        platform_hal::paths::config_dir().join("updater_state.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(state) = serde_json::from_str::<UpdateState>(&data)
        {
            return state;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn is_time_for_auto_check(&self, now: u64) -> bool {
        self.auto_check_enabled && (now >= self.last_check_timestamp + CHECK_INTERVAL_SECS)
    }

    pub fn record_check_now(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_check_timestamp = now;
        let _ = self.save();
    }
}

/// Progress event emitted during asset download
#[derive(Debug, Clone)]
pub enum DownloadProgressEvent {
    Progress { downloaded: u64, total: Option<u64> },
    Finished { installer: VerifiedInstaller },
    Failed { error: String },
}

/// Result of checking GitHub for updates
#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    UpToDate { current_version: String },
    UpdateAvailable(Box<ReleaseInfo>),
    Skipped(Box<ReleaseInfo>),
    Error(String),
}

/// Formats byte counts into human-readable strings (e.g. "42.5 MB")
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Cleans version strings by trimming 'v' or 'V' prefixes
pub fn clean_version_str(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

/// Determines whether `latest_tag` is semantically newer than `current_ver`
pub fn is_version_newer(latest_tag: &str, current_ver: &str) -> bool {
    let latest_clean = clean_version_str(latest_tag);
    let current_clean = clean_version_str(current_ver);

    match (
        semver::Version::parse(latest_clean),
        semver::Version::parse(current_clean),
    ) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => {
            // Fallback for tags that aren't strictly 3-part semver
            latest_clean != current_clean && !latest_clean.is_empty()
        }
    }
}

/// Finds the most suitable release asset for the current OS and architecture
pub fn find_matching_asset(assets: &[ReleaseAsset]) -> Option<ReleaseAsset> {
    #[cfg(target_os = "macos")]
    {
        // 1. Prefer macOS DMG for arm64 / Apple Silicon
        if let Some(a) = assets.iter().find(|a| {
            let lower = a.name.to_lowercase();
            lower.ends_with(".dmg") && (lower.contains("arm64") || lower.contains("aarch64"))
        }) {
            return Some(a.clone());
        }

        // 2. Any macOS DMG
        if let Some(a) = assets.iter().find(|a| {
            let lower = a.name.to_lowercase();
            lower.ends_with(".dmg")
        }) {
            return Some(a.clone());
        }

        // 3. macOS ZIP for arm64
        if let Some(a) = assets.iter().find(|a| {
            let lower = a.name.to_lowercase();
            lower.ends_with(".zip") && (lower.contains("arm64") || lower.contains("macos"))
        }) {
            return Some(a.clone());
        }

        // 4. Any ZIP
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".zip"))
        {
            return Some(a.clone());
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 1. AppImage
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".appimage"))
        {
            return Some(a.clone());
        }

        // 2. Debian package
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".deb"))
        {
            return Some(a.clone());
        }

        // 3. tar.gz archive
        if let Some(a) = assets.iter().find(|a| {
            let lower = a.name.to_lowercase();
            lower.ends_with(".tar.gz") || lower.ends_with(".tgz")
        }) {
            return Some(a.clone());
        }
    }

    None
}

/// Performs an asynchronous HTTP check against GitHub Releases API
pub async fn check_github_latest_release(
    http_client: &reqwest::Client,
    repo: &str,
    current_version: &str,
    state: &UpdateState,
    is_manual: bool,
) -> Result<UpdateCheckResult> {
    // GPUI's executor has no Tokio reactor. Keep all reqwest polling (and its
    // connection pool tasks) on the shared runtime, including calls from tests.
    let http_client = http_client.clone();
    let repo = repo.to_owned();
    let current_version = current_version.to_owned();
    let state = state.clone();
    db_wire::run_on_shared_runtime(async move {
        check_github_latest_release_inner(&http_client, &repo, &current_version, &state, is_manual)
            .await
    })
    .await
}

async fn check_github_latest_release_inner(
    http_client: &reqwest::Client,
    repo: &str,
    current_version: &str,
    state: &UpdateState,
    is_manual: bool,
) -> Result<UpdateCheckResult> {
    // Testing override: allows simulating a new release (e.g. FASTDB_OVERRIDE_LATEST_VERSION=0.2.0)
    if let Ok(sim_version) = std::env::var("FASTDB_OVERRIDE_LATEST_VERSION") {
        let tag = if sim_version.starts_with('v') || sim_version.starts_with('V') {
            sim_version
        } else {
            format!("v{}", sim_version)
        };
        let asset_name = format!("FastDB-{}-arm64.dmg", clean_version_str(&tag));
        let info = ReleaseInfo {
            tag_name: tag.clone(),
            name: format!("FastDB {}", tag),
            body: "### What's New in FastDB:\n- Auto-update feature inspired by Laplace Terminal\n- Native macOS & Linux automated installation\n- High-performance query streaming\n- Pure Rust GPU-accelerated UI".to_string(),
            html_url: format!("https://github.com/{repo}/releases/tag/{}", tag),
            published_at: "2026-10-27T00:00:00Z".to_string(),
            assets: vec![ReleaseAsset {
                name: asset_name.clone(),
                browser_download_url: format!("https://github.com/{repo}/releases"),
                size: 42_500_000,
            }],
            matching_asset: Some(ReleaseAsset {
                name: asset_name,
                browser_download_url: format!("https://github.com/{repo}/releases"),
                size: 42_500_000,
            }),
        };
        return Ok(UpdateCheckResult::UpdateAvailable(Box::new(info)));
    }

    let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let web_url = format!("https://github.com/{repo}/releases/latest");
    let tag_prefix = format!("https://github.com/{repo}/releases/tag/");
    let user_agent = format!("FastDB/{current_version}");
    let Some(info) =
        fetch_latest_release(http_client, &api_url, &web_url, &tag_prefix, &user_agent).await?
    else {
        return Ok(UpdateCheckResult::UpToDate {
            current_version: current_version.to_string(),
        });
    };

    Ok(classify_release(info, current_version, state, is_manual))
}

fn classify_release(
    info: ReleaseInfo,
    current_version: &str,
    state: &UpdateState,
    is_manual: bool,
) -> UpdateCheckResult {
    if is_version_newer(&info.tag_name, current_version) {
        if !is_manual && state.skipped_version.as_deref() == Some(&info.tag_name) {
            return UpdateCheckResult::Skipped(Box::new(info));
        }
        UpdateCheckResult::UpdateAvailable(Box::new(info))
    } else {
        UpdateCheckResult::UpToDate {
            current_version: current_version.to_string(),
        }
    }
}

async fn fetch_latest_release(
    http_client: &reqwest::Client,
    api_url: &str,
    web_url: &str,
    tag_prefix: &str,
    user_agent: &str,
) -> Result<Option<ReleaseInfo>> {
    match fetch_release_from_api(http_client, api_url, user_agent).await {
        Ok(release) => Ok(release),
        Err(api_error) => {
            // Like Litty, use the public release redirect when the API is unavailable.
            // This lookup does not consume the unauthenticated REST API quota.
            fetch_release_redirect(http_client, web_url, tag_prefix, user_agent)
                .await
                .map_err(|fallback_error| {
                    anyhow::anyhow!(
                        "{api_error:#}. Release page fallback failed: {fallback_error:#}"
                    )
                })
        }
    }
}

async fn fetch_release_from_api(
    http_client: &reqwest::Client,
    api_url: &str,
    user_agent: &str,
) -> Result<Option<ReleaseInfo>> {
    let resp = http_client
        .get(api_url)
        .timeout(std::time::Duration::from_secs(30))
        .header("User-Agent", user_agent)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("Failed to connect to GitHub Releases API")?;

    // If GitHub returns 404, the repo has no releases yet -> current version is latest
    if resp.status().as_u16() == 404 {
        info!(
            "GitHub Releases returned 404: no published releases yet, treating current version as latest"
        );
        return Ok(None);
    }

    if !resp.status().is_success() {
        let status = resp.status();
        bail!("GitHub Releases API returned status {}", status);
    }

    let dto = resp
        .json::<GithubReleaseDto>()
        .await
        .context("Failed to parse GitHub Release JSON response")?;

    let assets: Vec<ReleaseAsset> = dto
        .assets
        .into_iter()
        .map(|a| ReleaseAsset {
            name: a.name,
            browser_download_url: a.browser_download_url,
            size: a.size,
        })
        .collect();

    let matching_asset = find_matching_asset(&assets);

    Ok(Some(ReleaseInfo {
        tag_name: dto.tag_name.clone(),
        name: dto.name.unwrap_or_else(|| dto.tag_name.clone()),
        body: dto.body.unwrap_or_default(),
        html_url: dto.html_url,
        published_at: dto.published_at.unwrap_or_default(),
        assets,
        matching_asset,
    }))
}

fn release_tag_from_url<'a>(url: &'a str, tag_prefix: &str) -> Result<&'a str> {
    let tag = url
        .strip_prefix(tag_prefix)
        .context("Release redirect did not resolve to this repository's release tag")?;
    // Only accept plain semantic version tags, never paths, queries or fragments.
    anyhow::ensure!(
        tag.len() <= 64
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)),
        "Release redirect contains an invalid version tag"
    );
    semver::Version::parse(clean_version_str(tag)).context("Invalid release version")?;
    Ok(tag)
}

async fn fetch_release_redirect(
    http_client: &reqwest::Client,
    web_url: &str,
    tag_prefix: &str,
    user_agent: &str,
) -> Result<Option<ReleaseInfo>> {
    let response = http_client
        .head(web_url)
        .header("User-Agent", user_agent)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("Failed to connect to the GitHub release page")?
        .error_for_status()
        .context("GitHub release page request failed")?;
    let html_url = response.url().as_str().to_string();
    if Some(html_url.as_str()) == tag_prefix.strip_suffix("/tag/") {
        // With no published releases, GitHub redirects /latest to /releases.
        // Confirm the empty page rather than treating any non-tag redirect as current.
        let page = http_client
            .get(&html_url)
            .header("User-Agent", user_agent)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?;
        anyhow::ensure!(
            page.url().as_str() == html_url,
            "Unexpected release page redirect"
        );
        let body = page.text().await?;
        anyhow::ensure!(
            body.contains("There aren’t any releases here")
                || body.contains("There aren't any releases here")
                || body.contains("There aren&#39;t any releases here"),
            "GitHub did not identify a latest release or confirm an empty release list"
        );
        return Ok(None);
    }
    let tag_name = release_tag_from_url(&html_url, tag_prefix)?.to_string();
    Ok(Some(ReleaseInfo {
        name: format!("FastDB {tag_name}"),
        tag_name,
        html_url,
        body: "Release details could not be loaded from the GitHub API. View the release on GitHub for notes and downloads.".into(),
        published_at: String::new(),
        assets: Vec::new(),
        matching_asset: None,
    }))
}

pub fn release_page_url(tag: &str) -> String {
    let releases = format!("https://github.com/{DEFAULT_REPO}/releases");
    let prefix = format!("{releases}/tag/");
    let url = format!("{prefix}{tag}");
    if release_tag_from_url(&url, &prefix).is_ok() {
        url
    } else {
        releases
    }
}

/// Runs on the shared Tokio reactor even when called from a GPUI background task.
pub async fn download_asset_file(
    client: reqwest::Client,
    assets: Vec<ReleaseAsset>,
    asset: ReleaseAsset,
    dir: PathBuf,
    sender: flume::Sender<DownloadProgressEvent>,
) {
    download_with_key(client, assets, asset, dir, sender, RELEASE_KEY).await
}

async fn download_with_key(
    client: reqwest::Client,
    assets: Vec<ReleaseAsset>,
    asset: ReleaseAsset,
    dir: PathBuf,
    sender: flume::Sender<DownloadProgressEvent>,
    trusted_key: &'static str,
) {
    let progress = sender.clone();
    let result = db_wire::run_on_shared_runtime(async move {
        verification::download(client, assets, asset, dir, progress, trusted_key).await
    })
    .await;
    let event = match result {
        Ok(installer) => DownloadProgressEvent::Finished { installer },
        Err(error) => DownloadProgressEvent::Failed {
            error: format!("{error:#}"),
        },
    };
    let _ = sender.send_async(event).await;
}

enum InstallOutcome {
    Applied,
    #[cfg(any(target_os = "macos", test))]
    OpenManually,
}

pub struct InstallerActions<'a> {
    apply_native: &'a dyn Fn(&Path) -> Result<InstallOutcome>,
    open_native: &'a dyn Fn(&Path) -> Result<()>,
}

pub const NATIVE_INSTALLER: InstallerActions<'static> = InstallerActions {
    apply_native: &apply_native_installer,
    open_native: &open_native_installer,
};

impl InstallerActions<'_> {
    pub fn open(&self, installer: &VerifiedInstaller) -> Result<()> {
        installer.with_verified_file(self.open_native)
    }

    pub fn apply(&self, installer: &VerifiedInstaller) -> Result<()> {
        match installer.with_verified_file(self.apply_native)? {
            InstallOutcome::Applied => Ok(()),
            #[cfg(any(target_os = "macos", test))]
            InstallOutcome::OpenManually => self.open(installer),
        }
    }
}

fn open_native_installer(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "dmg" {
            // Open the DMG directly, which mounts it on macOS
            std::process::Command::new("open")
                .arg(path)
                .spawn()
                .context("Failed to open DMG installer")?;
        } else {
            // Reveal file in Finder
            std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn()
                .context("Failed to reveal file in Finder")?;
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Try xdg-open
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .context("Failed to open downloaded file with xdg-open")?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Detect if current executable is running inside an `.app` bundle on macOS
#[cfg(target_os = "macos")]
pub fn find_macos_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut current = exe.parent()?;
    while let Some(parent) = current.parent() {
        if current.extension().is_some_and(|ext| ext == "app") {
            return Some(current.to_path_buf());
        }
        current = parent;
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn apply_native_installer(path: &Path) -> Result<InstallOutcome> {
    open_native_installer(path)?;
    Ok(InstallOutcome::Applied)
}

#[cfg(target_os = "macos")]
fn apply_native_installer(path: &Path) -> Result<InstallOutcome> {
    let bundle =
        find_macos_app_bundle().unwrap_or_else(|| PathBuf::from("/Applications/FastDB.app"));
    apply_macos_installer(path, &platform_hal::paths::updates_dir(), &bundle)
}

#[cfg(any(target_os = "macos", test))]
fn apply_macos_installer(package: &Path, updates: &Path, bundle: &Path) -> Result<InstallOutcome> {
    use std::process::Command;

    let ext = package
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "dmg" | "zip") {
        return Ok(InstallOutcome::OpenManually);
    }
    let _lock = install_workspace::acquire(updates)?;
    let stage = install_workspace::new_stage(updates)?;
    info!("Applying update to app bundle: {}", bundle.display());
    if ext == "dmg" {
        // A mount point must never be removed recursively: detach first, then remove the empty directory.
        let mount = stage.keep();
        let attach = Command::new("hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
            .arg(&mount)
            .arg(package)
            .status();
        if !matches!(&attach, Ok(status) if status.success()) {
            fs::remove_dir(&mount).context("Cannot clean up update mount point")?;
            attach.context("Failed to run hdiutil attach")?;
            return Ok(InstallOutcome::OpenManually);
        }
        let staged_app = mount.join("FastDB.app");
        let installed = staged_app.exists()
            && Command::new("/usr/bin/ditto")
                .arg(&staged_app)
                .arg(bundle)
                .status()
                .is_ok_and(|status| status.success());
        if installed {
            let _ = Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(bundle)
                .status();
            let _ = Command::new("touch").arg(bundle).status();
        }
        let detach = Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(&mount)
            .status()
            .with_context(|| format!("Cannot detach update mount {}", mount.display()))?;
        anyhow::ensure!(
            detach.success(),
            "Cannot detach update mount {}",
            mount.display()
        );
        fs::remove_dir(&mount).context("Cannot remove update mount point")?;
        return Ok(if installed {
            InstallOutcome::Applied
        } else {
            InstallOutcome::OpenManually
        });
    }

    let extracted = Command::new("/usr/bin/ditto")
        .args(["-xk"])
        .arg(package)
        .arg(stage.path())
        .status()
        .context("Failed to extract update zip archive")?;
    let staged_app = stage.path().join("FastDB.app");
    if !extracted.success() || !staged_app.exists() {
        stage
            .close()
            .context("Cannot clean up update staging directory")?;
        return Ok(InstallOutcome::OpenManually);
    }
    let _ = Command::new("/usr/bin/ditto")
        .arg(&staged_app)
        .arg(bundle)
        .status();
    let _ = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(bundle)
        .status();
    let _ = Command::new("touch").arg(bundle).status();
    stage
        .close()
        .context("Cannot clean up update staging directory")?;
    Ok(InstallOutcome::Applied)
}

/// Relaunches the updated FastDB application and terminates current process
pub fn restart_application() {
    #[cfg(target_os = "macos")]
    {
        if let Some(app_path) = find_macos_app_bundle() {
            let _ = std::process::Command::new("open")
                .arg("-n")
                .arg(app_path)
                .spawn();
        } else if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new("open")
                .arg("-n")
                .arg(exe)
                .spawn();
        }
    }
    std::process::exit(0);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::verification::{parse_asset_checksum, release_metadata_asset, validate_asset_name};
    use super::*;

    fn lookup_release(responses: Vec<(&'static str, String)>) -> Result<Option<ReleaseInfo>> {
        let (base_url, server) = serve_http(responses);
        let result = futures::executor::block_on(db_wire::run_on_shared_runtime(async move {
            let client = reqwest::Client::builder().no_proxy().build()?;
            fetch_latest_release(
                &client,
                &format!("{base_url}/api"),
                &format!("{base_url}/latest"),
                &format!("{base_url}/releases/tag/"),
                "FastDB-test",
            )
            .await
        }));
        server.join().unwrap();
        result
    }

    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn release_api_success_preserves_metadata_and_assets() {
        let release = lookup_release(vec![("/api", response("200 OK", "", r#"{
            "tag_name":"v0.2.0", "name":"FastDB 0.2.0", "body":"Release notes",
            "html_url":"https://github.com/Matrix-I/fastdb/releases/tag/v0.2.0",
            "assets":[{"name":"FastDB-arm64.dmg","browser_download_url":"https://github.com/Matrix-I/fastdb/releases/download/v0.2.0/FastDB-arm64.dmg","size":123}]
        }"#))]).unwrap().unwrap();
        assert_eq!(release.body, "Release notes");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].size, 123);
    }

    #[test]
    fn rate_limit_and_invalid_json_use_the_release_redirect() {
        for api_response in [
            response(
                "403 Forbidden",
                "X-RateLimit-Remaining: 0\r\n",
                "rate limited",
            ),
            response("429 Too Many Requests", "", "rate limited"),
            response("200 OK", "", "invalid json"),
        ] {
            let release = lookup_release(vec![
                ("/api", api_response),
                (
                    "/latest",
                    response("302 Found", "Location: /releases/tag/v0.2.0\r\n", ""),
                ),
                ("/releases/tag/v0.2.0", response("200 OK", "", "")),
            ])
            .unwrap()
            .unwrap();
            assert_eq!(release.tag_name, "v0.2.0");
            assert!(release.assets.is_empty());
            assert!(release.matching_asset.is_none());
            assert!(matches!(
                classify_release(release.clone(), "0.2.0", &UpdateState::default(), true),
                UpdateCheckResult::UpToDate { .. }
            ));
            assert!(matches!(
                classify_release(release.clone(), "0.1.0", &UpdateState::default(), true),
                UpdateCheckResult::UpdateAvailable(_)
            ));
            let state = UpdateState {
                skipped_version: Some("v0.2.0".into()),
                ..UpdateState::default()
            };
            assert!(matches!(
                classify_release(release.clone(), "0.1.0", &state, false),
                UpdateCheckResult::Skipped(_)
            ));
            assert!(matches!(
                classify_release(release, "0.1.0", &state, true),
                UpdateCheckResult::UpdateAvailable(_)
            ));
        }
    }

    #[test]
    fn failed_fallback_remains_an_error() {
        let error = lookup_release(vec![
            ("/api", response("403 Forbidden", "", "")),
            ("/latest", response("500 Internal Server Error", "", "")),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("403"), "{error}");
        assert!(error.contains("500"), "{error}");
        assert!(
            lookup_release(vec![
                ("/api", response("403 Forbidden", "", "")),
                ("/latest", response("200 OK", "", "login page")),
            ])
            .is_err()
        );
    }

    #[test]
    fn release_redirect_requires_the_expected_repository_and_a_version() {
        let prefix = "https://github.com/Matrix-I/fastdb/releases/tag/";
        assert_eq!(
            release_tag_from_url(&format!("{prefix}v1.2.3"), prefix).unwrap(),
            "v1.2.3"
        );
        for url in [
            "https://github.com/Matrix-I/other/releases/tag/v1.2.3",
            "http://github.com/Matrix-I/fastdb/releases/tag/v1.2.3",
            "https://github.com.evil.test/Matrix-I/fastdb/releases/tag/v1.2.3",
        ] {
            assert!(release_tag_from_url(url, prefix).is_err());
        }
        for tag in [
            "",
            "latest",
            "v1.2.3/asset",
            "v1.2.3?query",
            "v1.2.3#fragment",
            "v1.2.3%2fasset",
        ] {
            assert!(release_tag_from_url(&format!("{prefix}{tag}"), prefix).is_err());
        }
    }

    #[test]
    fn no_published_release_preserves_the_existing_behavior() {
        assert!(
            lookup_release(vec![("/api", response("404 Not Found", "", ""))])
                .unwrap()
                .is_none()
        );
        assert!(
            lookup_release(vec![
                ("/api", response("403 Forbidden", "", "")),
                (
                    "/latest",
                    response("302 Found", "Location: /releases\r\n", "")
                ),
                ("/releases", response("200 OK", "", "")),
                (
                    "/releases",
                    response("200 OK", "", "<h2>There aren’t any releases here</h2>")
                ),
            ])
            .unwrap()
            .is_none()
        );
        assert!(
            lookup_release(vec![
                ("/api", response("403 Forbidden", "", "")),
                (
                    "/latest",
                    response("302 Found", "Location: /releases\r\n", "")
                ),
                ("/releases", response("200 OK", "", "")),
                ("/releases", response("200 OK", "", "Unrecognized page")),
            ])
            .is_err()
        );
    }

    // GPUI polls background work without entering a Tokio runtime. Exercise the
    // same entry points with a plain executor and a local HTTP server.
    fn serve_http(responses: Vec<(&'static str, String)>) -> (String, std::thread::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            futures::executor::block_on(db_wire::run_on_shared_runtime(async move {
                let listener = tokio::net::TcpListener::from_std(listener)?;
                for (path, response) in responses {
                    tokio::time::timeout(std::time::Duration::from_secs(10), async {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let mut request = Vec::new();
                        let mut chunk = [0; 1024];
                        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            let count = socket.read(&mut chunk).await.unwrap();
                            assert!(count > 0);
                            request.extend_from_slice(&chunk[..count]);
                            assert!(request.len() <= 64 * 1024);
                        }
                        let request = String::from_utf8(request).unwrap();
                        assert_eq!(request.split_whitespace().nth(1), Some(path));
                        socket.write_all(response.as_bytes()).await.unwrap();
                    })
                    .await
                    .with_context(|| format!("Timed out serving {path}"))?;
                }
                Ok(())
            }))
            .unwrap();
        });
        (base_url, server)
    }

    #[test]
    fn signed_checksum_uses_one_exact_safe_filename() {
        let zeros = "00".repeat(32);
        let lines = format!("{zeros}  other.dmg\n{zeros} *FastDB.dmg\n");
        assert_eq!(
            parse_asset_checksum(lines.as_bytes(), "FastDB.dmg").unwrap(),
            [0; 32]
        );
        assert!(parse_asset_checksum(lines.as_bytes(), "fastdb.dmg").is_err());

        let duplicate = format!("{zeros}  FastDB.dmg\n{zeros} *FastDB.dmg\n");
        assert!(parse_asset_checksum(duplicate.as_bytes(), "FastDB.dmg").is_err());
        assert!(parse_asset_checksum(b"invalid  FastDB.dmg\n", "FastDB.dmg").is_err());

        for name in [
            "",
            ".",
            "..",
            "../FastDB.dmg",
            "/tmp/file.dmg",
            "a\\file.dmg",
        ] {
            assert!(validate_asset_name(name).is_err(), "{name:?}");
        }
        assert!(validate_asset_name("FastDB.dmg").is_ok());
    }

    #[test]
    fn release_metadata_lookup_rejects_ambiguity() {
        let manifest = ReleaseAsset {
            name: "SHA256SUMS.txt".into(),
            browser_download_url: "https://example.invalid/manifest".into(),
            size: 16,
        };
        assert!(release_metadata_asset(&[], "SHA256SUMS.txt").is_err());
        assert_eq!(
            release_metadata_asset(std::slice::from_ref(&manifest), "SHA256SUMS.txt").unwrap(),
            &manifest,
        );
        assert!(release_metadata_asset(&[manifest.clone(), manifest], "SHA256SUMS.txt").is_err());
    }

    #[test]
    fn release_link_opens_only_the_configured_github_release_page() {
        let releases = "https://github.com/Matrix-I/fastdb/releases";
        assert_eq!(
            release_page_url("v0.3.0-rc.1"),
            format!("{releases}/tag/v0.3.0-rc.1"),
        );
        for tag in [
            "file:///tmp/update.dmg",
            "open-installer:payload",
            "../../download",
            "v1.2.3?x=1",
        ] {
            assert_eq!(release_page_url(tag), releases);
        }
    }

    const MANIFEST: &str = include_str!("updater/fixtures/manifest");
    const SIGNATURE: &str = include_str!("updater/fixtures/manifest.minisig");
    const FIXTURE_KEY: &str = include_str!("updater/fixtures/test.pub");

    fn download_fixture(
        responses: Vec<(&'static str, String)>,
        key: &'static str,
        dir: &Path,
    ) -> Vec<DownloadProgressEvent> {
        let (base, server) = serve_http(responses);
        let asset = |name: &str| ReleaseAsset {
            name: name.into(),
            browser_download_url: format!("{base}/{name}"),
            size: 0,
        };
        let package = asset("update.dmg");
        let assets = vec![
            package.clone(),
            asset("SHA256SUMS.txt"),
            asset("SHA256SUMS.txt.minisig"),
        ];
        let (tx, rx) = flume::unbounded();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        futures::executor::block_on(download_with_key(
            client,
            assets,
            package,
            dir.to_owned(),
            tx,
            key,
        ));
        server.join().unwrap();
        rx.try_iter().collect()
    }

    fn signed_responses(manifest: &str, signature: &str) -> Vec<(&'static str, String)> {
        vec![
            ("/SHA256SUMS.txt", response("200 OK", "", manifest)),
            ("/SHA256SUMS.txt.minisig", response("200 OK", "", signature)),
        ]
    }

    pub(crate) fn download_test_installer(dir: &Path) -> VerifiedInstaller {
        let mut responses = signed_responses(MANIFEST, SIGNATURE);
        responses.push(("/update.dmg", response("200 OK", "", "test update bytes")));
        let events = download_fixture(responses, FIXTURE_KEY, dir);
        let Some(DownloadProgressEvent::Finished { installer }) = events.last() else {
            panic!("{events:?}")
        };
        installer.clone()
    }

    pub(crate) fn recording_apply_error(installer: &VerifiedInstaller) -> anyhow::Error {
        let actions = InstallerActions {
            apply_native: &|_| bail!("Fixture native installer unavailable"),
            open_native: &|_| panic!("Changed file reached native open"),
        };
        actions.apply(installer).unwrap_err()
    }

    #[test]
    fn download_works_without_a_caller_tokio_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("update.dmg");
        let installer = download_test_installer(temp.path());
        assert_eq!(installer.path(), dest);
        assert_eq!(fs::read(&dest).unwrap(), b"test update bytes");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        let calls = std::cell::RefCell::new(Vec::new());
        let actions = InstallerActions {
            apply_native: &|path| {
                assert_eq!(path, dest);
                calls.borrow_mut().push("apply");
                Ok(InstallOutcome::Applied)
            },
            open_native: &|path| {
                assert_eq!(path, dest);
                calls.borrow_mut().push("open");
                Ok(())
            },
        };
        actions.apply(&installer).unwrap();
        actions.open(&installer).unwrap();
        let assert_blocked = || {
            assert!(
                actions
                    .apply(&installer)
                    .unwrap_err()
                    .is::<VerificationFailure>()
            );
            assert!(
                actions
                    .open(&installer)
                    .unwrap_err()
                    .is::<VerificationFailure>()
            );
        };
        let fallback = InstallerActions {
            apply_native: &|path| {
                fs::write(path, b"changed")?;
                Ok(InstallOutcome::OpenManually)
            },
            open_native: actions.open_native,
        };
        assert!(
            fallback
                .apply(&installer)
                .unwrap_err()
                .is::<VerificationFailure>()
        );
        assert_blocked();
        fs::remove_file(&dest).unwrap();
        assert_blocked();
        #[cfg(unix)]
        {
            let original = temp.path().join("original");
            fs::write(&original, b"test update bytes").unwrap();
            std::os::unix::fs::symlink(original, &dest).unwrap();
            assert_blocked();
        }
        assert_eq!(calls.into_inner(), ["apply", "open"]);
    }

    #[test]
    fn installation_workspaces_are_isolated_and_exclusive() {
        let root = tempfile::tempdir().unwrap();
        let first = install_workspace::acquire(root.path()).unwrap();
        let stage_one = install_workspace::new_stage(root.path()).unwrap();
        let stage_two = install_workspace::new_stage(root.path()).unwrap();
        fs::write(stage_one.path().join("FastDB.app"), b"first").unwrap();
        fs::write(stage_two.path().join("FastDB.app"), b"second").unwrap();
        assert_eq!(
            fs::read(stage_one.path().join("FastDB.app")).unwrap(),
            b"first"
        );
        assert_ne!(stage_one.path(), stage_two.path());
        assert!(
            install_workspace::acquire(root.path())
                .unwrap_err()
                .to_string()
                .contains("already installing")
        );
        let install = apply_macos_installer(
            Path::new("update.zip"),
            root.path(),
            &root.path().join("FastDB.app"),
        );
        assert!(matches!(install, Err(error) if error.is::<fs::TryLockError>()));
        drop(first);
        let _next = install_workspace::acquire(root.path()).unwrap();
        assert!(install_workspace::acquire(root.path()).is_err());
        let second = stage_two.path().to_owned();
        stage_two.close().unwrap();
        assert!(!second.exists());
        assert!(stage_one.path().is_dir());
    }

    #[test]
    fn unverified_download_preserves_destination_and_reports_reason() {
        let missing = || response("404 Not Found", "", "");
        let mut bad_package = signed_responses(MANIFEST, SIGNATURE);
        bad_package.push(("/update.dmg", response("200 OK", "", "changed")));
        for (responses, key, reason) in [
            (
                vec![("/SHA256SUMS.txt", missing())],
                FIXTURE_KEY,
                "SHA256SUMS.txt returned HTTP 404",
            ),
            (
                vec![
                    ("/SHA256SUMS.txt", response("200 OK", "", MANIFEST)),
                    ("/SHA256SUMS.txt.minisig", missing()),
                ],
                FIXTURE_KEY,
                "SHA256SUMS.txt.minisig returned HTTP 404",
            ),
            (
                signed_responses(MANIFEST, "invalid"),
                FIXTURE_KEY,
                "Invalid update manifest signature",
            ),
            (
                signed_responses(MANIFEST, SIGNATURE),
                RELEASE_KEY,
                "signature verification failed",
            ),
            (
                signed_responses(include_str!("updater/fixtures/other-manifest"), SIGNATURE),
                FIXTURE_KEY,
                "signature verification failed",
            ),
            (
                signed_responses(
                    include_str!("updater/fixtures/other-manifest"),
                    include_str!("updater/fixtures/other-manifest.minisig"),
                ),
                FIXTURE_KEY,
                "No checksum",
            ),
            (bad_package, FIXTURE_KEY, "checksum mismatch"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let dest = temp.path().join("update.dmg");
            fs::write(&dest, "existing").unwrap();
            let events = download_fixture(responses, key, temp.path());
            let terminal: Vec<_> = events
                .iter()
                .filter(|e| !matches!(e, DownloadProgressEvent::Progress { .. }))
                .collect();
            assert!(
                matches!(terminal.as_slice(), [DownloadProgressEvent::Failed{error}] if error.contains(reason)),
                "{events:?}"
            );
            assert_eq!(fs::read(&dest).unwrap(), b"existing");
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn invalid_release_metadata_is_rejected_before_any_download() {
        let asset = |name: &str| ReleaseAsset {
            name: name.into(),
            browser_download_url: "unused".into(),
            size: 0,
        };
        let package = "update.dmg";
        let manifest = "SHA256SUMS.txt";
        let signature = "SHA256SUMS.txt.minisig";
        for (selected, names, reason) in [
            (
                package,
                vec![package, manifest],
                "missing SHA256SUMS.txt.minisig",
            ),
            (package, vec![package, signature], "missing SHA256SUMS.txt"),
            (
                package,
                vec![package, manifest, manifest, signature],
                "Duplicate release asset SHA256SUMS.txt",
            ),
            (
                package,
                vec![package, package, manifest, signature],
                "Duplicate release asset update.dmg",
            ),
            (
                "../update.dmg",
                vec!["../update.dmg", manifest, signature],
                "Invalid update asset filename",
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("downloads");
            fs::create_dir(&dir).unwrap();
            let dest = dir.join(package);
            fs::write(&dest, b"existing").unwrap();
            let (tx, rx) = flume::unbounded();
            futures::executor::block_on(download_asset_file(
                reqwest::Client::new(),
                names.into_iter().map(asset).collect(),
                asset(selected),
                dir.clone(),
                tx,
            ));
            let events: Vec<_> = rx.try_iter().collect();
            assert!(
                matches!(events.as_slice(), [DownloadProgressEvent::Failed { error }] if error.contains(reason)),
                "{events:?}"
            );
            assert_eq!(fs::read(dest).unwrap(), b"existing");
            assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
            assert!(!temp.path().join(package).exists());
        }
    }

    #[test]
    fn update_check_works_without_a_caller_tokio_runtime() {
        // Even an unreachable endpoint must return an error, never panic because
        // the executor has no Tokio reactor. This does not require internet access.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(proxy).unwrap())
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let result = futures::executor::block_on(check_github_latest_release(
            &client,
            DEFAULT_REPO,
            "0.1.0",
            &UpdateState::default(),
            false,
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_clean_version_str() {
        assert_eq!(clean_version_str("v0.1.0"), "0.1.0");
        assert_eq!(clean_version_str("V1.2.3"), "1.2.3");
        assert_eq!(clean_version_str("0.1.0"), "0.1.0");
        assert_eq!(clean_version_str("  v2.0.0  "), "2.0.0");
    }

    #[test]
    fn test_is_version_newer() {
        assert!(is_version_newer("v0.2.0", "0.1.0"));
        assert!(is_version_newer("0.1.1", "0.1.0"));
        assert!(is_version_newer("v1.0.0", "0.9.9"));
        assert!(!is_version_newer("v0.1.0", "0.1.0"));
        assert!(!is_version_newer("0.0.9", "0.1.0"));
        assert!(!is_version_newer("v0.1.0", "0.2.0"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "2 KB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_bytes(1500 * 1024 * 1024), "1.46 GB");
    }

    #[test]
    fn test_update_state_serialization() {
        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("updater_state.json");

        let state = UpdateState {
            last_check_timestamp: 123456789,
            auto_check_enabled: true,
            skipped_version: Some("v0.1.9".to_string()),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        fs::write(&state_path, json).unwrap();

        let loaded_json = fs::read_to_string(&state_path).unwrap();
        let loaded: UpdateState = serde_json::from_str(&loaded_json).unwrap();

        assert_eq!(loaded.last_check_timestamp, 123456789);
        assert!(loaded.auto_check_enabled);
        assert_eq!(loaded.skipped_version.as_deref(), Some("v0.1.9"));
    }

    #[test]
    fn test_find_matching_asset() {
        let assets = vec![
            ReleaseAsset {
                name: "SHA256SUMS.txt".into(),
                browser_download_url: "https://example.com/sha".into(),
                size: 200,
            },
            ReleaseAsset {
                name: "FastDB-0.2.0-macos-arm64.dmg".into(),
                browser_download_url: "https://example.com/dmg".into(),
                size: 45_000_000,
            },
            ReleaseAsset {
                name: "fastdb_0.2.0_amd64.deb".into(),
                browser_download_url: "https://example.com/deb".into(),
                size: 30_000_000,
            },
        ];

        let matched = find_matching_asset(&assets);
        assert!(matched.is_some());
        #[cfg(target_os = "macos")]
        assert_eq!(matched.unwrap().name, "FastDB-0.2.0-macos-arm64.dmg");
        #[cfg(target_os = "linux")]
        assert_eq!(matched.unwrap().name, "fastdb_0.2.0_amd64.deb");
    }

    #[test]
    fn test_parse_github_release_dto() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "name": "FastDB v0.2.0",
            "body": "- Added auto update\n- Improved grid performance",
            "html_url": "https://github.com/Matrix-I/fastdb/releases/tag/v0.2.0",
            "published_at": "2026-10-26T12:00:00Z",
            "assets": [
                {
                    "name": "FastDB-0.2.0-arm64.dmg",
                    "browser_download_url": "https://github.com/Matrix-I/fastdb/releases/download/v0.2.0/FastDB-0.2.0-arm64.dmg",
                    "size": 42000000
                }
            ]
        }"#;

        let dto: GithubReleaseDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.tag_name, "v0.2.0");
        assert_eq!(dto.assets.len(), 1);
        assert_eq!(dto.assets[0].name, "FastDB-0.2.0-arm64.dmg");
    }
}
