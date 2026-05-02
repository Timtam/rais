//! JAWS-for-Windows install probe.
//!
//! RAIS only offers the "JAWS-for-REAPER scripts" package when JAWS itself is
//! present on the host. Detection here is intentionally per-user: JAWS writes
//! the per-version settings tree under
//! `%APPDATA%\Freedom Scientific\JAWS\<version>\Settings\<lang>` the first
//! time a user opens it, and that is also where script files like
//! `reaper.jss` and `reaper.jsb` belong. So a host where the current user has
//! never run JAWS will look "JAWS-less" to RAIS — exactly what we want, since
//! we don't have anywhere meaningful to drop scripts in that case.
//!
//! This module is callable on every platform and returns `None` on non-Windows
//! hosts, so the wizard filter doesn't have to spread `cfg(target_os)` around.
//!
//! Phase A only needs "is there a JAWS install we can target?" — picking the
//! exact `Settings\<lang>` subdirectory the scripts should land in is left to
//! the install pipeline (Phase C), which will need to know the user's JAWS
//! interface language.

use std::path::{Path, PathBuf};

use crate::paths::user_appdata_dir;

/// A detected JAWS install for the current user. Holds the version-specific
/// JAWS profile root (`%APPDATA%\Freedom Scientific\JAWS\<version>`) so the
/// caller can locate `Settings\<lang>` underneath it without re-walking the
/// directory tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JawsInstall {
    /// Display version as it appears under `JAWS\` (e.g. `"2024"`,
    /// `"2025.2412.X"`, …). JAWS uses year-based version directories, so
    /// lexicographic ordering matches numeric ordering for normal releases.
    pub version: String,
    /// Per-version JAWS profile root for the current user.
    pub profile_root: PathBuf,
    /// `Settings` directory inside `profile_root`. Always a child of
    /// `profile_root`; not guaranteed to exist (a user can launch JAWS for
    /// the first time without ever opening a setting that creates it), but
    /// in practice it does for any JAWS that has been used.
    pub settings_root: PathBuf,
}

/// Returns the highest-version JAWS profile root that exists for the current
/// user, or `None` if no JAWS profile is found (or this isn't Windows).
///
/// "Highest" is the lexicographically last directory name under
/// `Freedom Scientific\JAWS\` that has a child `Settings` directory. JAWS
/// version names follow the form `YYYY` or `YYYY.<build>.<patch>`, so
/// lexicographic ordering matches the order users perceive for current
/// releases. Older JAWS (pre-2018) used numeric versions like `17.0` and
/// `18.0`, which also happen to lex-order correctly within their cohort —
/// they just sort *before* the year-based names, which is the desired
/// behavior anyway (a host with both should target the newer one).
pub fn detect_jaws_install() -> Option<JawsInstall> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    detect_jaws_install_under(&user_appdata_dir()?)
}

/// Same as [`detect_jaws_install`] but with an explicit `%APPDATA%` root, so
/// tests can pin a synthetic directory layout without mutating process
/// environment variables.
pub fn detect_jaws_install_under(appdata: &Path) -> Option<JawsInstall> {
    let jaws_root = appdata.join("Freedom Scientific").join("JAWS");
    let entries = std::fs::read_dir(&jaws_root).ok()?;

    let mut candidates: Vec<JawsInstall> = entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.file_type().ok()?;
            if !metadata.is_dir() {
                return None;
            }
            let version = entry.file_name().to_string_lossy().into_owned();
            let profile_root = entry.path();
            let settings_root = profile_root.join("Settings");
            if !settings_root.is_dir() {
                return None;
            }
            Some(JawsInstall {
                version,
                profile_root,
                settings_root,
            })
        })
        .collect();

    candidates.sort_by(|a, b| a.version.cmp(&b.version));
    candidates.pop()
}

/// Convenience: `true` when [`detect_jaws_install`] would return `Some`.
pub fn is_jaws_installed() -> bool {
    detect_jaws_install().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::tempdir;

    fn make_jaws_layout(jaws_versions: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let appdata = dir.path().join("appdata");
        fs::create_dir_all(&appdata).unwrap();
        let jaws_root = appdata.join("Freedom Scientific").join("JAWS");
        for version in jaws_versions {
            fs::create_dir_all(jaws_root.join(version).join("Settings")).unwrap();
        }
        (dir, appdata)
    }

    #[test]
    fn returns_none_when_no_jaws_directory_exists() {
        let (_guard, appdata) = make_jaws_layout(&[]);
        assert!(detect_jaws_install_under(&appdata).is_none());
    }

    #[test]
    fn picks_highest_version_subdirectory() {
        let (_guard, appdata) = make_jaws_layout(&["2023", "2024", "2025"]);
        let install = detect_jaws_install_under(&appdata).unwrap();
        assert_eq!(install.version, "2025");
        assert_eq!(
            install.profile_root,
            appdata.join("Freedom Scientific").join("JAWS").join("2025")
        );
        assert_eq!(install.settings_root, install.profile_root.join("Settings"));
    }

    #[test]
    fn ignores_version_directories_without_settings() {
        let (_guard, appdata) = make_jaws_layout(&["2024"]);
        // Add a half-initialized 2025 profile (no Settings dir): probe
        // should fall back to 2024.
        fs::create_dir_all(appdata.join("Freedom Scientific").join("JAWS").join("2025")).unwrap();
        let install = detect_jaws_install_under(&appdata).unwrap();
        assert_eq!(install.version, "2024");
    }
}
