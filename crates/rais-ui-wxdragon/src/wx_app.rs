use std::cell::{Cell, RefCell};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use rais_core::localization::{Localizer, resolve_runtime_locale};
use rais_core::lock::PackageInstallLockMetadata;
use rais_core::self_update::SelfUpdateCheckReport;

// FluentBundle is !Send, so we keep one Localizer instance per UI thread and
// have call_after bodies read it from this thread-local rather than capturing
// it through worker threads. The wxdragon event loop runs every call_after
// body on the same thread that initialised UI_LOCALIZER (the main thread).
thread_local! {
    static UI_LOCALIZER: RefCell<Option<Rc<Localizer>>> = const { RefCell::new(None) };
    /// Post-install rescan hook. The install click handler arms this with a
    /// closure that captures the UI-thread `Rc<RefCell>` shared state for
    /// `package_rows`/`package_notes`/`can_install`. The wizard install
    /// runs on a worker thread; its `call_after` success branch fires the
    /// hook so the cached package state reflects what the just-completed
    /// install left on disk. Lives in a thread-local because the
    /// `Rc<RefCell>` it captures is `!Send` and can't ride inside the
    /// `call_after` `Box<dyn FnOnce + Send>`.
    static POST_INSTALL_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

fn install_ui_localizer(localizer: Localizer) {
    UI_LOCALIZER.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(localizer));
    });
}

fn with_ui_localizer<F: FnOnce(&Localizer)>(f: F) {
    UI_LOCALIZER.with(|cell| {
        if let Some(localizer) = cell.borrow().as_ref() {
            f(localizer);
        }
    });
}

fn arm_post_install_hook(callback: impl FnOnce() + 'static) {
    POST_INSTALL_HOOK.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(callback));
    });
}

fn fire_post_install_hook() {
    let callback = POST_INSTALL_HOOK.with(|cell| cell.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

/// Stages of the deferred latest-version fetch the wizard runs once the user
/// transitions Target → Packages.
enum VersionCheckEvent {
    /// "Checking <package>…" — emitted before each fetch starts.
    Checking { package_id: String },
    /// Per-package outcome: a fetched version, or an error message.
    Result {
        package_id: String,
        outcome: std::result::Result<String, String>,
    },
    /// Worker has finished iterating all packages — the UI should rebuild the
    /// package list with the fetched data and re-enable interaction.
    Finished,
}

/// Dispatcher set up by the Target → Packages click handler so the
/// version-check worker's `call_after` posts can mutate UI-thread-only state
/// (Rc-based package_rows, package_notes, can_install) without violating Send.
type VersionCheckDispatcher = Box<dyn FnMut(VersionCheckEvent)>;

thread_local! {
    static VERSION_CHECK_DISPATCHER: RefCell<Option<VersionCheckDispatcher>> =
        const { RefCell::new(None) };
}

fn install_version_check_dispatcher(dispatcher: VersionCheckDispatcher) {
    VERSION_CHECK_DISPATCHER.with(|cell| {
        *cell.borrow_mut() = Some(dispatcher);
    });
}

fn dispatch_version_check_event(event: VersionCheckEvent) {
    VERSION_CHECK_DISPATCHER.with(|cell| {
        if let Some(dispatcher) = cell.borrow_mut().as_mut() {
            dispatcher(event);
        }
    });
}
use crate::{
    OsaraKeymapChoice, PackageRow, TargetRow, UiBootstrapOptions, WizardInstallOptions,
    WizardModel, WizardOutcomeReport, apply_checkbox_state_to_package_row,
    build_review_preview_for_package_rows, custom_portable_target_row, execute_wizard_install,
    format_package_install_lock_blocking_message, format_self_update_apply_summary,
    format_self_update_check_summary, install_request_from_target_and_rows, load_wizard_model,
    localized_package_display_name, localizer_from_options, osara_keymap_note,
    osara_selected_for_rows, reapack_selected_for_install_or_update, refreshed_target_row,
    relaunch_rais_after_apply, run_wizard_package_install_lock_check, run_wizard_self_update_apply,
    run_wizard_self_update_check, save_wizard_outcome_report, wizard_desired_package_ids,
    wizard_outcome_report_from_error, wizard_outcome_report_from_success,
    wizard_package_plan_for_target, wizard_package_plan_for_target_with_available,
};
use rais_core::latest::fetch_latest_for_package;
use rais_core::plan::AvailablePackage;
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

const TARGET_STEP: usize = 0;
const VERSION_CHECK_STEP: usize = 1;
const PACKAGES_STEP: usize = 2;
const REAPACK_ACK_STEP: usize = 3;
const REVIEW_STEP: usize = 4;
const PROGRESS_STEP: usize = 5;
const DONE_STEP: usize = 6;

#[derive(Default)]
struct SelfUpdateUiState {
    /// Result of the one-shot manifest check at startup. `None` while the
    /// startup probe is still running; `Some(Ok)` on success; `Some(Err)`
    /// carries the formatted error message (RaisError isn't Clone).
    check: Option<std::result::Result<SelfUpdateCheckReport, String>>,
    /// Most recent lock-state observation, refreshed by the polling thread.
    lock_holder: Option<PackageInstallLockMetadata>,
    /// Last status string written to the status bar — used to suppress
    /// screen-reader re-announcements when nothing has changed.
    last_status: String,
    /// Last apply-button enable state — same de-dup intent.
    last_apply_enabled: bool,
}

fn render_self_update_status(
    widgets: WizardWidgets,
    model: &Arc<WizardModel>,
    localizer: &Localizer,
    state: &Arc<Mutex<SelfUpdateUiState>>,
) {
    let mut state = state.lock().unwrap();
    let Some(check) = state.check.as_ref() else {
        // Startup probe hasn't completed yet; leave the initial
        // "Checking for RAIS updates…" placeholder in place.
        return;
    };

    let mut status = match check {
        Ok(report) => format_self_update_check_summary(localizer, report),
        Err(error) => format!("{}: {}", model.text.done_self_update_error_prefix, error),
    };
    let mut apply_enabled = matches!(check, Ok(report) if report.update_available);
    if let Some(holder) = &state.lock_holder {
        if !status.is_empty() {
            status.push(' ');
        }
        status.push_str(&format_package_install_lock_blocking_message(
            localizer, holder,
        ));
        apply_enabled = false;
    }

    let status_changed = status != state.last_status;
    let enable_changed = apply_enabled != state.last_apply_enabled;
    if status_changed {
        widgets.self_update_status.set_status_text(&status, 0);
        state.last_status = status;
    }
    if enable_changed {
        widgets.done_self_update_apply.enable(apply_enabled);
        state.last_apply_enabled = apply_enabled;
    }
}

/// `wx/defs.h`: `WXK_SPACE = 32` (just the ASCII value). The default toggle
/// key on a focused wxCheckListBox row.
const WXK_SPACE: i32 = 32;

#[derive(Clone, Copy)]
struct WizardWidgets {
    target_choice: Choice,
    portable_folder: DirPickerCtrl,
    target_details: TextCtrl,
    version_check_status: StaticText,
    version_check_gauge: Gauge,
    version_check_error_heading: StaticText,
    version_check_error_log: TextCtrl,
    package_checklist: CheckListBox,
    package_details: TextCtrl,
    osara_keymap_replace: CheckBox,
    osara_keymap_note: TextCtrl,
    reapack_ack_confirm: CheckBox,
    review_text: TextCtrl,
    progress_status: StaticText,
    progress_gauge: Gauge,
    progress_details: TextCtrl,
    done_status: TextCtrl,
    done_details: TextCtrl,
    done_launch_reaper: Button,
    done_open_resource: Button,
    done_rescan: Button,
    done_save_report: Button,
    done_self_update_apply: Button,
    self_update_status: StatusBar,
}

pub fn run() {
    let _ = wxdragon::main(|_| {
        let bootstrap = UiBootstrapOptions {
            locale: resolve_runtime_locale(),
            online_versions: false,
            ..UiBootstrapOptions::default()
        };
        match localizer_from_options(&bootstrap) {
            Ok(localizer) => install_ui_localizer(localizer),
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        }
        let model = match load_wizard_model(bootstrap) {
            Ok(model) => model,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };

        let frame = Frame::builder()
            .with_title(&model.window_title)
            .with_size(Size::new(820, 600))
            .build();
        frame.set_name("rais-main-window");

        let root_panel = Panel::builder(&frame).build();
        root_panel.set_name("rais-root-panel");

        let root = BoxSizer::builder(Orientation::Vertical).build();
        let step_label = StaticText::builder(&root_panel)
            .with_label(&step_status(&model, TARGET_STEP))
            .build();
        step_label.set_name("rais-step-status");
        root.add(&step_label, 0, SizerFlag::All | SizerFlag::Expand, 12);

        // Use the frame's wxStatusBar for self-update status. NVDA's "Report
        // status bar" command (NVDA+End) reads exactly this control, JAWS
        // exposes it via its status-bar review keys, and Narrator/UIA expose
        // the StatusBar role natively. Updating via SetStatusText fires the
        // platform notifications that screen readers auto-announce.
        let self_update_status = frame.create_status_bar(1, 0, 0, "rais-self-update-status");
        self_update_status.set_status_text(&model.text.self_update_status_checking, 0);

        let book = SimpleBook::builder(&root_panel).build();
        book.set_name("rais-wizard-pages");
        let package_rows = Rc::new(RefCell::new(model.package_rows.clone()));
        let package_notes = Rc::new(RefCell::new(model.notes.clone()));
        let can_install = Rc::new(Cell::new(model.controls.can_install));
        let review_can_install = Rc::new(Cell::new(false));
        let last_report = Arc::new(Mutex::new(None::<WizardOutcomeReport>));
        let last_reaper_app_path = Arc::new(Mutex::new(None::<PathBuf>));
        let last_resource_path = Arc::new(Mutex::new(None::<PathBuf>));
        let wizard_widgets = add_pages(&book, &model, Rc::clone(&package_rows), self_update_status);
        root.add(&book, 1, SizerFlag::All | SizerFlag::Expand, 12);

        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);

        let back = Button::builder(&root_panel)
            .with_label(&model.controls.back_label)
            .build();
        back.set_name("rais-back-button");
        back.add_style(WindowStyle::TabStop);
        back.set_can_focus(true);
        buttons.add(&back, 0, SizerFlag::All, 6);

        let next = Button::builder(&root_panel)
            .with_label(&model.controls.next_label)
            .build();
        next.set_name("rais-next-button");
        next.add_style(WindowStyle::TabStop);
        next.set_can_focus(true);
        buttons.add(&next, 0, SizerFlag::All, 6);

        let install = Button::builder(&root_panel)
            .with_label(&model.controls.install_label)
            .build();
        install.set_name("rais-install-button");
        install.add_style(WindowStyle::TabStop);
        install.set_can_focus(true);
        buttons.add(&install, 0, SizerFlag::All, 6);

        let close = Button::builder(&root_panel)
            .with_label(&model.controls.close_label)
            .build();
        close.set_name("rais-close-button");
        close.add_style(WindowStyle::TabStop);
        close.set_can_focus(true);
        buttons.add(&close, 0, SizerFlag::All, 6);

        root.add_sizer(&buttons, 0, SizerFlag::All | SizerFlag::Expand, 6);

        build_language_footer(&root_panel, &root, &model);

        root_panel.set_sizer(root, true);

        let frame_sizer = BoxSizer::builder(Orientation::Vertical).build();
        frame_sizer.add(&root_panel, 1, SizerFlag::Expand, 0);
        frame.set_sizer(frame_sizer, true);

        let current_step = Arc::new(AtomicUsize::new(TARGET_STEP));
        let labels = Arc::new(
            (TARGET_STEP..=DONE_STEP)
                .map(|step| step_status(&model, step))
                .collect::<Vec<_>>(),
        );
        let model = Arc::new(model);

        update_navigation(
            TARGET_STEP,
            &book,
            &step_label,
            labels.as_slice(),
            &back,
            &next,
            &install,
            effective_can_install(&can_install, &review_can_install),
            target_is_valid(&model, &wizard_widgets),
            reapack_ack_confirmed(&wizard_widgets),
        );
        bind_target_navigation_updates(&model, wizard_widgets, &current_step, &next);
        bind_reapack_ack_navigation_updates(wizard_widgets, &current_step, &next);

        {
            let book = book;
            let step_label = step_label;
            let back = back;
            let next = next;
            let install = install;
            let current_step = Arc::clone(&current_step);
            let labels = Arc::clone(&labels);
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let can_install = Rc::clone(&can_install);
            let review_can_install = Rc::clone(&review_can_install);
            let back_package_rows = Rc::clone(&package_rows);
            back.on_click(move |_| {
                // Custom Back routing:
                // - PACKAGES_STEP → TARGET_STEP (skip version check; re-running
                //   the fetch from a Back press isn't what the user asked for).
                // - REAPACK_ACK_STEP → PACKAGES_STEP and clear the
                //   acknowledgement (going back resets the explicit consent).
                // - REVIEW_STEP → REAPACK_ACK_STEP if ReaPack is in the
                //   currently-selected plan; otherwise PACKAGES_STEP, again to
                //   skip the now-irrelevant ack page.
                let current = current_step.load(Ordering::SeqCst);
                let step = match current {
                    PACKAGES_STEP => TARGET_STEP,
                    REAPACK_ACK_STEP => {
                        widgets.reapack_ack_confirm.set_value(false);
                        PACKAGES_STEP
                    }
                    REVIEW_STEP => {
                        let rows = back_package_rows.borrow();
                        let checked = checked_package_indices(&widgets.package_checklist);
                        if reapack_selected_for_install_or_update(&rows, &checked) {
                            REAPACK_ACK_STEP
                        } else {
                            PACKAGES_STEP
                        }
                    }
                    other => other.saturating_sub(1),
                };
                current_step.store(step, Ordering::SeqCst);
                update_navigation(
                    step,
                    &book,
                    &step_label,
                    labels.as_slice(),
                    &back,
                    &next,
                    &install,
                    effective_can_install(&can_install, &review_can_install),
                    target_is_valid(&model, &widgets),
                    reapack_ack_confirmed(&widgets),
                );
            });
        }

        {
            let book = book;
            let step_label = step_label;
            let back = back;
            let next = next;
            let install = install;
            let current_step = Arc::clone(&current_step);
            let labels = Arc::clone(&labels);
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let package_rows = Rc::clone(&package_rows);
            let package_notes = Rc::clone(&package_notes);
            let can_install = Rc::clone(&can_install);
            let review_can_install = Rc::clone(&review_can_install);
            next.on_click(move |_| {
                let step = match current_step.load(Ordering::SeqCst) {
                    TARGET_STEP => {
                        let Some(selected_target) = selected_target_row(&model, &widgets) else {
                            return;
                        };
                        match wizard_package_plan_for_target(&model, Some(&selected_target)) {
                            Ok(plan) => {
                                *package_rows.borrow_mut() = plan.package_rows;
                                *package_notes.borrow_mut() = plan.notes;
                                can_install.set(plan.can_install);
                                review_can_install.set(false);
                                refresh_package_checklist(
                                    &widgets.package_checklist,
                                    &widgets.package_details,
                                    &widgets.osara_keymap_replace,
                                    &widgets.osara_keymap_note,
                                    &model,
                                    &package_rows.borrow(),
                                );
                                start_version_check(VersionCheckUi {
                                    widgets,
                                    model: Arc::clone(&model),
                                    package_rows: Rc::clone(&package_rows),
                                    package_notes: Rc::clone(&package_notes),
                                    can_install: Rc::clone(&can_install),
                                    review_can_install: Rc::clone(&review_can_install),
                                    target: selected_target,
                                    book: book.clone(),
                                    step_label: step_label.clone(),
                                    labels: Arc::clone(&labels),
                                    back: back.clone(),
                                    next: next.clone(),
                                    install: install.clone(),
                                    current_step: Arc::clone(&current_step),
                                });
                                VERSION_CHECK_STEP
                            }
                            Err(error) => {
                                widgets.target_details.set_value(&error.to_string());
                                TARGET_STEP
                            }
                        }
                    }
                    PACKAGES_STEP => {
                        let selected_target = selected_target_row(&model, &widgets);
                        let rows = package_rows.borrow();
                        let notes = package_notes.borrow();
                        let checked = checked_package_indices(&widgets.package_checklist);
                        let review_preview = build_review_preview_for_package_rows(
                            &model,
                            selected_target.as_ref(),
                            &checked,
                            &rows,
                            &notes,
                            osara_keymap_choice(&widgets.osara_keymap_replace),
                        );
                        review_can_install.set(review_preview.can_install);
                        widgets
                            .review_text
                            .set_value(&review_preview.lines.join("\n"));
                        // Route through the ReaPack donation acknowledgement
                        // page when the user has ReaPack in the install/update
                        // plan; everyone else goes straight to Review.
                        if reapack_selected_for_install_or_update(&rows, &checked) {
                            REAPACK_ACK_STEP
                        } else {
                            REVIEW_STEP
                        }
                    }
                    REAPACK_ACK_STEP => REVIEW_STEP,
                    PROGRESS_STEP => DONE_STEP,
                    other => other,
                };
                current_step.store(step, Ordering::SeqCst);
                update_navigation(
                    step,
                    &book,
                    &step_label,
                    labels.as_slice(),
                    &back,
                    &next,
                    &install,
                    effective_can_install(&can_install, &review_can_install),
                    target_is_valid(&model, &widgets),
                    reapack_ack_confirmed(&widgets),
                );
                if step == VERSION_CHECK_STEP {
                    // Pull the screen reader onto the progress bar so the
                    // user hears that a check is running. Without this,
                    // focus would stay on the Next button from the Target
                    // page and the version-check progress wouldn't be
                    // announced until the auto-advance to Packages fires.
                    widgets.version_check_gauge.set_focus();
                }
            });
        }

        {
            let book = book;
            let step_label = step_label;
            let back = back;
            let next = next;
            let install = install;
            let current_step = Arc::clone(&current_step);
            let labels = Arc::clone(&labels);
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let package_rows = Rc::clone(&package_rows);
            let package_notes = Rc::clone(&package_notes);
            let can_install = Rc::clone(&can_install);
            let review_can_install = Rc::clone(&review_can_install);
            let last_report = Arc::clone(&last_report);
            let last_reaper_app_path = Arc::clone(&last_reaper_app_path);
            let last_resource_path = Arc::clone(&last_resource_path);
            install.on_click(move |_| {
                current_step.store(PROGRESS_STEP, Ordering::SeqCst);
                update_navigation(
                    PROGRESS_STEP,
                    &book,
                    &step_label,
                    labels.as_slice(),
                    &back,
                    &next,
                    &install,
                    effective_can_install(&can_install, &review_can_install),
                    target_is_valid(&model, &widgets),
                    reapack_ack_confirmed(&widgets),
                );
                back.enable(false);
                next.enable(false);
                install.enable(false);
                widgets.done_launch_reaper.enable(false);
                widgets.done_open_resource.enable(false);
                widgets.done_rescan.enable(false);
                widgets.done_save_report.enable(false);
                widgets
                    .progress_status
                    .set_label(&model.text.progress_status_running);
                widgets.progress_gauge.set_value(10);
                set_last_report(&last_report, None);

                let selected_target = selected_target_row(&model, &widgets);
                set_last_path(
                    &last_reaper_app_path,
                    selected_target
                        .as_ref()
                        .map(planned_reaper_launch_path_for_target),
                );
                set_last_resource_path(
                    &last_resource_path,
                    selected_target.as_ref().map(|target| target.path.clone()),
                );
                let selected_packages = checked_package_indices(&widgets.package_checklist);
                let rows = package_rows.borrow();
                widgets
                    .progress_details
                    .set_value(&progress_details_for_start(
                        &model,
                        selected_target.as_ref(),
                        &selected_packages,
                        &rows,
                        osara_keymap_choice(&widgets.osara_keymap_replace),
                        None,
                    ));
                let request = match selected_target
                    .as_ref()
                    .ok_or_else(|| rais_core::RaisError::PreflightFailed {
                        message: model.text.review_no_target.clone(),
                    })
                    .and_then(|target| {
                        install_request_from_target_and_rows(
                            &model,
                            target,
                            &rows,
                            &selected_packages,
                            WizardInstallOptions {
                                osara_keymap_choice: osara_keymap_choice(
                                    &widgets.osara_keymap_replace,
                                ),
                                ..WizardInstallOptions::default()
                            },
                        )
                    }) {
                    Ok(request) => request,
                    Err(error) => {
                        widgets.progress_gauge.set_value(100);
                        widgets
                            .progress_status
                            .set_label(&model.text.done_status_error);
                        // Done page: short reason on the always-visible
                        // status TextCtrl; full error text in the
                        // collapsible details below.
                        widgets.done_status.set_value(&model.text.done_status_error);
                        widgets.done_details.set_value(&error.to_string());
                        widgets
                            .progress_details
                            .set_value(&format!("{}\n\n{}", model.text.done_status_error, error));
                        widgets
                            .done_open_resource
                            .enable(clone_last_resource_path(&last_resource_path).is_some());
                        widgets
                            .done_launch_reaper
                            .enable(can_launch_last_reaper_path(&last_reaper_app_path));
                        widgets.done_save_report.enable(false);
                        widgets.done_rescan.enable(true);
                        current_step.store(DONE_STEP, Ordering::SeqCst);
                        update_navigation(
                            DONE_STEP,
                            &book,
                            &step_label,
                            labels.as_slice(),
                            &back,
                            &next,
                            &install,
                            effective_can_install(&can_install, &review_can_install),
                            target_is_valid(&model, &widgets),
                            reapack_ack_confirmed(&widgets),
                        );
                        return;
                    }
                };
                widgets
                    .progress_details
                    .set_value(&progress_details_for_start(
                        &model,
                        selected_target.as_ref(),
                        &selected_packages,
                        &rows,
                        osara_keymap_choice(&widgets.osara_keymap_replace),
                        Some(&request.cache_dir),
                    ));
                drop(rows);

                // Arm the post-install rescan hook. The hook captures the
                // UI-thread `Rc<RefCell>` shared state so the call_after
                // success arm can refresh it without smuggling non-Send
                // references across threads. The hook closure runs on the
                // UI thread; it re-detects the selected target, runs the
                // offline package plan against the now-fresh receipts, and
                // updates both the cached state and the on-screen package
                // list — so navigating Back from the Done page (or
                // re-opening the Packages step via Rescan) shows the
                // post-install version without the user having to click
                // anything.
                {
                    let model = Arc::clone(&model);
                    let widgets = widgets;
                    let package_rows = Rc::clone(&package_rows);
                    let package_notes = Rc::clone(&package_notes);
                    let can_install = Rc::clone(&can_install);
                    let review_can_install = Rc::clone(&review_can_install);
                    let last_reaper_app_path = Arc::clone(&last_reaper_app_path);
                    let last_resource_path = Arc::clone(&last_resource_path);
                    arm_post_install_hook(move || {
                        let Some(target) = selected_target_row(&model, &widgets) else {
                            return;
                        };
                        let refreshed_target = refreshed_target_row(&model, &target);
                        let Ok(plan) =
                            wizard_package_plan_for_target(&model, Some(&refreshed_target))
                        else {
                            return;
                        };
                        *package_rows.borrow_mut() = plan.package_rows;
                        *package_notes.borrow_mut() = plan.notes;
                        can_install.set(plan.can_install);
                        review_can_install.set(false);
                        refresh_package_checklist(
                            &widgets.package_checklist,
                            &widgets.package_details,
                            &widgets.osara_keymap_replace,
                            &widgets.osara_keymap_note,
                            &model,
                            &package_rows.borrow(),
                        );
                        refresh_target_choice(
                            &model,
                            &widgets.target_choice,
                            refreshed_target_index(&model, &widgets),
                            &refreshed_target,
                        );
                        widgets.target_details.set_value(&refreshed_target.details);
                        set_last_path(
                            &last_reaper_app_path,
                            Some(planned_reaper_launch_path_for_target(&refreshed_target)),
                        );
                        set_last_resource_path(
                            &last_resource_path,
                            Some(refreshed_target.path.clone()),
                        );
                    });
                }

                let ui_model = Arc::clone(&model);
                let ui_current_step = Arc::clone(&current_step);
                let ui_labels = Arc::clone(&labels);
                let ui_last_report = Arc::clone(&last_report);
                let ui_last_reaper_app_path = Arc::clone(&last_reaper_app_path);
                let ui_last_resource_path = Arc::clone(&last_resource_path);
                let can_install = effective_can_install(&can_install, &review_can_install);
                let request_for_report = request.clone();
                std::thread::spawn(move || {
                    let result = execute_wizard_install(request);
                    wxdragon::call_after(Box::new(move || {
                        widgets.progress_gauge.set_value(100);
                        match result {
                            Ok(report) => {
                                let outcome_report = wizard_outcome_report_from_success(
                                    &ui_model,
                                    &request_for_report,
                                    &report,
                                );
                                widgets.progress_details.set_value(&format!(
                                    "{}\n\n{}",
                                    outcome_report.status_line,
                                    outcome_report.detail_lines.join("\n")
                                ));
                                set_last_resource_path(
                                    &ui_last_resource_path,
                                    Some(report.resource_path.clone()),
                                );
                                set_last_report(&ui_last_report, Some(outcome_report.clone()));
                                // Auto-save the outcome report under
                                // <resource>/RAIS/logs/ so users always have
                                // a JSON+text trail without having to
                                // remember to click "Save report". Best
                                // effort: log to stderr and continue if the
                                // save itself fails.
                                if let Err(error) = save_wizard_outcome_report(&outcome_report) {
                                    eprintln!("could not auto-save wizard outcome report: {error}");
                                }
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_success);
                                // Done page: show the success summary
                                // sentence on the status TextCtrl and the
                                // full setup-report detail block in the
                                // collapsible TextCtrl.
                                widgets.done_status.set_value(&format!(
                                    "{}\n\n{}",
                                    ui_model.text.done_status_success, outcome_report.status_line,
                                ));
                                widgets
                                    .done_details
                                    .set_value(&outcome_report.detail_lines.join("\n"));
                                set_last_path(
                                    &ui_last_reaper_app_path,
                                    request_for_report
                                        .target_app_path
                                        .as_ref()
                                        .filter(|path| path.exists())
                                        .cloned(),
                                );
                                widgets
                                    .done_launch_reaper
                                    .enable(can_launch_last_reaper_path(&ui_last_reaper_app_path));
                                widgets.done_open_resource.enable(true);
                                widgets.done_rescan.enable(true);
                                widgets.done_save_report.enable(true);
                                // Auto-rescan: the install pipeline just
                                // wrote a fresh receipt for whatever
                                // landed, and the cached package_rows
                                // still reflect pre-install state. Fire
                                // the post-install hook the click handler
                                // armed earlier so navigating back from
                                // the Done page (or via Rescan) reflects
                                // the new on-disk state without the user
                                // having to click anything.
                                fire_post_install_hook();
                            }
                            Err(error) => {
                                let outcome_report = wizard_outcome_report_from_error(
                                    &ui_model,
                                    &request_for_report,
                                    &error,
                                );
                                set_last_report(&ui_last_report, Some(outcome_report.clone()));
                                // Same auto-save policy as the success path:
                                // failure runs are exactly when a saved log
                                // helps users diagnose what went wrong.
                                if let Err(save_error) = save_wizard_outcome_report(&outcome_report)
                                {
                                    eprintln!(
                                        "could not auto-save wizard outcome report: {save_error}"
                                    );
                                }
                                widgets.progress_details.set_value(&format!(
                                    "{}\n\n{}",
                                    outcome_report.status_line,
                                    outcome_report.detail_lines.join("\n")
                                ));
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_error);
                                widgets.done_status.set_value(&outcome_report.status_line);
                                widgets
                                    .done_details
                                    .set_value(&outcome_report.detail_lines.join("\n"));
                                widgets
                                    .done_launch_reaper
                                    .enable(can_launch_last_reaper_path(&ui_last_reaper_app_path));
                                widgets.done_open_resource.enable(
                                    clone_last_resource_path(&ui_last_resource_path).is_some(),
                                );
                                widgets.done_rescan.enable(true);
                                widgets.done_save_report.enable(true);
                            }
                        }
                        ui_current_step.store(DONE_STEP, Ordering::SeqCst);
                        update_navigation(
                            DONE_STEP,
                            &book,
                            &step_label,
                            ui_labels.as_slice(),
                            &back,
                            &next,
                            &install,
                            can_install,
                            target_is_valid(&ui_model, &widgets),
                            reapack_ack_confirmed(&widgets),
                        );
                    }));
                });
            });
        }

        let frame_for_close = frame.clone();
        close.on_click(move |_| {
            frame_for_close.close(true);
        });

        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let last_reaper_app_path = Arc::clone(&last_reaper_app_path);
            widgets.done_launch_reaper.on_click(move |_| {
                let Some(app_path) = clone_last_path(&last_reaper_app_path) else {
                    append_done_status(&widgets.done_status, &model.text.done_no_reaper_app);
                    return;
                };
                if let Err(error) = launch_reaper(&app_path) {
                    append_done_status(
                        &widgets.done_status,
                        &format!("{}: {}", model.text.done_launch_reaper_error_prefix, error),
                    );
                }
            });
        }

        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let last_resource_path = Arc::clone(&last_resource_path);
            widgets.done_open_resource.on_click(move |_| {
                let Some(path) = clone_last_resource_path(&last_resource_path) else {
                    append_done_status(&widgets.done_status, &model.text.review_no_target);
                    return;
                };
                if let Err(error) = open_resource_folder(&path) {
                    append_done_status(
                        &widgets.done_status,
                        &format!("{}: {}", model.text.done_open_resource_error_prefix, error),
                    );
                }
            });
        }

        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let last_report = Arc::clone(&last_report);
            widgets.done_save_report.on_click(move |_| {
                let Some(report) = clone_last_report(&last_report) else {
                    append_done_status(&widgets.done_status, &model.text.done_no_report);
                    return;
                };
                match save_wizard_outcome_report(&report) {
                    Ok(path) => append_done_status(
                        &widgets.done_status,
                        &format!(
                            "{}: {}",
                            model.text.done_report_saved_prefix,
                            path.display()
                        ),
                    ),
                    Err(error) => append_done_status(
                        &widgets.done_status,
                        &format!("{}: {}", model.text.done_report_save_error_prefix, error),
                    ),
                }
            });
        }

        let self_update_state = Arc::new(Mutex::new(SelfUpdateUiState::default()));

        // One-shot startup probe: runs the manifest check and the lock probe,
        // stores both into the shared state, then renders.
        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let state = Arc::clone(&self_update_state);
            std::thread::spawn(move || {
                let check = run_wizard_self_update_check();
                let lock = run_wizard_package_install_lock_check().ok().flatten();
                {
                    let mut state = state.lock().unwrap();
                    state.check = Some(match check {
                        Ok(report) => Ok(report),
                        Err(error) => Err(error.to_string()),
                    });
                    state.lock_holder = lock;
                }
                let render_state = Arc::clone(&state);
                let render_model = Arc::clone(&model);
                wxdragon::call_after(Box::new(move || {
                    with_ui_localizer(|localizer| {
                        render_self_update_status(widgets, &render_model, localizer, &render_state);
                    });
                }));
            });
        }

        // Polling thread: re-checks the install lock every few seconds and
        // re-renders only when the holder changes (so screen readers do not
        // re-announce an unchanged status). This catches the case where another
        // RAIS process starts an install after our startup probe ran.
        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let state = Arc::clone(&self_update_state);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    let new_lock = run_wizard_package_install_lock_check().ok().flatten();
                    let changed = {
                        let mut state = state.lock().unwrap();
                        let changed = state.lock_holder != new_lock;
                        state.lock_holder = new_lock;
                        changed
                    };
                    if !changed {
                        continue;
                    }
                    let render_state = Arc::clone(&state);
                    let render_model = Arc::clone(&model);
                    wxdragon::call_after(Box::new(move || {
                        with_ui_localizer(|localizer| {
                            render_self_update_status(
                                widgets,
                                &render_model,
                                localizer,
                                &render_state,
                            );
                        });
                    }));
                }
            });
        }

        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            widgets.done_self_update_apply.on_click(move |_| {
                append_done_status(
                    &widgets.done_status,
                    &model.text.done_self_update_apply_running,
                );
                let model = Arc::clone(&model);
                std::thread::spawn(move || {
                    let result = run_wizard_self_update_apply();
                    wxdragon::call_after(Box::new(move || match result {
                        Ok(report) => {
                            with_ui_localizer(|localizer| {
                                append_done_status(
                                    &widgets.done_status,
                                    &format_self_update_apply_summary(localizer, &report),
                                );
                            });
                            if !report.replaced_files.is_empty() {
                                match relaunch_rais_after_apply() {
                                    Ok(pid) => append_done_status(
                                        &widgets.done_status,
                                        &format!(
                                            "{}: PID {}",
                                            model.text.done_self_update_relaunch_prefix, pid
                                        ),
                                    ),
                                    Err(error) => append_done_status(
                                        &widgets.done_status,
                                        &format!(
                                            "{}: {}",
                                            model.text.done_self_update_error_prefix, error
                                        ),
                                    ),
                                }
                            }
                        }
                        Err(error) => append_done_status(
                            &widgets.done_status,
                            &format!("{}: {}", model.text.done_self_update_error_prefix, error),
                        ),
                    }));
                });
            });
        }

        {
            let book = book;
            let step_label = step_label;
            let back = back;
            let next = next;
            let install = install;
            let current_step = Arc::clone(&current_step);
            let labels = Arc::clone(&labels);
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let package_rows = Rc::clone(&package_rows);
            let package_notes = Rc::clone(&package_notes);
            let can_install = Rc::clone(&can_install);
            let review_can_install = Rc::clone(&review_can_install);
            let last_reaper_app_path = Arc::clone(&last_reaper_app_path);
            let last_resource_path = Arc::clone(&last_resource_path);
            widgets.done_rescan.on_click(move |_| {
                let Some(target) = selected_target_row(&model, &widgets) else {
                    append_done_status(&widgets.done_status, &model.text.review_no_target);
                    return;
                };
                let refreshed_target = refreshed_target_row(&model, &target);
                match wizard_package_plan_for_target(&model, Some(&refreshed_target)) {
                    Ok(plan) => {
                        *package_rows.borrow_mut() = plan.package_rows;
                        *package_notes.borrow_mut() = plan.notes;
                        can_install.set(plan.can_install);
                        review_can_install.set(false);
                        refresh_package_checklist(
                            &widgets.package_checklist,
                            &widgets.package_details,
                            &widgets.osara_keymap_replace,
                            &widgets.osara_keymap_note,
                            &model,
                            &package_rows.borrow(),
                        );
                        refresh_target_choice(
                            &model,
                            &widgets.target_choice,
                            refreshed_target_index(&model, &widgets),
                            &refreshed_target,
                        );
                        widgets.target_details.set_value(&refreshed_target.details);
                        set_last_path(
                            &last_reaper_app_path,
                            Some(planned_reaper_launch_path_for_target(&refreshed_target)),
                        );
                        set_last_resource_path(
                            &last_resource_path,
                            Some(refreshed_target.path.clone()),
                        );
                        current_step.store(PACKAGES_STEP, Ordering::SeqCst);
                        update_navigation(
                            PACKAGES_STEP,
                            &book,
                            &step_label,
                            labels.as_slice(),
                            &back,
                            &next,
                            &install,
                            effective_can_install(&can_install, &review_can_install),
                            target_is_valid(&model, &widgets),
                            reapack_ack_confirmed(&widgets),
                        );
                    }
                    Err(error) => append_done_status(
                        &widgets.done_status,
                        &format!("{}: {}", model.text.done_rescan_error_prefix, error),
                    ),
                }
            });
        }

        frame.centre();
        frame.show(true);
    });
}

fn add_pages(
    book: &SimpleBook,
    model: &WizardModel,
    package_rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    self_update_status: StatusBar,
) -> WizardWidgets {
    let target_page = Panel::builder(book).build();
    let (target_choice, portable_folder, target_details) = build_target_page(&target_page, model);
    book.add_page(&target_page, &model.steps[TARGET_STEP].label, true, None);

    let version_check_page = Panel::builder(book).build();
    let (
        version_check_status,
        version_check_gauge,
        version_check_error_heading,
        version_check_error_log,
    ) = build_version_check_page(
        &version_check_page,
        model,
        wizard_desired_package_ids(model.platform).len() as i32,
    );
    book.add_page(
        &version_check_page,
        &model.steps[VERSION_CHECK_STEP].label,
        false,
        None,
    );

    let packages_page = Panel::builder(book).build();
    let (package_checklist, package_details, osara_keymap_replace, osara_keymap_note) =
        build_packages_page(&packages_page, model, package_rows);
    book.add_page(
        &packages_page,
        &model.steps[PACKAGES_STEP].label,
        false,
        None,
    );

    let reapack_ack_page = Panel::builder(book).build();
    let (_reapack_donate_link, reapack_ack_confirm) =
        build_reapack_ack_page(&reapack_ack_page, model);
    book.add_page(
        &reapack_ack_page,
        &model.steps[REAPACK_ACK_STEP].label,
        false,
        None,
    );

    let review_page = Panel::builder(book).build();
    let review_text = build_review_page(&review_page, model);
    book.add_page(&review_page, &model.steps[REVIEW_STEP].label, false, None);

    let progress_page = Panel::builder(book).build();
    let (progress_status, progress_gauge, progress_details) =
        build_progress_page(&progress_page, model);
    book.add_page(
        &progress_page,
        &model.steps[PROGRESS_STEP].label,
        false,
        None,
    );

    let done_page = Panel::builder(book).build();
    let (
        done_status,
        done_details,
        done_launch_reaper,
        done_open_resource,
        done_rescan,
        done_save_report,
        done_self_update_apply,
    ) = build_done_page(&done_page, model);
    book.add_page(&done_page, &model.steps[DONE_STEP].label, false, None);

    WizardWidgets {
        target_choice,
        portable_folder,
        target_details,
        version_check_status,
        version_check_gauge,
        version_check_error_heading,
        version_check_error_log,
        package_checklist,
        package_details,
        osara_keymap_replace,
        osara_keymap_note,
        reapack_ack_confirm,
        review_text,
        progress_status,
        progress_gauge,
        progress_details,
        done_status,
        done_details,
        done_launch_reaper,
        done_open_resource,
        done_rescan,
        done_save_report,
        done_self_update_apply,
        self_update_status,
    }
}

fn build_target_page(page: &Panel, model: &WizardModel) -> (Choice, DirPickerCtrl, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.target_heading,
        "rais-target-heading",
    );

    add_label(
        page,
        &sizer,
        &model.text.target_choice_label,
        "rais-target-choice-label",
    );

    let choice = Choice::builder(page).build();
    choice.set_name("rais-target-choice");
    for row in &model.target_rows {
        choice.append(&row.label);
    }
    let portable_index = portable_choice_index(model);
    choice.append(&model.text.target_portable_choice);
    choice.set_selection(model.selected_target_index.unwrap_or(portable_index) as u32);
    sizer.add(&choice, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.target_portable_folder_label,
        "rais-target-portable-folder-label",
    );
    let portable_folder = DirPickerCtrl::builder(page)
        .with_message(&model.text.target_portable_folder_message)
        .with_size(Size::new(-1, -1))
        .build();
    portable_folder.set_name("rais-target-portable-folder");
    portable_folder.add_style(WindowStyle::TabStop);
    configure_portable_folder(
        &portable_folder,
        choice
            .get_selection()
            .map(|index| index as usize == portable_index)
            .unwrap_or(false),
    );
    sizer.add(&portable_folder, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.target_details_label,
        "rais-target-details-label",
    );
    let initial_details = selected_target_details(model, &choice, &portable_folder);
    let details = TextCtrl::builder(page)
        .with_value(&initial_details)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    details.set_name("rais-target-details");
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    let choice_model = model.clone();
    let choice_portable_folder = portable_folder;
    let choice_details = details;
    choice.on_selection_changed(move |event| {
        if let Some(index) = event.get_selection() {
            let index = index as usize;
            let portable_selected = index == portable_choice_index(&choice_model);
            configure_portable_folder(&choice_portable_folder, portable_selected);
            let value = if portable_selected {
                portable_target_details(&choice_model, &choice_portable_folder)
            } else {
                target_details_for_index(&choice_model, index)
            };
            choice_details.set_value(&value);
        }
    });

    {
        let model = model.clone();
        let dir_choice = choice;
        let dir_details = details;
        portable_folder.on_dir_changed(move |_| {
            let portable_index = portable_choice_index(&model);
            if dir_choice
                .get_selection()
                .map(|index| index as usize != portable_index)
                .unwrap_or(true)
            {
                dir_choice.set_selection(portable_index as u32);
                configure_portable_folder(&portable_folder, true);
            }
            dir_details.set_value(&portable_target_details(&model, &portable_folder));
        });
    }

    page.set_sizer(sizer, true);
    choice.set_focus();
    (choice, portable_folder, details)
}

/// Base id for the language popup menu's radio items. Item id at index `i`
/// in `WizardModel::language_options` is `LANGUAGE_MENU_ID_BASE + i`.
const LANGUAGE_MENU_ID_BASE: i32 = 13700;

/// Build the language-picker footer inside the root panel, below the wizard
/// buttons. Adding it as a sibling of the button row means tab order naturally
/// reaches it after the last button (rather than partway through the page),
/// then wraps back to the page's first focusable widget.
fn build_language_footer(root_panel: &Panel, root: &BoxSizer, model: &WizardModel) {
    add_label(
        root_panel,
        root,
        &model.text.target_language_label,
        "rais-target-language-label",
    );

    let current_display_name = model
        .language_options
        .iter()
        .find(|option| option.locale == model.current_language)
        .map(|option| option.display_name.clone())
        .unwrap_or_else(|| model.current_language.clone());

    let language_button = Button::builder(root_panel)
        .with_label(&current_display_name)
        .build();
    language_button.set_name("rais-target-language");
    language_button.add_style(WindowStyle::TabStop);
    language_button.set_can_focus(true);
    root.add(&language_button, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        root_panel,
        root,
        &model.text.target_language_restart_note,
        "rais-target-language-restart-note",
    );

    let language_options = model.language_options.clone();
    let current_locale = model.current_language.clone();

    // The popup menu dispatches its EVT_MENU to the popup's owner window
    // (root_panel here), not to the button — only Panel/ScrolledWindow
    // implement MenuEvents in wxdragon today.
    {
        let language_options = language_options.clone();
        let current_locale = current_locale.clone();
        root_panel.on_menu_selected(move |event| {
            let id = event.get_id();
            let raw_index = id - LANGUAGE_MENU_ID_BASE;
            if raw_index < 0 || (raw_index as usize) >= language_options.len() {
                return;
            }
            let Some(option) = language_options.get(raw_index as usize) else {
                return;
            };
            if option.locale == current_locale {
                return;
            }
            relaunch_with_locale(&option.locale);
        });
    }

    let menu_owner = root_panel.clone();
    language_button.on_click(move |_| {
        let mut builder = Menu::builder();
        for (index, option) in language_options.iter().enumerate() {
            let id = LANGUAGE_MENU_ID_BASE + index as i32;
            builder = builder.append_radio_item(id, &option.display_name, "");
        }
        let menu = builder.build();
        for (index, option) in language_options.iter().enumerate() {
            if option.locale == current_locale {
                let id = LANGUAGE_MENU_ID_BASE + index as i32;
                menu.check_item(id, true);
            }
        }
        let mut menu = menu;
        menu_owner.popup_menu(&mut menu, None);
    });
}

/// Captures everything the version-check dispatcher needs to drive the
/// dedicated version-check page: widgets, model, package-row state for the
/// auto-rebuild on success, and the navigation handles needed to advance to
/// the Packages step.
struct VersionCheckUi {
    widgets: WizardWidgets,
    model: Arc<WizardModel>,
    package_rows: Rc<RefCell<Vec<PackageRow>>>,
    package_notes: Rc<RefCell<Vec<String>>>,
    can_install: Rc<Cell<bool>>,
    review_can_install: Rc<Cell<bool>>,
    target: TargetRow,
    book: SimpleBook,
    step_label: StaticText,
    labels: Arc<Vec<String>>,
    back: Button,
    next: Button,
    install: Button,
    current_step: Arc<AtomicUsize>,
}

/// Reset the version-check page to its starting state, install the dispatcher
/// that handles per-package events on the UI thread, and spawn the worker
/// thread. The dispatcher auto-advances to the Packages step on full success;
/// on any failure it stays on the version-check page with the error log
/// populated and the Back button enabled.
fn start_version_check(ui: VersionCheckUi) {
    let package_ids = wizard_desired_package_ids(ui.model.platform);
    let package_count = package_ids.len() as i32;
    ui.widgets
        .version_check_status
        .set_label(&ui.model.text.version_check_status_pending);
    ui.widgets.version_check_gauge.set_value(0);
    ui.widgets
        .version_check_gauge
        .set_range(package_count.max(1));
    ui.widgets.version_check_error_log.set_value("");
    // The error region stays out of the tab order and the a11y tree until a
    // check actually fails — see render_version_check_errors for the show.
    ui.widgets.version_check_error_heading.hide();
    ui.widgets.version_check_error_log.hide();

    let mut accumulated: Vec<AvailablePackage> = Vec::new();
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut completed: i32 = 0;

    let dispatcher = move |event: VersionCheckEvent| match event {
        VersionCheckEvent::Checking { package_id } => {
            with_ui_localizer(|localizer| {
                let display = localized_package_display_name(localizer, &package_id);
                let line = localizer
                    .format(
                        "wizard-version-check-status-checking",
                        &[("package", display.as_str())],
                    )
                    .value;
                ui.widgets.version_check_status.set_label(&line);
            });
        }
        VersionCheckEvent::Result {
            package_id,
            outcome,
        } => {
            completed += 1;
            ui.widgets.version_check_gauge.set_value(completed);
            match outcome {
                Ok(version_str) => match rais_core::version::Version::parse(&version_str) {
                    Ok(version) => {
                        accumulated.push(AvailablePackage {
                            package_id,
                            version: Some(version),
                        });
                    }
                    Err(error) => {
                        errors.push((package_id, error.to_string()));
                    }
                },
                Err(message) => {
                    errors.push((package_id, message));
                }
            }
        }
        VersionCheckEvent::Finished => {
            if errors.is_empty() {
                match wizard_package_plan_for_target_with_available(
                    &ui.model,
                    Some(&ui.target),
                    &accumulated,
                ) {
                    Ok(plan) => {
                        *ui.package_rows.borrow_mut() = plan.package_rows;
                        *ui.package_notes.borrow_mut() = plan.notes;
                        ui.can_install.set(plan.can_install);
                        ui.review_can_install.set(false);
                        rebuild_package_list_widgets(&ui.widgets, &ui.package_rows.borrow());
                        ui.current_step.store(PACKAGES_STEP, Ordering::SeqCst);
                        update_navigation(
                            PACKAGES_STEP,
                            &ui.book,
                            &ui.step_label,
                            ui.labels.as_slice(),
                            &ui.back,
                            &ui.next,
                            &ui.install,
                            effective_can_install(&ui.can_install, &ui.review_can_install),
                            true,
                            reapack_ack_confirmed(&ui.widgets),
                        );
                    }
                    Err(error) => {
                        errors.push((String::new(), error.to_string()));
                        render_version_check_errors(&ui, &errors);
                    }
                }
            } else {
                render_version_check_errors(&ui, &errors);
            }
        }
    };

    install_version_check_dispatcher(Box::new(dispatcher));
    spawn_version_check_worker(package_ids);
}

/// Render error lines to the version-check page's error TextCtrl and update
/// the status text to point the user at Back/Close.
fn render_version_check_errors(ui: &VersionCheckUi, errors: &[(String, String)]) {
    with_ui_localizer(|localizer| {
        let mut lines = Vec::with_capacity(errors.len());
        for (package_id, message) in errors {
            let display = if package_id.is_empty() {
                String::new()
            } else {
                localized_package_display_name(localizer, package_id)
            };
            let line = localizer
                .format(
                    "wizard-version-check-error-line",
                    &[("package", display.as_str()), ("message", message.as_str())],
                )
                .value;
            lines.push(line);
        }
        ui.widgets
            .version_check_error_log
            .set_value(&lines.join("\n"));
        // Surface the error region now that there is content for screen
        // readers + the tab order to expose.
        ui.widgets.version_check_error_heading.show(true);
        ui.widgets.version_check_error_log.show(true);
        let status = localizer
            .format(
                "wizard-version-check-status-error",
                &[("error_count", errors.len().to_string().as_str())],
            )
            .value;
        ui.widgets.version_check_status.set_label(&status);
    });
}

/// Re-render the package CheckListBox after the deferred fetch repopulates
/// `package_rows`. Invoked on successful version check, just before the
/// auto-advance to the Packages step.
fn rebuild_package_list_widgets(widgets: &WizardWidgets, package_rows: &[PackageRow]) {
    widgets.package_checklist.clear();
    for (index, row) in package_rows.iter().enumerate() {
        widgets.package_checklist.append(&row.summary);
        widgets.package_checklist.check(index as u32, row.selected);
    }
    let initial = package_rows
        .first()
        .map(package_details)
        .unwrap_or_default();
    widgets.package_details.set_value(&initial);
}

/// Spawn the deferred latest-version fetch on a background thread. Each
/// per-package outcome is forwarded to the UI thread via `call_after`, which
/// invokes the dispatcher installed by the click handler.
fn spawn_version_check_worker(package_ids: Vec<String>) {
    std::thread::spawn(move || {
        for package_id in package_ids {
            let id_for_checking = package_id.clone();
            wxdragon::call_after(Box::new(move || {
                dispatch_version_check_event(VersionCheckEvent::Checking {
                    package_id: id_for_checking,
                });
            }));

            let outcome = match fetch_latest_for_package(&package_id) {
                Ok(version) => Ok(version.to_string()),
                Err(error) => Err(error.to_string()),
            };

            let id_for_result = package_id.clone();
            wxdragon::call_after(Box::new(move || {
                dispatch_version_check_event(VersionCheckEvent::Result {
                    package_id: id_for_result,
                    outcome,
                });
            }));
        }
        wxdragon::call_after(Box::new(move || {
            dispatch_version_check_event(VersionCheckEvent::Finished);
        }));
    });
}

/// Relaunch the running RAIS executable with `RAIS_LOCALE=<locale>` set so the
/// new locale takes effect immediately, then exit. Errors during relaunch are
/// printed to stderr and the current process keeps running so the user is not
/// left without a UI.
fn relaunch_with_locale(locale: &str) {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("could not resolve current executable for relaunch: {error}");
            return;
        }
    };
    match Command::new(&exe).env("RAIS_LOCALE", locale).spawn() {
        Ok(_) => std::process::exit(0),
        Err(error) => {
            eprintln!("could not relaunch RAIS with locale {locale}: {error}");
        }
    }
}

fn build_packages_page(
    page: &Panel,
    model: &WizardModel,
    package_rows: Rc<RefCell<Vec<crate::PackageRow>>>,
) -> (CheckListBox, TextCtrl, CheckBox, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.packages_heading,
        "rais-packages-heading",
    );
    add_label(
        page,
        &sizer,
        &model.text.packages_list_label,
        "rais-packages-list-label",
    );

    let checklist = CheckListBox::builder(page)
        .with_size(Size::new(-1, 180))
        .build();
    checklist.set_name("rais-package-list");
    for (index, row) in package_rows.borrow().iter().enumerate() {
        checklist.append(&row.summary);
        checklist.check(index as u32, row.selected);
    }
    sizer.add(&checklist, 1, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.package_details_label,
        "rais-package-details-label",
    );
    let initial_details = package_rows
        .borrow()
        .first()
        .map(package_details)
        .unwrap_or_default();
    let details = TextCtrl::builder(page)
        .with_value(&initial_details)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    details.set_name("rais-package-details");
    sizer.add(&details, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.packages_osara_keymap_heading,
        "rais-osara-keymap-heading",
    );
    let osara_keymap_replace = CheckBox::builder(page)
        .with_label(&model.text.packages_osara_keymap_replace_label)
        .build();
    osara_keymap_replace.set_name(&model.text.packages_osara_keymap_replace_label);
    osara_keymap_replace.set_label(&model.text.packages_osara_keymap_replace_label);
    osara_keymap_replace.add_style(WindowStyle::TabStop);
    osara_keymap_replace.set_value(matches!(
        WizardInstallOptions::default().osara_keymap_choice,
        OsaraKeymapChoice::ReplaceCurrent
    ));
    osara_keymap_replace.set_can_focus(false);
    sizer.add(
        &osara_keymap_replace,
        0,
        SizerFlag::All | SizerFlag::Expand,
        6,
    );

    let osara_keymap_note = TextCtrl::builder(page)
        .with_value(&model.text.packages_osara_keymap_unavailable_note)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 68))
        .build();
    osara_keymap_note.set_name("rais-osara-keymap-note");
    osara_keymap_note.enable(false);
    osara_keymap_note.set_can_focus(false);
    sizer.add(&osara_keymap_note, 0, SizerFlag::All | SizerFlag::Expand, 6);

    sync_osara_keymap_widgets(
        model,
        &package_rows.borrow(),
        &checklist,
        &osara_keymap_replace,
        &osara_keymap_note,
    );

    {
        let package_rows = Rc::clone(&package_rows);
        let model = model.clone();
        let checklist_widget = checklist;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        checklist.on_selected(move |event| {
            if let Some(index) = event.get_selection() {
                if let Some(value) = package_rows
                    .borrow()
                    .get(index as usize)
                    .map(package_details)
                {
                    details.set_value(&value);
                }
            }
            sync_osara_keymap_widgets(
                &model,
                &package_rows.borrow(),
                &checklist_widget,
                &osara_checkbox,
                &osara_note,
            );
        });
    }
    // Keyboard-clean disable for unavailable rows: intercept SPACE on
    // `EVT_KEY_DOWN` BEFORE wxCheckListBox's default toggle handler runs.
    // wxdragon's event trampoline pre-sets `Skip(true)`, so we have to
    // explicitly call `event.skip(false)` to consume — the toggle event
    // then never fires on disabled rows, so no flip-back is needed.
    // (Mouse clicks still go through the `on_toggled` veto below because
    // wxdragon doesn't expose `wxListBox::HitTest` to pinpoint which row
    // the click landed on, so we can't intercept clicks before the
    // toggle. Most accessibility users navigate via keyboard, where this
    // intercept is the dominant path.)
    {
        let key_checklist = checklist;
        let key_rows = Rc::clone(&package_rows);
        checklist.on_key_down(move |event| {
            let code = if let WindowEventData::Keyboard(kbd) = &event {
                kbd.get_key_code()
            } else {
                None
            };
            if code == Some(WXK_SPACE) {
                if let Some(index) = key_checklist.get_selection() {
                    let unavailable = key_rows
                        .borrow()
                        .get(index as usize)
                        .is_some_and(|row| !row.available_for_target);
                    if unavailable {
                        event.skip(false);
                        return;
                    }
                }
            }
        });
    }

    let toggled_package_rows = Rc::clone(&package_rows);
    let toggled_model = model.clone();
    let toggled_checklist = checklist;
    let toggled_osara_checkbox = osara_keymap_replace;
    let toggled_osara_note = osara_keymap_note;
    checklist.on_toggled(move |event| {
        if let Some(index) = event.get_selection() {
            let checked = toggled_checklist.is_checked(index);
            // Reject toggles on rows the wizard has marked unavailable
            // for the current target (e.g. JAWS-for-REAPER scripts when
            // the target is portable). The CheckListBox doesn't expose
            // a per-item disable, so we bounce the check state back to
            // unchecked so the user can't enqueue an install we can't
            // honor. The label already carries an "(not available: …)"
            // indicator from `mark_row_unavailable`.
            let unavailable = toggled_package_rows
                .borrow()
                .get(index as usize)
                .is_some_and(|row| !row.available_for_target);
            if unavailable {
                if checked {
                    toggled_checklist.check(index, false);
                }
            } else if let Some(row) = toggled_package_rows.borrow_mut().get_mut(index as usize) {
                // Recompute the row's action/label/summary so the visible
                // "Install / Update / Keep" text follows the new checkbox
                // state, then refresh both the CheckListBox label and the
                // details pane.
                let _ = apply_checkbox_state_to_package_row(&toggled_model, row, checked);
            }
            refresh_checklist_summaries(&toggled_checklist, &toggled_package_rows.borrow());
            if let Some(value) = toggled_package_rows
                .borrow()
                .get(index as usize)
                .map(package_details)
            {
                details.set_value(&value);
            }
        }
        sync_osara_keymap_widgets(
            &toggled_model,
            &toggled_package_rows.borrow(),
            &toggled_checklist,
            &toggled_osara_checkbox,
            &toggled_osara_note,
        );
    });

    {
        let model = model.clone();
        let rows = Rc::clone(&package_rows);
        let checklist_widget = checklist;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        osara_keymap_replace.on_toggled(move |_| {
            sync_osara_keymap_widgets(
                &model,
                &rows.borrow(),
                &checklist_widget,
                &osara_checkbox,
                &osara_note,
            );
        });
    }

    page.set_sizer(sizer, true);
    (checklist, details, osara_keymap_replace, osara_keymap_note)
}

fn build_version_check_page(
    page: &Panel,
    model: &WizardModel,
    package_count: i32,
) -> (StaticText, Gauge, StaticText, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.version_check_heading,
        "rais-version-check-heading",
    );
    let status = StaticText::builder(page)
        .with_label(&model.text.version_check_status_pending)
        .build();
    status.set_name("rais-version-check-status");
    sizer.add(&status, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.version_check_progress_label,
        "rais-version-check-progress-label",
    );
    let gauge = Gauge::builder(page)
        .with_range(package_count.max(1))
        .build();
    gauge.set_name("rais-version-check-progress");
    sizer.add(&gauge, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let error_heading = StaticText::builder(page)
        .with_label(&model.text.version_check_error_heading)
        .build();
    error_heading.set_name("rais-version-check-error-heading");
    sizer.add(&error_heading, 0, SizerFlag::All | SizerFlag::Expand, 6);
    let error_log = TextCtrl::builder(page)
        .with_value("")
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    error_log.set_name("rais-version-check-error-log");
    sizer.add(&error_log, 1, SizerFlag::All | SizerFlag::Expand, 6);

    // Hide the error region until something fails so screen readers do not
    // see an empty Failed-checks/error-log pair while a check is in progress.
    // Show()/Hide() removes the controls from the tab order and the
    // accessibility tree; we re-Show() them in render_version_check_errors.
    error_heading.hide();
    error_log.hide();

    page.set_sizer(sizer, true);
    (status, gauge, error_heading, error_log)
}

/// Build the ReaPack donation-acknowledgement page. The page is only ever
/// shown when ReaPack is in the install/update plan — the Packages → Review
/// transition routes through it conditionally. The Continue button stays
/// disabled until the user checks the acknowledgement; that gating happens
/// in `update_navigation` based on `reapack_ack_confirm.get_value()`.
fn build_reapack_ack_page(page: &Panel, model: &WizardModel) -> (Button, CheckBox) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.reapack_ack_heading,
        "rais-reapack-ack-heading",
    );
    let body = TextCtrl::builder(page)
        .with_value(&model.text.reapack_ack_body)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    body.set_name("rais-reapack-ack-body");
    sizer.add(&body, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let donate_link = Button::builder(page)
        .with_label(&model.text.reapack_ack_link_label)
        .build();
    donate_link.set_name("rais-reapack-ack-donate-link");
    donate_link.add_style(WindowStyle::TabStop);
    donate_link.set_can_focus(true);
    sizer.add(&donate_link, 0, SizerFlag::All, 6);
    donate_link.on_click(move |_| {
        // Best-effort: open the donation page in the user's default browser
        // so the donation hint surfaces on a real, current upstream page
        // rather than a stale cached blurb in the wizard.
        let _ = open_external_url("https://reapack.com/donate");
    });

    let confirm = CheckBox::builder(page)
        .with_label(&model.text.reapack_ack_confirm_label)
        .build();
    confirm.set_name("rais-reapack-ack-confirm");
    confirm.add_style(WindowStyle::TabStop);
    confirm.set_value(false);
    sizer.add(&confirm, 0, SizerFlag::All, 6);

    page.set_sizer(sizer, true);
    (donate_link, confirm)
}

fn open_external_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = url;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "opening URLs is only implemented on Windows and macOS",
        ))
    }
}

fn build_review_page(page: &Panel, model: &WizardModel) -> TextCtrl {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.review_heading,
        "rais-review-heading",
    );
    let review = TextCtrl::builder(page)
        .with_value(&model.review_lines.join("\n"))
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    review.set_name("rais-review-text");
    sizer.add(&review, 1, SizerFlag::All | SizerFlag::Expand, 6);
    page.set_sizer(sizer, true);
    review
}

fn build_progress_page(page: &Panel, model: &WizardModel) -> (StaticText, Gauge, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.progress_heading,
        "rais-progress-heading",
    );
    let status = StaticText::builder(page)
        .with_label(&model.text.progress_status)
        .build();
    status.set_name("rais-progress-status");
    sizer.add(&status, 0, SizerFlag::All | SizerFlag::Expand, 6);
    let gauge = Gauge::builder(page).with_range(100).build();
    gauge.set_name("rais-progress-gauge");
    sizer.add(&gauge, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.progress_details_label,
        "rais-progress-details-label",
    );
    let details = TextCtrl::builder(page)
        .with_value(&model.text.progress_details_idle)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    details.set_name("rais-progress-details");
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    page.set_sizer(sizer, true);
    (status, gauge, details)
}

fn build_done_page(
    page: &Panel,
    model: &WizardModel,
) -> (TextCtrl, TextCtrl, Button, Button, Button, Button, Button) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(page, &sizer, &model.text.done_heading, "rais-done-heading");
    // One short status TextCtrl (always visible) carries the success /
    // failure sentence + any follow-up status updates ("Report saved at …",
    // "REAPER could not be launched: …"). Power-user details live in the
    // collapsible TextCtrl below — kept hidden by default per the
    // streamlined wizard design.
    let status = TextCtrl::builder(page)
        .with_value(&model.text.done_status)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 80))
        .build();
    status.set_name("rais-done-status");
    sizer.add(&status, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let show_details = CheckBox::builder(page)
        .with_label(&model.text.done_show_details_label)
        .build();
    // Mirror the OSARA-keymap checkbox pattern: on this wxdragon version
    // the visible label appears to be driven by the wxWindow *name* on
    // Windows (the `with_label` builder argument doesn't reliably stick),
    // so set both name and label to the same localized string and the
    // checkbox renders correctly in every locale.
    show_details.set_name(&model.text.done_show_details_label);
    show_details.set_label(&model.text.done_show_details_label);
    show_details.add_style(WindowStyle::TabStop);
    show_details.set_value(false);
    sizer.add(&show_details, 0, SizerFlag::All, 6);

    let details = TextCtrl::builder(page)
        .with_value("")
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    details.set_name("rais-done-details");
    details.hide();
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    let toggle_details = details;
    let toggle_page = page.clone();
    show_details.on_toggled(move |event| {
        let visible = event.is_checked();
        toggle_details.show(visible);
        toggle_page.layout();
    });

    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    actions.add_stretch_spacer(1);

    let launch_reaper = Button::builder(page)
        .with_label(&model.text.done_launch_reaper_label)
        .build();
    launch_reaper.set_name("rais-done-launch-reaper");
    launch_reaper.add_style(WindowStyle::TabStop);
    launch_reaper.set_can_focus(true);
    launch_reaper.enable(false);
    actions.add(&launch_reaper, 0, SizerFlag::All, 6);

    let open_resource = Button::builder(page)
        .with_label(&model.text.done_open_resource_label)
        .build();
    open_resource.set_name("rais-done-open-resource");
    open_resource.add_style(WindowStyle::TabStop);
    open_resource.set_can_focus(true);
    open_resource.enable(false);
    actions.add(&open_resource, 0, SizerFlag::All, 6);

    let rescan = Button::builder(page)
        .with_label(&model.text.done_rescan_label)
        .build();
    rescan.set_name("rais-done-rescan");
    rescan.add_style(WindowStyle::TabStop);
    rescan.set_can_focus(true);
    actions.add(&rescan, 0, SizerFlag::All, 6);

    let save_report = Button::builder(page)
        .with_label(&model.text.done_save_report_label)
        .build();
    save_report.set_name("rais-done-save-report");
    save_report.add_style(WindowStyle::TabStop);
    save_report.set_can_focus(true);
    save_report.enable(false);
    actions.add(&save_report, 0, SizerFlag::All, 6);

    let self_update_apply = Button::builder(page)
        .with_label(&model.text.done_self_update_apply_label)
        .build();
    self_update_apply.set_name("rais-done-self-update-apply");
    self_update_apply.add_style(WindowStyle::TabStop);
    self_update_apply.set_can_focus(true);
    self_update_apply.enable(false);
    actions.add(&self_update_apply, 0, SizerFlag::All, 6);

    sizer.add_sizer(&actions, 0, SizerFlag::All | SizerFlag::Expand, 0);
    page.set_sizer(sizer, true);
    (
        status,
        details,
        launch_reaper,
        open_resource,
        rescan,
        save_report,
        self_update_apply,
    )
}

fn add_heading(page: &Panel, sizer: &BoxSizer, label: &str, name: &str) {
    let heading = StaticText::builder(page).with_label(label).build();
    heading.set_name(name);
    sizer.add(&heading, 0, SizerFlag::All | SizerFlag::Expand, 6);
}

fn add_label(page: &Panel, sizer: &BoxSizer, label: &str, name: &str) {
    let widget = StaticText::builder(page).with_label(label).build();
    widget.set_name(name);
    sizer.add(
        &widget,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );
}

fn selected_target_details(
    model: &WizardModel,
    choice: &Choice,
    portable_folder: &DirPickerCtrl,
) -> String {
    match choice.get_selection().map(|index| index as usize) {
        Some(index) if index == portable_choice_index(model) => {
            portable_target_details(model, portable_folder)
        }
        Some(index) => target_details_for_index(model, index),
        None => model.text.target_empty.clone(),
    }
}

fn target_details_for_index(model: &WizardModel, index: usize) -> String {
    model
        .target_rows
        .get(index)
        .map(|row| refreshed_target_row(model, row).details)
        .unwrap_or_else(|| model.text.target_empty.clone())
}

fn package_details(row: &crate::PackageRow) -> String {
    row.details.clone()
}

/// Rebuild the package CheckListBox so each row's label reflects the latest
/// `summary` value in `package_rows`. wxdragon's safe API has no
/// `set_string`-style mutator for CheckListBox items, so we clear and
/// re-append; the per-row checked state and the current selection are
/// preserved. Cheap for the small wizard package list.
fn refresh_checklist_summaries(checklist: &CheckListBox, package_rows: &[PackageRow]) {
    let selection = checklist.get_selection();
    checklist.clear();
    for (index, row) in package_rows.iter().enumerate() {
        checklist.append(&row.summary);
        checklist.check(index as u32, row.selected);
    }
    if let Some(index) = selection {
        if (index as usize) < package_rows.len() {
            checklist.set_selection(index, true);
        }
    }
}

fn progress_details_for_start(
    model: &WizardModel,
    target: Option<&TargetRow>,
    selected_package_indices: &[usize],
    package_rows: &[crate::PackageRow],
    osara_keymap_choice: OsaraKeymapChoice,
    cache_dir: Option<&Path>,
) -> String {
    let mut lines = vec![model.text.progress_details_starting.clone()];
    if let Some(target) = target {
        lines.push(format!(
            "{}: {}",
            model.text.review_target_prefix,
            target.path.display()
        ));
    } else {
        lines.push(model.text.review_no_target.clone());
    }

    if selected_package_indices.is_empty() {
        lines.push(model.text.review_no_package.clone());
    } else {
        for index in selected_package_indices {
            if let Some(row) = package_rows.get(*index) {
                lines.push(format!("{}: {}", row.display_name, row.action_label));
            }
        }
    }

    if osara_selected_for_rows(package_rows, selected_package_indices) {
        lines.push(model.text.review_osara_keymap_heading.clone());
        lines.push(match osara_keymap_choice {
            OsaraKeymapChoice::PreserveCurrent => model.text.review_osara_keymap_preserve.clone(),
            OsaraKeymapChoice::ReplaceCurrent => model.text.review_osara_keymap_replace.clone(),
        });
    }

    if let Some(cache_dir) = cache_dir {
        lines.push(format!(
            "{}: {}",
            model.text.progress_details_cache_prefix,
            cache_dir.display()
        ));
    }

    lines.join("\n")
}

fn step_status(model: &WizardModel, step: usize) -> String {
    model
        .steps
        .get(step)
        .map(|step| step.label.clone())
        .unwrap_or_else(|| model.window_title.clone())
}

fn selected_target_row(model: &WizardModel, widgets: &WizardWidgets) -> Option<TargetRow> {
    let index = widgets.target_choice.get_selection()? as usize;
    if index == portable_choice_index(model) {
        return portable_folder_path(&widgets.portable_folder)
            .map(|path| custom_portable_target_row(model, path, true));
    }
    model
        .target_rows
        .get(index)
        .map(|row| refreshed_target_row(model, row))
}

fn refreshed_target_index(model: &WizardModel, widgets: &WizardWidgets) -> Option<usize> {
    widgets.target_choice.get_selection().map(|index| {
        let index = index as usize;
        if index == portable_choice_index(model) {
            portable_choice_index(model)
        } else {
            index
        }
    })
}

fn refresh_target_choice(
    model: &WizardModel,
    choice: &Choice,
    selected_index: Option<usize>,
    refreshed_target: &TargetRow,
) {
    let selected_index = selected_index.unwrap_or_else(|| portable_choice_index(model));
    choice.clear();
    for (index, row) in model.target_rows.iter().enumerate() {
        if index == selected_index {
            choice.append(&refreshed_target.label);
        } else {
            choice.append(&row.label);
        }
    }
    choice.append(&model.text.target_portable_choice);
    choice.set_selection(selected_index as u32);
}

fn checked_package_indices(checklist: &CheckListBox) -> Vec<usize> {
    (0..checklist.get_count())
        .filter(|index| checklist.is_checked(*index))
        .map(|index| index as usize)
        .collect()
}

fn osara_keymap_choice(checkbox: &CheckBox) -> OsaraKeymapChoice {
    if checkbox.get_value() {
        OsaraKeymapChoice::ReplaceCurrent
    } else {
        OsaraKeymapChoice::PreserveCurrent
    }
}

fn effective_can_install(plan_can_install: &Cell<bool>, review_can_install: &Cell<bool>) -> bool {
    plan_can_install.get() && review_can_install.get()
}

fn refresh_package_checklist(
    checklist: &CheckListBox,
    details: &TextCtrl,
    osara_keymap_replace: &CheckBox,
    osara_keymap_note: &TextCtrl,
    model: &WizardModel,
    rows: &[crate::PackageRow],
) {
    checklist.clear();
    for (index, row) in rows.iter().enumerate() {
        checklist.append(&row.summary);
        checklist.check(index as u32, row.selected);
    }
    details.set_value(&rows.first().map(package_details).unwrap_or_default());
    sync_osara_keymap_widgets(
        model,
        rows,
        checklist,
        osara_keymap_replace,
        osara_keymap_note,
    );
}

fn sync_osara_keymap_widgets(
    model: &WizardModel,
    rows: &[crate::PackageRow],
    checklist: &CheckListBox,
    checkbox: &CheckBox,
    note: &TextCtrl,
) {
    let selected_indices = checked_package_indices(checklist);
    let osara_selected = osara_selected_for_rows(rows, &selected_indices);
    checkbox.enable(osara_selected);
    checkbox.set_can_focus(osara_selected);
    note.set_value(&osara_keymap_note(
        model,
        osara_selected,
        osara_keymap_choice(checkbox),
    ));
    note.enable(osara_selected);
    note.set_can_focus(osara_selected);
}

fn portable_choice_index(model: &WizardModel) -> usize {
    model.target_rows.len()
}

fn portable_folder_path(portable_folder: &DirPickerCtrl) -> Option<PathBuf> {
    let path = portable_folder.get_path();
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn portable_target_details(model: &WizardModel, portable_folder: &DirPickerCtrl) -> String {
    portable_folder_path(portable_folder)
        .map(|path| custom_portable_target_row(model, path, true).details)
        .unwrap_or_else(|| model.text.target_portable_pending_details.clone())
}

fn target_is_valid(model: &WizardModel, widgets: &WizardWidgets) -> bool {
    selected_target_row(model, widgets)
        .map(|target| target.writable)
        .unwrap_or(false)
}

/// Whether the user has checked the ReaPack-donation acknowledgement on
/// the dedicated wizard page. Used by `update_navigation` to gate the
/// Next button on REAPACK_ACK_STEP — the page never shows up in the run
/// at all when ReaPack isn't being installed/updated, so on every other
/// step this value is irrelevant.
fn reapack_ack_confirmed(widgets: &WizardWidgets) -> bool {
    widgets.reapack_ack_confirm.get_value()
}

fn bind_reapack_ack_navigation_updates(
    widgets: WizardWidgets,
    current_step: &Arc<AtomicUsize>,
    next: &Button,
) {
    let current_step = Arc::clone(current_step);
    let next = *next;
    widgets.reapack_ack_confirm.on_toggled(move |event| {
        if current_step.load(Ordering::SeqCst) == REAPACK_ACK_STEP {
            next.enable(event.is_checked());
        }
    });
}

fn bind_target_navigation_updates(
    model: &Arc<WizardModel>,
    widgets: WizardWidgets,
    current_step: &Arc<AtomicUsize>,
    next: &Button,
) {
    {
        let model = Arc::clone(model);
        let current_step = Arc::clone(current_step);
        let next = *next;
        widgets.target_choice.on_selection_changed(move |_| {
            if current_step.load(Ordering::SeqCst) == TARGET_STEP {
                next.enable(target_is_valid(&model, &widgets));
            }
        });
    }
    {
        let model = Arc::clone(model);
        let current_step = Arc::clone(current_step);
        let next = *next;
        widgets.portable_folder.on_dir_changed(move |_| {
            if current_step.load(Ordering::SeqCst) == TARGET_STEP {
                next.enable(target_is_valid(&model, &widgets));
            }
        });
    }
}

fn configure_portable_folder(portable_folder: &DirPickerCtrl, enabled: bool) {
    portable_folder.enable(enabled);
    portable_folder.set_can_focus(enabled);
}

fn set_last_report(
    state: &Arc<Mutex<Option<WizardOutcomeReport>>>,
    report: Option<WizardOutcomeReport>,
) {
    if let Ok(mut slot) = state.lock() {
        *slot = report;
    }
}

fn clone_last_report(
    state: &Arc<Mutex<Option<WizardOutcomeReport>>>,
) -> Option<WizardOutcomeReport> {
    state.lock().ok().and_then(|slot| slot.clone())
}

fn set_last_resource_path(state: &Arc<Mutex<Option<PathBuf>>>, path: Option<PathBuf>) {
    set_last_path(state, path);
}

fn clone_last_resource_path(state: &Arc<Mutex<Option<PathBuf>>>) -> Option<PathBuf> {
    clone_last_path(state)
}

fn set_last_path(state: &Arc<Mutex<Option<PathBuf>>>, path: Option<PathBuf>) {
    if let Ok(mut slot) = state.lock() {
        *slot = path;
    }
}

fn clone_last_path(state: &Arc<Mutex<Option<PathBuf>>>) -> Option<PathBuf> {
    state.lock().ok().and_then(|slot| slot.clone())
}

fn planned_reaper_launch_path_for_target(target: &TargetRow) -> PathBuf {
    target.planned_app_path.clone()
}

fn can_launch_reaper_path(path: Option<&Path>) -> bool {
    path.is_some_and(Path::exists)
}

fn can_launch_last_reaper_path(state: &Arc<Mutex<Option<PathBuf>>>) -> bool {
    can_launch_reaper_path(clone_last_path(state).as_deref())
}

fn append_done_status(status: &TextCtrl, message: &str) {
    let current = status.get_value();
    if current.trim().is_empty() {
        status.set_value(message);
    } else {
        status.set_value(&format!("{current}\n\n{message}"));
    }
}

fn open_resource_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer.exe").arg(path).spawn()?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "opening folders is only implemented on Windows and macOS",
        ))
    }
}

fn launch_reaper(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new(path).spawn()?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        {
            Command::new("open").arg(path).spawn()?;
        } else {
            Command::new(path).spawn()?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "launching REAPER is only implemented on Windows and macOS",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{can_launch_reaper_path, planned_reaper_launch_path_for_target};
    use crate::TargetRow;

    #[test]
    fn launchability_requires_existing_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reaper.exe");

        assert!(!can_launch_reaper_path(Some(&path)));

        fs::write(&path, b"stub").unwrap();

        assert!(can_launch_reaper_path(Some(&path)));
        assert!(!can_launch_reaper_path(None));
    }

    #[test]
    fn planned_launch_path_uses_target_planned_app_path() {
        let target = TargetRow {
            label: "Portable REAPER".to_string(),
            details: String::new(),
            app_path: None,
            planned_app_path: PathBuf::from("C:/PortableREAPER/reaper.exe"),
            path: PathBuf::from("C:/PortableREAPER"),
            version: None,
            portable: true,
            selected: true,
            writable: true,
        };

        assert_eq!(
            planned_reaper_launch_path_for_target(&target),
            PathBuf::from("C:/PortableREAPER/reaper.exe")
        );
    }
}

fn update_navigation(
    step: usize,
    book: &SimpleBook,
    step_label: &StaticText,
    labels: &[String],
    back: &Button,
    next: &Button,
    install: &Button,
    can_install: bool,
    target_valid: bool,
    reapack_ack_confirmed: bool,
) {
    book.set_selection(step);
    if let Some(label) = labels.get(step) {
        step_label.set_label(label);
    }
    back.enable(step > TARGET_STEP && step < DONE_STEP);
    next.enable(match step {
        TARGET_STEP => target_valid,
        // VERSION_CHECK_STEP auto-advances on success; never user-driven.
        PACKAGES_STEP | PROGRESS_STEP => true,
        REAPACK_ACK_STEP => reapack_ack_confirmed,
        _ => false,
    });
    install.enable(step == REVIEW_STEP && can_install);
}
