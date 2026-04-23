use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use rais_core::artifact::default_cache_dir;
use rais_core::detection::{DiscoveryOptions, detect_components, discover_installations};
use rais_core::latest::fetch_latest_versions;
use rais_core::localization::{DEFAULT_LOCALE, Localizer};
use rais_core::model::{Architecture, Installation, InstallationKind, Platform};
use rais_core::operation::PackageOperationStatus;
use rais_core::package::{PackageSpec, builtin_package_specs, default_desired_package_ids};
use rais_core::plan::{InstallPlan, PlanAction, PlanActionKind, build_install_plan};
use rais_core::resource::ResourceInitActionKind;
use rais_core::setup::{SetupOptions, SetupReport, execute_setup_operation};
use rais_core::{RaisError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiBootstrapOptions {
    pub locale: String,
    pub locales_dir: Option<PathBuf>,
    pub portable_roots: Vec<PathBuf>,
    pub online_versions: bool,
}

impl Default for UiBootstrapOptions {
    fn default() -> Self {
        Self {
            locale: DEFAULT_LOCALE.to_string(),
            locales_dir: None,
            portable_roots: Vec::new(),
            online_versions: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardModel {
    pub window_title: String,
    pub platform: Platform,
    pub architecture: Architecture,
    pub text: WizardText,
    pub current_step: WizardStep,
    pub steps: Vec<WizardStepLabel>,
    pub target_rows: Vec<TargetRow>,
    pub selected_target_index: Option<usize>,
    pub package_rows: Vec<PackageRow>,
    pub review_lines: Vec<String>,
    pub notes: Vec<String>,
    pub controls: WizardControls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardText {
    pub target_heading: String,
    pub target_choice_label: String,
    pub target_details_label: String,
    pub target_empty: String,
    pub target_portable_choice: String,
    pub target_portable_folder_label: String,
    pub target_portable_folder_message: String,
    pub target_portable_pending_details: String,
    pub target_custom_portable_label: String,
    pub target_custom_portable_path_label: String,
    pub target_custom_portable_writable_label: String,
    pub target_custom_portable_note: String,
    pub packages_heading: String,
    pub packages_list_label: String,
    pub package_details_label: String,
    pub review_heading: String,
    pub review_target_prefix: String,
    pub review_no_target: String,
    pub review_no_package: String,
    pub progress_heading: String,
    pub progress_status: String,
    pub progress_status_running: String,
    pub done_heading: String,
    pub done_status: String,
    pub done_status_success: String,
    pub done_status_error: String,
    pub done_status_no_packages: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Target,
    Packages,
    Review,
    Progress,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardStepLabel {
    pub step: WizardStep,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRow {
    pub label: String,
    pub details: String,
    pub path: PathBuf,
    pub portable: bool,
    pub selected: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRow {
    pub package_id: String,
    pub display_name: String,
    pub selected: bool,
    pub summary: String,
    pub installed_version: String,
    pub available_version: String,
    pub action: PlanActionKind,
    pub action_label: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardControls {
    pub back_label: String,
    pub next_label: String,
    pub install_label: String,
    pub close_label: String,
    pub can_go_back: bool,
    pub can_go_next: bool,
    pub can_install: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInstallOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub cache_dir: Option<PathBuf>,
}

impl Default for WizardInstallOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            allow_reaper_running: false,
            stage_unsupported: true,
            cache_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInstallRequest {
    pub resource_path: PathBuf,
    pub package_ids: Vec<String>,
    pub platform: Platform,
    pub architecture: Architecture,
    pub portable: bool,
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInstallSummary {
    pub status_line: String,
    pub detail_lines: Vec<String>,
}

pub fn load_wizard_model(options: UiBootstrapOptions) -> Result<WizardModel> {
    let platform = Platform::current().ok_or(RaisError::UnsupportedPlatform)?;
    let architecture = Architecture::current();
    let localizer = localizer_from_options(&options)?;
    let installations = discover_installations(&DiscoveryOptions {
        include_standard: true,
        portable_roots: options.portable_roots,
    })?;
    let selected_target_index = installations
        .iter()
        .position(|installation| installation.writable);
    let target = selected_target_index.and_then(|index| installations.get(index).cloned());
    let detections = match target.as_ref() {
        Some(target) => detect_components(&target.resource_path, platform)?,
        None => Vec::new(),
    };
    let available = if options.online_versions {
        fetch_latest_versions()?
    } else {
        Vec::new()
    };
    let desired = default_desired_package_ids();
    let plan = build_install_plan(target, &detections, &desired, &available);

    Ok(model_from_plan(
        &localizer,
        platform,
        architecture,
        installations,
        selected_target_index,
        plan,
    ))
}

fn localizer_from_options(options: &UiBootstrapOptions) -> Result<Localizer> {
    match &options.locales_dir {
        Some(locales_dir) => Localizer::from_locale_dir(locales_dir, &options.locale),
        None => Localizer::embedded(&options.locale),
    }
}

pub fn model_from_plan(
    localizer: &Localizer,
    platform: Platform,
    architecture: Architecture,
    installations: Vec<Installation>,
    selected_target_index: Option<usize>,
    plan: InstallPlan,
) -> WizardModel {
    let package_specs = builtin_package_specs(platform);
    let target_rows = target_rows(localizer, &installations, selected_target_index);
    let package_rows = package_rows(localizer, &package_specs, &plan.actions);
    let review_lines = review_lines(localizer, &target_rows, &package_rows, &plan.notes);
    let can_install = package_rows
        .iter()
        .any(|row| matches!(row.action, PlanActionKind::Install | PlanActionKind::Update));

    WizardModel {
        window_title: localizer.text("app-title").value,
        platform,
        architecture,
        current_step: WizardStep::Target,
        steps: wizard_steps(localizer),
        selected_target_index,
        target_rows,
        package_rows,
        review_lines,
        notes: plan.notes,
        text: wizard_text(localizer),
        controls: WizardControls {
            back_label: localizer.text("wizard-button-back").value,
            next_label: localizer.text("wizard-button-next").value,
            install_label: localizer.text("wizard-button-install").value,
            close_label: localizer.text("wizard-button-close").value,
            can_go_back: false,
            can_go_next: selected_target_index.is_some(),
            can_install,
        },
    }
}

fn wizard_text(localizer: &Localizer) -> WizardText {
    WizardText {
        target_heading: localizer.text("wizard-target-heading").value,
        target_choice_label: localizer.text("wizard-target-choice-label").value,
        target_details_label: localizer.text("wizard-target-details-label").value,
        target_empty: localizer.text("wizard-target-empty").value,
        target_portable_choice: localizer.text("wizard-target-portable-choice").value,
        target_portable_folder_label: localizer.text("wizard-target-portable-folder-label").value,
        target_portable_folder_message: localizer
            .text("wizard-target-portable-folder-message")
            .value,
        target_portable_pending_details: localizer
            .text("wizard-target-portable-pending-details")
            .value,
        target_custom_portable_label: localizer.text("wizard-target-custom-portable-label").value,
        target_custom_portable_path_label: localizer
            .text("wizard-target-custom-portable-path-label")
            .value,
        target_custom_portable_writable_label: localizer
            .text("wizard-target-custom-portable-writable-label")
            .value,
        target_custom_portable_note: localizer.text("wizard-target-custom-portable-note").value,
        packages_heading: localizer.text("wizard-packages-heading").value,
        packages_list_label: localizer.text("wizard-packages-list-label").value,
        package_details_label: localizer.text("wizard-package-details-label").value,
        review_heading: localizer.text("wizard-review-heading").value,
        review_target_prefix: localizer.text("wizard-review-target-prefix").value,
        review_no_target: localizer.text("wizard-review-no-target").value,
        review_no_package: localizer.text("wizard-review-no-package").value,
        progress_heading: localizer.text("wizard-progress-heading").value,
        progress_status: localizer.text("wizard-progress-status-idle").value,
        done_heading: localizer.text("wizard-done-heading").value,
        done_status: localizer.text("wizard-done-status-idle").value,
        progress_status_running: localizer.text("wizard-progress-status-running").value,
        done_status_success: localizer.text("wizard-done-status-success").value,
        done_status_error: localizer.text("wizard-done-status-error").value,
        done_status_no_packages: localizer.text("wizard-done-status-no-packages").value,
    }
}

fn wizard_steps(localizer: &Localizer) -> Vec<WizardStepLabel> {
    [
        (WizardStep::Target, "wizard-step-target"),
        (WizardStep::Packages, "wizard-step-packages"),
        (WizardStep::Review, "wizard-step-review"),
        (WizardStep::Progress, "wizard-step-progress"),
        (WizardStep::Done, "wizard-step-done"),
    ]
    .into_iter()
    .map(|(step, key)| WizardStepLabel {
        step,
        label: localizer.text(key).value,
    })
    .collect()
}

fn target_rows(
    localizer: &Localizer,
    installations: &[Installation],
    selected_target_index: Option<usize>,
) -> Vec<TargetRow> {
    installations
        .iter()
        .enumerate()
        .map(|(index, installation)| {
            let kind = format!("{:?}", installation.kind);
            let version = installation
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| localizer.text("detect-version-unknown").value);
            TargetRow {
                label: localizer
                    .format(
                        "wizard-target-row",
                        &[
                            ("kind", kind.as_str()),
                            ("version", version.as_str()),
                            ("path", &installation.resource_path.display().to_string()),
                        ],
                    )
                    .value,
                details: localizer
                    .format(
                        "wizard-target-details",
                        &[
                            ("path", &installation.resource_path.display().to_string()),
                            (
                                "writable",
                                yes_no(localizer, installation.writable).as_str(),
                            ),
                            ("confidence", &format!("{:?}", installation.confidence)),
                        ],
                    )
                    .value,
                path: installation.resource_path.clone(),
                portable: installation.kind == InstallationKind::Portable,
                selected: Some(index) == selected_target_index,
                writable: installation.writable,
            }
        })
        .collect()
}

pub fn install_request_from_model(
    model: &WizardModel,
    selected_target_index: Option<usize>,
    selected_package_indices: &[usize],
    options: WizardInstallOptions,
) -> Result<WizardInstallRequest> {
    let target = selected_target_index
        .and_then(|index| model.target_rows.get(index))
        .ok_or_else(|| RaisError::PreflightFailed {
            message: "No REAPER installation target was selected.".to_string(),
        })?;

    install_request_from_target(model, target, selected_package_indices, options)
}

pub fn install_request_from_target(
    model: &WizardModel,
    target: &TargetRow,
    selected_package_indices: &[usize],
    options: WizardInstallOptions,
) -> Result<WizardInstallRequest> {
    if !target.writable {
        return Err(RaisError::PreflightFailed {
            message: format!(
                "Target resource path is not writable: {}",
                target.path.display()
            ),
        });
    }

    let package_ids = package_ids_for_indices(model, selected_package_indices);
    if package_ids.is_empty() {
        return Err(RaisError::PreflightFailed {
            message: "No package was selected for installation or update.".to_string(),
        });
    }

    Ok(WizardInstallRequest {
        resource_path: target.path.clone(),
        package_ids,
        platform: model.platform,
        architecture: model.architecture,
        portable: target.portable,
        dry_run: options.dry_run,
        allow_reaper_running: options.allow_reaper_running,
        stage_unsupported: options.stage_unsupported,
        cache_dir: options.cache_dir.unwrap_or_else(default_cache_dir),
    })
}

pub fn package_ids_for_indices(model: &WizardModel, indices: &[usize]) -> Vec<String> {
    let mut package_ids = Vec::new();
    for index in indices {
        let Some(row) = model.package_rows.get(*index) else {
            continue;
        };
        if !package_ids.contains(&row.package_id) {
            package_ids.push(row.package_id.clone());
        }
    }
    package_ids
}

pub fn review_lines_for_indices(
    model: &WizardModel,
    selected_target_index: Option<usize>,
    selected_package_indices: &[usize],
) -> Vec<String> {
    let target = selected_target_index.and_then(|index| model.target_rows.get(index));
    review_lines_for_target(model, target, selected_package_indices)
}

pub fn review_lines_for_target(
    model: &WizardModel,
    target: Option<&TargetRow>,
    selected_package_indices: &[usize],
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(target) = target {
        lines.push(format!(
            "{}: {}",
            model.text.review_target_prefix,
            target.path.display()
        ));
    } else {
        lines.push(model.text.review_no_target.clone());
    }

    let package_ids = package_ids_for_indices(model, selected_package_indices);
    if package_ids.is_empty() {
        lines.push(model.text.review_no_package.clone());
    } else {
        for package_id in package_ids {
            if let Some(package) = model
                .package_rows
                .iter()
                .find(|package| package.package_id == package_id)
            {
                lines.push(format!(
                    "{}: {}",
                    package.display_name, package.action_label
                ));
            }
        }
    }

    lines.extend(model.notes.iter().cloned());
    lines
}

pub fn custom_portable_target_row(model: &WizardModel, path: PathBuf, selected: bool) -> TargetRow {
    let writable = is_probably_writable(&path);
    let writable_text = if writable {
        "yes".to_string()
    } else {
        "no".to_string()
    };
    TargetRow {
        label: format!(
            "{}: {}",
            model.text.target_custom_portable_label,
            path.display()
        ),
        details: format!(
            "{}: {}\n{}: {}\n{}",
            model.text.target_custom_portable_path_label,
            path.display(),
            model.text.target_custom_portable_writable_label,
            writable_text,
            model.text.target_custom_portable_note
        ),
        path,
        portable: true,
        selected,
        writable,
    }
}

pub fn execute_wizard_install(request: WizardInstallRequest) -> Result<SetupReport> {
    execute_setup_operation(
        &request.resource_path,
        &request.package_ids,
        request.platform,
        request.architecture,
        &request.cache_dir,
        &SetupOptions {
            dry_run: request.dry_run,
            portable: request.portable,
            allow_reaper_running: request.allow_reaper_running,
            stage_unsupported: request.stage_unsupported,
        },
    )
}

pub fn summarize_setup_report(report: &SetupReport) -> WizardInstallSummary {
    let created_resources = report
        .resource_init
        .actions
        .iter()
        .filter(|action| action.action == ResourceInitActionKind::Created)
        .count();
    let installed_or_checked = report
        .package_operation
        .items
        .iter()
        .filter(|item| item.status == PackageOperationStatus::InstalledOrChecked)
        .count();
    let skipped_current = report
        .package_operation
        .items
        .iter()
        .filter(|item| item.status == PackageOperationStatus::SkippedCurrent)
        .count();
    let manual_items = report
        .package_operation
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                PackageOperationStatus::SkippedUnsupported
                    | PackageOperationStatus::SkippedManualReview
            )
        })
        .count();

    let mut detail_lines = vec![
        format!("Target: {}", report.resource_path.display()),
        format!("Dry run: {}", if report.dry_run { "yes" } else { "no" }),
        format!("Resource items created: {created_resources}"),
        format!("Packages installed or checked: {installed_or_checked}"),
        format!("Packages already current: {skipped_current}"),
        format!("Packages requiring manual attention: {manual_items}"),
    ];

    for item in &report.package_operation.items {
        detail_lines.push(format!("{}: {}", item.package_id, item.message));
        if let Some(manual) = &item.manual_instruction {
            detail_lines.push(format!("{}:", manual.title));
            detail_lines.extend(manual.steps.iter().map(|step| format!("  {step}")));
        }
    }

    WizardInstallSummary {
        status_line: format!(
            "Finished. {installed_or_checked} package item(s) installed or checked; {manual_items} require manual attention."
        ),
        detail_lines,
    }
}

fn package_rows(
    localizer: &Localizer,
    package_specs: &[PackageSpec],
    actions: &[PlanAction],
) -> Vec<PackageRow> {
    let specs_by_id: BTreeMap<_, _> = package_specs
        .iter()
        .map(|spec| (spec.id.as_str(), spec))
        .collect();
    actions
        .iter()
        .map(|action| {
            let display_name = specs_by_id
                .get(action.package_id.as_str())
                .map(|spec| localizer.text(&spec.display_name_key).value)
                .unwrap_or_else(|| action.package_id.clone());
            let installed_version = version_text(localizer, action.installed_version.as_ref());
            let available_version = version_text(localizer, action.available_version.as_ref());
            let action_label = action_label(localizer, action.action);
            PackageRow {
                package_id: action.package_id.clone(),
                summary: localizer
                    .format(
                        "wizard-package-row",
                        &[
                            ("package", display_name.as_str()),
                            ("action", action_label.as_str()),
                            ("installed", installed_version.as_str()),
                            ("available", available_version.as_str()),
                        ],
                    )
                    .value,
                display_name: display_name.clone(),
                selected: matches!(
                    action.action,
                    PlanActionKind::Install | PlanActionKind::Update
                ),
                installed_version,
                available_version,
                action: action.action,
                action_label,
                reason: action.reason.clone(),
            }
        })
        .collect()
}

fn review_lines(
    localizer: &Localizer,
    target_rows: &[TargetRow],
    package_rows: &[PackageRow],
    notes: &[String],
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(target) = target_rows.iter().find(|target| target.selected) {
        lines.push(
            localizer
                .format(
                    "wizard-review-target",
                    &[("path", &target.path.display().to_string())],
                )
                .value,
        );
    } else {
        lines.push(localizer.text("wizard-review-no-target").value);
    }

    for package in package_rows {
        lines.push(
            localizer
                .format(
                    "wizard-review-package",
                    &[
                        ("package", package.display_name.as_str()),
                        ("action", package.action_label.as_str()),
                    ],
                )
                .value,
        );
    }

    lines.extend(notes.iter().cloned());
    lines
}

fn version_text(localizer: &Localizer, version: Option<&rais_core::version::Version>) -> String {
    version
        .map(ToString::to_string)
        .unwrap_or_else(|| localizer.text("detect-version-unknown").value)
}

fn action_label(localizer: &Localizer, action: PlanActionKind) -> String {
    let key = match action {
        PlanActionKind::Install => "action-install",
        PlanActionKind::Update => "action-update",
        PlanActionKind::Keep => "action-keep",
        PlanActionKind::ManualReview => "action-review",
    };
    localizer.text(key).value
}

fn yes_no(localizer: &Localizer, value: bool) -> String {
    if value {
        localizer.text("common-yes").value
    } else {
        localizer.text("common-no").value
    }
}

fn is_probably_writable(path: &Path) -> bool {
    let existing_path = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    fs::metadata(existing_path)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rais_core::localization::{DEFAULT_LOCALE, Localizer};
    use rais_core::model::{Architecture, Confidence, Installation, InstallationKind, Platform};
    use rais_core::package::{PACKAGE_OSARA, PACKAGE_REAPACK};
    use rais_core::plan::{InstallPlan, PlanAction, PlanActionKind};
    use rais_core::version::Version;
    use tempfile::tempdir;

    use super::{
        UiBootstrapOptions, custom_portable_target_row, localizer_from_options, model_from_plan,
    };

    #[test]
    fn default_options_use_embedded_localization() {
        let options = UiBootstrapOptions::default();
        let localizer = localizer_from_options(&options).unwrap();

        assert_eq!(localizer.active_locale(), DEFAULT_LOCALE);
        assert!(localizer.source_path().is_none());
        assert_eq!(
            localizer.text("app-title").value,
            "REAPER Accessibility Installation Software"
        );
    }

    #[test]
    fn locale_directory_override_remains_available_for_development() {
        let dir = tempdir().unwrap();
        let locale_dir = dir.path().join("de-DE");
        std::fs::create_dir_all(&locale_dir).unwrap();
        std::fs::write(locale_dir.join("rais.ftl"), "app-title = RAIS Test\n").unwrap();
        let options = UiBootstrapOptions {
            locale: "de-DE".to_string(),
            locales_dir: Some(dir.path().to_path_buf()),
            portable_roots: Vec::new(),
            online_versions: false,
        };

        let localizer = localizer_from_options(&options).unwrap();

        assert_eq!(localizer.active_locale(), "de-DE");
        assert!(localizer.source_path().is_some());
        assert_eq!(localizer.text("app-title").value, "RAIS Test");
    }

    #[test]
    fn builds_initial_wizard_model_from_plan() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = fake_installation();
        let plan = InstallPlan {
            target: Some(installation.clone()),
            actions: vec![
                PlanAction {
                    package_id: PACKAGE_OSARA.to_string(),
                    action: PlanActionKind::Install,
                    installed_version: None,
                    available_version: Some(Version::parse("2026.1").unwrap()),
                    reason: "Missing".to_string(),
                },
                PlanAction {
                    package_id: PACKAGE_REAPACK.to_string(),
                    action: PlanActionKind::Keep,
                    installed_version: Some(Version::parse("1.2.6").unwrap()),
                    available_version: Some(Version::parse("1.2.6").unwrap()),
                    reason: "Current".to_string(),
                },
            ],
            notes: vec!["Review note".to_string()],
        };

        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![installation],
            Some(0),
            plan,
        );

        assert_eq!(
            model.window_title,
            "REAPER Accessibility Installation Software"
        );
        assert_eq!(model.steps.len(), 5);
        assert_eq!(model.target_rows.len(), 1);
        assert!(model.target_rows[0].selected);
        assert!(model.target_rows[0].portable);
        assert!(model.target_rows[0].details.contains("Writable"));
        assert_eq!(model.package_rows.len(), 2);
        assert_eq!(model.package_rows[0].display_name, "OSARA");
        assert!(model.package_rows[0].summary.contains("OSARA"));
        assert_eq!(model.package_rows[0].action_label, "Install");
        assert!(model.package_rows[0].selected);
        assert_eq!(model.package_rows[1].action_label, "Keep");
        assert!(!model.package_rows[1].selected);
        assert!(model.controls.can_go_next);
        assert!(model.controls.can_install);
        assert!(model.review_lines.iter().any(|line| line.contains("OSARA")));
    }

    #[test]
    fn disables_next_when_no_target_is_selected() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            Vec::new(),
            None,
            InstallPlan {
                target: None,
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );

        assert!(!model.controls.can_go_next);
        assert!(!model.controls.can_install);
        assert_eq!(model.review_lines[0], "No target selected.");
    }

    #[test]
    fn builds_install_request_from_selected_rows() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = fake_installation();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![installation],
            Some(0),
            InstallPlan {
                target: None,
                actions: vec![
                    PlanAction {
                        package_id: PACKAGE_OSARA.to_string(),
                        action: PlanActionKind::Install,
                        installed_version: None,
                        available_version: None,
                        reason: "Missing".to_string(),
                    },
                    PlanAction {
                        package_id: PACKAGE_REAPACK.to_string(),
                        action: PlanActionKind::Keep,
                        installed_version: None,
                        available_version: None,
                        reason: "Current".to_string(),
                    },
                ],
                notes: Vec::new(),
            },
        );

        let request = super::install_request_from_model(
            &model,
            Some(0),
            &[0],
            super::WizardInstallOptions {
                dry_run: true,
                allow_reaper_running: true,
                stage_unsupported: false,
                cache_dir: Some(PathBuf::from("C:/cache")),
            },
        )
        .unwrap();

        assert_eq!(request.resource_path, PathBuf::from("C:/REAPER"));
        assert_eq!(request.package_ids, vec![PACKAGE_OSARA.to_string()]);
        assert!(request.portable);
        assert!(request.dry_run);
        assert_eq!(request.cache_dir, PathBuf::from("C:/cache"));
    }

    #[test]
    fn install_request_requires_selected_package() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = fake_installation();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![installation],
            Some(0),
            InstallPlan {
                target: None,
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );

        let error = super::install_request_from_model(
            &model,
            Some(0),
            &[],
            super::WizardInstallOptions::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("No package"));
    }

    #[test]
    fn builds_custom_portable_target_row() {
        let dir = tempdir().unwrap();
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            Vec::new(),
            None,
            InstallPlan {
                target: None,
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );

        let row = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);

        assert!(row.selected);
        assert!(row.portable);
        assert!(row.writable);
        assert!(row.label.contains("Portable REAPER folder"));
        assert!(row.details.contains("Portable resource path"));
    }

    fn fake_installation() -> Installation {
        Installation {
            kind: InstallationKind::Portable,
            platform: Platform::Windows,
            app_path: PathBuf::from("C:/REAPER/reaper.exe"),
            resource_path: PathBuf::from("C:/REAPER"),
            version: Some(Version::parse("7.69").unwrap()),
            architecture: Some(Architecture::X64),
            writable: true,
            confidence: Confidence::High,
            evidence: Vec::new(),
        }
    }
}
