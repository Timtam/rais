use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

use crate::model::Platform;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
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
    let mut checks = vec![resource_path_check(resource_path, options.dry_run)];
    checks.push(reaper_process_check(
        running_processes,
        options.allow_reaper_running || options.dry_run,
    ));

    let passed = checks
        .iter()
        .all(|check| check.status != PreflightStatus::Fail);
    PreflightReport { passed, checks }
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
                })
            } else {
                None
            }
        })
        .collect()
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
            },
            &[RunningProcess {
                pid: "123".to_string(),
                name: "reaper.exe".to_string(),
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
            },
            &[RunningProcess {
                pid: "123".to_string(),
                name: "reaper.exe".to_string(),
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
}
