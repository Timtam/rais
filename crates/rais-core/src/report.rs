use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{IoPathContext, JsonPathContext, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedJsonReport {
    pub schema_version: u32,
    pub rais_version: String,
    pub created_at: String,
    pub report: serde_json::Value,
}

pub fn save_json_report<T>(path: &Path, report: &T) -> Result<SavedJsonReport>
where
    T: Serialize + ?Sized,
{
    let envelope = SavedJsonReport {
        schema_version: 1,
        rais_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: report_timestamp(),
        report: serde_json::to_value(report).with_json_path(path)?,
    };

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_path(parent)?;
    }

    let content = serde_json::to_string_pretty(&envelope).with_json_path(path)?;
    fs::write(path, content).with_path(path)?;
    Ok(envelope)
}

pub fn default_report_path(resource_path: &Path, operation_name: &str) -> PathBuf {
    resource_path
        .join("RAIS")
        .join("logs")
        .join(format!("{operation_name}-{}.json", report_timestamp()))
}

fn report_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix-{seconds}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::save_json_report;

    #[test]
    fn saves_report_envelope_to_nested_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("RAIS/logs/report.json");

        let saved = save_json_report(&path, &json!({"ok": true})).unwrap();
        assert_eq!(saved.schema_version, 1);
        assert_eq!(saved.report["ok"], true);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"rais_version\""));
        assert!(content.contains("\"report\""));
    }
}
