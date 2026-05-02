#[cfg(feature = "gui")]
mod wx_app;

/// Run the wxDragon wizard. Wraps the internal `wx_app::run` so the merged
/// `rais` binary can spawn the GUI without needing to know the module's
/// internals.
#[cfg(feature = "gui")]
pub fn run_gui() {
    wx_app::run();
}

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use rais_core::artifact::{default_cache_dir, expected_artifact_kind};
use rais_core::detection::{
    DiscoveryOptions, default_standard_installation, detect_components, discover_installations,
};
use rais_core::latest::fetch_latest_versions;
use rais_core::localization::{DEFAULT_LOCALE, Localizer, embedded_locales};
use rais_core::metadata::file_version;
use rais_core::model::{Architecture, Confidence, Installation, InstallationKind, Platform};
use rais_core::operation::{
    PackageAutomationSupport, PackageOperationStatus, PlannedExecutionKind,
    package_automation_support, preview_manual_instruction,
};
use rais_core::package::{
    HostCapabilities, PACKAGE_JAWS_SCRIPTS, PACKAGE_OSARA, PackageSpec, builtin_package_specs,
    detect_host_capabilities, host_supports_package,
};
use rais_core::plan::{
    AvailablePackage, InstallPlan, PlanAction, PlanActionKind, build_install_plan,
};
use rais_core::report::{default_report_path, save_json_and_text_reports};
use rais_core::resource::{ResourceInitActionKind, ResourceInitOptions, initialize_resource_path};
use rais_core::self_update::{
    ApplySelfUpdateOptions, DEFAULT_SELF_UPDATE_MANIFEST_URL, SelfUpdateApplyReport,
    SelfUpdateCheckReport, apply_self_update, check_self_update, default_self_update_staging_dir,
    relaunch_current_executable, stage_self_update,
};
use rais_core::setup::{
    SetupOptions, SetupReport, execute_setup_operation, setup_requires_extension_support,
};
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
    pub language_options: Vec<LanguageOption>,
    pub current_language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageOption {
    pub locale: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardText {
    pub common_yes: String,
    pub common_no: String,
    pub target_heading: String,
    pub target_language_label: String,
    pub target_language_restart_note: String,
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
    pub target_custom_portable_writable_label: String,
    pub target_custom_portable_note: String,
    pub packages_heading: String,
    pub packages_list_label: String,
    pub reapack_ack_heading: String,
    pub reapack_ack_body: String,
    pub reapack_ack_link_label: String,
    pub reapack_ack_confirm_label: String,
    pub version_check_heading: String,
    pub version_check_status_pending: String,
    pub version_check_progress_label: String,
    pub version_check_error_heading: String,
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
    pub review_package_heading: String,
    pub review_osara_keymap_heading: String,
    pub review_osara_keymap_preserve: String,
    pub review_osara_keymap_replace: String,
    pub review_notes_heading: String,
    pub review_preflight_prefix: String,
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
    pub done_show_details_label: String,
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
    pub done_self_update_apply_label: String,
    pub done_self_update_apply_running: String,
    pub done_self_update_error_prefix: String,
    pub done_self_update_relaunch_prefix: String,
    pub self_update_status_checking: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Target,
    VersionCheck,
    Packages,
    ReapackAcknowledgement,
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
    pub description: String,
    pub selected: bool,
    pub summary: String,
    pub details: String,
    pub installed_version: String,
    pub available_version: String,
    pub action: PlanActionKind,
    pub action_label: String,
    /// Plan-time action, captured before any user toggle of the package
    /// checkbox. Used by the wizard's checklist handler to decide whether
    /// re-checking a row means "Install" (originally not installed) or
    /// "Update" (already installed) — the displayed `action` mutates as
    /// the user clicks, but `original_action` is the authoritative anchor.
    pub original_action: PlanActionKind,
    pub reason: String,
    pub handling_summary: String,
    pub manual_attention_expected: bool,
    /// `false` when this package can't be installed against the currently
    /// selected target — the row is shown but its checkbox is disabled and
    /// the row label carries a localized indicator. Today only true → false
    /// flip is "JAWS-for-REAPER scripts on a portable REAPER target", since
    /// the NSIS installer hard-codes `%APPDATA%\REAPER\UserPlugins\` and
    /// can't honor the portable destination.
    pub available_for_target: bool,
    /// Localized reason matching `available_for_target == false`. `None`
    /// when the row is available.
    pub unavailability_reason: Option<String>,
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
            osara_keymap_choice: OsaraKeymapChoice::ReplaceCurrent,
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
    /// Packages whose plan-time decision was `Keep` (already current) but
    /// the user explicitly checked the box anyway, opting in to a
    /// re-install. The setup pipeline promotes these from Keep to Update
    /// so the install step actually runs instead of being silently
    /// skipped.
    pub force_reinstall_packages: Vec<String>,
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
    let desired = wizard_desired_package_ids(platform);
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

pub fn localizer_from_options(options: &UiBootstrapOptions) -> Result<Localizer> {
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
        language_options: language_options(localizer),
        current_language: localizer.active_locale().to_string(),
    }
}

fn language_options(localizer: &Localizer) -> Vec<LanguageOption> {
    let mut options: Vec<LanguageOption> = embedded_locales()
        .iter()
        .map(|locale| {
            let key = format!("wizard-locale-name-{locale}");
            let text = localizer.text(&key);
            let display_name = if text.missing {
                (*locale).to_string()
            } else {
                text.value
            };
            LanguageOption {
                locale: (*locale).to_string(),
                display_name,
            }
        })
        .collect();
    options.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    options
}

fn wizard_text(localizer: &Localizer) -> WizardText {
    WizardText {
        common_yes: localizer.text("common-yes").value,
        common_no: localizer.text("common-no").value,
        target_heading: localizer.text("wizard-target-heading").value,
        target_language_label: localizer.text("wizard-target-language-label").value,
        target_language_restart_note: localizer.text("wizard-target-language-restart-note").value,
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
        target_custom_portable_writable_label: localizer
            .text("wizard-target-custom-portable-writable-label")
            .value,
        target_custom_portable_note: localizer.text("wizard-target-custom-portable-note").value,
        packages_heading: localizer.text("wizard-packages-heading").value,
        packages_list_label: localizer.text("wizard-packages-list-label").value,
        reapack_ack_heading: localizer.text("wizard-reapack-ack-heading").value,
        reapack_ack_body: localizer.text("wizard-reapack-ack-body").value,
        reapack_ack_link_label: localizer.text("wizard-reapack-ack-link-label").value,
        reapack_ack_confirm_label: localizer.text("wizard-reapack-ack-confirm-label").value,
        version_check_heading: localizer.text("wizard-version-check-heading").value,
        version_check_status_pending: localizer.text("wizard-version-check-status-pending").value,
        version_check_progress_label: localizer.text("wizard-version-check-progress-label").value,
        version_check_error_heading: localizer.text("wizard-version-check-error-heading").value,
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
        review_package_heading: localizer.text("wizard-review-package-heading").value,
        review_osara_keymap_heading: localizer.text("wizard-review-osara-keymap-heading").value,
        review_osara_keymap_preserve: localizer.text("wizard-review-osara-keymap-preserve").value,
        review_osara_keymap_replace: localizer.text("wizard-review-osara-keymap-replace").value,
        review_notes_heading: localizer.text("wizard-review-notes-heading").value,
        review_preflight_prefix: localizer.text("wizard-review-preflight-prefix").value,
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
        done_show_details_label: localizer.text("wizard-done-show-details").value,
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
        done_self_update_apply_label: localized_wx_mnemonic_label(
            localizer,
            "wizard-done-self-update-apply",
            "wizard-done-self-update-apply-mnemonic",
        ),
        done_self_update_apply_running: localizer
            .text("wizard-done-self-update-apply-running")
            .value,
        done_self_update_error_prefix: localizer.text("wizard-done-self-update-error-prefix").value,
        done_self_update_relaunch_prefix: localizer
            .text("wizard-done-self-update-relaunch-prefix")
            .value,
        self_update_status_checking: localizer.text("wizard-self-update-status-checking").value,
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
        (WizardStep::VersionCheck, "wizard-step-version-check"),
        (WizardStep::Packages, "wizard-step-packages"),
        (
            WizardStep::ReapackAcknowledgement,
            "wizard-step-reapack-acknowledgement",
        ),
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
                    ("path", &installation.resource_path.display().to_string()),
                    (
                        "writable",
                        yes_no(localizer, installation.writable).as_str(),
                    ),
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
    // A row is "force reinstall" when the user has the box checked but
    // the original detection said the package was already current — i.e.
    // the plan would normally Keep it, but the user explicitly opted in
    // to a re-run. Detected via `original_action == Keep` on a checked
    // row (the toggle helper promotes the displayed `action` to Update
    // in that case, but `original_action` keeps the plan-time decision
    // for exactly this disambiguation).
    let force_reinstall_packages = selected_package_indices
        .iter()
        .filter_map(|index| {
            let row = package_rows.get(*index)?;
            if row.original_action == PlanActionKind::Keep {
                Some(row.package_id.clone())
            } else {
                None
            }
        })
        .collect();

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
        force_reinstall_packages,
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

/// Returns true when ReaPack is selected and its planned action would
/// actually stage the package (Install or Update), i.e. the run will need
/// the donation acknowledgement before it proceeds.
pub fn reapack_selected_for_install_or_update(
    package_rows: &[PackageRow],
    indices: &[usize],
) -> bool {
    indices
        .iter()
        .filter_map(|index| package_rows.get(*index))
        .any(|row| {
            row.package_id == rais_core::package::PACKAGE_REAPACK
                && matches!(row.action, PlanActionKind::Install | PlanActionKind::Update)
        })
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

    let mut lines = vec![format!(
        "{}: {}",
        model.text.review_target_prefix,
        target.path.display()
    )];

    let mut can_install = !selected_package_indices.is_empty();

    // Run the resource-path preflight to surface fatal blockers (read-only
    // target, REAPER currently running, etc.) — those still need to land in
    // the GUI summary so the user knows install is blocked. Successful
    // resource-init details (the long "Create directory…" / "Create file…"
    // list) are now report-only; the GUI just notes that backups will be
    // taken if needed.
    match initialize_resource_path(
        &target.path,
        &ResourceInitOptions {
            dry_run: true,
            portable: target.portable,
            include_extension_support_dirs: target.portable
                || setup_requires_extension_support(
                    &selected_package_indices
                        .iter()
                        .filter_map(|index| package_rows.get(*index))
                        .map(|package| package.package_id.clone())
                        .collect::<Vec<_>>(),
                ),
            allow_reaper_running: false,
            target_app_path: Some(target.planned_app_path.clone()),
        },
    ) {
        Ok(_) => {}
        Err(error) => {
            can_install = false;
            lines.push(format!("{}: {}", model.text.review_preflight_prefix, error));
        }
    }

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

    if !notes.is_empty() {
        lines.push(String::new());
        lines.push(model.text.review_notes_heading.clone());
        lines.extend(notes.iter().cloned());
    }

    WizardReviewPreview { lines, can_install }
}

pub fn package_requires_manual_attention(
    _model: &WizardModel,
    package: &PackageRow,
    _osara_keymap_choice: OsaraKeymapChoice,
) -> bool {
    matches!(
        package.action,
        PlanActionKind::Install | PlanActionKind::Update
    ) && package.manual_attention_expected
}

pub fn manual_attention_handling_summary(
    _model: &WizardModel,
    package: &PackageRow,
    _osara_keymap_choice: OsaraKeymapChoice,
) -> String {
    package.handling_summary.clone()
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
    wizard_package_plan_for_target_with_available(model, target, &model.available_packages)
}

/// Like `wizard_package_plan_for_target`, but uses an explicit
/// `available_packages` list instead of the one stored in the model. This is
/// what the wizard calls after the GUI's background latest-version fetch
/// completes so the package list can be re-rendered with fresh upstream data
/// without rebuilding the whole model.
pub fn wizard_package_plan_for_target_with_available(
    model: &WizardModel,
    target: Option<&TargetRow>,
    available_packages: &[AvailablePackage],
) -> Result<WizardPackagePlan> {
    let localizer = localizer_from_options(&model.bootstrap_options)?;
    let detections = match target {
        Some(target) => detect_components(&target.path, model.platform)?,
        None => Vec::new(),
    };
    let desired = wizard_desired_package_ids(model.platform);
    let plan = build_install_plan(
        target.map(|target| installation_from_target_row(model, target)),
        &detections,
        &desired,
        available_packages,
    );
    let package_specs = builtin_package_specs(model.platform);
    let mut package_rows = package_rows(
        &localizer,
        &model.text,
        model.platform,
        model.architecture,
        &package_specs,
        &plan.actions,
    );

    // Portable + JAWS-for-REAPER scripts: the NSIS package's
    // `RequestExecutionLevel admin` path hard-codes
    // `%APPDATA%\REAPER\UserPlugins\` for the COM bridge DLL, so the
    // REAPER-side helper always lands in the *standard* REAPER folder
    // regardless of the portable target the user picked. Mark the row
    // as unavailable so the checklist disables its checkbox and the
    // row label carries a localized indicator — more discoverable than
    // a separate wizard-notes paragraph.
    if target.is_some_and(|target| target.portable) {
        for row in &mut package_rows {
            if row.package_id == PACKAGE_JAWS_SCRIPTS {
                mark_row_unavailable(&localizer, row, "wizard-package-row-unavailable-portable");
            }
        }
    }

    let can_install = package_rows.iter().any(|row| {
        row.available_for_target
            && matches!(row.action, PlanActionKind::Install | PlanActionKind::Update)
    });

    Ok(WizardPackagePlan {
        package_rows,
        notes: plan.notes,
        can_install,
    })
}

/// Mark `row` as unavailable: force-uncheck it, record the localized reason,
/// and append a localized "(not available: <reason>)" indicator to the row
/// summary so the indicator shows up in the package CheckListBox label.
fn mark_row_unavailable(localizer: &Localizer, row: &mut PackageRow, reason_key: &str) {
    let reason = localizer.text(reason_key).value;
    row.available_for_target = false;
    row.selected = false;
    let indicator = localizer
        .format(
            "wizard-package-row-unavailable-suffix",
            &[("reason", reason.as_str())],
        )
        .value;
    row.summary = format!("{} {}", row.summary, indicator);
    row.unavailability_reason = Some(reason);
}

/// Recompute a `PackageRow`'s `action`, `action_label`, `summary`, and
/// `selected` fields to match a new checkbox state. Used by the wizard's
/// CheckListBox toggle handler so the visible "Install/Update/Keep" label
/// follows what the user just clicked. Returns the freshly-formatted
/// summary so the caller can also push it into the CheckListBox label.
pub fn apply_checkbox_state_to_package_row(
    model: &WizardModel,
    row: &mut PackageRow,
    checked: bool,
) -> Result<String> {
    let localizer = localizer_from_options(&model.bootstrap_options)?;
    let new_action = if checked {
        // Originally-not-installed packages stay "Install" when re-checked;
        // anything else means the package is on disk, so re-checking it
        // means "Update" (re-stage the latest known upstream version).
        match row.original_action {
            PlanActionKind::Install => PlanActionKind::Install,
            _ => PlanActionKind::Update,
        }
    } else {
        PlanActionKind::Keep
    };
    let action_label = action_label(&localizer, new_action);
    let summary = localizer
        .format(
            "wizard-package-row",
            &[
                ("package", row.display_name.as_str()),
                ("action", action_label.as_str()),
                ("installed", row.installed_version.as_str()),
                ("available", row.available_version.as_str()),
            ],
        )
        .value;
    row.action = new_action;
    row.action_label = action_label;
    row.summary = summary.clone();
    row.selected = checked;
    Ok(summary)
}

/// Localized package display name for `package_id`, falling back to the raw id
/// when no Fluent key is available. Used by the version-check progress log.
pub fn localized_package_display_name(localizer: &Localizer, package_id: &str) -> String {
    let key = format!("package-{package_id}");
    let text = localizer.text(&key);
    if text.missing {
        package_id.to_string()
    } else {
        text.value
    }
}

/// List of package ids the wizard cares about for a given platform — exposed
/// so the GUI can iterate them without duplicating builtin_package_specs.
/// Host-conditional packages (e.g. JAWS-for-REAPER scripts) are filtered out
/// when the corresponding host facility isn't available.
pub fn wizard_desired_package_ids(platform: Platform) -> Vec<String> {
    wizard_desired_package_ids_for_host(platform, &detect_host_capabilities())
}

/// Same as [`wizard_desired_package_ids`] but with an explicit host snapshot,
/// so tests can pin "JAWS detected"/"JAWS missing" without touching the real
/// filesystem.
pub fn wizard_desired_package_ids_for_host(
    platform: Platform,
    host: &HostCapabilities,
) -> Vec<String> {
    builtin_package_specs(platform)
        .into_iter()
        .filter(|spec| host_supports_package(spec, host))
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
    TargetRow {
        label: format!(
            "{}: {}",
            model.text.target_custom_portable_label,
            path.display()
        ),
        details: format!(
            "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}",
            model.text.target_custom_portable_app_path_label,
            app_path
                .as_ref()
                .unwrap_or(&default_portable_reaper_app_path(model.platform, &path))
                .display(),
            model.text.target_custom_portable_path_label,
            path.display(),
            model.text.target_custom_portable_version_label,
            version_text,
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
            lock_path: None,
            force_reinstall_packages: request.force_reinstall_packages.clone(),
        },
    )
}

pub fn run_wizard_self_update_check() -> Result<SelfUpdateCheckReport> {
    let platform = Platform::current().ok_or(RaisError::UnsupportedPlatform)?;
    check_self_update(platform, DEFAULT_SELF_UPDATE_MANIFEST_URL)
}

pub fn run_wizard_self_update_apply() -> Result<SelfUpdateApplyReport> {
    let platform = Platform::current().ok_or(RaisError::UnsupportedPlatform)?;
    let staging_dir = default_self_update_staging_dir();
    let stage = stage_self_update(platform, DEFAULT_SELF_UPDATE_MANIFEST_URL, &staging_dir)?;
    apply_self_update(
        &stage,
        &ApplySelfUpdateOptions {
            install_root: None,
            install_target_basename: None,
        },
    )
}

pub fn relaunch_rais_after_apply() -> Result<u32> {
    relaunch_current_executable()
}

pub fn format_self_update_check_summary(
    localizer: &Localizer,
    report: &SelfUpdateCheckReport,
) -> String {
    let current = report.current_version.to_string();
    let latest = report.latest_version.to_string();
    if report.update_available {
        localizer
            .format(
                "self-update-status-update-available",
                &[
                    ("current", current.as_str()),
                    ("latest", latest.as_str()),
                    ("channel", report.channel.as_str()),
                ],
            )
            .value
    } else {
        localizer
            .format(
                "self-update-status-up-to-date",
                &[
                    ("current", current.as_str()),
                    ("channel", report.channel.as_str()),
                ],
            )
            .value
    }
}

pub fn format_self_update_apply_summary(
    localizer: &Localizer,
    report: &SelfUpdateApplyReport,
) -> String {
    let version = report.stage.check.latest_version.to_string();
    if report.replaced_files.is_empty() {
        return localizer
            .format(
                "self-update-apply-no-files-replaced",
                &[("version", version.as_str())],
            )
            .value;
    }

    let count = report.replaced_files.len().to_string();
    let install_root = report.install_root.display().to_string();
    let mut summary = localizer
        .format(
            "self-update-apply-replaced-summary",
            &[
                ("count", count.as_str()),
                ("root", install_root.as_str()),
                ("version", version.as_str()),
            ],
        )
        .value;
    if let Some(signature_summary) = format_signature_verdict_summary(localizer, report) {
        summary.push(' ');
        summary.push_str(&signature_summary);
    }
    summary
}

fn format_signature_verdict_summary(
    localizer: &Localizer,
    report: &SelfUpdateApplyReport,
) -> Option<String> {
    use rais_core::signature::SignatureVerdict;

    if report.signature_verdicts.is_empty() {
        return None;
    }
    let mut signed = 0usize;
    let mut unsigned = 0usize;
    for record in &report.signature_verdicts {
        match record.verdict {
            SignatureVerdict::Signed { .. } => signed += 1,
            SignatureVerdict::Unsigned { .. } => unsigned += 1,
            SignatureVerdict::Invalid { .. } => {}
        }
    }
    let signed_str = signed.to_string();
    let unsigned_str = unsigned.to_string();
    let value = match (signed, unsigned) {
        (0, 0) => return None,
        (_, 0) => {
            localizer
                .format(
                    "self-update-apply-signature-summary-signed-only",
                    &[("signed", signed_str.as_str())],
                )
                .value
        }
        (0, _) => {
            localizer
                .format(
                    "self-update-apply-signature-summary-unsigned-only",
                    &[("unsigned", unsigned_str.as_str())],
                )
                .value
        }
        _ => {
            localizer
                .format(
                    "self-update-apply-signature-summary-mixed",
                    &[
                        ("signed", signed_str.as_str()),
                        ("unsigned", unsigned_str.as_str()),
                    ],
                )
                .value
        }
    };
    Some(value)
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
        .filter(|item| matches!(item.status, PackageOperationStatus::DeferredUnattended))
        .count();

    let architecture_label = architecture_label_for_summary(model.architecture);
    let mut detail_lines = vec![
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-target",
            &[("path", report.resource_path.display().to_string())],
            format!("Target: {}", report.resource_path.display()),
        ),
        format_localized_message(
            localizer.as_ref(),
            "wizard-summary-architecture",
            &[("architecture", architecture_label.clone())],
            format!("Architecture: {architecture_label}"),
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

    let item_backup_paths = report
        .package_operation
        .items
        .iter()
        .flat_map(|item| item.backup_paths.iter())
        .collect::<Vec<_>>();
    let item_backup_manifest_paths = report
        .package_operation
        .items
        .iter()
        .filter_map(|item| item.backup_manifest_path.as_ref())
        .collect::<Vec<_>>();
    let package_receipt_backup_path = report.package_operation.receipt_backup_path.as_ref();
    let package_receipt_backup_manifest_path = report
        .package_operation
        .receipt_backup_manifest_path
        .as_ref();
    if report.package_operation.install_report.is_none()
        && (!item_backup_paths.is_empty()
            || !item_backup_manifest_paths.is_empty()
            || package_receipt_backup_path.is_some()
            || package_receipt_backup_manifest_path.is_some())
    {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-backup-files-created",
            &[(
                "count",
                (item_backup_paths.len() + usize::from(package_receipt_backup_path.is_some()))
                    .to_string(),
            )],
            format!(
                "Backup files created: {}",
                item_backup_paths.len() + usize::from(package_receipt_backup_path.is_some())
            ),
        ));
    }
    for path in item_backup_paths {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-backup-file",
            &[("path", path.display().to_string())],
            format!("Backup file: {}", path.display()),
        ));
    }
    for path in item_backup_manifest_paths {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-backup-manifest",
            &[("path", path.display().to_string())],
            format!("Backup manifest: {}", path.display()),
        ));
    }
    if let Some(path) = package_receipt_backup_path {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-receipt-backup",
            &[("path", path.display().to_string())],
            format!("Receipt backup: {}", path.display()),
        ));
    }
    if let Some(path) = package_receipt_backup_manifest_path {
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-backup-manifest",
            &[("path", path.display().to_string())],
            format!("Backup manifest: {}", path.display()),
        ));
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
        let plan_action_label = action_label_for_summary(localizer.as_ref(), item.plan_action);
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-package-plan-action",
            &[("action", plan_action_label.clone())],
            format!("  Plan action: {plan_action_label}"),
        ));
        let status_label = status_label_for_summary(localizer.as_ref(), item.status);
        detail_lines.push(format_localized_message(
            localizer.as_ref(),
            "wizard-summary-package-status",
            &[("status", status_label.clone())],
            format!("  Status: {status_label}"),
        ));
        // Surface the installed version so users can confirm the install
        // landed without having to scroll through the install report. The
        // artifact descriptor's version is what RAIS chose to install (and,
        // for an InstalledOrChecked status, what now lives on disk per the
        // receipt the operation pipeline just wrote).
        if matches!(
            item.status,
            PackageOperationStatus::InstalledOrChecked | PackageOperationStatus::SkippedCurrent
        ) {
            let installed_version = item.artifact.version.to_string();
            detail_lines.push(format_localized_message(
                localizer.as_ref(),
                "wizard-summary-package-installed-version",
                &[("version", installed_version.clone())],
                format!("  Installed version: {installed_version}"),
            ));
        }
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
        PlannedExecutionKind::ExtractArchiveAndCopyOsaraAssets => (
            "wizard-planned-runner-extract-archive-copy-osara",
            "Extract archive and copy OSARA installer assets",
        ),
        PlannedExecutionKind::MountDiskImageAndRunInstaller => (
            "wizard-planned-runner-mount-disk-image",
            "Mount disk image and run contained installer",
        ),
        PlannedExecutionKind::MountDiskImageAndCopyAppBundle => (
            "wizard-planned-runner-mount-disk-image-copy-app",
            "Mount disk image and copy contained app bundle",
        ),
    };
    localizer
        .map(|localizer| localizer.text(id).value)
        .unwrap_or_else(|| fallback.to_string())
}

/// Localized "Install / Update / Keep" label resolver scoped to the saved
/// summary report. Mirrors the wizard's `action_label` but works against an
/// `Option<&Localizer>` so the summarizer can degrade gracefully when no
/// localizer is available.
fn action_label_for_summary(localizer: Option<&Localizer>, action: PlanActionKind) -> String {
    let (id, fallback) = match action {
        PlanActionKind::Install => ("action-install", "Install"),
        PlanActionKind::Update => ("action-update", "Update"),
        PlanActionKind::Keep => ("action-keep", "Keep"),
    };
    localizer
        .map(|localizer| localizer.text(id).value)
        .unwrap_or_else(|| fallback.to_string())
}

/// Localized status-label resolver for `PackageOperationStatus` values
/// surfaced by the saved summary report.
fn status_label_for_summary(
    localizer: Option<&Localizer>,
    status: PackageOperationStatus,
) -> String {
    let (id, fallback) = match status {
        PackageOperationStatus::InstalledOrChecked => {
            ("status-installed-or-checked", "Installed or checked")
        }
        PackageOperationStatus::PlannedUnattended => {
            ("status-planned-unattended", "Planned unattended")
        }
        PackageOperationStatus::DeferredUnattended => {
            ("status-deferred-unattended", "Deferred unattended")
        }
        PackageOperationStatus::SkippedCurrent => {
            ("status-skipped-current", "Skipped (already current)")
        }
    };
    localizer
        .map(|localizer| localizer.text(id).value)
        .unwrap_or_else(|| fallback.to_string())
}

/// Format the wizard's detected architecture as a stable short token used in
/// the summary report. Not localized — these are the same identifiers the
/// CLI's `rais detect` output uses, so external tooling can grep for them.
fn architecture_label_for_summary(architecture: Architecture) -> String {
    match architecture {
        Architecture::X86 => "x86".to_string(),
        Architecture::X64 => "x64".to_string(),
        Architecture::Arm64 => "arm64".to_string(),
        Architecture::Arm64Ec => "arm64ec".to_string(),
        Architecture::Universal => "universal".to_string(),
        Architecture::Unknown => "unknown".to_string(),
    }
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
            let spec = specs_by_id.get(action.package_id.as_str()).copied();
            let display_name = spec
                .map(|spec| localizer.text(&spec.display_name_key).value)
                .unwrap_or_else(|| action.package_id.clone());
            let description = spec
                .map(|spec| localizer.text(&spec.display_description_key))
                .filter(|text| !text.missing)
                .map(|text| text.value)
                .unwrap_or_default();
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
            // Compose the details text shown in the wizard's package
            // details pane. The localized description follows the summary
            // line so users can see what a package is before deciding
            // what to do with it. The plan-reason string ("Installed
            // version is current or newer…") and the handling-summary /
            // automation-kind detail are not localized for end users —
            // both stay on PackageRow as structured fields for the saved
            // report and stay out of the wizard pane.
            let details = if description.is_empty() {
                summary.clone()
            } else {
                format!("{summary}\n\n{description}")
            };
            PackageRow {
                package_id: action.package_id.clone(),
                summary: summary.clone(),
                details,
                display_name: display_name.clone(),
                description,
                selected: matches!(
                    action.action,
                    PlanActionKind::Install | PlanActionKind::Update
                ),
                installed_version,
                available_version,
                action: action.action,
                action_label,
                original_action: action.action,
                reason: action.reason.clone(),
                handling_summary,
                manual_attention_expected,
                available_for_target: true,
                unavailability_reason: None,
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

fn unknown_version_text(model: &WizardModel) -> String {
    localizer_from_options(&model.bootstrap_options)
        .map(|localizer| localizer.text("detect-version-unknown").value)
        .unwrap_or_else(|_| "Version unknown".to_string())
}

fn action_label(localizer: &Localizer, action: PlanActionKind) -> String {
    let key = match action {
        PlanActionKind::Install => "action-install",
        PlanActionKind::Update => "action-update",
        PlanActionKind::Keep => "action-keep",
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

    use rais_core::artifact::{ArtifactDescriptor, ArtifactKind};
    use rais_core::install::{InstallFileAction, InstallFileReport, InstallReport};
    use rais_core::localization::{DEFAULT_LOCALE, Localizer};
    use rais_core::model::{Architecture, Confidence, Installation, InstallationKind, Platform};
    use rais_core::operation::{
        ManualInstallInstruction, PackageOperationItem, PackageOperationReport,
        PackageOperationStatus, PlannedExecutionKind, PlannedExecutionPlan,
    };
    use rais_core::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS};
    use rais_core::plan::{InstallPlan, PlanAction, PlanActionKind};
    use rais_core::preflight::PreflightReport;
    use rais_core::resource::{
        ResourceInitAction, ResourceInitActionKind, ResourceInitItemKind, ResourceInitReport,
    };
    use rais_core::setup::SetupReport;
    use rais_core::version::Version;
    use tempfile::tempdir;

    use super::{
        HostCapabilities, OsaraKeymapChoice, PackageRow, UiBootstrapOptions, WizardInstallRequest,
        custom_portable_target_row, format_self_update_apply_summary, localizer_from_options,
        model_from_plan, refreshed_target_row, wizard_desired_package_ids_for_host,
    };

    #[test]
    fn jaws_scripts_only_appear_when_jaws_is_detected() {
        let with_jaws = wizard_desired_package_ids_for_host(
            Platform::Windows,
            &HostCapabilities {
                jaws_installed: true,
            },
        );
        assert!(with_jaws.iter().any(|id| id == "jaws-scripts"));

        let without_jaws = wizard_desired_package_ids_for_host(
            Platform::Windows,
            &HostCapabilities {
                jaws_installed: false,
            },
        );
        assert!(!without_jaws.iter().any(|id| id == "jaws-scripts"));

        // macOS never sees the JAWS row regardless of the host flag — the
        // package itself is platform-gated to Windows.
        let macos_with_jaws = wizard_desired_package_ids_for_host(
            Platform::MacOs,
            &HostCapabilities {
                jaws_installed: true,
            },
        );
        assert!(!macos_with_jaws.iter().any(|id| id == "jaws-scripts"));
    }

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
        assert_eq!(model.steps.len(), 7);
        assert_eq!(model.target_rows.len(), 1);
        assert!(model.target_rows[0].selected);
        assert!(model.target_rows[0].portable);
        assert!(model.target_rows[0].details.contains("REAPER application"));
        assert!(model.target_rows[0].details.contains("REAPER version"));
        assert!(!model.target_rows[0].details.contains("Architecture"));
        assert!(
            !model.target_rows[0]
                .details
                .contains("Detection confidence")
        );
        assert!(model.target_rows[0].details.contains("Writable"));
        assert_eq!(model.package_rows.len(), 2);
        assert_eq!(model.package_rows[0].display_name, "OSARA");
        assert!(model.package_rows[0].summary.contains("OSARA"));
        // The wizard package details pane no longer surfaces the internal
        // handling-kind line; it stays in the saved report instead.
        assert!(!model.package_rows[0].details.contains("Handling:"));
        // The localized package description from the embedded en-US locale
        // should land in `description` and inside `details` so the wizard's
        // package details pane explains what the package is for.
        assert!(
            model.package_rows[0].description.contains("screen readers"),
            "expected OSARA description in row, got {:?}",
            model.package_rows[0].description
        );
        assert!(
            model.package_rows[0]
                .details
                .contains(&model.package_rows[0].description),
            "expected OSARA description embedded in details"
        );
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
    fn toggling_a_package_row_updates_its_action_label_and_summary() {
        // The package list row label must follow the user's checkbox state:
        // unchecking should switch the visible action to "Keep", and
        // re-checking it should restore the install/update action that the
        // plan originally chose for this package.
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
        let mut row = model.package_rows[0].clone();
        assert_eq!(row.action, PlanActionKind::Install);
        assert_eq!(row.action_label, "Install");
        assert!(row.selected);

        let summary = super::apply_checkbox_state_to_package_row(&model, &mut row, false).unwrap();
        assert_eq!(row.action, PlanActionKind::Keep);
        assert_eq!(row.action_label, "Keep");
        assert!(!row.selected);
        assert!(summary.contains("Keep"));
        assert!(row.summary.contains("Keep"));

        // Re-checking restores the original install action because the
        // package was originally not installed.
        let summary = super::apply_checkbox_state_to_package_row(&model, &mut row, true).unwrap();
        assert_eq!(row.action, PlanActionKind::Install);
        assert_eq!(row.action_label, "Install");
        assert!(row.selected);
        assert!(summary.contains("Install"));
    }

    #[test]
    fn toggling_a_keep_row_to_checked_promotes_it_to_update() {
        // For a package that was already installed and current, the plan's
        // original action is Keep. If the user explicitly checks the row,
        // they're asking RAIS to re-stage the package — that translates to
        // Update so the install pipeline runs.
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
                    package_id: PACKAGE_REAPACK.to_string(),
                    action: PlanActionKind::Keep,
                    installed_version: Some(Version::parse("1.2.6").unwrap()),
                    available_version: Some(Version::parse("1.2.6").unwrap()),
                    reason: "Current".to_string(),
                }],
                notes: Vec::new(),
            },
        );
        let mut row = model.package_rows[0].clone();
        assert_eq!(row.action, PlanActionKind::Keep);
        assert!(!row.selected);

        let _ = super::apply_checkbox_state_to_package_row(&model, &mut row, true).unwrap();
        assert_eq!(row.action, PlanActionKind::Update);
        assert_eq!(row.action_label, "Update");
        assert!(row.selected);
        assert!(row.summary.contains("Update"));
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
        assert!(!row.details.contains("Architecture"));
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
        // Handling-kind no longer surfaces in the wizard details pane; it
        // remains as a structured field on PackageRow for the saved report.
        assert_eq!(
            reaper.handling_summary,
            model.text.package_handling_unattended
        );
        assert!(reaper.selected);
    }

    #[cfg(target_os = "windows")]
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
    fn sws_windows_uses_unattended_handling_summary() {
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

        let (handling_summary, manual_attention_expected) = super::package_handling_summary(
            &model.text,
            PACKAGE_SWS,
            Platform::Windows,
            Architecture::X64,
        );

        assert_eq!(handling_summary, model.text.package_handling_unattended);
        assert!(!manual_attention_expected);
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
        assert!(
            !preview
                .lines
                .iter()
                .any(|line| line == "Manual attention expected")
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
    fn default_install_options_replace_osara_keymap() {
        assert_eq!(
            super::WizardInstallOptions::default().osara_keymap_choice,
            OsaraKeymapChoice::ReplaceCurrent
        );
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
                receipt_backup_path: None,
                receipt_backup_manifest_path: None,
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
                    backup_paths: Vec::new(),
                    backup_manifest_path: None,
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
                        requires_elevation: false,
                        freshness_paths: Vec::new(),
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
        // Architecture line + per-package plan action / status are now part
        // of the saved report so power users have everything the wizard hides.
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Architecture:") && line.contains("x64"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Plan action:") && line.contains("Install"))
        );
        assert!(
            summary
                .detail_lines
                .iter()
                .any(|line| line.contains("Status:") && line.contains("Deferred unattended"))
        );
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
                receipt_backup_path: None,
                receipt_backup_manifest_path: None,
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
                    backup_paths: Vec::new(),
                    backup_manifest_path: None,
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
    fn setup_summary_includes_unattended_receipt_backup_paths() {
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
                install_report: None,
                receipt_backup_path: Some(PathBuf::from(
                    "C:/PortableREAPER/RAIS/backups/unattended-1/RAIS/install-state.json",
                )),
                receipt_backup_manifest_path: Some(PathBuf::from(
                    "C:/PortableREAPER/RAIS/backups/unattended-1/backup-manifest.json",
                )),
                items: vec![PackageOperationItem {
                    package_id: PACKAGE_OSARA.to_string(),
                    plan_action: PlanActionKind::Install,
                    status: PackageOperationStatus::InstalledOrChecked,
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
                    backup_paths: Vec::new(),
                    backup_manifest_path: None,
                    planned_execution: None,
                    manual_instruction: None,
                    message: "RAIS ran the upstream installer unattended, verified the expected target paths, and updated the RAIS receipt.".to_string(),
                }],
            },
        };

        let summary = super::summarize_setup_report(&model, &report);

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

    #[test]
    fn apply_summary_appends_signed_count_when_signatures_were_verified() {
        use rais_core::self_update::{ReplacedFile, SignatureVerdictRecord};
        use rais_core::signature::SignatureVerdict;

        let report = sample_apply_report(
            vec![ReplacedFile {
                install_path: PathBuf::from("/install/RAIS"),
                backup_path: PathBuf::from("/install/RAIS.rais-old"),
            }],
            vec![SignatureVerdictRecord {
                source_path: PathBuf::from("/staging/RAIS"),
                verdict: SignatureVerdict::Signed {
                    details: "valid on disk".to_string(),
                },
            }],
        );

        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let summary = format_self_update_apply_summary(&localizer, &report);
        assert!(summary.contains("Replaced 1 file(s)"));
        assert!(summary.contains("Signature verification: 1 signed."));
    }

    #[test]
    fn apply_summary_omits_signature_clause_when_no_verdicts_recorded() {
        use rais_core::self_update::ReplacedFile;

        let report = sample_apply_report(
            vec![ReplacedFile {
                install_path: PathBuf::from("/install/RAIS"),
                backup_path: PathBuf::from("/install/RAIS.rais-old"),
            }],
            Vec::new(),
        );

        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let summary = format_self_update_apply_summary(&localizer, &report);
        assert!(summary.contains("Replaced 1 file(s)"));
        assert!(!summary.contains("Signature verification"));
    }

    #[test]
    fn apply_summary_reports_signed_and_unsigned_split() {
        use rais_core::self_update::{ReplacedFile, SignatureVerdictRecord};
        use rais_core::signature::SignatureVerdict;

        let report = sample_apply_report(
            vec![
                ReplacedFile {
                    install_path: PathBuf::from("/install/RAIS"),
                    backup_path: PathBuf::from("/install/RAIS.rais-old"),
                },
                ReplacedFile {
                    install_path: PathBuf::from("/install/rais-cli"),
                    backup_path: PathBuf::from("/install/rais-cli.rais-old"),
                },
            ],
            vec![
                SignatureVerdictRecord {
                    source_path: PathBuf::from("/staging/RAIS"),
                    verdict: SignatureVerdict::Signed {
                        details: "ok".to_string(),
                    },
                },
                SignatureVerdictRecord {
                    source_path: PathBuf::from("/staging/rais-cli"),
                    verdict: SignatureVerdict::Unsigned {
                        reason: "no signtool".to_string(),
                    },
                },
            ],
        );

        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();
        let summary = format_self_update_apply_summary(&localizer, &report);
        assert!(summary.contains("Signature verification: 1 signed, 1 unsigned."));
    }

    fn sample_apply_report(
        replaced_files: Vec<rais_core::self_update::ReplacedFile>,
        signature_verdicts: Vec<rais_core::self_update::SignatureVerdictRecord>,
    ) -> rais_core::self_update::SelfUpdateApplyReport {
        use rais_core::model::Platform;
        use rais_core::self_update::{
            SelfUpdateApplyReport, SelfUpdateAssetSelection, SelfUpdateCheckReport,
            SelfUpdateStageReport,
        };

        let check = SelfUpdateCheckReport {
            manifest_url: "https://example.test/rais-update-stable.json".to_string(),
            current_version: Version::parse("0.1.0").unwrap(),
            latest_version: Version::parse("0.2.0").unwrap(),
            channel: "stable".to_string(),
            published_at: "2026-04-25T00:00:00Z".to_string(),
            release_notes_url: None,
            minimum_supported_previous_version: None,
            update_available: true,
            requires_manual_transition: false,
            asset: SelfUpdateAssetSelection {
                platform: Platform::Windows,
                url: "https://example.test/RAIS-windows.zip".to_string(),
                sha256: "0".repeat(64),
            },
        };
        let stage = SelfUpdateStageReport {
            check,
            staging_dir: PathBuf::from("/staging"),
            staged_asset_path: Some(PathBuf::from("/staging/0.2.0/RAIS-windows.zip")),
            downloaded: true,
            reused_existing_file: false,
            verified_sha256: Some("0".repeat(64)),
            ready_to_apply: true,
            status_message: "ready".to_string(),
        };
        SelfUpdateApplyReport {
            stage,
            install_root: PathBuf::from("/install"),
            replaced_files,
            skipped_files: Vec::new(),
            signature_verdicts,
            status_message: "applied".to_string(),
        }
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
                receipt_backup_path: None,
                receipt_backup_manifest_path: None,
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
            force_reinstall_packages: Vec::new(),
        }
    }
}
