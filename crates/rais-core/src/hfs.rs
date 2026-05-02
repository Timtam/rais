//! Tiny client for rejetto's [HTTP File Server](https://github.com/rejetto/hfs)
//! REST API.
//!
//! `hoard.reaperaccessibility.com` is an HFS instance, and at least one of the
//! packages RAIS installs (the JAWS-for-REAPER scripts) is published there.
//! HFS exposes the contents of a folder via
//! `POST /~/api/get_file_list` with a JSON body of the form
//! `{"uri": "/folder/", "limit": <n>}` (note: the parameter is `uri`, not
//! `path` — HFS will silently fall back to the root listing if the field
//! name is wrong, which is a hard-to-spot bug). The response shape is
//! roughly:
//!
//! ```json
//! { "list": [
//!     { "n": "JFRSCRIPTS_v3.18.zip", "s": 124533, "m": "2026-01-01T..." },
//!     { "n": "subfolder/", "s": null }
//! ] }
//! ```
//!
//! This module only depends on `reqwest` + `serde_json` and does **not**
//! decide which file in a folder is "the right one" — that policy lives with
//! the caller (e.g. `latest.rs` for JAWS scripts: highest-version `*.zip`).

use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::error::{RaisError, Result};

/// One entry from a folder listing. Files have a name without a trailing
/// slash; the listing may also contain subdirectories which we surface so the
/// caller can filter them out (or recurse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfsListEntry {
    pub name: String,
    pub size: Option<u64>,
    pub is_directory: bool,
}

/// POSTs to `<base>/~/api/get_file_list` with the given folder path and parses
/// the response. `base` should be the HFS root (no trailing slash, e.g.
/// `https://hoard.reaperaccessibility.com`); `folder` is the path relative to
/// that root, with leading slash (e.g. `/jaws-for-reaper/`).
pub fn fetch_file_list(client: &Client, base: &str, folder: &str) -> Result<Vec<HfsListEntry>> {
    let url = format!("{}/~/api/get_file_list", base.trim_end_matches('/'));
    let response = client
        .post(&url)
        .json(&json!({ "uri": folder, "limit": 1000 }))
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|source| RaisError::Http {
            url: url.clone(),
            source,
        })?;

    let body = response.text().map_err(|source| RaisError::Http {
        url: url.clone(),
        source,
    })?;
    parse_get_file_list_response(&body, &url)
}

/// Pure parser for an HFS `get_file_list` response. Separated from
/// [`fetch_file_list`] so unit tests can pin the expected shape without
/// hitting the network.
pub fn parse_get_file_list_response(body: &str, url: &str) -> Result<Vec<HfsListEntry>> {
    let value: Value = serde_json::from_str(body).map_err(|source| RaisError::RemoteData {
        url: url.to_string(),
        message: source.to_string(),
    })?;
    let list =
        value
            .get("list")
            .and_then(Value::as_array)
            .ok_or_else(|| RaisError::RemoteData {
                url: url.to_string(),
                message: "missing array field: list".to_string(),
            })?;

    let mut entries = Vec::with_capacity(list.len());
    for item in list {
        let Some(name) = item.get("n").and_then(Value::as_str) else {
            continue;
        };
        let size = item.get("s").and_then(Value::as_u64);
        let is_directory = name.ends_with('/');
        let trimmed = name.trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            continue;
        }
        entries.push(HfsListEntry {
            name: trimmed,
            size,
            is_directory,
        });
    }

    Ok(entries)
}

/// Build the absolute URL for a file inside an HFS folder. HFS serves files
/// at the same path you list them under, so this is `<base><folder><name>`
/// with simple path-segment normalization.
pub fn file_url(base: &str, folder: &str, file_name: &str) -> String {
    let base = base.trim_end_matches('/');
    let folder = if folder.starts_with('/') {
        folder.to_string()
    } else {
        format!("/{folder}")
    };
    let folder = if folder.ends_with('/') {
        folder
    } else {
        format!("{folder}/")
    };
    format!("{base}{folder}{file_name}")
}

#[cfg(test)]
mod tests {
    use super::{HfsListEntry, file_url, parse_get_file_list_response};

    const URL: &str = "https://hoard.reaperaccessibility.com/~/api/get_file_list";

    #[test]
    fn parses_typical_hfs_listing() {
        let body = r#"{
            "list": [
                {"n": "JFRSCRIPTS_v3.18.zip", "s": 124533, "m": "2026-01-01T00:00:00Z"},
                {"n": "JFRSCRIPTS_v3.17.zip", "s": 119001, "m": "2025-12-01T00:00:00Z"},
                {"n": "old/", "s": null}
            ]
        }"#;
        let entries = parse_get_file_list_response(body, URL).unwrap();
        assert_eq!(
            entries,
            vec![
                HfsListEntry {
                    name: "JFRSCRIPTS_v3.18.zip".to_string(),
                    size: Some(124533),
                    is_directory: false,
                },
                HfsListEntry {
                    name: "JFRSCRIPTS_v3.17.zip".to_string(),
                    size: Some(119001),
                    is_directory: false,
                },
                HfsListEntry {
                    name: "old".to_string(),
                    size: None,
                    is_directory: true,
                },
            ]
        );
    }

    #[test]
    fn rejects_response_without_list_field() {
        let body = r#"{"error": "not-found"}"#;
        let error = parse_get_file_list_response(body, URL).unwrap_err();
        assert!(error.to_string().contains("missing array field: list"));
    }

    #[test]
    fn ignores_entries_without_name() {
        let body = r#"{"list": [{"s": 100}, {"n": "ok.zip"}]}"#;
        let entries = parse_get_file_list_response(body, URL).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "ok.zip");
    }

    #[test]
    fn builds_file_url_with_canonical_separators() {
        assert_eq!(
            file_url(
                "https://hoard.reaperaccessibility.com/",
                "jaws-for-reaper/",
                "JFRSCRIPTS_v3.18.zip",
            ),
            "https://hoard.reaperaccessibility.com/jaws-for-reaper/JFRSCRIPTS_v3.18.zip",
        );
        assert_eq!(
            file_url(
                "https://hoard.reaperaccessibility.com",
                "/jaws-for-reaper",
                "JFRSCRIPTS_v3.18.zip",
            ),
            "https://hoard.reaperaccessibility.com/jaws-for-reaper/JFRSCRIPTS_v3.18.zip",
        );
    }
}
