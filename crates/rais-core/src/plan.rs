use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{ComponentDetection, Installation};
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
    ManualReview,
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
        let detection = detections_by_id.get(package_id.as_str()).copied();
        let available = available_by_id.get(package_id.as_str()).copied();

        let installed = detection.is_some_and(|detection| detection.installed);
        let installed_version = detection.and_then(|detection| detection.version.clone());
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
            (
                PlanActionKind::ManualReview,
                "Package is installed, but its installed version could not be detected."
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

#[cfg(test)]
mod tests {
    use crate::model::{ComponentDetection, Confidence};
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK};
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
    fn plans_manual_review_when_installed_version_is_unknown() {
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

        assert_eq!(plan.actions[0].action, PlanActionKind::ManualReview);
    }
}
