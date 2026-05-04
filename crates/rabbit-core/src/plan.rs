use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::{ComponentDetection, Installation};
use crate::package::PACKAGE_REAPER;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailablePackage {
    pub package_id: String,
    pub version: Option<Version>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub target: Option<Installation>,
    pub actions: Vec<PlanAction>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanAction {
    pub package_id: String,
    pub action: PlanActionKind,
    pub installed_version: Option<Version>,
    pub available_version: Option<Version>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanActionKind {
    Install,
    Update,
    Keep,
}

pub fn build_install_plan(
    target: Option<Installation>,
    detections: &[ComponentDetection],
    desired_package_ids: &[String],
    available_packages: &[AvailablePackage],
) -> InstallPlan {
    let detections_by_id: BTreeMap<_, _> = detections
        .iter()
        .map(|detection| (detection.package_id.as_str(), detection))
        .collect();
    let available_by_id: BTreeMap<_, _> = available_packages
        .iter()
        .map(|available| (available.package_id.as_str(), available))
        .collect();

    let mut actions = Vec::new();
    for package_id in desired_package_ids {
        let available = available_by_id.get(package_id.as_str()).copied();
        let detection = detections_by_id.get(package_id.as_str()).copied();
        let (installed, installed_version) = if package_id == PACKAGE_REAPER {
            target_reaper_state(target.as_ref())
        } else {
            (
                detection.is_some_and(|detection| detection.installed),
                detection.and_then(|detection| detection.version.clone()),
            )
        };
        let available_version = available.and_then(|available| available.version.clone());

        let (action, reason) = if !installed {
            (
                PlanActionKind::Install,
                "Package is not installed in the selected REAPER resource path.".to_string(),
            )
        } else if let (Some(installed), Some(available)) = (&installed_version, &available_version)
        {
            if installed.cmp_lenient(available).is_lt() {
                (
                    PlanActionKind::Update,
                    "Installed version is older than the available version.".to_string(),
                )
            } else {
                (
                    PlanActionKind::Keep,
                    "Installed version is current or newer than the available version.".to_string(),
                )
            }
        } else if installed_version.is_none() && available_version.is_some() {
            // The package is on disk but its installed version couldn't be
            // read. Rather than asking a non-technical user to "review
            // manually", treat it as Update: re-install the latest known
            // upstream version on top, with the standard backup/receipt
            // safety net protecting the prior files.
            (
                PlanActionKind::Update,
                "Package is installed but its version could not be detected; updating to the latest available version."
                    .to_string(),
            )
        } else {
            (
                PlanActionKind::Keep,
                "Package is installed; no available version metadata was provided.".to_string(),
            )
        };

        actions.push(PlanAction {
            package_id: package_id.clone(),
            action,
            installed_version,
            available_version,
            reason,
        });
    }

    let mut notes = Vec::new();
    if target.is_none() {
        notes.push("No REAPER installation target was selected.".to_string());
    }
    if available_packages.is_empty() {
        notes.push("Latest-version providers are not implemented yet; the plan only identifies missing packages and packages with known supplied versions.".to_string());
    }

    InstallPlan {
        target,
        actions,
        notes,
    }
}

fn target_reaper_state(target: Option<&Installation>) -> (bool, Option<Version>) {
    let Some(target) = target else {
        return (false, None);
    };

    let installed = target_reaper_app_exists(&target.app_path);
    let installed_version = installed.then(|| target.version.clone()).flatten();
    (installed, installed_version)
}

fn target_reaper_app_exists(app_path: &Path) -> bool {
    app_path.is_file()
        || app_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
            && app_path.exists()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::model::{ComponentDetection, Confidence, Installation, InstallationKind, Platform};
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_REAPER};
    use crate::plan::{AvailablePackage, PlanActionKind, build_install_plan};
    use crate::version::Version;

    #[test]
    fn plans_install_for_missing_package() {
        let desired = vec![PACKAGE_OSARA.to_string()];
        let plan = build_install_plan(None, &[], &desired, &[]);

        assert_eq!(plan.actions[0].action, PlanActionKind::Install);
    }

    #[test]
    fn plans_update_when_available_version_is_newer() {
        let detections = vec![ComponentDetection {
            package_id: PACKAGE_OSARA.to_string(),
            display_name: "OSARA".to_string(),
            installed: true,
            version: Some(Version::parse("2024.1").unwrap()),
            detector: "test".to_string(),
            confidence: Confidence::High,
            files: Vec::new(),
            notes: Vec::new(),
        }];
        let available = vec![AvailablePackage {
            package_id: PACKAGE_OSARA.to_string(),
            version: Some(Version::parse("2024.2").unwrap()),
        }];
        let desired = vec![PACKAGE_OSARA.to_string()];

        let plan = build_install_plan(None, &detections, &desired, &available);

        assert_eq!(plan.actions[0].action, PlanActionKind::Update);
    }

    #[test]
    fn plans_update_when_installed_version_is_unknown_but_available_is_known() {
        // When the package is on disk but its version is unreadable, the
        // wizard should NOT push a "Review manually" decision onto the user.
        // RABBIT plans an Update instead (re-install on top, with backup +
        // receipt protecting the prior files).
        let detections = vec![ComponentDetection {
            package_id: PACKAGE_REAPACK.to_string(),
            display_name: "ReaPack".to_string(),
            installed: true,
            version: None,
            detector: "test".to_string(),
            confidence: Confidence::Medium,
            files: Vec::new(),
            notes: Vec::new(),
        }];
        let available = vec![AvailablePackage {
            package_id: PACKAGE_REAPACK.to_string(),
            version: Some(Version::parse("1.2.6").unwrap()),
        }];
        let desired = vec![PACKAGE_REAPACK.to_string()];

        let plan = build_install_plan(None, &detections, &desired, &available);

        assert_eq!(plan.actions[0].action, PlanActionKind::Update);
        assert!(
            plan.actions[0].reason.contains("could not be detected"),
            "expected the reason text to explain the version-detection fallback, got {:?}",
            plan.actions[0].reason
        );
    }

    #[test]
    fn plans_install_for_reaper_when_target_app_is_missing() {
        let dir = tempdir().unwrap();
        let installation = fake_reaper_installation(
            dir.path().join("reaper.exe"),
            dir.path().to_path_buf(),
            None,
        );
        let desired = vec![PACKAGE_REAPER.to_string()];
        let available = vec![AvailablePackage {
            package_id: PACKAGE_REAPER.to_string(),
            version: Some(Version::parse("7.70").unwrap()),
        }];

        let plan = build_install_plan(Some(installation), &[], &desired, &available);

        assert_eq!(plan.actions[0].action, PlanActionKind::Install);
        assert_eq!(plan.actions[0].installed_version, None);
        assert_eq!(
            plan.actions[0].available_version,
            Some(Version::parse("7.70").unwrap())
        );
    }

    #[test]
    fn plans_keep_for_reaper_when_target_app_exists() {
        let dir = tempdir().unwrap();
        let app_path = dir.path().join("reaper.exe");
        fs::write(&app_path, b"").unwrap();
        let installation = fake_reaper_installation(
            app_path,
            dir.path().to_path_buf(),
            Some(Version::parse("7.69").unwrap()),
        );
        let desired = vec![PACKAGE_REAPER.to_string()];

        let plan = build_install_plan(Some(installation), &[], &desired, &[]);

        assert_eq!(plan.actions[0].action, PlanActionKind::Keep);
        assert_eq!(
            plan.actions[0].installed_version,
            Some(Version::parse("7.69").unwrap())
        );
        assert!(plan.actions[0].available_version.is_none());
    }

    #[test]
    fn plans_update_for_reaper_when_available_version_is_newer() {
        let dir = tempdir().unwrap();
        let app_path = dir.path().join("reaper.exe");
        fs::write(&app_path, b"").unwrap();
        let installation = fake_reaper_installation(
            app_path,
            dir.path().to_path_buf(),
            Some(Version::parse("7.68").unwrap()),
        );
        let desired = vec![PACKAGE_REAPER.to_string()];
        let available = vec![AvailablePackage {
            package_id: PACKAGE_REAPER.to_string(),
            version: Some(Version::parse("7.70").unwrap()),
        }];

        let plan = build_install_plan(Some(installation), &[], &desired, &available);

        assert_eq!(plan.actions[0].action, PlanActionKind::Update);
        assert_eq!(
            plan.actions[0].installed_version,
            Some(Version::parse("7.68").unwrap())
        );
        assert_eq!(
            plan.actions[0].available_version,
            Some(Version::parse("7.70").unwrap())
        );
    }

    fn fake_reaper_installation(
        app_path: std::path::PathBuf,
        resource_path: std::path::PathBuf,
        version: Option<Version>,
    ) -> Installation {
        Installation {
            kind: InstallationKind::Portable,
            platform: Platform::Windows,
            app_path,
            resource_path,
            version,
            architecture: None,
            writable: true,
            confidence: Confidence::High,
            evidence: Vec::new(),
        }
    }
}
