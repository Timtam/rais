use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::artifact::ArtifactDescriptor;
use crate::model::{Architecture, Platform};
use crate::operation::{
    PackageOperationOptions, PackageOperationReport, execute_package_operation,
    execute_resolved_package_operation,
};
use crate::resource::{ResourceInitOptions, ResourceInitReport, initialize_resource_path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupOptions {
    pub dry_run: bool,
    pub portable: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub replace_osara_keymap: bool,
    pub target_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupReport {
    pub resource_path: PathBuf,
    pub dry_run: bool,
    pub resource_init: ResourceInitReport,
    pub package_operation: PackageOperationReport,
}

pub fn execute_setup_operation(
    resource_path: &Path,
    package_ids: &[String],
    platform: Platform,
    architecture: Architecture,
    cache_dir: &Path,
    options: &SetupOptions,
) -> Result<SetupReport> {
    let resource_init = initialize_resource_path(
        resource_path,
        &ResourceInitOptions {
            dry_run: options.dry_run,
            portable: options.portable,
            allow_reaper_running: options.allow_reaper_running,
            target_app_path: options.target_app_path.clone(),
        },
    )?;
    let package_operation = execute_package_operation(
        resource_path,
        package_ids,
        platform,
        architecture,
        cache_dir,
        &PackageOperationOptions {
            dry_run: options.dry_run,
            allow_reaper_running: options.allow_reaper_running,
            stage_unsupported: options.stage_unsupported,
            replace_osara_keymap: options.replace_osara_keymap,
            target_app_path: options.target_app_path.clone(),
        },
    )?;

    Ok(SetupReport {
        resource_path: resource_path.to_path_buf(),
        dry_run: options.dry_run,
        resource_init,
        package_operation,
    })
}

pub fn execute_resolved_setup_operation(
    resource_path: &Path,
    artifacts: Vec<ArtifactDescriptor>,
    cache_dir: &Path,
    options: &SetupOptions,
) -> Result<SetupReport> {
    let resource_init = initialize_resource_path(
        resource_path,
        &ResourceInitOptions {
            dry_run: options.dry_run,
            portable: options.portable,
            allow_reaper_running: options.allow_reaper_running,
            target_app_path: options.target_app_path.clone(),
        },
    )?;
    let package_operation = execute_resolved_package_operation(
        resource_path,
        artifacts,
        cache_dir,
        &PackageOperationOptions {
            dry_run: options.dry_run,
            allow_reaper_running: options.allow_reaper_running,
            stage_unsupported: options.stage_unsupported,
            replace_osara_keymap: options.replace_osara_keymap,
            target_app_path: options.target_app_path.clone(),
        },
    )?;

    Ok(SetupReport {
        resource_path: resource_path.to_path_buf(),
        dry_run: options.dry_run,
        resource_init,
        package_operation,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{SetupOptions, execute_resolved_setup_operation};
    use crate::artifact::{ArtifactDescriptor, ArtifactKind};
    use crate::install::InstallFileAction;
    use crate::model::{Architecture, Platform};
    use crate::package::PACKAGE_REAPACK;
    use crate::version::Version;

    #[test]
    fn dry_run_reports_resource_and_package_actions_without_writing() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source = dir.path().join("reaper_reapack-x64.dll");
        fs::write(&source, b"reapack").unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_setup_operation(
            &resource_path,
            vec![artifact(&source)],
            cache.path(),
            &SetupOptions {
                dry_run: true,
                portable: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(report.dry_run);
        assert!(!resource_path.exists());
        let install_report = report.package_operation.install_report.unwrap();
        assert_eq!(
            install_report.actions[0].action,
            InstallFileAction::WouldInstall
        );
    }

    #[test]
    fn apply_creates_resource_layout_and_installs_extension() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source = dir.path().join("reaper_reapack-x64.dll");
        fs::write(&source, b"reapack").unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_setup_operation(
            &resource_path,
            vec![artifact(&source)],
            cache.path(),
            &SetupOptions {
                dry_run: false,
                portable: true,
                allow_reaper_running: true,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(!report.dry_run);
        assert!(resource_path.join("reaper.ini").is_file());
        assert!(
            resource_path
                .join("UserPlugins/reaper_reapack-x64.dll")
                .is_file()
        );
        let install_report = report.package_operation.install_report.unwrap();
        assert_eq!(
            install_report.actions[0].action,
            InstallFileAction::Installed
        );
    }

    fn artifact(source: &std::path::Path) -> ArtifactDescriptor {
        ArtifactDescriptor {
            package_id: PACKAGE_REAPACK.to_string(),
            version: Version::parse("1.2.6").unwrap(),
            platform: Platform::Windows,
            architecture: Architecture::X64,
            kind: ArtifactKind::ExtensionBinary,
            url: source.display().to_string(),
            file_name: "reaper_reapack-x64.dll".to_string(),
        }
    }
}
