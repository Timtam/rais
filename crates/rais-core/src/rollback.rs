use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{IoPathContext, JsonPathContext, RaisError, Result};
use crate::hash::sha256_file;
use crate::preflight::{PreflightOptions, PreflightReport, run_install_preflight};

pub const BACKUP_MANIFEST_FILE: &str = "backup-manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSet {
    pub id: String,
    pub path: PathBuf,
    pub created_at: Option<String>,
    pub reason: Option<String>,
    pub manifest_path: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub rais_version: String,
    pub created_at: String,
    pub reason: String,
    pub files: Vec<BackupManifestFile>,
    pub receipt_backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifestFile {
    pub package_id: Option<String>,
    pub original_path: PathBuf,
    pub backup_path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreBackupOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreBackupReport {
    pub resource_path: PathBuf,
    pub backup_id: String,
    pub backup_path: PathBuf,
    pub dry_run: bool,
    pub preflight: PreflightReport,
    pub actions: Vec<RestoreBackupAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreBackupAction {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub current_backup_path: Option<PathBuf>,
    pub action: RestoreBackupActionKind,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreBackupActionKind {
    WouldRestore,
    Restored,
}

pub fn list_backup_sets(resource_path: &Path) -> Result<Vec<BackupSet>> {
    let backups_root = backups_root(resource_path);
    if !backups_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut sets = Vec::new();
    for entry in fs::read_dir(&backups_root).with_path(&backups_root)? {
        let entry = entry.with_path(&backups_root)?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToString::to_string)
        else {
            continue;
        };
        let manifest = load_backup_manifest(&path)?;
        let mut files = Vec::new();
        collect_relative_files(&path, &path, &mut files)?;
        files.sort();
        sets.push(BackupSet {
            id,
            manifest_path: manifest.as_ref().map(|_| path.join(BACKUP_MANIFEST_FILE)),
            created_at: manifest
                .as_ref()
                .map(|manifest| manifest.created_at.clone()),
            reason: manifest.as_ref().map(|manifest| manifest.reason.clone()),
            path,
            files,
        });
    }

    sets.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sets)
}

pub fn restore_backup_set(
    resource_path: &Path,
    backup_id: &str,
    options: &RestoreBackupOptions,
) -> Result<RestoreBackupReport> {
    let backup_path = backup_set_path(resource_path, backup_id)?;
    if !backup_path.is_dir() {
        return Err(RaisError::BackupNotFound(backup_path));
    }

    let preflight = run_install_preflight(
        resource_path,
        &PreflightOptions {
            dry_run: options.dry_run,
            allow_reaper_running: options.allow_reaper_running,
        },
    );
    if !preflight.passed {
        return Err(RaisError::PreflightFailed {
            message: preflight.failure_message(),
        });
    }

    let mut relative_files = Vec::new();
    collect_relative_files(&backup_path, &backup_path, &mut relative_files)?;
    relative_files.sort();

    let restore_timestamp = restore_timestamp();
    let mut actions = Vec::new();
    for relative_path in relative_files {
        let source_path = backup_path.join(&relative_path);
        let target_path = resource_path.join(&relative_path);
        let current_backup_path = if target_path.is_file() {
            Some(
                backups_root(resource_path)
                    .join(format!("{restore_timestamp}-before-restore"))
                    .join(&relative_path),
            )
        } else {
            None
        };
        let size = fs::metadata(&source_path).with_path(&source_path)?.len();
        let sha256 = sha256_file(&source_path)?;

        actions.push(RestoreBackupAction {
            source_path,
            target_path,
            current_backup_path,
            action: if options.dry_run {
                RestoreBackupActionKind::WouldRestore
            } else {
                RestoreBackupActionKind::Restored
            },
            size,
            sha256,
        });
    }

    if !options.dry_run {
        for action in &actions {
            restore_file(action)?;
        }
    }

    Ok(RestoreBackupReport {
        resource_path: resource_path.to_path_buf(),
        backup_id: backup_id.to_string(),
        backup_path,
        dry_run: options.dry_run,
        preflight,
        actions,
    })
}

fn restore_file(action: &RestoreBackupAction) -> Result<()> {
    if let Some(parent) = action.target_path.parent() {
        fs::create_dir_all(parent).with_path(parent)?;
    }

    let temp_path = temporary_restore_path(&action.target_path);
    if temp_path.exists() {
        fs::remove_file(&temp_path).with_path(&temp_path)?;
    }

    fs::copy(&action.source_path, &temp_path).with_path(&temp_path)?;
    let staged_hash = sha256_file(&temp_path)?;
    if staged_hash != action.sha256 {
        let _ = fs::remove_file(&temp_path);
        return Err(RaisError::HashMismatch {
            path: temp_path,
            expected: action.sha256.clone(),
            actual: staged_hash,
        });
    }

    if let Some(current_backup_path) = &action.current_backup_path {
        if let Some(parent) = current_backup_path.parent() {
            fs::create_dir_all(parent).with_path(parent)?;
        }
        fs::copy(&action.target_path, current_backup_path).with_path(current_backup_path)?;
    }

    if action.target_path.exists() {
        fs::remove_file(&action.target_path).with_path(&action.target_path)?;
    }

    match fs::rename(&temp_path, &action.target_path) {
        Ok(()) => Ok(()),
        Err(source) => {
            if let Some(current_backup_path) = &action.current_backup_path {
                if current_backup_path.is_file() && !action.target_path.exists() {
                    let _ = fs::copy(current_backup_path, &action.target_path);
                }
            }
            let _ = fs::remove_file(&temp_path);
            Err(RaisError::Io {
                path: action.target_path.clone(),
                source,
            })
        }
    }
}

fn collect_relative_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current).with_path(current)? {
        let entry = entry.with_path(current)?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(root, &path, files)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| RaisError::BackupNotFound(root.to_path_buf()))?;
            if relative == Path::new(BACKUP_MANIFEST_FILE) {
                continue;
            }
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

pub fn save_backup_manifest(backup_set: &Path, manifest: &BackupManifest) -> Result<PathBuf> {
    fs::create_dir_all(backup_set).with_path(backup_set)?;
    let path = backup_set.join(BACKUP_MANIFEST_FILE);
    let content = serde_json::to_string_pretty(manifest).with_json_path(&path)?;
    fs::write(&path, content).with_path(&path)?;
    Ok(path)
}

fn load_backup_manifest(backup_set: &Path) -> Result<Option<BackupManifest>> {
    let path = backup_set.join(BACKUP_MANIFEST_FILE);
    if !path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path).with_path(&path)?;
    let manifest = serde_json::from_str(&content).with_json_path(&path)?;
    Ok(Some(manifest))
}

fn backup_set_path(resource_path: &Path, backup_id: &str) -> Result<PathBuf> {
    if backup_id.is_empty()
        || backup_id == "."
        || backup_id == ".."
        || backup_id.contains('/')
        || backup_id.contains('\\')
    {
        return Err(RaisError::InvalidBackupId(backup_id.to_string()));
    }

    Ok(backups_root(resource_path).join(backup_id))
}

fn backups_root(resource_path: &Path) -> PathBuf {
    resource_path.join("RAIS").join("backups")
}

fn temporary_restore_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("restore");
    target_path.with_file_name(format!("{file_name}.rais-restore-tmp"))
}

fn restore_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("restore-unix-{seconds}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        BackupManifest, BackupManifestFile, RestoreBackupActionKind, RestoreBackupOptions,
        list_backup_sets, restore_backup_set, save_backup_manifest,
    };
    use crate::hash::sha256_file;

    #[test]
    fn lists_backup_sets_with_relative_files() {
        let dir = tempdir().unwrap();
        let backup_file = dir
            .path()
            .join("RAIS/backups/unix-1/UserPlugins/reaper_reapack-x64.dll");
        fs::create_dir_all(backup_file.parent().unwrap()).unwrap();
        fs::write(&backup_file, b"old").unwrap();
        save_backup_manifest(
            &dir.path().join("RAIS/backups/unix-1"),
            &BackupManifest {
                schema_version: 1,
                rais_version: "0.1.0".to_string(),
                created_at: "unix-1".to_string(),
                reason: "install-replacement".to_string(),
                files: vec![BackupManifestFile {
                    package_id: Some("reapack".to_string()),
                    original_path: std::path::PathBuf::from("UserPlugins/reaper_reapack-x64.dll"),
                    backup_path: backup_file.clone(),
                    size: 3,
                    sha256: sha256_file(&backup_file).unwrap(),
                }],
                receipt_backup_path: None,
            },
        )
        .unwrap();

        let sets = list_backup_sets(dir.path()).unwrap();

        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].id, "unix-1");
        assert_eq!(sets[0].created_at.as_deref(), Some("unix-1"));
        assert_eq!(sets[0].reason.as_deref(), Some("install-replacement"));
        assert!(sets[0].manifest_path.is_some());
        assert_eq!(
            sets[0].files[0],
            std::path::PathBuf::from("UserPlugins/reaper_reapack-x64.dll")
        );
        assert_eq!(sets[0].files.len(), 1);
    }

    #[test]
    fn dry_run_reports_restore_without_writing() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("UserPlugins/reaper_reapack-x64.dll");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"new").unwrap();
        let backup_file = dir
            .path()
            .join("RAIS/backups/unix-1/UserPlugins/reaper_reapack-x64.dll");
        fs::create_dir_all(backup_file.parent().unwrap()).unwrap();
        fs::write(&backup_file, b"old").unwrap();

        let report = restore_backup_set(
            dir.path(),
            "unix-1",
            &RestoreBackupOptions {
                dry_run: true,
                allow_reaper_running: false,
            },
        )
        .unwrap();

        assert_eq!(
            report.actions[0].action,
            RestoreBackupActionKind::WouldRestore
        );
        assert_eq!(fs::read(target).unwrap(), b"new");
    }

    #[test]
    fn restore_replaces_target_and_backs_up_current_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("UserPlugins/reaper_reapack-x64.dll");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"new").unwrap();
        let backup_file = dir
            .path()
            .join("RAIS/backups/unix-1/UserPlugins/reaper_reapack-x64.dll");
        fs::create_dir_all(backup_file.parent().unwrap()).unwrap();
        fs::write(&backup_file, b"old").unwrap();

        let report = restore_backup_set(
            dir.path(),
            "unix-1",
            &RestoreBackupOptions {
                dry_run: false,
                allow_reaper_running: true,
            },
        )
        .unwrap();

        assert_eq!(report.actions[0].action, RestoreBackupActionKind::Restored);
        assert_eq!(fs::read(&target).unwrap(), b"old");
        let current_backup = report.actions[0].current_backup_path.as_ref().unwrap();
        assert_eq!(fs::read(current_backup).unwrap(), b"new");
        assert!(
            !target
                .with_file_name("reaper_reapack-x64.dll.rais-restore-tmp")
                .exists()
        );
    }

    #[test]
    fn restore_can_restore_receipt_backup() {
        let dir = tempdir().unwrap();
        let receipt = dir.path().join("RAIS/install-state.json");
        fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        fs::write(
            &receipt,
            br#"{"schema_version":1,"packages":{"reapack":"new"}}"#,
        )
        .unwrap();
        let backup_receipt = dir
            .path()
            .join("RAIS/backups/unix-1/RAIS/install-state.json");
        fs::create_dir_all(backup_receipt.parent().unwrap()).unwrap();
        fs::write(
            &backup_receipt,
            br#"{"schema_version":1,"packages":{"reapack":"old"}}"#,
        )
        .unwrap();

        let report = restore_backup_set(
            dir.path(),
            "unix-1",
            &RestoreBackupOptions {
                dry_run: false,
                allow_reaper_running: true,
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&receipt).unwrap(),
            r#"{"schema_version":1,"packages":{"reapack":"old"}}"#
        );
        assert!(report.actions[0].current_backup_path.is_some());
    }
}
