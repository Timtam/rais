use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{IoPathContext, RaisError, Result};
use crate::hash::sha256_file;
use crate::latest::{
    OSARA_UPDATE_URL, REAKONTROL_GITHUB_LATEST_URL, REAPACK_GITHUB_LATEST_URL, REAPER_DOWNLOAD_URL,
    SWS_HOME_URL, parse_github_latest_release_json, parse_osara_update_json,
    parse_reaper_latest_version, parse_sws_latest_version, reakontrol_version_from_asset_name,
};
use crate::model::{Architecture, Platform};
use crate::package::{
    PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS,
};
use crate::version::Version;

const USER_AGENT: &str = "RAIS/0.1 (+https://github.com/reaper-accessibility/rais)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Installer,
    Archive,
    DiskImage,
    ExtensionBinary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDescriptor {
    pub package_id: String,
    pub version: Version,
    pub platform: Platform,
    pub architecture: Architecture,
    pub kind: ArtifactKind,
    pub url: String,
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedArtifact {
    pub descriptor: ArtifactDescriptor,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub reused_existing_file: bool,
}

pub fn resolve_latest_artifacts(
    package_ids: &[String],
    platform: Platform,
    architecture: Architecture,
) -> Result<Vec<ArtifactDescriptor>> {
    let client = http_client()?;
    let mut artifacts = Vec::new();

    for package_id in package_ids {
        let artifact = match package_id.as_str() {
            PACKAGE_REAPER => resolve_reaper_artifact(&client, platform, architecture)?,
            PACKAGE_OSARA => resolve_osara_artifact(&client, platform, architecture)?,
            PACKAGE_SWS => resolve_sws_artifact(&client, platform, architecture)?,
            PACKAGE_REAPACK => resolve_reapack_artifact(&client, platform, architecture)?,
            PACKAGE_REAKONTROL => resolve_reakontrol_artifact(&client, platform, architecture)?,
            _ => {
                return Err(RaisError::NoArtifactFound {
                    package_id: package_id.clone(),
                    platform,
                    architecture,
                });
            }
        };
        artifacts.push(artifact);
    }

    Ok(artifacts)
}

pub fn expected_artifact_kind(
    package_id: &str,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactKind> {
    match package_id {
        PACKAGE_REAPER => expected_reaper_artifact_kind(platform, architecture),
        PACKAGE_OSARA => expected_osara_artifact_kind(platform),
        PACKAGE_SWS => expected_sws_artifact_kind(platform, architecture),
        PACKAGE_REAPACK => expected_reapack_artifact_kind(platform, architecture),
        PACKAGE_REAKONTROL => expected_reakontrol_artifact_kind(platform),
        _ => Err(RaisError::NoArtifactFound {
            package_id: package_id.to_string(),
            platform,
            architecture,
        }),
    }
}

pub fn default_cache_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("RAIS").join("cache");
        }
    }

    if cfg!(target_os = "macos") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("RAIS");
        }
    }

    env::temp_dir().join("rais-cache")
}

pub fn download_artifacts(
    artifacts: &[ArtifactDescriptor],
    cache_dir: &Path,
) -> Result<Vec<CachedArtifact>> {
    let client = http_client()?;
    let mut cached = Vec::new();

    for artifact in artifacts {
        cached.push(download_artifact(&client, artifact, cache_dir)?);
    }

    Ok(cached)
}

fn download_artifact(
    client: &Client,
    artifact: &ArtifactDescriptor,
    cache_dir: &Path,
) -> Result<CachedArtifact> {
    let package_dir = cache_dir
        .join(&artifact.package_id)
        .join(artifact.version.raw().replace(',', "_"));
    fs::create_dir_all(&package_dir).with_path(&package_dir)?;

    let target_path = package_dir.join(&artifact.file_name);
    if target_path.is_file() {
        return cached_artifact(artifact, target_path, true);
    }

    if let Some(source_path) = local_artifact_source_path(&artifact.url)? {
        copy_local_artifact(artifact, &source_path, &target_path)?;
        return cached_artifact(artifact, target_path, false);
    }

    validate_remote_artifact_url(&artifact.url)?;

    let part_path = target_path.with_extension(format!(
        "{}.part",
        target_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ));

    let mut response = client
        .get(&artifact.url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RaisError::Http {
            url: artifact.url.clone(),
            source,
        })?;
    let mut file = fs::File::create(&part_path).with_path(&part_path)?;
    std::io::copy(&mut response, &mut file).with_path(&part_path)?;
    file.flush().with_path(&part_path)?;
    drop(file);

    fs::rename(&part_path, &target_path).with_path(&target_path)?;
    cached_artifact(artifact, target_path, false)
}

fn copy_local_artifact(
    artifact: &ArtifactDescriptor,
    source_path: &Path,
    target_path: &Path,
) -> Result<()> {
    let part_path = target_path.with_extension(format!(
        "{}.part",
        target_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ));

    fs::copy(source_path, &part_path).with_path(source_path)?;
    fs::rename(&part_path, target_path).with_path(target_path)?;
    if !target_path.is_file() {
        return Err(RaisError::RemoteData {
            url: artifact.url.clone(),
            message: "local artifact copy did not produce a cache file".to_string(),
        });
    }
    Ok(())
}

fn cached_artifact(
    descriptor: &ArtifactDescriptor,
    path: PathBuf,
    reused_existing_file: bool,
) -> Result<CachedArtifact> {
    let metadata = fs::metadata(&path).with_path(&path)?;
    let sha256 = sha256_file(&path)?;

    Ok(CachedArtifact {
        descriptor: descriptor.clone(),
        path,
        size: metadata.len(),
        sha256,
        reused_existing_file,
    })
}

fn resolve_reaper_artifact(
    client: &Client,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let body = http_get_text(client, REAPER_DOWNLOAD_URL)?;
    let version = parse_reaper_latest_version(&body, REAPER_DOWNLOAD_URL)?;
    let (fragment, kind, selected_architecture) = match (platform, architecture) {
        (Platform::Windows, Architecture::X86) => {
            ("-install.exe", ArtifactKind::Installer, Architecture::X86)
        }
        (
            Platform::Windows,
            Architecture::X64 | Architecture::Universal | Architecture::Unknown,
        ) => (
            "_x64-install.exe",
            ArtifactKind::Installer,
            Architecture::X64,
        ),
        (Platform::Windows, Architecture::Arm64 | Architecture::Arm64Ec) => {
            ("arm64ec", ArtifactKind::Installer, Architecture::Arm64Ec)
        }
        (Platform::MacOs, Architecture::X86) => {
            ("_i386.dmg", ArtifactKind::DiskImage, Architecture::X86)
        }
        (
            Platform::MacOs,
            Architecture::X64
            | Architecture::Arm64
            | Architecture::Arm64Ec
            | Architecture::Universal
            | Architecture::Unknown,
        ) => (
            "_universal.dmg",
            ArtifactKind::DiskImage,
            Architecture::Universal,
        ),
    };

    let href = find_href_containing(&body, fragment).ok_or_else(|| RaisError::NoArtifactFound {
        package_id: PACKAGE_REAPER.to_string(),
        platform,
        architecture,
    })?;
    artifact_from_href(
        PACKAGE_REAPER,
        version,
        platform,
        selected_architecture,
        kind,
        "https://www.reaper.fm/",
        &href,
    )
}

fn expected_reaper_artifact_kind(
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactKind> {
    match (platform, architecture) {
        (Platform::Windows, Architecture::X86)
        | (
            Platform::Windows,
            Architecture::X64 | Architecture::Universal | Architecture::Unknown,
        )
        | (Platform::Windows, Architecture::Arm64 | Architecture::Arm64Ec) => {
            Ok(ArtifactKind::Installer)
        }
        (Platform::MacOs, Architecture::X86)
        | (
            Platform::MacOs,
            Architecture::X64
            | Architecture::Arm64
            | Architecture::Arm64Ec
            | Architecture::Universal
            | Architecture::Unknown,
        ) => Ok(ArtifactKind::DiskImage),
    }
}

fn resolve_osara_artifact(
    client: &Client,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let update_body = http_get_text(client, OSARA_UPDATE_URL)?;
    let version = parse_osara_update_json(&update_body, OSARA_UPDATE_URL)?;
    let snapshot_body = http_get_text(client, "https://osara.reaperaccessibility.com/snapshots/")?;

    let (fragment, kind) = match platform {
        Platform::Windows => (".exe", ArtifactKind::Installer),
        Platform::MacOs => (".zip", ArtifactKind::Archive),
    };
    let href = find_href_with(&snapshot_body, |href, _context| {
        href.contains("/jcsteh/osara/releases/download/snapshots/osara_")
            && href.ends_with(fragment)
    })
    .ok_or_else(|| RaisError::NoArtifactFound {
        package_id: PACKAGE_OSARA.to_string(),
        platform,
        architecture,
    })?;

    artifact_from_href(
        PACKAGE_OSARA,
        version,
        platform,
        Architecture::Universal,
        kind,
        "https://osara.reaperaccessibility.com/snapshots/",
        &href,
    )
}

fn expected_osara_artifact_kind(platform: Platform) -> Result<ArtifactKind> {
    match platform {
        Platform::Windows => Ok(ArtifactKind::Installer),
        Platform::MacOs => Ok(ArtifactKind::Archive),
    }
}

fn resolve_sws_artifact(
    client: &Client,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let body = http_get_text(client, SWS_HOME_URL)?;
    let version = parse_sws_latest_version(&body, SWS_HOME_URL)?;
    let (fragment, kind, selected_architecture) = match (platform, architecture) {
        (Platform::Windows, Architecture::X86) => (
            "Windows-x86.exe",
            ArtifactKind::Installer,
            Architecture::X86,
        ),
        (Platform::Windows, Architecture::X64 | Architecture::Unknown) => (
            "Windows-x64.exe",
            ArtifactKind::Installer,
            Architecture::X64,
        ),
        (Platform::MacOs, Architecture::X86) => (
            "Darwin-i386.dmg",
            ArtifactKind::DiskImage,
            Architecture::X86,
        ),
        (Platform::MacOs, Architecture::X64 | Architecture::Unknown) => (
            "Darwin-x86_64.dmg",
            ArtifactKind::DiskImage,
            Architecture::X64,
        ),
        (Platform::MacOs, Architecture::Arm64) => (
            "Darwin-arm64.dmg",
            ArtifactKind::DiskImage,
            Architecture::Arm64,
        ),
        _ => {
            return Err(RaisError::NoArtifactFound {
                package_id: PACKAGE_SWS.to_string(),
                platform,
                architecture,
            });
        }
    };

    let href = find_href_containing(&body, fragment).ok_or_else(|| RaisError::NoArtifactFound {
        package_id: PACKAGE_SWS.to_string(),
        platform,
        architecture,
    })?;
    artifact_from_href(
        PACKAGE_SWS,
        version,
        platform,
        selected_architecture,
        kind,
        "https://sws-extension.org/",
        &href,
    )
}

fn expected_sws_artifact_kind(
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactKind> {
    match (platform, architecture) {
        (Platform::Windows, Architecture::X86)
        | (Platform::Windows, Architecture::X64 | Architecture::Unknown) => {
            Ok(ArtifactKind::Installer)
        }
        (Platform::MacOs, Architecture::X86)
        | (Platform::MacOs, Architecture::X64 | Architecture::Unknown)
        | (Platform::MacOs, Architecture::Arm64) => Ok(ArtifactKind::DiskImage),
        _ => Err(RaisError::NoArtifactFound {
            package_id: PACKAGE_SWS.to_string(),
            platform,
            architecture,
        }),
    }
}

fn resolve_reapack_artifact(
    client: &Client,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let body = http_get_text(client, REAPACK_GITHUB_LATEST_URL)?;
    let version = parse_github_latest_release_json(&body, REAPACK_GITHUB_LATEST_URL)?;
    let (asset_name, selected_architecture) = match (platform, architecture) {
        (Platform::Windows, Architecture::X86) => ("reaper_reapack-x86.dll", Architecture::X86),
        (
            Platform::Windows,
            Architecture::X64 | Architecture::Universal | Architecture::Unknown,
        ) => ("reaper_reapack-x64.dll", Architecture::X64),
        (Platform::Windows, Architecture::Arm64 | Architecture::Arm64Ec) => {
            ("reaper_reapack-arm64ec.dll", Architecture::Arm64Ec)
        }
        (Platform::MacOs, Architecture::X86) => ("reaper_reapack-i386.dylib", Architecture::X86),
        (Platform::MacOs, Architecture::X64 | Architecture::Unknown) => {
            ("reaper_reapack-x86_64.dylib", Architecture::X64)
        }
        (Platform::MacOs, Architecture::Arm64 | Architecture::Arm64Ec) => {
            ("reaper_reapack-arm64.dylib", Architecture::Arm64)
        }
        (Platform::MacOs, Architecture::Universal) => {
            ("reaper_reapack-arm64.dylib", Architecture::Arm64)
        }
    };

    let value: Value = serde_json::from_str(&body).map_err(|source| RaisError::RemoteData {
        url: REAPACK_GITHUB_LATEST_URL.to_string(),
        message: source.to_string(),
    })?;
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| RaisError::RemoteData {
            url: REAPACK_GITHUB_LATEST_URL.to_string(),
            message: "missing array field: assets".to_string(),
        })?;

    for asset in assets {
        let name = asset.get("name").and_then(Value::as_str);
        let download_url = asset.get("browser_download_url").and_then(Value::as_str);
        if name == Some(asset_name) {
            let Some(url) = download_url else {
                break;
            };
            return Ok(ArtifactDescriptor {
                package_id: PACKAGE_REAPACK.to_string(),
                version,
                platform,
                architecture: selected_architecture,
                kind: ArtifactKind::ExtensionBinary,
                url: url.to_string(),
                file_name: asset_name.to_string(),
            });
        }
    }

    Err(RaisError::NoArtifactFound {
        package_id: PACKAGE_REAPACK.to_string(),
        platform,
        architecture,
    })
}

fn expected_reapack_artifact_kind(
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactKind> {
    match (platform, architecture) {
        (Platform::Windows, Architecture::X86)
        | (
            Platform::Windows,
            Architecture::X64
            | Architecture::Universal
            | Architecture::Unknown
            | Architecture::Arm64
            | Architecture::Arm64Ec,
        )
        | (Platform::MacOs, Architecture::X86)
        | (
            Platform::MacOs,
            Architecture::X64
            | Architecture::Unknown
            | Architecture::Arm64
            | Architecture::Arm64Ec
            | Architecture::Universal,
        ) => Ok(ArtifactKind::ExtensionBinary),
    }
}

fn resolve_reakontrol_artifact(
    client: &Client,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let body = http_get_text(client, REAKONTROL_GITHUB_LATEST_URL)?;
    resolve_reakontrol_artifact_from_release_body(&body, platform, architecture)
}

fn resolve_reakontrol_artifact_from_release_body(
    body: &str,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: REAKONTROL_GITHUB_LATEST_URL.to_string(),
        message: source.to_string(),
    })?;
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| RaisError::RemoteData {
            url: REAKONTROL_GITHUB_LATEST_URL.to_string(),
            message: "missing array field: assets".to_string(),
        })?;

    let platform_token = match platform {
        Platform::Windows => "reaKontrol_windows_",
        Platform::MacOs => "reaKontrol_mac_",
    };

    let mut best: Option<(crate::version::Version, String, String)> = None;
    for asset in assets {
        let Some(name) = asset.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !name.starts_with(platform_token) || !name.ends_with(".zip") {
            continue;
        }
        let Some(url) = asset.get("browser_download_url").and_then(Value::as_str) else {
            continue;
        };
        let Some(version) = reakontrol_version_from_asset_name(name) else {
            continue;
        };
        best = Some(match best {
            Some((current_version, current_name, current_url))
                if current_version.cmp_lenient(&version).is_ge() =>
            {
                (current_version, current_name, current_url)
            }
            _ => (version, name.to_string(), url.to_string()),
        });
    }

    let (version, file_name, url) = best.ok_or_else(|| RaisError::NoArtifactFound {
        package_id: PACKAGE_REAKONTROL.to_string(),
        platform,
        architecture,
    })?;

    Ok(ArtifactDescriptor {
        package_id: PACKAGE_REAKONTROL.to_string(),
        version,
        platform,
        architecture: Architecture::Universal,
        kind: ArtifactKind::Archive,
        url,
        file_name,
    })
}

fn expected_reakontrol_artifact_kind(_platform: Platform) -> Result<ArtifactKind> {
    Ok(ArtifactKind::Archive)
}

fn artifact_from_href(
    package_id: &str,
    version: Version,
    platform: Platform,
    architecture: Architecture,
    kind: ArtifactKind,
    base_url: &str,
    href: &str,
) -> Result<ArtifactDescriptor> {
    let url = absolute_url(base_url, href);
    let file_name = file_name_from_url(&url).ok_or_else(|| RaisError::RemoteData {
        url: url.clone(),
        message: "artifact URL does not contain a file name".to_string(),
    })?;

    Ok(ArtifactDescriptor {
        package_id: package_id.to_string(),
        version,
        platform,
        architecture,
        kind,
        url,
        file_name,
    })
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

fn http_get_text(client: &Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RaisError::Http {
            url: url.to_string(),
            source,
        })?;

    response.text().map_err(|source| RaisError::Http {
        url: url.to_string(),
        source,
    })
}

fn find_href_containing(body: &str, fragment: &str) -> Option<String> {
    find_href_with(body, |href, _context| href.contains(fragment))
}

fn find_href_with(body: &str, predicate: impl Fn(&str, &str) -> bool) -> Option<String> {
    let mut offset = 0;
    while let Some(relative_start) = body[offset..].find("href=") {
        let href_start = offset + relative_start + "href=".len();
        let quote = body.as_bytes().get(href_start).copied()?;
        if quote != b'\'' && quote != b'"' {
            offset = href_start;
            continue;
        }

        let value_start = href_start + 1;
        let value_end = body[value_start..]
            .find(quote as char)
            .map(|relative_end| value_start + relative_end)?;
        let href = &body[value_start..value_end];
        let context_end = body.len().min(value_end + 400);
        let context = &body[value_end..context_end];

        if predicate(href, context) {
            return Some(decode_basic_entities(href));
        }

        offset = value_end + 1;
    }

    None
}

fn absolute_url(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else {
        format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            href.trim_start_matches('/')
        )
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

fn decode_basic_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn local_artifact_source_path(url_or_path: &str) -> Result<Option<PathBuf>> {
    if let Some(rest) = url_or_path.strip_prefix("file://") {
        return file_url_path(rest).map(Some);
    }

    let path = PathBuf::from(url_or_path);
    if path.is_file() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

fn validate_remote_artifact_url(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        return Ok(());
    }

    let message = if url.contains("://") {
        "remote artifact downloads must use HTTPS"
    } else {
        "artifact URL is neither an existing local file nor an HTTPS URL"
    };
    Err(RaisError::InvalidArtifactUrl {
        url: url.to_string(),
        message: message.to_string(),
    })
}

fn file_url_path(rest: &str) -> Result<PathBuf> {
    let without_host = rest.strip_prefix("localhost/").unwrap_or(rest);
    let decoded = percent_decode_file_url_path(without_host)?;
    let path = if cfg!(windows) {
        let windows_path = decoded
            .strip_prefix('/')
            .filter(|path| path.as_bytes().get(1) == Some(&b':'))
            .unwrap_or(&decoded);
        PathBuf::from(windows_path.replace('/', "\\"))
    } else {
        PathBuf::from(format!("/{}", decoded.trim_start_matches('/')))
    };
    Ok(path)
}

fn percent_decode_file_url_path(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(hex) = bytes.get(index + 1..index + 3) else {
                return Err(invalid_file_url(input));
            };
            let hex = std::str::from_utf8(hex).map_err(|_| invalid_file_url(input))?;
            let value = u8::from_str_radix(hex, 16).map_err(|_| invalid_file_url(input))?;
            output.push(value);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(output).map_err(|_| invalid_file_url(input))
}

fn invalid_file_url(input: &str) -> RaisError {
    RaisError::RemoteData {
        url: format!("file://{input}"),
        message: "invalid file URL path encoding".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::artifact::{
        absolute_url, expected_artifact_kind, file_name_from_url, find_href_containing,
        resolve_reakontrol_artifact_from_release_body, resolve_reapack_asset_from_fixture,
    };
    use crate::package::{
        PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS,
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn finds_href_by_fragment() {
        let body = r#"<a href="download/featured/sws-2.14.0.7-Windows-x64.exe">Download</a>"#;
        let href = find_href_containing(body, "Windows-x64.exe").unwrap();
        assert_eq!(href, "download/featured/sws-2.14.0.7-Windows-x64.exe");
    }

    #[test]
    fn resolves_relative_urls() {
        assert_eq!(
            absolute_url("https://sws-extension.org/", "download/file.exe"),
            "https://sws-extension.org/download/file.exe"
        );
    }

    #[test]
    fn extracts_file_names_from_urls() {
        assert_eq!(
            file_name_from_url("https://example.test/files/reaper.exe?download=1").unwrap(),
            "reaper.exe"
        );
    }

    #[test]
    fn resolves_reapack_asset_from_json_fixture() {
        let body = r#"{
            "tag_name": "v1.2.6",
            "assets": [
                {
                    "name": "reaper_reapack-x64.dll",
                    "browser_download_url": "https://github.com/cfillion/reapack/releases/download/v1.2.6/reaper_reapack-x64.dll"
                }
            ]
        }"#;
        let artifact =
            resolve_reapack_asset_from_fixture(body, Platform::Windows, Architecture::X64).unwrap();

        assert_eq!(artifact.file_name, "reaper_reapack-x64.dll");
        assert_eq!(artifact.version.raw(), "1.2.6");
    }

    #[test]
    fn caches_existing_local_path_artifact() {
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("osara-test.exe");
        fs::write(&source_path, b"local installer bytes").unwrap();

        let cache_dir = tempdir().unwrap();
        let artifact = ArtifactDescriptor {
            package_id: PACKAGE_OSARA.to_string(),
            version: Version::parse("1.2.3").unwrap(),
            platform: Platform::Windows,
            architecture: Architecture::X64,
            kind: ArtifactKind::Installer,
            url: source_path.display().to_string(),
            file_name: "osara-test.exe".to_string(),
        };

        let cached = download_artifacts(std::slice::from_ref(&artifact), cache_dir.path()).unwrap();
        assert_eq!(cached.len(), 1);
        assert!(!cached[0].reused_existing_file);
        assert_eq!(fs::read(&cached[0].path).unwrap(), b"local installer bytes");

        let cached_again = download_artifacts(&[artifact], cache_dir.path()).unwrap();
        assert!(cached_again[0].reused_existing_file);
    }

    #[test]
    fn caches_file_url_artifact() {
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("osara test.exe");
        fs::write(&source_path, b"file url installer bytes").unwrap();

        let cache_dir = tempdir().unwrap();
        let artifact = ArtifactDescriptor {
            package_id: PACKAGE_OSARA.to_string(),
            version: Version::parse("1.2.3").unwrap(),
            platform: Platform::Windows,
            architecture: Architecture::X64,
            kind: ArtifactKind::Installer,
            url: file_url_for_test(&source_path),
            file_name: "osara-test.exe".to_string(),
        };

        let cached = download_artifacts(&[artifact], cache_dir.path()).unwrap();
        assert_eq!(
            fs::read(&cached[0].path).unwrap(),
            b"file url installer bytes"
        );
    }

    #[test]
    fn rejects_non_https_remote_artifacts() {
        let cache_dir = tempdir().unwrap();
        let artifact = ArtifactDescriptor {
            package_id: PACKAGE_OSARA.to_string(),
            version: Version::parse("1.2.3").unwrap(),
            platform: Platform::Windows,
            architecture: Architecture::X64,
            kind: ArtifactKind::Installer,
            url: "http://example.test/osara-test.exe".to_string(),
            file_name: "osara-test.exe".to_string(),
        };

        let error = download_artifacts(&[artifact], cache_dir.path()).unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
    }

    #[test]
    fn reports_expected_artifact_kind_for_builtin_packages() {
        assert_eq!(
            expected_artifact_kind(PACKAGE_REAPER, Platform::Windows, Architecture::X64).unwrap(),
            ArtifactKind::Installer
        );
        assert_eq!(
            expected_artifact_kind(PACKAGE_OSARA, Platform::MacOs, Architecture::Arm64).unwrap(),
            ArtifactKind::Archive
        );
        assert_eq!(
            expected_artifact_kind(PACKAGE_SWS, Platform::MacOs, Architecture::X64).unwrap(),
            ArtifactKind::DiskImage
        );
        assert_eq!(
            expected_artifact_kind(PACKAGE_REAPACK, Platform::Windows, Architecture::X64).unwrap(),
            ArtifactKind::ExtensionBinary
        );
        assert_eq!(
            expected_artifact_kind(PACKAGE_REAKONTROL, Platform::Windows, Architecture::X64)
                .unwrap(),
            ArtifactKind::Archive
        );
        assert_eq!(
            expected_artifact_kind(PACKAGE_REAKONTROL, Platform::MacOs, Architecture::Arm64)
                .unwrap(),
            ArtifactKind::Archive
        );
    }

    #[test]
    fn resolves_reakontrol_archive_for_platform() {
        let body = r#"{
            "tag_name": "snapshots",
            "assets": [
                {
                    "name": "reaKontrol_windows_2025.6.6.7.bfbe7606.zip",
                    "browser_download_url": "https://github.com/jcsteh/reaKontrol/releases/download/snapshots/reaKontrol_windows_2025.6.6.7.bfbe7606.zip"
                },
                {
                    "name": "reaKontrol_windows_2026.2.16.100.cafef00d.zip",
                    "browser_download_url": "https://github.com/jcsteh/reaKontrol/releases/download/snapshots/reaKontrol_windows_2026.2.16.100.cafef00d.zip"
                },
                {
                    "name": "reaKontrol_mac_2026.2.16.100.cafef00d.zip",
                    "browser_download_url": "https://github.com/jcsteh/reaKontrol/releases/download/snapshots/reaKontrol_mac_2026.2.16.100.cafef00d.zip"
                }
            ]
        }"#;

        let windows = resolve_reakontrol_artifact_from_release_body(
            body,
            Platform::Windows,
            Architecture::X64,
        )
        .unwrap();
        assert_eq!(windows.kind, ArtifactKind::Archive);
        assert_eq!(windows.version.raw(), "2026.2.16.100");
        assert_eq!(
            windows.file_name,
            "reaKontrol_windows_2026.2.16.100.cafef00d.zip"
        );
        assert!(
            windows
                .url
                .starts_with("https://github.com/jcsteh/reaKontrol/")
        );
        assert_eq!(windows.architecture, Architecture::Universal);

        let mac = resolve_reakontrol_artifact_from_release_body(
            body,
            Platform::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
        assert_eq!(mac.file_name, "reaKontrol_mac_2026.2.16.100.cafef00d.zip");
    }

    #[test]
    fn errors_when_reakontrol_release_has_no_matching_assets() {
        let body = r#"{"tag_name": "snapshots", "assets": []}"#;
        let error = resolve_reakontrol_artifact_from_release_body(
            body,
            Platform::Windows,
            Architecture::X64,
        )
        .unwrap_err();
        assert!(matches!(error, RaisError::NoArtifactFound { .. }));
    }

    fn file_url_for_test(path: &Path) -> String {
        if cfg!(windows) {
            format!(
                "file:///{}",
                path.display()
                    .to_string()
                    .replace('\\', "/")
                    .replace(' ', "%20")
            )
        } else {
            format!("file://{}", path.display().to_string().replace(' ', "%20"))
        }
    }
}

#[cfg(test)]
fn resolve_reapack_asset_from_fixture(
    body: &str,
    platform: Platform,
    architecture: Architecture,
) -> Result<ArtifactDescriptor> {
    let version = parse_github_latest_release_json(body, REAPACK_GITHUB_LATEST_URL)?;
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: REAPACK_GITHUB_LATEST_URL.to_string(),
        message: source.to_string(),
    })?;
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| RaisError::RemoteData {
            url: REAPACK_GITHUB_LATEST_URL.to_string(),
            message: "missing array field: assets".to_string(),
        })?;

    let asset_name = match (platform, architecture) {
        (Platform::Windows, Architecture::X64) => "reaper_reapack-x64.dll",
        _ => "unknown",
    };

    for asset in assets {
        if asset.get("name").and_then(Value::as_str) == Some(asset_name) {
            let url = asset
                .get("browser_download_url")
                .and_then(Value::as_str)
                .unwrap();
            return Ok(ArtifactDescriptor {
                package_id: PACKAGE_REAPACK.to_string(),
                version,
                platform,
                architecture,
                kind: ArtifactKind::ExtensionBinary,
                url: url.to_string(),
                file_name: asset_name.to_string(),
            });
        }
    }

    Err(RaisError::NoArtifactFound {
        package_id: PACKAGE_REAPACK.to_string(),
        platform,
        architecture,
    })
}
