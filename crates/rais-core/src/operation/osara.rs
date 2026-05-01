use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;
use crate::error::{IoPathContext, RaisError, Result};
use crate::model::Platform;
use crate::package::PACKAGE_OSARA;

use super::{
    PackageAutomationSupport, PlannedAutomationKind, PlannedExecutionKind,
    PlannedExecutionOverride, UnattendedPostInstallReport, backup_file_for_unattended_change,
    replace_file_from_source, target_likely_portable,
};

pub(super) const TITLE: &str = "OSARA";

/// OSARA-specific automation routing. Today: Windows installer is unattended,
/// macOS archive is unattended via the OSARA-asset extractor.
pub(super) fn automation_support_for(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PackageAutomationSupport> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller),
        ),
        (ArtifactKind::Archive, Platform::MacOs) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::ArchiveExtraction),
        ),
        _ => None,
    }
}

/// OSARA-specific message variant used when the unattended path also applied
/// the key-map replacement step. Returns `None` when the replacement was not
/// requested (caller should fall back to the generic message).
pub(super) fn unattended_install_message(
    replace_osara_keymap: bool,
    keymap_was_backed_up: bool,
) -> Option<String> {
    if !replace_osara_keymap {
        return None;
    }
    Some(
        if keymap_was_backed_up {
            "RAIS ran the upstream installer unattended, backed up reaper-kb.ini, applied the OSARA key map replacement, and updated the RAIS receipt."
        } else {
            "RAIS ran the upstream installer unattended, applied the OSARA key map replacement, and updated the RAIS receipt."
        }
        .to_string(),
    )
}

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

/// Files installed by OSARA that the receipt should reference. Filtered to
/// the on-disk existing ones after the unattended run.
pub(super) fn receipt_paths(resource_path: &Path, replace_osara_keymap: bool) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let keymap_path = resource_path.join("KeyMaps").join("OSARA.ReaperKeyMap");
    if keymap_path.exists() {
        paths.push(keymap_path);
    }
    let support_dir = resource_path.join("osara");
    if support_dir.exists() {
        paths.push(support_dir);
    }
    if replace_osara_keymap {
        let current_keymap = resource_path.join("reaper-kb.ini");
        if current_keymap.exists() {
            paths.push(current_keymap);
        }
    }
    paths
}

/// Post-install fixups specific to OSARA: clean up the portable
/// uninstaller stub on Windows and apply the key-map replacement when the
/// user opted into it.
pub(super) fn post_install_unattended(
    resource_path: &Path,
    platform: Platform,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Result<UnattendedPostInstallReport> {
    let mut report = UnattendedPostInstallReport::default();
    if matches!(platform, Platform::Windows)
        && target_likely_portable(resource_path, target_app_path)
    {
        let uninstall_path = resource_path.join("osara").join("uninstall.exe");
        if uninstall_path.is_file() {
            std::fs::remove_file(&uninstall_path).with_path(&uninstall_path)?;
        }
    }
    if replace_osara_keymap {
        report = apply_osara_keymap_replacement(resource_path)?;
    }
    Ok(report)
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

pub(super) fn installer_arguments(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => {
            Some(osara_windows_installer_arguments(resource_path))
        }
        _ => None,
    }
}

pub(super) fn planned_execution_override(
    kind: ArtifactKind,
    platform: Platform,
    resource_path: &Path,
) -> Option<PlannedExecutionOverride> {
    match (kind, platform) {
        (ArtifactKind::Archive, Platform::MacOs) => Some(PlannedExecutionOverride {
            kind: PlannedExecutionKind::ExtractArchiveAndCopyOsaraAssets,
            arguments: vec![resource_path.display().to_string()],
            use_cached_working_dir: true,
        }),
        _ => None,
    }
}

fn osara_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
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
