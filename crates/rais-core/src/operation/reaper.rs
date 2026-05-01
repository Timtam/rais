use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;

use super::target_likely_portable;

pub(super) fn reaper_windows_installer_arguments(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let install_destination = target_app_path
        .map(reaper_install_destination)
        .unwrap_or_else(|| resource_path.to_path_buf());
    let mut arguments = Vec::new();
    if target_likely_portable(resource_path, target_app_path) {
        arguments.push("/PORTABLE".to_string());
    }
    arguments.push("/S".to_string());
    arguments.push(format!("/D={}", install_destination.display()));
    arguments
}

pub(super) fn reaper_manual_steps(
    kind: ArtifactKind,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let install_destination = target_app_path.map(reaper_install_destination);
    if target_likely_portable(resource_path, target_app_path) {
        return match kind {
            ArtifactKind::Installer => vec![
                format!(
                    "In the REAPER installer, choose Portable install and use this folder: {}",
                    resource_path.display()
                ),
                format!(
                    "After installation, confirm that {} exists.",
                    resource_path.join("reaper.ini").display()
                ),
            ],
            ArtifactKind::DiskImage | ArtifactKind::Archive => vec![
                format!(
                    "Copy REAPER into this portable folder: {}",
                    install_destination
                        .unwrap_or_else(|| resource_path.to_path_buf())
                        .display()
                ),
                format!(
                    "Create or keep {} for the portable resource layout.",
                    resource_path.join("reaper.ini").display()
                ),
            ],
            ArtifactKind::ExtensionBinary => vec![format!(
                "Place the REAPER application files under this target: {}",
                resource_path.display()
            )],
        };
    }

    match kind {
        ArtifactKind::Installer => {
            let destination = install_destination.unwrap_or_else(|| resource_path.to_path_buf());
            vec![
                format!(
                    "Install REAPER to this destination: {}",
                    destination.display()
                ),
                format!(
                    "After installation, start REAPER once if needed so this resource path exists: {}",
                    resource_path.display()
                ),
            ]
        }
        ArtifactKind::DiskImage | ArtifactKind::Archive => {
            let destination = install_destination.unwrap_or_else(|| resource_path.to_path_buf());
            vec![
                format!("Copy REAPER to this destination: {}", destination.display()),
                format!(
                    "After installation, start REAPER once if needed so this resource path exists: {}",
                    resource_path.display()
                ),
            ]
        }
        ArtifactKind::ExtensionBinary => vec![format!(
            "Install REAPER for the target that uses this resource path: {}",
            resource_path.display()
        )],
    }
}

pub(super) fn reaper_macos_app_bundle_install_target(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> (String, PathBuf) {
    let bundle = target_app_path
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "REAPER.app".to_string());
    let destination_dir = target_app_path
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| resource_path.to_path_buf());
    (bundle, destination_dir)
}

pub(super) fn reaper_install_destination(target_app_path: &Path) -> PathBuf {
    if target_app_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        target_app_path
            .parent()
            .unwrap_or(target_app_path)
            .to_path_buf()
    } else {
        target_app_path.to_path_buf()
    }
}
