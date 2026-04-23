use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{IoPathContext, JsonPathContext, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedJsonReport {
    pub schema_version: u32,
    pub rais_version: String,
    pub created_at: String,
    pub report: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedReportPaths {
    pub json_path: PathBuf,
    pub text_path: PathBuf,
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

pub fn save_json_and_text_reports<T>(json_path: &Path, report: &T) -> Result<SavedReportPaths>
where
    T: Serialize + ?Sized,
{
    let envelope = save_json_report(json_path, report)?;
    let text_path = text_report_path_for_json_path(json_path);
    save_text_report(&text_path, &envelope)?;
    Ok(SavedReportPaths {
        json_path: json_path.to_path_buf(),
        text_path,
    })
}

pub fn default_report_path(resource_path: &Path, operation_name: &str) -> PathBuf {
    resource_path
        .join("RAIS")
        .join("logs")
        .join(format!("{operation_name}-{}.json", report_timestamp()))
}

pub fn text_report_path_for_json_path(json_path: &Path) -> PathBuf {
    let is_text_path = json_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"));
    if is_text_path {
        json_path.with_extension("report.txt")
    } else {
        json_path.with_extension("txt")
    }
}

fn report_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix-{seconds}")
}

fn save_text_report(path: &Path, envelope: &SavedJsonReport) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_path(parent)?;
    }

    let content = render_text_report(envelope);
    fs::write(path, content).with_path(path)?;
    Ok(())
}

fn render_text_report(envelope: &SavedJsonReport) -> String {
    let mut lines = vec![
        "RAIS Report".to_string(),
        format!("Schema version: {}", envelope.schema_version),
        format!("RAIS version: {}", envelope.rais_version),
        format!("Created at: {}", envelope.created_at),
        String::new(),
        "Report".to_string(),
    ];
    render_named_value("report", &envelope.report, 0, &mut lines);
    lines.join("\n")
}

fn render_named_value(name: &str, value: &Value, indent: usize, lines: &mut Vec<String>) {
    let prefix = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            lines.push(format!("{prefix}{name}:"));
            if map.is_empty() {
                lines.push(format!("{prefix}  {{}}"));
                return;
            }
            for (key, child) in map {
                render_named_value(key, child, indent + 2, lines);
            }
        }
        Value::Array(items) => {
            lines.push(format!("{prefix}{name}:"));
            if items.is_empty() {
                lines.push(format!("{prefix}  []"));
                return;
            }
            for item in items {
                render_list_item(item, indent + 2, lines);
            }
        }
        _ => lines.push(format!("{prefix}{name}: {}", scalar_text(value))),
    }
}

fn render_list_item(value: &Value, indent: usize, lines: &mut Vec<String>) {
    let prefix = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                lines.push(format!("{prefix}- {{}}"));
                return;
            }
            let mut first = true;
            for (key, child) in map {
                if first {
                    match child {
                        Value::Object(_) | Value::Array(_) => {
                            lines.push(format!("{prefix}- {key}:"));
                            render_nested_value(child, indent + 4, lines);
                        }
                        _ => lines.push(format!("{prefix}- {key}: {}", scalar_text(child))),
                    }
                    first = false;
                } else {
                    render_named_value(key, child, indent + 2, lines);
                }
            }
        }
        Value::Array(items) => {
            lines.push(format!("{prefix}-"));
            if items.is_empty() {
                lines.push(format!("{prefix}  []"));
                return;
            }
            for item in items {
                render_list_item(item, indent + 2, lines);
            }
        }
        _ => lines.push(format!("{prefix}- {}", scalar_text(value))),
    }
}

fn render_nested_value(value: &Value, indent: usize, lines: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                lines.push(format!("{}{{}}", " ".repeat(indent)));
                return;
            }
            for (key, child) in map {
                render_named_value(key, child, indent, lines);
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                lines.push(format!("{}[]", " ".repeat(indent)));
                return;
            }
            for item in items {
                render_list_item(item, indent, lines);
            }
        }
        _ => lines.push(format!("{}{}", " ".repeat(indent), scalar_text(value))),
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => unreachable!("scalar_text only supports scalars"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{save_json_and_text_reports, save_json_report, text_report_path_for_json_path};

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

    #[test]
    fn saves_adjacent_text_report_with_plain_content() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("RAIS/logs/report.json");

        let saved = save_json_and_text_reports(
            &json_path,
            &json!({
                "ok": true,
                "items": [{"name": "OSARA", "status": "manual"}]
            }),
        )
        .unwrap();

        assert_eq!(saved.json_path, json_path);
        assert_eq!(
            saved.text_path,
            text_report_path_for_json_path(&saved.json_path)
        );
        assert!(saved.text_path.is_file());
        let content = std::fs::read_to_string(&saved.text_path).unwrap();
        assert!(content.contains("RAIS Report"));
        assert!(content.contains("report:"));
        assert!(content.contains("ok: true"));
        assert!(content.contains("- name: OSARA"));
    }

    #[test]
    fn avoids_text_report_path_collision_for_txt_input_path() {
        let path = PathBuf::from("C:/temp/report.txt");

        let text_path = text_report_path_for_json_path(&path);

        assert_eq!(text_path, PathBuf::from("C:/temp/report.report.txt"));
    }
}
