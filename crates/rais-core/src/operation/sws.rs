use std::path::{Path, PathBuf};

use crate::artifact::{ArtifactDescriptor, ArtifactKind};
use crate::model::{Architecture, Platform};

pub(super) fn sws_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
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
