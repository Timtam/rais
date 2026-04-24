use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use rais_core::artifact::{default_cache_dir, expected_artifact_kind};
use rais_core::detection::{
    DiscoveryOptions, default_standard_installation, detect_components, discover_installations,
};
use rais_core::latest::fetch_latest_versions;
use rais_core::localization::{DEFAULT_LOCALE, Localizer};
use rais_core::metadata::file_version;
use rais_core::model::{Architecture, Confidence, Installation, InstallationKind, Platform};
use rais_core::operation::{
    PackageAutomationSupport, PackageOperationStatus, PlannedExecutionKind,
    package_automation_support, preview_manual_instruction,
};
use rais_core::package::{
    BackupPolicy, PACKAGE_OSARA, PackageSpec, builtin_package_specs, package_specs_by_id,
};
use rais_core::plan::{
    AvailablePackage, InstallPlan, PlanAction, PlanActionKind, build_install_plan,
};
use rais_core::report::{default_report_path, save_json_and_text_reports};
use rais_core::resource::{
    ResourceInitActionKind, ResourceInitItemKind, ResourceInitOptions, ResourceInitReport,
    initialize_resource_path,
};
use rais_core::setup::{SetupOptions, SetupReport, execute_setup_operation};
use rais_core::version::Version;
use rais_core::{RaisError, Result};
use serde::Serialize;

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
    pub bootstrap_options: UiBootstrapOptions,
    pub current_step: WizardStep,
    pub steps: Vec<WizardStepLabel>,
    pub target_rows: Vec<TargetRow>,
    pub selected_target_index: Option<usize>,
    pub package_rows: Vec<PackageRow>,
    pub available_packages: Vec<AvailablePackage>,
    pub review_lines: Vec<String>,
    pub notes: Vec<String>,
    pub controls: WizardControls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardText {
    pub common_yes: String,
    pub common_no: String,
    pub target_heading: String,
    pub target_choice_label: String,
    pub target_details_label: String,
    pub target_empty: String,
    pub target_portable_choice: String,
    pub target_portable_folder_label: String,
    pub target_portable_folder_message: String,
    pub target_portable_pending_details: String,
    pub target_custom_portable_label: String,
    pub target_custom_portable_app_path_label: String,
    pub target_custom_portable_path_label: String,
    pub target_custom_portable_version_label: String,
    pub target_custom_portable_architecture_label: String,
    pub target_custom_portable_writable_label: String,
    pub target_custom_portable_note: String,
    pub packages_heading: String,
    pub packages_list_label: String,
    pub package_details_label: String,
    pub packages_osara_keymap_heading: String,
    pub packages_osara_keymap_replace_label: String,
    pub packages_osara_keymap_unavailable_note: String,
    pub packages_osara_keymap_preserve_note: String,
    pub packages_osara_keymap_replace_note: String,
    pub package_details_handling_prefix: String,
    pub package_handling_automatic: String,
    pub package_handling_unattended: String,
    pub package_handling_planned: String,
    pub package_handling_manual: String,
    pub package_handling_unavailable: String,
    pub review_heading: String,
    pub review_target_prefix: String,
    pub review_cache_prefix: String,
    pub review_resource_heading: String,
    pub review_resource_create_directory_prefix: String,
    pub review_resource_create_file_prefix: String,
    pub review_resource_no_changes: String,
    pub review_backup_heading: String,
    pub review_backup_file_prefix: String,
    pub review_backup_no_changes: String,
    pub review_admin_heading: String,
    pub review_admin_no_prompts: String,
    pub review_admin_app_prefix: String,
    pub review_admin_resource_prefix: String,
    pub review_package_heading: String,
    pub review_osara_keymap_heading: String,
    pub review_osara_keymap_preserve: String,
    pub review_osara_keymap_replace: String,
    pub review_notes_heading: String,
    pub review_preflight_prefix: String,
    pub review_manual_heading: String,
    pub review_no_target: String,
    pub review_no_package: String,
    pub progress_heading: String,
    pub progress_status: String,
    pub progress_status_running: String,
    pub progress_details_label: String,
    pub progress_details_idle: String,
    pub progress_details_starting: String,
    pub progress_details_cache_prefix: String,
    pub done_heading: String,
    pub done_status: String,
    pub done_status_success: String,
    pub done_status_error: String,
    pub done_status_no_packages: String,
    pub done_launch_reaper_label: String,
    pub done_open_resource_label: String,
    pub done_rescan_label: String,
    pub done_save_report_label: String,
    pub done_no_reaper_app: String,
    pub done_no_report: String,
    pub done_report_saved_prefix: String,
    pub done_report_save_error_prefix: String,
    pub done_launch_reaper_error_prefix: String,
    pub done_open_resource_error_prefix: String,
    pub done_rescan_error_prefix: String,
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
    pub app_path: Option<PathBuf>,
    pub planned_app_path: PathBuf,
    pub path: PathBuf,
    pub version: Option<Version>,
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
    pub details: String,
    pub installed_version: String,
    pub available_version: String,
    pub action: PlanActionKind,
    pub action_label: String,
    pub reason: String,
    pub handling_summary: String,
    pub manual_attention_expected: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OsaraKeymapChoice {
    PreserveCurrent,
    ReplaceCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInstallOptions {
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub osara_keymap_choice: OsaraKeymapChoice,
    pub cache_dir: Option<PathBuf>,
}

impl Default for WizardInstallOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            allow_reaper_running: false,
            stage_unsupported: true,
            osara_keymap_choice: OsaraKeymapChoice::PreserveCurrent,
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
    pub target_app_path: Option<PathBuf>,
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub osara_keymap_choice: OsaraKeymapChoice,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInstallSummary {
    pub status_line: String,
    pub detail_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardPackagePlan {
    pub package_rows: Vec<PackageRow>,
    pub notes: Vec<String>,
    pub can_install: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardReviewPreview {
    pub lines: Vec<String>,
    pub can_install: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WizardOutcomeStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WizardOutcomeReport {
    pub status: WizardOutcomeStatus,
    pub resource_path: PathBuf,
    pub target_app_path: Option<PathBuf>,
    pub package_ids: Vec<String>,
    pub platform: Platform,
    pub architecture: Architecture,
    pub portable: bool,
    pub dry_run: bool,
    pub allow_reaper_running: bool,
    pub stage_unsupported: bool,
    pub cache_dir: PathBuf,
    pub osara_keymap_choice: OsaraKeymapChoice,
    pub status_line: String,
    pub detail_lines: Vec<String>,
    pub error_message: Option<String>,
    pub setup_report: Option<SetupReport>,
}

pub fn load_wizard_model(options: UiBootstrapOptions) -> Result<WizardModel> {
    let platform = Platform::current().ok_or(RaisError::UnsupportedPlatform)?;
    let architecture = Architecture::current();
    let localizer = localizer_from_options(&options)?;
    let discovered_installations = discover_installations(&DiscoveryOptions {
        include_standard: true,
        portable_roots: options.portable_roots.clone(),
    })?;
    let installations = selectable_installations(platform, discovered_installations);
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
    let desired = wizard_package_ids(platform);
    let plan = build_install_plan(target, &detections, &desired, &available);

    Ok(model_from_plan_with_options(
        &localizer,
        options,
        platform,
        architecture,
        installations,
        selected_target_index,
        available,
        plan,
    ))
}

fn selectable_installations(
    platform: Platform,
    mut installations: Vec<Installation>,
) -> Vec<Installation> {
    if !installations
        .iter()
        .any(|installation| installation.kind == InstallationKind::Standard)
    {
        if let Some(standard) = default_standard_installation(platform) {
            installations.push(standard);
        }
    }
    installations
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
    model_from_plan_with_options(
        localizer,
        UiBootstrapOptions::default(),
        platform,
        architecture,
        installations,
        selected_target_index,
        Vec::new(),
        plan,
    )
}

fn model_from_plan_with_options(
    localizer: &Localizer,
    bootstrap_options: UiBootstrapOptions,
    platform: Platform,
    architecture: Architecture,
    installations: Vec<Installation>,
    selected_target_index: Option<usize>,
    available_packages: Vec<AvailablePackage>,
    plan: InstallPlan,
) -> WizardModel {
    let package_specs = builtin_package_specs(platform);
    let text = wizard_text(localizer);
    let target_rows = target_rows(localizer, &installations, selected_target_index);
    let package_rows = package_rows(
        localizer,
        &text,
        platform,
        architecture,
        &package_specs,
        &plan.actions,
    );
    let review_lines = review_lines(localizer, &target_rows, &package_rows, &plan.notes);
    let can_install = package_rows
        .iter()
        .any(|row| matches!(row.action, PlanActionKind::Install | PlanActionKind::Update));

    WizardModel {
        window_title: localizer.text("app-title").value,
        platform,
        architecture,
        bootstrap_options,
        current_step: WizardStep::Target,
        steps: wizard_steps(localizer),
        selected_target_index,
        target_rows,
        package_rows,
        available_packages,
        review_lines,
        notes: plan.notes,
        text,
        controls: WizardControls {
            back_label: localized_wx_mnemonic_label(
                localizer,
                "wizard-button-back",
                "wizard-button-back-mnemonic",
            ),
            next_label: localized_wx_mnemonic_label(
                localizer,
                "wizard-button-next",
                "wizard-button-next-mnemonic",
            ),
            install_label: localized_wx_mnemonic_label(
                localizer,
                "wizard-button-install",
                "wizard-button-install-mnemonic",
            ),
            close_label: localized_wx_mnemonic_label(
                localizer,
                "wizard-button-close",
                "wizard-button-close-mnemonic",
            ),
            can_go_back: false,
            can_go_next: selected_target_index.is_some(),
            can_install,
        },
    }
}

fn wizard_text(localizer: &Localizer) -> WizardText {
    WizardText {
        common_yes: localizer.text("common-yes").value,
        common_no: localizer.text("common-no").value,
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
        target_custom_portable_app_path_label: localizer
            .text("wizard-target-custom-portable-app-path-label")
            .value,
        target_custom_portable_path_label: localizer
            .text("wizard-target-custom-portable-path-label")
            .value,
        target_custom_portable_version_label: localizer
            .text("wizard-target-custom-portable-version-label")
            .value,
        target_custom_portable_architecture_label: localizer
            .text("wizard-target-custom-portable-architecture-label")
            .value,
        target_custom_portable_writable_label: localizer
            .text("wizard-target-custom-portable-writable-label")
            .value,
        target_custom_portable_note: localizer.text("wizard-target-custom-portable-note").value,
        packages_heading: localizer.text("wizard-packages-heading").value,
        packages_list_label: localizer.text("wizard-packages-list-label").value,
        package_details_label: localizer.text("wizard-package-details-label").value,
        packages_osara_keymap_heading: localizer.text("wizard-packages-osara-keymap-heading").value,
        packages_osara_keymap_replace_label: localizer
            .text("wizard-packages-osara-keymap-replace-label")
            .value,
        packages_osara_keymap_unavailable_note: localizer
            .text("wizard-packages-osara-keymap-unavailable-note")
            .value,
        packages_osara_keymap_preserve_note: localizer
            .text("wizard-packages-osara-keymap-preserve-note")
            .value,
        packages_osara_keymap_replace_note: localizer
            .text("wizard-packages-osara-keymap-replace-note")
            .value,
        package_details_handling_prefix: localizer
            .text("wizard-package-details-handling-prefix")
            .value,
        package_handling_automatic: localizer.text("wizard-package-handling-automatic").value,
        package_handling_unattended: localizer.text("wizard-package-handling-unattended").value,
        package_handling_planned: localizer.text("wizard-package-handling-planned").value,
        package_handling_manual: localizer.text("wizard-package-handling-manual").value,
        package_handling_unavailable: localizer.text("wizard-package-handling-unavailable").value,
        review_heading: localizer.text("wizard-review-heading").value,
        review_target_prefix: localizer.text("wizard-review-target-prefix").value,
        review_cache_prefix: localizer.text("wizard-review-cache-prefix").value,
        review_resource_heading: localizer.text("wizard-review-resource-heading").value,
        review_resource_create_directory_prefix: localizer
            .text("wizard-review-resource-create-directory-prefix")
            .value,
        review_resource_create_file_prefix: localizer
            .text("wizard-review-resource-create-file-prefix")
            .value,
        review_resource_no_changes: localizer.text("wizard-review-resource-no-changes").value,
        review_backup_heading: localizer.text("wizard-review-backup-heading").value,
        review_backup_file_prefix: localizer.text("wizard-review-backup-file-prefix").value,
        review_backup_no_changes: localizer.text("wizard-review-backup-no-changes").value,
        review_admin_heading: localizer.text("wizard-review-admin-heading").value,
        review_admin_no_prompts: localizer.text("wizard-review-admin-no-prompts").value,
        review_admin_app_prefix: localizer.text("wizard-review-admin-app-prefix").value,
        review_admin_resource_prefix: localizer.text("wizard-review-admin-resource-prefix").value,
        review_package_heading: localizer.text("wizard-review-package-heading").value,
        review_osara_keymap_heading: localizer.text("wizard-review-osara-keymap-heading").value,
        review_osara_keymap_preserve: localizer.text("wizard-review-osara-keymap-preserve").value,
        review_osara_keymap_replace: localizer.text("wizard-review-osara-keymap-replace").value,
        review_notes_heading: localizer.text("wizard-review-notes-heading").value,
        review_preflight_prefix: localizer.text("wizard-review-preflight-prefix").value,
        review_manual_heading: localizer.text("wizard-review-manual-heading").value,
        review_no_target: localizer.text("wizard-review-no-target").value,
        review_no_package: localizer.text("wizard-review-no-package").value,
        progress_heading: localizer.text("wizard-progress-heading").value,
        progress_status: localizer.text("wizard-progress-status-idle").value,
        done_heading: localizer.text("wizard-done-heading").value,
        done_status: localizer.text("wizard-done-status-idle").value,
        progress_status_running: localizer.text("wizard-progress-status-running").value,
        progress_details_label: localizer.text("wizard-progress-details-label").value,
        progress_details_idle: localizer.text("wizard-progress-details-idle").value,
        progress_details_starting: localizer.text("wizard-progress-details-starting").value,
        progress_details_cache_prefix: localizer.text("wizard-progress-details-cache-prefix").value,
        done_status_success: localizer.text("wizard-done-status-success").value,
        done_status_error: localizer.text("wizard-done-status-error").value,
        done_status_no_packages: localizer.text("wizard-done-status-no-packages").value,
        done_launch_reaper_label: localized_wx_mnemonic_label(
            localizer,
            "wizard-done-launch-reaper",
            "wizard-done-launch-reaper-mnemonic",
        ),
        done_open_resource_label: localized_wx_mnemonic_label(
            localizer,
            "wizard-done-open-resource",
            "wizard-done-open-resource-mnemonic",
        ),
        done_rescan_label: localized_wx_mnemonic_label(
            localizer,
            "wizard-done-rescan",
            "wizard-done-rescan-mnemonic",
        ),
        done_save_report_label: localized_wx_mnemonic_label(
            localizer,
            "wizard-done-save-report",
            "wizard-done-save-report-mnemonic",
        ),
        done_no_reaper_app: localizer.text("wizard-done-no-reaper-app").value,
        done_no_report: localizer.text("wizard-done-no-report").value,
        done_report_saved_prefix: localizer.text("wizard-done-report-saved-prefix").value,
        done_report_save_error_prefix: localizer.text("wizard-done-report-save-error-prefix").value,
        done_launch_reaper_error_prefix: localizer
            .text("wizard-done-launch-reaper-error-prefix")
            .value,
        done_open_resource_error_prefix: localizer
            .text("wizard-done-open-resource-error-prefix")
            .value,
        done_rescan_error_prefix: localizer.text("wizard-done-rescan-error-prefix").value,
    }
}

fn localized_wx_mnemonic_label(localizer: &Localizer, label_id: &str, mnemonic_id: &str) -> String {
    wx_mnemonic_label(
        &localizer.text(label_id).value,
        &localizer.text(mnemonic_id).value,
    )
}

fn wx_mnemonic_label(label: &str, mnemonic: &str) -> String {
    let Some(key) = mnemonic.trim().chars().next() else {
        return escape_wx_label(label);
    };

    let mut output = String::new();
    let mut inserted = false;
    for label_char in label.chars() {
        if !inserted && mnemonic_matches(label_char, key) {
            output.push('&');
            inserted = true;
        }
        push_escaped_wx_label_char(&mut output, label_char);
    }

    if !inserted {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push('(');
        output.push('&');
        push_escaped_wx_label_char(&mut output, key);
        output.push(')');
    }

    output
}

fn escape_wx_label(label: &str) -> String {
    let mut output = String::new();
    for label_char in label.chars() {
        push_escaped_wx_label_char(&mut output, label_char);
    }
    output
}

fn push_escaped_wx_label_char(output: &mut String, label_char: char) {
    if label_char == '&' {
        output.push_str("&&");
    } else {
        output.push(label_char);
    }
}

fn mnemonic_matches(label_char: char, mnemonic: char) -> bool {
    label_char == mnemonic
        || label_char.eq_ignore_ascii_case(&mnemonic)
        || label_char.to_lowercase().to_string() == mnemonic.to_lowercase().to_string()
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
            target_row(
                localizer,
                installation,
                Some(index) == selected_target_index,
            )
        })
        .collect()
}

fn target_row(localizer: &Localizer, installation: &Installation, selected: bool) -> TargetRow {
    let kind = format!("{:?}", installation.kind);
    let version = installation
        .version
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| localizer.text("detect-version-unknown").value);
    let architecture = architecture_text(localizer, installation.architecture);
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
                    ("app_path", &installation.app_path.display().to_string()),
                    ("version", version.as_str()),
                    ("architecture", architecture.as_str()),
                    ("path", &installation.resource_path.display().to_string()),
                    (
                        "writable",
                        yes_no(localizer, installation.writable).as_str(),
                    ),
                    ("confidence", &format!("{:?}", installation.confidence)),
                ],
            )
            .value,
        app_path: installation
            .app_path
            .exists()
            .then(|| installation.app_path.clone()),
        planned_app_path: installation.app_path.clone(),
        path: installation.resource_path.clone(),
        version: installation.version.clone(),
        portable: installation.kind == InstallationKind::Portable,
        selected,
        writable: installation.writable,
    }
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
    install_request_from_target_and_rows(
        model,
        target,
        &model.package_rows,
        selected_package_indices,
        options,
    )
}

pub fn install_request_from_target_and_rows(
    model: &WizardModel,
    target: &TargetRow,
    package_rows: &[PackageRow],
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

    let package_ids = package_ids_for_rows(package_rows, selected_package_indices);
    if package_ids.is_empty() {
        return Err(RaisError::PreflightFailed {
            message: "No package was selected for installation or update.".to_string(),
        });
    }
    let osara_selected = package_ids.iter().any(|id| id == PACKAGE_OSARA);

    Ok(WizardInstallRequest {
        resource_path: target.path.clone(),
        package_ids,
        platform: model.platform,
        architecture: model.architecture,
        portable: target.portable,
        target_app_path: Some(target.planned_app_path.clone()),
        dry_run: options.dry_run,
        allow_reaper_running: options.allow_reaper_running,
        stage_unsupported: options.stage_unsupported,
        osara_keymap_choice: if osara_selected {
            options.osara_keymap_choice
        } else {
            OsaraKeymapChoice::PreserveCurrent
        },
        cache_dir: options.cache_dir.unwrap_or_else(default_cache_dir),
    })
}

pub fn package_ids_for_indices(model: &WizardModel, indices: &[usize]) -> Vec<String> {
    package_ids_for_rows(&model.package_rows, indices)
}

pub fn package_ids_for_rows(package_rows: &[PackageRow], indices: &[usize]) -> Vec<String> {
    let mut package_ids = Vec::new();
    for index in indices {
        let Some(row) = package_rows.get(*index) else {
            continue;
        };
        if !package_ids.contains(&row.package_id) {
            package_ids.push(row.package_id.clone());
        }
    }
    package_ids
}

pub fn osara_selected_for_rows(package_rows: &[PackageRow], indices: &[usize]) -> bool {
    indices
        .iter()
        .filter_map(|index| package_rows.get(*index))
        .any(|row| row.package_id == PACKAGE_OSARA)
}

pub fn osara_keymap_note(
    model: &WizardModel,
    osara_selected: bool,
    choice: OsaraKeymapChoice,
) -> String {
    if !osara_selected {
        return model.text.packages_osara_keymap_unavailable_note.clone();
    }

    match choice {
        OsaraKeymapChoice::PreserveCurrent => {
            model.text.packages_osara_keymap_preserve_note.clone()
        }
        OsaraKeymapChoice::ReplaceCurrent => model.text.packages_osara_keymap_replace_note.clone(),
    }
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
    review_lines_for_package_rows(
        model,
        target,
        selected_package_indices,
        &model.package_rows,
        &model.notes,
    )
}

pub fn review_lines_for_package_rows(
    model: &WizardModel,
    target: Option<&TargetRow>,
    selected_package_indices: &[usize],
    package_rows: &[PackageRow],
    notes: &[String],
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

    let package_ids = package_ids_for_rows(package_rows, selected_package_indices);
    if package_ids.is_empty() {
        lines.push(model.text.review_no_package.clone());
    } else {
        for package_id in package_ids {
            if let Some(package) = package_rows
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

    lines.extend(notes.iter().cloned());
    lines
}

pub fn build_review_preview_for_package_rows(
    model: &WizardModel,
    target: Option<&TargetRow>,
    selected_package_indices: &[usize],
    package_rows: &[PackageRow],
    notes: &[String],
    osara_keymap_choice: OsaraKeymapChoice,
) -> WizardReviewPreview {
    let Some(target) = target else {
        return WizardReviewPreview {
            lines: vec![model.text.review_no_target.clone()],
            can_install: false,
        };
    };

    let mut lines = vec![
        format!(
            "{}: {}",
            model.text.review_target_prefix,
            target.path.display()
        ),
        format!(
            "{}: {}",
            model.text.review_cache_prefix,
            default_cache_dir().display()
        ),
        String::new(),
        model.text.review_resource_heading.clone(),
    ];

    let mut can_install = !selected_package_indices.is_empty();
    match initialize_resource_path(
        &target.path,
        &ResourceInitOptions {
            dry_run: true,
            portable: target.portable,
            allow_reaper_running: false,
            target_app_path: Some(target.planned_app_path.clone()),
        },
    ) {
        Ok(report) => {
            let resource_lines = review_resource_lines(model, &report);
            if resource_lines.is_empty() {
                lines.push(model.text.review_resource_no_changes.clone());
            } else {
                lines.extend(resource_lines);
            }
        }
        Err(error) => {
            can_install = false;
            lines.push(format!("{}: {}", model.text.review_preflight_prefix, error));
        }
    }

    lines.push(String::new());
    lines.push(model.text.review_backup_heading.clone());
    lines.extend(review_backup_lines(
        model,
        target,
        selected_package_indices,
        package_rows,
        osara_keymap_choice,
    ));

    lines.push(String::new());
    lines.push(model.text.review_admin_heading.clone());
    lines.extend(review_admin_lines(
        model,
        target,
        selected_package_indices,
        package_rows,
    ));

    lines.push(String::new());
    lines.push(model.text.review_package_heading.clone());
    if selected_package_indices.is_empty() {
        lines.push(model.text.review_no_package.clone());
    } else {
        for index in selected_package_indices {
            if let Some(package) = package_rows.get(*index) {
                lines.push(package.summary.clone());
            }
        }
    }

    if osara_selected_for_rows(package_rows, selected_package_indices) {
        lines.push(String::new());
        lines.push(model.text.review_osara_keymap_heading.clone());
        lines.push(match osara_keymap_choice {
            OsaraKeymapChoice::PreserveCurrent => model.text.review_osara_keymap_preserve.clone(),
            OsaraKeymapChoice::ReplaceCurrent => model.text.review_osara_keymap_replace.clone(),
        });
    }

    let manual_items = selected_package_indices
        .iter()
        .filter_map(|index| package_rows.get(*index))
        .filter(|package| package_requires_manual_attention(model, package, osara_keymap_choice))
        .collect::<Vec<_>>();
    if !manual_items.is_empty() {
        lines.push(String::new());
        lines.push(model.text.review_manual_heading.clone());
        for package in manual_items {
            lines.push(format!(
                "{}: {}",
                package.display_name,
                manual_attention_handling_summary(model, package, osara_keymap_choice)
            ));
            lines.extend(preview_manual_instruction_lines(
                model,
                target,
                package,
                osara_keymap_choice,
            ));
        }
    }

    if !notes.is_empty() {
        lines.push(String::new());
        lines.push(model.text.review_notes_heading.clone());
        lines.extend(notes.iter().cloned());
    }

    WizardReviewPreview { lines, can_install }
}

pub fn package_requires_manual_attention(
    model: &WizardModel,
    package: &PackageRow,
    osara_keymap_choice: OsaraKeymapChoice,
) -> bool {
    matches!(
        package.action,
        PlanActionKind::Install | PlanActionKind::Update
    ) && (package.manual_attention_expected
        || (package.package_id == PACKAGE_OSARA
            && matches!(model.platform, Platform::Windows)
            && matches!(osara_keymap_choice, OsaraKeymapChoice::ReplaceCurrent)))
}

pub fn manual_attention_handling_summary(
    model: &WizardModel,
    package: &PackageRow,
    osara_keymap_choice: OsaraKeymapChoice,
) -> String {
    if package.package_id == PACKAGE_OSARA
        && matches!(model.platform, Platform::Windows)
        && matches!(osara_keymap_choice, OsaraKeymapChoice::ReplaceCurrent)
    {
        model.text.package_handling_planned.clone()
    } else {
        package.handling_summary.clone()
    }
}

fn review_resource_lines(model: &WizardModel, report: &ResourceInitReport) -> Vec<String> {
    let mut lines = Vec::new();
    for action in &report.actions {
        if action.action != ResourceInitActionKind::WouldCreate {
            continue;
        }

        let prefix = match action.kind {
            ResourceInitItemKind::Directory => &model.text.review_resource_create_directory_prefix,
            ResourceInitItemKind::File => &model.text.review_resource_create_file_prefix,
        };
        lines.push(format!("{prefix}: {}", action.path.display()));
    }
    lines
}

fn review_backup_lines(
    model: &WizardModel,
    target: &TargetRow,
    selected_package_indices: &[usize],
    package_rows: &[PackageRow],
    osara_keymap_choice: OsaraKeymapChoice,
) -> Vec<String> {
    let backup_paths = predicted_backup_paths_for_package_rows(
        model,
        target,
        selected_package_indices,
        package_rows,
        osara_keymap_choice,
    );
    if backup_paths.is_empty() {
        vec![model.text.review_backup_no_changes.clone()]
    } else {
        backup_paths
            .into_iter()
            .map(|path| {
                format!(
                    "{}: {}",
                    model.text.review_backup_file_prefix,
                    path.display()
                )
            })
            .collect()
    }
}

fn review_admin_lines(
    model: &WizardModel,
    target: &TargetRow,
    selected_package_indices: &[usize],
    package_rows: &[PackageRow],
) -> Vec<String> {
    let mut lines = Vec::new();
    let reaper_selected = selected_package_indices
        .iter()
        .filter_map(|index| package_rows.get(*index))
        .any(|package| {
            package.package_id == rais_core::package::PACKAGE_REAPER
                && matches!(
                    package.action,
                    PlanActionKind::Install | PlanActionKind::Update
                )
        });

    if path_likely_requires_admin_prompt(model.platform, &target.path) {
        lines.push(format!(
            "{}: {}",
            model.text.review_admin_resource_prefix,
            target.path.display()
        ));
    }

    if reaper_selected
        && !path_is_same_or_nested(&target.planned_app_path, &target.path)
        && path_likely_requires_admin_prompt(model.platform, &target.planned_app_path)
    {
        lines.push(format!(
            "{}: {}",
            model.text.review_admin_app_prefix,
            target.planned_app_path.display()
        ));
    }

    if lines.is_empty() {
        vec![model.text.review_admin_no_prompts.clone()]
    } else {
        lines
    }
}

pub fn preview_manual_instruction_lines(
    model: &WizardModel,
    target: &TargetRow,
    package: &PackageRow,
    osara_keymap_choice: OsaraKeymapChoice,
) -> Vec<String> {
    let Ok(kind) = expected_artifact_kind(&package.package_id, model.platform, model.architecture)
    else {
        return Vec::new();
    };
    let instruction = preview_manual_instruction(
        &package.package_id,
        kind,
        &target.path,
        Some(&target.planned_app_path),
        matches!(osara_keymap_choice, OsaraKeymapChoice::ReplaceCurrent),
    );
    let mut lines = instruction
        .steps
        .into_iter()
        .map(|step| format!("  {step}"))
        .collect::<Vec<_>>();
    lines.extend(
        instruction
            .notes
            .into_iter()
            .map(|note| format!("  Note: {note}")),
    );
    lines
}

pub fn wizard_package_plan_for_target(
    model: &WizardModel,
    target: Option<&TargetRow>,
) -> Result<WizardPackagePlan> {
    let localizer = localizer_from_options(&model.bootstrap_options)?;
    let detections = match target {
        Some(target) => detect_components(&target.path, model.platform)?,
        None => Vec::new(),
    };
    let desired = wizard_package_ids(model.platform);
    let plan = build_install_plan(
        target.map(|target| installation_from_target_row(model, target)),
        &detections,
        &desired,
        &model.available_packages,
    );
    let package_specs = builtin_package_specs(model.platform);
    let package_rows = package_rows(
        &localizer,
        &model.text,
        model.platform,
        model.architecture,
        &package_specs,
        &plan.actions,
    );
    let can_install = package_rows
        .iter()
        .any(|row| matches!(row.action, PlanActionKind::Install | PlanActionKind::Update));

    Ok(WizardPackagePlan {
        package_rows,
        notes: plan.notes,
        can_install,
    })
}

fn wizard_package_ids(platform: Platform) -> Vec<String> {
    builtin_package_specs(platform)
        .into_iter()
        .map(|spec| spec.id)
        .collect()
}

pub fn custom_portable_target_row(model: &WizardModel, path: PathBuf, selected: bool) -> TargetRow {
    let writable = is_probably_writable(&path);
    let writable_text = if writable {
        model.text.common_yes.clone()
    } else {
        model.text.common_no.clone()
    };
    let app_path = portable_reaper_app_path(model.platform, &path);
    let version = app_path
        .as_ref()
        .and_then(|path| file_version(path).ok().flatten());
    let version_text = version
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| unknown_version_text(model));
    let architecture_text = if app_path.is_some() {
        match model.architecture {
            Architecture::X86 => "x86".to_string(),
            Architecture::X64 => "x64".to_string(),
            Architecture::Arm64 => "arm64".to_string(),
            Architecture::Arm64Ec => "arm64ec".to_string(),
            Architecture::Universal => "universal".to_string(),
            Architecture::Unknown => unknown_architecture_text(model),
        }
    } else {
        unknown_architecture_text(model)
    };
    TargetRow {
        label: format!(
            "{}: {}",
            model.text.target_custom_portable_label,
            path.display()
        ),
        details: format!(
            "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}",
            model.text.target_custom_portable_app_path_label,
            app_path
                .as_ref()
                .unwrap_or(&default_portable_reaper_app_path(model.platform, &path))
                .display(),
            model.text.target_custom_portable_path_label,
            path.display(),
            model.text.target_custom_portable_version_label,
            version_text,
            model.text.target_custom_portable_architecture_label,
            architecture_text,
            model.text.target_custom_portable_writable_label,
            writable_text,
            model.text.target_custom_portable_note
        ),
        app_path: app_path.clone(),
        planned_app_path: app_path
            .unwrap_or_else(|| default_portable_reaper_app_path(model.platform, &path)),
        path,
        version,
        portable: true,
        selected,
        writable,
    }
}

pub fn refreshed_target_row(model: &WizardModel, target: &TargetRow) -> TargetRow {
    if target.portable {
        return custom_portable_target_row(model, target.path.clone(), target.selected);
    }

    let installation = Installation {
        kind: InstallationKind::Standard,
        platform: model.platform,
        app_path: target.planned_app_path.clone(),
        resource_path: target.path.clone(),
        version: file_version(&target.planned_app_path).ok().flatten(),
        architecture: Some(model.architecture),
        writable: is_probably_writable(&target.path),
        confidence: Confidence::Medium,
        evidence: Vec::new(),
    };

    match localizer_from_options(&model.bootstrap_options) {
        Ok(localizer) => target_row(&localizer, &installation, target.selected),
        Err(_) => TargetRow {
            label: target.label.clone(),
            details: target.details.clone(),
            app_path: installation
                .app_path
                .exists()
                .then(|| installation.app_path.clone()),
            planned_app_path: installation.app_path.clone(),
            path: installation.resource_path.clone(),
            version: installation.version.clone(),
            portable: false,
            selected: target.selected,
            writable: installation.writable,
        },
    }
}

fn installation_from_target_row(model: &WizardModel, target: &TargetRow) -> Installation {
    Installation {
        kind: if target.portable {
            InstallationKind::Portable
        } else {
            InstallationKind::Standard
        },
        platform: model.platform,
        app_path: target.planned_app_path.clone(),
        resource_path: target.path.clone(),
        version: target.version.clone(),
        architecture: Some(model.architecture),
        writable: target.writable,
        confidence: Confidence::Medium,
        evidence: Vec::new(),
    }
}

fn portable_reaper_app_path(platform: Platform, resource_path: &Path) -> Option<PathBuf> {
    match platform {
        Platform::Windows => {
            let app_path = resource_path.join("reaper.exe");
            app_path.is_file().then_some(app_path)
        }
        Platform::MacOs => fs::read_dir(resource_path)
            .ok()?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.to_ascii_lowercase().contains("reaper"))
            }),
    }
}

fn default_portable_reaper_app_path(platform: Platform, resource_path: &Path) -> PathBuf {
    match platform {
        Platform::Windows => resource_path.join("reaper.exe"),
        Platform::MacOs => resource_path.join("REAPER.app"),
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
            replace_osara_keymap: matches!(
                request.osara_keymap_choice,
                OsaraKeymapChoice::ReplaceCurrent
            ),
            target_app_path: request.target_app_path.clone(),
        },
    )
}

pub fn wizard_outcome_report_from_success(
    model: &WizardModel,
    request: &WizardInstallRequest,
    report: &SetupReport,
) -> WizardOutcomeReport {
    let summary = summarize_setup_report(model, report);
    WizardOutcomeReport {
        status: WizardOutcomeStatus::Success,
        resource_path: report.resource_path.clone(),
        target_app_path: request.target_app_path.clone(),
        package_ids: request.package_ids.clone(),
        platform: request.platform,
        architecture: request.architecture,
        portable: request.portable,
        dry_run: request.dry_run,
        allow_reaper_running: request.allow_reaper_running,
        stage_unsupported: request.stage_unsupported,
        cache_dir: request.cache_dir.clone(),
        osara_keymap_choice: request.osara_keymap_choice,
        status_line: summary.status_line,
        detail_lines: summary.detail_lines,
        error_message: None,
        setup_report: Some(report.clone()),
    }
}

pub fn summarize_wizard_error(
    model: &WizardModel,
    request: &WizardInstallRequest,
    error: &RaisError,
) -> WizardInstallSummary {
    let localizer = localizer_from_options(&model.bootstrap_options).ok();
    let selected_packages = if request.package_ids.is_empty() {
        model.text.review_no_package.clone()
    } else {
        request
            .package_ids
            .iter()
            .map(|package_id| package_display_name(model, package_id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut detail_lines = vec![
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-target",
            &[("path", request.resource_path.display().to_string())],
            format!("Target: {}", request.resource_path.display()),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-portable",
            &[(
                "value",
                if request.portable {
                    model.text.common_yes.clone()
                } else {
                    model.text.common_no.clone()
                },
            )],
            format!(
                "Portable target: {}",
                if request.portable {
                    &model.text.common_yes
                } else {
                    &model.text.common_no
                }
            ),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-dry-run",
            &[(
                "value",
                if request.dry_run {
                    model.text.common_yes.clone()
                } else {
                    model.text.common_no.clone()
                },
            )],
            format!(
                "Dry run: {}",
                if request.dry_run {
                    &model.text.common_yes
                } else {
                    &model.text.common_no
                }
            ),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-packages-selected",
            &[("packages", selected_packages.clone())],
            format!("Packages selected: {selected_packages}"),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-cache",
            &[("path", request.cache_dir.display().to_string())],
            format!("Cache: {}", request.cache_dir.display()),
        ),
    ];

    if let Some(target_app_path) = &request.target_app_path {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-planned-app",
            &[("path", target_app_path.display().to_string())],
            format!("Planned app path: {}", target_app_path.display()),
        ));
    }

    if request
        .package_ids
        .iter()
        .any(|package_id| package_id == PACKAGE_OSARA)
    {
        detail_lines.push(model.text.review_osara_keymap_heading.clone());
        detail_lines.push(match request.osara_keymap_choice {
            OsaraKeymapChoice::PreserveCurrent => model.text.review_osara_keymap_preserve.clone(),
            OsaraKeymapChoice::ReplaceCurrent => model.text.review_osara_keymap_replace.clone(),
        });
    }

    detail_lines.push(format_localized_message(
        localizer.as_ref(),
        "wizard-summary-error",
        &[("message", error.to_string())],
        format!("Error: {error}"),
    ));

    WizardInstallSummary {
        status_line: model.text.done_status_error.clone(),
        detail_lines,
    }
}

pub fn wizard_outcome_report_from_error(
    model: &WizardModel,
    request: &WizardInstallRequest,
    error: &RaisError,
) -> WizardOutcomeReport {
    let summary = summarize_wizard_error(model, request, error);
    WizardOutcomeReport {
        status: WizardOutcomeStatus::Error,
        resource_path: request.resource_path.clone(),
        target_app_path: request.target_app_path.clone(),
        package_ids: request.package_ids.clone(),
        platform: request.platform,
        architecture: request.architecture,
        portable: request.portable,
        dry_run: request.dry_run,
        allow_reaper_running: request.allow_reaper_running,
        stage_unsupported: request.stage_unsupported,
        cache_dir: request.cache_dir.clone(),
        osara_keymap_choice: request.osara_keymap_choice,
        status_line: summary.status_line,
        detail_lines: summary.detail_lines,
        error_message: Some(error.to_string()),
        setup_report: None,
    }
}

pub fn save_wizard_outcome_report(report: &WizardOutcomeReport) -> Result<PathBuf> {
    let json_path = default_report_path(&report.resource_path, "setup");
    let saved = save_json_and_text_reports(&json_path, report)?;
    Ok(saved.text_path)
}

pub fn save_wizard_setup_report(report: &SetupReport) -> Result<PathBuf> {
    let json_path = default_report_path(&report.resource_path, "setup");
    let saved = save_json_and_text_reports(&json_path, report)?;
    Ok(saved.text_path)
}

pub fn summarize_setup_report(model: &WizardModel, report: &SetupReport) -> WizardInstallSummary {
    let localizer = localizer_from_options(&model.bootstrap_options).ok();
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
                PackageOperationStatus::DeferredUnattended
                    | PackageOperationStatus::SkippedManualReview
            )
        })
        .count();

    let mut detail_lines = vec![
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-target",
            &[("path", report.resource_path.display().to_string())],
            format!("Target: {}", report.resource_path.display()),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-dry-run",
            &[(
                "value",
                if report.dry_run {
                    model.text.common_yes.clone()
                } else {
                    model.text.common_no.clone()
                },
            )],
            format!(
                "Dry run: {}",
                if report.dry_run {
                    &model.text.common_yes
                } else {
                    &model.text.common_no
                }
            ),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-resource-items-created",
            &[("count", created_resources.to_string())],
            format!("Resource items created: {created_resources}"),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-packages-installed-or-checked",
            &[("count", installed_or_checked.to_string())],
            format!("Packages installed or checked: {installed_or_checked}"),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-packages-current",
            &[("count", skipped_current.to_string())],
            format!("Packages already current: {skipped_current}"),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-packages-manual",
            &[("count", manual_items.to_string())],
            format!("Packages requiring manual attention: {manual_items}"),
        ),
    ];

    if let Some(install_report) = &report.package_operation.install_report {
        let backup_paths = install_report
            .actions
            .iter()
            .filter_map(|action| action.backup_path.as_ref())
            .collect::<Vec<_>>();
        if !backup_paths.is_empty()
            || install_report.receipt_backup_path.is_some()
            || install_report.backup_manifest_path.is_some()
        {
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-backup-files-created",
                &[("count", backup_paths.len().to_string())],
                format!("Backup files created: {}", backup_paths.len()),
            ));
            for path in backup_paths {
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-backup-file",
                    &[("path", path.display().to_string())],
                    format!("Backup file: {}", path.display()),
                ));
            }
            if let Some(path) = &install_report.receipt_backup_path {
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-receipt-backup",
                    &[("path", path.display().to_string())],
                    format!("Receipt backup: {}", path.display()),
                ));
            }
            if let Some(path) = &install_report.backup_manifest_path {
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-backup-manifest",
                    &[("path", path.display().to_string())],
                    format!("Backup manifest: {}", path.display()),
                ));
            }
        }
    }

    for item in &report.package_operation.items {
        let package_name = package_display_name(model, &item.package_id);
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-package-message",
            &[
                ("package", package_name.clone()),
                ("message", item.message.clone()),
            ],
            format!("{package_name}: {}", item.message),
        ));
        if let Some(plan) = &item.planned_execution {
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-planned-execution-title",
                &[],
                "Planned unattended execution:".to_string(),
            ));
            let runner = planned_execution_runner_label(localizer.as_ref(), plan.kind);
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-planned-execution-runner",
                &[("runner", runner.clone())],
                format!("  Runner: {runner}"),
            ));
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-planned-execution-artifact",
                &[("artifact", plan.artifact_location.clone())],
                format!("  Artifact: {}", plan.artifact_location),
            ));
            if let Some(program) = &plan.program {
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-planned-execution-program",
                    &[("program", program.clone())],
                    format!("  Program: {program}"),
                ));
            }
            if !plan.arguments.is_empty() {
                let arguments = plan.arguments.join(" ");
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-planned-execution-arguments",
                    &[("arguments", arguments.clone())],
                    format!("  Arguments: {arguments}"),
                ));
            }
            if let Some(path) = &plan.working_directory {
                detail_lines.push(format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-planned-execution-working-directory",
                    &[("path", path.display().to_string())],
                    format!("  Working directory: {}", path.display()),
                ));
            }
            detail_lines.extend(plan.verification_paths.iter().map(|path| {
                format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-planned-execution-verify",
                    &[("path", path.display().to_string())],
                    format!("  Verify: {}", path.display()),
                )
            }));
        }
        if let Some(manual) = &item.manual_instruction {
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-manual-title",
                &[("title", manual.title.clone())],
                format!("{}:", manual.title),
            ));
            detail_lines.extend(manual.steps.iter().map(|step| {
                format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-manual-step",
                    &[("step", step.clone())],
                    format!("  {step}"),
                )
            }));
            detail_lines.extend(manual.notes.iter().map(|note| {
                format_localized_message(
                    localizer.as_ref(),
                    "wizard-summary-manual-note",
                    &[("note", note.clone())],
                    format!("  Note: {note}"),
                )
            }));
        }
    }

    WizardInstallSummary {
        status_line: format_localized_message(
            localizer.as_ref(),
            "wizard-summary-status-finished",
            &[
                ("installed", installed_or_checked.to_string()),
                ("manual", manual_items.to_string()),
            ],
            format!(
                "Finished. {installed_or_checked} package item(s) installed or checked; {manual_items} require manual attention."
            ),
        ),
        detail_lines,
    }
}

fn format_localized_message(
    localizer: Option<&Localizer>,
    id: &str,
    args: &[(&str, String)],
    fallback: String,
) -> String {
    let Some(localizer) = localizer else {
        return fallback;
    };
    let borrowed_args = args
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();
    localizer.format(id, &borrowed_args).value
}

fn planned_execution_runner_label(
    localizer: Option<&Localizer>,
    kind: PlannedExecutionKind,
) -> String {
    let (id, fallback) = match kind {
        PlannedExecutionKind::LaunchInstallerExecutable => (
            "wizard-planned-runner-launch-installer",
            "Launch installer executable",
        ),
        PlannedExecutionKind::ExtractArchiveAndRunInstaller => (
            "wizard-planned-runner-extract-archive",
            "Extract archive and run contained installer",
        ),
        PlannedExecutionKind::MountDiskImageAndRunInstaller => (
            "wizard-planned-runner-mount-disk-image",
            "Mount disk image and run contained installer",
        ),
    };
    localizer
        .map(|localizer| localizer.text(id).value)
        .unwrap_or_else(|| fallback.to_string())
}

fn package_rows(
    localizer: &Localizer,
    text: &WizardText,
    platform: Platform,
    architecture: Architecture,
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
            let summary = localizer
                .format(
                    "wizard-package-row",
                    &[
                        ("package", display_name.as_str()),
                        ("action", action_label.as_str()),
                        ("installed", installed_version.as_str()),
                        ("available", available_version.as_str()),
                    ],
                )
                .value;
            let (handling_summary, manual_attention_expected) =
                package_handling_summary(text, &action.package_id, platform, architecture);
            PackageRow {
                package_id: action.package_id.clone(),
                summary: summary.clone(),
                details: format!(
                    "{summary}\n\n{}\n\n{}: {}",
                    action.reason, text.package_details_handling_prefix, handling_summary
                ),
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
                handling_summary,
                manual_attention_expected,
            }
        })
        .collect()
}

fn package_handling_summary(
    text: &WizardText,
    package_id: &str,
    platform: Platform,
    architecture: Architecture,
) -> (String, bool) {
    match package_automation_support(package_id, platform, architecture) {
        PackageAutomationSupport::Direct => (text.package_handling_automatic.clone(), false),
        PackageAutomationSupport::AvailableUnattended(_) => {
            (text.package_handling_unattended.clone(), false)
        }
        PackageAutomationSupport::PlannedUnattended(_) => {
            (text.package_handling_planned.clone(), true)
        }
        PackageAutomationSupport::Unavailable => (text.package_handling_unavailable.clone(), true),
    }
}

fn package_display_name(model: &WizardModel, package_id: &str) -> String {
    if let Ok(localizer) = localizer_from_options(&model.bootstrap_options) {
        if let Some(spec) = builtin_package_specs(model.platform)
            .into_iter()
            .find(|spec| spec.id == package_id)
        {
            return localizer.text(&spec.display_name_key).value;
        }
    }

    builtin_package_specs(model.platform)
        .into_iter()
        .find(|spec| spec.id == package_id)
        .map(|spec| spec.display_name)
        .unwrap_or_else(|| package_id.to_string())
}

fn predicted_backup_paths_for_package_rows(
    model: &WizardModel,
    target: &TargetRow,
    selected_package_indices: &[usize],
    package_rows: &[PackageRow],
    osara_keymap_choice: OsaraKeymapChoice,
) -> Vec<PathBuf> {
    let package_specs = package_specs_by_id(model.platform);
    let mut backup_paths = Vec::new();

    for index in selected_package_indices {
        let Some(package) = package_rows.get(*index) else {
            continue;
        };
        if package_requires_manual_attention(model, package, osara_keymap_choice) {
            continue;
        }

        let Some(spec) = package_specs.get(&package.package_id) else {
            continue;
        };
        if spec.backup_policy != BackupPolicy::BackupOverwrittenFiles {
            continue;
        }

        backup_paths.extend(existing_package_backup_sources(&target.path, spec));
    }

    if !backup_paths.is_empty() {
        let receipt_path = target.path.join("RAIS").join("install-state.json");
        if receipt_path.is_file() {
            backup_paths.push(receipt_path);
        }
    }

    backup_paths.sort();
    backup_paths.dedup();
    backup_paths
}

fn existing_package_backup_sources(resource_path: &Path, spec: &PackageSpec) -> Vec<PathBuf> {
    let user_plugins = resource_path.join("UserPlugins");
    if !user_plugins.is_dir() {
        return Vec::new();
    }

    fs::read_dir(&user_plugins)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(std::result::Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            let file_name = file_name.to_ascii_lowercase();
            let prefix_matches = spec.user_plugin_prefixes.is_empty()
                || spec
                    .user_plugin_prefixes
                    .iter()
                    .any(|prefix| file_name.starts_with(&prefix.to_ascii_lowercase()));
            let suffix_matches = spec.user_plugin_suffixes.is_empty()
                || spec
                    .user_plugin_suffixes
                    .iter()
                    .any(|suffix| file_name.ends_with(&suffix.to_ascii_lowercase()));
            prefix_matches && suffix_matches
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

fn architecture_text(localizer: &Localizer, architecture: Option<Architecture>) -> String {
    match architecture.unwrap_or(Architecture::Unknown) {
        Architecture::X86 => "x86".to_string(),
        Architecture::X64 => "x64".to_string(),
        Architecture::Arm64 => "arm64".to_string(),
        Architecture::Arm64Ec => "arm64ec".to_string(),
        Architecture::Universal => "universal".to_string(),
        Architecture::Unknown => localizer.text("detect-architecture-unknown").value,
    }
}

fn unknown_version_text(model: &WizardModel) -> String {
    localizer_from_options(&model.bootstrap_options)
        .map(|localizer| localizer.text("detect-version-unknown").value)
        .unwrap_or_else(|_| "Version unknown".to_string())
}

fn unknown_architecture_text(model: &WizardModel) -> String {
    localizer_from_options(&model.bootstrap_options)
        .map(|localizer| localizer.text("detect-architecture-unknown").value)
        .unwrap_or_else(|_| "Architecture unknown".to_string())
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

fn path_likely_requires_admin_prompt(platform: Platform, path: &Path) -> bool {
    nearest_existing_ancestor(path)
        .and_then(|existing_path| fs::metadata(existing_path).ok())
        .is_some_and(|metadata| metadata.permissions().readonly())
        || protected_system_roots(platform)
            .iter()
            .any(|root| path_is_same_or_nested(path, root))
}

fn protected_system_roots(platform: Platform) -> Vec<PathBuf> {
    match platform {
        Platform::Windows => {
            let mut roots = Vec::new();
            for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
                if let Some(value) = std::env::var_os(key) {
                    roots.push(PathBuf::from(value));
                }
            }
            if roots.is_empty() {
                roots.push(PathBuf::from(r"C:\Program Files"));
                roots.push(PathBuf::from(r"C:\Program Files (x86)"));
            }
            roots
        }
        Platform::MacOs => vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/Library"),
            PathBuf::from("/System"),
        ],
    }
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if current.exists() {
            return Some(current);
        }
        current = current.parent()?.to_path_buf();
    }
}

fn path_is_same_or_nested(path: &Path, root: &Path) -> bool {
    if cfg!(target_os = "windows") {
        let path = normalize_windows_path(path);
        let root = normalize_windows_path(root);
        path == root || path.starts_with(&(root + "\\"))
    } else {
        path == root || path.starts_with(root)
    }
}

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rais_core::artifact::{ArtifactDescriptor, ArtifactKind};
    use rais_core::install::{InstallFileAction, InstallFileReport, InstallReport};
    use rais_core::localization::{DEFAULT_LOCALE, Localizer};
    use rais_core::model::{Architecture, Confidence, Installation, InstallationKind, Platform};
    use rais_core::operation::{
        ManualInstallInstruction, PackageOperationItem, PackageOperationReport,
        PackageOperationStatus, PlannedExecutionKind, PlannedExecutionPlan,
    };
    use rais_core::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_REAPER};
    use rais_core::plan::{InstallPlan, PlanAction, PlanActionKind};
    use rais_core::preflight::PreflightReport;
    use rais_core::resource::{
        ResourceInitAction, ResourceInitActionKind, ResourceInitItemKind, ResourceInitReport,
    };
    use rais_core::setup::SetupReport;
    use rais_core::version::Version;
    use tempfile::tempdir;

    use super::{
        OsaraKeymapChoice, UiBootstrapOptions, WizardInstallRequest, custom_portable_target_row,
        localizer_from_options, model_from_plan, refreshed_target_row,
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
    fn wizard_command_labels_include_native_mnemonics() {
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

        assert_eq!(model.controls.back_label, "&Back");
        assert_eq!(model.controls.next_label, "&Next");
        assert_eq!(model.controls.install_label, "&Install");
        assert_eq!(model.controls.close_label, "&Close");
        assert_eq!(
            model.text.package_handling_unattended,
            "RAIS can install this package unattended, including launching its installer when required."
        );
        assert_eq!(
            model.text.package_handling_planned,
            "RAIS is designed to run this package's installer or setup routine itself and finish the installation unattended, but this build still reports the steps instead of executing them."
        );
        assert_eq!(
            model.text.packages_osara_keymap_replace_label,
            "Replace current key map with OSARA key map"
        );
        assert_eq!(model.text.done_launch_reaper_label, "&Launch REAPER");
        assert_eq!(model.text.done_open_resource_label, "&Open resource folder");
        assert_eq!(model.text.done_rescan_label, "&Rescan target");
        assert_eq!(model.text.done_save_report_label, "&Save report");
    }

    #[test]
    fn wx_mnemonic_labels_support_translated_access_keys() {
        assert_eq!(super::wx_mnemonic_label("Weiter", "W"), "&Weiter");
        assert_eq!(super::wx_mnemonic_label("Schliessen", "S"), "&Schliessen");
        assert_eq!(
            super::wx_mnemonic_label("Bericht speichern", "S"),
            "Bericht &speichern"
        );
        assert_eq!(super::wx_mnemonic_label("Weiter", "X"), "Weiter (&X)");
        assert_eq!(
            super::wx_mnemonic_label("Save & report", "S"),
            "&Save && report"
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
        assert!(
            model.target_rows[0]
                .details
                .contains("REAPER application path")
        );
        assert!(model.target_rows[0].details.contains("REAPER version"));
        assert!(model.target_rows[0].details.contains("Architecture"));
        assert!(model.target_rows[0].details.contains("Writable"));
        assert_eq!(model.package_rows.len(), 2);
        assert_eq!(model.package_rows[0].display_name, "OSARA");
        assert!(model.package_rows[0].summary.contains("OSARA"));
        assert!(model.package_rows[0].details.contains("Handling:"));
        assert_eq!(model.package_rows[0].action_label, "Install");
        assert!(!model.package_rows[0].manual_attention_expected);
        assert_eq!(
            model.package_rows[0].handling_summary,
            model.text.package_handling_unattended
        );
        assert!(model.package_rows[0].selected);
        assert_eq!(model.package_rows[1].action_label, "Keep");
        assert!(!model.package_rows[1].manual_attention_expected);
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
                osara_keymap_choice: OsaraKeymapChoice::ReplaceCurrent,
                cache_dir: Some(PathBuf::from("C:/cache")),
            },
        )
        .unwrap();

        assert_eq!(request.resource_path, PathBuf::from("C:/REAPER"));
        assert_eq!(request.package_ids, vec![PACKAGE_OSARA.to_string()]);
        assert!(request.portable);
        assert_eq!(
            request.target_app_path,
            Some(PathBuf::from("C:/REAPER/reaper.exe"))
        );
        assert!(request.dry_run);
        assert_eq!(
            request.osara_keymap_choice,
            OsaraKeymapChoice::ReplaceCurrent
        );
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
        assert!(row.app_path.is_none());
        assert_eq!(
            row.planned_app_path,
            dir.path().join("PortableREAPER").join("reaper.exe")
        );
        assert!(row.label.contains("Portable REAPER folder"));
        assert!(row.details.contains("REAPER application path"));
        assert!(row.details.contains("REAPER version: Version unknown"));
        assert!(row.details.contains("Architecture: Architecture unknown"));
        assert!(row.details.contains("Portable resource path"));
    }

    #[test]
    fn custom_portable_target_uses_reaper_exe_when_present() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        std::fs::create_dir_all(&resource_path).unwrap();
        std::fs::write(resource_path.join("reaper.exe"), b"").unwrap();
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

        let row = custom_portable_target_row(&model, resource_path.clone(), true);

        assert_eq!(row.app_path, Some(resource_path.join("reaper.exe")));
        assert_eq!(row.planned_app_path, resource_path.join("reaper.exe"));
    }

    #[test]
    fn refreshed_standard_target_row_detects_app_that_appeared_after_startup() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("REAPER");
        let app_path = dir
            .path()
            .join("Program Files")
            .join("REAPER")
            .join("reaper.exe");
        std::fs::create_dir_all(&resource_path).unwrap();
        std::fs::create_dir_all(app_path.parent().unwrap()).unwrap();

        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = Installation {
            kind: InstallationKind::Standard,
            platform: Platform::Windows,
            app_path: app_path.clone(),
            resource_path: resource_path.clone(),
            version: None,
            architecture: Some(Architecture::X64),
            writable: true,
            confidence: Confidence::Low,
            evidence: Vec::new(),
        };
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

        assert!(model.target_rows[0].app_path.is_none());

        std::fs::write(&app_path, b"").unwrap();

        let refreshed = refreshed_target_row(&model, &model.target_rows[0]);

        assert_eq!(refreshed.app_path, Some(app_path.clone()));
        assert_eq!(refreshed.planned_app_path, app_path);
        assert!(
            refreshed
                .details
                .contains(&resource_path.display().to_string())
        );
    }

    #[test]
    fn builds_package_plan_for_custom_target_path() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("PortableREAPER").join("UserPlugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("reaper_reapack-x64.dll"), b"installed").unwrap();
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
        let target = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);

        let plan = super::wizard_package_plan_for_target(&model, Some(&target)).unwrap();
        let reapack = plan
            .package_rows
            .iter()
            .find(|row| row.package_id == PACKAGE_REAPACK)
            .unwrap();

        assert_eq!(reapack.action, PlanActionKind::Keep);
        assert!(!reapack.selected);
        assert!(plan.package_rows.iter().any(|row| row.selected));
    }

    #[test]
    fn package_plan_includes_reaper_for_empty_custom_target() {
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
        let target = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);

        let plan = super::wizard_package_plan_for_target(&model, Some(&target)).unwrap();
        let reaper = plan
            .package_rows
            .iter()
            .find(|row| row.package_id == PACKAGE_REAPER)
            .unwrap();

        assert_eq!(reaper.display_name, "REAPER");
        assert_eq!(reaper.action, PlanActionKind::Install);
        assert!(!reaper.manual_attention_expected);
        assert!(reaper.details.contains("Handling:"));
        assert!(
            reaper
                .details
                .contains(&model.text.package_handling_unattended)
        );
        assert!(reaper.selected);
    }

    #[test]
    fn selectable_installations_appends_standard_target_when_missing() {
        let installations =
            super::selectable_installations(Platform::Windows, vec![fake_installation()]);

        assert_eq!(installations[0].kind, InstallationKind::Portable);
        assert_eq!(
            installations
                .iter()
                .filter(|installation| installation.kind == InstallationKind::Standard)
                .count(),
            1
        );
    }

    #[test]
    fn selectable_installations_does_not_duplicate_detected_standard_target() {
        let installations = super::selectable_installations(
            Platform::Windows,
            vec![fake_standard_installation(), fake_installation()],
        );

        assert_eq!(
            installations
                .iter()
                .filter(|installation| installation.kind == InstallationKind::Standard)
                .count(),
            1
        );
    }

    #[test]
    fn review_preview_lists_manual_attention_for_selected_packages() {
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
        let target = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);
        let plan = super::wizard_package_plan_for_target(&model, Some(&target)).unwrap();
        let selected = plan
            .package_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| (row.package_id == PACKAGE_OSARA).then_some(index))
            .collect::<Vec<_>>();

        let preview = super::build_review_preview_for_package_rows(
            &model,
            Some(&target),
            &selected,
            &plan.package_rows,
            &plan.notes,
            OsaraKeymapChoice::ReplaceCurrent,
        );

        assert!(preview.can_install);
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line == "Manual attention expected")
        );
        assert!(preview.lines.iter().any(|line| {
            line.contains("OSARA")
                && line.contains(
                    "RAIS is designed to run this package's installer or setup routine itself",
                )
        }));
        assert!(preview.lines.iter().any(|line| {
            line.contains("RAIS will download the upstream installer during the run.")
        }));
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.contains("target, choose this resource or portable folder"))
        );
    }

    #[test]
    fn reaper_windows_row_uses_unattended_handling() {
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
        let target = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);
        let plan = super::wizard_package_plan_for_target(&model, Some(&target)).unwrap();
        let reaper_row = plan
            .package_rows
            .iter()
            .find(|row| row.package_id == PACKAGE_REAPER)
            .unwrap();

        assert!(!reaper_row.manual_attention_expected);
        assert_eq!(
            reaper_row.handling_summary,
            model.text.package_handling_unattended
        );
    }

    #[test]
    fn review_preview_includes_osara_keymap_choice() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = fake_installation();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![installation.clone()],
            Some(0),
            InstallPlan {
                target: Some(installation),
                actions: vec![PlanAction {
                    package_id: PACKAGE_OSARA.to_string(),
                    action: PlanActionKind::Install,
                    installed_version: None,
                    available_version: Some(Version::parse("2026.1").unwrap()),
                    reason: "Missing".to_string(),
                }],
                notes: Vec::new(),
            },
        );

        let preview = super::build_review_preview_for_package_rows(
            &model,
            model.target_rows.first(),
            &[0],
            &model.package_rows,
            &model.notes,
            OsaraKeymapChoice::ReplaceCurrent,
        );

        assert!(preview.lines.iter().any(|line| line == "OSARA key map"));
        assert!(preview.lines.iter().any(|line| {
            line.contains("Replace the current key map") && line.contains("reaper-kb.ini")
        }));
    }

    #[test]
    fn review_preview_lists_expected_backup_files_for_direct_updates() {
        let dir = tempdir().unwrap();
        let resource_path = dir.path().join("PortableREAPER");
        let plugins = resource_path.join("UserPlugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(resource_path.join("RAIS")).unwrap();
        std::fs::write(plugins.join("reaper_reapack-x64.dll"), b"old").unwrap();
        std::fs::write(resource_path.join("RAIS/install-state.json"), b"{}").unwrap();

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
        let target = custom_portable_target_row(&model, resource_path, true);
        let package_rows = vec![super::PackageRow {
            package_id: PACKAGE_REAPACK.to_string(),
            display_name: "ReaPack".to_string(),
            selected: true,
            summary: "ReaPack: Update".to_string(),
            details: "ReaPack details".to_string(),
            installed_version: "1.2.5".to_string(),
            available_version: "1.2.6".to_string(),
            action: PlanActionKind::Update,
            action_label: "Update".to_string(),
            reason: "Outdated".to_string(),
            handling_summary: model.text.package_handling_automatic.clone(),
            manual_attention_expected: false,
        }];

        let preview = super::build_review_preview_for_package_rows(
            &model,
            Some(&target),
            &[0],
            &package_rows,
            &[],
            OsaraKeymapChoice::PreserveCurrent,
        );

        assert!(preview.lines.iter().any(|line| line == "Backups expected"));
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.contains("reaper_reapack-x64.dll"))
        );
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.contains("install-state.json"))
        );
    }

    #[test]
    fn review_preview_lists_admin_prompt_for_standard_reaper_target() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let installation = fake_standard_installation();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![installation.clone()],
            Some(0),
            InstallPlan {
                target: Some(installation),
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );
        let package_rows = vec![super::PackageRow {
            package_id: PACKAGE_REAPER.to_string(),
            display_name: "REAPER".to_string(),
            selected: true,
            summary: "REAPER: Install".to_string(),
            details: "REAPER details".to_string(),
            installed_version: "Version unknown".to_string(),
            available_version: "7.69".to_string(),
            action: PlanActionKind::Install,
            action_label: "Install".to_string(),
            reason: "Missing".to_string(),
            handling_summary: model.text.package_handling_unattended.clone(),
            manual_attention_expected: false,
        }];

        let preview = super::build_review_preview_for_package_rows(
            &model,
            model.target_rows.first(),
            &[0],
            &package_rows,
            &[],
            OsaraKeymapChoice::PreserveCurrent,
        );

        assert!(
            preview
                .lines
                .iter()
                .any(|line| line == "Administrator prompts expected")
        );
        assert!(preview.lines.iter().any(|line| {
            line.contains("Administrator approval may be required for the REAPER application path")
                && line.contains("Program Files")
        }));
    }

    #[test]
    fn review_preview_reports_no_admin_prompt_for_user_portable_target() {
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
        let target = custom_portable_target_row(&model, dir.path().join("PortableREAPER"), true);
        let package_rows = vec![super::PackageRow {
            package_id: PACKAGE_REAPACK.to_string(),
            display_name: "ReaPack".to_string(),
            selected: true,
            summary: "ReaPack: Install".to_string(),
            details: "ReaPack details".to_string(),
            installed_version: "Version unknown".to_string(),
            available_version: "1.2.6".to_string(),
            action: PlanActionKind::Install,
            action_label: "Install".to_string(),
            reason: "Missing".to_string(),
            handling_summary: model.text.package_handling_automatic.clone(),
            manual_attention_expected: false,
        }];

        let preview = super::build_review_preview_for_package_rows(
            &model,
            Some(&target),
            &[0],
            &package_rows,
            &[],
            OsaraKeymapChoice::PreserveCurrent,
        );

        assert!(
            preview
                .lines
                .iter()
                .any(|line| { line == "No administrator prompt is currently expected." })
        );
    }

    #[test]
    fn osara_keymap_note_defaults_to_unavailable_when_osara_is_not_selected() {
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

        let note = super::osara_keymap_note(&model, false, OsaraKeymapChoice::PreserveCurrent);

        assert!(note.contains("Select OSARA"));
    }

    #[test]
    fn setup_summary_includes_manual_instruction_notes() {
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
        let report = SetupReport {
            resource_path: PathBuf::from("C:/PortableREAPER"),
            dry_run: true,
            resource_init: ResourceInitReport {
                resource_path: PathBuf::from("C:/PortableREAPER"),
                dry_run: true,
                portable: true,
                preflight: PreflightReport {
                    passed: true,
                    checks: Vec::new(),
                },
                actions: Vec::new(),
            },
            package_operation: PackageOperationReport {
                resource_path: PathBuf::from("C:/PortableREAPER"),
                dry_run: true,
                install_report: None,
                items: vec![PackageOperationItem {
                    package_id: PACKAGE_OSARA.to_string(),
                    plan_action: PlanActionKind::Install,
                    status: PackageOperationStatus::DeferredUnattended,
                    artifact: ArtifactDescriptor {
                        package_id: PACKAGE_OSARA.to_string(),
                        version: Version::parse("2026.1").unwrap(),
                        platform: Platform::Windows,
                        architecture: Architecture::X64,
                        kind: ArtifactKind::Installer,
                        url: "https://example.test/osara.exe".to_string(),
                        file_name: "osara.exe".to_string(),
                    },
                    cached_artifact: None,
                    install_action: None,
                    planned_execution: Some(PlannedExecutionPlan {
                        kind: PlannedExecutionKind::LaunchInstallerExecutable,
                        artifact_location: "https://example.test/osara.exe".to_string(),
                        program: Some("https://example.test/osara.exe".to_string()),
                        arguments: Vec::new(),
                        working_directory: None,
                        verification_paths: vec![
                            PathBuf::from("C:/PortableREAPER/UserPlugins"),
                            PathBuf::from("C:/PortableREAPER/osara"),
                        ],
                    }),
                    manual_instruction: Some(ManualInstallInstruction {
                        title: "Manual install required for osara".to_string(),
                        steps: vec!["Use this artifact: https://example.test/osara.exe".to_string()],
                        notes: vec![
                            "The selected workflow preserves the current key map. Leave reaper-kb.ini unchanged.".to_string(),
                        ],
                    }),
                    message: "This build has not implemented the planned unattended vendor installer execution path yet. RAIS did not download or run the artifact.".to_string(),
                }],
            },
        };

        let summary = super::summarize_setup_report(&model, &report);

        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Planned unattended execution"))
        );
        assert!(
            summary.detail_lines.iter().any(
                |line| line.contains("Runner:") && line.contains("Launch installer executable")
            )
        );
        assert!(summary.detail_lines.iter().any(|line| {
            line.contains("Note:") && line.contains("Leave reaper-kb.ini unchanged")
        }));
    }

    #[test]
    fn setup_summary_includes_backup_paths_when_present() {
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
        let report = SetupReport {
            resource_path: PathBuf::from("C:/PortableREAPER"),
            dry_run: false,
            resource_init: ResourceInitReport {
                resource_path: PathBuf::from("C:/PortableREAPER"),
                dry_run: false,
                portable: true,
                preflight: PreflightReport {
                    passed: true,
                    checks: Vec::new(),
                },
                actions: Vec::new(),
            },
            package_operation: PackageOperationReport {
                resource_path: PathBuf::from("C:/PortableREAPER"),
                dry_run: false,
                install_report: Some(InstallReport {
                    resource_path: PathBuf::from("C:/PortableREAPER"),
                    dry_run: false,
                    preflight: PreflightReport {
                        passed: true,
                        checks: Vec::new(),
                    },
                    receipt_written: true,
                    receipt_backup_path: Some(PathBuf::from(
                        "C:/PortableREAPER/RAIS/backups/unix-1/RAIS/install-state.json",
                    )),
                    backup_manifest_path: Some(PathBuf::from(
                        "C:/PortableREAPER/RAIS/backups/unix-1/backup-manifest.json",
                    )),
                    actions: vec![InstallFileReport {
                        package_id: PACKAGE_REAPACK.to_string(),
                        source_path: PathBuf::from("C:/cache/reaper_reapack-x64.dll"),
                        target_path: PathBuf::from(
                            "C:/PortableREAPER/UserPlugins/reaper_reapack-x64.dll",
                        ),
                        backup_path: Some(PathBuf::from(
                            "C:/PortableREAPER/RAIS/backups/unix-1/UserPlugins/reaper_reapack-x64.dll",
                        )),
                        action: InstallFileAction::Replaced,
                        size: 7,
                        sha256: "hash".to_string(),
                    }],
                }),
                items: vec![PackageOperationItem {
                    package_id: PACKAGE_REAPACK.to_string(),
                    plan_action: PlanActionKind::Update,
                    status: PackageOperationStatus::InstalledOrChecked,
                    artifact: ArtifactDescriptor {
                        package_id: PACKAGE_REAPACK.to_string(),
                        version: Version::parse("1.2.6").unwrap(),
                        platform: Platform::Windows,
                        architecture: Architecture::X64,
                        kind: ArtifactKind::ExtensionBinary,
                        url: "https://example.test/reaper_reapack-x64.dll".to_string(),
                        file_name: "reaper_reapack-x64.dll".to_string(),
                    },
                    cached_artifact: None,
                    install_action: None,
                    planned_execution: None,
                    manual_instruction: None,
                    message: "Single extension binary handled by RAIS installer.".to_string(),
                }],
            },
        };

        let summary = super::summarize_setup_report(&model, &report);

        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Backup file:"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Receipt backup:"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Backup manifest:"))
        );
    }

    #[test]
    fn review_resource_lines_only_include_pending_changes() {
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
        let report = ResourceInitReport {
            resource_path: PathBuf::from("C:/PortableREAPER"),
            dry_run: true,
            portable: true,
            preflight: PreflightReport {
                passed: true,
                checks: Vec::new(),
            },
            actions: vec![
                ResourceInitAction {
                    path: PathBuf::from("C:/PortableREAPER"),
                    kind: ResourceInitItemKind::Directory,
                    action: ResourceInitActionKind::AlreadyExists,
                },
                ResourceInitAction {
                    path: PathBuf::from("C:/PortableREAPER/UserPlugins"),
                    kind: ResourceInitItemKind::Directory,
                    action: ResourceInitActionKind::WouldCreate,
                },
                ResourceInitAction {
                    path: PathBuf::from("C:/PortableREAPER/reaper.ini"),
                    kind: ResourceInitItemKind::File,
                    action: ResourceInitActionKind::WouldCreate,
                },
            ],
        };

        let lines = super::review_resource_lines(&model, &report);

        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("Create directory:"));
        assert!(lines[0].contains("UserPlugins"));
        assert!(lines[1].starts_with("Create file:"));
        assert!(lines[1].contains("reaper.ini"));
    }

    #[test]
    fn wizard_error_summary_includes_selected_request_context() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![fake_installation()],
            Some(0),
            InstallPlan {
                target: None,
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );
        let request = sample_install_request(PathBuf::from("C:/PortableREAPER"));
        let error = rais_core::RaisError::PreflightFailed {
            message: "REAPER is running.".to_string(),
        };

        let summary = super::summarize_wizard_error(&model, &request, &error);

        assert_eq!(
            summary.status_line,
            "Installation failed. Review the error below."
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Packages selected: OSARA, ReaPack"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line == "OSARA key map")
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Replace the current key map"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Error: preflight failed: REAPER is running."))
        );
    }

    #[test]
    fn saves_wizard_outcome_error_report_under_resource_logs() {
        let dir = tempdir().unwrap();
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let model = model_from_plan(
            &localizer,
            Platform::Windows,
            Architecture::X64,
            vec![fake_installation()],
            Some(0),
            InstallPlan {
                target: None,
                actions: Vec::new(),
                notes: Vec::new(),
            },
        );
        let request = sample_install_request(dir.path().join("PortableREAPER"));
        let error = rais_core::RaisError::PreflightFailed {
            message: "Target path blocked".to_string(),
        };
        let report = super::wizard_outcome_report_from_error(&model, &request, &error);

        let path = super::save_wizard_outcome_report(&report).unwrap();
        let json_path = path.with_extension("json");

        assert!(path.starts_with(dir.path().join("PortableREAPER/RAIS/logs")));
        assert!(path.is_file());
        assert!(json_path.is_file());
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("status: error"));
        assert!(content.contains("error_message: preflight failed: Target path blocked"));
    }

    #[test]
    fn saves_wizard_setup_report_under_resource_logs() {
        let dir = tempdir().unwrap();
        let report = empty_setup_report(dir.path().join("PortableREAPER"));

        let path = super::save_wizard_setup_report(&report).unwrap();
        let json_path = path.with_extension("json");

        assert!(path.starts_with(dir.path().join("PortableREAPER/RAIS/logs")));
        assert!(path.is_file());
        assert!(json_path.is_file());
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("RAIS Report"));
        assert!(content.contains("resource_path:"));
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

    fn fake_standard_installation() -> Installation {
        Installation {
            kind: InstallationKind::Standard,
            platform: Platform::Windows,
            app_path: PathBuf::from("C:/Program Files/REAPER/reaper.exe"),
            resource_path: PathBuf::from("C:/Users/Test/AppData/Roaming/REAPER"),
            version: Some(Version::parse("7.69").unwrap()),
            architecture: Some(Architecture::X64),
            writable: true,
            confidence: Confidence::High,
            evidence: Vec::new(),
        }
    }

    fn empty_setup_report(resource_path: PathBuf) -> SetupReport {
        SetupReport {
            resource_path: resource_path.clone(),
            dry_run: true,
            resource_init: ResourceInitReport {
                resource_path: resource_path.clone(),
                dry_run: true,
                portable: true,
                preflight: PreflightReport {
                    passed: true,
                    checks: Vec::new(),
                },
                actions: Vec::new(),
            },
            package_operation: PackageOperationReport {
                resource_path,
                dry_run: true,
                install_report: None,
                items: Vec::new(),
            },
        }
    }

    fn sample_install_request(resource_path: PathBuf) -> WizardInstallRequest {
        WizardInstallRequest {
            resource_path: resource_path.clone(),
            package_ids: vec![PACKAGE_OSARA.to_string(), PACKAGE_REAPACK.to_string()],
            platform: Platform::Windows,
            architecture: Architecture::X64,
            portable: true,
            target_app_path: Some(resource_path.join("reaper.exe")),
            dry_run: false,
            allow_reaper_running: false,
            stage_unsupported: true,
            osara_keymap_choice: OsaraKeymapChoice::ReplaceCurrent,
            cache_dir: PathBuf::from("C:/cache"),
        }
    }
}
