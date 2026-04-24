use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use crate::error::RaisError;
use crate::operation::{PlannedExecutionKind, PlannedExecutionPlan};

pub fn execute_planned_execution(plan: &PlannedExecutionPlan, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    match plan.kind {
        PlannedExecutionKind::LaunchInstallerExecutable => execute_program_plan(plan)?,
        PlannedExecutionKind::ExtractArchiveAndRunInstaller
        | PlannedExecutionKind::MountDiskImageAndRunInstaller => {
            return Err(RaisError::InvalidPlannedExecution {
                message: format!("runner {:?} is not implemented yet", plan.kind),
            });
        }
    }

    Ok(())
}

pub fn verify_planned_execution_paths(plan: &PlannedExecutionPlan) -> Result<()> {
    verify_paths(&plan.verification_paths)
}

fn execute_program_plan(plan: &PlannedExecutionPlan) -> Result<()> {
    let Some(program) = &plan.program else {
        return Err(RaisError::InvalidPlannedExecution {
            message: "launch plan did not provide a program path".to_string(),
        });
    };

    let mut command = Command::new(program);
    command.args(&plan.arguments);
    if let Some(working_directory) = &plan.working_directory {
        command.current_dir(working_directory);
    }

    let status = command.status().map_err(|source| RaisError::Io {
        path: PathBuf::from(program),
        source,
    })?;
    if !status.success() {
        return Err(RaisError::ProcessFailed {
            program: program.clone(),
            exit_code: status.code(),
        });
    }

    Ok(())
}

fn verify_paths(paths: &[PathBuf]) -> Result<()> {
    let missing_paths = paths
        .iter()
        .filter(|path| !path.exists())
        .cloned()
        .collect::<Vec<_>>();
    if missing_paths.is_empty() {
        Ok(())
    } else {
        Err(RaisError::PostInstallVerificationFailed { missing_paths })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{execute_planned_execution, verify_planned_execution_paths};
    use crate::operation::{PlannedExecutionKind, PlannedExecutionPlan};

    #[test]
    fn dry_run_does_not_execute_program() {
        let dir = tempdir().unwrap();
        let marker_path = dir.path().join("marker.txt");
        let plan = success_plan(&marker_path);

        execute_planned_execution(&plan, true).unwrap();

        assert!(!marker_path.exists());
    }

    #[test]
    fn executes_program_and_verifies_output() {
        let dir = tempdir().unwrap();
        let marker_path = dir.path().join("marker.txt");
        let plan = success_plan(&marker_path);

        execute_planned_execution(&plan, false).unwrap();
        verify_planned_execution_paths(&plan).unwrap();

        assert!(marker_path.is_file());
    }

    #[test]
    fn fails_when_program_returns_nonzero() {
        let dir = tempdir().unwrap();
        let marker_path = dir.path().join("marker.txt");
        let plan = failure_plan(&marker_path);

        let error = execute_planned_execution(&plan, false).unwrap_err();

        assert!(error.to_string().contains("process failed"));
    }

    #[test]
    fn verification_fails_when_expected_output_is_missing() {
        let dir = tempdir().unwrap();
        let marker_path = dir.path().join("missing.txt");
        let plan = PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            artifact_location: "noop".to_string(),
            program: None,
            arguments: Vec::new(),
            working_directory: None,
            verification_paths: vec![marker_path],
        };

        let error = verify_planned_execution_paths(&plan).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("post-install verification failed")
        );
    }

    #[cfg(target_os = "windows")]
    fn success_plan(marker_path: &std::path::Path) -> PlannedExecutionPlan {
        PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            artifact_location: "powershell.exe".to_string(),
            program: Some("powershell.exe".to_string()),
            arguments: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                format!(
                    "Set-Content -Path '{}' -Value 'ok'",
                    escaped_path(marker_path)
                ),
            ],
            working_directory: None,
            verification_paths: vec![marker_path.to_path_buf()],
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn success_plan(marker_path: &std::path::Path) -> PlannedExecutionPlan {
        PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            artifact_location: "sh".to_string(),
            program: Some("sh".to_string()),
            arguments: vec![
                "-c".to_string(),
                format!("printf ok > \"{}\"", escaped_path(marker_path)),
            ],
            working_directory: None,
            verification_paths: vec![marker_path.to_path_buf()],
        }
    }

    #[cfg(target_os = "windows")]
    fn failure_plan(marker_path: &std::path::Path) -> PlannedExecutionPlan {
        PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            artifact_location: "powershell.exe".to_string(),
            program: Some("powershell.exe".to_string()),
            arguments: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                format!(
                    "Set-Content -Path '{}' -Value 'fail'; exit 7",
                    escaped_path(marker_path)
                ),
            ],
            working_directory: None,
            verification_paths: vec![marker_path.to_path_buf()],
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn failure_plan(marker_path: &std::path::Path) -> PlannedExecutionPlan {
        PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            artifact_location: "sh".to_string(),
            program: Some("sh".to_string()),
            arguments: vec![
                "-c".to_string(),
                format!("printf fail > \"{}\"; exit 7", escaped_path(marker_path)),
            ],
            working_directory: None,
            verification_paths: vec![marker_path.to_path_buf()],
        }
    }

    #[cfg(target_os = "windows")]
    fn escaped_path(path: &std::path::Path) -> String {
        path.display().to_string().replace('\'', "''")
    }

    #[cfg(not(target_os = "windows"))]
    fn escaped_path(path: &std::path::Path) -> String {
        path.display().to_string().replace('"', "\\\"")
    }
}
