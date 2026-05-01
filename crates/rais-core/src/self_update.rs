use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::archive::extract_all_files_flat;
use crate::error::{IoPathContext, RaisError};
use crate::hash::sha256_file;
use crate::lock::{default_package_install_lock_path, package_install_lock_active_at};
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
    pub install_root: Option<PathBuf>,
    pub package_install_lock_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfUpdateApplyReport {
    pub stage: SelfUpdateStageReport,
    pub install_root: PathBuf,
    pub extraction_dir: PathBuf,
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

pub fn default_self_update_staging_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local_app_data) = rais_platform::user_local_appdata_dir() {
            return local_app_data.join("RAIS").join("self-update");
        }
    }

    if cfg!(target_os = "macos") {
        if let Some(home) = rais_platform::user_home_dir() {
            return home
                .join("Library")
                .join("Caches")
                .join("RAIS")
                .join("self-update");
        }
    }

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

    let lock_path = options
        .package_install_lock_path
        .clone()
        .unwrap_or_else(default_package_install_lock_path);
    if let Some(holder) = package_install_lock_active_at(&lock_path)? {
        return Err(RaisError::PackageInstallInProgress {
            lock_path,
            pid: holder.pid,
        });
    }

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

    let install_root = match options.install_root.clone() {
        Some(root) => root,
        None => current_install_root()?,
    };
    let extraction_dir = staged_asset
        .parent()
        .map(|parent| parent.join("extracted"))
        .unwrap_or_else(|| stage.staging_dir.join("extracted"));
    if extraction_dir.exists() {
        fs::remove_dir_all(&extraction_dir).with_path(&extraction_dir)?;
    }
    fs::create_dir_all(&extraction_dir).with_path(&extraction_dir)?;

    let extracted_files = extract_all_files_flat(staged_asset, &extraction_dir)?;

    let signature_verdicts = match verify_replacement_signatures(&extracted_files, &install_root) {
        Ok(verdicts) => verdicts,
        Err(error) => {
            let _ = fs::remove_dir_all(&extraction_dir);
            return Err(error);
        }
    };

    let mut replaced = Vec::new();
    let mut skipped = Vec::new();
    if let Err(error) =
        swap_install_files(&extracted_files, &install_root, &mut replaced, &mut skipped)
    {
        rollback_replaced_files(&replaced);
        let _ = fs::remove_dir_all(&extraction_dir);
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
        extraction_dir,
        replaced_files: replaced,
        skipped_files: skipped,
        signature_verdicts,
        status_message,
    })
}

fn verify_replacement_signatures(
    extracted_files: &[PathBuf],
    install_root: &Path,
) -> Result<Vec<SignatureVerdictRecord>> {
    let mut verdicts = Vec::new();
    for source in extracted_files {
        let Some(basename) = source.file_name() else {
            continue;
        };
        let install_path = install_root.join(basename);
        if !install_path.is_file() {
            continue;
        }
        let verdict = verify_executable_signature(source)?;
        if let SignatureVerdict::Invalid { reason } = &verdict {
            return Err(RaisError::SelfUpdateSignatureInvalid {
                path: source.clone(),
                reason: reason.clone(),
            });
        }
        verdicts.push(SignatureVerdictRecord {
            source_path: source.clone(),
            verdict,
        });
    }
    Ok(verdicts)
}

fn swap_install_files(
    extracted_files: &[PathBuf],
    install_root: &Path,
    replaced: &mut Vec<ReplacedFile>,
    skipped: &mut Vec<PathBuf>,
) -> Result<()> {
    for source in extracted_files {
        let Some(basename) = source.file_name() else {
            continue;
        };
        let install_path = install_root.join(basename);
        if !install_path.is_file() {
            skipped.push(install_path);
            continue;
        }

        let backup_path = backup_path_for(&install_path);
        if backup_path.exists() {
            fs::remove_file(&backup_path).with_path(&backup_path)?;
        }
        fs::rename(&install_path, &backup_path).with_path(&install_path)?;
        if let Err(error) = fs::copy(source, &install_path) {
            let _ = fs::rename(&backup_path, &install_path);
            return Err(RaisError::Io {
                path: install_path,
                source: error,
            });
        }
        replaced.push(ReplacedFile {
            install_path,
            backup_path,
        });
    }
    Ok(())
}

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

    use std::io::Write;

    use super::{
        ApplySelfUpdateOptions, SelfUpdateAssetSelection, SelfUpdateCheckReport,
        SelfUpdateManifest, SelfUpdateStageReport, apply_self_update, current_rais_version,
        evaluate_self_update_report, parse_self_update_manifest, stage_self_update_from_report,
    };
    use crate::RaisError;
    use crate::hash::sha256_file;
    use crate::model::Platform;
    use crate::version::Version;
    use zip::write::SimpleFileOptions;

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

    fn write_test_release_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    fn staged_report_for_zip(
        archive_path: &std::path::Path,
        staging_dir: &std::path::Path,
    ) -> SelfUpdateStageReport {
        let archive_sha = sha256_file(archive_path).unwrap();
        let mut check = sample_check_report(archive_path.display().to_string(), &archive_sha);
        check.asset.url = archive_path.display().to_string();
        SelfUpdateStageReport {
            check,
            staging_dir: staging_dir.to_path_buf(),
            staged_asset_path: Some(archive_path.to_path_buf()),
            downloaded: true,
            reused_existing_file: false,
            verified_sha256: Some(archive_sha),
            ready_to_apply: true,
            status_message: "ready".to_string(),
        }
    }

    #[test]
    fn apply_self_update_replaces_matching_install_files_and_keeps_rollback() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let archive_path = staging_root.path().join("0.2.0").join("RAIS-windows.zip");
        fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        write_test_release_zip(
            &archive_path,
            &[
                ("RAIS.exe", b"new-rais-binary"),
                ("rais-cli.exe", b"new-cli-binary"),
                ("README.txt", b"release notes"),
            ],
        );

        fs::write(install_root.path().join("RAIS.exe"), b"old-rais-binary").unwrap();
        fs::write(install_root.path().join("rais-cli.exe"), b"old-cli-binary").unwrap();

        let stage = staged_report_for_zip(&archive_path, staging_root.path());
        let report = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                package_install_lock_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.replaced_files.len(), 2);
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe")).unwrap(),
            b"new-rais-binary"
        );
        assert_eq!(
            fs::read(install_root.path().join("rais-cli.exe")).unwrap(),
            b"new-cli-binary"
        );
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe.rais-old")).unwrap(),
            b"old-rais-binary"
        );
        assert_eq!(
            fs::read(install_root.path().join("rais-cli.exe.rais-old")).unwrap(),
            b"old-cli-binary"
        );
        assert!(
            report
                .skipped_files
                .iter()
                .any(|path| path.ends_with("README.txt"))
        );
        assert!(report.extraction_dir.is_dir());
    }

    #[test]
    fn apply_self_update_flattens_macos_zip_layout() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let archive_path = staging_root.path().join("0.2.0").join("RAIS-macos.zip");
        fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        write_test_release_zip(
            &archive_path,
            &[
                ("macos/RAIS", b"new-mac-binary"),
                ("macos/rais-cli", b"new-mac-cli"),
            ],
        );

        fs::write(install_root.path().join("RAIS"), b"old-mac-binary").unwrap();

        let stage = staged_report_for_zip(&archive_path, staging_root.path());
        let report = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                package_install_lock_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.replaced_files.len(), 1);
        assert_eq!(
            fs::read(install_root.path().join("RAIS")).unwrap(),
            b"new-mac-binary"
        );
        assert_eq!(
            fs::read(install_root.path().join("RAIS.rais-old")).unwrap(),
            b"old-mac-binary"
        );
        assert!(
            report
                .skipped_files
                .iter()
                .any(|path| path.ends_with("rais-cli"))
        );
    }

    #[test]
    fn apply_self_update_rejects_hash_mismatch_without_touching_install() {
        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let archive_path = staging_root.path().join("0.2.0").join("RAIS-windows.zip");
        fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        write_test_release_zip(&archive_path, &[("RAIS.exe", b"new-rais-binary")]);

        fs::write(install_root.path().join("RAIS.exe"), b"old-rais-binary").unwrap();

        let mut stage = staged_report_for_zip(&archive_path, staging_root.path());
        stage.check.asset.sha256 =
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();

        let error = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                package_install_lock_path: None,
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
            "https://example.test/RAIS-windows.zip".to_string(),
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
                package_install_lock_path: None,
            },
        )
        .unwrap_err();

        assert!(matches!(error, RaisError::InvalidPlannedExecution { .. }));
    }

    #[test]
    fn apply_self_update_refuses_when_package_install_lock_is_held() {
        use std::io::Write as _;
        use zip::write::SimpleFileOptions;

        let staging_root = tempdir().unwrap();
        let install_root = tempdir().unwrap();
        let lock_dir = tempdir().unwrap();
        let lock_path = lock_dir.path().join("install.lock");

        let archive_path = staging_root.path().join("0.2.0").join("RAIS-windows.zip");
        fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        let file = fs::File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("RAIS.exe", options).unwrap();
        writer.write_all(b"new").unwrap();
        writer.finish().unwrap();

        fs::write(install_root.path().join("RAIS.exe"), b"old").unwrap();
        let stage = staged_report_for_zip(&archive_path, staging_root.path());

        let _install_lock = crate::lock::acquire_package_install_lock_at(&lock_path).unwrap();

        let error = apply_self_update(
            &stage,
            &ApplySelfUpdateOptions {
                install_root: Some(install_root.path().to_path_buf()),
                package_install_lock_path: Some(lock_path.clone()),
            },
        )
        .unwrap_err();

        assert!(matches!(error, RaisError::PackageInstallInProgress { .. }));
        assert_eq!(
            fs::read(install_root.path().join("RAIS.exe")).unwrap(),
            b"old"
        );
        assert!(!install_root.path().join("RAIS.exe.rais-old").exists());
    }
}
