use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

use crate::detection::{DiscoveryOptions, discover_installations};
use crate::error::{RaisError, Result};
use crate::model::Platform;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub target_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightReport {
    pub passed: bool,
    pub checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    pub fn failure_message(&self) -> String {
        self.checks
            .iter()
            .filter(|check| check.status == PreflightStatus::Fail)
            .map(|check| format!("{}: {}", check.name, check.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightCheck {
    pub name: String,
    pub status: PreflightStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreflightStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningProcess {
    pub pid: String,
    pub name: String,
    pub executable_path: Option<PathBuf>,
}

pub fn run_install_preflight(resource_path: &Path, options: &PreflightOptions) -> PreflightReport {
    run_install_preflight_with_processes(
        resource_path,
        options,
        &running_reaper_processes(Platform::current()),
    )
}

pub fn run_install_preflight_with_processes(
    resource_path: &Path,
    options: &PreflightOptions,
    running_processes: &[RunningProcess],
) -> PreflightReport {
    let target_app_path =
        effective_target_app_path(resource_path, options.target_app_path.as_deref());
    let relevant_processes =
        relevant_running_processes(resource_path, running_processes, target_app_path.as_deref());
    let mut checks = vec![resource_path_check(resource_path, options.dry_run)];
    checks.push(reaper_process_check(
        &relevant_processes,
        options.allow_reaper_running || options.dry_run,
    ));

    let passed = checks
        .iter()
        .all(|check| check.status != PreflightStatus::Fail);
    PreflightReport { passed, checks }
}

pub fn ensure_resource_path_ready(resource_path: &Path, dry_run: bool) -> Result<()> {
    let check = resource_path_check(resource_path, dry_run);
    if check.status == PreflightStatus::Fail {
        return Err(RaisError::PreflightFailed {
            message: format!("{}: {}", check.name, check.message),
        });
    }
    Ok(())
}

pub fn running_reaper_processes(platform: Option<Platform>) -> Vec<RunningProcess> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let name = process.name().to_string_lossy().to_string();
            if is_reaper_process_name(platform, &name) {
                Some(RunningProcess {
                    pid: pid.to_string(),
                    name,
                    executable_path: process.exe().map(Path::to_path_buf),
                })
            } else {
                None
            }
        })
        .collect()
}

fn effective_target_app_path(
    resource_path: &Path,
    explicit_target_app_path: Option<&Path>,
) -> Option<PathBuf> {
    explicit_target_app_path
        .map(Path::to_path_buf)
        .or_else(|| portable_target_app_path(resource_path, Platform::current()))
        .or_else(|| detected_standard_app_path(resource_path))
}

fn portable_target_app_path(resource_path: &Path, platform: Option<Platform>) -> Option<PathBuf> {
    match platform {
        Some(Platform::Windows) => {
            let app_path = resource_path.join("reaper.exe");
            app_path.is_file().then_some(app_path)
        }
        Some(Platform::MacOs) => fs::read_dir(resource_path)
            .ok()?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.to_ascii_lowercase().contains("reaper"))
            }),
        None => None,
    }
}

fn detected_standard_app_path(resource_path: &Path) -> Option<PathBuf> {
    discover_installations(&DiscoveryOptions {
        include_standard: true,
        portable_roots: Vec::new(),
    })
    .ok()?
    .into_iter()
    .find(|installation| installation.resource_path == resource_path)
    .map(|installation| installation.app_path)
}

fn relevant_running_processes(
    resource_path: &Path,
    running_processes: &[RunningProcess],
    target_app_path: Option<&Path>,
) -> Vec<RunningProcess> {
    match target_app_path {
        Some(target_app_path) => running_processes
            .iter()
            .filter(|process| process_matches_target(process, target_app_path))
            .cloned()
            .collect(),
        None if is_distinct_portable_like_resource_path(resource_path) => running_processes
            .iter()
            .filter(|process| process_runs_within_resource_path(process, resource_path))
            .cloned()
            .collect(),
        None => running_processes.to_vec(),
    }
}

fn process_matches_target(process: &RunningProcess, target_app_path: &Path) -> bool {
    let Some(process_path) = process.executable_path.as_deref() else {
        return false;
    };

    paths_match_target(process_path, target_app_path)
}

fn process_runs_within_resource_path(process: &RunningProcess, resource_path: &Path) -> bool {
    let Some(process_path) = process.executable_path.as_deref() else {
        return false;
    };

    let process_path = normalize_path_for_match(process_path);
    let resource_path = normalize_path_for_match(resource_path);
    process_path.starts_with(resource_path)
}

fn paths_match_target(process_path: &Path, target_app_path: &Path) -> bool {
    let process_path = normalize_path_for_match(process_path);
    let target_app_path = normalize_path_for_match(target_app_path);

    same_path(&process_path, &target_app_path)
        || (is_app_bundle(&target_app_path) && process_path.starts_with(&target_app_path))
}

fn normalize_path_for_match(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn same_path(left: &Path, right: &Path) -> bool {
    if cfg!(target_os = "windows") {
        normalize_windows_path(left) == normalize_windows_path(right)
    } else {
        left == right
    }
}

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn is_distinct_portable_like_resource_path(resource_path: &Path) -> bool {
    let Some(standard_resource_path) = standard_resource_path(Platform::current()) else {
        return false;
    };

    !same_path(resource_path, &standard_resource_path)
}

fn standard_resource_path(platform: Option<Platform>) -> Option<PathBuf> {
    match platform {
        Some(Platform::Windows) => {
            rais_platform::user_appdata_dir().map(|path| path.join("REAPER"))
        }
        Some(Platform::MacOs) => rais_platform::user_home_dir().map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("REAPER")
        }),
        None => None,
    }
}

fn resource_path_check(resource_path: &Path, dry_run: bool) -> PreflightCheck {
    let nearest = nearest_existing_ancestor(resource_path);
    let Some(existing_path) = nearest else {
        return PreflightCheck {
            name: "resource-path".to_string(),
            status: PreflightStatus::Fail,
            message: format!(
                "No existing ancestor could be found for {}.",
                resource_path.display()
            ),
        };
    };

    match fs::metadata(&existing_path) {
        Ok(metadata) if metadata.permissions().readonly() => PreflightCheck {
            name: "resource-path".to_string(),
            status: PreflightStatus::Fail,
            message: format!("{} is read-only.", existing_path.display()),
        },
        Ok(_) => PreflightCheck {
            name: "resource-path".to_string(),
            status: PreflightStatus::Pass,
            message: if resource_path.exists() {
                format!("{} exists and appears writable.", resource_path.display())
            } else if dry_run {
                format!(
                    "{} does not exist; nearest existing ancestor is {}.",
                    resource_path.display(),
                    existing_path.display()
                )
            } else {
                format!(
                    "{} can be created under {}.",
                    resource_path.display(),
                    existing_path.display()
                )
            },
        },
        Err(error) => PreflightCheck {
            name: "resource-path".to_string(),
            status: PreflightStatus::Fail,
            message: format!("Could not inspect {}: {error}", existing_path.display()),
        },
    }
}

fn reaper_process_check(
    running_processes: &[RunningProcess],
    allow_reaper_running: bool,
) -> PreflightCheck {
    if running_processes.is_empty() {
        return PreflightCheck {
            name: "reaper-process".to_string(),
            status: PreflightStatus::Pass,
            message: "No running REAPER process was detected.".to_string(),
        };
    }

    let process_list = running_processes
        .iter()
        .map(|process| format!("{} ({})", process.name, process.pid))
        .collect::<Vec<_>>()
        .join(", ");

    if allow_reaper_running {
        PreflightCheck {
            name: "reaper-process".to_string(),
            status: PreflightStatus::Warn,
            message: format!("REAPER appears to be running: {process_list}."),
        }
    } else {
        PreflightCheck {
            name: "reaper-process".to_string(),
            status: PreflightStatus::Fail,
            message: format!("Close REAPER before installing extensions: {process_list}."),
        }
    }
}

fn is_reaper_process_name(platform: Option<Platform>, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    match platform {
        Some(Platform::Windows) => {
            matches!(
                lower.as_str(),
                "reaper.exe" | "reaper64.exe" | "reaper_host32.exe" | "reaper_host64.exe"
            )
        }
        Some(Platform::MacOs) => lower == "reaper" || lower == "reaper64",
        None => lower.starts_with("reaper"),
    }
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if current.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        PreflightOptions, PreflightStatus, RunningProcess, run_install_preflight_with_processes,
    };

    #[test]
    fn passes_when_target_parent_exists_and_reaper_is_not_running() {
        let dir = tempdir().unwrap();
        let report = run_install_preflight_with_processes(
            &dir.path().join("REAPER"),
            &PreflightOptions {
                dry_run: true,
                allow_reaper_running: false,
                target_app_path: None,
            },
            &[],
        );

        assert!(report.passed);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.status == PreflightStatus::Pass)
        );
    }

    #[test]
    fn fails_when_reaper_is_running_without_override() {
        let dir = tempdir().unwrap();
        let report = run_install_preflight_with_processes(
            dir.path(),
            &PreflightOptions {
                dry_run: false,
                allow_reaper_running: false,
                target_app_path: Some(PathBuf::from(r"C:\REAPER\reaper.exe")),
            },
            &[RunningProcess {
                pid: "123".to_string(),
                name: "reaper.exe".to_string(),
                executable_path: Some(PathBuf::from(r"C:\REAPER\reaper.exe")),
            }],
        );

        assert!(!report.passed);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.name == "reaper-process")
                .unwrap()
                .status,
            PreflightStatus::Fail
        );
    }

    #[test]
    fn warns_when_reaper_running_override_is_enabled() {
        let dir = tempdir().unwrap();
        let report = run_install_preflight_with_processes(
            dir.path(),
            &PreflightOptions {
                dry_run: false,
                allow_reaper_running: true,
                target_app_path: Some(PathBuf::from(r"C:\REAPER\reaper.exe")),
            },
            &[RunningProcess {
                pid: "123".to_string(),
                name: "reaper.exe".to_string(),
                executable_path: Some(PathBuf::from(r"C:\REAPER\reaper.exe")),
            }],
        );

        assert!(report.passed);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.name == "reaper-process")
                .unwrap()
                .status,
            PreflightStatus::Warn
        );
    }

    #[test]
    fn ignores_running_reaper_when_explicit_target_app_differs() {
        let dir = tempdir().unwrap();
        let report = run_install_preflight_with_processes(
            dir.path(),
            &PreflightOptions {
                dry_run: false,
                allow_reaper_running: false,
                target_app_path: Some(PathBuf::from(r"C:\Portable\REAPER\reaper.exe")),
            },
            &[RunningProcess {
                pid: "456".to_string(),
                name: "reaper.exe".to_string(),
                executable_path: Some(PathBuf::from(r"C:\Program Files\REAPER\reaper.exe")),
            }],
        );

        assert!(report.passed);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.name == "reaper-process")
                .unwrap()
                .status,
            PreflightStatus::Pass
        );
    }

    #[test]
    fn ignores_running_reaper_from_other_portable_folder() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        std::fs::write(resource_path.join("reaper.exe"), b"").unwrap();

        let report = run_install_preflight_with_processes(
            &resource_path,
            &PreflightOptions {
                dry_run: false,
                allow_reaper_running: false,
                target_app_path: None,
            },
            &[RunningProcess {
                pid: "789".to_string(),
                name: "reaper.exe".to_string(),
                executable_path: Some(PathBuf::from(r"C:\OtherPortable\reaper.exe")),
            }],
        );

        assert!(report.passed);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.name == "reaper-process")
                .unwrap()
                .status,
            PreflightStatus::Pass
        );
    }

    #[test]
    fn ignores_running_standard_reaper_for_empty_portable_target_folder() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("EmptyPortableTarget");
        std::fs::create_dir_all(&resource_path).unwrap();

        let report = run_install_preflight_with_processes(
            &resource_path,
            &PreflightOptions {
                dry_run: false,
                allow_reaper_running: false,
                target_app_path: None,
            },
            &[RunningProcess {
                pid: "999".to_string(),
                name: "reaper.exe".to_string(),
                executable_path: Some(PathBuf::from(r"C:\Program Files\REAPER\reaper.exe")),
            }],
        );

        assert!(report.passed);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.name == "reaper-process")
                .unwrap()
                .status,
            PreflightStatus::Pass
        );
    }
}
