//! `jafiz-gui.exe` — the GUI front-end (GUI subsystem, launched by the
//! Foundry32 hub). Windows Classic chrome over the same engine the CLI uses.
//!
//! Two lists side by side: scenarios on the left with their rolled-up status,
//! the selected scenario's steps on the right. The tester marks each step as
//! they go, and the current-step marker advances on its own — the whole point
//! is that recording a step costs one click while both hands are on the app
//! under test.
//!
//! Everything is synchronous: parsing a suite and writing a small JSON are
//! sub-millisecond, so unlike WITN there is no scanner thread. The only
//! threads are the modal dialogs, which follow the nwg multithread pattern
//! (see crates/mcp-console/src/gui/preferences_dialog.rs).
//!
//! This module is behaviour only. How the window is built, laid out and
//! captioned lives in `chrome`.

#![windows_subsystem = "windows"]

mod chrome;
mod history_dialog;
mod i18n;
mod note_dialog;
mod preferences_dialog;
mod run_dialog;

use i18n::{t, Lang, T};

use native_windows_gui as nwg;
pub(crate) use foundry_common::ui::{ensure_visible, select_only};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use jafiz::model::{display_symbol, scenario_done, tally, Run, RunFile, StepStatus, Suite};
use jafiz::parser::Diagnostic;
use jafiz::report;
use jafiz::runs;
use jafiz::settings::AppSettings;
use jafiz::store::{self, LocationKind};

/// Shown in the About box.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the format guide is dropped for whatever the user reads Markdown with.
const GUIDE_FILE: &str = "jafiz-formato.md";

// The verdict keys. F1 is help and F5 is reload by Windows convention, so the
// four verdicts take what is left around them — and the button captions say so.
const VK_F2: u32 = 0x71;
const VK_F3: u32 = 0x72;
const VK_F4: u32 = 0x73;
const VK_F5: u32 = 0x74;
const VK_F6: u32 = 0x75;

/// Mailbox the modal dialog threads fill; drained on the UI thread when the
/// Notice fires. `pub(crate)` because the dialog modules write into it.
///
/// Every slot spells "cancelled" as `Some(None)`, and only one dialog can be
/// open at a time (the main window is disabled while one is up), so the four
/// slots never contend.
#[derive(Default)]
pub(crate) struct Shared {
    /// `Some(None)` means the dialog was cancelled.
    pub environment: Option<Option<String>>,
    pub note: Option<Option<String>>,
    /// The run id picked in Histórico.
    pub history: Option<Option<String>>,
    /// The language picked in Preferências.
    pub preferences: Option<Option<Lang>>,
}

/// Which `Shared` slot a dialog answers through.
#[derive(Clone, Copy)]
pub(crate) enum DialogSlot {
    Environment,
    Note,
    History,
    Preferences,
}

impl DialogSlot {
    /// Writes "cancelled" into this dialog's slot.
    fn cancel(self, shared: &mut Shared) {
        match self {
            DialogSlot::Environment => shared.environment = Some(None),
            DialogSlot::Note => shared.note = Some(None),
            DialogSlot::History => shared.history = Some(None),
            DialogSlot::Preferences => shared.preferences = Some(None),
        }
    }
}

/// Last-resort notifier for a dialog thread that unwinds before it answers.
///
/// Every dialog builds its window with `.expect(…)`, and the event handler that
/// would post the answer is installed last. A panic anywhere before that leaves
/// no outcome, no `Notice` and therefore no `drain_dialog` — and the main
/// window, disabled just before `spawn`, stays `WS_DISABLED` forever: the user
/// cannot even close it. Held on the dialog thread's stack from its first line,
/// this posts a cancellation on the way out if `send_outcome` never ran.
///
/// The flag is the very `Cell` the handler's `send_outcome` sets, so the
/// exactly-once rule still holds: on the normal path the guard finds it already
/// true and does nothing. It cannot help a release build — the workspace's
/// release profile is `panic = "abort"`, which takes the process down before
/// any destructor runs — but it covers dev builds and every non-panicking early
/// return out of a dialog thread.
pub(crate) struct DialogGuard {
    sent: Rc<Cell<bool>>,
    slot: DialogSlot,
    shared: Arc<Mutex<Shared>>,
    notify: nwg::NoticeSender,
}

impl DialogGuard {
    /// Arms the guard. `sent` must be the same flag the dialog's `send_outcome`
    /// sets, or the guard would fire behind a dialog that already answered.
    pub(crate) fn new(
        sent: Rc<Cell<bool>>,
        slot: DialogSlot,
        shared: Arc<Mutex<Shared>>,
        notify: nwg::NoticeSender,
    ) -> DialogGuard {
        DialogGuard { sent, slot, shared, notify }
    }
}

impl Drop for DialogGuard {
    fn drop(&mut self) {
        if self.sent.replace(true) {
            return; // the dialog answered for itself
        }
        // Nothing in here may panic: this can run while the thread is already
        // unwinding, and a panic during unwind aborts the process. That is why
        // the mutex is taken back out of a poisoned lock instead of unwrapped —
        // the panic that poisoned it is very likely the one being handled.
        let mut shared = match self.shared.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.slot.cancel(&mut shared);
        drop(shared);
        self.notify.notice();
    }
}

/// What the environment dialog's answer applies to. `Nova execução…` and
/// `Ambiente…` are the same window and answer through the same mailbox slot,
/// so the main window records which of the two it asked for before spawning
/// the thread — reading it back off the menu handle is not possible once the
/// answer arrives on a `Notice`.
#[derive(Clone, Copy)]
pub(crate) enum EnvTarget {
    /// Start a run with the answer.
    NewRun,
    /// Re-describe the environment of the run already open.
    ActiveRun,
}

pub(crate) struct UiState {
    lang: Lang,
    settings: AppSettings,
    location: PathBuf,
    location_kind: LocationKind,
    /// Suite files in the current location, in list order.
    suite_paths: Vec<PathBuf>,
    suite: Option<Suite>,
    /// Why the picked suite could not be read, already phrased for the status
    /// bar. Kept apart from `suite: None`, which is what an empty folder looks
    /// like — a file the picker lists but the loader could not open is a
    /// different thing to tell the user.
    load_error: Option<String>,
    diagnostics: Vec<Diagnostic>,
    run_file: RunFile,
    /// Why the run history came back empty, when a file was there and could
    /// not be used. Shown in the status bar beside the diagnostics counter:
    /// an empty history that is really a damaged file looks exactly like a
    /// suite nobody ever ran, and the tester would start recording over it.
    runs_damage: Option<String>,
    /// Index into `suite.scenarios`.
    selected_scenario: Option<usize>,
    /// 1-based step number within the selected scenario.
    selected_step: Option<usize>,
    /// The run picked in Histórico, when the tester went looking at an older
    /// one. `None` — the normal state — means "whatever `viewed_run` resolves
    /// to": the active run, else the most recent. Nothing that *records* ever
    /// reads this field; a verdict always lands in the active run.
    viewed_run: Option<String>,
    /// What the environment dialog currently on screen was asked for.
    env_target: EnvTarget,
    /// The environments the toolbar's picker is offering, in its own row order.
    /// The picker also carries a translated row no run has an environment for
    /// ("Outro…"), so a selected index cannot be read back off the control —
    /// this is what each row means.
    env_choices: Vec<String>,
    /// The scenario id and step number the open note dialog is annotating,
    /// captured when it was spawned. Re-reading the selection when the answer
    /// arrives would attach the note to whatever is selected *then*, which is
    /// not necessarily what the tester was looking at when they typed it.
    note_target: Option<(String, usize)>,
}

pub(crate) struct JafizApp {
    window: nwg::Window,
    suite_label: nwg::Label,
    suite_combo: nwg::ComboBox<String>,
    location_label: nwg::Label,
    // The run toolbar, which replaced the "Execução" menu: a run's whole life
    // cycle, with each control greyed out when it has nothing to act on.
    btn_run_new: nwg::Button,
    btn_run_finish: nwg::Button,
    env_label: nwg::Label,
    env_combo: nwg::ComboBox<String>,
    btn_history: nwg::Button,
    scenarios: nwg::ListView,
    steps: nwg::ListView,
    btn_pass: nwg::Button,
    btn_fail: nwg::Button,
    btn_blocked: nwg::Button,
    btn_skip: nwg::Button,
    btn_note: nwg::Button,
    btn_evidence: nwg::Button,
    status_bar: nwg::StatusBar,
    notice: nwg::Notice,
    menu_open: nwg::MenuItem,
    menu_reload: nwg::MenuItem,
    menu_exit: nwg::MenuItem,
    menu_report_copy: nwg::MenuItem,
    menu_report_save: nwg::MenuItem,
    menu_prefs: nwg::MenuItem,
    menu_help_format: nwg::MenuItem,
    menu_about: nwg::MenuItem,
    // The four top-level menu-bar entries. `relabel_menus` re-captions them
    // through `foundry_common::ui::set_submenu_text`, one named field each,
    // but they would have to be fields even if nothing read them: `nwg::Menu`'s
    // `Drop` calls `DestroyMenu`, so letting them go out of scope at the end of
    // `chrome::build_app` would destroy the whole menu bar, items included,
    // before the window is ever shown. They are declared after the `MenuItem`
    // fields on purpose: fields drop in declaration order, and `MenuItem`'s
    // `Drop` calls `DeleteMenu` on its parent, which must still exist.
    menu_file: nwg::Menu,
    menu_report: nwg::Menu,
    menu_tools: nwg::Menu,
    menu_help: nwg::Menu,
    /// How wide the scenario list is, in client pixels — where the tester left
    /// the divider between the two lists. A `Cell` rather than a `UiState`
    /// field because `layout` reads it while the state may be borrowed: it runs
    /// from the resize handler and from the middle of a drag.
    scenarios_w: Cell<i32>,
    /// True between grabbing the divider and letting it go, while this window
    /// holds the mouse capture.
    dragging: Cell<bool>,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
}

fn main() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");

    let settings = AppSettings::load();
    let app = Rc::new(chrome::build_app(settings));
    wire_events(&app);
    wire_splitter(&app);
    app.layout();
    app.open_initial_location();
    nwg::dispatch_thread_events();
}

fn wire_events(app: &Rc<JafizApp>) {
    let weak = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = weak.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => {
                app.layout();
            }
            E::OnComboxBoxSelection if handle == app.suite_combo.handle => {
                app.load_selected_suite();
            }
            E::OnComboxBoxSelection if handle == app.env_combo.handle => {
                app.pick_environment();
            }
            E::OnButtonClick if handle == app.btn_run_new.handle => app.new_run(),
            E::OnButtonClick if handle == app.btn_run_finish.handle => app.finish_run(),
            E::OnButtonClick if handle == app.btn_history.handle => app.show_history(),
            E::OnButtonClick if handle == app.btn_pass.handle => {
                app.mark_selected(StepStatus::Pass)
            }
            E::OnButtonClick if handle == app.btn_fail.handle => {
                app.mark_selected(StepStatus::Fail)
            }
            E::OnButtonClick if handle == app.btn_blocked.handle => {
                app.mark_selected(StepStatus::Blocked)
            }
            E::OnButtonClick if handle == app.btn_skip.handle => {
                app.mark_selected(StepStatus::Skip)
            }
            E::OnButtonClick if handle == app.btn_note.handle => app.edit_note(),
            E::OnButtonClick if handle == app.btn_evidence.handle => app.attach_evidence(),
            E::OnMenuItemSelected => {
                if handle == app.menu_open.handle {
                    app.open_folder();
                } else if handle == app.menu_reload.handle {
                    app.load_selected_suite();
                } else if handle == app.menu_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.menu_help_format.handle {
                    app.show_format_help();
                } else if handle == app.menu_about.handle {
                    app.show_about();
                } else if handle == app.menu_report_copy.handle {
                    app.copy_report();
                } else if handle == app.menu_report_save.handle {
                    app.save_report();
                } else if handle == app.menu_prefs.handle {
                    app.open_preferences();
                }
            }
            // The verdict keys are the whole point of the tool: both hands are
            // on the app under test, so recording a step has to be one
            // keystroke that never needs the mouse.
            E::OnKeyRelease => match evt_data {
                nwg::EventData::OnKey(VK_F2) => app.mark_selected(StepStatus::Pass),
                nwg::EventData::OnKey(VK_F3) => app.mark_selected(StepStatus::Fail),
                nwg::EventData::OnKey(VK_F4) => app.mark_selected(StepStatus::Blocked),
                nwg::EventData::OnKey(VK_F5) => app.load_selected_suite(),
                nwg::EventData::OnKey(VK_F6) => app.mark_selected(StepStatus::Skip),
                _ => {}
            },
            // Double-clicking a step annotates it — the gesture a tester
            // reaches for, and the one that does not cost a trip to a button.
            E::OnListViewItemActivated if handle == app.steps.handle => app.edit_note(),
            E::OnListViewItemChanged | E::OnListViewClick if handle == app.scenarios.handle => {
                if let Some(row) = changed_row(&evt_data) {
                    app.select_scenario_row(row);
                }
            }
            E::OnListViewItemChanged | E::OnListViewClick if handle == app.steps.handle => {
                if let Some(row) = changed_row(&evt_data) {
                    app.select_step_row(row);
                }
            }
            E::OnNotice if handle == app.notice.handle => app.drain_dialog(),
            _ => {}
        }
    });
    std::mem::forget(handler); // lives for the whole process (single window)
}

/// Id of the divider's raw message hook. Anything up to 0xFFFF is reserved by
/// nwg, which panics rather than accept one.
const SPLITTER_HANDLER_ID: usize = 0x1_0000;

/// The divider between the two lists.
///
/// nwg has no splitter control and no event for `WM_SETCURSOR`, so the drag is
/// driven straight off the window's messages through a raw subclass: the sizing
/// cursor while the pointer is over the gap, the mouse captured while the button
/// is down, and the layout re-run on every move. There is no control sitting in
/// the gap — it is bare parent window between the two lists, which is exactly
/// why the parent sees these messages at all.
fn wire_splitter(app: &Rc<JafizApp>) {
    use winapi::shared::windef::{HWND, POINT};
    use winapi::um::winuser::{
        GetCursorPos, LoadCursorW, ReleaseCapture, ScreenToClient, SetCapture, SetCursor, HTCLIENT,
        IDC_SIZEWE, WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR,
    };

    let weak = Rc::downgrade(app);
    // The returned token is dropped on purpose, unlike the one `wire_events`
    // leaks: `RawEventHandler` has no `Drop` of its own — only
    // `nwg::unbind_raw_event_handler` takes the subclass back off — so the hook
    // stays installed for as long as the window lives.
    nwg::bind_raw_event_handler(
        &app.window.handle,
        SPLITTER_HANDLER_ID,
        move |hwnd, msg, wparam, lparam| {
            let app = weak.upgrade()?;
            // Every mouse message carries the pointer in the two halves of
            // lParam, as signed client coordinates. (WM_SETCURSOR does not —
            // its lParam is the hit-test code, read on its own below.)
            let x = (lparam & 0xFFFF) as u16 as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32;
            match msg {
                WM_SETCURSOR => {
                    // Sent to whichever window the pointer is over, and handed
                    // up here by every child's DefWindowProc, so the window and
                    // the hit-test both have to be ours before the cursor is
                    // touched — otherwise the lists get the sizing cursor too.
                    if wparam as HWND != hwnd || (lparam & 0xFFFF) != HTCLIENT {
                        return None;
                    }
                    let mut point = POINT { x: 0, y: 0 };
                    // SAFETY: both calls take a pointer to this stack POINT,
                    // and `hwnd` is the live window being subclassed.
                    let located = unsafe {
                        GetCursorPos(&mut point) != 0 && ScreenToClient(hwnd, &mut point) != 0
                    };
                    if !located || !app.on_splitter(point.x, point.y) {
                        return None;
                    }
                    // SAFETY: IDC_SIZEWE is a system cursor, and LoadCursorW
                    // with a null instance hands back a shared handle that
                    // nothing has to free.
                    unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_SIZEWE)) };
                    Some(1) // TRUE: the cursor is set, stop here
                }
                WM_LBUTTONDOWN if app.on_splitter(x, y) => {
                    app.dragging.set(true);
                    // Capture, or a pointer that outruns the divider — which it
                    // will, since the divider stops at the clamp — takes the
                    // drag with it into whatever window it lands on.
                    // SAFETY: a live window handle; SetCapture takes no more.
                    unsafe { SetCapture(hwnd) };
                    Some(0)
                }
                WM_MOUSEMOVE if app.dragging.get() => {
                    app.drag_splitter_to(x);
                    Some(0)
                }
                WM_LBUTTONUP if app.dragging.get() => {
                    app.dragging.set(false);
                    // SAFETY: releases this thread's own capture, no arguments.
                    unsafe { ReleaseCapture() };
                    app.remember_splitter();
                    Some(0)
                }
                // Something else took the mouse (Alt+Tab, a modal put up from a
                // Notice): the drag is over, and the button-up never arrives.
                // The flag is cleared before `ReleaseCapture` above, so this
                // cannot fire for a capture this window let go of itself.
                WM_CAPTURECHANGED if app.dragging.get() => {
                    app.dragging.set(false);
                    app.remember_splitter();
                    None
                }
                _ => None,
            }
        },
    )
    .expect("splitter handler");
}

/// The row a list-view event points at. nwg reports a selection through two
/// different payloads depending on how it was made (mouse click vs. keyboard
/// or a programmatic change), so both are unwrapped in one place.
fn changed_row(data: &nwg::EventData) -> Option<usize> {
    match *data {
        nwg::EventData::OnListViewItemIndex { row_index, .. } => Some(row_index),
        nwg::EventData::OnListViewItemChanged { row_index, selected: true, .. } => Some(row_index),
        _ => None,
    }
}

/// Flattens a note to one line.
///
/// The dialog is a multi-line box because notes are prose, but a note is one
/// field of one line everywhere it is read: `report` renders it as
/// `**FALHOU**: <note>` and the step list shows it in a column. An embedded
/// newline would break both, so the tester's paragraphs are joined with a
/// space rather than silently truncated at the first line.
fn fold_lines(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The run the window is showing: the one picked in Histórico, else the active
/// one, else the most recent.
///
/// Every read of the run — both lists, the status bar, the report — goes
/// through here. Every *write* goes through `RunFile::active_mut` instead, so
/// reading an old run can never let a verdict land in it.
fn viewed_run(state: &UiState) -> Option<&Run> {
    match state.viewed_run.as_deref() {
        Some(id) => state.run_file.run(id),
        None => state.run_file.latest(),
    }
}

/// True while the window is showing a run other than the one recording would
/// land in. The verdict buttons are disabled for exactly this state: pressing
/// F2 while reading last week's run would silently mark a step of *today's*.
fn viewing_other_run(state: &UiState) -> bool {
    match state.viewed_run.as_deref() {
        Some(id) => state.run_file.active_run.as_deref() != Some(id),
        None => false,
    }
}

impl JafizApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    /// Opens the folder this launch should show. The engine's cascade decides
    /// first: `JAFIZ_DIR` or a project's `tests/manual` found from the cwd are
    /// deliberate statements about *this* launch and outrank a folder
    /// remembered from an earlier session. Only when the cascade falls through
    /// to the central library does the remembered folder win.
    fn open_initial_location(&self) {
        // Both preferences are read before anything is loaded: opening the
        // folder immediately rewrites `last_suite` to whatever the picker
        // lands on, which would erase the suite we are about to restore.
        let (remembered, last_suite) = {
            let state = self.state.borrow();
            (state.settings.last_location.clone(), state.settings.last_suite.clone())
        };
        let cascade = store::resolve_location(None);
        let location = match cascade.kind {
            LocationKind::Env | LocationKind::Project => cascade,
            // A remembered folder that has since been deleted must not strand
            // the window on an empty list — fall through to the cascade.
            _ => match remembered.filter(|dir| dir.is_dir()) {
                Some(dir) => store::Location { dir, kind: LocationKind::Explicit },
                None => cascade,
            },
        };
        self.load_location(&location.dir, location.kind);
        if let Some(stem) = last_suite {
            self.select_suite_by_stem(&stem);
        }
    }

    /// Reads the folder, fills the suite picker, and opens the first suite.
    /// Deliberately free of side effects on the preferences: only the user
    /// choosing a folder is worth remembering, and persisting every folder the
    /// cascade produced is what would freeze the cascade from launch 2 onward.
    fn load_location(&self, dir: &std::path::Path, kind: LocationKind) {
        let paths = store::list_suites(dir);
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
            .collect();
        {
            let mut state = self.state.borrow_mut();
            state.location = dir.to_path_buf();
            state.location_kind = kind;
            state.suite_paths = paths;
        }
        self.suite_combo.set_collection(names);
        self.set_location_label();
        self.suite_combo.set_selection(Some(0));
        self.load_selected_suite();
    }

    /// Rewrites the header's "Local:" line.
    ///
    /// Which rung of the cascade produced the folder is part of the answer to
    /// "why am I looking at these suites" — a folder the user picked,
    /// `JAFIZ_DIR`, the enclosing project or the central library all look alike
    /// otherwise. The wording is the GUI's own: `LocationKind::label` is a
    /// pt-BR CLI constant and would leak into an English window — which is also
    /// why this is its own method, called again when the language changes.
    fn set_location_label(&self) {
        let text = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            let source = match state.location_kind {
                LocationKind::Explicit => tr.loc_explicit,
                LocationKind::Env => tr.loc_env,
                LocationKind::Project => tr.loc_project,
                LocationKind::Library => tr.loc_library,
            };
            format!("{} {} ({})", tr.lbl_location, state.location.display(), source)
        };
        self.location_label.set_text(&text);
    }

    /// Loads the suite the picker points at, plus its run history.
    fn load_selected_suite(&self) {
        let path = {
            let state = self.state.borrow();
            self.suite_combo.selection().and_then(|i| state.suite_paths.get(i).cloned())
        };
        {
            let mut state = self.state.borrow_mut();
            // `state` is a RefMut, so field-level split borrows go through
            // Deref — read the location out first instead of borrowing it
            // inside an assignment to a sibling field.
            let dir = state.location.clone();
            let tr = t(state.lang);
            state.load_error = None;
            match path.as_ref().map(|p| store::load_suite(p)) {
                Some(Ok(outcome)) => {
                    let loaded = store::load_runs(&dir, &outcome.suite.stem);
                    state.run_file = loaded.file;
                    state.runs_damage = loaded.damage;
                    // Only write when it actually changed: every F5 and every
                    // combo change comes through here, and settings.json is not
                    // worth a disk write per keystroke.
                    if state.settings.last_suite.as_deref() != Some(outcome.suite.stem.as_str()) {
                        state.settings.last_suite = Some(outcome.suite.stem.clone());
                        let _ = state.settings.save();
                    }
                    state.diagnostics = outcome.diagnostics;
                    state.suite = Some(outcome.suite);
                }
                // Deleted between the listing and this load, locked, or simply
                // unreadable. Falling into the same arm as "nothing selected"
                // would put "no suites in this folder" under a picker that is
                // visibly listing suites — say what actually went wrong.
                Some(Err(error)) => {
                    let name = path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    state.load_error =
                        Some(tr.read_error.replace("%S", &name).replace("%E", &error.to_string()));
                    state.suite = None;
                    state.diagnostics.clear();
                    state.run_file = RunFile::default();
                    state.runs_damage = None;
                }
                None => {
                    state.suite = None;
                    state.diagnostics.clear();
                    state.run_file = RunFile::default();
                    state.runs_damage = None;
                }
            }
            state.selected_scenario = None;
            state.selected_step = None;
            // The history belongs to the suite that was open: an id from the
            // old one would resolve to nothing here, leaving the window blank
            // with no way back except another trip through Histórico.
            state.viewed_run = None;
        }
        self.populate_scenarios();
        self.populate_steps();
        self.update_buttons();
        self.update_run_controls();
        self.update_status();
    }

    /// Points the picker at a named suite, if the current folder has one.
    /// Used to restore the suite the user had open last.
    fn select_suite_by_stem(&self, stem: &str) {
        // `to_lowercase`, not `eq_ignore_ascii_case`: the CLI's own stem
        // matching (`store::match_suites`) folds case that way, and a suite
        // named `Ação.md` must not resolve differently in the two front-ends.
        let wanted = stem.to_lowercase();
        let index = {
            let state = self.state.borrow();
            state
                .suite_paths
                .iter()
                .position(|p| {
                    p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == wanted)
                })
        };
        let Some(index) = index else { return };
        if self.suite_combo.selection() == Some(index) {
            return; // already showing it — reloading would only cost a reparse
        }
        self.suite_combo.set_selection(Some(index));
        self.load_selected_suite();
    }

    fn populate_scenarios(&self) {
        self.scenarios.clear();
        let state = self.state.borrow();
        let Some(suite) = state.suite.as_ref() else { return };
        let run = viewed_run(&state);
        for scenario in &suite.scenarios {
            let row = [
                display_symbol(scenario, run).to_string(),
                scenario.id.clone(),
                scenario.title.clone(),
                format!("{}/{}", scenario_done(scenario, run), scenario.steps.len()),
            ];
            self.scenarios.insert_items_row(None, &row);
        }
    }

    /// The selected scenario's steps. The current step is prefixed with `▶`,
    /// which is what makes "what is being tested right now" visible at a
    /// glance instead of buried in the status bar.
    fn populate_steps(&self) {
        self.steps.clear();
        let state = self.state.borrow();
        let Some(suite) = state.suite.as_ref() else { return };
        let Some(index) = state.selected_scenario else { return };
        let Some(scenario) = suite.scenarios.get(index) else { return };
        let run = viewed_run(&state);
        let current = run.and_then(|r| r.current.clone());
        for step in &scenario.steps {
            let result = run.and_then(|r| r.result(&scenario.id, step.number));
            let status = result.map_or(StepStatus::Pending, |r| r.status);
            let is_current = current
                .as_ref()
                .is_some_and(|c| c.scenario == scenario.id && c.step == step.number);
            let stale = result.is_some_and(|r| jafiz::model::is_stale(step, r));
            let note = result.map(|r| r.note.clone()).unwrap_or_default();
            let row = [
                format!("{}{}", if is_current { "▶ " } else { "" }, step.number),
                format!("{}{}", status.symbol(), if stale { self.tr().stale_marker } else { "" }),
                step.action.clone(),
                step.expected.clone(),
                note,
            ];
            self.steps.insert_items_row(None, &row);
        }
    }

    /// Run id, environment, progress, and what is being tested right now.
    fn update_status(&self) {
        let text = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            let mut text = match (state.load_error.as_ref(), state.suite.as_ref()) {
                (Some(error), _) => error.clone(),
                (None, None) => tr.no_suites.replace("%D", &state.location.display().to_string()),
                (None, Some(suite)) => match viewed_run(&state) {
                    None => format!("{} · {}", suite.title, tr.no_run),
                    Some(run) => report::status_line(suite, Some(run), state.lang),
                },
            };
            // Reading an old run looks exactly like recording into it until the
            // window says otherwise — and the greyed-out verdict buttons alone
            // read as a bug rather than as an explanation.
            if viewing_other_run(&state) {
                if let Some(id) = state.viewed_run.as_deref() {
                    text.push_str(&format!(" · {}", tr.viewing_run.replace("%I", id)));
                }
            }
            // A suite the parser could not fully read shows fewer scenarios
            // than the file has. Say so here rather than let the list quietly
            // lie about what is going to be tested.
            if !state.diagnostics.is_empty() {
                let count = state.diagnostics.len().to_string();
                text.push_str(&format!(" · {}", tr.diagnostics.replace("%N", &count)));
            }
            // Same reasoning one line up, for the other file: a run history
            // that exists but could not be read must say so, or the window
            // shows "no run recorded" over a file it is about to replace.
            if let Some(reason) = state.runs_damage.as_deref() {
                text.push_str(&format!(" · {}", tr.runs_damaged.replace("%E", reason)));
            }
            text
        };
        self.status_bar.set_text(0, &text);
    }

    /// Remembers which scenario the tester clicked and shows its steps.
    fn select_scenario_row(&self, row: usize) {
        {
            // Refilling a list makes Windows emit selection changes of its own.
            // Those land while `populate_*` still holds the state borrowed —
            // and the refill resets the selection right after — so dropping
            // them is both safe and the behavior we want.
            let Ok(mut state) = self.state.try_borrow_mut() else { return };
            let exists = state
                .suite
                .as_ref()
                .is_some_and(|suite| row < suite.scenarios.len());
            // A list view reports "no item" as a huge row index, never as an
            // absence — validate rather than trust it.
            state.selected_scenario = exists.then_some(row);
            state.selected_step = None;
        }
        self.populate_steps();
        self.update_buttons();
    }

    /// Remembers which step the tester clicked. The row is the step's position
    /// in the scenario; what is stored is its 1-based number, because that is
    /// what a verdict is recorded against.
    fn select_step_row(&self, row: usize) {
        {
            let Ok(mut state) = self.state.try_borrow_mut() else { return };
            let number = state
                .suite
                .as_ref()
                .zip(state.selected_scenario)
                .and_then(|(suite, index)| suite.scenarios.get(index))
                .and_then(|scenario| scenario.steps.get(row))
                .map(|step| step.number);
            state.selected_step = number;
        }
        self.update_buttons();
    }

    /// All six buttons need a step to record against *and* a run open to
    /// record into: `runs::set_note` and `runs::add_evidence` go through
    /// `RunFile::active_mut()` exactly like `runs::mark` does, so without a run
    /// the note and evidence buttons would look available and do nothing.
    ///
    /// They also need the window to be showing that same run. Recording targets
    /// the active run whatever is on screen, so leaving the buttons live while
    /// an older run is being read would let a click land somewhere the tester
    /// is not looking.
    fn update_buttons(&self) {
        let recording = {
            // Refilling a list re-enters the selection handlers while
            // `populate_*` still holds the state borrowed; skipping the update
            // is right, because the refill calls back into here afterwards.
            let Ok(state) = self.state.try_borrow() else { return };
            state.selected_scenario.is_some()
                && state.selected_step.is_some()
                && state.run_file.active().is_some()
                && !viewing_other_run(&state)
        };
        for button in [
            &self.btn_pass,
            &self.btn_fail,
            &self.btn_blocked,
            &self.btn_skip,
            &self.btn_note,
            &self.btn_evidence,
        ] {
            button.set_enabled(recording);
        }
    }

    /// The run toolbar's enabled state — the reason the four run commands left
    /// the menu bar. `Nova execução` only needs a suite to run against, and
    /// `Histórico` only needs a run to have happened; the other two edit the
    /// *active* run, so they need one open **and** on screen. Leaving them live
    /// while an older run is being read would let a click finish, or re-label,
    /// a run the tester is not looking at — the same mistake the verdict
    /// buttons are disabled to prevent.
    fn update_run_controls(&self) {
        let (has_suite, recording, has_history) = {
            // Refilling a list re-enters the selection handlers while
            // `populate_*` still holds the state borrowed; skipping is right,
            // because the refill calls back into here afterwards.
            let Ok(state) = self.state.try_borrow() else { return };
            (
                state.suite.is_some(),
                state.run_file.active().is_some() && !viewing_other_run(&state),
                !state.run_file.runs.is_empty(),
            )
        };
        self.btn_run_new.set_enabled(has_suite);
        self.btn_run_finish.set_enabled(recording);
        self.env_combo.set_enabled(recording);
        self.btn_history.set_enabled(has_history);
        self.refresh_env_combo();
    }

    /// Fills the environment picker: the environment of the run on screen
    /// first, then every other environment this suite has been run in, newest
    /// first, and `Outro…` last for typing one that is not in the list.
    ///
    /// Re-testing the same build is the common case, which is what makes the
    /// list worth having — and showing the current environment on the toolbar
    /// is half of why it is there at all, since a run's environment is what the
    /// report is read against.
    fn refresh_env_combo(&self) {
        let choices = {
            let Ok(mut state) = self.state.try_borrow_mut() else { return };
            let current = viewed_run(&state).map(|run| run.environment.clone());
            let mut choices: Vec<String> = current.into_iter().collect();
            for run in state.run_file.runs.iter().rev() {
                if !run.environment.is_empty() && !choices.contains(&run.environment) {
                    choices.push(run.environment.clone());
                }
            }
            state.env_choices = choices.clone();
            choices
        };
        let tr = self.tr();
        // An empty list means no run has ever been recorded for this suite —
        // there is no environment to show and nothing to pick from.
        let mut rows: Vec<String> = choices
            .iter()
            .map(|env| if env.is_empty() { tr.env_unset.to_string() } else { env.clone() })
            .collect();
        if !rows.is_empty() {
            rows.push(tr.env_other.to_string());
        }
        let selected = (!rows.is_empty()).then_some(0);
        // Every verdict comes through here, and rebuilding the list under the
        // tester's eyes for a run whose environment did not change is a flicker
        // for nothing. The comparison is against the rendered rows, so a
        // language change — same environments, translated last row — still
        // rebuilds. (The `Ref` is dropped at the semicolon: `set_collection`
        // borrows the same cell mutably.)
        let unchanged = *self.env_combo.collection() == rows;
        if !unchanged {
            self.env_combo.set_collection(rows);
        }
        // Always re-selected, even when the rows stayed: this is what puts the
        // picker back after `Outro…` was chosen and the dialog cancelled.
        // CB_SETCURSEL raises no selection notification, so it cannot re-enter
        // `pick_environment` and re-apply what it is only displaying.
        self.env_combo.set_selection(selected);
    }

    /// Applies the environment picked on the toolbar. Every row but the last is
    /// an environment some run already carries; the last one (`Outro…`) has no
    /// entry in `env_choices` and opens the dialog instead.
    fn pick_environment(&self) {
        let Some(index) = self.env_combo.selection() else { return };
        let chosen = {
            let Ok(state) = self.state.try_borrow() else { return };
            state.env_choices.get(index).cloned()
        };
        match chosen {
            Some(environment) => self.apply_environment(&environment),
            None => self.edit_environment(),
        }
    }

    /// Remembers where the divider was dropped, so the next launch opens with
    /// the lists the tester's own size instead of back at the default.
    fn remember_splitter(&self) {
        let width = self.scenarios_w.get();
        // A modal can be pumping messages while the mouse is still captured;
        // the position is not worth fighting a borrow over.
        let Ok(mut state) = self.state.try_borrow_mut() else { return };
        if state.settings.scenarios_width == Some(width) {
            return; // unchanged — not worth a disk write per drag
        }
        state.settings.scenarios_width = Some(width);
        let _ = state.settings.save();
    }

    /// Picks a different suite folder. Chosen by hand, so it enters the
    /// cascade as `Explicit` — and this is the only path that remembers a
    /// folder, because it is the only one where the user said which one.
    fn open_folder(&self) {
        let start = self.state.borrow().location.clone();
        let mut builder = nwg::FileDialog::builder()
            .title(self.tr().menu_open.trim_end_matches('…'))
            .action(nwg::FileDialogAction::OpenDirectory);
        if start.is_dir() {
            builder = builder.default_folder(start.display().to_string());
        }
        let mut dialog = nwg::FileDialog::default();
        if builder.build(&mut dialog).is_err() {
            return;
        }
        if !dialog.run(Some(&self.window)) {
            return;
        }
        let Ok(chosen) = dialog.get_selected_item() else { return };
        let dir = PathBuf::from(chosen);
        {
            let mut state = self.state.borrow_mut();
            state.settings.remember_location(&dir);
            let _ = state.settings.save();
        }
        self.load_location(&dir, LocationKind::Explicit);
    }

    /// Persists the run file after every change. It is a small JSON and a
    /// sub-millisecond write, and it is what `jafiz report` in another
    /// terminal reads — so Claude sees a verdict the instant it is recorded,
    /// not whenever the window happens to close.
    fn save_runs(&self) {
        // The borrow is released before anything modal: `modal_error_message`
        // pumps messages, and a `Notice` arriving inside it would re-enter
        // `drain_dialog` and try to borrow the state mutably.
        let outcome = {
            let state = self.state.borrow();
            store::save_runs(&state.location, &state.run_file)
        };
        if let Err(error) = outcome {
            // Losing a verdict silently is the worst thing this tool could do,
            // so a failed write is said out loud rather than logged nowhere.
            nwg::modal_error_message(&self.window.handle, self.tr().app_title, &error);
        }
    }

    /// Starts a run after asking for the environment. The last run's
    /// environment is offered as the starting point — retesting the same build
    /// is the common case.
    ///
    /// Gated on `confirm_new_run_over_damage` before anything else: `state.
    /// runs_damage` being set means `load_selected_suite` already substituted
    /// a fresh empty `RunFile` for a history the loader could not read, and
    /// from here `apply_new_run` → `runs::start_run` → `save_runs()` would
    /// write straight over the damaged file. The status bar says so, but a
    /// tester who missed that note must not lose it silently — see C1, the
    /// same shape of data loss this wave's atomic save was written to close.
    fn new_run(&self) {
        let (lang, environment) = {
            let mut state = self.state.borrow_mut();
            if state.suite.is_none() {
                return; // nothing to run against
            }
            state.env_target = EnvTarget::NewRun;
            let environment =
                state.run_file.latest().map(|run| run.environment.clone()).unwrap_or_default();
            (state.lang, environment)
        };
        if !self.confirm_new_run_over_damage() {
            return;
        }
        self.open_env_dialog(lang, environment, t(lang).run_dlg_title);
    }

    /// True unless a damaged run history exists and the tester backs out of
    /// replacing it. No-op (returns `true` immediately) when `runs_damage` is
    /// unset — the common case must not pay for a modal round trip.
    ///
    /// Uses the same synchronous `nwg::modal_message` pattern as `finish_run`'s
    /// confirmation rather than the async dialog-thread pattern the other
    /// dialogs use: this is a plain Yes/No with no fields to fill in, so the
    /// extra machinery (mailbox slot, spawned thread, `DialogGuard`) would
    /// buy nothing.
    fn confirm_new_run_over_damage(&self) -> bool {
        let (tr, reason) = {
            let state = self.state.borrow();
            (t(state.lang), state.runs_damage.clone())
        };
        let Some(reason) = reason else { return true };
        let choice = nwg::modal_message(
            &self.window.handle,
            &nwg::MessageParams {
                title: tr.new_run_damaged_title,
                content: &tr.new_run_damaged_body.replace("%E", &reason),
                buttons: nwg::MessageButtons::YesNo,
                icons: nwg::MessageIcons::Warning,
            },
        );
        choice == nwg::MessageChoice::Yes
    }

    /// Re-describes the open run's environment. Same window as `new_run`, but
    /// the answer edits the run instead of starting one: the tester usually
    /// learns the exact build only after the app is in front of them.
    fn edit_environment(&self) {
        let prepared = {
            let mut state = self.state.borrow_mut();
            state.env_target = EnvTarget::ActiveRun;
            state.run_file.active().map(|run| (state.lang, run.environment.clone()))
        };
        let Some((lang, environment)) = prepared else {
            self.warn_no_run();
            return;
        };
        self.open_env_dialog(lang, environment, t(lang).env_dlg_title);
    }

    /// Puts the environment dialog on screen. It runs on its own thread, so
    /// nothing but this disable makes it modal; `drain_dialog` hands the
    /// window back when the answer arrives.
    fn open_env_dialog(&self, lang: Lang, environment: String, title: &'static str) {
        self.window.set_enabled(false);
        run_dialog::spawn(run_dialog::RunParams {
            lang,
            environment,
            title,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Drains whatever a dialog thread left behind and hands the window back.
    fn drain_dialog(&self) {
        // Unconditionally, and before the mailbox is even inspected: a dialog
        // thread that panics or exits without writing an outcome would
        // otherwise leave a `WS_DISABLED` top-level window the user cannot
        // even close. Each dialog notifies exactly once, so re-enabling here
        // can never unlock the window under a modal that is still up.
        self.window.set_enabled(true);
        let (environment, note, history, preferences) = {
            let mut shared = self.shared.lock().unwrap();
            (
                shared.environment.take(),
                shared.note.take(),
                shared.history.take(),
                shared.preferences.take(),
            )
        };
        match environment {
            Some(Some(text)) => {
                // Read out of the borrow before dispatching: both arms borrow
                // the state mutably.
                let target = self.state.borrow().env_target;
                match target {
                    EnvTarget::NewRun => self.apply_new_run(&text),
                    EnvTarget::ActiveRun => self.apply_environment(&text),
                }
            }
            // Cancelled. When the dialog was opened from the toolbar's `Outro…`
            // row, that row is the one still selected — put the picker back on
            // the environment the run actually has.
            Some(None) => self.refresh_env_combo(),
            None => {}
        }
        match note {
            Some(Some(text)) => self.apply_note(&text),
            // Cancelled: forget which step was being annotated, so a later
            // answer can never land on a step the tester has moved off.
            Some(None) => self.state.borrow_mut().note_target = None,
            None => {}
        }
        if let Some(Some(id)) = history {
            self.apply_viewed_run(&id);
        }
        if let Some(Some(lang)) = preferences {
            self.apply_language(lang);
        }
    }

    /// Opens a run over the suite on screen and follows its cursor to the
    /// first step.
    fn apply_new_run(&self, environment: &str) {
        {
            let mut state = self.state.borrow_mut();
            // Cloned because `runs::start_run` needs `run_file` mutably while
            // reading the suite; both live in the same `RefMut`.
            let Some(suite) = state.suite.clone() else { return };
            runs::start_run(&mut state.run_file, &suite, environment);
            // Starting a run is a statement about what to look at now, so it
            // takes the window off whatever old run Histórico had left on it.
            state.viewed_run = None;
        }
        self.save_runs();
        self.refresh_all();
        self.follow_cursor();
    }

    /// Applies a re-typed — or re-picked — environment to the run already open.
    fn apply_environment(&self, environment: &str) {
        {
            let mut state = self.state.borrow_mut();
            // Nothing to write when it did not change: the toolbar's picker
            // re-selects the environment it is already showing every time the
            // list is rebuilt, and every save is a disk write.
            let same = state
                .run_file
                .active()
                .is_some_and(|run| run.environment == environment.trim());
            if same {
                return;
            }
            if !runs::set_environment(&mut state.run_file, environment) {
                // The run was finished while the dialog was up. Drop the borrow
                // first: the picker reads the state to put itself back.
                drop(state);
                self.refresh_env_combo();
                return;
            }
        }
        self.save_runs();
        self.refresh_all();
    }

    /// Records a verdict on the selected step and moves the current marker on.
    /// One click, and the tester's place moves by itself — recording per step
    /// is worthless if finding the next step costs a search.
    fn mark_selected(&self, status: StepStatus) {
        let Some((scenario, step)) = self.recording_target() else { return };
        let moved = {
            let mut state = self.state.borrow_mut();
            let Some(suite) = state.suite.clone() else { return };
            runs::mark(&mut state.run_file, &suite, &scenario, step, status);
            // Point "now" at what was just decided before advancing, so the
            // search for the next pending step starts from here and not from
            // wherever the marker had drifted to.
            runs::set_current(&mut state.run_file, &scenario, step);
            runs::advance(&mut state.run_file, &suite)
        };
        self.save_runs();
        self.refresh_all();
        // Follow the marker, into another scenario if that is where it went.
        if let Some(cursor) = moved {
            self.select_scenario_by_id(&cursor.scenario);
            self.select_step_number(cursor.step);
        }
    }

    /// The step a recording action applies to — the selected scenario's id and
    /// the selected step's number — but only while a run is open to record
    /// into *and* on screen. The state borrow is released before the warning: a
    /// modal pumps messages, and a `Notice` arriving inside one re-enters
    /// `drain_dialog`.
    ///
    /// The read-only check has to be here and not only in `update_buttons`:
    /// F2–F6 are bound on the window, not on the buttons, so a disabled button
    /// stops the mouse and nothing else. Without this, reading an old run and
    /// pressing F4 out of habit would land a verdict in today's run, silently,
    /// on whatever step happened to be selected.
    fn recording_target(&self) -> Option<(String, usize)> {
        let (recording, reading, target) = {
            let state = self.state.borrow();
            let target = state
                .suite
                .as_ref()
                .zip(state.selected_scenario)
                .and_then(|(suite, index)| suite.scenarios.get(index))
                .map(|scenario| scenario.id.clone())
                .zip(state.selected_step);
            let reading = viewing_other_run(&state).then(|| state.viewed_run.clone()).flatten();
            (state.run_file.active().is_some(), reading, target)
        };
        if let Some(id) = reading {
            let tr = self.tr();
            let body = tr.read_only.replace("%I", &id);
            nwg::modal_info_message(&self.window.handle, tr.app_title, &body);
            return None;
        }
        if !recording {
            self.warn_no_run();
            return None;
        }
        target
    }

    fn warn_no_run(&self) {
        nwg::modal_info_message(&self.window.handle, self.tr().app_title, self.tr().no_run);
    }

    /// Opens the note dialog for the selected step, pre-filled with whatever
    /// is already written there.
    fn edit_note(&self) {
        let Some((scenario, step)) = self.recording_target() else { return };
        let (lang, note, title) = {
            let mut state = self.state.borrow_mut();
            let note = state
                .run_file
                .active()
                .and_then(|run| run.result(&scenario, step))
                .map(|result| result.note.clone())
                .unwrap_or_default();
            let title = t(state.lang)
                .note_dlg_title
                .replace("%C", &scenario)
                .replace("%S", &step.to_string());
            state.note_target = Some((scenario, step));
            (state.lang, note, title)
        };
        self.window.set_enabled(false);
        note_dialog::spawn(note_dialog::NoteParams {
            lang,
            note,
            title,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Attaches the typed note to the step the dialog was opened on.
    fn apply_note(&self, text: &str) {
        let target = self.state.borrow_mut().note_target.take();
        let Some((scenario, step)) = target else { return };
        let note = fold_lines(text);
        {
            let mut state = self.state.borrow_mut();
            runs::set_note(&mut state.run_file, &scenario, step, &note);
        }
        self.save_runs();
        self.refresh_all();
    }

    /// File picker; the chosen path is attached to the selected step and shows
    /// up in the report, so a screenshot travels with the verdict it explains.
    fn attach_evidence(&self) {
        let Some((scenario, step)) = self.recording_target() else { return };
        let start = self.state.borrow().location.clone();
        let mut builder = nwg::FileDialog::builder()
            .action(nwg::FileDialogAction::Open)
            .title(self.tr().btn_evidence.trim_end_matches('…'));
        if start.is_dir() {
            builder = builder.default_folder(start.display().to_string());
        }
        let mut dialog = nwg::FileDialog::default();
        if builder.build(&mut dialog).is_err() {
            return;
        }
        // `run` is modal on this thread and pumps messages, so no state borrow
        // may be held across it.
        if !dialog.run(Some(&self.window)) {
            return;
        }
        let Ok(chosen) = dialog.get_selected_item() else { return };
        let path = PathBuf::from(chosen).display().to_string();
        {
            let mut state = self.state.borrow_mut();
            runs::add_evidence(&mut state.run_file, &scenario, step, &path);
        }
        self.save_runs();
        self.refresh_all();
    }

    /// Closes the run. Confirmed first: a finished run stops accepting
    /// verdicts, and the only way back is to start another one.
    fn finish_run(&self) {
        let (tr, id) = {
            let state = self.state.borrow();
            (t(state.lang), state.run_file.active().map(|run| run.id.clone()))
        };
        let Some(id) = id else {
            self.warn_no_run();
            return;
        };
        let choice = nwg::modal_message(
            &self.window.handle,
            &nwg::MessageParams {
                title: tr.finish_title,
                content: &tr.finish_body.replace("%I", &id),
                buttons: nwg::MessageButtons::YesNo,
                icons: nwg::MessageIcons::Question,
            },
        );
        if choice != nwg::MessageChoice::Yes {
            return;
        }
        {
            let mut state = self.state.borrow_mut();
            runs::finish_run(&mut state.run_file);
        }
        self.save_runs();
        self.refresh_all();
    }

    /// Rebuilds both lists and the status bar, preserving the selection — a
    /// tester who has to find their place again after every verdict will stop
    /// using the tool.
    fn refresh_all(&self) {
        let (scenario, step) = {
            let state = self.state.borrow();
            (state.selected_scenario, state.selected_step)
        };
        self.populate_scenarios();
        self.populate_steps();
        self.update_status();
        if let Some(index) = scenario {
            select_only(&self.scenarios, index);
            // Refilling the list dropped the selection, and re-selecting it
            // runs `select_scenario_row`, which clears `selected_step`. Both
            // fields are put back here and in `select_step_number` below.
            if let Ok(mut state) = self.state.try_borrow_mut() {
                state.selected_scenario = Some(index);
            }
        }
        if let Some(number) = step {
            self.select_step_number(number);
        }
        self.update_buttons();
        self.update_run_controls();
    }

    /// Moves the selection to wherever the active run says testing is now.
    fn follow_cursor(&self) {
        let cursor = {
            let state = self.state.borrow();
            state.run_file.active().and_then(|run| run.current.clone())
        };
        let Some(cursor) = cursor else { return };
        self.select_scenario_by_id(&cursor.scenario);
        self.select_step_number(cursor.step);
    }

    /// Points the scenario list at a scenario by its id, showing its steps.
    fn select_scenario_by_id(&self, id: &str) {
        let index = {
            let state = self.state.borrow();
            state
                .suite
                .as_ref()
                .and_then(|suite| suite.scenarios.iter().position(|s| s.id == id))
        };
        let Some(index) = index else { return };
        let already = self.state.borrow().selected_scenario == Some(index);
        select_only(&self.scenarios, index);
        ensure_visible(&self.scenarios, index);
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state.selected_scenario = Some(index);
        }
        // Selecting a row that is already selected raises no notification, so
        // the step list would keep showing the previous scenario's steps.
        if already {
            return;
        }
        self.populate_steps();
    }

    /// Points the step list at a step by its 1-based number. The number is not
    /// the row: a scenario's steps are numbered from 1 in file order, and the
    /// row is only where that step happens to sit in the list.
    fn select_step_number(&self, number: usize) {
        let row = {
            let state = self.state.borrow();
            state
                .suite
                .as_ref()
                .zip(state.selected_scenario)
                .and_then(|(suite, index)| suite.scenarios.get(index))
                .and_then(|scenario| scenario.steps.iter().position(|s| s.number == number))
        };
        let Some(row) = row else { return };
        select_only(&self.steps, row);
        ensure_visible(&self.steps, row);
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state.selected_step = Some(number);
        }
        self.update_buttons();
    }

    /// The format contract, verbatim from the same text `jafiz format` prints —
    /// one source for what the parser accepts, so the GUI can never drift from
    /// what the CLI tells Claude.
    ///
    /// Dropped in `%TEMP%` and handed to the shell rather than shown in a
    /// message box: the guide is pages of Markdown with examples in it, and a
    /// message box would stack that into one unscrollable column of plain text.
    /// Opening it by association also puts it in whatever the user already
    /// reads Markdown with, where they can copy from it.
    fn show_format_help(&self) {
        let (tr, guide) = {
            let state = self.state.borrow();
            let guide = match state.lang {
                Lang::PtBr => jafiz::parser::GUIDE_PT,
                Lang::En => jafiz::parser::GUIDE_EN,
            };
            (t(state.lang), guide)
        };
        let path = std::env::temp_dir().join(GUIDE_FILE);
        // The two languages share one file name on purpose: the guide is
        // rewritten on every open, so switching language and asking again
        // replaces it rather than leaving two half-read copies behind.
        if std::fs::write(&path, guide).is_ok() {
            // `explorer.exe <file>` opens it with its associated program.
            // Nothing is done with the exit status: explorer routinely returns
            // non-zero after handing the file off successfully.
            if std::process::Command::new("explorer.exe").arg(&path).spawn().is_ok() {
                return;
            }
        }
        // No writable temp folder, or no shell to hand it to. A cramped guide
        // still beats no guide.
        nwg::modal_info_message(
            &self.window.handle,
            tr.menu_help_format.trim_end_matches('…'),
            guide,
        );
    }

    fn show_about(&self) {
        let tr = self.tr();
        let body = tr.about_body.replace("%V", CURRENT_VERSION);
        nwg::modal_info_message(&self.window.handle, tr.about_title, &body);
    }

    /// The report the window would hand Claude right now: the run on screen,
    /// in the chosen language. `None` when no suite is open — there is nothing
    /// to report about, and an empty clipboard is not an improvement.
    fn report_text(&self) -> Option<String> {
        let state = self.state.borrow();
        let suite = state.suite.as_ref()?;
        Some(report::report(suite, viewed_run(&state), state.lang))
    }

    /// Puts the report on the clipboard — the whole outbound path in one menu
    /// command, because the alternative is the tester retyping what they just
    /// spent twenty minutes finding out.
    fn copy_report(&self) {
        let Some(text) = self.report_text() else { return };
        nwg::Clipboard::set_data_text(&self.window, &text);
        // Transient by design: the next refresh puts the run's own status back.
        self.status_bar.set_text(0, self.tr().copied);
    }

    /// Writes the same text to a file the tester picks, suggested as
    /// `<suite>-<run id>.md` next to the suite — a report worth keeping is
    /// usually one that goes into a ticket or a commit.
    fn save_report(&self) {
        let Some(text) = self.report_text() else { return };
        // Everything the dialog needs is read out before it opens: it pumps
        // messages, and a `Notice` arriving inside it re-enters `drain_dialog`.
        let (folder, suggested, title, filters) = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            let stem = state.suite.as_ref().map(|s| s.stem.as_str()).unwrap_or("jafiz");
            let suggested = match viewed_run(&state) {
                Some(run) => format!("{stem}-{}.md", run.id),
                None => format!("{stem}.md"),
            };
            (
                // Deliberately NOT `state.location`: that is the suite folder,
                // and `store::list_suites` reads every `.md` it finds there as
                // a suite. A report saved next to the suite is picked back up
                // on the next launch and shows up in the suite picker — and in
                // `jafiz list` — as though it were a real one. Documents is a
                // default a tester saving a report is not likely to mind.
                documents_dir(),
                suggested,
                tr.menu_report_save.trim_end_matches('…'),
                [tr.filter_md, tr.filter_all],
            )
        };
        let Some(path) = save_file_dialog(&self.window, title, &folder, &suggested, &filters)
        else {
            return;
        };
        if let Err(error) = std::fs::write(&path, &text) {
            let message = format!("{}: {error}", path.display());
            nwg::modal_error_message(&self.window.handle, self.tr().app_title, &message);
            return;
        }
        let saved = self.tr().saved.replace("%P", &path.display().to_string());
        self.status_bar.set_text(0, &saved);
    }

    /// Opens the run history. The rows are rendered here, on the UI thread,
    /// because computing progress needs the suite — and the dialog thread has
    /// no business borrowing the window's state.
    fn show_history(&self) {
        let prepared = {
            let state = self.state.borrow();
            let Some(suite) = state.suite.as_ref() else { return };
            let tr = t(state.lang);
            // Newest first: the run someone goes looking for is nearly always
            // a recent one, and `RunFile::runs` is stored oldest first.
            let rows: Vec<history_dialog::HistoryRow> = state
                .run_file
                .runs
                .iter()
                .rev()
                .map(|run| {
                    let counts = tally(suite, Some(run));
                    history_dialog::HistoryRow {
                        id: run.id.clone(),
                        cells: [
                            run.id.clone(),
                            run.started_at.clone(),
                            run.finished_at.clone().unwrap_or_else(|| tr.hist_running.into()),
                            run.environment.clone(),
                            format!("{}/{}", counts.done_steps, counts.total_steps),
                        ],
                    }
                })
                .collect();
            // Start on the run already on screen, so OK without touching
            // anything is a no-op rather than a jump.
            let showing = viewed_run(&state).map(|run| run.id.clone());
            let selected = showing.and_then(|id| rows.iter().position(|row| row.id == id));
            (state.lang, rows, selected)
        };
        let (lang, rows, selected) = prepared;
        if rows.is_empty() {
            self.warn_no_run();
            return;
        }
        self.window.set_enabled(false);
        history_dialog::spawn(history_dialog::HistoryParams {
            lang,
            rows,
            selected,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Points the window at the run picked in Histórico.
    fn apply_viewed_run(&self, id: &str) {
        {
            let mut state = self.state.borrow_mut();
            // Picking the active run clears the override rather than storing
            // its id: `viewed_run: None` is the same run, and it is the state
            // the read-only banner and the disabled buttons key off.
            state.viewed_run = match state.run_file.active_run.as_deref() {
                Some(active) if active == id => None,
                _ => Some(id.to_string()),
            };
        }
        self.refresh_all();
    }

    /// Opens the preferences window — the same section-sidebar dialog MCP
    /// Console uses, over JAFIZ's own string table.
    fn open_preferences(&self) {
        let lang = self.state.borrow().lang;
        self.window.set_enabled(false);
        preferences_dialog::spawn(preferences_dialog::PreferencesParams {
            lang,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Switches language in place, exactly as MCP Console does: the window
    /// keeps its suite, its run and its selection, and every caption is
    /// rewritten. Restarting to see a menu in another language would mean
    /// losing the run in progress.
    fn apply_language(&self, lang: Lang) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.lang != lang;
            state.lang = lang;
            state.settings.lang = lang;
            changed
        };
        if !changed {
            return;
        }
        // The CLI reads the same setting, so a report asked for in another
        // terminal comes back in the language just chosen here.
        let outcome = self.state.borrow().settings.save();
        if let Err(error) = outcome {
            nwg::modal_error_message(&self.window.handle, self.tr().app_title, &error);
        }
        self.relabel_all();
    }
}

/// The Save-as dialog's default folder: the user's Documents, resolved the
/// same way the rest of the workspace resolves user folders — through
/// `std::env` rather than `SHGetKnownFolderPath` — so this stays free of a
/// new dependency (compare `store::library_dir`, which reads `LOCALAPPDATA`
/// with a `USERPROFILE`-based fallback the same way).
///
/// Returns an empty path when `USERPROFILE` is not set at all; `save_file_dialog`
/// already treats a non-directory `folder` as "no default", so that is enough
/// to leave the dialog with whatever folder Windows last remembered.
fn documents_dir() -> PathBuf {
    let Ok(profile) = std::env::var("USERPROFILE") else {
        return PathBuf::new();
    };
    let profile = PathBuf::from(profile);
    let documents = profile.join("Documents");
    if documents.is_dir() {
        documents
    } else {
        profile
    }
}

/// A Save-as dialog with the file name already filled in.
///
/// `nwg::FileDialog` cannot do that: its builder has no default-name option,
/// and it keeps its `IFileDialog` pointer private, so `SetFileName` is out of
/// reach. Suggesting `<suite>-<run id>.md` is most of the point of this command
/// — the tester should be able to press Save, not compose a file name — so it
/// goes straight to `GetSaveFileNameW`, which takes the suggestion in the very
/// buffer it writes the answer back into. Returns `None` when cancelled.
fn save_file_dialog(
    parent: &nwg::Window,
    title: &str,
    folder: &std::path::Path,
    suggested: &str,
    filters: &[&str; 2],
) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use winapi::um::commdlg::{
        GetSaveFileNameW, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };

    // The suggestion goes in, the chosen path comes back out, in one buffer.
    // 32K UTF-16 units is the ceiling the shell itself works to.
    let mut file = wide(suggested);
    file.resize(32768, 0);

    // comdlg32 wants NUL-separated label/pattern pairs closed by a second NUL —
    // not something a plain string can carry.
    let mut filter: Vec<u16> = Vec::new();
    for (label, pattern) in [(filters[0], "*.md"), (filters[1], "*.*")] {
        filter.extend(wide(label));
        filter.extend(wide(pattern));
    }
    filter.push(0);

    let dir: Vec<u16> =
        folder.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let title = wide(title);
    let extension = wide("md");

    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = parent.handle.hwnd().unwrap_or(std::ptr::null_mut());
    ofn.lpstrFilter = filter.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrInitialDir = if folder.is_dir() { dir.as_ptr() } else { std::ptr::null() };
    ofn.lpstrTitle = title.as_ptr();
    // Appended when the tester types a bare name, so "checkout" still lands as
    // a `.md` the way the filter promises.
    ofn.lpstrDefExt = extension.as_ptr();
    // NOCHANGEDIR matters more than it looks: without it the dialog leaves the
    // process sitting in whatever folder was browsed, and the suite-location
    // cascade walks up from the current directory.
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;

    // SAFETY: `ofn` is zeroed, so no field is left uninitialised; every pointer
    // it holds refers to a NUL-terminated buffer that outlives the call; and
    // `lpstrFile` is writable with `nMaxFile` units of room, which is what the
    // dialog writes the answer into.
    let confirmed = unsafe { GetSaveFileNameW(&mut ofn) };
    if confirmed == 0 {
        return None; // cancelled, or the dialog could not be shown
    }
    let end = file.iter().position(|&unit| unit == 0).unwrap_or(file.len());
    Some(PathBuf::from(std::ffi::OsString::from_wide(&file[..end])))
}
