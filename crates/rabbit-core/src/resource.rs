use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{IoPathContext, RabbitError, Result};
use crate::preflight::{PreflightOptions, PreflightReport, run_install_preflight};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceInitOptions {
    pub dry_run: bool,
    pub portable: bool,
    pub include_extension_support_dirs: bool,
    pub allow_reaper_running: bool,
    pub target_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceInitReport {
    pub resource_path: PathBuf,
    pub dry_run: bool,
    pub portable: bool,
    pub preflight: PreflightReport,
    pub actions: Vec<ResourceInitAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceInitAction {
    pub path: PathBuf,
    pub kind: ResourceInitItemKind,
    pub action: ResourceInitActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceInitItemKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceInitActionKind {
    WouldCreate,
    Created,
    AlreadyExists,
}

pub fn initialize_resource_path(
    resource_path: &Path,
    options: &ResourceInitOptions,
) -> Result<ResourceInitReport> {
    let preflight = run_install_preflight(
        resource_path,
        &PreflightOptions {
            dry_run: options.dry_run,
            allow_reaper_running: options.allow_reaper_running,
            target_app_path: options.target_app_path.clone(),
        },
    );
    if !preflight.passed {
        return Err(RabbitError::PreflightFailed {
            message: preflight.failure_message(),
        });
    }

    let mut report = ResourceInitReport {
        resource_path: resource_path.to_path_buf(),
        dry_run: options.dry_run,
        portable: options.portable,
        preflight,
        actions: Vec::new(),
    };

    for directory in resource_directories(resource_path, options.include_extension_support_dirs) {
        report
            .actions
            .push(ensure_directory(&directory, options.dry_run)?);
    }

    if options.portable {
        report.actions.push(ensure_empty_file(
            &resource_path.join("reaper.ini"),
            options.dry_run,
        )?);
    }

    Ok(report)
}

fn resource_directories(
    resource_path: &Path,
    include_extension_support_dirs: bool,
) -> Vec<PathBuf> {
    let mut directories = vec![
        resource_path.to_path_buf(),
        resource_path.join("RABBIT"),
        resource_path.join("RABBIT").join("logs"),
        resource_path.join("RABBIT").join("backups"),
    ];

    if include_extension_support_dirs {
        directories.push(resource_path.join("UserPlugins"));
        directories.push(resource_path.join("KeyMaps"));
    }

    directories
}

fn ensure_directory(path: &Path, dry_run: bool) -> Result<ResourceInitAction> {
    if path.exists() {
        if !path.is_dir() {
            return Err(RabbitError::PreflightFailed {
                message: format!("{} exists but is not a directory.", path.display()),
            });
        }
        return Ok(ResourceInitAction {
            path: path.to_path_buf(),
            kind: ResourceInitItemKind::Directory,
            action: ResourceInitActionKind::AlreadyExists,
        });
    }

    if dry_run {
        return Ok(ResourceInitAction {
            path: path.to_path_buf(),
            kind: ResourceInitItemKind::Directory,
            action: ResourceInitActionKind::WouldCreate,
        });
    }

    fs::create_dir_all(path).with_path(path)?;
    Ok(ResourceInitAction {
        path: path.to_path_buf(),
        kind: ResourceInitItemKind::Directory,
        action: ResourceInitActionKind::Created,
    })
}

fn ensure_empty_file(path: &Path, dry_run: bool) -> Result<ResourceInitAction> {
    if path.exists() {
        if !path.is_file() {
            return Err(RabbitError::PreflightFailed {
                message: format!("{} exists but is not a file.", path.display()),
            });
        }
        return Ok(ResourceInitAction {
            path: path.to_path_buf(),
            kind: ResourceInitItemKind::File,
            action: ResourceInitActionKind::AlreadyExists,
        });
    }

    if dry_run {
        return Ok(ResourceInitAction {
            path: path.to_path_buf(),
            kind: ResourceInitItemKind::File,
            action: ResourceInitActionKind::WouldCreate,
        });
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_path(parent)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_path(path)?;

    Ok(ResourceInitAction {
        path: path.to_path_buf(),
        kind: ResourceInitItemKind::File,
        action: ResourceInitActionKind::Created,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ResourceInitActionKind, ResourceInitOptions, initialize_resource_path};

    #[test]
    fn dry_run_reports_layout_without_creating_it() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = initialize_resource_path(
            &resource_path,
            &ResourceInitOptions {
                dry_run: true,
                portable: true,
                include_extension_support_dirs: true,
                allow_reaper_running: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(
            report
                .actions
                .iter()
                .all(|action| action.action == ResourceInitActionKind::WouldCreate)
        );
        assert!(!resource_path.exists());
    }

    #[test]
    fn creates_portable_resource_layout() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = initialize_resource_path(
            &resource_path,
            &ResourceInitOptions {
                dry_run: false,
                portable: true,
                include_extension_support_dirs: true,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(
            report
                .actions
                .iter()
                .any(|action| action.action == ResourceInitActionKind::Created)
        );
        assert!(resource_path.join("reaper.ini").is_file());
        assert!(resource_path.join("UserPlugins").is_dir());
        assert!(resource_path.join("KeyMaps").is_dir());
        assert!(resource_path.join("RABBIT/logs").is_dir());
        assert!(resource_path.join("RABBIT/backups").is_dir());
    }

    #[test]
    fn refuses_to_initialize_when_resource_path_is_a_file() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("REAPER");
        fs::write(&resource_path, b"not a directory").unwrap();

        let error = initialize_resource_path(
            &resource_path,
            &ResourceInitOptions {
                dry_run: false,
                portable: false,
                include_extension_support_dirs: true,
                allow_reaper_running: true,
                target_app_path: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("not a directory"));
    }

    #[test]
    fn standard_reaper_only_layout_stays_minimal() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("REAPER");

        let report = initialize_resource_path(
            &resource_path,
            &ResourceInitOptions {
                dry_run: false,
                portable: false,
                include_extension_support_dirs: false,
                allow_reaper_running: true,
                target_app_path: Some(
                    dir.path()
                        .join("Program Files")
                        .join("REAPER")
                        .join("reaper.exe"),
                ),
            },
        )
        .unwrap();

        assert!(resource_path.is_dir());
        assert!(resource_path.join("RABBIT/logs").is_dir());
        assert!(resource_path.join("RABBIT/backups").is_dir());
        assert!(!resource_path.join("UserPlugins").exists());
        assert!(!resource_path.join("KeyMaps").exists());
        assert!(!resource_path.join("reaper.ini").exists());
        assert!(report.actions.iter().all(
            |action| !action.path.ends_with("UserPlugins") && !action.path.ends_with("KeyMaps")
        ));
    }
}
