use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;
use crate::error::{RaisError, Result};
use crate::package::PACKAGE_OSARA;

use super::{
    UnattendedPostInstallReport, backup_file_for_unattended_change, replace_file_from_source,
};

pub(super) const TITLE: &str = "OSARA";

pub(super) fn manual_install_notes(
    resource_path: &Path,
    replace_osara_keymap: bool,
) -> Vec<String> {
    let mut notes = vec![
        "OSARA's Windows installer supports standard and portable REAPER targets; preserve an existing key map unless the user explicitly chooses replacement."
            .to_string(),
    ];
    if replace_osara_keymap {
        notes.push(format!(
            "The selected workflow replaces the current key map. Back up {} before replacing it with the OSARA key map.",
            resource_path.join("reaper-kb.ini").display()
        ));
    } else {
        notes.push(format!(
            "The selected workflow preserves the current key map. Leave {} unchanged.",
            resource_path.join("reaper-kb.ini").display()
        ));
    }
    notes
}

pub(super) fn verification_paths(resource_path: &Path, replace_osara_keymap: bool) -> Vec<PathBuf> {
    let mut paths = vec![
        resource_path.join("UserPlugins"),
        resource_path.join("KeyMaps").join("OSARA.ReaperKeyMap"),
        resource_path.join("osara"),
    ];
    if replace_osara_keymap {
        paths.push(resource_path.join("reaper-kb.ini"));
    }
    paths
}

pub(super) fn osara_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
    vec!["/S".to_string(), format!("/D={}", resource_path.display())]
}

pub(super) fn osara_manual_steps(
    kind: ArtifactKind,
    resource_path: &Path,
    replace_osara_keymap: bool,
) -> Vec<String> {
    let mut steps = match kind {
        ArtifactKind::Installer => vec![format!(
            "When the OSARA installer asks for the REAPER target, choose this resource or portable folder: {}",
            resource_path.display()
        )],
        ArtifactKind::Archive => vec![format!(
            "Run the OSARA installer from the extracted archive and target this REAPER resource or portable folder: {}",
            resource_path.display()
        )],
        ArtifactKind::DiskImage => vec![format!(
            "Run the OSARA installer from the opened disk image and target this REAPER resource or portable folder: {}",
            resource_path.display()
        )],
        ArtifactKind::ExtensionBinary => vec![format!(
            "Copy the OSARA extension into this REAPER UserPlugins folder: {}",
            resource_path.join("UserPlugins").display()
        )],
    };
    if replace_osara_keymap {
        steps.push(format!(
            "After backing up {}, replace the current key map with the OSARA key map if the installer offers that option.",
            resource_path.join("reaper-kb.ini").display()
        ));
    } else {
        steps.push(
            "Preserve the current key map if the OSARA installer offers a replacement option."
                .to_string(),
        );
    }
    steps
}

pub(super) fn apply_osara_keymap_replacement(
    resource_path: &Path,
) -> Result<UnattendedPostInstallReport> {
    let replacement_source = resource_path.join("KeyMaps").join("OSARA.ReaperKeyMap");
    if !replacement_source.is_file() {
        return Err(RaisError::PostInstallVerificationFailed {
            missing_paths: vec![replacement_source],
        });
    }

    let current_keymap = resource_path.join("reaper-kb.ini");
    let mut report = UnattendedPostInstallReport::default();

    if current_keymap.is_file() {
        let (backup_path, backup_manifest_path) = backup_file_for_unattended_change(
            resource_path,
            PACKAGE_OSARA,
            &current_keymap,
            "osara-keymap-replacement",
        )?;
        report.backup_paths.push(backup_path);
        report.backup_manifest_path = Some(backup_manifest_path);
    }

    replace_file_from_source(&replacement_source, &current_keymap)?;
    Ok(report)
}
