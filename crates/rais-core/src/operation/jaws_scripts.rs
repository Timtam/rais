//! Per-package automation hooks for the JAWS-for-REAPER scripts.
//!
//! Upstream ships an NSIS self-extracting installer (the file we get from
//! `hoard.reaperaccessibility.com` is `Reaper_JawsScripts_NN.exe`). The
//! installer:
//!
//!   * Drops `Reaper_ComAccess32.dll` and `Reaper_ComAccess64.dll` into
//!     `%APPDATA%\REAPER\UserPlugins\` (the standard REAPER UserPlugins
//!     folder — paths are hard-coded inside the NSIS script).
//!   * Detects the installed JAWS version + interface language and copies
//!     the script files (`reaper.JSS`, `reaper.JSB`, `*.jcf`, sounds, …)
//!     into the matching `%APPDATA%\Freedom Scientific\JAWS\<ver>\Settings\<lang>\`.
//!
//! Because all of that lives inside the NSIS script (which runs the
//! `JAWSSetupUtility.dll` plugin to pick the right JAWS slot), RAIS doesn't
//! need to replicate any of that logic — we just launch the installer with
//! `/S` for a silent, unattended run and verify the standard REAPER
//! UserPlugins side landed afterwards.
//!
//! The installer hard-codes the standard REAPER `%APPDATA%\REAPER` path, so
//! a portable REAPER target won't receive the `Reaper_ComAccess*.dll` files
//! — that's a real limitation, but matches what users get if they run the
//! installer manually. We still let the package run for portable targets so
//! the JAWS-side scripts do get refreshed; the verification path falls back
//! to the per-user JAWS Settings file in that case.

use std::path::{Path, PathBuf};

use crate::artifact::ArtifactKind;
use crate::model::Platform;

use super::{PackageAutomationSupport, PlannedAutomationKind};

pub(super) const TITLE: &str = "JAWS-for-REAPER scripts";

/// `Some(AvailableUnattended)` only for an `.exe` artifact on Windows; the
/// package is platform-gated to Windows in the manifest, so this is mostly a
/// safety check.
pub(super) fn automation_support_for(
    kind: ArtifactKind,
    platform: Platform,
) -> Option<PackageAutomationSupport> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller),
        ),
        _ => None,
    }
}

/// `["/S"]` — NSIS-3 silent install. The installer's `JAWSSetupUtility.dll`
/// auto-picks the JAWS version + language; no `/D=` override since the
/// destination paths are hard-coded inside the NSIS script.
pub(super) fn installer_arguments(kind: ArtifactKind, platform: Platform) -> Option<Vec<String>> {
    match (kind, platform) {
        (ArtifactKind::Installer, Platform::Windows) => Some(vec!["/S".to_string()]),
        _ => None,
    }
}

/// Files the post-install verifier should look for. The hard-coded REAPER
/// UserPlugins target is the most reliable signal that the installer ran;
/// we only require the 64-bit DLL since 32-bit REAPER is rare today and
/// the installer drops both side-by-side.
pub(super) fn verification_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(appdata) = rais_platform::user_appdata_dir() {
        paths.push(
            appdata
                .join("REAPER")
                .join("UserPlugins")
                .join("Reaper_ComAccess64.dll"),
        );
    }
    paths
}

/// Files NSIS *generates* at install time (not extracted from the package),
/// whose mtime should reflect the new install. Today only `Uninstall.exe`
/// fits — `WriteUninstaller` writes it with the current clock — so a stale
/// mtime there is the canary for "silent install no-op'd because of the
/// missing UAC elevation we used to skip". Returns an empty vector when the
/// `Reaper_JawsScripts` Programs-and-Features uninstall key isn't recorded
/// (first-ever install on this host); the regular existence verification
/// still gates that case.
pub(super) fn freshness_paths() -> Vec<PathBuf> {
    let Some(install_dir) =
        rais_platform::read_uninstall_value("Reaper_JawsScripts", "UninstallDirectory")
    else {
        return Vec::new();
    };
    vec![PathBuf::from(install_dir).join("Uninstall.exe")]
}

/// Files the install receipt should reference (so an uninstall or a
/// re-install can find them). Subset of [`verification_paths`] that
/// actually exist on disk, since we only want to record real artifacts.
pub(super) fn receipt_paths(_resource_path: &Path) -> Vec<PathBuf> {
    verification_paths()
        .into_iter()
        .filter(|path| path.exists())
        .collect()
}
