use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::artifact::{
    ArtifactDescriptor, ArtifactKind, CachedArtifact, download_artifacts, expected_artifact_kind,
    resolve_latest_artifacts,
};
use crate::detection::{
    default_standard_installation, detect_components, matching_user_plugin_files,
};
use crate::error::{IoPathContext, RaisError};
use crate::hash::sha256_file;
use crate::install::{InstallFileReport, InstallOptions, InstallReport, install_cached_artifacts};
use crate::model::{Architecture, ComponentDetection, Platform};
use crate::package::package_specs_by_id;
use crate::plan::PlanActionKind;
use crate::preflight::ensure_resource_path_ready;
use crate::receipt::{
    InstallState, RECEIPT_RELATIVE_PATH, load_install_state, receipt_path, save_install_state,
    upsert_package_receipt,
};
use crate::rollback::{BackupManifest, BackupManifestFile, save_backup_manifest};
use crate::upstream::{execute_planned_execution, verify_planned_execution_paths};

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
    pub receipt_backup_path: Option<PathBuf>,
    pub receipt_backup_manifest_path: Option<PathBuf>,
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
    pub backup_paths: Vec<PathBuf>,
    pub backup_manifest_path: Option<PathBuf>,
    pub planned_execution: Option<PlannedExecutionPlan>,
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
pub enum PlannedAutomationKind {
    VendorInstaller,
    ArchiveExtraction,
    DiskImageInstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlannedExecutionKind {
    LaunchInstallerExecutable,
    ExtractArchiveAndRunInstaller,
    MountDiskImageAndRunInstaller,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedExecutionPlan {
    pub kind: PlannedExecutionKind,
    pub artifact_location: String,
    pub program: Option<String>,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub verification_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageAutomationSupport {
    Direct,
    AvailableUnattended(PlannedAutomationKind),
    PlannedUnattended(PlannedAutomationKind),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageOperationStatus {
    InstalledOrChecked,
    PlannedUnattended,
    DeferredUnattended,
    SkippedCurrent,
    SkippedManualReview,
}

#[derive(Debug, Default, Clone)]
struct UnattendedPostInstallReport {
    backup_paths: Vec<PathBuf>,
    backup_manifest_path: Option<PathBuf>,
}

pub fn package_automation_support(
    package_id: &str,
    platform: Platform,
    architecture: Architecture,
) -> PackageAutomationSupport {
    match (
        package_id,
        expected_artifact_kind(package_id, platform, architecture),
    ) {
        (_, Ok(ArtifactKind::ExtensionBinary)) => PackageAutomationSupport::Direct,
        (crate::package::PACKAGE_REAPER, Ok(ArtifactKind::Installer))
            if matches!(platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        (crate::package::PACKAGE_OSARA, Ok(ArtifactKind::Installer))
            if matches!(platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        (crate::package::PACKAGE_SWS, Ok(ArtifactKind::Installer))
            if matches!(platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        (_, Ok(ArtifactKind::Installer)) => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::VendorInstaller)
        }
        (_, Ok(ArtifactKind::Archive)) => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::ArchiveExtraction)
        }
        (_, Ok(ArtifactKind::DiskImage)) => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::DiskImageInstall)
        }
        (_, Err(_)) => PackageAutomationSupport::Unavailable,
    }
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
    let mut direct_installable = Vec::new();
    let mut unattended_installable = Vec::new();
    let mut deferred_installable = Vec::new();

    for artifact in artifacts {
        let plan_action = plan_action_for_artifact(&artifact, detections);
        match plan_action {
            PlanActionKind::Install | PlanActionKind::Update => {
                match automation_support_for_artifact(&artifact, options) {
                    PackageAutomationSupport::Direct => direct_installable.push(PlannedArtifact {
                        artifact,
                        plan_action,
                    }),
                    PackageAutomationSupport::AvailableUnattended(_) => unattended_installable
                        .push(PlannedArtifact {
                            artifact,
                            plan_action,
                        }),
                    PackageAutomationSupport::PlannedUnattended(_)
                    | PackageAutomationSupport::Unavailable => {
                        deferred_installable.push(PlannedArtifact {
                            artifact,
                            plan_action,
                        })
                    }
                }
            }
            PlanActionKind::Keep => items.push(skipped_current_item(artifact, detections)),
            PlanActionKind::ManualReview => items.push(manual_review_item(artifact, detections)),
        }
    }

    let staged_deferred = if options.stage_unsupported && !deferred_installable.is_empty() {
        let artifacts = deferred_installable
            .iter()
            .map(|planned| planned.artifact.clone())
            .collect::<Vec<_>>();
        download_artifacts(&artifacts, cache_dir)?
    } else {
        Vec::new()
    };

    if options.stage_unsupported {
        items.extend(
            deferred_installable
                .iter()
                .map(|planned| {
                    let cached = staged_deferred
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
            deferred_installable
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

    let mut receipt_backup_path = None;
    let mut receipt_backup_manifest_path = None;
    let mut unattended_state = if options.dry_run || unattended_installable.is_empty() {
        None
    } else {
        Some(load_install_state(resource_path)?.unwrap_or_default())
    };
    let mut unattended_receipts_updated = false;

    if options.dry_run {
        items.extend(unattended_installable.into_iter().map(|planned| {
            planned_unattended_item(
                planned.artifact,
                planned.plan_action,
                resource_path,
                options.target_app_path.as_deref(),
                options.replace_osara_keymap,
            )
        }));
    } else if !unattended_installable.is_empty() {
        let artifacts = unattended_installable
            .iter()
            .map(|planned| planned.artifact.clone())
            .collect::<Vec<_>>();
        let cached_unattended = download_artifacts(&artifacts, cache_dir)?;
        for (planned, cached) in unattended_installable.iter().zip(cached_unattended.iter()) {
            items.push(executed_unattended_item(
                planned,
                cached,
                resource_path,
                options.target_app_path.as_deref(),
                options.replace_osara_keymap,
            )?);
            if let Some(state) = &mut unattended_state {
                upsert_unattended_package_receipt(
                    state,
                    resource_path,
                    &planned.artifact,
                    cached,
                    options.target_app_path.as_deref(),
                    options.replace_osara_keymap,
                )?;
                unattended_receipts_updated = true;
            }
        }
        if unattended_receipts_updated {
            let backup_id = operation_timestamp();
            let backup_set = resource_path.join("RAIS").join("backups").join(&backup_id);
            receipt_backup_path = backup_receipt_if_present(resource_path, &backup_set)?;
            if let Some(path) = &receipt_backup_path {
                receipt_backup_manifest_path = Some(write_receipt_backup_manifest(
                    &backup_set,
                    &backup_id,
                    path,
                )?);
            }
            if let Some(state) = &unattended_state {
                save_install_state(resource_path, state)?;
            }
        }
    }

    let cached_artifacts = if direct_installable.is_empty() {
        Vec::new()
    } else {
        let artifacts = direct_installable
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
        for ((planned, cached), action) in direct_installable
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
                backup_paths: Vec::new(),
                backup_manifest_path: None,
                planned_execution: None,
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
        receipt_backup_path,
        receipt_backup_manifest_path,
        items,
    })
}

fn automation_support_for_artifact(
    artifact: &ArtifactDescriptor,
    _options: &PackageOperationOptions,
) -> PackageAutomationSupport {
    match artifact.kind {
        ArtifactKind::ExtensionBinary => PackageAutomationSupport::Direct,
        ArtifactKind::Installer
            if artifact.package_id == crate::package::PACKAGE_REAPER
                && matches!(artifact.platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        ArtifactKind::Installer
            if artifact.package_id == crate::package::PACKAGE_OSARA
                && matches!(artifact.platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        ArtifactKind::Installer
            if artifact.package_id == crate::package::PACKAGE_SWS
                && matches!(artifact.platform, Platform::Windows) =>
        {
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        }
        ArtifactKind::Installer => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::VendorInstaller)
        }
        ArtifactKind::Archive => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::ArchiveExtraction)
        }
        ArtifactKind::DiskImage => {
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::DiskImageInstall)
        }
    }
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
        backup_paths: Vec::new(),
        backup_manifest_path: None,
        planned_execution: None,
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
        backup_paths: Vec::new(),
        backup_manifest_path: None,
        planned_execution: None,
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
    let planned_execution = Some(planned_execution_for_artifact(
        &artifact,
        cached_artifact.as_ref(),
        resource_path,
        target_app_path,
        replace_osara_keymap,
    ));
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
        status: PackageOperationStatus::DeferredUnattended,
        message: if cached_artifact.is_some() {
            format!(
                "This build has not implemented the planned unattended {} execution path yet. RAIS staged the artifact in the cache but did not run it.",
                planned_automation_description(artifact.kind)
            )
        } else {
            format!(
                "This build has not implemented the planned unattended {} execution path yet. RAIS did not download or run the artifact.",
                planned_automation_description(artifact.kind)
            )
        },
        artifact,
        cached_artifact,
        install_action: None,
        backup_paths: Vec::new(),
        backup_manifest_path: None,
        planned_execution,
        manual_instruction,
    }
}

fn planned_unattended_item(
    artifact: ArtifactDescriptor,
    plan_action: PlanActionKind,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> PackageOperationItem {
    let planned_execution = Some(planned_execution_for_artifact(
        &artifact,
        None,
        resource_path,
        target_app_path,
        replace_osara_keymap,
    ));
    PackageOperationItem {
        package_id: artifact.package_id.clone(),
        plan_action,
        status: PackageOperationStatus::PlannedUnattended,
        message: format!(
            "Dry run: RAIS would download and run this {} unattended.",
            planned_automation_description(artifact.kind)
        ),
        artifact,
        cached_artifact: None,
        install_action: None,
        backup_paths: Vec::new(),
        backup_manifest_path: None,
        planned_execution,
        manual_instruction: None,
    }
}

fn executed_unattended_item(
    planned: &PlannedArtifact,
    cached_artifact: &CachedArtifact,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Result<PackageOperationItem> {
    let planned_execution = planned_execution_for_artifact(
        &planned.artifact,
        Some(cached_artifact),
        resource_path,
        target_app_path,
        replace_osara_keymap,
    );
    execute_planned_execution(&planned_execution, false)?;
    let post_install = post_execute_unattended_artifact(
        &planned.artifact,
        resource_path,
        target_app_path,
        replace_osara_keymap,
    )?;
    verify_planned_execution_paths(&planned_execution)?;

    let message = if planned.artifact.package_id == crate::package::PACKAGE_OSARA
        && replace_osara_keymap
    {
        if post_install.backup_paths.is_empty() {
            "RAIS ran the upstream installer unattended, applied the OSARA key map replacement, and updated the RAIS receipt."
                .to_string()
        } else {
            "RAIS ran the upstream installer unattended, backed up reaper-kb.ini, applied the OSARA key map replacement, and updated the RAIS receipt."
                .to_string()
        }
    } else {
        "RAIS ran the upstream installer unattended, verified the expected target paths, and updated the RAIS receipt."
            .to_string()
    };

    Ok(PackageOperationItem {
        package_id: planned.artifact.package_id.clone(),
        plan_action: planned.plan_action,
        status: PackageOperationStatus::InstalledOrChecked,
        message,
        artifact: planned.artifact.clone(),
        cached_artifact: Some(cached_artifact.clone()),
        install_action: None,
        backup_paths: post_install.backup_paths,
        backup_manifest_path: post_install.backup_manifest_path,
        planned_execution: Some(planned_execution),
        manual_instruction: None,
    })
}

fn upsert_unattended_package_receipt(
    state: &mut InstallState,
    resource_path: &Path,
    artifact: &ArtifactDescriptor,
    cached_artifact: &CachedArtifact,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Result<()> {
    let installed_paths = receipt_paths_for_artifact(
        artifact,
        resource_path,
        target_app_path,
        replace_osara_keymap,
    )?;
    upsert_package_receipt(
        state,
        resource_path,
        &artifact.package_id,
        Some(artifact.version.clone()),
        Some(artifact.url.clone()),
        Some(cached_artifact.sha256.clone()),
        &installed_paths,
        Some(operation_timestamp()),
        Some(artifact.architecture),
    )
}

fn receipt_paths_for_artifact(
    artifact: &ArtifactDescriptor,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Result<Vec<PathBuf>> {
    let effective_target_app_path =
        effective_target_app_path(artifact, resource_path, target_app_path);
    let mut paths = Vec::new();

    if artifact.package_id == crate::package::PACKAGE_REAPER {
        if let Some(path) = effective_target_app_path
            .as_ref()
            .filter(|path| path.exists())
        {
            paths.push(path.clone());
            if target_likely_portable(resource_path, Some(path)) {
                let ini_path = resource_path.join("reaper.ini");
                if ini_path.exists() {
                    paths.push(ini_path);
                }
            }
        }
    }

    let package_specs = package_specs_by_id(artifact.platform);
    if let Some(spec) = package_specs.get(&artifact.package_id) {
        paths.extend(matching_user_plugin_files(
            resource_path,
            artifact.platform,
            spec,
        )?);
    }

    match artifact.package_id.as_str() {
        crate::package::PACKAGE_OSARA => {
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
        }
        crate::package::PACKAGE_SWS => {
            let script_path = resource_path.join("Scripts").join("sws_python.py");
            if script_path.exists() {
                paths.push(script_path);
            }
            let grooves_path = resource_path.join("Data").join("Grooves");
            if grooves_path.exists() {
                paths.push(grooves_path);
            }
        }
        _ => {}
    }

    paths.sort();
    paths.dedup();

    if paths.is_empty() {
        return Err(RaisError::PostInstallVerificationFailed {
            missing_paths: planned_verification_paths(
                artifact,
                resource_path,
                effective_target_app_path.as_deref(),
                replace_osara_keymap,
            ),
        });
    }

    Ok(paths)
}

fn post_execute_unattended_artifact(
    artifact: &ArtifactDescriptor,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Result<UnattendedPostInstallReport> {
    let mut report = UnattendedPostInstallReport::default();

    if artifact.package_id == crate::package::PACKAGE_OSARA
        && matches!(artifact.platform, Platform::Windows)
    {
        if target_likely_portable(resource_path, target_app_path) {
            let uninstall_path = resource_path.join("osara").join("uninstall.exe");
            if uninstall_path.is_file() {
                std::fs::remove_file(&uninstall_path).with_path(&uninstall_path)?;
            }
        }

        if replace_osara_keymap {
            report = apply_osara_keymap_replacement(resource_path)?;
        }
    }

    Ok(report)
}

fn apply_osara_keymap_replacement(resource_path: &Path) -> Result<UnattendedPostInstallReport> {
    let replacement_source = resource_path.join("KeyMaps").join("OSARA.ReaperKeyMap");
    if !replacement_source.is_file() {
        return Err(crate::error::RaisError::PostInstallVerificationFailed {
            missing_paths: vec![replacement_source],
        });
    }

    let current_keymap = resource_path.join("reaper-kb.ini");
    let mut report = UnattendedPostInstallReport::default();

    if current_keymap.is_file() {
        let (backup_path, backup_manifest_path) = backup_file_for_unattended_change(
            resource_path,
            crate::package::PACKAGE_OSARA,
            &current_keymap,
            "osara-keymap-replacement",
        )?;
        report.backup_paths.push(backup_path);
        report.backup_manifest_path = Some(backup_manifest_path);
    }

    replace_file_from_source(&replacement_source, &current_keymap)?;
    Ok(report)
}

fn backup_file_for_unattended_change(
    resource_path: &Path,
    package_id: &str,
    source_path: &Path,
    reason: &str,
) -> Result<(PathBuf, PathBuf)> {
    let relative_path = source_path
        .strip_prefix(resource_path)
        .map_err(|_| crate::error::RaisError::InvalidPlannedExecution {
            message: format!(
                "backup source is outside the selected resource path: {}",
                source_path.display()
            ),
        })?
        .to_path_buf();
    let backup_id = operation_timestamp();
    let backup_set = resource_path.join("RAIS").join("backups").join(&backup_id);
    let backup_path = backup_set.join(&relative_path);

    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent).with_path(parent)?;
    }
    std::fs::copy(source_path, &backup_path).with_path(&backup_path)?;

    let manifest_path = save_backup_manifest(
        &backup_set,
        &BackupManifest {
            schema_version: 1,
            rais_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: backup_id,
            reason: reason.to_string(),
            files: vec![BackupManifestFile {
                package_id: Some(package_id.to_string()),
                original_path: relative_path,
                backup_path: backup_path.clone(),
                size: std::fs::metadata(&backup_path)
                    .with_path(&backup_path)?
                    .len(),
                sha256: sha256_file(&backup_path)?,
            }],
            receipt_backup_path: None,
        },
    )?;

    Ok((backup_path, manifest_path))
}

fn backup_receipt_if_present(resource_path: &Path, backup_set: &Path) -> Result<Option<PathBuf>> {
    let source_path = receipt_path(resource_path);
    if !source_path.is_file() {
        return Ok(None);
    }

    let backup_path = backup_set.join(RECEIPT_RELATIVE_PATH);
    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent).with_path(parent)?;
    }
    std::fs::copy(&source_path, &backup_path).with_path(&backup_path)?;
    Ok(Some(backup_path))
}

fn write_receipt_backup_manifest(
    backup_set: &Path,
    created_at: &str,
    receipt_backup_path: &Path,
) -> Result<PathBuf> {
    save_backup_manifest(
        backup_set,
        &BackupManifest {
            schema_version: 1,
            rais_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: created_at.to_string(),
            reason: "unattended-receipt-update".to_string(),
            files: vec![BackupManifestFile {
                package_id: None,
                original_path: PathBuf::from(RECEIPT_RELATIVE_PATH),
                backup_path: receipt_backup_path.to_path_buf(),
                size: std::fs::metadata(receipt_backup_path)
                    .with_path(receipt_backup_path)?
                    .len(),
                sha256: sha256_file(receipt_backup_path)?,
            }],
            receipt_backup_path: Some(receipt_backup_path.to_path_buf()),
        },
    )
}

fn replace_file_from_source(source_path: &Path, target_path: &Path) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).with_path(parent)?;
    }

    let temp_path = temporary_target_path(target_path);
    if temp_path.exists() {
        std::fs::remove_file(&temp_path).with_path(&temp_path)?;
    }

    std::fs::copy(source_path, &temp_path).with_path(&temp_path)?;

    if target_path.exists() {
        std::fs::remove_file(target_path).with_path(target_path)?;
    }

    std::fs::rename(&temp_path, target_path).with_path(target_path)
}

fn temporary_target_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("target");
    target_path.with_file_name(format!("{file_name}.rais-tmp"))
}

fn operation_timestamp() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "unattended-unix-{}-{:09}",
        duration.as_secs(),
        duration.subsec_nanos()
    )
}

fn planned_execution_for_artifact(
    artifact: &ArtifactDescriptor,
    cached_artifact: Option<&CachedArtifact>,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> PlannedExecutionPlan {
    let artifact_location = cached_artifact
        .map(|cached| cached.path.display().to_string())
        .unwrap_or_else(|| artifact.url.clone());
    let effective_target_app_path =
        effective_target_app_path(artifact, resource_path, target_app_path);
    let verification_paths = planned_verification_paths(
        artifact,
        resource_path,
        effective_target_app_path.as_deref(),
        replace_osara_keymap,
    );

    match artifact.kind {
        ArtifactKind::Installer => PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            program: Some(artifact_location.clone()),
            arguments: installer_arguments_for_artifact(
                artifact,
                resource_path,
                effective_target_app_path.as_deref(),
            ),
            working_directory: cached_artifact
                .and_then(|cached| cached.path.parent().map(Path::to_path_buf)),
            artifact_location,
            verification_paths,
        },
        ArtifactKind::Archive => PlannedExecutionPlan {
            kind: PlannedExecutionKind::ExtractArchiveAndRunInstaller,
            program: None,
            arguments: Vec::new(),
            working_directory: cached_artifact
                .and_then(|cached| cached.path.parent().map(Path::to_path_buf)),
            artifact_location,
            verification_paths,
        },
        ArtifactKind::DiskImage => PlannedExecutionPlan {
            kind: PlannedExecutionKind::MountDiskImageAndRunInstaller,
            program: None,
            arguments: Vec::new(),
            working_directory: None,
            artifact_location,
            verification_paths,
        },
        ArtifactKind::ExtensionBinary => PlannedExecutionPlan {
            kind: PlannedExecutionKind::LaunchInstallerExecutable,
            program: Some(artifact_location.clone()),
            arguments: Vec::new(),
            working_directory: cached_artifact
                .and_then(|cached| cached.path.parent().map(Path::to_path_buf)),
            artifact_location,
            verification_paths,
        },
    }
}

fn installer_arguments_for_artifact(
    artifact: &ArtifactDescriptor,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Vec<String> {
    if artifact.package_id == crate::package::PACKAGE_REAPER
        && artifact.kind == ArtifactKind::Installer
        && matches!(artifact.platform, Platform::Windows)
    {
        return reaper_windows_installer_arguments(resource_path, target_app_path);
    }
    if artifact.package_id == crate::package::PACKAGE_OSARA
        && artifact.kind == ArtifactKind::Installer
        && matches!(artifact.platform, Platform::Windows)
    {
        return osara_windows_installer_arguments(resource_path);
    }
    if artifact.package_id == crate::package::PACKAGE_SWS
        && artifact.kind == ArtifactKind::Installer
        && matches!(artifact.platform, Platform::Windows)
    {
        return sws_windows_installer_arguments(resource_path);
    }

    Vec::new()
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

fn osara_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
    vec!["/S".to_string(), format!("/D={}", resource_path.display())]
}

fn sws_windows_installer_arguments(resource_path: &Path) -> Vec<String> {
    vec!["/S".to_string(), format!("/D={}", resource_path.display())]
}

fn effective_target_app_path(
    artifact: &ArtifactDescriptor,
    resource_path: &Path,
    target_app_path: Option<&Path>,
) -> Option<PathBuf> {
    target_app_path
        .map(Path::to_path_buf)
        .or_else(|| inferred_target_app_path(artifact.platform, resource_path))
}

fn inferred_target_app_path(platform: Platform, resource_path: &Path) -> Option<PathBuf> {
    if let Some(standard) = default_standard_installation(platform)
        .filter(|installation| installation.resource_path == resource_path)
    {
        return Some(standard.app_path);
    }

    Some(portable_target_app_path(platform, resource_path))
}

fn portable_target_app_path(platform: Platform, resource_path: &Path) -> PathBuf {
    match platform {
        Platform::Windows => resource_path.join("reaper.exe"),
        Platform::MacOs => resource_path.join("REAPER.app"),
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
        format!(
            "RAIS is designed to launch and complete this package through an unattended {} flow, but this build still requires manual completion.",
            planned_automation_description(kind)
        ),
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
                "REAPER application installers should be launched and completed by RAIS itself in supported builds, but this engine slice does not execute them yet."
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

fn planned_automation_description(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Installer => "vendor installer",
        ArtifactKind::Archive => "archive extraction",
        ArtifactKind::DiskImage => "disk image install",
        ArtifactKind::ExtensionBinary => "direct file install",
    }
}

fn planned_verification_paths(
    artifact: &ArtifactDescriptor,
    resource_path: &Path,
    target_app_path: Option<&Path>,
    replace_osara_keymap: bool,
) -> Vec<PathBuf> {
    let mut paths = match artifact.package_id.as_str() {
        crate::package::PACKAGE_REAPER => {
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
        crate::package::PACKAGE_OSARA => {
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
        crate::package::PACKAGE_SWS => {
            let mut paths = vec![resource_path.join("UserPlugins")];
            if let Some(plugin_path) = sws_primary_plugin_path(resource_path, artifact) {
                paths.push(plugin_path);
            }
            paths
        }
        crate::package::PACKAGE_REAPACK => {
            vec![resource_path.join("UserPlugins")]
        }
        crate::package::PACKAGE_REAKONTROL => {
            vec![resource_path.join("UserPlugins")]
        }
        _ => vec![resource_path.to_path_buf()],
    };

    paths.sort();
    paths.dedup();
    paths
}

fn sws_primary_plugin_path(resource_path: &Path, artifact: &ArtifactDescriptor) -> Option<PathBuf> {
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
        crate::package::PACKAGE_REAKONTROL => "ReaKontrol",
        _ => "package",
    }
}

fn path_is_same_or_nested(path: &Path, root: &Path) -> bool {
    let path = normalize_path_for_match(path);
    let root = normalize_path_for_match(root);
    path == root || path.starts_with(&root)
}

fn normalize_path_for_match(path: &Path) -> PathBuf {
    strip_windows_verbatim_prefix(
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
    )
}

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let raw = path.display().to_string();
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        PackageAutomationSupport, PackageOperationOptions, PackageOperationStatus,
        PlannedAutomationKind, PlannedExecutionKind, execute_resolved_package_operation,
        execute_resolved_package_operation_with_detections,
    };
    use crate::artifact::{ArtifactDescriptor, ArtifactKind};
    use crate::detection::detect_components;
    use crate::error::RaisError;
    use crate::model::{Architecture, ComponentDetection, Confidence, Platform};
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS};
    use crate::plan::PlanActionKind;
    use crate::receipt::{InstallState, load_install_state, save_install_state};
    use crate::version::Version;

    #[test]
    fn skips_deferred_artifacts_without_install_report() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let report = execute_resolved_package_operation(
            dir.path(),
            vec![artifact(
                PACKAGE_REAPER,
                ArtifactKind::DiskImage,
                "reaper.dmg",
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
            PackageOperationStatus::DeferredUnattended
        );
        assert!(report.items[0].manual_instruction.is_some());
        assert!(
            report.items[0]
                .manual_instruction
                .as_ref()
                .unwrap()
                .notes
                .iter()
                .any(|note| note.contains("manual completion"))
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
        let source_path = source_dir.path().join("reapack-installer.exe");
        fs::write(&source_path, b"installer").unwrap();
        let report = execute_resolved_package_operation(
            resource_dir.path(),
            vec![artifact_with_url(
                PACKAGE_REAPACK,
                ArtifactKind::Installer,
                "reapack-installer.exe",
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
            PackageOperationStatus::DeferredUnattended
        );
        assert!(report.items[0].cached_artifact.is_some());
        assert!(
            report.items[0]
                .message
                .contains("staged the artifact in the cache but did not run it")
        );
    }

    #[test]
    fn staged_installer_exposes_launch_plan_with_cached_path() {
        let resource_dir = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();
        let source_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("reapack-installer.exe");
        fs::write(&source_path, b"installer").unwrap();

        let report = execute_resolved_package_operation(
            resource_dir.path(),
            vec![artifact_with_url(
                PACKAGE_REAPACK,
                ArtifactKind::Installer,
                "reapack-installer.exe",
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

        let plan = report.items[0].planned_execution.as_ref().unwrap();
        let cached_path = report.items[0]
            .cached_artifact
            .as_ref()
            .unwrap()
            .path
            .display()
            .to_string();

        assert_eq!(plan.kind, PlannedExecutionKind::LaunchInstallerExecutable);
        assert_eq!(plan.artifact_location, cached_path);
        assert_eq!(plan.program.as_deref(), Some(cached_path.as_str()));
        assert_eq!(
            plan.working_directory.as_deref(),
            report.items[0]
                .cached_artifact
                .as_ref()
                .unwrap()
                .path
                .parent()
        );
        assert!(
            plan.verification_paths
                .contains(&resource_dir.path().join("UserPlugins"))
        );
    }

    #[test]
    fn reports_planned_unattended_support_for_installer_artifacts() {
        assert_eq!(
            super::package_automation_support(PACKAGE_REAPER, Platform::Windows, Architecture::X64),
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        );
        assert_eq!(
            super::package_automation_support(PACKAGE_OSARA, Platform::Windows, Architecture::X64),
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        );
        assert_eq!(
            super::package_automation_support(PACKAGE_SWS, Platform::Windows, Architecture::X64),
            PackageAutomationSupport::AvailableUnattended(PlannedAutomationKind::VendorInstaller)
        );
        assert_eq!(
            super::package_automation_support(PACKAGE_OSARA, Platform::MacOs, Architecture::Arm64),
            PackageAutomationSupport::PlannedUnattended(PlannedAutomationKind::ArchiveExtraction)
        );
        assert_eq!(
            super::package_automation_support(
                PACKAGE_REAPACK,
                Platform::Windows,
                Architecture::X64
            ),
            PackageAutomationSupport::Direct
        );
    }

    #[test]
    fn dry_run_reaper_windows_uses_unattended_plan() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        std::fs::write(resource_path.join("reaper.ini"), b"portable").unwrap();

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

        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::PlannedUnattended
        );
        assert!(report.items[0].manual_instruction.is_none());
        assert_eq!(
            report.items[0]
                .planned_execution
                .as_ref()
                .unwrap()
                .arguments,
            vec![
                "/PORTABLE".to_string(),
                "/S".to_string(),
                format!("/D={}", resource_path.display()),
            ]
        );
    }

    #[test]
    fn dry_run_reaper_windows_standard_verifies_app_without_resource_directory() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("Roaming").join("REAPER");
        let target_app_path = dir
            .path()
            .join("Program Files")
            .join("REAPER")
            .join("reaper.exe");

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
                target_app_path: Some(target_app_path.clone()),
            },
        )
        .unwrap();

        let plan = report.items[0].planned_execution.as_ref().unwrap();

        assert_eq!(plan.kind, PlannedExecutionKind::LaunchInstallerExecutable);
        assert_eq!(
            plan.arguments,
            vec![
                "/S".to_string(),
                format!("/D={}", target_app_path.parent().unwrap().display())
            ]
        );
        assert_eq!(plan.verification_paths, vec![target_app_path]);
    }

    #[test]
    fn dry_run_osara_windows_preserve_uses_unattended_plan() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
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
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::PlannedUnattended
        );
        assert!(report.items[0].manual_instruction.is_none());
        assert_eq!(
            report.items[0]
                .planned_execution
                .as_ref()
                .unwrap()
                .arguments,
            vec!["/S".to_string(), format!("/D={}", resource_path.display()),]
        );
    }

    #[test]
    fn dry_run_osara_windows_replace_uses_unattended_plan() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
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
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::PlannedUnattended
        );
        assert!(report.items[0].manual_instruction.is_none());
        assert!(
            report.items[0]
                .planned_execution
                .as_ref()
                .unwrap()
                .verification_paths
                .contains(&resource_path.join("reaper-kb.ini"))
        );
    }

    #[test]
    fn dry_run_sws_windows_uses_unattended_plan() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact(
                PACKAGE_SWS,
                ArtifactKind::Installer,
                "sws-installer.exe",
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

        assert_eq!(report.items.len(), 1);
        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::PlannedUnattended
        );
        assert!(report.items[0].manual_instruction.is_none());
        assert_eq!(
            report.items[0]
                .planned_execution
                .as_ref()
                .unwrap()
                .arguments,
            vec!["/S".to_string(), format!("/D={}", resource_path.display()),]
        );
        assert!(
            report.items[0]
                .planned_execution
                .as_ref()
                .unwrap()
                .verification_paths
                .contains(&resource_path.join("UserPlugins").join("reaper_sws-x64.dll"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn executes_reaper_windows_portable_installer_unattended_and_writes_receipt() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("reaper-installer.cmd");
        std::fs::write(&source_path, reaper_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_REAPER,
                ArtifactKind::Installer,
                "reaper-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert!(report.items[0].message.contains("updated the RAIS receipt"));

        let state = load_install_state(&resource_path).unwrap().unwrap();
        let receipt = state.packages.get(PACKAGE_REAPER).unwrap();
        assert_eq!(receipt.version.as_ref().unwrap().raw(), "1.2.3");
        assert!(
            receipt
                .installed_files
                .iter()
                .any(|file| file.path == PathBuf::from("reaper.exe"))
        );
        assert!(
            receipt
                .installed_files
                .iter()
                .any(|file| file.path == PathBuf::from("reaper.ini"))
        );

        let detections = detect_components(&resource_path, Platform::Windows).unwrap();
        let reaper = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_REAPER)
            .unwrap();
        assert!(reaper.installed);
        assert_eq!(reaper.detector, "rais-receipt");
        assert_eq!(reaper.version.as_ref().unwrap().raw(), "1.2.3");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn executes_reaper_windows_standard_installer_and_receipt_tracks_app_only() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("reaper-installer.cmd");
        std::fs::write(&source_path, reaper_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("AppData").join("Roaming").join("REAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        let target_app_path = dir
            .path()
            .join("Program Files")
            .join("REAPER")
            .join("reaper.exe");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_REAPER,
                ArtifactKind::Installer,
                "reaper-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(target_app_path.clone()),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert!(target_app_path.is_file());
        assert!(!resource_path.join("reaper.ini").exists());

        let state = load_install_state(&resource_path).unwrap().unwrap();
        let receipt = state.packages.get(PACKAGE_REAPER).unwrap();
        assert_eq!(receipt.installed_files.len(), 1);
        assert_eq!(receipt.installed_files[0].path, target_app_path);
    }

    #[test]
    fn executes_osara_windows_installer_unattended_and_cleans_portable_uninstaller() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("osara-installer.cmd");
        std::fs::write(&source_path, osara_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert!(resource_path.join("UserPlugins").is_dir());
        assert!(
            resource_path
                .join("KeyMaps")
                .join("OSARA.ReaperKeyMap")
                .is_file()
        );
        assert!(resource_path.join("osara").join("locale").is_dir());
        assert!(!resource_path.join("osara").join("uninstall.exe").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn executes_osara_windows_installer_unattended_and_replaces_keymap_with_backup() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("osara-installer.cmd");
        std::fs::write(&source_path, osara_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        std::fs::write(resource_path.join("reaper-kb.ini"), b"old keymap").unwrap();

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: true,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert_eq!(
            std::fs::read_to_string(resource_path.join("reaper-kb.ini")).unwrap(),
            "osara keymap\r\n"
        );
        assert_eq!(report.items[0].backup_paths.len(), 1);
        assert_eq!(
            std::fs::read(&report.items[0].backup_paths[0]).unwrap(),
            b"old keymap"
        );
        assert!(report.items[0].backup_manifest_path.is_some());
        assert!(
            report.items[0]
                .message
                .contains("applied the OSARA key map replacement")
        );
        assert!(!resource_path.join("osara").join("uninstall.exe").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn executes_osara_windows_installer_unattended_and_creates_keymap_for_new_portable_target() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("osara-installer.cmd");
        std::fs::write(&source_path, osara_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_OSARA,
                ArtifactKind::Installer,
                "osara-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: true,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert_eq!(
            std::fs::read_to_string(resource_path.join("reaper-kb.ini")).unwrap(),
            "osara keymap\r\n"
        );
        assert!(report.items[0].backup_paths.is_empty());
        assert!(report.items[0].backup_manifest_path.is_none());
        assert!(
            report.items[0]
                .message
                .contains("applied the OSARA key map replacement")
        );
        assert!(!resource_path.join("osara").join("uninstall.exe").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn executes_sws_windows_installer_unattended() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source_path = dir.path().join("sws-installer.cmd");
        std::fs::write(&source_path, sws_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![artifact_with_url(
                PACKAGE_SWS,
                ArtifactKind::Installer,
                "sws-installer.cmd",
                &source_path.display().to_string(),
            )],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert_eq!(
            report.items[0].status,
            PackageOperationStatus::InstalledOrChecked
        );
        assert!(
            resource_path
                .join("UserPlugins")
                .join("reaper_sws-x64.dll")
                .is_file()
        );
        assert!(
            resource_path
                .join("Scripts")
                .join("sws_python.py")
                .is_file()
        );
        assert!(resource_path.join("Data").join("Grooves").is_dir());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unattended_installers_backup_existing_receipt_once_and_merge_package_state() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let osara_source = dir.path().join("osara-installer.cmd");
        std::fs::write(&osara_source, osara_mock_installer_script()).unwrap();
        let sws_source = dir.path().join("sws-installer.cmd");
        std::fs::write(&sws_source, sws_mock_installer_script()).unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        save_install_state(&resource_path, &InstallState::default()).unwrap();

        let report = execute_resolved_package_operation(
            &resource_path,
            vec![
                artifact_with_url(
                    PACKAGE_OSARA,
                    ArtifactKind::Installer,
                    "osara-installer.cmd",
                    &osara_source.display().to_string(),
                ),
                artifact_with_url(
                    PACKAGE_SWS,
                    ArtifactKind::Installer,
                    "sws-installer.cmd",
                    &sws_source.display().to_string(),
                ),
            ],
            cache.path(),
            &PackageOperationOptions {
                dry_run: false,
                allow_reaper_running: false,
                stage_unsupported: false,
                replace_osara_keymap: false,
                target_app_path: Some(resource_path.join("reaper.exe")),
            },
        )
        .unwrap();

        assert!(
            report
                .receipt_backup_path
                .as_ref()
                .is_some_and(|path| path.is_file())
        );
        assert!(
            report
                .receipt_backup_manifest_path
                .as_ref()
                .is_some_and(|path| path.is_file())
        );
        assert_eq!(
            std::fs::read_dir(resource_path.join("RAIS").join("backups"))
                .unwrap()
                .count(),
            1
        );

        let state = load_install_state(&resource_path).unwrap().unwrap();
        assert!(state.packages.contains_key(PACKAGE_OSARA));
        assert!(state.packages.contains_key(PACKAGE_SWS));
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
        let instruction = super::preview_manual_instruction(
            PACKAGE_OSARA,
            ArtifactKind::Installer,
            dir.path(),
            None,
            true,
        );

        assert!(
            instruction
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
        let source_path = source_dir.path().join("reapack-installer.exe");
        fs::write(&source_path, b"installer").unwrap();

        let report = execute_resolved_package_operation(
            resource_dir.path(),
            vec![artifact_with_url(
                PACKAGE_REAPACK,
                ArtifactKind::Installer,
                "reapack-installer.exe",
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
        let resource_path = dir.path().join("PortableREAPER");
        let instruction = super::preview_manual_instruction(
            PACKAGE_REAPER,
            ArtifactKind::Installer,
            &resource_path,
            Some(&resource_path.join("reaper.exe")),
            false,
        );

        assert!(
            instruction
                .steps
                .iter()
                .any(|step| step.contains("Portable install") && step.contains("PortableREAPER"))
        );
    }

    #[test]
    fn reaper_portable_plan_verifies_app_and_reaper_ini() {
        let dir = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        let target_app_path = resource_path.join("reaper.exe");
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
                target_app_path: Some(target_app_path.clone()),
            },
        )
        .unwrap();

        let plan = report.items[0].planned_execution.as_ref().unwrap();

        assert_eq!(plan.kind, PlannedExecutionKind::LaunchInstallerExecutable);
        assert!(
            plan.verification_paths.contains(&target_app_path),
            "missing target app path in verification set: {:?}",
            plan.verification_paths
        );
        assert!(
            plan.verification_paths
                .contains(&resource_path.join("reaper.ini")),
            "missing reaper.ini in verification set: {:?}",
            plan.verification_paths
        );
    }

    #[test]
    fn osara_manual_instruction_mentions_selected_resource_path() {
        let dir = tempdir().unwrap();
        let instruction = super::preview_manual_instruction(
            PACKAGE_OSARA,
            ArtifactKind::Installer,
            dir.path(),
            None,
            false,
        );

        assert!(
            instruction
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

    #[cfg(target_os = "windows")]
    fn osara_mock_installer_script() -> &'static str {
        r#"@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "DEST="
:next
if "%~1"=="" goto args_done
set "ARG=%~1"
if /I "!ARG:~0,3!"=="/D=" set "DEST=!ARG:~3!"
shift
goto next
:args_done
if "%DEST%"=="" exit /b 4
mkdir "%DEST%\UserPlugins" 2>nul
mkdir "%DEST%\KeyMaps" 2>nul
mkdir "%DEST%\osara\locale" 2>nul
echo osara dll> "%DEST%\UserPlugins\reaper_osara64.dll"
echo osara keymap> "%DEST%\KeyMaps\OSARA.ReaperKeyMap"
echo en locale> "%DEST%\osara\locale\en.po"
echo uninstall> "%DEST%\osara\uninstall.exe"
exit /b 0
"#
    }

    #[cfg(target_os = "windows")]
    fn reaper_mock_installer_script() -> &'static str {
        r#"@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "DEST="
set "PORTABLE=0"
:next
if "%~1"=="" goto args_done
set "ARG=%~1"
if /I "!ARG!"=="/PORTABLE" set "PORTABLE=1"
if /I "!ARG:~0,3!"=="/D=" set "DEST=!ARG:~3!"
shift
goto next
:args_done
if "%DEST%"=="" exit /b 4
mkdir "%DEST%" 2>nul
echo reaper exe> "%DEST%\reaper.exe"
if "%PORTABLE%"=="1" echo portable ini> "%DEST%\reaper.ini"
exit /b 0
"#
    }

    #[cfg(target_os = "windows")]
    fn sws_mock_installer_script() -> &'static str {
        r#"@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "DEST="
:next
if "%~1"=="" goto args_done
set "ARG=%~1"
if /I "!ARG:~0,3!"=="/D=" set "DEST=!ARG:~3!"
shift
goto next
:args_done
if "%DEST%"=="" exit /b 4
mkdir "%DEST%\UserPlugins" 2>nul
mkdir "%DEST%\Scripts" 2>nul
mkdir "%DEST%\Data\Grooves" 2>nul
type nul > "%DEST%\UserPlugins\reaper_sws-x64.dll"
type nul > "%DEST%\Scripts\sws_python.py"
type nul > "%DEST%\Data\Grooves\default.rgt"
exit /b 0
"#
    }
}
