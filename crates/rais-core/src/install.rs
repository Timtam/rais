use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::artifact::{ArtifactKind, CachedArtifact};
use crate::error::{IoPathContext, RaisError, Result};
use crate::hash::sha256_file;
use crate::preflight::{PreflightOptions, PreflightReport, run_install_preflight};
use crate::receipt::{
    RECEIPT_RELATIVE_PATH, load_install_state, receipt_path, save_install_state,
    upsert_package_receipt,
};
use crate::rollback::{BackupManifest, BackupManifestFile, save_backup_manifest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub target_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallReport {
    pub resource_path: PathBuf,
    pub dry_run: bool,
    pub preflight: PreflightReport,
    pub receipt_written: bool,
    pub receipt_backup_path: Option<PathBuf>,
    pub backup_manifest_path: Option<PathBuf>,
    pub actions: Vec<InstallFileReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallFileReport {
    pub package_id: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub action: InstallFileAction,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallFileAction {
    WouldInstall,
    WouldReplace,
    WouldKeep,
    Installed,
    Replaced,
    Kept,
}

pub fn install_cached_artifacts(
    resource_path: &Path,
    artifacts: &[CachedArtifact],
    options: &InstallOptions,
) -> Result<InstallReport> {
    let preflight = run_install_preflight(
        resource_path,
        &PreflightOptions {
            dry_run: options.dry_run,
            allow_reaper_running: options.allow_reaper_running,
            target_app_path: options.target_app_path.clone(),
        },
    );
    if !preflight.passed {
        return Err(RaisError::PreflightFailed {
            message: preflight.failure_message(),
        });
    }

    let timestamp = install_timestamp();
    let mut report = InstallReport {
        resource_path: resource_path.to_path_buf(),
        dry_run: options.dry_run,
        preflight,
        receipt_written: false,
        receipt_backup_path: None,
        backup_manifest_path: None,
        actions: Vec::new(),
    };

    let mut state = load_install_state(resource_path)?.unwrap_or_default();
    let mut replacement_backup_set: Option<PathBuf> = None;

    for artifact in artifacts {
        if artifact.descriptor.kind != ArtifactKind::ExtensionBinary {
            return Err(RaisError::UnsupportedArtifactKind {
                package_id: artifact.descriptor.package_id.clone(),
                kind: artifact.descriptor.kind,
            });
        }

        let relative_target = PathBuf::from("UserPlugins").join(&artifact.descriptor.file_name);
        let target_path = resource_path.join(&relative_target);
        let target_exists = target_path.is_file();
        let target_matches = target_exists && sha256_file(&target_path)? == artifact.sha256;
        let backup_path = if target_exists && !target_matches {
            let backup_set = resource_path.join("RAIS").join("backups").join(&timestamp);
            replacement_backup_set.get_or_insert_with(|| backup_set.clone());
            Some(backup_set.join(&relative_target))
        } else {
            None
        };

        let action = classify_action(options.dry_run, target_exists, target_matches);
        report.actions.push(InstallFileReport {
            package_id: artifact.descriptor.package_id.clone(),
            source_path: artifact.path.clone(),
            target_path: target_path.clone(),
            backup_path: backup_path.clone(),
            action,
            size: artifact.size,
            sha256: artifact.sha256.clone(),
        });

        if options.dry_run {
            continue;
        }

        if !target_matches {
            install_extension_file(artifact, &target_path, backup_path.as_deref())?;
        }

        upsert_package_receipt(
            &mut state,
            resource_path,
            &artifact.descriptor.package_id,
            Some(artifact.descriptor.version.clone()),
            Some(artifact.descriptor.url.clone()),
            Some(artifact.sha256.clone()),
            &[target_path],
            Some(install_timestamp()),
            Some(artifact.descriptor.architecture),
        )?;
    }

    if !options.dry_run && !artifacts.is_empty() {
        if let Some(backup_set) = &replacement_backup_set {
            report.receipt_backup_path = backup_receipt_if_present(resource_path, backup_set)?;
            report.backup_manifest_path = Some(write_backup_manifest(
                backup_set,
                &timestamp,
                &report.actions,
                report.receipt_backup_path.as_ref(),
            )?);
        }
        save_install_state(resource_path, &state)?;
        report.receipt_written = true;
    } else if options.dry_run {
        if let Some(backup_set) = &replacement_backup_set {
            let source_path = receipt_path(resource_path);
            if source_path.is_file() {
                report.receipt_backup_path = Some(backup_set.join(RECEIPT_RELATIVE_PATH));
            }
            report.backup_manifest_path =
                Some(backup_set.join(crate::rollback::BACKUP_MANIFEST_FILE));
        }
    }

    Ok(report)
}

fn classify_action(dry_run: bool, target_exists: bool, target_matches: bool) -> InstallFileAction {
    match (dry_run, target_exists, target_matches) {
        (true, false, _) => InstallFileAction::WouldInstall,
        (true, true, true) => InstallFileAction::WouldKeep,
        (true, true, false) => InstallFileAction::WouldReplace,
        (false, false, _) => InstallFileAction::Installed,
        (false, true, true) => InstallFileAction::Kept,
        (false, true, false) => InstallFileAction::Replaced,
    }
}

fn install_extension_file(
    artifact: &CachedArtifact,
    target_path: &Path,
    backup_path: Option<&Path>,
) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).with_path(parent)?;
    }

    let temp_path = temporary_target_path(target_path);
    if temp_path.exists() {
        fs::remove_file(&temp_path).with_path(&temp_path)?;
    }

    fs::copy(&artifact.path, &temp_path).with_path(&temp_path)?;
    let staged_hash = sha256_file(&temp_path)?;
    if staged_hash != artifact.sha256 {
        let _ = fs::remove_file(&temp_path);
        return Err(RaisError::HashMismatch {
            path: temp_path,
            expected: artifact.sha256.clone(),
            actual: staged_hash,
        });
    }

    if let Some(backup_path) = backup_path {
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).with_path(parent)?;
        }
        fs::copy(target_path, backup_path).with_path(backup_path)?;
    }

    if target_path.exists() {
        fs::remove_file(target_path).with_path(target_path)?;
    }

    match fs::rename(&temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(source) => {
            if let Some(backup_path) = backup_path {
                if backup_path.is_file() && !target_path.exists() {
                    let _ = fs::copy(backup_path, target_path);
                }
            }
            let _ = fs::remove_file(&temp_path);
            Err(RaisError::Io {
                path: target_path.to_path_buf(),
                source,
            })
        }
    }
}

fn temporary_target_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("extension");
    target_path.with_file_name(format!("{file_name}.rais-tmp"))
}

fn backup_receipt_if_present(resource_path: &Path, backup_set: &Path) -> Result<Option<PathBuf>> {
    let source_path = receipt_path(resource_path);
    if !source_path.is_file() {
        return Ok(None);
    }

    let backup_path = backup_set.join(RECEIPT_RELATIVE_PATH);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).with_path(parent)?;
    }
    fs::copy(&source_path, &backup_path).with_path(&backup_path)?;
    Ok(Some(backup_path))
}

fn write_backup_manifest(
    backup_set: &Path,
    created_at: &str,
    actions: &[InstallFileReport],
    receipt_backup_path: Option<&PathBuf>,
) -> Result<PathBuf> {
    let mut files = Vec::new();
    for action in actions {
        let Some(backup_path) = &action.backup_path else {
            continue;
        };

        files.push(BackupManifestFile {
            package_id: Some(action.package_id.clone()),
            original_path: action.target_path.clone(),
            backup_path: backup_path.clone(),
            size: fs::metadata(backup_path).with_path(backup_path)?.len(),
            sha256: sha256_file(backup_path)?,
        });
    }

    let receipt_backup_path = receipt_backup_path.cloned();
    if let Some(path) = &receipt_backup_path {
        files.push(BackupManifestFile {
            package_id: None,
            original_path: PathBuf::from(RECEIPT_RELATIVE_PATH),
            backup_path: path.clone(),
            size: fs::metadata(path).with_path(path)?.len(),
            sha256: sha256_file(path)?,
        });
    }

    save_backup_manifest(
        backup_set,
        &BackupManifest {
            schema_version: 1,
            rais_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: created_at.to_string(),
            reason: "install-replacement".to_string(),
            files,
            receipt_backup_path,
        },
    )
}

fn install_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix-{seconds}")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{InstallFileAction, InstallOptions, install_cached_artifacts};
    use crate::artifact::{ArtifactDescriptor, ArtifactKind, CachedArtifact};
    use crate::error::RaisError;
    use crate::hash::sha256_file;
    use crate::model::{Architecture, Platform};
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK};
    use crate::receipt::{
        InstallState, InstalledFileReceipt, PackageReceipt, load_install_state, save_install_state,
    };
    use crate::version::Version;

    #[test]
    fn installs_extension_binary_and_writes_receipt() {
        let dir = tempdir().unwrap();
        let artifact = cached_artifact(
            dir.path(),
            PACKAGE_REAPACK,
            "reaper_reapack-x64.dll",
            b"new",
        );

        let report = install_cached_artifacts(
            dir.path(),
            &[artifact],
            &InstallOptions {
                dry_run: false,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.actions[0].action, InstallFileAction::Installed);
        assert!(
            dir.path()
                .join("UserPlugins/reaper_reapack-x64.dll")
                .is_file()
        );

        let state = load_install_state(dir.path()).unwrap().unwrap();
        assert!(state.packages.contains_key(PACKAGE_REAPACK));
    }

    #[test]
    fn backs_up_existing_extension_before_replacing() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(plugins.join("reaper_reapack-x64.dll"), b"old").unwrap();
        let mut packages = BTreeMap::new();
        packages.insert(
            PACKAGE_REAPACK.to_string(),
            PackageReceipt {
                id: PACKAGE_REAPACK.to_string(),
                version: Some(Version::parse("1.2.5").unwrap()),
                source_url: Some("https://example.test/old.dll".to_string()),
                source_sha256: Some(sha256_file(&plugins.join("reaper_reapack-x64.dll")).unwrap()),
                installed_files: vec![InstalledFileReceipt {
                    path: PathBuf::from("UserPlugins/reaper_reapack-x64.dll"),
                    sha256: None,
                    size: Some(3),
                }],
                installed_at: Some("unix-old".to_string()),
                rais_version: Some("0.1.0".to_string()),
                architecture: Some(Architecture::X64),
            },
        );
        save_install_state(
            dir.path(),
            &InstallState {
                schema_version: 1,
                packages,
            },
        )
        .unwrap();
        let artifact = cached_artifact(
            dir.path(),
            PACKAGE_REAPACK,
            "reaper_reapack-x64.dll",
            b"new",
        );

        let report = install_cached_artifacts(
            dir.path(),
            &[artifact],
            &InstallOptions {
                dry_run: false,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.actions[0].action, InstallFileAction::Replaced);
        let backup = report.actions[0].backup_path.as_ref().unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"old");
        let receipt_backup = report.receipt_backup_path.as_ref().unwrap();
        let backed_up_state: InstallState =
            serde_json::from_str(&fs::read_to_string(receipt_backup).unwrap()).unwrap();
        assert_eq!(
            backed_up_state.packages[PACKAGE_REAPACK]
                .version
                .as_ref()
                .unwrap()
                .raw(),
            "1.2.5"
        );
        let backup_manifest_path = report.backup_manifest_path.as_ref().unwrap();
        let backup_manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(backup_manifest_path).unwrap()).unwrap();
        assert_eq!(backup_manifest["reason"], "install-replacement");
        assert_eq!(backup_manifest["files"].as_array().unwrap().len(), 2);
        assert_eq!(
            fs::read(plugins.join("reaper_reapack-x64.dll")).unwrap(),
            b"new"
        );
        let current_state = load_install_state(dir.path()).unwrap().unwrap();
        assert_eq!(
            current_state.packages[PACKAGE_REAPACK]
                .version
                .as_ref()
                .unwrap()
                .raw(),
            "1.2.6"
        );
        assert!(!plugins.join("reaper_reapack-x64.dll.rais-tmp").exists());
    }

    #[test]
    fn dry_run_does_not_write_files_or_receipts() {
        let dir = tempdir().unwrap();
        let artifact = cached_artifact(
            dir.path(),
            PACKAGE_REAPACK,
            "reaper_reapack-x64.dll",
            b"new",
        );

        let report = install_cached_artifacts(
            dir.path(),
            &[artifact],
            &InstallOptions {
                dry_run: true,
                allow_reaper_running: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.actions[0].action, InstallFileAction::WouldInstall);
        assert!(
            !dir.path()
                .join("UserPlugins/reaper_reapack-x64.dll")
                .exists()
        );
        assert!(load_install_state(dir.path()).unwrap().is_none());
    }

    #[test]
    fn rejects_non_extension_binary_artifacts() {
        let dir = tempdir().unwrap();
        let mut artifact = cached_artifact(dir.path(), PACKAGE_OSARA, "osara.exe", b"installer");
        artifact.descriptor.kind = ArtifactKind::Installer;

        let error = install_cached_artifacts(
            dir.path(),
            &[artifact],
            &InstallOptions {
                dry_run: false,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("not supported"));
    }

    #[test]
    fn hash_mismatch_does_not_replace_existing_extension() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        let target = plugins.join("reaper_reapack-x64.dll");
        fs::write(&target, b"old").unwrap();
        let mut artifact = cached_artifact(
            dir.path(),
            PACKAGE_REAPACK,
            "reaper_reapack-x64.dll",
            b"new",
        );
        artifact.sha256 = "wrong-hash".to_string();

        let error = install_cached_artifacts(
            dir.path(),
            &[artifact],
            &InstallOptions {
                dry_run: false,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap_err();

        assert!(matches!(error, RaisError::HashMismatch { .. }));
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!plugins.join("reaper_reapack-x64.dll.rais-tmp").exists());
    }

    fn cached_artifact(
        root: &std::path::Path,
        package_id: &str,
        file_name: &str,
        contents: &[u8],
    ) -> CachedArtifact {
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let path = cache_dir.join(file_name);
        fs::write(&path, contents).unwrap();
        let sha256 = sha256_file(&path).unwrap();

        CachedArtifact {
            descriptor: ArtifactDescriptor {
                package_id: package_id.to_string(),
                version: Version::parse("1.2.6").unwrap(),
                platform: Platform::Windows,
                architecture: Architecture::X64,
                kind: ArtifactKind::ExtensionBinary,
                url: format!("https://example.test/{file_name}"),
                file_name: file_name.to_string(),
            },
            path,
            size: contents.len() as u64,
            sha256,
            reused_existing_file: false,
        }
    }
}
