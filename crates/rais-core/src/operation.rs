use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::artifact::{
    ArtifactDescriptor, ArtifactKind, CachedArtifact, download_artifacts, resolve_latest_artifacts,
};
use crate::detection::detect_components;
use crate::install::{InstallFileReport, InstallOptions, InstallReport, install_cached_artifacts};
use crate::model::{Architecture, ComponentDetection, Platform};
use crate::plan::PlanActionKind;
use crate::preflight::ensure_resource_path_ready;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageOperationOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub replace_osara_keymap: bool,
    pub target_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageOperationReport {
    pub resource_path: PathBuf,
    pub dry_run: bool,
    pub install_report: Option<InstallReport>,
    pub items: Vec<PackageOperationItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageOperationItem {
    pub package_id: String,
    pub plan_action: PlanActionKind,
    pub status: PackageOperationStatus,
    pub artifact: ArtifactDescriptor,
    pub cached_artifact: Option<CachedArtifact>,
    pub install_action: Option<InstallFileReport>,
    pub manual_instruction: Option<ManualInstallInstruction>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualInstallInstruction {
    pub title: String,
    pub steps: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageOperationStatus {
    InstalledOrChecked,
    SkippedUnsupported,
    SkippedCurrent,
    SkippedManualReview,
}

pub fn execute_package_operation(
    resource_path: &Path,
    package_ids: &[String],
    platform: Platform,
    architecture: Architecture,
    cache_dir: &Path,
    options: &PackageOperationOptions,
) -> Result<PackageOperationReport> {
    let artifacts = resolve_latest_artifacts(package_ids, platform, architecture)?;
    let detections = detect_components(resource_path, platform)?;
    execute_resolved_package_operation_with_detections(
        resource_path,
        artifacts,
        &detections,
        cache_dir,
        options,
    )
}

pub fn execute_resolved_package_operation(
    resource_path: &Path,
    artifacts: Vec<ArtifactDescriptor>,
    cache_dir: &Path,
    options: &PackageOperationOptions,
) -> Result<PackageOperationReport> {
    execute_resolved_package_operation_with_detections(
        resource_path,
        artifacts,
        &[],
        cache_dir,
        options,
    )
}

pub fn execute_resolved_package_operation_with_detections(
    resource_path: &Path,
    artifacts: Vec<ArtifactDescriptor>,
    detections: &[ComponentDetection],
    cache_dir: &Path,
    options: &PackageOperationOptions,
) -> Result<PackageOperationReport> {
    ensure_resource_path_ready(resource_path, options.dry_run)?;

    let mut items = Vec::new();
    let mut planned_artifacts = Vec::new();

    for artifact in artifacts {
        let plan_action = plan_action_for_artifact(&artifact, detections);
        match plan_action {
            PlanActionKind::Install | PlanActionKind::Update => {
                planned_artifacts.push(PlannedArtifact {
                    artifact,
                    plan_action,
                });
            }
            PlanActionKind::Keep => items.push(skipped_current_item(artifact, detections)),
            PlanActionKind::ManualReview => items.push(manual_review_item(artifact, detections)),
        }
    }

    let (installable, unsupported): (Vec<_>, Vec<_>) = planned_artifacts
        .into_iter()
        .partition(|planned| planned.artifact.kind == ArtifactKind::ExtensionBinary);

    let staged_unsupported = if options.stage_unsupported && !unsupported.is_empty() {
        let artifacts = unsupported
            .iter()
            .map(|planned| planned.artifact.clone())
            .collect::<Vec<_>>();
        download_artifacts(&artifacts, cache_dir)?
    } else {
        Vec::new()
    };

    if options.stage_unsupported {
        items.extend(
            unsupported
                .iter()
                .map(|planned| {
                    let cached = staged_unsupported
                        .iter()
                        .find(|cached| cached.descriptor.package_id == planned.artifact.package_id)
                        .cloned();
                    skipped_item(
                        planned.artifact.clone(),
                        planned.plan_action,
                        resource_path,
                        cached,
                        options.target_app_path.as_deref(),
                        options.replace_osara_keymap,
                    )
                })
                .collect::<Vec<_>>(),
        );
    } else {
        items.extend(
            unsupported
                .into_iter()
                .map(|planned| {
                    skipped_item(
                        planned.artifact,
                        planned.plan_action,
                        resource_path,
                        None,
                        options.target_app_path.as_deref(),
                        options.replace_osara_keymap,
                    )
                })
                .collect::<Vec<_>>(),
        );
    }

    let cached_artifacts = if installable.is_empty() {
        Vec::new()
    } else {
        let artifacts = installable
            .iter()
            .map(|planned| planned.artifact.clone())
            .collect::<Vec<_>>();
        download_artifacts(&artifacts, cache_dir)?
    };

    let install_report = if cached_artifacts.is_empty() {
        None
    } else {
        Some(install_cached_artifacts(
            resource_path,
            &cached_artifacts,
            &InstallOptions {
                dry_run: options.dry_run,
                allow_reaper_running: options.allow_reaper_running,
                target_app_path: options.target_app_path.clone(),
            },
        )?)
    };

    if let Some(install_report) = &install_report {
        for ((planned, cached), action) in installable
            .iter()
            .zip(cached_artifacts.iter())
            .zip(&install_report.actions)
        {
            items.push(PackageOperationItem {
                package_id: cached.descriptor.package_id.clone(),
                plan_action: planned.plan_action,
                status: PackageOperationStatus::InstalledOrChecked,
                artifact: cached.descriptor.clone(),
                cached_artifact: Some(cached.clone()),
                install_action: Some(action.clone()),
                manual_instruction: None,
                message: "Single extension binary handled by RAIS installer.".to_string(),
            });
        }
    }

    items.sort_by(|left, right| left.package_id.cmp(&right.package_id));

    Ok(PackageOperationReport {
        resource_path: resource_path.to_path_buf(),
        dry_run: options.dry_run,
        install_report,
        items,
    })
}

#[derive(Debug, Clone)]
struct PlannedArtifact {
    artifact: ArtifactDescriptor,
    plan_action: PlanActionKind,
}

fn plan_action_for_artifact(
    artifact: &ArtifactDescriptor,
    detections: &[ComponentDetection],
) -> PlanActionKind {
    let Some(detection) = detections
        .iter()
        .find(|detection| detection.package_id == artifact.package_id)
    else {
        return PlanActionKind::Install;
    };

    if !detection.installed {
        return PlanActionKind::Install;
    }

    let Some(installed_version) = &detection.version else {
        return PlanActionKind::ManualReview;
    };

    if installed_version.cmp_lenient(&artifact.version).is_lt() {
        PlanActionKind::Update
    } else {
        PlanActionKind::Keep
    }
}

fn skipped_current_item(
    artifact: ArtifactDescriptor,
    detections: &[ComponentDetection],
) -> PackageOperationItem {
    let installed_version = detections
        .iter()
        .find(|detection| detection.package_id == artifact.package_id)
        .and_then(|detection| detection.version.as_ref())
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown".to_string());

    PackageOperationItem {
        package_id: artifact.package_id.clone(),
        plan_action: PlanActionKind::Keep,
        status: PackageOperationStatus::SkippedCurrent,
        message: format!(
            "Installed version {installed_version} is current or newer than available version {}.",
            artifact.version
        ),
        artifact,
        cached_artifact: None,
        install_action: None,
        manual_instruction: None,
    }
}

fn manual_review_item(
    artifact: ArtifactDescriptor,
    _detections: &[ComponentDetection],
) -> PackageOperationItem {
    PackageOperationItem {
        package_id: artifact.package_id.clone(),
        plan_action: PlanActionKind::ManualReview,
        status: PackageOperationStatus::SkippedManualReview,
        message: "Package is installed, but RAIS could not detect its installed version; leaving it unchanged.".to_string(),
        artifact,
        cached_artifact: None,
        install_action: None,
        manual_instruction: None,
    }
}

fn skipped_item(
    artifact: ArtifactDescriptor,
    plan_action: PlanActionKind,
    resource_path: &Path,
    cached_artifact: Option<CachedArtifact>,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> PackageOperationItem {
    let manual_instruction = Some(manual_instruction_for_artifact(
        &artifact,
        cached_artifact.as_ref(),
        resource_path,
        target_app_path,
        replace_osara_keymap,
    ));
    PackageOperationItem {
        package_id: artifact.package_id.clone(),
        plan_action,
        status: PackageOperationStatus::SkippedUnsupported,
        message: if cached_artifact.is_some() {
            format!(
                "Artifact kind {:?} requires a dedicated installer implementation. It was staged in the cache but not executed.",
                artifact.kind
            )
        } else {
            format!(
                "Artifact kind {:?} requires a dedicated installer implementation and was not downloaded or executed.",
                artifact.kind
            )
        },
        artifact,
        cached_artifact,
        install_action: None,
        manual_instruction,
    }
}

fn manual_instruction_for_artifact(
    artifact: &ArtifactDescriptor,
    cached_artifact: Option<&CachedArtifact>,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> ManualInstallInstruction {
    let artifact_location = cached_artifact
        .map(|cached| cached.path.display().to_string())
        .unwrap_or_else(|| artifact.url.clone());
    build_manual_instruction(
        &artifact.package_id,
        artifact.kind,
        artifact_access_step(artifact.kind, &artifact_location),
        resource_path,
        target_app_path,
        replace_osara_keymap,
    )
}

pub fn preview_manual_instruction(
    package_id: &str,
    kind: ArtifactKind,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> ManualInstallInstruction {
    build_manual_instruction(
        package_id,
        kind,
        preview_artifact_access_step(kind),
        resource_path,
        target_app_path,
        replace_osara_keymap,
    )
}

fn build_manual_instruction(
    package_id: &str,
    kind: ArtifactKind,
    artifact_access: String,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> ManualInstallInstruction {
    let mut steps = vec![artifact_access];
    let mut notes = vec![
        "RAIS has not yet implemented a package-specific automated installer for this artifact kind.".to_string(),
        "Close REAPER before running the installer or copying extension files.".to_string(),
    ];

    match package_id {
        crate::package::PACKAGE_OSARA => {
            steps.extend(osara_manual_steps(
                kind,
                resource_path,
                replace_osara_keymap,
            ));
            notes.push(
                "OSARA's Windows installer supports standard and portable REAPER targets; preserve an existing key map unless the user explicitly chooses replacement."
                    .to_string(),
            );
            if replace_osara_keymap {
                notes.push(
                    format!(
                        "The selected workflow replaces the current key map. Back up {} before replacing it with the OSARA key map.",
                        resource_path.join("reaper-kb.ini").display()
                    )
                );
            } else {
                notes.push(format!(
                    "The selected workflow preserves the current key map. Leave {} unchanged.",
                    resource_path.join("reaper-kb.ini").display()
                ));
            }
        }
        crate::package::PACKAGE_SWS => {
            steps.extend(sws_manual_steps(kind, resource_path));
            notes.push(
                format!(
                    "The SWS installer should target the REAPER installation that uses this resource folder: {}.",
                    resource_path.display()
                )
            );
        }
        crate::package::PACKAGE_REAPER => {
            steps.extend(reaper_manual_steps(kind, resource_path, target_app_path));
            notes.push(
                "REAPER application installers are not executed by this RAIS engine slice yet."
                    .to_string(),
            );
            if target_likely_portable(resource_path, target_app_path) {
                notes.push(
                    format!(
                        "This looks like a portable target. REAPER application files and reaper.ini should end up under {}.",
                        resource_path.display()
                    )
                );
            } else if let Some(target_app_path) = target_app_path {
                notes.push(format!(
                    "This target may require administrator approval if REAPER is installed to {}.",
                    reaper_install_destination(target_app_path).display()
                ));
            }
        }
        _ => {
            steps.push(format!(
                "Install or extract the package for this REAPER target: {}",
                resource_path.display()
            ));
        }
    }

    steps.push(
        "Return to RAIS and run detection again to verify the installed version.".to_string(),
    );

    ManualInstallInstruction {
        title: format!(
            "Manual install required for {}",
            package_title_name(package_id)
        ),
        steps,
        notes,
    }
}

fn artifact_access_step(kind: ArtifactKind, artifact_location: &str) -> String {
    match kind {
        ArtifactKind::Installer => format!("Run this installer: {artifact_location}"),
        ArtifactKind::Archive => format!("Extract this archive: {artifact_location}"),
        ArtifactKind::DiskImage => format!("Open this disk image: {artifact_location}"),
        ArtifactKind::ExtensionBinary => format!("Use this extension file: {artifact_location}"),
    }
}

fn preview_artifact_access_step(kind: ArtifactKind) -> String {
    match kind {
        ArtifactKind::Installer => {
            "RAIS will download the upstream installer during the run.".to_string()
        }
        ArtifactKind::Archive => {
            "RAIS will download the upstream archive during the run.".to_string()
        }
        ArtifactKind::DiskImage => "RAIS will download the disk image during the run.".to_string(),
        ArtifactKind::ExtensionBinary => {
            "RAIS will use the extension file resolved for this target during the run.".to_string()
        }
    }
}

fn osara_manual_steps(
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

fn sws_manual_steps(kind: ArtifactKind, resource_path: &Path) -> Vec<String> {
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

fn reaper_manual_steps(
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

fn target_likely_portable(resource_path: &Path, target_app_path: Option<&Path>) -> bool {
    target_app_path
        .is_some_and(|target_app_path| path_is_same_or_nested(target_app_path, resource_path))
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

fn package_title_name(package_id: &str) -> &'static str {
    match package_id {
        crate::package::PACKAGE_REAPER => "REAPER",
        crate::package::PACKAGE_OSARA => "OSARA",
        crate::package::PACKAGE_SWS => "SWS",
        crate::package::PACKAGE_REAPACK => "ReaPack",
        _ => "package",
    }
}

fn path_is_same_or_nested(path: &Path, root: &Path) -> bool {
    let path = normalize_path_for_match(path);
    let root = normalize_path_for_match(root);
    path == root || path.starts_with(&root)
}

fn normalize_path_for_match(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        PackageOperationOptions, PackageOperationStatus, execute_resolved_package_operation,
        execute_resolved_package_operation_with_detections,
    };
    use crate::artifact::{ArtifactDescriptor, ArtifactKind};
    use crate::error::RaisError;
    use crate::model::{Architecture, ComponentDetection, Confidence, Platform};
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_REAPER};
    use crate::plan::PlanActionKind;
    use crate::version::Version;

    #[test]
    fn skips_unsupported_artifacts_without_install_report() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation(
            dir.path(),
            vec![artifact(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara.exe",
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(report.install_report.is_none());
        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::SkippedUnsupported
        );
        assert!(report.items[0].manual_instruction.is_some());
        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .notes
                .iter()
                .any(|note| note.contains("preserves the current key map"))
        );
    }

    #[test]
    fn sorts_report_items_by_package_id() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation(
            dir.path(),
            vec![
                artifact(PACKAGE_REAPACK, ArtifactKind::Installer, "reapack.exe"),
                artifact(PACKAGE_OSARA, ArtifactKind::Installer, "osara.exe"),
            ],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert_eq!(report.items[0].package_id, PACKAGE_OSARA);
        assert_eq!(report.items[1].package_id, PACKAGE_REAPACK);
    }

    #[test]
    fn stages_unsupported_artifacts_when_requested() {
        let resource_dir = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("osara.exe");
        fs::write(&source_path, b"installer").unwrap();
        let report = execute_resolved_package_operation(
            resource_dir.path(),
            vec![artifact_with_url(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara.exe",
                &source_path.display().to_string(),
            )],
            cache_dir.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: true,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(report.install_report.is_none());
        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::SkippedUnsupported
        );
        assert!(report.items[0].cached_artifact.is_some());
        assert!(
            report.items[0]
                .message
                .contains("staged in the cache but not executed")
        );
    }

    #[test]
    fn skips_current_artifacts_before_download() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation_with_detections(
            dir.path(),
            vec![artifact(
                PACKAGE_REAPACK,
                ArtifactKind::ExtensionBinary,
                "reaper_reapack-x64.dll",
            )],
            &[detection(PACKAGE_REAPACK, Some("1.2.3"))],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(report.install_report.is_none());
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].plan_action, PlanActionKind::Keep);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::SkippedCurrent
        );
        assert!(report.items[0].cached_artifact.is_none());
    }

    #[test]
    fn skips_installed_unknown_version_for_manual_review() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation_with_detections(
            dir.path(),
            vec![artifact(
                PACKAGE_REAPACK,
                ArtifactKind::ExtensionBinary,
                "reaper_reapack-x64.dll",
            )],
            &[detection(PACKAGE_REAPACK, None)],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(report.install_report.is_none());
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].plan_action, PlanActionKind::ManualReview);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::SkippedManualReview
        );
    }

    #[test]
    fn osara_manual_instruction_reflects_replace_keymap_choice() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation(
            dir.path(),
            vec![artifact(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara.exe",
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: true,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .notes
                .iter()
                .any(|note| note.contains("Back up") && note.contains("reaper-kb.ini"))
        );
    }

    #[test]
    fn staged_unsupported_instruction_points_to_cached_artifact() {
        let resource_dir = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("osara.exe");
        fs::write(&source_path, b"installer").unwrap();

        let report = execute_resolved_package_operation(
            resource_dir.path(),
            vec![artifact_with_url(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara.exe",
                &source_path.display().to_string(),
            )],
            cache_dir.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: true,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        let cached_path = report.items[0]
            .cached_artifact
            .as_ref()
            .unwrap()
            .path
            .display()
            .to_string();
        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .steps
                .iter()
                .any(|step| step.contains(&cached_path))
        );
    }

    #[test]
    fn reaper_manual_instruction_mentions_portable_install_folder() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact(
                PACKAGE_REAPER,
                ArtifactKind::Installer,
                "reaper-install.exe",
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .steps
                .iter()
                .any(|step| step.contains("Portable install") && step.contains("PortableREAPER"))
        );
    }

    #[test]
    fn osara_manual_instruction_mentions_selected_resource_path() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation(
            dir.path(),
            vec![artifact(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara.exe",
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: true,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        )
        .unwrap();

        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .steps
                .iter()
                .any(|step| step.contains(&dir.path().display().to_string()))
        );
    }

    #[test]
    fn preview_manual_instruction_uses_preview_download_step() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("reaper.exe"), b"stub").unwrap();
        let instruction = super::preview_manual_instruction(
            PACKAGE_REAPER,
            ArtifactKind::Installer,
            dir.path(),
            Some(&dir.path().join("reaper.exe")),
            false,
        );

        assert!(instruction.steps[0].contains("download the upstream installer"));
        assert!(
            instruction
                .steps
                .iter()
                .any(|step| step.contains("Portable install"))
        );
    }

    #[test]
    fn fails_target_preflight_before_attempting_download() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("ProtectedREAPER");
        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(dir.path(), permissions).unwrap();

        let result = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_REAPACK,
                ArtifactKind::ExtensionBinary,
                "reaper_reapack-x64.dll",
                "http://example.test/reaper_reapack-x64.dll",
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: None,
            },
        );

        let mut restored = fs::metadata(dir.path()).unwrap().permissions();
        restored.set_readonly(false);
        fs::set_permissions(dir.path(), restored).unwrap();

        match result.unwrap_err() {
            RaisError::PreflightFailed { message } => {
                assert!(message.contains("resource-path"));
                assert!(message.contains("read-only"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn artifact(package_id: &str, kind: ArtifactKind, file_name: &str) -> ArtifactDescriptor {
        artifact_with_url(
            package_id,
            kind,
            file_name,
            &format!("https://example.test/{file_name}"),
        )
    }

    fn artifact_with_url(
        package_id: &str,
        kind: ArtifactKind,
        file_name: &str,
        url: &str,
    ) -> ArtifactDescriptor {
        ArtifactDescriptor {
            package_id: package_id.to_string(),
            version: Version::parse("1.2.3").unwrap(),
            platform: Platform::Windows,
            architecture: Architecture::X64,
            kind,
            url: url.to_string(),
            file_name: file_name.to_string(),
        }
    }

    fn detection(package_id: &str, version: Option<&str>) -> ComponentDetection {
        ComponentDetection {
            package_id: package_id.to_string(),
            display_name: package_id.to_string(),
            installed: true,
            version: version.map(|version| Version::parse(version).unwrap()),
            detector: "test".to_string(),
            confidence: Confidence::High,
            files: Vec::new(),
            notes: Vec::new(),
        }
    }
}
