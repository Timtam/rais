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
use rais_ui_wxdragon::{
    OsaraKeymapChoice, TargetRow, UiBootstrapOptions, WizardInstallOptions, WizardModel,
    WizardOutcomeReport, build_review_preview_for_package_rows, custom_portable_target_row,
    execute_wizard_install, format_package_install_lock_blocking_message,
    format_self_update_apply_summary, format_self_update_check_summary,
    install_request_from_target_and_rows, load_wizard_model, localizer_from_options,
    manual_attention_handling_summary, osara_keymap_note, osara_selected_for_rows,
    package_requires_manual_attention, preview_manual_instruction_lines, refreshed_target_row,
    relaunch_rais_after_apply, run_wizard_package_install_lock_check, run_wizard_self_update_apply,
    run_wizard_self_update_check, save_wizard_outcome_report, wizard_outcome_report_from_error,
    wizard_outcome_report_from_success, wizard_package_plan_for_target,
};
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

const TARGET_STEP: usize = 0;
const PACKAGES_STEP: usize = 1;
const REVIEW_STEP: usize = 2;
const PROGRESS_STEP: usize = 3;
const DONE_STEP: usize = 4;

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

#[derive(Clone, Copy)]
struct WizardWidgets {
    target_choice: Choice,
    portable_folder: DirPickerCtrl,
    target_details: TextCtrl,
    package_checklist: CheckListBox,
    package_details: TextCtrl,
    osara_keymap_replace: CheckBox,
    osara_keymap_note: TextCtrl,
    review_text: TextCtrl,
    progress_status: StaticText,
    progress_gauge: Gauge,
    progress_details: TextCtrl,
    done_status: TextCtrl,
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
        );
        bind_target_navigation_updates(&model, wizard_widgets, &current_step, &next);

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
            back.on_click(move |_| {
                let step = current_step.load(Ordering::SeqCst).saturating_sub(1);
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
                                PACKAGES_STEP
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
                        let review_preview = build_review_preview_for_package_rows(
                            &model,
                            selected_target.as_ref(),
                            &checked_package_indices(&widgets.package_checklist),
                            &rows,
                            &notes,
                            osara_keymap_choice(&widgets.osara_keymap_replace),
                        );
                        review_can_install.set(review_preview.can_install);
                        widgets
                            .review_text
                            .set_value(&review_preview.lines.join("\n"));
                        REVIEW_STEP
                    }
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
                        widgets
                            .done_status
                            .set_value(&format!("{}\n\n{}", model.text.done_status_error, error));
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
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_success);
                                widgets.done_status.set_value(&format!(
                                    "{}\n\n{}\n\n{}",
                                    ui_model.text.done_status_success,
                                    outcome_report.status_line,
                                    outcome_report.detail_lines.join("\n")
                                ));
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
                            }
                            Err(error) => {
                                let outcome_report = wizard_outcome_report_from_error(
                                    &ui_model,
                                    &request_for_report,
                                    &error,
                                );
                                set_last_report(&ui_last_report, Some(outcome_report.clone()));
                                widgets.progress_details.set_value(&format!(
                                    "{}\n\n{}",
                                    outcome_report.status_line,
                                    outcome_report.detail_lines.join("\n")
                                ));
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_error);
                                widgets.done_status.set_value(&format!(
                                    "{}\n\n{}",
                                    outcome_report.status_line,
                                    outcome_report.detail_lines.join("\n")
                                ));
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
    package_rows: Rc<RefCell<Vec<rais_ui_wxdragon::PackageRow>>>,
    self_update_status: StatusBar,
) -> WizardWidgets {
    let target_page = Panel::builder(book).build();
    let (target_choice, portable_folder, target_details) = build_target_page(&target_page, model);
    book.add_page(&target_page, &model.steps[TARGET_STEP].label, true, None);

    let packages_page = Panel::builder(book).build();
    let (package_checklist, package_details, osara_keymap_replace, osara_keymap_note) =
        build_packages_page(&packages_page, model, package_rows);
    book.add_page(
        &packages_page,
        &model.steps[PACKAGES_STEP].label,
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
        package_checklist,
        package_details,
        osara_keymap_replace,
        osara_keymap_note,
        review_text,
        progress_status,
        progress_gauge,
        progress_details,
        done_status,
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
    package_rows: Rc<RefCell<Vec<rais_ui_wxdragon::PackageRow>>>,
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
    let toggled_package_rows = Rc::clone(&package_rows);
    let toggled_model = model.clone();
    let toggled_checklist = checklist;
    let toggled_osara_checkbox = osara_keymap_replace;
    let toggled_osara_note = osara_keymap_note;
    checklist.on_toggled(move |event| {
        if let Some(index) = event.get_selection() {
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
) -> (TextCtrl, Button, Button, Button, Button, Button) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(page, &sizer, &model.text.done_heading, "rais-done-heading");
    let status = TextCtrl::builder(page)
        .with_value(&model.text.done_status)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    status.set_name("rais-done-status");
    sizer.add(&status, 1, SizerFlag::All | SizerFlag::Expand, 6);

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

fn package_details(row: &rais_ui_wxdragon::PackageRow) -> String {
    row.details.clone()
}

fn progress_details_for_start(
    model: &WizardModel,
    target: Option<&TargetRow>,
    selected_package_indices: &[usize],
    package_rows: &[rais_ui_wxdragon::PackageRow],
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

    let manual_items = selected_package_indices
        .iter()
        .filter_map(|index| package_rows.get(*index))
        .filter(|package| package_requires_manual_attention(model, package, osara_keymap_choice))
        .collect::<Vec<_>>();
    if !manual_items.is_empty() {
        lines.push(model.text.review_manual_heading.clone());
        for package in manual_items {
            lines.push(format!(
                "{}: {}",
                package.display_name,
                manual_attention_handling_summary(model, package, osara_keymap_choice)
            ));
            if let Some(target) = target {
                lines.extend(preview_manual_instruction_lines(
                    model,
                    target,
                    package,
                    osara_keymap_choice,
                ));
            }
        }
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
    rows: &[rais_ui_wxdragon::PackageRow],
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
    rows: &[rais_ui_wxdragon::PackageRow],
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
    use rais_ui_wxdragon::TargetRow;

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
) {
    book.set_selection(step);
    if let Some(label) = labels.get(step) {
        step_label.set_label(label);
    }
    back.enable(step > TARGET_STEP && step < DONE_STEP);
    next.enable(match step {
        TARGET_STEP => target_valid,
        PACKAGES_STEP | PROGRESS_STEP => true,
        _ => false,
    });
    install.enable(step == REVIEW_STEP && can_install);
}
