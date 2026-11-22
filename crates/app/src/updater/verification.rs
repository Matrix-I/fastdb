use super::{DownloadProgressEvent, ReleaseAsset};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MANIFEST: &str = "SHA256SUMS.txt";
const SIGNATURE: &str = "SHA256SUMS.txt.minisig";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const SIGNATURE_LIMIT: u64 = 16 * 1024;
const PACKAGE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct VerifiedInstaller {
    path: PathBuf,
    checksum: [u8; 32],
}

impl VerifiedInstaller {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Recheck immediately before every handoff, including a fallback after a failed native action.
    pub(super) fn with_verified_file<T>(
        &self,
        action: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<T> {
        self.verify_current_file().map_err(VerificationFailure)?;
        action(&self.path)
    }

    fn verify_current_file(&self) -> Result<()> {
        let metadata =
            fs::symlink_metadata(&self.path).context("Downloaded update is unavailable")?;
        ensure!(
            metadata.is_file(),
            "Downloaded update is not a regular file"
        );
        ensure!(
            metadata.len() <= PACKAGE_LIMIT,
            "Downloaded update exceeds the size limit"
        );
        let file = fs::File::open(&self.path).context("Cannot open downloaded update")?;
        let mut hash = Sha256::new();
        let bytes = std::io::copy(&mut file.take(PACKAGE_LIMIT + 1), &mut hash)?;
        ensure!(
            bytes <= PACKAGE_LIMIT,
            "Downloaded update exceeds the size limit"
        );
        ensure!(
            <[u8; 32]>::from(hash.finalize()) == self.checksum,
            "Downloaded update checksum mismatch"
        );
        Ok(())
    }
}

#[derive(Debug)]
pub struct VerificationFailure(anyhow::Error);

impl std::fmt::Display for VerificationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Update verification failed: {:#}", self.0)
    }
}
impl std::error::Error for VerificationFailure {}

pub(super) fn validate_asset_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', ':'])
            && !name.chars().any(char::is_control),
        "Invalid update asset filename"
    );
    Ok(())
}

pub(super) fn release_metadata_asset<'a>(
    assets: &'a [ReleaseAsset],
    name: &str,
) -> Result<&'a ReleaseAsset> {
    let mut matches = assets.iter().filter(|a| a.name == name);
    let asset = matches
        .next()
        .with_context(|| format!("Release is missing {name}"))?;
    ensure!(matches.next().is_none(), "Duplicate release asset {name}");
    Ok(asset)
}

pub(super) fn parse_asset_checksum(manifest: &[u8], name: &str) -> Result<[u8; 32]> {
    validate_asset_name(name)?;
    let mut checksum = None;
    for line in std::str::from_utf8(manifest)
        .context("Invalid checksum manifest encoding")?
        .lines()
    {
        let (digest, entry) = line
            .split_once(' ')
            .context("Invalid checksum manifest entry")?;
        let entry = entry
            .strip_prefix([' ', '*'])
            .context("Invalid checksum manifest marker")?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid SHA-256 checksum"
        );
        if entry == name {
            ensure!(checksum.is_none(), "Duplicate checksum for {name}");
            let mut bytes = [0; 32];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16)?;
            }
            checksum = Some(bytes);
        }
    }
    checksum.with_context(|| format!("No checksum for {name} in the signed manifest"))
}

async fn request(
    client: &reqwest::Client,
    asset: &ReleaseAsset,
    limit: u64,
    timeout: Duration,
) -> Result<reqwest::Response> {
    ensure!(asset.size <= limit, "{} exceeds the size limit", asset.name);
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "FastDB-Updater")
        .timeout(timeout)
        .send()
        .await
        .with_context(|| format!("Failed to download {}", asset.name))?;
    ensure!(
        response.status().is_success(),
        "Download of {} returned HTTP {}",
        asset.name,
        response.status()
    );
    ensure!(
        response.content_length().unwrap_or(0) <= limit,
        "{} exceeds the size limit",
        asset.name
    );
    Ok(response)
}

async fn metadata(client: &reqwest::Client, asset: &ReleaseAsset, limit: u64) -> Result<Vec<u8>> {
    let name = &asset.name;
    let mut response = request(client, asset, limit, Duration::from_secs(30)).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("Failed reading {name}"))?
    {
        ensure!(
            (bytes.len() + chunk.len()) as u64 <= limit,
            "{name} exceeds the size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) async fn download(
    client: reqwest::Client,
    assets: Vec<ReleaseAsset>,
    asset: ReleaseAsset,
    dir: PathBuf,
    sender: flume::Sender<DownloadProgressEvent>,
    trusted_key: &str,
) -> Result<VerifiedInstaller> {
    validate_asset_name(&asset.name)?;
    ensure!(
        release_metadata_asset(&assets, &asset.name)? == &asset,
        "Selected package does not match release asset"
    );
    let manifest_asset = release_metadata_asset(&assets, MANIFEST)?;
    let signature_asset = release_metadata_asset(&assets, SIGNATURE)?;
    let manifest = metadata(&client, manifest_asset, MANIFEST_LIMIT).await?;
    let signature = metadata(&client, signature_asset, SIGNATURE_LIMIT).await?;
    let key = minisign_verify::PublicKey::decode(trusted_key)
        .context("Invalid update verification key")?;
    let signature =
        std::str::from_utf8(&signature).context("Invalid update manifest signature encoding")?;
    let signature = minisign_verify::Signature::decode(signature)
        .context("Invalid update manifest signature")?;
    key.verify(&manifest, &signature, false)
        .context("Update manifest signature verification failed")?;
    let checksum = parse_asset_checksum(&manifest, &asset.name)?;
    let mut response = request(&client, &asset, PACKAGE_LIMIT, Duration::from_secs(600)).await?;
    let total = response.content_length();
    fs::create_dir_all(&dir).context("Failed to create download directory")?;
    let mut stage = tempfile::NamedTempFile::new_in(&dir).context("Failed to stage update")?;
    let mut writer = BufWriter::new(stage.as_file_mut());
    let mut hash = Sha256::new();
    let mut downloaded = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Failed reading update package")?
    {
        downloaded += chunk.len() as u64;
        ensure!(
            downloaded <= PACKAGE_LIMIT,
            "Update package exceeds the size limit"
        );
        writer
            .write_all(&chunk)
            .context("Failed writing update package")?;
        hash.update(&chunk);
        // The next event carries cumulative progress, so a busy UI can skip intermediate samples.
        let _ = sender.try_send(DownloadProgressEvent::Progress { downloaded, total });
    }
    writer.flush().context("Failed to flush update package")?;
    drop(writer);
    ensure!(
        <[u8; 32]>::from(hash.finalize()) == checksum,
        "Update package checksum mismatch"
    );
    let path = dir.join(asset.name);
    stage
        .persist(&path)
        .context("Failed to publish verified update")?;
    Ok(VerifiedInstaller { path, checksum })
}
