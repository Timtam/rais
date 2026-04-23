use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use rais_ui_wxdragon::{
    UiBootstrapOptions, WizardInstallOptions, WizardModel, execute_wizard_install,
    install_request_from_model, load_wizard_model, review_lines_for_indices,
    summarize_setup_report,
};
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

const TARGET_STEP: usize = 0;
const PACKAGES_STEP: usize = 1;
const REVIEW_STEP: usize = 2;
const PROGRESS_STEP: usize = 3;
const DONE_STEP: usize = 4;

#[derive(Clone, Copy)]
struct WizardWidgets {
    target_choice: Choice,
    package_checklist: CheckListBox,
    review_text: TextCtrl,
    progress_status: StaticText,
    progress_gauge: Gauge,
    done_status: TextCtrl,
}

pub fn run() {
    let _ = wxdragon::main(|_| {
        let model = match load_wizard_model(UiBootstrapOptions::default()) {
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

        let book = SimpleBook::builder(&root_panel).build();
        book.set_name("rais-wizard-pages");
        let wizard_widgets = add_pages(&book, &model);
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
        let can_install = model.controls.can_install;
        let model = Arc::new(model);

        update_navigation(
            TARGET_STEP,
            &book,
            &step_label,
            labels.as_slice(),
            &back,
            &next,
            &install,
            can_install,
        );

        {
            let book = book;
            let step_label = step_label;
            let back = back;
            let next = next;
            let install = install;
            let current_step = Arc::clone(&current_step);
            let labels = Arc::clone(&labels);
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
                    can_install,
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
            next.on_click(move |_| {
                let step = match current_step.load(Ordering::SeqCst) {
                    TARGET_STEP => PACKAGES_STEP,
                    PACKAGES_STEP => {
                        let review_lines = review_lines_for_indices(
                            &model,
                            selected_target_index(&widgets.target_choice),
                            &checked_package_indices(&widgets.package_checklist),
                        );
                        widgets.review_text.set_value(&review_lines.join("\n"));
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
                    can_install,
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
                    can_install,
                );
                back.enable(false);
                next.enable(false);
                install.enable(false);
                widgets
                    .progress_status
                    .set_label(&model.text.progress_status_running);
                widgets.progress_gauge.set_value(10);

                let selected_target = selected_target_index(&widgets.target_choice);
                let selected_packages = checked_package_indices(&widgets.package_checklist);
                let request = match install_request_from_model(
                    &model,
                    selected_target,
                    &selected_packages,
                    WizardInstallOptions::default(),
                ) {
                    Ok(request) => request,
                    Err(error) => {
                        widgets.progress_gauge.set_value(100);
                        widgets
                            .progress_status
                            .set_label(&model.text.done_status_error);
                        widgets
                            .done_status
                            .set_value(&format!("{}\n\n{}", model.text.done_status_error, error));
                        current_step.store(DONE_STEP, Ordering::SeqCst);
                        update_navigation(
                            DONE_STEP,
                            &book,
                            &step_label,
                            labels.as_slice(),
                            &back,
                            &next,
                            &install,
                            can_install,
                        );
                        return;
                    }
                };

                let ui_model = Arc::clone(&model);
                let ui_current_step = Arc::clone(&current_step);
                let ui_labels = Arc::clone(&labels);
                std::thread::spawn(move || {
                    let result = execute_wizard_install(request);
                    wxdragon::call_after(Box::new(move || {
                        widgets.progress_gauge.set_value(100);
                        match result {
                            Ok(report) => {
                                let summary = summarize_setup_report(&report);
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_success);
                                widgets.done_status.set_value(&format!(
                                    "{}\n\n{}\n\n{}",
                                    ui_model.text.done_status_success,
                                    summary.status_line,
                                    summary.detail_lines.join("\n")
                                ));
                            }
                            Err(error) => {
                                widgets
                                    .progress_status
                                    .set_label(&ui_model.text.done_status_error);
                                widgets.done_status.set_value(&format!(
                                    "{}\n\n{}",
                                    ui_model.text.done_status_error, error
                                ));
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
                        );
                    }));
                });
            });
        }

        let frame_for_close = frame.clone();
        close.on_click(move |_| {
            frame_for_close.close(true);
        });

        frame.centre();
        frame.show(true);
    });
}

fn add_pages(book: &SimpleBook, model: &WizardModel) -> WizardWidgets {
    let target_page = Panel::builder(book).build();
    let target_choice = build_target_page(&target_page, model);
    book.add_page(&target_page, &model.steps[TARGET_STEP].label, true, None);

    let packages_page = Panel::builder(book).build();
    let package_checklist = build_packages_page(&packages_page, model);
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
    let (progress_status, progress_gauge) = build_progress_page(&progress_page, model);
    book.add_page(
        &progress_page,
        &model.steps[PROGRESS_STEP].label,
        false,
        None,
    );

    let done_page = Panel::builder(book).build();
    let done_status = build_done_page(&done_page, model);
    book.add_page(&done_page, &model.steps[DONE_STEP].label, false, None);

    WizardWidgets {
        target_choice,
        package_checklist,
        review_text,
        progress_status,
        progress_gauge,
        done_status,
    }
}

fn build_target_page(page: &Panel, model: &WizardModel) -> Choice {
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
    if model.target_rows.is_empty() {
        choice.append(&model.text.target_empty);
        choice.enable(false);
    } else {
        for row in &model.target_rows {
            choice.append(&row.label);
        }
        if let Some(index) = model.selected_target_index {
            choice.set_selection(index as u32);
        }
    }
    sizer.add(&choice, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.target_details_label,
        "rais-target-details-label",
    );
    let initial_details = selected_target_details(model);
    let details = TextCtrl::builder(page)
        .with_value(&initial_details)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    details.set_name("rais-target-details");
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    let detail_values = Rc::new(
        model
            .target_rows
            .iter()
            .map(|row| row.details.clone())
            .collect::<Vec<_>>(),
    );
    choice.on_selection_changed(move |event| {
        if let Some(index) = event.get_selection() {
            if let Some(value) = detail_values.get(index as usize) {
                details.set_value(value);
            }
        }
    });

    page.set_sizer(sizer, true);
    choice
}

fn build_packages_page(page: &Panel, model: &WizardModel) -> CheckListBox {
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
    for (index, row) in model.package_rows.iter().enumerate() {
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
    let initial_details = model
        .package_rows
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

    let detail_values = Rc::new(
        model
            .package_rows
            .iter()
            .map(package_details)
            .collect::<Vec<_>>(),
    );
    {
        let detail_values = Rc::clone(&detail_values);
        checklist.on_selected(move |event| {
            if let Some(index) = event.get_selection() {
                if let Some(value) = detail_values.get(index as usize) {
                    details.set_value(value);
                }
            }
        });
    }
    checklist.on_toggled(move |event| {
        if let Some(index) = event.get_selection() {
            if let Some(value) = detail_values.get(index as usize) {
                details.set_value(value);
            }
        }
    });

    page.set_sizer(sizer, true);
    checklist
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

fn build_progress_page(page: &Panel, model: &WizardModel) -> (StaticText, Gauge) {
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
    page.set_sizer(sizer, true);
    (status, gauge)
}

fn build_done_page(page: &Panel, model: &WizardModel) -> TextCtrl {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(page, &sizer, &model.text.done_heading, "rais-done-heading");
    let status = TextCtrl::builder(page)
        .with_value(&model.text.done_status)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    status.set_name("rais-done-status");
    sizer.add(&status, 1, SizerFlag::All | SizerFlag::Expand, 6);
    page.set_sizer(sizer, true);
    status
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

fn selected_target_details(model: &WizardModel) -> String {
    model
        .selected_target_index
        .and_then(|index| model.target_rows.get(index))
        .map(|row| row.details.clone())
        .unwrap_or_else(|| model.text.target_empty.clone())
}

fn package_details(row: &rais_ui_wxdragon::PackageRow) -> String {
    format!("{}\n\n{}", row.summary, row.reason)
}

fn step_status(model: &WizardModel, step: usize) -> String {
    model
        .steps
        .get(step)
        .map(|step| step.label.clone())
        .unwrap_or_else(|| model.window_title.clone())
}

fn selected_target_index(choice: &Choice) -> Option<usize> {
    choice.get_selection().map(|index| index as usize)
}

fn checked_package_indices(checklist: &CheckListBox) -> Vec<usize> {
    (0..checklist.get_count())
        .filter(|index| checklist.is_checked(*index))
        .map(|index| index as usize)
        .collect()
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
) {
    book.set_selection(step);
    if let Some(label) = labels.get(step) {
        step_label.set_label(label);
    }
    back.enable(step > TARGET_STEP && step < DONE_STEP);
    next.enable(matches!(step, TARGET_STEP | PACKAGES_STEP | PROGRESS_STEP));
    install.enable(step == REVIEW_STEP && can_install);
}
