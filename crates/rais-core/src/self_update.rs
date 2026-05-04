use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::error::{IoPathContext, RaisError};
use crate::hash::sha256_file;
use crate::model::Platform;
use crate::signature::{SignatureVerdict, verify_executable_signature};
use crate::version::Version;

const ROLLBACK_SUFFIX: &str = "rais-old";

const USER_AGENT: &str = concat!(
    "RAIS/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Timtam/rais)"
);

pub const DEFAULT_SELF_UPDATE_MANIFEST_URL: &str =
    "https://github.com/Timtam/rais/releases/latest/download/rais-update-stable.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateManifest {
    pub version: Version,
    pub channel: String,
    pub published_at: String,
    pub release_notes_url: Option<String>,
    pub minimum_supported_previous_version: Option<Version>,
    pub assets: SelfUpdateAssets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateAssets {
    pub windows: Option<SelfUpdateAsset>,
    pub macos: Option<SelfUpdateAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateAsset {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateAssetSelection {
    pub platform: Platform,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateCheckReport {
    pub manifest_url: String,
    pub current_version: Version,
    pub latest_version: Version,
    pub channel: String,
    pub published_at: String,
    pub release_notes_url: Option<String>,
    pub minimum_supported_previous_version: Option<Version>,
    pub update_available: bool,
    pub requires_manual_transition: bool,
    pub asset: SelfUpdateAssetSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateStageReport {
    pub check: SelfUpdateCheckReport,
    pub staging_dir: PathBuf,
    pub staged_asset_path: Option<PathBuf>,
    pub downloaded: bool,
    pub reused_existing_file: bool,
    pub verified_sha256: Option<String>,
    pub ready_to_apply: bool,
    pub status_message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ApplySelfUpdateOptions {
    /// Override the directory the swap operates in. Defaults to the parent
    /// of `current_exe()` (`current_install_root`).
    pub install_root: Option<PathBuf>,
    /// Override the install target's filename. Defaults to the basename of
    /// `current_exe()`. The new artifact filename
    /// (`rais-<version>-<os>-<arch>[.exe]`) does not have to match the
    /// install target — RAIS swaps in place under the user's existing
    /// filename regardless of what the download was called.
    pub install_target_basename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateApplyReport {
    pub stage: SelfUpdateStageReport,
    pub install_root: PathBuf,
    pub replaced_files: Vec<ReplacedFile>,
    pub skipped_files: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signature_verdicts: Vec<SignatureVerdictRecord>,
    pub status_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureVerdictRecord {
    pub source_path: PathBuf,
    pub verdict: SignatureVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacedFile {
    pub install_path: PathBuf,
    pub backup_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RawSelfUpdateManifest {
    version: String,
    channel: String,
    published_at: String,
    release_notes_url: Option<String>,
    minimum_supported_previous_version: Option<String>,
    assets: RawSelfUpdateAssets,
}

#[derive(Debug, Deserialize)]
struct RawSelfUpdateAssets {
    windows: Option<RawSelfUpdateAsset>,
    macos: Option<RawSelfUpdateAsset>,
}

#[derive(Debug, Deserialize)]
struct RawSelfUpdateAsset {
    url: String,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

pub fn current_rais_version() -> Result<Version> {
    parse_semantic_version(
        env!("CARGO_PKG_VERSION"),
        "build-metadata",
        "current_version",
    )
}

/// Ephemeral staging directory for the self-update download. Lives under
/// the OS temp dir (cleaned periodically by the OS) so RAIS doesn't leave
/// persistent files in `%LOCALAPPDATA%` / `~/Library/Caches/`. Callers
/// generally don't need to keep this around between runs — the download is
/// validated, swapped in place, and the staging dir is removed at the end
/// of `apply_self_update`.
pub fn default_self_update_staging_dir() -> PathBuf {
    env::temp_dir().join("rais-self-update")
}

pub fn fetch_self_update_manifest(manifest_url: &str) -> Result<SelfUpdateManifest> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|source| RaisError::Http {
            url: "client-builder".to_string(),
            source,
        })?;

    let body = client
        .get(manifest_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RaisError::Http {
            url: manifest_url.to_string(),
            source,
        })?
        .text()
        .map_err(|source| RaisError::Http {
            url: manifest_url.to_string(),
            source,
        })?;

    parse_self_update_manifest(&body, manifest_url)
}

pub fn parse_self_update_manifest(body: &str, manifest_url: &str) -> Result<SelfUpdateManifest> {
    let raw: RawSelfUpdateManifest =
        serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
            url: manifest_url.to_string(),
            message: source.to_string(),
        })?;

    let version = parse_semantic_version(&raw.version, manifest_url, "version")?;
    let minimum_supported_previous_version = raw
        .minimum_supported_previous_version
        .as_deref()
        .map(|value| {
            parse_semantic_version(value, manifest_url, "minimum_supported_previous_version")
        })
        .transpose()?;
    let assets = SelfUpdateAssets {
        windows: raw
            .assets
            .windows
            .as_ref()
            .map(|asset| parse_asset(asset, manifest_url, "windows"))
            .transpose()?,
        macos: raw
            .assets
            .macos
            .as_ref()
            .map(|asset| parse_asset(asset, manifest_url, "macos"))
            .transpose()?,
    };

    Ok(SelfUpdateManifest {
        version,
        channel: raw.channel,
        published_at: raw.published_at,
        release_notes_url: raw.release_notes_url,
        minimum_supported_previous_version,
        assets,
    })
}

pub fn check_self_update(platform: Platform, manifest_url: &str) -> Result<SelfUpdateCheckReport> {
    let manifest = fetch_self_update_manifest(manifest_url)?;
    evaluate_self_update_report(platform, manifest_url, current_rais_version()?, &manifest)
}

pub fn stage_self_update(
    platform: Platform,
    manifest_url: &str,
    staging_dir: &Path,
) -> Result<SelfUpdateStageReport> {
    let report = check_self_update(platform, manifest_url)?;
    stage_self_update_from_report(&report, staging_dir)
}

pub fn relaunch_current_executable() -> Result<u32> {
    let exe = env::current_exe().map_err(|source| RaisError::Io {
        path: PathBuf::from("current_exe"),
        source,
    })?;
    let child = std::process::Command::new(&exe)
        .spawn()
        .map_err(|source| RaisError::Io { path: exe, source })?;
    Ok(child.id())
}

pub fn current_install_root() -> Result<PathBuf> {
    let exe = env::current_exe().map_err(|source| RaisError::Io {
        path: PathBuf::from("current_exe"),
        source,
    })?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| RaisError::InvalidPlannedExecution {
            message: format!(
                "current executable {} has no parent directory",
                exe.display()
            ),
        })
}

pub fn apply_self_update(
    stage: &SelfUpdateStageReport,
    options: &ApplySelfUpdateOptions,
) -> Result<SelfUpdateApplyReport> {
    if !stage.ready_to_apply {
        return Err(RaisError::InvalidPlannedExecution {
            message: format!(
                "self-update is not ready to apply: {}",
                stage.status_message
            ),
        });
    }

    // (Old behavior: refuse to apply while *any* package install was
    // running, via a global LocalAppData lock. The lock is now per-target
    // — RAIS doesn't have a single resource path to ask about during
    // self-update — so the cross-target check is gone. Two concurrent
    // self-updates would race the file rename below, which is rare
    // enough that we let it surface as a normal IO error rather than
    // adding a separate global mutex.)

    let staged_asset =
        stage
            .staged_asset_path
            .as_ref()
            .ok_or_else(|| RaisError::InvalidPlannedExecution {
                message: "self-update apply requires a staged asset path".to_string(),
            })?;

    let observed_sha256 = sha256_file(staged_asset)?;
    if observed_sha256 != stage.check.asset.sha256 {
        return Err(RaisError::HashMismatch {
            path: staged_asset.clone(),
            expected: stage.check.asset.sha256.clone(),
            actual: observed_sha256,
        });
    }

    let install_target = resolve_install_target(options)?;
    let install_root = install_target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| match options.install_root.clone() {
            Some(root) => root,
            None => PathBuf::new(),
        });

    // The release pipeline publishes the bare RAIS executable as a
    // single-file asset, so the staged file *is* the new binary — no zip
    // flat-extract step. The download's filename may differ from the
    // install target (e.g. `rais-0.2.0-windows-x86_64.exe` vs. `RAIS.exe`);
    // the swap copies bytes regardless of either name.
    let signature_verdicts = match verify_replacement_signature(staged_asset, &install_target)? {
        Some(record) => vec![record],
        None => Vec::new(),
    };

    let mut replaced = Vec::new();
    let mut skipped = Vec::new();
    if let Err(error) =
        swap_install_file(staged_asset, &install_target, &mut replaced, &mut skipped)
    {
        rollback_replaced_files(&replaced);
        return Err(error);
    }

    let signed_count = signature_verdicts
        .iter()
        .filter(|record| matches!(record.verdict, SignatureVerdict::Signed { .. }))
        .count();
    let status_message = if replaced.is_empty() {
        "Self-update did not match any binary in the install directory.".to_string()
    } else if signed_count > 0 {
        format!(
            "Replaced {} file(s) with RAIS {} ({} signed); rollback copies retained as .{}.",
            replaced.len(),
            stage.check.latest_version,
            signed_count,
            ROLLBACK_SUFFIX
        )
    } else {
        format!(
            "Replaced {} file(s) with RAIS {}; rollback copies retained as .{}.",
            replaced.len(),
            stage.check.latest_version,
            ROLLBACK_SUFFIX
        )
    };

    Ok(SelfUpdateApplyReport {
        stage: stage.clone(),
        install_root,
        replaced_files: replaced,
        skipped_files: skipped,
        signature_verdicts,
        status_message,
    })
}

fn resolve_install_target(options: &ApplySelfUpdateOptions) -> Result<PathBuf> {
    if options.install_root.is_none() && options.install_target_basename.is_none() {
        return env::current_exe().map_err(|source| RaisError::Io {
            path: PathBuf::from("current_exe"),
            source,
        });
    }
    let root = match &options.install_root {
        Some(root) => root.clone(),
        None => current_install_root()?,
    };
    let basename = match options.install_target_basename.clone() {
        Some(name) => name,
        None => current_exe_basename()?,
    };
    Ok(root.join(basename))
}

fn current_exe_basename() -> Result<String> {
    let exe = env::current_exe().map_err(|source| RaisError::Io {
        path: PathBuf::from("current_exe"),
        source,
    })?;
    exe.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .ok_or_else(|| RaisError::InvalidPlannedExecution {
            message: "current executable has no file name".to_string(),
        })
}

fn verify_replacement_signature(
    source: &Path,
    install_target: &Path,
) -> Result<Option<SignatureVerdictRecord>> {
    if !install_target.is_file() {
        return Ok(None);
    }
    let verdict = verify_executable_signature(source)?;
    if let SignatureVerdict::Invalid { reason } = &verdict {
        return Err(RaisError::SelfUpdateSignatureInvalid {
            path: source.to_path_buf(),
            reason: reason.clone(),
        });
    }
    Ok(Some(SignatureVerdictRecord {
        source_path: source.to_path_buf(),
        verdict,
    }))
}

fn swap_install_file(
    source: &Path,
    install_target: &Path,
    replaced: &mut Vec<ReplacedFile>,
    skipped: &mut Vec<PathBuf>,
) -> Result<()> {
    if !install_target.is_file() {
        skipped.push(install_target.to_path_buf());
        return Ok(());
    }
    let backup_path = backup_path_for(install_target);
    if backup_path.exists() {
        fs::remove_file(&backup_path).with_path(&backup_path)?;
    }
    fs::rename(install_target, &backup_path).with_path(install_target)?;
    if let Err(error) = fs::copy(source, install_target) {
        let _ = fs::rename(&backup_path, install_target);
        return Err(RaisError::Io {
            path: install_target.to_path_buf(),
            source: error,
        });
    }
    clear_macos_quarantine(install_target);
    replaced.push(ReplacedFile {
        install_path: install_target.to_path_buf(),
        backup_path,
    });
    Ok(())
}

/// Strip the `com.apple.quarantine` extended attribute from the freshly
/// swapped binary. Some macOS configurations re-quarantine files written by
/// processes whose own binary still carries the attribute; clearing it here
/// keeps post-update launches from re-triggering Gatekeeper. Failure is
/// ignored — the attribute may simply not be present.
#[cfg(target_os = "macos")]
fn clear_macos_quarantine(path: &Path) {
    let _ = std::process::Command::new("xattr")
        .arg("-d")
        .arg("com.apple.quarantine")
        .arg(path)
        .status();
}

#[cfg(not(target_os = "macos"))]
fn clear_macos_quarantine(_path: &Path) {}

fn rollback_replaced_files(replaced: &[ReplacedFile]) {
    for entry in replaced.iter().rev() {
        let _ = fs::remove_file(&entry.install_path);
        let _ = fs::rename(&entry.backup_path, &entry.install_path);
    }
}

fn backup_path_for(install_path: &Path) -> PathBuf {
    let file_name = install_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rais-target");
    install_path.with_file_name(format!("{file_name}.{ROLLBACK_SUFFIX}"))
}

fn evaluate_self_update_report(
    platform: Platform,
    manifest_url: &str,
    current_version: Version,
    manifest: &SelfUpdateManifest,
) -> Result<SelfUpdateCheckReport> {
    let current_semver =
        semantic_version_from_version(&current_version, manifest_url, "current_version")?;
    let latest_semver = semantic_version_from_version(&manifest.version, manifest_url, "version")?;
    let minimum_supported_previous_version = manifest.minimum_supported_previous_version.clone();
    let requires_manual_transition = minimum_supported_previous_version
        .as_ref()
        .map(|minimum| {
            semantic_version_from_version(
                minimum,
                manifest_url,
                "minimum_supported_previous_version",
            )
            .map(|minimum| current_semver < minimum)
        })
        .transpose()?
        .unwrap_or(false);

    Ok(SelfUpdateCheckReport {
        manifest_url: manifest_url.to_string(),
        current_version,
        latest_version: manifest.version.clone(),
        channel: manifest.channel.clone(),
        published_at: manifest.published_at.clone(),
        release_notes_url: manifest.release_notes_url.clone(),
        minimum_supported_previous_version,
        update_available: latest_semver > current_semver,
        requires_manual_transition,
        asset: select_asset_for_platform(platform, manifest, manifest_url)?,
    })
}

fn stage_self_update_from_report(
    report: &SelfUpdateCheckReport,
    staging_dir: &Path,
) -> Result<SelfUpdateStageReport> {
    if !report.update_available {
        return Ok(SelfUpdateStageReport {
            check: report.clone(),
            staging_dir: staging_dir.to_path_buf(),
            staged_asset_path: None,
            downloaded: false,
            reused_existing_file: false,
            verified_sha256: None,
            ready_to_apply: false,
            status_message: "Current RAIS version is already up to date.".to_string(),
        });
    }

    if report.requires_manual_transition {
        return Ok(SelfUpdateStageReport {
            check: report.clone(),
            staging_dir: staging_dir.to_path_buf(),
            staged_asset_path: None,
            downloaded: false,
            reused_existing_file: false,
            verified_sha256: None,
            ready_to_apply: false,
            status_message:
                "This RAIS update requires a manual transition before staging can continue."
                    .to_string(),
        });
    }

    let (file_name, local_source_path) = resolve_update_asset_source(&report.asset.url)?;
    let version_dir = staging_dir.join(report.latest_version.raw());
    fs::create_dir_all(&version_dir).with_path(&version_dir)?;

    let target_path = version_dir.join(file_name);
    if target_path.is_file() {
        let existing_sha256 = sha256_file(&target_path)?;
        if existing_sha256 == report.asset.sha256 {
            return Ok(SelfUpdateStageReport {
                check: report.clone(),
                staging_dir: staging_dir.to_path_buf(),
                staged_asset_path: Some(target_path),
                downloaded: false,
                reused_existing_file: true,
                verified_sha256: Some(existing_sha256),
                ready_to_apply: true,
                status_message: format!(
                    "Verified existing staged RAIS update {}.",
                    report.latest_version
                ),
            });
        }

        fs::remove_file(&target_path).with_path(&target_path)?;
    }

    download_self_update_asset(
        &report.asset.url,
        local_source_path.as_deref(),
        &target_path,
    )?;
    let verified_sha256 = sha256_file(&target_path)?;
    if verified_sha256 != report.asset.sha256 {
        let _ = fs::remove_file(&target_path);
        return Err(RaisError::HashMismatch {
            path: target_path,
            expected: report.asset.sha256.clone(),
            actual: verified_sha256,
        });
    }

    Ok(SelfUpdateStageReport {
        check: report.clone(),
        staging_dir: staging_dir.to_path_buf(),
        staged_asset_path: Some(target_path),
        downloaded: true,
        reused_existing_file: false,
        verified_sha256: Some(report.asset.sha256.clone()),
        ready_to_apply: true,
        status_message: format!(
            "Downloaded and verified staged RAIS update {}.",
            report.latest_version
        ),
    })
}

fn select_asset_for_platform(
    platform: Platform,
    manifest: &SelfUpdateManifest,
    manifest_url: &str,
) -> Result<SelfUpdateAssetSelection> {
    let asset = match platform {
        Platform::Windows => manifest.assets.windows.as_ref(),
        Platform::MacOs => manifest.assets.macos.as_ref(),
    }
    .ok_or_else(|| RaisError::RemoteData {
        url: manifest_url.to_string(),
        message: format!("missing asset entry for platform {platform:?}"),
    })?;

    Ok(SelfUpdateAssetSelection {
        platform,
        url: asset.url.clone(),
        sha256: asset.sha256.clone(),
    })
}

fn parse_asset(
    asset: &RawSelfUpdateAsset,
    manifest_url: &str,
    field: &str,
) -> Result<SelfUpdateAsset> {
    if !asset.url.starts_with("https://") {
        return Err(RaisError::RemoteData {
            url: manifest_url.to_string(),
            message: format!("{field} asset url must use https: {}", asset.url),
        });
    }
    if !is_valid_sha256(&asset.sha256) {
        return Err(RaisError::RemoteData {
            url: manifest_url.to_string(),
            message: format!("{field} asset sha256 must be 64 lowercase hexadecimal characters"),
        });
    }

    Ok(SelfUpdateAsset {
        url: asset.url.clone(),
        sha256: asset.sha256.clone(),
    })
}

fn download_self_update_asset(
    url: &str,
    local_source_path: Option<&Path>,
    target_path: &Path,
) -> Result<()> {
    let part_path = target_path.with_extension(format!(
        "{}.part",
        target_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ));

    if let Some(source_path) = local_source_path {
        fs::copy(source_path, &part_path).with_path(source_path)?;
        fs::rename(&part_path, target_path).with_path(target_path)?;
        return Ok(());
    }

    validate_remote_self_update_url(url)?;
    let client = http_client()?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RaisError::Http {
            url: url.to_string(),
            source,
        })?;
    let mut file = fs::File::create(&part_path).with_path(&part_path)?;
    std::io::copy(&mut response, &mut file).with_path(&part_path)?;
    file.flush().with_path(&part_path)?;
    drop(file);

    fs::rename(&part_path, target_path).with_path(target_path)?;
    Ok(())
}

fn resolve_update_asset_source(url_or_path: &str) -> Result<(String, Option<PathBuf>)> {
    if let Some(path) = local_update_asset_source_path(url_or_path)? {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| RaisError::RemoteData {
                url: url_or_path.to_string(),
                message: "self-update asset path does not contain a file name".to_string(),
            })?;
        return Ok((file_name.to_string(), Some(path)));
    }

    validate_remote_self_update_url(url_or_path)?;
    let file_name = file_name_from_url(url_or_path).ok_or_else(|| RaisError::RemoteData {
        url: url_or_path.to_string(),
        message: "self-update asset URL does not contain a file name".to_string(),
    })?;
    Ok((file_name, None))
}

fn local_update_asset_source_path(url_or_path: &str) -> Result<Option<PathBuf>> {
    let path = PathBuf::from(url_or_path);
    if path.is_file() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

fn validate_remote_self_update_url(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(RaisError::InvalidArtifactUrl {
            url: url.to_string(),
            message: "self-update downloads must use HTTPS".to_string(),
        })
    }
}

fn file_name_from_url(url: &str) -> Option<String> {
    let without_query = url.split_once('?').map_or(url, |(path, _query)| path);
    without_query
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|source| RaisError::Http {
            url: "client-builder".to_string(),
            source,
        })
}

fn parse_semantic_version(raw: &str, url: &str, field: &str) -> Result<Version> {
    semantic_version_from_str(raw, url, field)?;
    Version::parse(raw)
}

fn semantic_version_from_version(
    version: &Version,
    url: &str,
    field: &str,
) -> Result<SemanticVersion> {
    semantic_version_from_str(version.raw(), url, field)
}

fn semantic_version_from_str(raw: &str, url: &str, field: &str) -> Result<SemanticVersion> {
    let trimmed = raw.trim();
    let parts = trimmed.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: format!("{field} must use semantic versioning (major.minor.patch): {trimmed}"),
        });
    }

    let parse_part = |name: &str, value: &str| {
        value.parse::<u64>().map_err(|_| RaisError::RemoteData {
            url: url.to_string(),
            message: format!("{field} contains a non-numeric {name} segment: {trimmed}"),
        })
    };

    Ok(SemanticVersion {
        major: parse_part("major", parts[0])?,
        minor: parse_part("minor", parts[1])?,
        patch: parse_part("patch", parts[2])?,
    })
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        ApplySelfUpdateOptions, SelfUpdateAssetSelection, SelfUpdateCheckReport,
        SelfUpdateManifest, SelfUpdateStageReport, apply_self_update, current_rais_version,
        evaluate_self_update_report, parse_self_update_manifest, stage_self_update_from_report,
    };
    use crate::RaisError;
    use crate::hash::sha256_file;
    use crate::model::Platform;
    use crate::version::Version;

    const MANIFEST_URL: &str = "https://example.test/rais-update-stable.json";

    #[test]
    fn parses_valid_self_update_manifest() {
        let manifest = parse_self_update_manifest(
            r#"{
              "version": "0.2.0",
              "channel": "stable",
              "published_at": "2026-04-25T00:00:00Z",
              "release_notes_url": "https://example.test/releases/v0.2.0",
              "minimum_supported_previous_version": "0.1.0",
              "assets": {
                "windows": {
                  "url": "https://example.test/RAIS-windows.zip",
                  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                },
                "macos": {
                  "url": "https://example.test/RAIS-macos.zip",
                  "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                }
              }
            }"#,
            MANIFEST_URL,
        )
        .unwrap();

        assert_eq!(manifest.version.raw(), "0.2.0");
        assert_eq!(manifest.channel, "stable");
        assert_eq!(
            manifest
                .minimum_supported_previous_version
                .as_ref()
                .unwrap()
                .raw(),
            "0.1.0"
        );
    }

    #[test]
    fn rejects_non_semantic_manifest_version() {
        let error = parse_self_update_manifest(
            r#"{
              "version": "0.2",
              "channel": "stable",
              "published_at": "2026-04-25T00:00:00Z",
              "assets": {
                "windows": {
                  "url": "https://example.test/RAIS-windows.zip",
                  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                }
              }
            }"#,
            MANIFEST_URL,
        )
        .unwrap_err();

        assert!(error.to_string().contains("semantic versioning"));
    }

    #[test]
    fn rejects_non_https_asset_url() {
        let error = parse_self_update_manifest(
            r#"{
              "version": "0.2.0",
              "channel": "stable",
              "published_at": "2026-04-25T00:00:00Z",
              "assets": {
                "windows": {
                  "url": "http://example.test/RAIS-windows.zip",
                  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                }
              }
            }"#,
            MANIFEST_URL,
        )
        .unwrap_err();

        assert!(error.to_string().contains("must use https"));
    }

    #[test]
    fn reports_update_available_for_newer_version() {
        let manifest = sample_manifest();

        let report = evaluate_self_update_report(
            Platform::Windows,
            MANIFEST_URL,
            Version::parse("0.1.0").unwrap(),
            &manifest,
        )
        .unwrap();

        assert!(report.update_available);
        assert!(!report.requires_manual_transition);
        assert_eq!(report.asset.platform, Platform::Windows);
        assert!(report.asset.url.contains("RAIS-windows.zip"));
    }

    #[test]
    fn reports_manual_transition_requirement() {
        let manifest = sample_manifest();

        let report = evaluate_self_update_report(
            Platform::Windows,
            MANIFEST_URL,
            Version::parse("0.0.9").unwrap(),
            &manifest,
        )
        .unwrap();

        assert!(report.update_available);
        assert!(report.requires_manual_transition);
    }

    #[test]
    fn current_build_version_is_semantic() {
        let version = current_rais_version().unwrap();

        assert_eq!(version.raw(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn stages_update_from_local_asset_and_verifies_hash() {
        let source_dir = tempdir().unwrap();
        let staging_dir = tempdir().unwrap();
        let asset_path = source_dir.path().join("RAIS-windows.zip");
        fs::write(&asset_path, b"rais-update").unwrap();
        let expected_sha256 = sha256_file(&asset_path).unwrap();

        let report = stage_self_update_from_report(
            &sample_check_report(asset_path.display().to_string(), &expected_sha256),
            staging_dir.path(),
        )
        .unwrap();

        assert!(report.downloaded);
        assert!(!report.reused_existing_file);
        assert!(report.ready_to_apply);
        assert_eq!(
            report.staged_asset_path.as_ref().unwrap(),
            &staging_dir.path().join("0.2.0").join("RAIS-windows.zip")
        );
        assert_eq!(
            report.verified_sha256.as_deref(),
            Some(expected_sha256.as_str())
        );
    }

    #[test]
    fn reuses_existing_staged_update_when_hash_matches() {
        let source_dir = tempdir().unwrap();
        let staging_dir = tempdir().unwrap();
        let asset_path = source_dir.path().join("RAIS-windows.zip");
        fs::write(&asset_path, b"rais-update").unwrap();
        let expected_sha256 = sha256_file(&asset_path).unwrap();
        let check = sample_check_report(asset_path.display().to_string(), &expected_sha256);

        let first = stage_self_update_from_report(&check, staging_dir.path()).unwrap();
        let second = stage_self_update_from_report(&check, staging_dir.path()).unwrap();

        assert!(first.downloaded);
        assert!(!first.reused_existing_file);
        assert!(second.reused_existing_file);
        assert!(!second.downloaded);
        assert!(second.ready_to_apply);
    }

    #[test]
    fn does_not_stage_when_current_version_is_already_latest() {
        let staging_dir = tempdir().unwrap();
        let mut check = sample_check_report(
            "https://example.test/RAIS-windows.zip".to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        check.update_available = false;

        let report = stage_self_update_from_report(&check, staging_dir.path()).unwrap();

        assert!(!report.ready_to_apply);
        assert!(report.staged_asset_path.is_none());
        assert!(report.status_message.contains("up to date"));
    }

    #[test]
    fn removes_bad_staged_file_when_hash_mismatch_is_detected() {
        let source_dir = tempdir().unwrap();
        let staging_dir = tempdir().unwrap();
        let asset_path = source_dir.path().join("RAIS-windows.zip");
        fs::write(&asset_path, b"rais-update").unwrap();

        let error = stage_self_update_from_report(
            &sample_check_report(
                asset_path.display().to_string(),
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
            staging_dir.path(),
        )
        .unwrap_err();

        let staged_path = staging_dir.path().join("0.2.0").join("RAIS-windows.zip");
        assert!(matches!(error, RaisError::HashMismatch { .. }));
        assert!(!staged_path.exists());
    }

    fn sample_manifest() -> SelfUpdateManifest {
        parse_self_update_manifest(
            r#"{
              "version": "0.2.0",
              "channel": "stable",
              "published_at": "2026-04-25T00:00:00Z",
              "release_notes_url": "https://example.test/releases/v0.2.0",
              "minimum_supported_previous_version": "0.1.0",
              "assets": {
                "windows": {
                  "url": "https://example.test/RAIS-windows.zip",
                  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                },
                "macos": {
                  "url": "https://example.test/RAIS-macos.zip",
                  "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                }
              }
            }"#,
            MANIFEST_URL,
        )
        .unwrap()
    }

    fn sample_check_report(url: String, sha256: &str) -> SelfUpdateCheckReport {
        SelfUpdateCheckReport {
            manifest_url: MANIFEST_URL.to_string(),
            current_version: Version::parse("0.1.0").unwrap(),
            latest_version: Version::parse("0.2.0").unwrap(),
            channel: "stable".to_string(),
            published_at: "2026-04-25T00:00:00Z".to_string(),
            release_notes_url: Some("https://example.test/releases/v0.2.0".to_string()),
            minimum_supported_previous_version: Some(Version::parse("0.1.0").unwrap()),
            update_available: true,
            requires_manual_transition: false,
            asset: SelfUpdateAssetSelection {
                platform: Platform::Windows,
                url,
                sha256: sha256.to_string(),
            },
        }
    }

    fn write_test_release_binary(path: &std::path::Path, contents: &[u8]) {
        fs::write(path, contents).unwrap();
    }

    fn staged_report_for_binary(
        binary_path: &std::path::Path,
        staging_dir: &std::path::Path,
    ) -> SelfUpdateStageReport {
        let binary_sha = sha256_file(binary_path).unwrap();
        let mut check = sample_check_report(binary_path.display().to_string(), &binary_sha);
        check.asset.url = binary_path.display().to_string();
        SelfUpdateStageReport {
            check,
            staging_dir: staging_dir.to_path_buf(),
            staged_asset_path: Some(binary_path.to_path_buf()),
            downloaded: true,
            reused_existing_file: false,
            verified_sha256: Some(binary_sha),
            ready_to_apply: true,
            status_message: "ready".to_string(),
        }
    }

    #[test]
    fn apply_self_update_replaces_install_file_using_versioned_source_name() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        // The staged source file follows the new versioned naming
        // (`rais-<version>-<os>-<arch>.exe`); the install target is
        // whatever the user named their binary on disk (`RAIS.exe`). The
        // swap should not require the two names to match.
        let staged_binary_path = staging_root
            .path()
            .join("0.2.0")
            .join("rais-0.2.0-windows-x86_64.exe");
        fs::create_dir_all(staged_binary_path.parent().unwrap()).unwrap();
        write_test_release_binary(&staged_binary_path, b"new-rais-binary");

        fs::write(install_root.path().join("RAIS.exe"), b"old-rais-binary").unwrap();

        let stage = staged_report_for_binary(&staged_binary_path, staging_root.path());
        let report = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                install_target_basename: Some("RAIS.exe".to_string()),
            },
        )
        .unwrap();

        assert_eq!(report.replaced_files.len(), 1);
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe")).unwrap(),
            b"new-rais-binary"
        );
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe.rais-old")).unwrap(),
            b"old-rais-binary"
        );
        assert!(report.skipped_files.is_empty());
    }

    #[test]
    fn apply_self_update_skips_missing_install_target_without_writing() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let staged_binary_path = staging_root
            .path()
            .join("0.2.0")
            .join("rais-0.2.0-macos-aarch64");
        fs::create_dir_all(staged_binary_path.parent().unwrap()).unwrap();
        write_test_release_binary(&staged_binary_path, b"new-mac-binary");

        // Install root does not contain a `RAIS` file yet — the swap step
        // should record it as skipped without creating one.
        let stage = staged_report_for_binary(&staged_binary_path, staging_root.path());
        let report = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                install_target_basename: Some("RAIS".to_string()),
            },
        )
        .unwrap();

        assert!(report.replaced_files.is_empty());
        assert!(
            report
                .skipped_files
                .iter()
                .any(|path| path.ends_with("RAIS"))
        );
        assert!(!install_root.path().join("RAIS").exists());
    }

    #[test]
    fn apply_self_update_rejects_hash_mismatch_without_touching_install() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let staged_binary_path = staging_root
            .path()
            .join("0.2.0")
            .join("rais-0.2.0-windows-x86_64.exe");
        fs::create_dir_all(staged_binary_path.parent().unwrap()).unwrap();
        write_test_release_binary(&staged_binary_path, b"new-rais-binary");

        fs::write(install_root.path().join("RAIS.exe"), b"old-rais-binary").unwrap();

        let mut stage = staged_report_for_binary(&staged_binary_path, staging_root.path());
        stage.check.asset.sha256 =
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();

        let error = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                install_target_basename: Some("RAIS.exe".to_string()),
            },
        )
        .unwrap_err();

        assert!(matches!(error, RaisError::HashMismatch { .. }));
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe")).unwrap(),
            b"old-rais-binary"
        );
        assert!(!install_root.path().join("RAIS.exe.rais-old").exists());
    }

    #[test]
    fn apply_self_update_rejects_when_stage_is_not_ready() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let mut stage = sample_check_report(
            "https://example.test/rais-0.2.0-windows-x86_64.exe".to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        stage.update_available = false;

        let stage_report = SelfUpdateStageReport {
            check: stage,
            staging_dir: staging_root.path().to_path_buf(),
            staged_asset_path: None,
            downloaded: false,
            reused_existing_file: false,
            verified_sha256: None,
            ready_to_apply: false,
            status_message: "Current RAIS version is already up to date.".to_string(),
        };

        let error = apply_self_update(
            &stage_report,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                install_target_basename: Some("RAIS.exe".to_string()),
            },
        )
        .unwrap_err();

        assert!(matches!(error, RaisError::InvalidPlannedExecution { .. }));
    }

    // (`apply_self_update_refuses_when_package_install_lock_is_held`
    // used to assert that a global package-install lock blocked the
    // self-update apply path. The lock is now per-target so the cross-
    // target check is gone — see `apply_self_update`'s comment for the
    // rationale.)
}
