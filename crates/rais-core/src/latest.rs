use reqwest::blocking::Client;
use serde_json::Value;

use crate::error::{RaisError, Result};
use crate::package::{
    PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS,
};
use crate::plan::AvailablePackage;
use crate::version::Version;

const USER_AGENT: &str = "RAIS/0.1 (+https://github.com/Timtam/rais)";

pub const REAPER_DOWNLOAD_URL: &str = "https://www.reaper.fm/download.php";
pub const OSARA_UPDATE_URL: &str = "https://osara.reaperaccessibility.com/snapshots/update.json";
pub const SWS_HOME_URL: &str = "https://sws-extension.org/";
pub const REAPACK_GITHUB_LATEST_URL: &str =
    "https://api.github.com/repos/cfillion/reapack/releases/latest";
pub const REAKONTROL_GITHUB_LATEST_URL: &str =
    "https://api.github.com/repos/jcsteh/reaKontrol/releases/latest";

pub fn fetch_latest_versions() -> Result<Vec<AvailablePackage>> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|source| RaisError::Http {
            url: "client-builder".to_string(),
            source,
        })?;

    let providers = [
        (
            PACKAGE_REAPER,
            REAPER_DOWNLOAD_URL,
            parse_reaper_latest_version as VersionParser,
        ),
        (
            PACKAGE_OSARA,
            OSARA_UPDATE_URL,
            parse_osara_update_json as VersionParser,
        ),
        (
            PACKAGE_SWS,
            SWS_HOME_URL,
            parse_sws_latest_version as VersionParser,
        ),
        (
            PACKAGE_REAPACK,
            REAPACK_GITHUB_LATEST_URL,
            parse_github_latest_release_json as VersionParser,
        ),
        (
            PACKAGE_REAKONTROL,
            REAKONTROL_GITHUB_LATEST_URL,
            parse_reakontrol_snapshot_version as VersionParser,
        ),
    ];

    let mut packages = Vec::new();
    for (package_id, url, parser) in providers {
        let body = http_get_text(&client, url)?;
        let version = parser(&body, url)?;
        packages.push(AvailablePackage {
            package_id: package_id.to_string(),
            version: Some(version),
        });
    }

    Ok(packages)
}

type VersionParser = fn(&str, &str) -> Result<Version>;

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

pub fn parse_osara_update_json(body: &str, url: &str) -> Result<Version> {
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let Some(version) = value.get("version").and_then(Value::as_str) else {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: "missing string field: version".to_string(),
        });
    };
    Version::parse(version)
}

pub fn parse_github_latest_release_json(body: &str, url: &str) -> Result<Version> {
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let Some(tag_name) = value.get("tag_name").and_then(Value::as_str) else {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: "missing string field: tag_name".to_string(),
        });
    };
    Version::parse(tag_name.trim_start_matches('v'))
}

pub fn parse_reakontrol_snapshot_version(body: &str, url: &str) -> Result<Version> {
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| RaisError::RemoteData {
            url: url.to_string(),
            message: "missing array field: assets".to_string(),
        })?;

    let mut latest: Option<Version> = None;
    for asset in assets {
        let Some(name) = asset.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(version) = reakontrol_version_from_asset_name(name) else {
            continue;
        };
        latest = Some(match latest {
            Some(current) if current.cmp_lenient(&version).is_ge() => current,
            _ => version,
        });
    }

    latest.ok_or_else(|| RaisError::RemoteData {
        url: url.to_string(),
        message: "no ReaKontrol snapshot asset matched the expected name pattern".to_string(),
    })
}

pub(crate) fn reakontrol_version_from_asset_name(name: &str) -> Option<Version> {
    let stem = name.strip_suffix(".zip")?;
    let after_platform = stem
        .strip_prefix("reaKontrol_windows_")
        .or_else(|| stem.strip_prefix("reaKontrol_mac_"))?;
    let version_part = after_platform
        .rsplit_once('.')
        .map(|(left, _commit)| left)?;
    Version::parse(version_part).ok()
}

pub fn parse_sws_latest_version(body: &str, url: &str) -> Result<Version> {
    let marker = "Latest stable version:";
    let Some(marker_start) = body.find(marker) else {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: "missing latest stable version marker".to_string(),
        });
    };
    let tail_start = marker_start + marker.len();
    let tail = &body[tail_start..body.len().min(tail_start + 160)];
    let Some(version_start) = tail.find('v') else {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: "missing SWS version prefix".to_string(),
        });
    };

    let base = collect_version_chars(&tail[version_start + 1..]);
    let build = tail
        .find('#')
        .map(|index| collect_digits(&tail[index + 1..]))
        .filter(|digits| !digits.is_empty());

    if base.is_empty() {
        return Err(RaisError::RemoteData {
            url: url.to_string(),
            message: "missing SWS version number".to_string(),
        });
    }

    let version = match build {
        Some(build) => format!("{base}.{build}"),
        None => base,
    };
    Version::parse(version)
}

pub fn parse_reaper_latest_version(body: &str, url: &str) -> Result<Version> {
    if let Some(version) = version_after_marker(body, "Version ") {
        return Version::parse(version);
    }
    if let Some(version) = version_after_marker(body, "REAPER v") {
        return Version::parse(version);
    }

    Err(RaisError::RemoteData {
        url: url.to_string(),
        message: "missing REAPER version token".to_string(),
    })
}

fn version_after_marker<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let marker_start = text.find(marker)?;
    let start = marker_start + marker.len();
    first_version_like_token(&text[start..text.len().min(start + 80)])
}

fn first_version_like_token(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit() {
            continue;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let candidate = &text[start..end];
        if candidate.contains('.') {
            return Some(candidate);
        }
    }
    None
}

fn collect_version_chars(text: &str) -> String {
    text.chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect()
}

fn collect_digits(text: &str) -> String {
    text.chars()
        .skip_while(|ch| ch.is_ascii_whitespace())
        .take_while(char::is_ascii_digit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        OSARA_UPDATE_URL, REAKONTROL_GITHUB_LATEST_URL, REAPACK_GITHUB_LATEST_URL,
        REAPER_DOWNLOAD_URL, SWS_HOME_URL, parse_github_latest_release_json,
        parse_osara_update_json, parse_reakontrol_snapshot_version, parse_reaper_latest_version,
        parse_sws_latest_version, reakontrol_version_from_asset_name,
    };

    #[test]
    fn parses_osara_update_json() {
        let version =
            parse_osara_update_json(r#"{"version":"2026.4.16.2157,593ff26b"}"#, OSARA_UPDATE_URL)
                .unwrap();
        assert_eq!(version.raw(), "2026.4.16.2157,593ff26b");
    }

    #[test]
    fn parses_sws_home_page_version() {
        let version = parse_sws_latest_version(
            "## Latest stable version: v2.14.0 #7 - September 07, 2025",
            SWS_HOME_URL,
        )
        .unwrap();
        assert_eq!(version.raw(), "2.14.0.7");
    }

    #[test]
    fn parses_reapack_github_latest_release() {
        let version =
            parse_github_latest_release_json(r#"{"tag_name":"v1.2.6"}"#, REAPACK_GITHUB_LATEST_URL)
                .unwrap();
        assert_eq!(version.raw(), "1.2.6");
    }

    #[test]
    fn parses_reaper_download_page_version() {
        let version = parse_reaper_latest_version(
            "<div class='hdrbottom'>Version 7.69: April 12, 2026</div>",
            REAPER_DOWNLOAD_URL,
        )
        .unwrap();
        assert_eq!(version.raw(), "7.69");
    }

    #[test]
    fn extracts_reakontrol_version_from_asset_name() {
        let version =
            reakontrol_version_from_asset_name("reaKontrol_windows_2025.6.6.7.bfbe7606.zip")
                .unwrap();
        assert_eq!(version.raw(), "2025.6.6.7");
        let version =
            reakontrol_version_from_asset_name("reaKontrol_mac_2026.2.16.100.deadbeef.zip")
                .unwrap();
        assert_eq!(version.raw(), "2026.2.16.100");
        assert!(reakontrol_version_from_asset_name("README.md").is_none());
    }

    #[test]
    fn picks_highest_reakontrol_snapshot_version_from_assets() {
        let body = r#"{
            "tag_name": "snapshots",
            "assets": [
                {"name": "reaKontrol_windows_2025.6.6.7.bfbe7606.zip"},
                {"name": "reaKontrol_mac_2026.2.16.100.cafef00d.zip"},
                {"name": "reaKontrol_windows_2026.2.16.100.cafef00d.zip"},
                {"name": "reaKontrol_mac_2025.7.25.10.4ce6b01f.zip"}
            ]
        }"#;
        let version =
            parse_reakontrol_snapshot_version(body, REAKONTROL_GITHUB_LATEST_URL).unwrap();
        assert_eq!(version.raw(), "2026.2.16.100");
    }

    #[test]
    fn rejects_reakontrol_release_with_no_matching_assets() {
        let body = r#"{"tag_name": "snapshots", "assets": [{"name": "README.md"}]}"#;
        let error =
            parse_reakontrol_snapshot_version(body, REAKONTROL_GITHUB_LATEST_URL).unwrap_err();
        assert!(error.to_string().contains("ReaKontrol"));
    }
}
