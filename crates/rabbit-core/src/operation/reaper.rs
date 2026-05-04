use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;
use crate::model::Platform;

use super::{
    PackageAutomationSupport, PlannedAutomationKind, PlannedExecutionKind,
    PlannedExecutionOverride, target_likely_portable,
};

pub(super) const TITLE: &str = "REAPER";

/// REAPER-specific automation routing. Returns `Some(verdict)` when REAPER
/// upgrades the generic planned-unattended verdict to an unattended one for
/// the given (kind, platform); returns `None` to defer to the generic
/// fallback chain.
pub(super) fn automation_support_for(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PackageAutomationSupport> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller),
        ),
        (ArtifactKind::DiskImage, Platform::MacOs) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::DiskImageInstall),
        ),
        _ => None,
    }
}

pub(super) fn manual_install_notes(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    let mut notes = vec![
        "REAPER application installers should be launched and completed by RABBIT itself in supported builds, but this engine slice does not execute them yet."
            .to_string(),
    ];
    if target_likely_portable(resource_path, target_app_path) {
        notes.push(format!(
            "This looks like a portable target. REAPER application files and reaper.ini should end up under {}.",
            resource_path.display()
        ));
    } else if let Some(target_app_path) = target_app_path {
        notes.push(format!(
            "This target may require administrator approval if REAPER is installed to {}.",
            reaper_install_destination(target_app_path).display()
        ));
    }
    notes
}

/// Files written by an unattended REAPER install that the receipt should
/// reference, scoped to ones that actually exist on disk after the run.
pub(super) fn receipt_paths(resource_path: &Path, target_app_path: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = target_app_path.filter(|path| path.exists()) {
        paths.push(path.to_path_buf());
        if target_likely_portable(resource_path, Some(path)) {
            let ini_path = resource_path.join("reaper.ini");
            if ini_path.exists() {
                paths.push(ini_path);
            }
        }
    }
    paths
}

pub(super) fn verification_paths(
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(target_app_path) = target_app_path {
        paths.push(target_app_path.to_path_buf());
        if target_likely_portable(resource_path, Some(target_app_path)) {
            paths.push(resource_path.join("reaper.ini"));
        }
    } else {
        paths.push(resource_path.to_path_buf());
    }
    paths
}

pub(super) fn installer_arguments(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(reaper_windows_installer_arguments(
            resource_path,
            target_app_path,
        )),
        _ => None,
    }
}

pub(super) fn planned_execution_override(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Option<PlannedExecutionOverride> {
    match (kind, platform) {
        (ArtifactKind::DiskImage, Platform::MacOs) => {
            let (bundle_basename, install_destination) =
                reaper_macos_app_bundle_install_target(resource_path, target_app_path);
            Some(PlannedExecutionOverride {
                kind: PlannedExecutionKind::MountDiskImageAndCopyAppBundle,
                arguments: vec![bundle_basename, install_destination.display().to_string()],
                use_cached_working_dir: false,
            })
        }
        _ => None,
    }
}

fn reaper_windows_installer_arguments(
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

fn reaper_macos_app_bundle_install_target(
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

fn reaper_install_destination(target_app_path: &Path) -> PathBuf {
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
