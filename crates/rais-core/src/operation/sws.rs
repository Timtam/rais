use std::path::{Path, PathBuf};

use crate::artifact::{ArtifactDescriptor, ArtifactKind};
use crate::model::{Architecture, Platform};

use super::{PackageAutomationSupport, PlannedAutomationKind};

pub(super) const TITLE: &str = "SWS";

/// SWS-specific automation routing. Windows installer runs unattended; the
/// macOS DMG is direct-extractable (no installer to invoke).
pub(super) fn automation_support_for(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PackageAutomationSupport> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller),
        ),
        (ArtifactKind::DiskImage, Platform::MacOs) => Some(PackageAutomationSupport::Direct),
        _ => None,
    }
}

pub(super) fn manual_install_notes(resource_path: &Path) -> Vec<String> {
    vec![format!(
        "The SWS installer should target the REAPER installation that uses this resource folder: {}.",
        resource_path.display()
    )]
}

/// SWS-installed support files outside `UserPlugins` that the receipt should
/// reference when present (script bridge + factory data).
pub(super) fn receipt_paths(resource_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let script_path = resource_path.join("Scripts").join("sws_python.py");
    if script_path.exists() {
        paths.push(script_path);
    }
    let grooves_path = resource_path.join("Data").join("Grooves");
    if grooves_path.exists() {
        paths.push(grooves_path);
    }
    paths
}

pub(super) fn verification_paths(
    resource_path: &Path,
    artifact: &ArtifactDescriptor,
) -> Vec<PathBuf> {
    let mut paths = vec![resource_path.join("UserPlugins")];
    if let Some(plugin_path) = sws_primary_plugin_path(resource_path, artifact) {
        paths.push(plugin_path);
    }
    paths
}

pub(super) fn installer_arguments(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => {
            Some(sws_windows_installer_arguments(resource_path))
        }
        _ => None,
    }
}

fn sws_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
    vec!["/S".to_string(), format!("/D={}", resource_path.display())]
}

pub(super) fn sws_manual_steps(kind: ArtifactKind, resource_path: &Path) -> Vec<String> {
    match kind {
        ArtifactKind::Installer => vec![format!(
            "When the SWS installer asks which REAPER installation to update, choose the one that uses this resource folder: {}",
            resource_path.display()
        )],
        ArtifactKind::DiskImage | ArtifactKind::Archive => vec![
            "Run the SWS installer from the opened package.".to_string(),
            format!(
                "Choose the REAPER target that uses this resource folder: {}",
                resource_path.display()
            ),
        ],
        ArtifactKind::ExtensionBinary => vec![format!(
            "Copy the SWS extension into this REAPER UserPlugins folder: {}",
            resource_path.join("UserPlugins").display()
        )],
    }
}

pub(super) fn sws_primary_plugin_path(
    resource_path: &Path,
    artifact: &ArtifactDescriptor,
) -> Option<PathBuf> {
    let file_name = match (artifact.platform, artifact.architecture) {
        (Platform::Windows, Architecture::X86) => "reaper_sws-x86.dll",
        (Platform::Windows, Architecture::X64 | Architecture::Unknown) => "reaper_sws-x64.dll",
        (Platform::MacOs, Architecture::X86) => "reaper_sws-i386.dylib",
        (Platform::MacOs, Architecture::X64 | Architecture::Unknown) => "reaper_sws-x86_64.dylib",
        (Platform::MacOs, Architecture::Arm64) => "reaper_sws-arm64.dylib",
        _ => return None,
    };

    Some(resource_path.join("UserPlugins").join(file_name))
}
