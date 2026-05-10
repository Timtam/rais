use std::cell::{Cell, RefCell};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use rabbit_core::localization::{Localizer, resolve_runtime_locale};
use rabbit_core::self_update::SelfUpdateCheckReport;

// FluentBundle is !Send, so we keep one Localizer instance per UI thread and
// have call_after bodies read it from this thread-local rather than capturing
// it through worker threads. The wxdragon event loop runs every call_after
// body on the same thread that initialised UI_LOCALIZER (the main thread).
thread_local! {
    static UI_LOCALIZER: RefCell<Option<Rc<Localizer>>> = const { RefCell::new(None) };
    /// Top-level wizard frame, stashed here so transient modal dialogs
    /// (e.g., the once-per-session "RABBIT update available" prompt) can
    /// parent themselves on the wizard window without `Frame` having to
    /// ride inside `Send`-requiring `call_after` closures or the
    /// `WizardWidgets` struct (which is captured by those closures).
    /// `Frame` doesn't impl `Send` because its underlying pointer is
    /// `*mut`; in practice we only ever access it on the UI thread, but
    /// the type system won't accept that as a static promise.
    static UI_FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
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

fn install_ui_frame(frame: Frame) {
    UI_FRAME.with(|cell| {
        *cell.borrow_mut() = Some(frame);
    });
}

fn with_ui_frame<F: FnOnce(&Frame)>(f: F) {
    UI_FRAME.with(|cell| {
        if let Some(frame) = cell.borrow().as_ref() {
            f(frame);
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
// `ConfigurationRow` and `recompute_configuration_row_availability` are
// brought in for the upcoming Configuration tree-group wiring; the model
// currently feeds the install pipeline its initial recommended-selection
// directly via `selected_configuration_step_ids` so the import-warnings
// suppression is a stop-gap until the tree UI lands.
#[allow(unused_imports)]
use crate::{ConfigurationRow, recompute_configuration_row_availability};
use crate::{
    OsaraKeymapChoice, PackageRow, TargetRow, UiBootstrapOptions, WizardInstallOptions,
    WizardModel, WizardOutcomeReport, apply_checkbox_state_to_package_row,
    build_review_preview_for_package_rows, custom_portable_target_row, execute_wizard_install,
    format_self_update_apply_summary, format_self_update_check_summary,
    install_request_from_target_and_rows, load_wizard_model, localized_package_display_name,
    localizer_from_options, osara_keymap_note, osara_selected_for_rows,
    reapack_selected_for_install_or_update, refreshed_target_row, relaunch_rabbit_after_apply,
    run_wizard_self_update_apply, run_wizard_self_update_check, save_wizard_outcome_report,
    selected_configuration_step_ids, wizard_desired_package_ids, wizard_outcome_report_from_error,
    wizard_outcome_report_from_success, wizard_package_plan_for_target,
    wizard_package_plan_for_target_with_available,
};
use rabbit_core::latest::fetch_latest_for_package;
use rabbit_core::plan::{AvailablePackage, PlanActionKind};
#[cfg(target_os = "windows")]
use wxdragon::event::tree_events::TreeEventData;
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;
#[cfg(target_os = "windows")]
use wxdragon::widgets::treectrl::{TreeCtrl, TreeCtrlStyle, TreeItemId};

// Non-Windows uses wxDataViewCtrl with a custom tree model + a toggle
// renderer because there's no equivalent of TVS_CHECKBOXES on macOS's
// NSOutlineView or GTK's GtkTreeView. wxDataView's DataViewToggleRenderer
// is rendered by the platform's native cell-rendering path and (on macOS
// in particular) is exposed through NSAccessibility as a real checkbox
// cell — not as good as Windows' TVS_CHECKBOXES on UIA, but the closest
// portable option without forking wxdragon.
#[cfg(not(target_os = "windows"))]
use wxdragon::widgets::dataview::{
    CustomDataViewTreeModel, DataViewAlign, DataViewCellMode, DataViewColumn, DataViewColumnFlags,
    DataViewCtrl, DataViewEventHandler, DataViewStyle, DataViewTextRenderer,
    DataViewToggleRenderer, Variant, VariantType,
};

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
    /// carries the formatted error message (RabbitError isn't Clone).
    check: Option<std::result::Result<SelfUpdateCheckReport, String>>,
    /// Last status string written to the status bar — used to suppress
    /// screen-reader re-announcements when nothing has changed.
    last_status: String,
    /// `true` once the once-per-session "RABBIT update available" prompt
    /// dialog has been shown. Re-renders that follow the same check
    /// result (e.g., a step change that re-invokes render) skip the
    /// modal so the user isn't re-prompted after dismissing it.
    prompted: bool,
}

fn render_self_update_status(
    widgets: WizardWidgets,
    model: &Arc<WizardModel>,
    localizer: &Localizer,
    state: &Arc<Mutex<SelfUpdateUiState>>,
) {
    let mut state_guard = state.lock().unwrap();
    // Clone the check result up-front so the rest of the function can
    // freely mutate `state_guard` without fighting the borrow checker
    // over an `as_ref()` view of `state_guard.check`. The clone is
    // cheap (a single `Result<SelfUpdateCheckReport, String>`) and the
    // function only runs on completion of a one-shot manifest probe.
    let Some(check) = state_guard.check.clone() else {
        // Startup probe hasn't completed yet; leave the initial
        // "Checking for RABBIT updates…" placeholder in place.
        return;
    };

    // (The package-install lock used to be a single LocalAppData path so
    // RABBIT could warn that another install was in progress before
    // applying a self-update. With locks now scoped per-target we don't
    // have a single global lock to consult here, so the cross-target
    // status line is gone. Concurrent self-update + install on the same
    // target still races at the file rename and surfaces a normal IO
    // error.)
    let status = match &check {
        Ok(report) => format_self_update_check_summary(localizer, report),
        Err(error) => format!("{}: {}", model.text.done_self_update_error_prefix, error),
    };
    let apply_enabled = matches!(&check, Ok(report) if report.update_available);

    let status_changed = status != state_guard.last_status;
    if status_changed {
        widgets.self_update_status.set_status_text(&status, 0);
        state_guard.last_status = status;
    }

    // Once-per-session prompt: if an update is available, ask up front
    // instead of forcing the user to navigate to the Done page to find
    // the apply button. The Done-page button stays around as a fallback
    // for users who pick "No" here and change their mind later.
    if !apply_enabled || state_guard.prompted {
        return;
    }
    state_guard.prompted = true;
    let Ok(report) = check else { return };
    // Drop the lock before showing the modal — `MessageDialog::show_modal`
    // runs a nested wxWidgets event loop, and any UI-thread callback that
    // re-enters `render_self_update_status` while the modal is open would
    // deadlock on a still-held mutex.
    drop(state_guard);

    let title = localizer.text("wizard-self-update-prompt-title").value;
    let current = report.current_version.to_string();
    let latest = report.latest_version.to_string();
    let body = localizer
        .format(
            "wizard-self-update-prompt-body",
            &[("current", current.as_str()), ("latest", latest.as_str())],
        )
        .value;

    // Pull the frame from the UI-thread-local rather than from the
    // captured `widgets` so we don't have to send a non-`Send` `Frame`
    // through the `call_after` closure that wraps this function. The
    // closure runs on the UI thread, so the thread-local was populated
    // by `run()` before any worker fired.
    with_ui_frame(|frame| {
        let dialog = MessageDialog::builder(frame, &body, &title)
            .with_style(
                MessageDialogStyle::YesNo
                    | MessageDialogStyle::IconQuestion
                    | MessageDialogStyle::Centre,
            )
            .build();

        if dialog.show_modal() == ID_YES {
            start_self_update_apply(
                widgets.done_status,
                widgets.self_update_status,
                Arc::clone(model),
            );
        }
    });
}

/// `wx/defs.h`: `WXK_SPACE = 32` (just the ASCII value). Kept around as a
/// fallback intercept on platforms without TVS_CHECKBOXES; on Windows the
/// native tree handles Space toggles internally.
#[allow(dead_code)]
const WXK_SPACE: i32 = 32;

/// Per-platform state handle that the orchestrator (run, button click
/// handlers, post-install hook, version-check dispatcher) holds onto and
/// passes through to `build_packages_page` / `refresh_package_checklist` /
/// `rebuild_package_list_widgets` without caring which widget is on the
/// page. On Windows it carries the live `TreeItemId`s for the native
/// TreeCtrl rows; elsewhere it carries the `CustomDataViewTreeModel`
/// handle so the refresh helpers can re-emit notifications and rebuild
/// the model's userdata in place.
#[cfg(target_os = "windows")]
type PackagesStateCell = Rc<RefCell<PackageItems>>;
#[cfg(not(target_os = "windows"))]
type PackagesStateCell = Rc<RefCell<Option<CustomDataViewTreeModel>>>;

/// Type alias used by `WizardWidgets` for the package list widget itself.
/// Windows: native `wxTreeCtrl` (`SysTreeView32` underneath, with
/// `TVS_CHECKBOXES` enabled by `native_tree_checkboxes::enable_checkboxes`).
/// Non-Windows: `wxDataViewCtrl` driven by a `CustomDataViewTreeModel`
/// with a `DataViewToggleRenderer` for the checkbox column.
#[cfg(target_os = "windows")]
type PackagesView = TreeCtrl;
#[cfg(not(target_os = "windows"))]
type PackagesView = DataViewCtrl;

/// Build the empty per-platform state container that lives for the
/// lifetime of the wizard. On Windows it starts with no leaf TreeItemIds
/// (populated during `build_packages_page`); on non-Windows it starts
/// with `None` for the model handle (populated immediately after the
/// model is constructed in `build_packages_page`).
fn new_packages_state() -> PackagesStateCell {
    #[cfg(target_os = "windows")]
    {
        Rc::new(RefCell::new(PackageItems::empty()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Rc::new(RefCell::new(None))
    }
}

/// Live wxTreeItemId handles for both top-level groups in the Packages
/// tree — "Packages" with its package leaves, and "Configuration" with
/// its configuration-step leaves. Index `i` in each leaves vec
/// corresponds to index `i` in the matching `Vec<PackageRow>` /
/// `Vec<ConfigurationRow>`. Kept in an `Rc<RefCell>` so the closures
/// that handle the state-image-click event, the LEFT_UP fallback, the
/// keyboard fallbacks, and the post-install / version-check rebuild
/// helpers can all reach the same TreeItemIds the populate routine
/// handed out. `TreeItemId` is not `Copy` (it owns a pointer with
/// custom Drop), so we can't store it on the `Copy`-derived
/// `WizardWidgets` directly.
#[cfg(target_os = "windows")]
struct PackageItems {
    /// The "Packages" group node under the (hidden) virtual root.
    /// Becomes `None` between `populate_packages_tree` calls; populated
    /// immediately after each rebuild.
    packages_group: Option<TreeItemId>,
    /// One TreeItemId per package row, in the same order as
    /// `package_rows`.
    packages_leaves: Vec<TreeItemId>,
    /// The "Configuration" group node sitting alongside the Packages
    /// group under the virtual root.
    configuration_group: Option<TreeItemId>,
    /// One TreeItemId per configuration row, in the same order as
    /// `configuration_rows`.
    configuration_leaves: Vec<TreeItemId>,
}

#[cfg(target_os = "windows")]
impl PackageItems {
    fn empty() -> Self {
        Self {
            packages_group: None,
            packages_leaves: Vec::new(),
            configuration_group: None,
            configuration_leaves: Vec::new(),
        }
    }
}

/// Identifies a row in the non-Windows `CustomDataViewTreeModel`. `Package`
/// carries the index into `package_rows`; `Group` is the synthetic
/// "Packages" parent under the invisible root. The `Box<Node>` storage
/// owned by `PackageTreeData` is heap-stable, so `*mut Node` pointers
/// passed across the FFI boundary as opaque item ids stay valid for the
/// model's lifetime.
#[cfg(not(target_os = "windows"))]
#[derive(Clone, Copy, Debug)]
enum NodeKind {
    /// The synthetic "Packages" parent under the invisible root.
    PackagesGroup,
    /// A package leaf — index into `package_rows`.
    Package(usize),
    /// The synthetic "Configuration" parent, sibling of PackagesGroup.
    ConfigurationGroup,
    /// A configuration-step leaf — index into `configuration_rows`.
    Configuration(usize),
}

#[cfg(not(target_os = "windows"))]
#[derive(Debug)]
struct Node {
    kind: NodeKind,
}

/// Userdata stored inside the non-Windows `CustomDataViewTreeModel`. Owns
/// the heap-stable node objects we hand to wxDataView as item ids, and
/// holds clones of the shared `package_rows` and `configuration_rows`
/// Rcs so model callbacks can read row state without going through any
/// external lookup.
#[cfg(not(target_os = "windows"))]
struct PackageTreeData {
    rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    packages_group_label: String,
    configuration_group_label: String,
    packages_group_node: Box<Node>,
    package_nodes: Vec<Box<Node>>,
    configuration_group_node: Box<Node>,
    configuration_nodes: Vec<Box<Node>>,
}

#[cfg(not(target_os = "windows"))]
impl PackageTreeData {
    fn new(
        rows: Rc<RefCell<Vec<crate::PackageRow>>>,
        configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
        packages_group_label: String,
        configuration_group_label: String,
    ) -> Self {
        let package_len = rows.borrow().len();
        let package_nodes: Vec<Box<Node>> = (0..package_len)
            .map(|i| {
                Box::new(Node {
                    kind: NodeKind::Package(i),
                })
            })
            .collect();
        let configuration_len = configuration_rows.borrow().len();
        let configuration_nodes: Vec<Box<Node>> = (0..configuration_len)
            .map(|i| {
                Box::new(Node {
                    kind: NodeKind::Configuration(i),
                })
            })
            .collect();
        Self {
            rows,
            configuration_rows,
            packages_group_label,
            configuration_group_label,
            packages_group_node: Box::new(Node {
                kind: NodeKind::PackagesGroup,
            }),
            package_nodes,
            configuration_group_node: Box::new(Node {
                kind: NodeKind::ConfigurationGroup,
            }),
            configuration_nodes,
        }
    }

    fn packages_group_ptr(&self) -> *const Node {
        self.packages_group_node.as_ref()
    }

    fn configuration_group_ptr(&self) -> *const Node {
        self.configuration_group_node.as_ref()
    }

    fn package_ptr(&self, idx: usize) -> *const Node {
        self.package_nodes[idx].as_ref()
    }

    fn configuration_ptr(&self, idx: usize) -> *const Node {
        self.configuration_nodes[idx].as_ref()
    }

    fn all_package_ptrs(&self) -> Vec<*const Node> {
        self.package_nodes
            .iter()
            .map(|b| b.as_ref() as *const Node)
            .collect()
    }

    fn all_configuration_ptrs(&self) -> Vec<*const Node> {
        self.configuration_nodes
            .iter()
            .map(|b| b.as_ref() as *const Node)
            .collect()
    }
}

/// Model column indices for the non-Windows DataView path.
#[cfg(not(target_os = "windows"))]
const PACKAGE_COL_TOGGLE: u32 = 0;
#[cfg(not(target_os = "windows"))]
const PACKAGE_COL_LABEL: u32 = 1;

/// Windows-only helpers that turn the wx-created `wxTreeCtrl` into a
/// `SysTreeView32` with `TVS_CHECKBOXES` set. wxdragon doesn't expose any of
/// the native APIs we need (no `EnableCheckBoxes`, no `SetItemState`, no
/// `SetStateImageList`), so we reach down to the underlying `HWND` via
/// `Window::get_handle()` and drive the control directly through user32 +
/// raw `SendMessageW` traffic. `wxTreeItemId` on wxMSW is a single-member
/// struct (`void* m_pItem`, no vtable / no padding), so reading the first
/// pointer-sized word of the wxd_TreeItemId_t* gives us the native
/// `HTREEITEM`. This is implementation-dependent but stable on wxMSW today
/// — we live with that fragility because the alternative is forking
/// wxdragon-sys, and the prize is screen-reader-correct native checkboxes
/// (UIA Toggle pattern on each tree row).
#[cfg(target_os = "windows")]
mod native_tree_checkboxes {
    use super::TreeItemId;
    use std::ffi::c_void;

    pub const GWL_STYLE: i32 = -16;
    pub const TVS_CHECKBOXES: u32 = 0x0100;
    const TV_FIRST: u32 = 0x1100;
    const TVM_SETITEMW: u32 = TV_FIRST + 63;
    const TVM_GETITEMW: u32 = TV_FIRST + 62;
    const TVM_SETIMAGELIST: u32 = TV_FIRST + 9;
    const TVM_HITTEST: u32 = TV_FIRST + 17;
    const TVSIL_STATE: usize = 2;
    const TVIF_HANDLE: u32 = 0x0010;
    const TVIF_STATE: u32 = 0x0008;
    const TVIS_STATEIMAGEMASK: u32 = 0xF000;
    pub const TVHT_ONITEMSTATEICON: u32 = 0x0040;

    /// Themed checkbox state ids. See `BP_CHECKBOX` (= 3) of `BUTTON`
    /// theme class in `<vsstyle.h>`. We use the "normal" variants because
    /// the tree control overlays its own selection/hover effects on top.
    const BP_CHECKBOX: i32 = 3;
    const CBS_UNCHECKEDNORMAL: i32 = 1;
    const CBS_CHECKEDNORMAL: i32 = 5;
    const CBS_MIXEDNORMAL: i32 = 9;
    const TS_TRUE: i32 = 1;

    /// `DrawFrameControl` flags used as the unthemed fallback when
    /// `OpenThemeData("BUTTON")` returns null (classic theme / no themes).
    const DFC_BUTTON: u32 = 4;
    const DFCS_BUTTONCHECK: u32 = 0x0000;
    const DFCS_CHECKED: u32 = 0x0400;
    const DFCS_BUTTON3STATE: u32 = 0x0008;

    /// `ImageList_Create` flags: 32-bit color + mask channel.
    const ILC_COLOR32: u32 = 0x0020;
    const ILC_MASK: u32 = 0x0001;

    /// State-image indices we use. `TVS_CHECKBOXES` indices are 1-based
    /// (index 0 means "no state image"). `Mixed` is what the parent
    /// "Packages" group shows when only some children are checked.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TriState {
        Unchecked,
        Checked,
        Mixed,
    }

    impl TriState {
        fn state_image_index(self) -> u32 {
            match self {
                TriState::Unchecked => 1,
                TriState::Checked => 2,
                TriState::Mixed => 3,
            }
        }
    }

    /// Layout-compatible mirror of `TVITEMW` from `<commctrl.h>`.
    #[repr(C)]
    struct Tvitemw {
        mask: u32,
        h_item: *mut c_void,
        state: u32,
        state_mask: u32,
        text: *mut u16,
        text_max: i32,
        image: i32,
        selected_image: i32,
        children: i32,
        l_param: isize,
    }

    #[repr(C)]
    struct RectStruct {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct SizeStruct {
        cx: i32,
        cy: i32,
    }

    unsafe extern "system" {
        fn GetWindowLongPtrW(h_wnd: *mut c_void, n_index: i32) -> isize;
        fn SetWindowLongPtrW(h_wnd: *mut c_void, n_index: i32, dw_new_long: isize) -> isize;
        fn SendMessageW(h_wnd: *mut c_void, msg: u32, w_param: usize, l_param: isize) -> isize;
        fn GetDC(h_wnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(h_wnd: *mut c_void, hdc: *mut c_void) -> i32;
        fn DrawFrameControl(
            hdc: *mut c_void,
            lprc: *const RectStruct,
            type_: u32,
            state: u32,
        ) -> i32;
        fn FillRect(hdc: *mut c_void, lprc: *const RectStruct, hbr: *mut c_void) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: *mut c_void) -> *mut c_void;
        fn DeleteDC(hdc: *mut c_void) -> i32;
        fn CreateCompatibleBitmap(hdc: *mut c_void, cx: i32, cy: i32) -> *mut c_void;
        fn DeleteObject(obj: *mut c_void) -> i32;
        fn SelectObject(hdc: *mut c_void, obj: *mut c_void) -> *mut c_void;
        fn CreateSolidBrush(color: u32) -> *mut c_void;
    }

    #[link(name = "uxtheme")]
    unsafe extern "system" {
        fn OpenThemeData(h_wnd: *mut c_void, classlist: *const u16) -> *mut c_void;
        fn CloseThemeData(htheme: *mut c_void) -> i32;
        fn DrawThemeBackground(
            htheme: *mut c_void,
            hdc: *mut c_void,
            partid: i32,
            stateid: i32,
            prect: *const RectStruct,
            pcliprect: *const RectStruct,
        ) -> i32;
        fn GetThemePartSize(
            htheme: *mut c_void,
            hdc: *mut c_void,
            partid: i32,
            stateid: i32,
            prc: *const RectStruct,
            esize: i32,
            psz: *mut SizeStruct,
        ) -> i32;
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn ImageList_Create(cx: i32, cy: i32, flags: u32, initial: i32, grow: i32) -> *mut c_void;
        fn ImageList_AddMasked(himl: *mut c_void, hbm_image: *mut c_void, cr_mask: u32) -> i32;
        fn ImageList_Destroy(himl: *mut c_void) -> i32;
    }

    /// OR-in the `TVS_CHECKBOXES` style on an existing tree's `HWND` AND
    /// install our own 3-image state list so the synthetic Packages group
    /// can show an indeterminate ("half-checked") state when only some of
    /// its children are checked. The native control by default creates a
    /// 2-image list (unchecked, checked); we replace it with a 3-image
    /// list (unchecked, checked, mixed). The first two come from the
    /// `BUTTON` theme via `DrawThemeBackground` so they match the rest of
    /// the OS's checkboxes; the third uses `CBS_MIXEDNORMAL` to draw the
    /// system's standard mixed-state checkbox glyph.
    pub fn enable_checkboxes(hwnd: *mut c_void) {
        if hwnd.is_null() {
            return;
        }
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            if (style as u32 & TVS_CHECKBOXES) == 0 {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style | TVS_CHECKBOXES as isize);
            }
        }
        install_tristate_state_image_list(hwnd);
    }

    /// Build a 3-image `HIMAGELIST` containing themed unchecked + checked +
    /// mixed checkbox glyphs and install it as the tree's state image
    /// list. Replaces (and frees) whatever list `TVS_CHECKBOXES` may have
    /// auto-created.
    fn install_tristate_state_image_list(hwnd: *mut c_void) {
        // BGR magenta serves as the transparency key for the image list:
        // anywhere we leave magenta is treated as transparent on draw.
        const MAGENTA_BGR: u32 = 0x00FF00FF;

        let class: Vec<u16> = "BUTTON".encode_utf16().chain(std::iter::once(0)).collect();
        let htheme = unsafe { OpenThemeData(hwnd, class.as_ptr()) };

        // Determine checkbox glyph size from the theme (DPI-aware) when
        // available; fall back to a sensible default otherwise.
        let mut size = SizeStruct { cx: 13, cy: 13 };
        if !htheme.is_null() {
            unsafe {
                let _ = GetThemePartSize(
                    htheme,
                    std::ptr::null_mut(),
                    BP_CHECKBOX,
                    CBS_UNCHECKEDNORMAL,
                    std::ptr::null(),
                    TS_TRUE,
                    &mut size,
                );
            }
        }
        let cx = size.cx.max(13);
        let cy = size.cy.max(13);

        let himl = unsafe { ImageList_Create(cx, cy, ILC_COLOR32 | ILC_MASK, 3, 0) };
        if himl.is_null() {
            if !htheme.is_null() {
                unsafe {
                    CloseThemeData(htheme);
                }
            }
            return;
        }

        let states = [CBS_UNCHECKEDNORMAL, CBS_CHECKEDNORMAL, CBS_MIXEDNORMAL];

        unsafe {
            let hdc_screen = GetDC(std::ptr::null_mut());
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let key_brush = CreateSolidBrush(MAGENTA_BGR);

            for state in &states {
                let hbm = CreateCompatibleBitmap(hdc_screen, cx, cy);
                let prev_bm = SelectObject(hdc_mem, hbm);

                // Fill with magenta — anything DrawThemeBackground (or the
                // unthemed fallback) leaves untouched stays magenta and
                // becomes transparent in the image list.
                let rc = RectStruct {
                    left: 0,
                    top: 0,
                    right: cx,
                    bottom: cy,
                };
                let _ = FillRect(hdc_mem, &rc, key_brush);

                if !htheme.is_null() {
                    let _ = DrawThemeBackground(
                        htheme,
                        hdc_mem,
                        BP_CHECKBOX,
                        *state,
                        &rc,
                        std::ptr::null(),
                    );
                } else {
                    let dfcs = match *state {
                        CBS_UNCHECKEDNORMAL => DFCS_BUTTONCHECK,
                        CBS_CHECKEDNORMAL => DFCS_BUTTONCHECK | DFCS_CHECKED,
                        CBS_MIXEDNORMAL => DFCS_BUTTON3STATE | DFCS_CHECKED,
                        _ => DFCS_BUTTONCHECK,
                    };
                    let _ = DrawFrameControl(hdc_mem, &rc, DFC_BUTTON, dfcs);
                }

                SelectObject(hdc_mem, prev_bm);
                let _ = ImageList_AddMasked(himl, hbm, MAGENTA_BGR);
                DeleteObject(hbm);
            }

            DeleteObject(key_brush);
            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);

            if !htheme.is_null() {
                CloseThemeData(htheme);
            }

            // Hand the new image list to the tree control. The control
            // takes ownership of the new list and returns the old one for
            // us to destroy.
            let old_himl = SendMessageW(hwnd, TVM_SETIMAGELIST, TVSIL_STATE, himl as isize);
            if old_himl != 0 {
                ImageList_Destroy(old_himl as *mut c_void);
            }
        }
    }

    #[repr(C)]
    struct TvHitTestPoint {
        x: i32,
        y: i32,
    }

    /// Layout-compatible mirror of `TVHITTESTINFO` from `<commctrl.h>`.
    #[repr(C)]
    struct TvHitTestInfo {
        pt: TvHitTestPoint,
        flags: u32,
        h_item: *mut c_void,
    }

    /// Send `TVM_HITTEST` to the native tree control and return the hit
    /// item handle plus the result flags. `(x, y)` is in the tree's
    /// client-area coordinates (which is what `wxEVT_LEFT_*` mouse-event
    /// positions report on the bound window).
    pub fn hit_test(hwnd: *mut c_void, x: i32, y: i32) -> (u32, *mut c_void) {
        if hwnd.is_null() {
            return (0, std::ptr::null_mut());
        }
        let mut info = TvHitTestInfo {
            pt: TvHitTestPoint { x, y },
            flags: 0,
            h_item: std::ptr::null_mut(),
        };
        unsafe {
            SendMessageW(hwnd, TVM_HITTEST, 0, &mut info as *mut _ as isize);
        }
        (info.flags, info.h_item)
    }

    /// Read the native `HTREEITEM` out of a wxdragon `TreeItemId`. Relies
    /// on:
    /// 1. `TreeItemId { ptr: *mut wxd_TreeItemId_t }` being a single-field
    ///    `repr(Rust)` struct — its layout matches the inner pointer.
    /// 2. `wxd_TreeItemId_t*` being a `reinterpret_cast` of `wxTreeItemId*`
    ///    (confirmed in wxdragon-sys/cpp/src/treectrl.cpp).
    /// 3. `wxTreeItemId` having `void* m_pItem` as its only non-static
    ///    member with no vtable.
    fn htreeitem_from(item: &TreeItemId) -> *mut c_void {
        // SAFETY: see the contract above. Reading the first pointer-sized
        // word of the `TreeItemId` wrapper yields its private `ptr` field;
        // reading the first word of that yields `wxTreeItemId::m_pItem`.
        let inner: *mut c_void = unsafe { std::mem::transmute_copy(item) };
        if inner.is_null() {
            return std::ptr::null_mut();
        }
        unsafe { *(inner as *const *mut c_void) }
    }

    pub fn set_check_state(hwnd: *mut c_void, item: &TreeItemId, checked: bool) {
        let state = if checked {
            TriState::Checked
        } else {
            TriState::Unchecked
        };
        set_check_state_tri(hwnd, item, state);
    }

    /// Set the state image index (1, 2, or 3 — see `TriState`) for the
    /// given tree item. Used both for leaf rows (only `Checked` /
    /// `Unchecked`) and the synthetic Packages group (any of the three).
    pub fn set_check_state_tri(hwnd: *mut c_void, item: &TreeItemId, state: TriState) {
        if hwnd.is_null() {
            return;
        }
        let h_item = htreeitem_from(item);
        if h_item.is_null() {
            return;
        }
        let state_value = state.state_image_index() << 12;
        let mut tvi = Tvitemw {
            mask: TVIF_STATE | TVIF_HANDLE,
            h_item,
            state: state_value,
            state_mask: TVIS_STATEIMAGEMASK,
            text: std::ptr::null_mut(),
            text_max: 0,
            image: 0,
            selected_image: 0,
            children: 0,
            l_param: 0,
        };
        unsafe {
            SendMessageW(hwnd, TVM_SETITEMW, 0, &mut tvi as *mut _ as isize);
        }
    }

    pub fn get_check_state(hwnd: *mut c_void, item: &TreeItemId) -> bool {
        if hwnd.is_null() {
            return false;
        }
        let h_item = htreeitem_from(item);
        if h_item.is_null() {
            return false;
        }
        let mut tvi = Tvitemw {
            mask: TVIF_STATE | TVIF_HANDLE,
            h_item,
            state: 0,
            state_mask: TVIS_STATEIMAGEMASK,
            text: std::ptr::null_mut(),
            text_max: 0,
            image: 0,
            selected_image: 0,
            children: 0,
            l_param: 0,
        };
        unsafe {
            SendMessageW(hwnd, TVM_GETITEMW, 0, &mut tvi as *mut _ as isize);
        }
        // State image index 2 = checked.
        ((tvi.state & TVIS_STATEIMAGEMASK) >> 12) == 2
    }
}

#[derive(Clone, Copy)]
struct WizardWidgets {
    target_choice: Choice,
    portable_folder: TextCtrl,
    target_details: TextCtrl,
    version_check_status: StaticText,
    version_check_gauge: Gauge,
    version_check_error_heading: StaticText,
    version_check_error_log: TextCtrl,
    package_checklist: PackagesView,
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
    self_update_status: StatusBar,
    /// Child Panel hosting the language picker + restart-note label,
    /// rendered below the wizard buttons. Hidden on every step except
    /// `TARGET_STEP` because switching languages relaunches RABBIT, so the
    /// dropdown is only useful before the user has invested any wizard
    /// progress.
    language_footer: Panel,
}

pub fn run() {
    // Pre-seat Cocoa's per-process language so VoiceOver picks a voice that
    // matches the in-app Fluent locale. Has to happen before `wxdragon::main`
    // because that brings NSApplication / NSBundle up, and `AppleLanguages`
    // is only consulted on first read of `[NSBundle preferredLocalizations]`.
    // No-op off macOS.
    seat_macos_apple_languages(&resolve_runtime_locale());

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
        frame.set_name("rabbit-main-window");
        install_ui_frame(frame);

        let root_panel = Panel::builder(&frame).build();
        root_panel.set_name("rabbit-root-panel");

        let root = BoxSizer::builder(Orientation::Vertical).build();
        let step_label = StaticText::builder(&root_panel)
            .with_label(&step_status(&model, TARGET_STEP))
            .build();
        step_label.set_name("rabbit-step-status");
        root.add(&step_label, 0, SizerFlag::All | SizerFlag::Expand, 12);

        // Use the frame's wxStatusBar for self-update status. NVDA's "Report
        // status bar" command (NVDA+End) reads exactly this control, JAWS
        // exposes it via its status-bar review keys, and Narrator/UIA expose
        // the StatusBar role natively. Updating via SetStatusText fires the
        // platform notifications that screen readers auto-announce.
        let self_update_status = frame.create_status_bar(1, 0, 0, "rabbit-self-update-status");
        self_update_status.set_status_text(&model.text.self_update_status_checking, 0);

        let book = SimpleBook::builder(&root_panel).build();
        book.set_name("rabbit-wizard-pages");
        let package_rows = Rc::new(RefCell::new(model.package_rows.clone()));
        let package_notes = Rc::new(RefCell::new(model.notes.clone()));
        let configuration_rows = Rc::new(RefCell::new(model.configuration_rows.clone()));
        // Per-platform shared state for the package list — see
        // `PackagesStateCell`. Populated by `build_packages_page` on the
        // first run and refreshed by `populate_packages_tree` /
        // `rebuild_packages_tree_model` on subsequent rebuilds (deferred
        // version-check finish, post-install rescan).
        let package_items: PackagesStateCell = new_packages_state();
        let can_install = Rc::new(Cell::new(model.controls.can_install));
        let review_can_install = Rc::new(Cell::new(false));
        let last_report = Arc::new(Mutex::new(None::<WizardOutcomeReport>));
        let last_reaper_app_path = Arc::new(Mutex::new(None::<PathBuf>));
        let last_resource_path = Arc::new(Mutex::new(None::<PathBuf>));
        // Build the wizard pages first, the buttons row, then the language
        // footer. Footer is constructed after the buttons so its widgets
        // come *after* the buttons in tab order, but it needs to exist
        // *before* `add_pages` so the WizardWidgets struct can capture
        // its Panel handle.
        root.add(&book, 1, SizerFlag::All | SizerFlag::Expand, 12);

        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);

        let back = Button::builder(&root_panel)
            .with_label(&model.controls.back_label)
            .build();
        back.set_name("rabbit-back-button");
        back.add_style(WindowStyle::TabStop);
        back.set_can_focus(true);
        buttons.add(&back, 0, SizerFlag::All, 6);

        let next = Button::builder(&root_panel)
            .with_label(&model.controls.next_label)
            .build();
        next.set_name("rabbit-next-button");
        next.add_style(WindowStyle::TabStop);
        next.set_can_focus(true);
        buttons.add(&next, 0, SizerFlag::All, 6);

        let install = Button::builder(&root_panel)
            .with_label(&model.controls.install_label)
            .build();
        install.set_name("rabbit-install-button");
        install.add_style(WindowStyle::TabStop);
        install.set_can_focus(true);
        buttons.add(&install, 0, SizerFlag::All, 6);

        let close = Button::builder(&root_panel)
            .with_label(&model.controls.close_label)
            .build();
        close.set_name("rabbit-close-button");
        close.add_style(WindowStyle::TabStop);
        close.set_can_focus(true);
        buttons.add(&close, 0, SizerFlag::All, 6);

        root.add_sizer(&buttons, 0, SizerFlag::All | SizerFlag::Expand, 6);

        let language_footer = build_language_footer(&root_panel, &root, &model);
        let wizard_widgets = add_pages(
            &book,
            &model,
            Rc::clone(&package_rows),
            Rc::clone(&configuration_rows),
            Rc::clone(&package_items),
            Rc::clone(&can_install),
            self_update_status,
            language_footer,
        );

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
            &language_footer,
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
                        let checked = checked_package_indices(&rows);
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
                    &widgets.language_footer,
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
            let package_items = Rc::clone(&package_items);
            let configuration_rows = Rc::clone(&configuration_rows);
            let can_install = Rc::clone(&can_install);
            let review_can_install = Rc::clone(&review_can_install);
            next.on_click(move |_| {
                let step = match current_step.load(Ordering::SeqCst) {
                    TARGET_STEP => {
                        let Some(selected_target) = selected_target_row(&model, &widgets) else {
                            return;
                        };
                        // No offline plan computation here: it would call
                        // `detect_components` (file-system + registry
                        // probes for every builtin package), blocking
                        // the UI thread for ~1–2s before the page
                        // transition fires — long enough that the
                        // screen reader and the page-flip both lag
                        // visibly. The version-check Finished handler
                        // already calls `wizard_package_plan_for_target_with_available`
                        // once latest versions are fetched, which does
                        // the same `detect_components` + `build_install_plan`
                        // work; doing it twice (offline first, then
                        // online) was redundant. Reset
                        // `review_can_install` so the Install button
                        // can't fire from a stale Review state, and let
                        // the worker thread do the heavy lifting.
                        review_can_install.set(false);
                        start_version_check(VersionCheckUi {
                            widgets,
                            model: Arc::clone(&model),
                            package_rows: Rc::clone(&package_rows),
                            package_notes: Rc::clone(&package_notes),
                            configuration_rows: Rc::clone(&configuration_rows),
                            package_items: Rc::clone(&package_items),
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
                    PACKAGES_STEP => {
                        let selected_target = selected_target_row(&model, &widgets);
                        let rows = package_rows.borrow();
                        let notes = package_notes.borrow();
                        let checked = checked_package_indices(&rows);
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
                    &widgets.language_footer,
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
            let package_items = Rc::clone(&package_items);
            let configuration_rows = Rc::clone(&configuration_rows);
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
                    &widgets.language_footer,
                    effective_can_install(&can_install, &review_can_install),
                    target_is_valid(&model, &widgets),
                    reapack_ack_confirmed(&widgets),
                );
                back.enable(false);
                next.enable(false);
                install.enable(false);
                widgets.done_launch_reaper.enable(false);
                widgets.done_open_resource.enable(false);
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
                let rows = package_rows.borrow();
                let selected_packages = checked_package_indices(&rows);
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
                    .ok_or_else(|| rabbit_core::RabbitError::PreflightFailed {
                        message: model.text.review_no_target.clone(),
                    })
                    .and_then(|target| {
                        let configuration_step_ids =
                            selected_configuration_step_ids(&configuration_rows.borrow());
                        install_request_from_target_and_rows(
                            &model,
                            target,
                            &rows,
                            &selected_packages,
                            configuration_step_ids,
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
                        current_step.store(DONE_STEP, Ordering::SeqCst);
                        update_navigation(
                            DONE_STEP,
                            &book,
                            &step_label,
                            labels.as_slice(),
                            &back,
                            &next,
                            &install,
                            &widgets.language_footer,
                            effective_can_install(&can_install, &review_can_install),
                            target_is_valid(&model, &widgets),
                            reapack_ack_confirmed(&widgets),
                        );
                        // Focus the always-visible status TextCtrl so the
                        // screen reader reads the success/failure summary
                        // immediately, and so Tab from there moves on to
                        // the Show-details CheckBox / action buttons
                        // instead of cycling back through earlier widgets.
                        widgets.done_status.set_focus();
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
                    let package_items = Rc::clone(&package_items);
                    let configuration_rows = Rc::clone(&configuration_rows);
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
                        // Recompute configuration row availability against
                        // the freshly-rebuilt package plan (e.g. ReaPack
                        // just got installed → REAPER Accessibility step
                        // becomes available).
                        if let Ok(localizer) = localizer_from_options(&model.bootstrap_options) {
                            recompute_configuration_row_availability(
                                &localizer,
                                &package_rows.borrow(),
                                Some(&refreshed_target.path),
                                &mut configuration_rows.borrow_mut(),
                            );
                        }
                        can_install.set(plan.can_install);
                        review_can_install.set(false);
                        refresh_package_checklist(
                            &widgets.package_checklist,
                            &package_items,
                            &widgets.package_details,
                            &widgets.osara_keymap_replace,
                            &widgets.osara_keymap_note,
                            &model,
                            &package_rows.borrow(),
                            &configuration_rows.borrow(),
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
                                // <resource>/RABBIT/logs/ so users always have
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
                            &widgets.language_footer,
                            can_install,
                            target_is_valid(&ui_model, &widgets),
                            reapack_ack_confirmed(&widgets),
                        );
                        // Focus the always-visible status TextCtrl so the
                        // screen reader announces the install result and
                        // Tab moves forward to the Show-details CheckBox
                        // and action buttons instead of cycling back to
                        // an earlier widget.
                        widgets.done_status.set_focus();
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
            let frame_for_launch = frame.clone();
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
                    return;
                }
                frame_for_launch.close(true);
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

        // (The "Save report" button used to live on the Done page so the
        // user could re-save the outcome JSON+text manually. RABBIT already
        // auto-saves under `<resource>/RABBIT/logs/` on every run — both
        // success and failure paths — so the manual button was redundant
        // and added clutter on a page meant to read like a destination,
        // not a dashboard.)

        let self_update_state = Arc::new(Mutex::new(SelfUpdateUiState::default()));

        // One-shot startup probe: runs the self-update manifest check
        // and stores the result into the shared state, then renders.
        // (Used to also poll a global package-install lock — that lock
        // is now per-target, so the cross-target probe is gone.)
        {
            let model = Arc::clone(&model);
            let widgets = wizard_widgets;
            let state = Arc::clone(&self_update_state);
            std::thread::spawn(move || {
                let check = run_wizard_self_update_check();
                {
                    let mut state = state.lock().unwrap();
                    state.check = Some(match check {
                        Ok(report) => Ok(report),
                        Err(error) => Err(error.to_string()),
                    });
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

        // (Used to also spawn a polling thread that re-checked a global
        // install lock and re-rendered when another RABBIT process started
        // an install. With per-target locks there's no global lock to
        // poll; if a same-target race happens, the install path surfaces
        // it as a `PackageInstallInProgress` error at acquire time.)

        // (The Done page used to host an "Apply RABBIT update" button as
        // an always-reachable fallback to the once-per-session prompt. It
        // was removed because users couldn't find it before completing an
        // install — the modal at startup is now the only entry point, and
        // a user who picks "No" gets re-prompted by relaunching RABBIT.)

        // (The "Rescan target" button used to live here so the user could
        // re-detect installed components on the Done page and jump back
        // to the Packages step. With the post-install auto-rescan hook,
        // package_rows is already up to date by the time the user lands
        // on Done — manual rescan is a debugging affordance. Users who
        // want to re-detect can just relaunch RABBIT.)

        frame.centre();
        frame.show(true);
    });
}

fn add_pages(
    book: &SimpleBook,
    model: &WizardModel,
    package_rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    package_items: PackagesStateCell,
    can_install: Rc<Cell<bool>>,
    self_update_status: StatusBar,
    language_footer: Panel,
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
        build_packages_page(
            &packages_page,
            model,
            package_rows,
            configuration_rows,
            package_items,
            can_install,
        );
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
    let (done_status, done_details, done_launch_reaper, done_open_resource) =
        build_done_page(&done_page, model);
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
        self_update_status,
        language_footer,
    }
}

fn build_target_page(page: &Panel, model: &WizardModel) -> (Choice, TextCtrl, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.target_heading,
        "rabbit-target-heading",
    );

    add_label(
        page,
        &sizer,
        &model.text.target_choice_label,
        "rabbit-target-choice-label",
    );

    let choice = Choice::builder(page).build();
    choice.set_name("rabbit-target-choice");
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
        "rabbit-target-portable-folder-label",
    );

    // We build the path input as a TextCtrl + Browse button instead of
    // wxDirPickerCtrl: wxdragon doesn't expose the picker's inner wxTextCtrl,
    // so the screen reader has no way to read a label off it. Mirroring the
    // picker's composition by hand lets us name the editable field directly,
    // and the user gets a real text input they can paste/type into.
    let portable_row = BoxSizer::builder(Orientation::Horizontal).build();
    let portable_folder = TextCtrl::builder(page).build();
    // Same wxdragon quirk as the ReaPack-ack checkbox below: the screen
    // reader reads the wxWindow *name*, not the preceding StaticText, so
    // set the name to the localized label instead of an internal id.
    portable_folder.set_name(&model.text.target_portable_folder_label);
    portable_folder.add_style(WindowStyle::TabStop);
    portable_row.add(&portable_folder, 1, SizerFlag::Expand | SizerFlag::Right, 6);

    let portable_folder_browse = Button::builder(page)
        .with_label(&model.text.target_portable_folder_browse_label)
        .build();
    portable_folder_browse.set_name(&model.text.target_portable_folder_browse_label);
    portable_folder_browse.add_style(WindowStyle::TabStop);
    portable_row.add(
        &portable_folder_browse,
        0,
        SizerFlag::AlignCenterVertical,
        0,
    );

    sizer.add_sizer(&portable_row, 0, SizerFlag::All | SizerFlag::Expand, 6);

    configure_portable_folder(
        &portable_folder,
        &portable_folder_browse,
        choice
            .get_selection()
            .map(|index| index as usize == portable_index)
            .unwrap_or(false),
    );

    add_label(
        page,
        &sizer,
        &model.text.target_details_label,
        "rabbit-target-details-label",
    );
    let initial_details = selected_target_details(model, &choice, &portable_folder);
    let details = TextCtrl::builder(page)
        .with_value(&initial_details)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    details.set_name("rabbit-target-details");
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    {
        let choice_model = model.clone();
        let choice_portable_folder = portable_folder;
        let choice_portable_browse = portable_folder_browse;
        let choice_details = details;
        choice.on_selection_changed(move |event| {
            if let Some(index) = event.get_selection() {
                let index = index as usize;
                let portable_selected = index == portable_choice_index(&choice_model);
                configure_portable_folder(
                    &choice_portable_folder,
                    &choice_portable_browse,
                    portable_selected,
                );
                let value = if portable_selected {
                    portable_target_details(&choice_model, &choice_portable_folder)
                } else {
                    target_details_for_index(&choice_model, index)
                };
                choice_details.set_value(&value);
            }
        });
    }

    {
        let model = model.clone();
        let dir_choice = choice;
        let dir_details = details;
        let dir_portable_folder = portable_folder;
        let dir_portable_browse = portable_folder_browse;
        // Fires both for keyboard input AND for `set_value` from the Browse
        // button below — wxTextCtrl::SetValue generates wxEVT_TEXT — so this
        // single handler handles typing and the picker dialog uniformly.
        portable_folder.on_text_changed(move |_| {
            let portable_index = portable_choice_index(&model);
            if dir_choice
                .get_selection()
                .map(|index| index as usize != portable_index)
                .unwrap_or(true)
            {
                dir_choice.set_selection(portable_index as u32);
                configure_portable_folder(&dir_portable_folder, &dir_portable_browse, true);
            }
            dir_details.set_value(&portable_target_details(&model, &dir_portable_folder));
        });
    }

    {
        let dialog_parent = *page;
        let model_for_browse = model.clone();
        let browse_target = portable_folder;
        portable_folder_browse.on_click(move |_| {
            let current = browse_target.get_value();
            let dialog = DirDialog::builder(
                &dialog_parent,
                &model_for_browse.text.target_portable_folder_message,
                &current,
            )
            .build();
            if dialog.show_modal() == ID_OK {
                if let Some(path) = dialog.get_path() {
                    // Fires on_text_changed, which runs the same flip-to-portable
                    // + update-details logic typing does.
                    browse_target.set_value(&path);
                }
            }
        });
    }

    page.set_sizer(sizer, true);
    choice.set_focus();
    (choice, portable_folder, details)
}

/// Base id for the language popup menu's radio items. Item id at index `i`
/// in `WizardModel::language_options` is `LANGUAGE_MENU_ID_BASE + i`.
const LANGUAGE_MENU_ID_BASE: i32 = 13700;

/// Build the language-picker footer inside a child Panel that lives below
/// the wizard buttons. The footer is only meaningful on the Target page —
/// switching languages relaunches RABBIT, so a switch from a later step
/// would discard the user's wizard progress anyway. Returning the child
/// Panel here lets the caller hide/show it via `update_navigation` based
/// on the current step. Adding it as a sibling of the button row means
/// tab order naturally reaches it after the last button (rather than
/// partway through the page), then wraps back to the page's first
/// focusable widget.
fn build_language_footer(root_panel: &Panel, root: &BoxSizer, model: &WizardModel) -> Panel {
    let footer = Panel::builder(root_panel).build();
    footer.set_name("rabbit-language-footer");
    let footer_sizer = BoxSizer::builder(Orientation::Vertical).build();

    add_label(
        &footer,
        &footer_sizer,
        &model.text.target_language_label,
        "rabbit-target-language-label",
    );

    let current_display_name = model
        .language_options
        .iter()
        .find(|option| option.locale == model.current_language)
        .map(|option| option.display_name.clone())
        .unwrap_or_else(|| model.current_language.clone());

    let language_button = Button::builder(&footer)
        .with_label(&current_display_name)
        .build();
    language_button.set_name("rabbit-target-language");
    language_button.add_style(WindowStyle::TabStop);
    language_button.set_can_focus(true);
    footer_sizer.add(&language_button, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        &footer,
        &footer_sizer,
        &model.text.target_language_restart_note,
        "rabbit-target-language-restart-note",
    );

    footer.set_sizer(footer_sizer, true);
    root.add(&footer, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let language_options = model.language_options.clone();
    let current_locale = model.current_language.clone();

    // The popup menu dispatches its EVT_MENU to the popup's owner window
    // (the footer Panel here), not to the button — only Panel/ScrolledWindow
    // implement MenuEvents in wxdragon today.
    {
        let language_options = language_options.clone();
        let current_locale = current_locale.clone();
        footer.on_menu_selected(move |event| {
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

    let menu_owner = footer;
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

    footer
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
    configuration_rows: Rc<RefCell<Vec<ConfigurationRow>>>,
    package_items: PackagesStateCell,
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
                Ok(version_str) => match rabbit_core::version::Version::parse(&version_str) {
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
                        // The deferred fetch may have promoted ReaPack to
                        // Update (or vice versa); refresh configuration
                        // row availability against the fresh plan.
                        if let Ok(localizer) = localizer_from_options(&ui.model.bootstrap_options) {
                            recompute_configuration_row_availability(
                                &localizer,
                                &ui.package_rows.borrow(),
                                Some(&ui.target.path),
                                &mut ui.configuration_rows.borrow_mut(),
                            );
                        }
                        ui.can_install.set(plan.can_install);
                        ui.review_can_install.set(false);
                        rebuild_package_list_widgets(
                            &ui.widgets,
                            &ui.package_items,
                            &ui.model,
                            &ui.package_rows.borrow(),
                            &ui.configuration_rows.borrow(),
                        );
                        ui.current_step.store(PACKAGES_STEP, Ordering::SeqCst);
                        update_navigation(
                            PACKAGES_STEP,
                            &ui.book,
                            &ui.step_label,
                            ui.labels.as_slice(),
                            &ui.back,
                            &ui.next,
                            &ui.install,
                            &ui.widgets.language_footer,
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

/// Re-render the package list after the deferred fetch repopulates
/// `package_rows`. Invoked on successful version check, just before the
/// auto-advance to the Packages step. Two implementations: Windows rebuilds
/// the native TreeCtrl from scratch; non-Windows mutates the DataView
/// model's userdata in place and emits a `cleared()` notification.
#[cfg(target_os = "windows")]
fn rebuild_package_list_widgets(
    widgets: &WizardWidgets,
    package_items: &PackagesStateCell,
    model: &WizardModel,
    package_rows: &[PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    populate_packages_tree(
        &widgets.package_checklist,
        package_items,
        model,
        package_rows,
        configuration_rows,
    );
    let initial = package_rows
        .first()
        .map(package_details)
        .unwrap_or_default();
    widgets.package_details.set_value(&initial);
}

/// Windows-only: tear down the existing native tree and rebuild both
/// top-level groups ("Packages" and "Configuration") from
/// `package_rows` + `configuration_rows`. Each leaf gets its native
/// `TVS_CHECKBOXES` state set to match its row's `selected`; each
/// group gets a tristate reflecting its children's aggregate.
#[cfg(target_os = "windows")]
fn populate_packages_tree(
    tree: &TreeCtrl,
    package_items: &PackagesStateCell,
    model: &WizardModel,
    package_rows: &[PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    tree.delete_all_items();
    {
        let mut items = package_items.borrow_mut();
        items.packages_group = None;
        items.packages_leaves.clear();
        items.configuration_group = None;
        items.configuration_leaves.clear();
    }

    let Some(root) = tree.add_root("", None, None) else {
        return;
    };

    // Packages group + leaves.
    let Some(packages_group) =
        tree.append_item(&root, &model.text.packages_tree_group_label, None, None)
    else {
        return;
    };
    let mut packages_leaves = Vec::with_capacity(package_rows.len());
    for row in package_rows.iter() {
        let label = format_row_label(&row.summary, row.selected);
        if let Some(item) = tree.append_item(&packages_group, &label, None, None) {
            native_tree_checkboxes::set_check_state(tree.get_handle(), &item, row.selected);
            packages_leaves.push(item);
        }
    }
    // Tristate reflecting available children's aggregate.
    let packages_state = compute_packages_group_tristate(package_rows);
    native_tree_checkboxes::set_check_state_tri(tree.get_handle(), &packages_group, packages_state);

    // Configuration group + leaves. Always created, even if no
    // configuration rows are recommended for this run — keeps the tree
    // shape stable so the user can find the section if/when it
    // populates after a target switch or post-install rescan.
    let Some(configuration_group) = tree.append_item(
        &root,
        &model.text.configuration_tree_group_label,
        None,
        None,
    ) else {
        return;
    };
    let mut configuration_leaves = Vec::with_capacity(configuration_rows.len());
    for row in configuration_rows.iter() {
        let label = format_row_label(&row.summary, row.selected);
        if let Some(item) = tree.append_item(&configuration_group, &label, None, None) {
            native_tree_checkboxes::set_check_state(tree.get_handle(), &item, row.selected);
            configuration_leaves.push(item);
        }
    }
    let configuration_state = compute_configuration_group_tristate(configuration_rows);
    native_tree_checkboxes::set_check_state_tri(
        tree.get_handle(),
        &configuration_group,
        configuration_state,
    );

    {
        let mut items = package_items.borrow_mut();
        items.packages_group = Some(packages_group.clone());
        items.packages_leaves = packages_leaves;
        items.configuration_group = Some(configuration_group.clone());
        items.configuration_leaves = configuration_leaves;
    }

    tree.expand(&packages_group);
    tree.expand(&configuration_group);
}

/// Windows-only: format a tree-row label. The native `TVS_CHECKBOXES`
/// style draws the checkbox for us, so this is currently just the summary
/// — kept as a single point so we can later add a status glyph or icon
/// without auditing every call site.
#[cfg(target_os = "windows")]
fn format_row_label(summary: &str, _selected: bool) -> String {
    summary.to_string()
}

/// Windows-only: aggregate the per-row `selected` flags into a tristate
/// for the synthetic "Packages" group node. Unavailable rows don't count
/// for either side because they can't enter the install plan and toggling
/// them is a no-op — we only look at the rows the user can actually flip.
#[cfg(target_os = "windows")]
fn compute_packages_group_tristate(rows: &[crate::PackageRow]) -> native_tree_checkboxes::TriState {
    let mut any = false;
    let mut all = true;
    let mut any_checked = false;
    for row in rows.iter().filter(|r| r.available_for_target) {
        any = true;
        if row.selected {
            any_checked = true;
        } else {
            all = false;
        }
    }
    if !any {
        // No selectable rows at all (everything's unavailable for this
        // target). Render the group as unchecked rather than mixed —
        // there's nothing to toggle.
        return native_tree_checkboxes::TriState::Unchecked;
    }
    if all {
        native_tree_checkboxes::TriState::Checked
    } else if any_checked {
        native_tree_checkboxes::TriState::Mixed
    } else {
        native_tree_checkboxes::TriState::Unchecked
    }
}

/// Windows-only: aggregate the per-row `selected` flags of every
/// actionable [`ConfigurationRow`] into a tristate for the synthetic
/// "Configuration" group node. Same convention as
/// `compute_packages_group_tristate` — unavailable rows AND
/// already-applied rows are excluded (the user can't toggle either),
/// and an empty actionable-set renders as Unchecked.
#[cfg(target_os = "windows")]
fn compute_configuration_group_tristate(
    rows: &[crate::ConfigurationRow],
) -> native_tree_checkboxes::TriState {
    let mut any = false;
    let mut all = true;
    let mut any_checked = false;
    for row in rows
        .iter()
        .filter(|r| r.available_for_target && !r.already_applied)
    {
        any = true;
        if row.selected {
            any_checked = true;
        } else {
            all = false;
        }
    }
    if !any {
        return native_tree_checkboxes::TriState::Unchecked;
    }
    if all {
        native_tree_checkboxes::TriState::Checked
    } else if any_checked {
        native_tree_checkboxes::TriState::Mixed
    } else {
        native_tree_checkboxes::TriState::Unchecked
    }
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

/// Trigger the self-update apply pipeline on a worker thread, routing
/// progress (start, summary, relaunch / error) to both the Done page's
/// `done_status` text control and the always-visible `self_update_status`
/// status bar. Two surfaces because the apply can be invoked from two
/// places: the Done page button (where `done_status` is the natural
/// detail surface and `self_update_status` is a redundant short-form),
/// and the once-per-session "RABBIT update available" prompt at startup
/// (where the user is on the Target step and only `self_update_status`
/// is visible). The duplication keeps both call sites simple — neither
/// has to know which surface their user can see.
///
/// Takes individual widget handles rather than the full `WizardWidgets`
/// because that struct now holds a `Frame` (for parenting modal
/// dialogs) and `Frame` isn't `Send` — capturing the whole struct
/// into the spawned worker would break the closure's `Send` bound.
fn start_self_update_apply(
    done_status: TextCtrl,
    self_update_status: StatusBar,
    model: Arc<WizardModel>,
) {
    append_done_status(&done_status, &model.text.done_self_update_apply_running);
    self_update_status.set_status_text(&model.text.done_self_update_apply_running, 0);
    let model_for_thread = Arc::clone(&model);
    std::thread::spawn(move || {
        let result = run_wizard_self_update_apply();
        wxdragon::call_after(Box::new(move || match result {
            Ok(report) => {
                with_ui_localizer(|localizer| {
                    let summary = format_self_update_apply_summary(localizer, &report);
                    append_done_status(&done_status, &summary);
                    self_update_status.set_status_text(&summary, 0);
                });
                if !report.replaced_files.is_empty() {
                    match relaunch_rabbit_after_apply() {
                        Ok(pid) => {
                            let msg = format!(
                                "{}: PID {}",
                                model_for_thread.text.done_self_update_relaunch_prefix, pid
                            );
                            append_done_status(&done_status, &msg);
                            self_update_status.set_status_text(&msg, 0);
                        }
                        Err(error) => {
                            let msg = format!(
                                "{}: {}",
                                model_for_thread.text.done_self_update_error_prefix, error
                            );
                            append_done_status(&done_status, &msg);
                            self_update_status.set_status_text(&msg, 0);
                        }
                    }
                }
            }
            Err(error) => {
                let msg = format!(
                    "{}: {}",
                    model_for_thread.text.done_self_update_error_prefix, error
                );
                append_done_status(&done_status, &msg);
                self_update_status.set_status_text(&msg, 0);
            }
        }));
    });
}

/// macOS: tell Cocoa what language this process is running in by setting the
/// `AppleLanguages` env var, which `[NSBundle preferredLocalizations]` honors
/// before falling back to the user's system-wide language preferences. The
/// bundle's `CFBundleLocalizations` (set in `packaging/macos/Info.plist`)
/// must list the same language codes for this to take effect — without that,
/// Cocoa refuses the override and falls through to its English default. The
/// payoff is VoiceOver picking a voice that matches the in-app UI language;
/// without it, the German UI gets read with the English voice on a system
/// configured for English.
///
/// Uses the BCP-47 language subtag only (`de-DE` → `de`) because that's what
/// matches the `.lproj` directory names and avoids needing region-specific
/// voices to exist on the host. Caller is `run`, before any AppKit init has
/// happened — `AppleLanguages` is read on first access and cached.
#[cfg(target_os = "macos")]
fn seat_macos_apple_languages(locale: &str) {
    let language = locale.split('-').next().unwrap_or(locale).trim();
    if language.is_empty() {
        return;
    }
    // Property-list array literal — Cocoa's preferred encoding for
    // `AppleLanguages` env var values. Single-language form is enough; we
    // don't ship a fallback chain.
    let value = format!("({language})");
    // SAFETY: `run` is called from `main` before any threads are spawned;
    // edition-2024 `set_var` only requires unsafe to flag the cross-thread
    // hazard, which doesn't apply at this point in startup.
    unsafe {
        std::env::set_var("AppleLanguages", value);
    }
}

#[cfg(not(target_os = "macos"))]
fn seat_macos_apple_languages(_locale: &str) {}

/// Relaunch the running RABBIT executable with `RABBIT_LOCALE=<locale>` set so the
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
    match Command::new(&exe).env("RABBIT_LOCALE", locale).spawn() {
        Ok(_) => std::process::exit(0),
        Err(error) => {
            eprintln!("could not relaunch RABBIT with locale {locale}: {error}");
        }
    }
}

/// Windows: native `wxTreeCtrl` driving `SysTreeView32` with
/// `TVS_CHECKBOXES`. Each row exposes UIA Toggle pattern, screen readers
/// announce checked state, Space toggles natively. See
/// `native_tree_checkboxes` for the raw Win32 plumbing that flips the
/// style after wx has created the control.
#[cfg(target_os = "windows")]
fn build_packages_page(
    page: &Panel,
    model: &WizardModel,
    package_rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    package_items: PackagesStateCell,
    can_install: Rc<Cell<bool>>,
) -> (PackagesView, TextCtrl, CheckBox, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.packages_heading,
        "rabbit-packages-heading",
    );
    add_label(
        page,
        &sizer,
        &model.text.packages_list_label,
        "rabbit-packages-list-label",
    );

    // wxTreeCtrl is a thin wrapper around the platform's native tree:
    // SysTreeView32 on Windows, NSOutlineView on macOS, GtkTreeView on GTK.
    // HasButtons + LinesAtRoot give the standard expand/collapse affordance;
    // HideRoot keeps the synthetic root invisible so the "Packages" group
    // appears as the top-level branch the user navigates first.
    let tree = TreeCtrl::builder(page)
        .with_style(
            TreeCtrlStyle::HasButtons
                | TreeCtrlStyle::LinesAtRoot
                | TreeCtrlStyle::Single
                | TreeCtrlStyle::HideRoot,
        )
        .with_size(Size::new(-1, 220))
        .build();
    tree.set_name("rabbit-package-list");

    // Switch the underlying SysTreeView32 to TVS_CHECKBOXES so each tree
    // row gets a real native checkbox — UIA exposes a Toggle pattern on
    // each TreeItem, screen readers announce the checked state, Space
    // toggles natively, and the visual is indistinguishable from File
    // Explorer's "items to copy" tree.
    native_tree_checkboxes::enable_checkboxes(tree.get_handle());

    populate_packages_tree(
        &tree,
        &package_items,
        model,
        &package_rows.borrow(),
        &configuration_rows.borrow(),
    );
    sizer.add(&tree, 1, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.package_details_label,
        "rabbit-package-details-label",
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
    details.set_name("rabbit-package-details");
    sizer.add(&details, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.packages_osara_keymap_heading,
        "rabbit-osara-keymap-heading",
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
    osara_keymap_note.set_name("rabbit-osara-keymap-note");
    osara_keymap_note.enable(false);
    osara_keymap_note.set_can_focus(false);
    sizer.add(&osara_keymap_note, 0, SizerFlag::All | SizerFlag::Expand, 6);

    sync_osara_keymap_widgets(
        model,
        &package_rows.borrow(),
        &osara_keymap_replace,
        &osara_keymap_note,
    );

    // Selection-change updates the package details text. The event fires
    // when the focused row changes via mouse or arrow keys; we use the
    // wxTreeItemId from the event to find the matching index in
    // `package_items.leaves`.
    {
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let model_text = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_selection_changed(move |event| {
            if let Some(item) = event.get_item() {
                match classify_leaf(&package_items.borrow(), &item) {
                    Some(WhichLeaf::Packages(idx)) => {
                        if let Some(value) = package_rows.borrow().get(idx).map(package_details) {
                            details.set_value(&value);
                        }
                    }
                    Some(WhichLeaf::Configuration(idx)) => {
                        if let Some(row) = configuration_rows.borrow().get(idx) {
                            details.set_value(&row.details);
                        }
                    }
                    None => {}
                }
            }
            sync_osara_keymap_widgets(
                &model_text,
                &package_rows.borrow(),
                &osara_checkbox,
                &osara_note,
            );
        });
    }

    // Native checkbox toggle handling for LEAVES: SysTreeView32 fires
    // `wxEVT_TREE_STATE_IMAGE_CLICK` whenever the user activates the
    // checkbox area of a leaf item — both mouse click and Space go through
    // the same notification. The typed `TreeEvents` trait doesn't expose
    // this variant, so we bind the raw `EventType::TREE_STATE_IMAGE_CLICK`
    // ourselves.
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let can_install = Rc::clone(&can_install);
        let wizard_model = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.bind_internal(EventType::TREE_STATE_IMAGE_CLICK, move |event| {
            handle_native_checkbox_toggle(
                &tree_widget,
                &package_items,
                &package_rows,
                &configuration_rows,
                &can_install,
                &wizard_model,
                &details,
                &osara_checkbox,
                &osara_note,
                TreeEventData::new(event).get_item(),
            );
        });
    }

    // Pre-empt mouse clicks on not-actionable leaves' state icons —
    // unavailable packages, already-applied / unavailable configuration
    // steps. SysTreeView32 cycles TVS_CHECKBOXES on WM_LBUTTONDOWN
    // (which gives the click immediate visual feedback); without this
    // pre-empt, the click flips the state image and the accessibility
    // layer announces "checked", and we then have to revert in the
    // TREE_STATE_IMAGE_CLICK path — the user sees a flicker plus a
    // doubled screen-reader announcement. Eating the event before
    // native sees it keeps the row's state stable. We deliberately
    // do NOT pre-empt clicks on the parent group's state icon here
    // (the LEFT_UP handler below handles those — we still want the
    // native side effects of focus/selection to run on a parent
    // click).
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        tree.on_mouse_left_down(move |event| {
            let WindowEventData::MouseButton(mb) = &event else {
                return;
            };
            let Some(pos) = mb.get_position() else { return };
            let hwnd = tree_widget.get_handle();
            let (flags, h_item) = native_tree_checkboxes::hit_test(hwnd, pos.x, pos.y);
            if (flags & native_tree_checkboxes::TVHT_ONITEMSTATEICON) == 0 || h_item.is_null() {
                return;
            }
            let items = package_items.borrow();
            let blocked = items
                .packages_leaves
                .iter()
                .position(|leaf| native_tree_handle(leaf) == h_item)
                .and_then(|idx| package_rows.borrow().get(idx).cloned())
                .map(|row| !row.available_for_target)
                .or_else(|| {
                    items
                        .configuration_leaves
                        .iter()
                        .position(|leaf| native_tree_handle(leaf) == h_item)
                        .and_then(|idx| configuration_rows.borrow().get(idx).cloned())
                        .map(|row| !row.available_for_target || row.already_applied)
                })
                .unwrap_or(false);
            drop(items);
            if blocked {
                event.skip(false);
            }
        });
    }

    // Parent-group toggle fallback: wxEVT_TREE_STATE_IMAGE_CLICK doesn't
    // fire reliably for the parent group (the native control's auto-cycle
    // on state image 3 doesn't propagate to wx in our setup), so we hit-
    // test on every left-button release and propagate manually if the
    // click landed on the group's state icon. Leaves are still handled
    // by the TREE_STATE_IMAGE_CLICK binding above; this handler ignores
    // them (see the early-return on `is_group` inside).
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let can_install = Rc::clone(&can_install);
        let wizard_model = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_mouse_left_up(move |event| {
            if let WindowEventData::MouseButton(mb) = &event {
                if let Some(pos) = mb.get_position() {
                    handle_packages_left_up(
                        &tree_widget,
                        &package_items,
                        &package_rows,
                        &configuration_rows,
                        &can_install,
                        &wizard_model,
                        &details,
                        &osara_checkbox,
                        &osara_note,
                        pos,
                    );
                }
            }
            event.skip(true);
        });
    }

    // Keyboard parent-toggle: Space on the parent group needs to
    // propagate just like a mouse click on its checkbox. Native
    // TVS_CHECKBOXES auto-cycles state on Space too, but its NM_CLICK
    // → wxEVT_TREE_STATE_IMAGE_CLICK dispatch has the same parent-skip
    // problem we hit with the mouse, so we intercept Space at key-down
    // time, propagate, and consume the event so the native cycle
    // doesn't get a chance to leave the parent in some weird half-state.
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let can_install = Rc::clone(&can_install);
        let wizard_model = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_key_down(move |event| {
            let key_code = if let WindowEventData::Keyboard(kbd) = &event {
                kbd.get_key_code()
            } else {
                None
            };
            if key_code != Some(WXK_SPACE) {
                return;
            }
            let Some(focused) = tree_widget.get_selection() else {
                return;
            };
            // Pre-empt Space on a leaf the user can't toggle —
            // unavailable packages, or already-applied / unavailable
            // configuration steps. If we let native TVS_CHECKBOXES
            // see the keystroke, it cycles the state image (and the
            // accessibility layer announces "checked"), and our key-up
            // handler then has to revert. The user experiences that
            // as a flicker plus a doubled screen-reader announcement.
            // Consuming the event here keeps the row's state stable.
            if let Some(leaf) = classify_leaf(&package_items.borrow(), &focused) {
                let blocked = match leaf {
                    WhichLeaf::Packages(idx) => package_rows
                        .borrow()
                        .get(idx)
                        .is_some_and(|row| !row.available_for_target),
                    WhichLeaf::Configuration(idx) => configuration_rows
                        .borrow()
                        .get(idx)
                        .is_some_and(|row| !row.available_for_target || row.already_applied),
                };
                if blocked {
                    event.skip(false);
                    return;
                }
                // Actionable leaf: let native cycle run; on_key_up
                // reconciles row.selected with the post-cycle state.
                return;
            }
            let group = classify_group(&package_items.borrow(), &focused);
            let Some(group) = group else {
                return;
            };
            propagate_group_toggle_to_leaves(
                &tree_widget,
                &package_items,
                group,
                &package_rows,
                &configuration_rows,
                &wizard_model,
            );
            refresh_after_packages_toggle(
                &tree_widget,
                &package_items,
                &package_rows,
                &configuration_rows,
                &can_install,
                &wizard_model,
                &details,
                &osara_checkbox,
                &osara_note,
            );
            // Consume the event so the native control doesn't *also*
            // toggle the parent's state image after us.
            event.skip(false);
        });
    }

    // Keyboard leaf-toggle: wxEVT_TREE_STATE_IMAGE_CLICK fires only off
    // NM_CLICK (mouse), not for keyboard Space — the native control's
    // TVS_CHECKBOXES auto-cycle on Space flips the visual but doesn't
    // route through any wx event we can hook. Without this handler, a
    // Space toggle on a leaf updates the visual but never updates
    // `package_rows`, leaving the row out of sync with the checkbox
    // and (as a knock-on) the parent's tristate stuck on whatever it
    // was before. We bind KEY_UP because by then the native auto-cycle
    // has already run, so reading `get_check_state` gives us the new
    // post-cycle state.
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let can_install = Rc::clone(&can_install);
        let wizard_model = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_key_up(move |event| {
            let key_code = if let WindowEventData::Keyboard(kbd) = &event {
                kbd.get_key_code()
            } else {
                None
            };
            if key_code != Some(WXK_SPACE) {
                return;
            }
            let Some(focused) = tree_widget.get_selection() else {
                return;
            };
            // Parent Space is already handled in on_key_down (which
            // consumed the event before native processing); only
            // leaves need post-cycle reconciliation here.
            if classify_group(&package_items.borrow(), &focused).is_some() {
                return;
            }
            let leaf = classify_leaf(&package_items.borrow(), &focused);
            let new_state =
                native_tree_checkboxes::get_check_state(tree_widget.get_handle(), &focused);
            match leaf {
                Some(WhichLeaf::Packages(idx)) => {
                    let unavailable = package_rows
                        .borrow()
                        .get(idx)
                        .is_some_and(|row| !row.available_for_target);
                    if unavailable {
                        native_tree_checkboxes::set_check_state(
                            tree_widget.get_handle(),
                            &focused,
                            false,
                        );
                        return;
                    }
                    if let Some(row) = package_rows.borrow_mut().get_mut(idx) {
                        let _ = apply_checkbox_state_to_package_row(&wizard_model, row, new_state);
                    }
                    if let Some(row) = package_rows.borrow().get(idx) {
                        let label = format_row_label(&row.summary, row.selected);
                        tree_widget.set_item_text(&focused, &label);
                    }
                }
                Some(WhichLeaf::Configuration(idx)) => {
                    let not_actionable = configuration_rows
                        .borrow()
                        .get(idx)
                        .is_some_and(|row| !row.available_for_target || row.already_applied);
                    if not_actionable {
                        native_tree_checkboxes::set_check_state(
                            tree_widget.get_handle(),
                            &focused,
                            false,
                        );
                        return;
                    }
                    if let Some(row) = configuration_rows.borrow_mut().get_mut(idx) {
                        row.selected = new_state;
                    }
                }
                None => return,
            }
            refresh_after_packages_toggle(
                &tree_widget,
                &package_items,
                &package_rows,
                &configuration_rows,
                &can_install,
                &wizard_model,
                &details,
                &osara_checkbox,
                &osara_note,
            );
        });
    }

    // Enter / double-click handler — Enter on the parent group needs to
    // propagate the toggle to all leaves (the native control fires
    // wxEVT_TREE_ITEM_ACTIVATED for Enter, regardless of TVS_CHECKBOXES).
    //
    // We deliberately ignore the leaf case here: wxMSW also dispatches
    // ITEM_ACTIVATED for Space on a focused leaf, and toggling the leaf
    // here would race with the TREE_STATE_IMAGE_CLICK leaf path (the
    // native auto-cycle has already flipped the state image, our
    // STATE_IMAGE_CLICK handler reads + applies the new state, and a
    // second flip from this handler would then leave the row out of
    // sync with the visual). Space + click on leaves continue to work
    // through the existing TREE_STATE_IMAGE_CLICK binding; Enter on a
    // leaf is intentionally a no-op.
    {
        let tree_widget = tree;
        let package_rows = Rc::clone(&package_rows);
        let configuration_rows = Rc::clone(&configuration_rows);
        let package_items = Rc::clone(&package_items);
        let can_install = Rc::clone(&can_install);
        let wizard_model = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_item_activated(move |event| {
            let Some(item) = event.get_item() else {
                return;
            };
            let Some(group) = classify_group(&package_items.borrow(), &item) else {
                return;
            };
            propagate_group_toggle_to_leaves(
                &tree_widget,
                &package_items,
                group,
                &package_rows,
                &configuration_rows,
                &wizard_model,
            );
            refresh_after_packages_toggle(
                &tree_widget,
                &package_items,
                &package_rows,
                &configuration_rows,
                &can_install,
                &wizard_model,
                &details,
                &osara_checkbox,
                &osara_note,
            );
        });
    }

    {
        let model_text = model.clone();
        let rows = Rc::clone(&package_rows);
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        osara_keymap_replace.on_toggled(move |_| {
            sync_osara_keymap_widgets(&model_text, &rows.borrow(), &osara_checkbox, &osara_note);
        });
    }

    page.set_sizer(sizer, true);
    (tree, details, osara_keymap_replace, osara_keymap_note)
}

/// Windows-only: which top-level group a tree item belongs to, if any.
/// Used by the toggle handlers to dispatch on Packages-vs-Configuration
/// without rebuilding the HTREEITEM-comparison plumbing at each call.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhichGroup {
    Packages,
    Configuration,
}

/// Windows-only: a leaf's owning group + its row index. Mirrors the
/// shape of the row vec the index applies to, so callers can reach into
/// the right `Vec<PackageRow>` / `Vec<ConfigurationRow>` without an
/// extra branch.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhichLeaf {
    Packages(usize),
    Configuration(usize),
}

/// Windows-only: identify which top-level group node a tree item is.
/// Returns `None` for leaves and for the (hidden) virtual root. Compares
/// via the native `HTREEITEM` (`m_pItem`) because wxdragon's
/// `TreeItemId` wraps a fresh allocation per event call — pointer
/// equality on the Rust wrappers wouldn't match our stored handles.
#[cfg(target_os = "windows")]
fn classify_group(items: &PackageItems, candidate: &TreeItemId) -> Option<WhichGroup> {
    let candidate_handle = native_tree_handle(candidate);
    if candidate_handle.is_null() {
        return None;
    }
    if items
        .packages_group
        .as_ref()
        .is_some_and(|group| native_tree_handle(group) == candidate_handle)
    {
        return Some(WhichGroup::Packages);
    }
    if items
        .configuration_group
        .as_ref()
        .is_some_and(|group| native_tree_handle(group) == candidate_handle)
    {
        return Some(WhichGroup::Configuration);
    }
    None
}

/// Windows-only: identify which group's leaves a tree item belongs to,
/// and at which index within that vec. Returns `None` for the group
/// nodes themselves, the hidden root, and items that aren't part of
/// our current row sets.
#[cfg(target_os = "windows")]
fn classify_leaf(items: &PackageItems, candidate: &TreeItemId) -> Option<WhichLeaf> {
    let candidate_handle = native_tree_handle(candidate);
    if candidate_handle.is_null() {
        return None;
    }
    if let Some(idx) = items
        .packages_leaves
        .iter()
        .position(|stored| native_tree_handle(stored) == candidate_handle)
    {
        return Some(WhichLeaf::Packages(idx));
    }
    if let Some(idx) = items
        .configuration_leaves
        .iter()
        .position(|stored| native_tree_handle(stored) == candidate_handle)
    {
        return Some(WhichLeaf::Configuration(idx));
    }
    None
}

/// Windows-only: read the native `HTREEITEM` behind a wxdragon
/// `TreeItemId`. SAFETY contract is the same as `native_tree_checkboxes`:
/// `TreeItemId` is a single-field `repr(Rust)` wrapper around
/// `*mut wxd_TreeItemId_t`, and that pointer is a `reinterpret_cast` of
/// `wxTreeItemId*` which holds a single `void* m_pItem` member.
#[cfg(target_os = "windows")]
fn native_tree_handle(item: &TreeItemId) -> *mut std::ffi::c_void {
    if !item.is_ok() {
        return std::ptr::null_mut();
    }
    // Read the wrapper's private `ptr` field by transmuting `&TreeItemId`
    // into a borrow of its inner pointer.
    let inner: *mut std::ffi::c_void = unsafe { std::mem::transmute_copy(item) };
    if inner.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { *(inner as *const *mut std::ffi::c_void) }
}

/// Windows-only: handle a `wxEVT_TREE_STATE_IMAGE_CLICK` for a leaf row
/// in either the Packages or the Configuration group.
///
/// Parent-group state-icon clicks are intentionally NOT handled here —
/// they're routed through the dedicated `LEFT_UP` + hit-test fallback
/// (`handle_packages_left_up`) because wxMSW's NM_CLICK →
/// wxEVT_TREE_STATE_IMAGE_CLICK dispatch isn't reliable for parent items
/// in our setup (the native auto-cycle sees state image index 3 and may
/// not propagate the event).
///
/// For leaves: `TVS_CHECKBOXES` has already flipped the state image by
/// the time this event fires, so we read the post-click state from the
/// native control rather than computing it ourselves. Package leaves
/// route through `apply_checkbox_state_to_package_row` (so action labels
/// flip Install/Update/Keep); configuration leaves just toggle
/// `row.selected`.
#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
fn handle_native_checkbox_toggle(
    tree: &TreeCtrl,
    package_items: &PackagesStateCell,
    package_rows: &Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: &Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    can_install: &Rc<Cell<bool>>,
    wizard_model: &WizardModel,
    details: &TextCtrl,
    osara_checkbox: &CheckBox,
    osara_note: &TextCtrl,
    item: Option<TreeItemId>,
) {
    let Some(item) = item else {
        return;
    };

    // Parent-group clicks defer to the LEFT_UP handler.
    if classify_group(&package_items.borrow(), &item).is_some() {
        return;
    }

    let leaf = classify_leaf(&package_items.borrow(), &item);
    let new_state = native_tree_checkboxes::get_check_state(tree.get_handle(), &item);
    match leaf {
        Some(WhichLeaf::Packages(idx)) => {
            let unavailable = package_rows
                .borrow()
                .get(idx)
                .is_some_and(|row| !row.available_for_target);
            if unavailable {
                native_tree_checkboxes::set_check_state(tree.get_handle(), &item, false);
                return;
            }
            if let Some(row) = package_rows.borrow_mut().get_mut(idx) {
                let _ = apply_checkbox_state_to_package_row(wizard_model, row, new_state);
            }
            if let Some(row) = package_rows.borrow().get(idx) {
                let label = format_row_label(&row.summary, row.selected);
                tree.set_item_text(&item, &label);
            }
        }
        Some(WhichLeaf::Configuration(idx)) => {
            let not_actionable = configuration_rows
                .borrow()
                .get(idx)
                .is_some_and(|row| !row.available_for_target || row.already_applied);
            if not_actionable {
                native_tree_checkboxes::set_check_state(tree.get_handle(), &item, false);
                return;
            }
            if let Some(row) = configuration_rows.borrow_mut().get_mut(idx) {
                row.selected = new_state;
            }
            // Configuration row labels don't include action text, so no
            // re-format is needed; the row's `summary` already matches
            // `display_name`.
        }
        None => return,
    }

    refresh_after_packages_toggle(
        tree,
        package_items,
        package_rows,
        configuration_rows,
        can_install,
        wizard_model,
        details,
        osara_checkbox,
        osara_note,
    );
}

/// Windows-only: hit-test a `wxEVT_LEFT_UP` mouse-up against the tree.
/// If the click landed on either group's state icon, propagate the
/// toggle to that group's available leaves. The native control may
/// have auto-cycled the parent's image to a state that disagrees with
/// the row aggregate, but we always rewrite the parent state via
/// `set_check_state_tri` at the end so the visual matches the data.
///
/// We deliberately ignore leaf state-icon clicks here — they go through
/// `wxEVT_TREE_STATE_IMAGE_CLICK` which is reliable for leaf items and
/// has the post-cycle state already populated, so duplicating the work
/// here would either double-toggle or fight the leaf path.
#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
fn handle_packages_left_up(
    tree: &TreeCtrl,
    package_items: &PackagesStateCell,
    package_rows: &Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: &Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    can_install: &Rc<Cell<bool>>,
    wizard_model: &WizardModel,
    details: &TextCtrl,
    osara_checkbox: &CheckBox,
    osara_note: &TextCtrl,
    pos: Point,
) {
    let hwnd = tree.get_handle();
    let (flags, h_item) = native_tree_checkboxes::hit_test(hwnd, pos.x, pos.y);
    if (flags & native_tree_checkboxes::TVHT_ONITEMSTATEICON) == 0 || h_item.is_null() {
        return;
    }
    // Identify which group's state icon was hit by comparing the raw
    // HTREEITEM directly against each stored group's native handle.
    // We don't go through `classify_group` here because we don't have
    // a `TreeItemId` wrapper yet — the hit-test message returns only
    // the native handle.
    let group = {
        let items = package_items.borrow();
        if items
            .packages_group
            .as_ref()
            .is_some_and(|g| native_tree_handle(g) == h_item)
        {
            Some(WhichGroup::Packages)
        } else if items
            .configuration_group
            .as_ref()
            .is_some_and(|g| native_tree_handle(g) == h_item)
        {
            Some(WhichGroup::Configuration)
        } else {
            None
        }
    };
    let Some(group) = group else {
        return;
    };

    propagate_group_toggle_to_leaves(
        tree,
        package_items,
        group,
        package_rows,
        configuration_rows,
        wizard_model,
    );
    refresh_after_packages_toggle(
        tree,
        package_items,
        package_rows,
        configuration_rows,
        can_install,
        wizard_model,
        details,
        osara_checkbox,
        osara_note,
    );
}

/// Windows-only: refresh both groups' tristate visuals + plan-level
/// `can_install` flag + OSARA widgets + details pane after any toggle
/// (leaf or group) that mutated `package_rows` or
/// `configuration_rows`. Also re-evaluates configuration-row
/// availability against the latest package plan so that, e.g.,
/// unchecking ReaPack greys out the REAPER Accessibility row in real
/// time.
#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
fn refresh_after_packages_toggle(
    tree: &TreeCtrl,
    package_items: &PackagesStateCell,
    package_rows: &Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: &Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    can_install: &Rc<Cell<bool>>,
    wizard_model: &WizardModel,
    details: &TextCtrl,
    osara_checkbox: &CheckBox,
    osara_note: &TextCtrl,
) {
    // Configuration rows depend on the package plan (e.g. ReaPack must
    // be installed/queued for the REAPER Accessibility step). Re-evaluate
    // before refreshing the tree so the leaves' selected/state-image
    // values match what's in `configuration_rows`.
    if let Ok(localizer) = localizer_from_options(&wizard_model.bootstrap_options) {
        // None for the resource-path argument: a package toggle can't
        // change `reapack.ini`, so preserve each row's existing
        // `already_applied` flag rather than re-reading from disk on
        // every click.
        recompute_configuration_row_availability(
            &localizer,
            &package_rows.borrow(),
            None,
            &mut configuration_rows.borrow_mut(),
        );
        // Push the recomputed leaf states into the tree visual so the
        // user sees the live re-evaluation.
        let items = package_items.borrow();
        let configuration_rows_borrowed = configuration_rows.borrow();
        for (idx, leaf) in items.configuration_leaves.iter().enumerate() {
            let Some(row) = configuration_rows_borrowed.get(idx) else {
                continue;
            };
            native_tree_checkboxes::set_check_state(
                tree.get_handle(),
                leaf,
                row.selected && row.available_for_target && !row.already_applied,
            );
            let label = format_row_label(&row.summary, row.selected);
            tree.set_item_text(leaf, &label);
        }
    }

    {
        let items = package_items.borrow();
        if let Some(group) = items.packages_group.as_ref() {
            let group_state = compute_packages_group_tristate(&package_rows.borrow());
            native_tree_checkboxes::set_check_state_tri(tree.get_handle(), group, group_state);
        }
        if let Some(group) = items.configuration_group.as_ref() {
            let group_state = compute_configuration_group_tristate(&configuration_rows.borrow());
            native_tree_checkboxes::set_check_state_tri(tree.get_handle(), group, group_state);
        }
    }

    let any_install_or_update = package_rows.borrow().iter().any(|row| {
        row.available_for_target
            && matches!(row.action, PlanActionKind::Install | PlanActionKind::Update)
    });
    can_install.set(any_install_or_update);

    if let Some(selected) = tree.get_selection() {
        match classify_leaf(&package_items.borrow(), &selected) {
            Some(WhichLeaf::Packages(idx)) => {
                if let Some(row) = package_rows.borrow().get(idx) {
                    details.set_value(&package_details(row));
                }
            }
            Some(WhichLeaf::Configuration(idx)) => {
                if let Some(row) = configuration_rows.borrow().get(idx) {
                    details.set_value(&row.details);
                }
            }
            None => {}
        }
    }

    sync_osara_keymap_widgets(
        wizard_model,
        &package_rows.borrow(),
        osara_checkbox,
        osara_note,
    );
}

/// Windows-only: implement the parent-checkbox propagation for the
/// requested group.
///
/// Convention (matches Windows Explorer / Visual Studio Installer):
/// - clicking a fully-checked parent → uncheck all available children;
/// - clicking an unchecked or mixed parent → check all available children.
///
/// Package leaves route mutations through `apply_checkbox_state_to_package_row`
/// so action labels flip Install / Update / Keep; configuration leaves
/// just flip `row.selected`. Unavailable rows in either group are left
/// untouched.
#[cfg(target_os = "windows")]
fn propagate_group_toggle_to_leaves(
    tree: &TreeCtrl,
    package_items: &PackagesStateCell,
    target_group: WhichGroup,
    package_rows: &Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: &Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    wizard_model: &WizardModel,
) {
    match target_group {
        WhichGroup::Packages => {
            let pre_state = compute_packages_group_tristate(&package_rows.borrow());
            let target = !matches!(pre_state, native_tree_checkboxes::TriState::Checked);
            let leaves: Vec<TreeItemId> = package_items
                .borrow()
                .packages_leaves
                .iter()
                .map(|leaf| leaf.clone())
                .collect();
            {
                let mut rows = package_rows.borrow_mut();
                for row in rows.iter_mut().filter(|r| r.available_for_target) {
                    let _ = apply_checkbox_state_to_package_row(wizard_model, row, target);
                }
            }
            let rows = package_rows.borrow();
            let hwnd = tree.get_handle();
            for (idx, leaf) in leaves.iter().enumerate() {
                let Some(row) = rows.get(idx) else { continue };
                if row.available_for_target {
                    native_tree_checkboxes::set_check_state(hwnd, leaf, row.selected);
                    let label = format_row_label(&row.summary, row.selected);
                    tree.set_item_text(leaf, &label);
                }
            }
        }
        WhichGroup::Configuration => {
            let pre_state = compute_configuration_group_tristate(&configuration_rows.borrow());
            let target = !matches!(pre_state, native_tree_checkboxes::TriState::Checked);
            let leaves: Vec<TreeItemId> = package_items
                .borrow()
                .configuration_leaves
                .iter()
                .map(|leaf| leaf.clone())
                .collect();
            {
                let mut rows = configuration_rows.borrow_mut();
                for row in rows
                    .iter_mut()
                    .filter(|r| r.available_for_target && !r.already_applied)
                {
                    row.selected = target;
                }
            }
            let rows = configuration_rows.borrow();
            let hwnd = tree.get_handle();
            for (idx, leaf) in leaves.iter().enumerate() {
                let Some(row) = rows.get(idx) else { continue };
                if row.available_for_target && !row.already_applied {
                    native_tree_checkboxes::set_check_state(hwnd, leaf, row.selected);
                    let label = format_row_label(&row.summary, row.selected);
                    tree.set_item_text(leaf, &label);
                }
            }
        }
    }
}

// ===========================================================================
// Non-Windows: wxDataViewCtrl + CustomDataViewTreeModel.
//
// Windows is special-cased via `TVS_CHECKBOXES`; on macOS and GTK the
// equivalent native pattern is "outline view with a check column" — i.e.
// wxDataView with `DataViewToggleRenderer` over `VariantType::Bool`. The
// model carries one synthetic Group node + one leaf per `PackageRow`, the
// toggle column gets `Activatable` mode so Space + click both route through
// `set_value`, and `is_enabled` returns false for unavailable rows so the
// platform draws (and exposes) them as disabled.
// ===========================================================================

/// Non-Windows: build the Packages page using a wxDataViewCtrl driven by a
/// `CustomDataViewTreeModel`. The model exposes a synthetic Packages group
/// + one leaf per `PackageRow`; column 0 is a Bool toggle, column 1 is the
/// row label (the column with the expander triangle). The model's
/// `set_value` callback owns all the toggle side effects.
#[cfg(not(target_os = "windows"))]
fn build_packages_page(
    page: &Panel,
    model: &WizardModel,
    package_rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    package_items: PackagesStateCell,
    can_install: Rc<Cell<bool>>,
) -> (PackagesView, TextCtrl, CheckBox, TextCtrl) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.packages_heading,
        "rabbit-packages-heading",
    );
    add_label(
        page,
        &sizer,
        &model.text.packages_list_label,
        "rabbit-packages-list-label",
    );

    let tree = DataViewCtrl::builder(page)
        .with_style(DataViewStyle::Single | DataViewStyle::RowLines | DataViewStyle::NoHeader)
        .with_size(Size::new(-1, 220))
        .build();
    tree.set_name("rabbit-package-list");

    // The model is constructed BEFORE associate_model so wx's internal
    // refcount stays sane. `package_items` (the model handle cell) gets
    // populated immediately afterwards so set_value's notification path
    // can find the model the next time the user toggles a row.
    let tree_data = PackageTreeData::new(
        Rc::clone(&package_rows),
        Rc::clone(&configuration_rows),
        model.text.packages_tree_group_label.clone(),
        model.text.configuration_tree_group_label.clone(),
    );
    let dv_model = build_packages_tree_model(
        tree_data,
        Rc::clone(&package_rows),
        Rc::clone(&configuration_rows),
        Rc::clone(&package_items),
        Rc::clone(&can_install),
        model.clone(),
    );
    *package_items.borrow_mut() = Some(dv_model.clone());

    let toggle_renderer = DataViewToggleRenderer::new(
        VariantType::Bool,
        DataViewCellMode::Activatable,
        DataViewAlign::Center,
    );
    let toggle_column = DataViewColumn::new(
        "",
        &toggle_renderer,
        PACKAGE_COL_TOGGLE as usize,
        28,
        DataViewAlign::Center,
        DataViewColumnFlags::DefaultNone,
    );
    tree.append_column(&toggle_column);

    let text_renderer = DataViewTextRenderer::new(
        VariantType::String,
        DataViewCellMode::Inert,
        DataViewAlign::Left,
    );
    let text_column = DataViewColumn::new(
        "",
        &text_renderer,
        PACKAGE_COL_LABEL as usize,
        -1,
        DataViewAlign::Left,
        DataViewColumnFlags::Resizable,
    );
    tree.append_column(&text_column);

    tree.associate_model(&dv_model);

    expand_packages_group(&tree, &dv_model);
    sizer.add(&tree, 1, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.package_details_label,
        "rabbit-package-details-label",
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
    details.set_name("rabbit-package-details");
    sizer.add(&details, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.packages_osara_keymap_heading,
        "rabbit-osara-keymap-heading",
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
    osara_keymap_note.set_name("rabbit-osara-keymap-note");
    osara_keymap_note.enable(false);
    osara_keymap_note.set_can_focus(false);
    sizer.add(&osara_keymap_note, 0, SizerFlag::All | SizerFlag::Expand, 6);

    sync_osara_keymap_widgets(
        model,
        &package_rows.borrow(),
        &osara_keymap_replace,
        &osara_keymap_note,
    );

    {
        let package_rows = Rc::clone(&package_rows);
        let model_text = model.clone();
        let details = details;
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        tree.on_selection_changed(move |event| {
            if let Some(item) = event.get_item() {
                if let Some(node_ptr) = item.get_id::<Node>() {
                    if !node_ptr.is_null() {
                        // SAFETY: node_ptr originated from a Box<Node>
                        // owned by the model's userdata; the model lives
                        // for as long as this closure can fire.
                        let node = unsafe { &*node_ptr };
                        if let NodeKind::Package(idx) = node.kind {
                            if let Some(value) = package_rows.borrow().get(idx).map(package_details)
                            {
                                details.set_value(&value);
                            }
                        }
                    }
                }
            }
            sync_osara_keymap_widgets(
                &model_text,
                &package_rows.borrow(),
                &osara_checkbox,
                &osara_note,
            );
        });
    }

    {
        let model_text = model.clone();
        let rows = Rc::clone(&package_rows);
        let osara_checkbox = osara_keymap_replace;
        let osara_note = osara_keymap_note;
        osara_keymap_replace.on_toggled(move |_| {
            sync_osara_keymap_widgets(&model_text, &rows.borrow(), &osara_checkbox, &osara_note);
        });
    }

    page.set_sizer(sizer, true);
    (tree, details, osara_keymap_replace, osara_keymap_note)
}

/// Non-Windows: build the `CustomDataViewTreeModel` that backs the packages
/// tree. The closures capture clones of `package_rows`, `package_items`
/// (the self-referential model handle cell), `can_install`, and the wizard
/// model, so `set_value` can mutate row state, fire item-changed
/// notifications and recompute downstream UI flags without going through
/// any external lookup.
#[cfg(not(target_os = "windows"))]
fn build_packages_tree_model(
    data: PackageTreeData,
    rows: Rc<RefCell<Vec<crate::PackageRow>>>,
    configuration_rows: Rc<RefCell<Vec<crate::ConfigurationRow>>>,
    model_cell: PackagesStateCell,
    can_install: Rc<Cell<bool>>,
    wizard_model: WizardModel,
) -> CustomDataViewTreeModel {
    type CompareFn = fn(&PackageTreeData, &Node, &Node, u32, bool) -> i32;

    let rows_for_get_value = Rc::clone(&rows);
    let rows_for_set_value = Rc::clone(&rows);
    let rows_for_is_enabled = Rc::clone(&rows);
    let configuration_rows_for_get_value = Rc::clone(&configuration_rows);
    let configuration_rows_for_set_value = Rc::clone(&configuration_rows);
    let configuration_rows_for_is_enabled = Rc::clone(&configuration_rows);
    let configuration_rows_for_recompute = Rc::clone(&configuration_rows);
    let wizard_model_for_recompute = wizard_model.clone();
    let model_cell_for_set_value = Rc::clone(&model_cell);

    CustomDataViewTreeModel::new(
        data,
        // get_parent
        |data: &PackageTreeData, item: Option<&Node>| -> Option<*mut Node> {
            match item {
                None => None,
                Some(node) => match node.kind {
                    NodeKind::PackagesGroup | NodeKind::ConfigurationGroup => None,
                    NodeKind::Package(_) => Some(data.packages_group_ptr() as *mut Node),
                    NodeKind::Configuration(_) => Some(data.configuration_group_ptr() as *mut Node),
                },
            }
        },
        // is_container
        |_data: &PackageTreeData, item: Option<&Node>| -> bool {
            match item {
                None => true,
                Some(node) => matches!(
                    node.kind,
                    NodeKind::PackagesGroup | NodeKind::ConfigurationGroup
                ),
            }
        },
        // get_children
        |data: &PackageTreeData, item: Option<&Node>| -> Vec<*mut Node> {
            match item {
                None => vec![
                    data.packages_group_ptr() as *mut Node,
                    data.configuration_group_ptr() as *mut Node,
                ],
                Some(node) => match node.kind {
                    NodeKind::PackagesGroup => data
                        .all_package_ptrs()
                        .into_iter()
                        .map(|p| p as *mut Node)
                        .collect(),
                    NodeKind::ConfigurationGroup => data
                        .all_configuration_ptrs()
                        .into_iter()
                        .map(|p| p as *mut Node)
                        .collect(),
                    NodeKind::Package(_) | NodeKind::Configuration(_) => Vec::new(),
                },
            }
        },
        // get_value
        move |data: &PackageTreeData, item: Option<&Node>, col: u32| -> Variant {
            let Some(node) = item else {
                return Variant::from_string("");
            };
            match node.kind {
                NodeKind::PackagesGroup => {
                    if col == PACKAGE_COL_TOGGLE {
                        // Aggregate state: true only if every available row
                        // is selected. The standard toggle renderer can't
                        // show a tristate, so a partially-selected group
                        // reads as unchecked.
                        let rows = rows_for_get_value.borrow();
                        let mut any_available = false;
                        let all_checked = rows
                            .iter()
                            .filter(|r| r.available_for_target)
                            .inspect(|_| any_available = true)
                            .all(|r| r.selected);
                        Variant::from_bool(any_available && all_checked)
                    } else {
                        Variant::from_string(&data.packages_group_label)
                    }
                }
                NodeKind::Package(idx) => {
                    let rows = rows_for_get_value.borrow();
                    let Some(row) = rows.get(idx) else {
                        return Variant::from_string("");
                    };
                    if col == PACKAGE_COL_TOGGLE {
                        Variant::from_bool(row.selected)
                    } else {
                        Variant::from_string(&row.summary)
                    }
                }
                NodeKind::ConfigurationGroup => {
                    if col == PACKAGE_COL_TOGGLE {
                        let cfg_rows = configuration_rows_for_get_value.borrow();
                        let mut any_available = false;
                        let all_checked = cfg_rows
                            .iter()
                            .filter(|r| r.available_for_target)
                            .inspect(|_| any_available = true)
                            .all(|r| r.selected);
                        Variant::from_bool(any_available && all_checked)
                    } else {
                        Variant::from_string(&data.configuration_group_label)
                    }
                }
                NodeKind::Configuration(idx) => {
                    let cfg_rows = configuration_rows_for_get_value.borrow();
                    let Some(row) = cfg_rows.get(idx) else {
                        return Variant::from_string("");
                    };
                    if col == PACKAGE_COL_TOGGLE {
                        Variant::from_bool(row.selected)
                    } else {
                        Variant::from_string(&row.summary)
                    }
                }
            }
        },
        // set_value
        Some(
            move |data: &PackageTreeData, item: Option<&Node>, col: u32, var: &Variant| -> bool {
                if col != PACKAGE_COL_TOGGLE {
                    return false;
                }
                let Some(node) = item else {
                    return false;
                };
                let new_state = var.get_bool().unwrap_or(false);

                match node.kind {
                    NodeKind::PackagesGroup => {
                        // Group toggle propagates to every available leaf;
                        // unavailable rows stay untouched so the install
                        // plan never carries something we can't honor.
                        let mut rows = rows_for_set_value.borrow_mut();
                        for row in rows.iter_mut() {
                            if row.available_for_target {
                                let _ = apply_checkbox_state_to_package_row(
                                    &wizard_model,
                                    row,
                                    new_state,
                                );
                            }
                        }
                    }
                    NodeKind::Package(idx) => {
                        let mut rows = rows_for_set_value.borrow_mut();
                        let Some(row) = rows.get_mut(idx) else {
                            return false;
                        };
                        if !row.available_for_target {
                            return false;
                        }
                        let _ = apply_checkbox_state_to_package_row(&wizard_model, row, new_state);
                    }
                    NodeKind::ConfigurationGroup => {
                        let mut cfg_rows = configuration_rows_for_set_value.borrow_mut();
                        for row in cfg_rows.iter_mut() {
                            if row.available_for_target && !row.already_applied {
                                row.selected = new_state;
                            }
                        }
                    }
                    NodeKind::Configuration(idx) => {
                        let mut cfg_rows = configuration_rows_for_set_value.borrow_mut();
                        let Some(row) = cfg_rows.get_mut(idx) else {
                            return false;
                        };
                        if !row.available_for_target || row.already_applied {
                            return false;
                        }
                        row.selected = new_state;
                    }
                }

                let any_install_or_update = rows_for_set_value.borrow().iter().any(|row| {
                    row.available_for_target
                        && matches!(row.action, PlanActionKind::Install | PlanActionKind::Update)
                });
                can_install.set(any_install_or_update);

                // Recompute configuration row availability whenever a
                // package toggle could have flipped a dependency state.
                let recomputed_configuration =
                    matches!(node.kind, NodeKind::PackagesGroup | NodeKind::Package(_));
                if recomputed_configuration {
                    if let Ok(localizer) =
                        crate::localizer_from_options(&wizard_model_for_recompute.bootstrap_options)
                    {
                        let package_rows_snapshot = rows_for_set_value.borrow();
                        let mut cfg_rows = configuration_rows_for_recompute.borrow_mut();
                        // None for the resource-path argument: a package
                        // toggle can't change `reapack.ini`, so preserve
                        // each row's existing `already_applied` flag.
                        crate::recompute_configuration_row_availability(
                            &localizer,
                            &package_rows_snapshot,
                            None,
                            &mut cfg_rows,
                        );
                    }
                }

                // Push the cell changes back into the view. SetValue's
                // true return only auto-refreshes the (item, col) we set;
                // we also need to refresh the row's label cell (the action
                // text flips Install/Update/Keep) and the parent group's
                // aggregate cell.
                if let Some(model) = model_cell_for_set_value.borrow().as_ref() {
                    match node.kind {
                        NodeKind::PackagesGroup => {
                            let parent_ptr = data.packages_group_ptr();
                            let leaf_ptrs = data.all_package_ptrs();
                            model.items_changed(&leaf_ptrs);
                            model.item_value_changed(parent_ptr, PACKAGE_COL_TOGGLE);
                        }
                        NodeKind::Package(idx) => {
                            let leaf_ptr = data.package_ptr(idx);
                            model.item_value_changed(leaf_ptr, PACKAGE_COL_LABEL);
                            model.item_value_changed(data.packages_group_ptr(), PACKAGE_COL_TOGGLE);
                        }
                        NodeKind::ConfigurationGroup => {
                            let parent_ptr = data.configuration_group_ptr();
                            let leaf_ptrs = data.all_configuration_ptrs();
                            model.items_changed(&leaf_ptrs);
                            model.item_value_changed(parent_ptr, PACKAGE_COL_TOGGLE);
                        }
                        NodeKind::Configuration(idx) => {
                            let leaf_ptr = data.configuration_ptr(idx);
                            model.item_value_changed(leaf_ptr, PACKAGE_COL_LABEL);
                            model.item_value_changed(
                                data.configuration_group_ptr(),
                                PACKAGE_COL_TOGGLE,
                            );
                        }
                    }

                    if recomputed_configuration {
                        let cfg_leaf_ptrs = data.all_configuration_ptrs();
                        model.items_changed(&cfg_leaf_ptrs);
                        model
                            .item_value_changed(data.configuration_group_ptr(), PACKAGE_COL_TOGGLE);
                    }
                }

                true
            },
        ),
        // is_enabled — gray out the checkbox + label of unavailable rows.
        Some(
            move |_data: &PackageTreeData, item: Option<&Node>, _col: u32| -> bool {
                let Some(node) = item else {
                    return true;
                };
                match node.kind {
                    NodeKind::PackagesGroup | NodeKind::ConfigurationGroup => true,
                    NodeKind::Package(idx) => rows_for_is_enabled
                        .borrow()
                        .get(idx)
                        .map(|row| row.available_for_target)
                        .unwrap_or(true),
                    NodeKind::Configuration(idx) => configuration_rows_for_is_enabled
                        .borrow()
                        .get(idx)
                        .map(|row| row.available_for_target && !row.already_applied)
                        .unwrap_or(true),
                }
            },
        ),
        // compare — left at None semantically; the explicit type is needed
        // because the closure-based `Option<CMP>` pattern doesn't infer
        // without it.
        None::<CompareFn>,
    )
}

/// Non-Windows: expand both synthetic group nodes ("Packages" and
/// "Configuration") so all leaves are visible without an extra click.
/// Reads the group pointers from the model's userdata so the model
/// owns the canonical Node addresses.
#[cfg(not(target_os = "windows"))]
fn expand_packages_group(tree: &PackagesView, model: &CustomDataViewTreeModel) {
    let mut packages_group_ptr: *const Node = std::ptr::null();
    let mut configuration_group_ptr: *const Node = std::ptr::null();
    model.with_userdata_mut::<PackageTreeData, ()>(|data| {
        packages_group_ptr = data.packages_group_ptr();
        configuration_group_ptr = data.configuration_group_ptr();
    });
    for ptr in [packages_group_ptr, configuration_group_ptr] {
        if ptr.is_null() {
            continue;
        }
        let item = wxdragon::widgets::dataview::DataViewItem::from_id_ptr(ptr);
        if item.is_ok() {
            tree.expand(&item);
        }
    }
}

/// Non-Windows: replace the row set inside the live
/// `CustomDataViewTreeModel`. Reuses the existing model + control
/// association so nothing has to be rewired; the model just gets new
/// userdata, then we tell the view that everything has changed via
/// `cleared()`. After cleared() the control re-queries the model for
/// visible items and the previously-selected row drops away (caller
/// resets `package_details` to the first row).
#[cfg(not(target_os = "windows"))]
fn rebuild_packages_tree_model(
    tree: &PackagesView,
    package_items: &PackagesStateCell,
    model: &WizardModel,
    package_rows: &[PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    let Some(dv_model) = package_items.borrow().as_ref().cloned() else {
        return;
    };
    let packages_group_label = model.text.packages_tree_group_label.clone();
    let configuration_group_label = model.text.configuration_tree_group_label.clone();
    dv_model.with_userdata_mut::<PackageTreeData, ()>(|data| {
        // Sync the shared Rc<RefCell<Vec<_>>>s in case the caller
        // hasn't pre-replaced them (the post-install hook does, the
        // version-check finish handler also does — be defensive in
        // case a future caller forgets).
        let pkg_len = package_rows.len();
        if data.rows.borrow().len() != pkg_len {
            *data.rows.borrow_mut() = package_rows.to_vec();
        }
        let cfg_len = configuration_rows.len();
        if data.configuration_rows.borrow().len() != cfg_len {
            *data.configuration_rows.borrow_mut() = configuration_rows.to_vec();
        }
        data.packages_group_label = packages_group_label;
        data.configuration_group_label = configuration_group_label;
        data.package_nodes = (0..pkg_len)
            .map(|i| {
                Box::new(Node {
                    kind: NodeKind::Package(i),
                })
            })
            .collect();
        data.configuration_nodes = (0..cfg_len)
            .map(|i| {
                Box::new(Node {
                    kind: NodeKind::Configuration(i),
                })
            })
            .collect();
    });
    dv_model.cleared();
    // wxDataViewCtrl auto-collapses the groups on Cleared; re-expand so
    // the user sees the leaves immediately.
    expand_packages_group(tree, &dv_model);
}

#[cfg(not(target_os = "windows"))]
fn refresh_package_checklist(
    tree: &PackagesView,
    package_items: &PackagesStateCell,
    details: &TextCtrl,
    osara_keymap_replace: &CheckBox,
    osara_keymap_note: &TextCtrl,
    model: &WizardModel,
    rows: &[crate::PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    rebuild_packages_tree_model(tree, package_items, model, rows, configuration_rows);
    details.set_value(&rows.first().map(package_details).unwrap_or_default());
    sync_osara_keymap_widgets(model, rows, osara_keymap_replace, osara_keymap_note);
}

#[cfg(not(target_os = "windows"))]
fn rebuild_package_list_widgets(
    widgets: &WizardWidgets,
    package_items: &PackagesStateCell,
    model: &WizardModel,
    package_rows: &[PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    rebuild_packages_tree_model(
        &widgets.package_checklist,
        package_items,
        model,
        package_rows,
        configuration_rows,
    );
    let initial = package_rows
        .first()
        .map(package_details)
        .unwrap_or_default();
    widgets.package_details.set_value(&initial);
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
        "rabbit-version-check-heading",
    );
    let status = StaticText::builder(page)
        .with_label(&model.text.version_check_status_pending)
        .build();
    status.set_name("rabbit-version-check-status");
    sizer.add(&status, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.version_check_progress_label,
        "rabbit-version-check-progress-label",
    );
    let gauge = Gauge::builder(page)
        .with_range(package_count.max(1))
        .build();
    gauge.set_name("rabbit-version-check-progress");
    sizer.add(&gauge, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let error_heading = StaticText::builder(page)
        .with_label(&model.text.version_check_error_heading)
        .build();
    error_heading.set_name("rabbit-version-check-error-heading");
    sizer.add(&error_heading, 0, SizerFlag::All | SizerFlag::Expand, 6);
    let error_log = TextCtrl::builder(page)
        .with_value("")
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    error_log.set_name("rabbit-version-check-error-log");
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
        "rabbit-reapack-ack-heading",
    );
    let body = TextCtrl::builder(page)
        .with_value(&model.text.reapack_ack_body)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .with_size(Size::new(-1, 120))
        .build();
    body.set_name("rabbit-reapack-ack-body");
    sizer.add(&body, 0, SizerFlag::All | SizerFlag::Expand, 6);

    let donate_link = Button::builder(page)
        .with_label(&model.text.reapack_ack_link_label)
        .build();
    donate_link.set_name("rabbit-reapack-ack-donate-link");
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
    // Mirror the OSARA-keymap / done-page CheckBox pattern: on this
    // wxdragon version the accessible name is driven by the wxWindow
    // *name* on Windows, not the visible `with_label` argument, so set
    // both `name` and `label` to the localized string. Without this the
    // screen reader announces the literal Fluent key
    // (`rabbit-reapack-ack-confirm`) instead of the translated label.
    confirm.set_name(&model.text.reapack_ack_confirm_label);
    confirm.set_label(&model.text.reapack_ack_confirm_label);
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
        "rabbit-review-heading",
    );
    let review = TextCtrl::builder(page)
        .with_value(&model.review_lines.join("\n"))
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    review.set_name("rabbit-review-text");
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
        "rabbit-progress-heading",
    );
    let status = StaticText::builder(page)
        .with_label(&model.text.progress_status)
        .build();
    status.set_name("rabbit-progress-status");
    sizer.add(&status, 0, SizerFlag::All | SizerFlag::Expand, 6);
    let gauge = Gauge::builder(page).with_range(100).build();
    gauge.set_name("rabbit-progress-gauge");
    sizer.add(&gauge, 0, SizerFlag::All | SizerFlag::Expand, 6);

    add_label(
        page,
        &sizer,
        &model.text.progress_details_label,
        "rabbit-progress-details-label",
    );
    let details = TextCtrl::builder(page)
        .with_value(&model.text.progress_details_idle)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    details.set_name("rabbit-progress-details");
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    page.set_sizer(sizer, true);
    (status, gauge, details)
}

fn build_done_page(page: &Panel, model: &WizardModel) -> (TextCtrl, TextCtrl, Button, Button) {
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    add_heading(
        page,
        &sizer,
        &model.text.done_heading,
        "rabbit-done-heading",
    );
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
    status.set_name("rabbit-done-status");
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
    details.set_name("rabbit-done-details");
    details.hide();
    sizer.add(&details, 1, SizerFlag::All | SizerFlag::Expand, 6);

    let toggle_details = details;
    let toggle_page = page.clone();
    show_details.on_toggled(move |event| {
        let visible = event.is_checked();
        toggle_details.show(visible);
        toggle_page.layout();
        // Move keyboard focus into the details TextCtrl as soon as the
        // user reveals it. Screen readers (NVDA, JAWS) announce the
        // newly-focused control, which both confirms the checkbox click
        // and reads out the install report without the user having to
        // hunt for it via Tab.
        if visible {
            toggle_details.set_focus();
        }
    });

    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    actions.add_stretch_spacer(1);

    let launch_reaper = Button::builder(page)
        .with_label(&model.text.done_launch_reaper_label)
        .build();
    launch_reaper.set_name("rabbit-done-launch-reaper");
    launch_reaper.add_style(WindowStyle::TabStop);
    launch_reaper.set_can_focus(true);
    launch_reaper.enable(false);
    actions.add(&launch_reaper, 0, SizerFlag::All, 6);

    let open_resource = Button::builder(page)
        .with_label(&model.text.done_open_resource_label)
        .build();
    open_resource.set_name("rabbit-done-open-resource");
    open_resource.add_style(WindowStyle::TabStop);
    open_resource.set_can_focus(true);
    open_resource.enable(false);
    actions.add(&open_resource, 0, SizerFlag::All, 6);

    sizer.add_sizer(&actions, 0, SizerFlag::All | SizerFlag::Expand, 0);
    page.set_sizer(sizer, true);
    (status, details, launch_reaper, open_resource)
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
    portable_folder: &TextCtrl,
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

fn checked_package_indices(rows: &[PackageRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.selected)
        .map(|(index, _)| index)
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

/// Windows: re-render the native TreeCtrl after a row replacement.
#[cfg(target_os = "windows")]
fn refresh_package_checklist(
    tree: &PackagesView,
    package_items: &PackagesStateCell,
    details: &TextCtrl,
    osara_keymap_replace: &CheckBox,
    osara_keymap_note: &TextCtrl,
    model: &WizardModel,
    rows: &[crate::PackageRow],
    configuration_rows: &[ConfigurationRow],
) {
    populate_packages_tree(tree, package_items, model, rows, configuration_rows);
    details.set_value(&rows.first().map(package_details).unwrap_or_default());
    sync_osara_keymap_widgets(model, rows, osara_keymap_replace, osara_keymap_note);
}

fn sync_osara_keymap_widgets(
    model: &WizardModel,
    rows: &[crate::PackageRow],
    checkbox: &CheckBox,
    note: &TextCtrl,
) {
    let selected_indices = checked_package_indices(rows);
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

fn portable_folder_path(portable_folder: &TextCtrl) -> Option<PathBuf> {
    let path = portable_folder.get_value();
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn portable_target_details(model: &WizardModel, portable_folder: &TextCtrl) -> String {
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
        widgets.portable_folder.on_text_changed(move |_| {
            if current_step.load(Ordering::SeqCst) == TARGET_STEP {
                next.enable(target_is_valid(&model, &widgets));
            }
        });
    }
}

fn configure_portable_folder(
    portable_folder: &TextCtrl,
    portable_folder_browse: &Button,
    enabled: bool,
) {
    portable_folder.enable(enabled);
    portable_folder.set_can_focus(enabled);
    portable_folder_browse.enable(enabled);
    portable_folder_browse.set_can_focus(enabled);
}

fn set_last_report(
    state: &Arc<Mutex<Option<WizardOutcomeReport>>>,
    report: Option<WizardOutcomeReport>,
) {
    if let Ok(mut slot) = state.lock() {
        *slot = report;
    }
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
            architecture: rabbit_core::model::Architecture::current(),
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
    language_footer: &Panel,
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
    // Language picker only matters on the Target step — switching languages
    // relaunches RABBIT and discards wizard progress, so a footer on later
    // pages would just be a tripwire.
    language_footer.show(step == TARGET_STEP);
}
