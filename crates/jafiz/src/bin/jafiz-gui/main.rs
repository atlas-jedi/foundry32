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

#![windows_subsystem = "windows"]

mod i18n;
mod note_dialog;
mod run_dialog;

use i18n::{t, Lang, T};

use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{apply_window_icon, insert_report_list_view_column};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use jafiz::model::{scenario_done, scenario_status, RunFile, StepStatus, Suite};
use jafiz::parser::Diagnostic;
use jafiz::report;
use jafiz::runs;
use jafiz::settings::AppSettings;
use jafiz::store::{self, LocationKind};

/// Shown in the About box.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Scenario list columns.
const SC_STATUS: usize = 0;
const SC_ID: usize = 1;
const SC_TITLE: usize = 2;
const SC_PROGRESS: usize = 3;

// Step list columns.
const ST_NUM: usize = 0;
const ST_STATUS: usize = 1;
const ST_ACTION: usize = 2;
const ST_EXPECTED: usize = 3;
const ST_NOTE: usize = 4;

const MARGIN: i32 = 8;
const HEADER_H: i32 = 26;
const BUTTON_W: i32 = 116;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const STATUS_H: i32 = 24;
const SCENARIOS_W: i32 = 330;
// The verdict keys. F1 is help and F5 is reload by Windows convention, so the
// four verdicts take what is left around them — and the button captions say so.
const VK_F2: u32 = 0x71;
const VK_F3: u32 = 0x72;
const VK_F4: u32 = 0x73;
const VK_F5: u32 = 0x74;
const VK_F6: u32 = 0x75;

/// Mailbox the modal dialog threads fill; drained on the UI thread when the
/// Notice fires. `pub(crate)` because the dialog modules write into it.
#[derive(Default)]
pub(crate) struct Shared {
    /// `Some(None)` means the dialog was cancelled.
    pub environment: Option<Option<String>>,
    pub note: Option<Option<String>>,
}

/// What the environment dialog's answer applies to. `Nova execução…` and
/// `Ambiente…` are the same window and answer through the same mailbox slot,
/// so the main window records which of the two it asked for before spawning
/// the thread — reading it back off the menu handle is not possible once the
/// answer arrives on a `Notice`.
#[derive(Clone, Copy)]
enum EnvTarget {
    /// Start a run with the answer.
    NewRun,
    /// Re-describe the environment of the run already open.
    ActiveRun,
}

struct UiState {
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
    /// Index into `suite.scenarios`.
    selected_scenario: Option<usize>,
    /// 1-based step number within the selected scenario.
    selected_step: Option<usize>,
    /// What the environment dialog currently on screen was asked for.
    env_target: EnvTarget,
    /// The scenario id and step number the open note dialog is annotating,
    /// captured when it was spawned. Re-reading the selection when the answer
    /// arrives would attach the note to whatever is selected *then*, which is
    /// not necessarily what the tester was looking at when they typed it.
    note_target: Option<(String, usize)>,
}

struct JafizApp {
    window: nwg::Window,
    suite_label: nwg::Label,
    suite_combo: nwg::ComboBox<String>,
    location_label: nwg::Label,
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
    menu_run_new: nwg::MenuItem,
    menu_run_env: nwg::MenuItem,
    menu_run_finish: nwg::MenuItem,
    menu_run_history: nwg::MenuItem,
    menu_report_copy: nwg::MenuItem,
    menu_report_save: nwg::MenuItem,
    menu_prefs: nwg::MenuItem,
    menu_help_format: nwg::MenuItem,
    menu_about: nwg::MenuItem,
    // The five top-level menu-bar entries. Nothing reads them yet — hence the
    // `_` prefix, which is what keeps `dead_code` quiet — but they cannot be
    // locals: `nwg::Menu`'s `Drop` calls `DestroyMenu`, so letting them go out
    // of scope at the end of `build_app` would destroy the whole menu bar,
    // items included, before the window is ever shown. They are declared after
    // the `MenuItem` fields on purpose: fields drop in declaration order, and
    // `MenuItem`'s `Drop` calls `DeleteMenu` on its parent, which must still
    // exist. Language switching will read them through
    // `foundry_common::ui::set_submenu_text`, one named field each.
    _menu_file: nwg::Menu,
    _menu_run: nwg::Menu,
    _menu_report: nwg::Menu,
    _menu_tools: nwg::Menu,
    _menu_help: nwg::Menu,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
}

fn main() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");

    let settings = AppSettings::load();
    let app = Rc::new(build_app(settings));
    wire_events(&app);
    app.layout();
    app.open_initial_location();
    nwg::dispatch_thread_events();
}

fn build_app(settings: AppSettings) -> JafizApp {
    let lang = settings.lang;
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((1020, 640))
        .position((160, 100))
        .title(tr.app_title)
        .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::RESIZABLE)
        .build(&mut window)
        .expect("window");

    // Menu bar. `nwg::Menu` is a top-level submenu, `nwg::MenuItem` a command;
    // both are addressed by handle in the event handler.
    let submenu = |text: &str, window: &nwg::Window| {
        let mut menu = nwg::Menu::default();
        nwg::Menu::builder().parent(window).text(text).build(&mut menu).expect("menu");
        menu
    };
    let item = |text: &str, parent: &nwg::Menu| {
        let mut menu_item = nwg::MenuItem::default();
        nwg::MenuItem::builder().parent(parent).text(text).build(&mut menu_item).expect("item");
        menu_item
    };
    let menu_file = submenu(tr.menu_file, &window);
    let menu_open = item(tr.menu_open, &menu_file);
    let menu_reload = item(tr.menu_reload, &menu_file);
    let menu_exit = item(tr.menu_exit, &menu_file);
    let menu_run = submenu(tr.menu_run, &window);
    let menu_run_new = item(tr.menu_run_new, &menu_run);
    let menu_run_env = item(tr.menu_run_env, &menu_run);
    let menu_run_finish = item(tr.menu_run_finish, &menu_run);
    let menu_run_history = item(tr.menu_run_history, &menu_run);
    let menu_report = submenu(tr.menu_report, &window);
    let menu_report_copy = item(tr.menu_report_copy, &menu_report);
    let menu_report_save = item(tr.menu_report_save, &menu_report);
    let menu_tools = submenu(tr.menu_tools, &window);
    let menu_prefs = item(tr.menu_prefs, &menu_tools);
    let menu_help = submenu(tr.menu_help, &window);
    let menu_help_format = item(tr.menu_help_format, &menu_help);
    let menu_about = item(tr.menu_about, &menu_help);

    // Header row: suite picker + the folder the suites came from.
    let mut suite_label = nwg::Label::default();
    nwg::Label::builder().parent(&window).text(tr.lbl_suite).build(&mut suite_label).expect("suite_label");
    let mut suite_combo: nwg::ComboBox<String> = nwg::ComboBox::default();
    nwg::ComboBox::builder().parent(&window).collection(Vec::new()).build(&mut suite_combo).expect("suite_combo");
    let mut location_label = nwg::Label::default();
    nwg::Label::builder().parent(&window).text("").build(&mut location_label).expect("location_label");

    // The two report-style lists.
    let list = |parent: &nwg::Window| {
        let mut listview = nwg::ListView::default();
        nwg::ListView::builder()
            .parent(parent)
            .list_style(nwg::ListViewStyle::Detailed)
            .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
            .build(&mut listview)
            .expect("listview");
        // nwg forces LVS_NOCOLUMNHEADER at creation for backward compatibility.
        listview.set_headers_enabled(true);
        apply_explorer_theme(&listview.handle);
        listview
    };
    let scenarios = list(&window);
    for (index, width, title) in [
        (SC_STATUS, 46, tr.col_status),
        (SC_ID, 60, tr.col_id),
        (SC_TITLE, 170, tr.col_scenario),
        (SC_PROGRESS, 52, tr.col_progress),
    ] {
        insert_report_list_view_column(&scenarios, index as i32, width, title);
    }
    let steps = list(&window);
    for (index, width, title) in [
        (ST_NUM, 32, tr.col_num),
        (ST_STATUS, 46, tr.col_status),
        (ST_ACTION, 250, tr.col_action),
        (ST_EXPECTED, 250, tr.col_expected),
        (ST_NOTE, 200, tr.col_note),
    ] {
        insert_report_list_view_column(&steps, index as i32, width, title);
    }

    // Verdict buttons, under the step list — where the tester's eyes already
    // are while reading the step they just executed.
    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let btn_pass = button(tr.btn_pass, &window);
    let btn_fail = button(tr.btn_fail, &window);
    let btn_blocked = button(tr.btn_blocked, &window);
    let btn_skip = button(tr.btn_skip, &window);
    let btn_note = button(tr.btn_note, &window);
    let btn_evidence = button(tr.btn_evidence, &window);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    // Title bar, taskbar and Alt+Tab icons (absent on plain GNU dev builds).
    apply_window_icon(&window.handle);

    // Every verdict button starts disabled — nothing is selected yet.
    for control in [&btn_pass, &btn_fail, &btn_blocked, &btn_skip, &btn_note, &btn_evidence] {
        control.set_enabled(false);
    }

    JafizApp {
        window,
        suite_label,
        suite_combo,
        location_label,
        scenarios,
        steps,
        btn_pass,
        btn_fail,
        btn_blocked,
        btn_skip,
        btn_note,
        btn_evidence,
        status_bar,
        notice,
        menu_open,
        menu_reload,
        menu_exit,
        menu_run_new,
        menu_run_env,
        menu_run_finish,
        menu_run_history,
        menu_report_copy,
        menu_report_save,
        menu_prefs,
        menu_help_format,
        menu_about,
        _menu_file: menu_file,
        _menu_run: menu_run,
        _menu_report: menu_report,
        _menu_tools: menu_tools,
        _menu_help: menu_help,
        state: RefCell::new(UiState {
            lang,
            settings,
            location: PathBuf::new(),
            location_kind: LocationKind::Library,
            suite_paths: Vec::new(),
            suite: None,
            load_error: None,
            diagnostics: Vec::new(),
            run_file: RunFile::default(),
            selected_scenario: None,
            selected_step: None,
            env_target: EnvTarget::NewRun,
            note_target: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
    }
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
                } else if handle == app.menu_run_new.handle {
                    app.new_run();
                } else if handle == app.menu_run_env.handle {
                    app.edit_environment();
                } else if handle == app.menu_run_finish.handle {
                    app.finish_run();
                } else if handle == app.menu_help_format.handle {
                    app.show_format_help();
                } else if handle == app.menu_about.handle {
                    app.show_about();
                } else if handle == app.menu_run_history.handle
                    || handle == app.menu_report_copy.handle
                    || handle == app.menu_report_save.handle
                    || handle == app.menu_prefs.handle
                {
                    app.not_wired_yet();
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

/// Moves a list view's selection to exactly one row.
///
/// `select_item` only *adds* to the selection — neither list is `LVS_SINGLESEL`
/// — so the rows the cursor moved off are cleared first. Otherwise every
/// advance would leave another row highlighted behind it, and the tester would
/// be looking at a list that claims three steps are current.
fn select_only(list: &nwg::ListView, row: usize) {
    for selected in list.selected_items() {
        if selected != row {
            list.select_item(selected, false);
        }
    }
    list.select_item(row, true);
}

/// Scrolls a row into view. A scenario can have more steps than the list
/// shows, and a current step the tester has to scroll to find defeats the
/// point of the marker moving on its own. nwg has no wrapper for this one.
fn ensure_visible(list: &nwg::ListView, row: usize) {
    use winapi::um::commctrl::LVM_ENSUREVISIBLE;
    let Some(hwnd) = list.handle.hwnd() else { return };
    // SAFETY: a live list-view HWND, and LVM_ENSUREVISIBLE takes no pointer —
    // wParam is the row index and lParam a "partial is enough" flag.
    unsafe {
        winapi::um::winuser::SendMessageW(hwnd, LVM_ENSUREVISIBLE, row, 0);
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

impl JafizApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    /// Header row across the top, scenarios on the left, steps on the right
    /// with the verdict buttons under them, status bar at the bottom.
    fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < 560 || height < 320 {
            return;
        }
        let header_y = MARGIN;
        let list_y = header_y + HEADER_H + MARGIN;
        let button_y = height - STATUS_H - BUTTON_H - MARGIN;
        let list_h = (button_y - list_y - MARGIN).max(80) as u32;

        self.suite_label.set_position(MARGIN, header_y + 4);
        self.suite_label.set_size(44, 20);
        self.suite_combo.set_position(MARGIN + 48, header_y);
        self.suite_combo.set_size(280, 24);
        self.location_label.set_position(MARGIN + 340, header_y + 4);
        self.location_label.set_size((width - MARGIN - 348).max(80) as u32, 20);

        self.scenarios.set_position(MARGIN, list_y);
        self.scenarios.set_size(SCENARIOS_W as u32, list_h);
        let steps_x = MARGIN + SCENARIOS_W + MARGIN;
        self.steps.set_position(steps_x, list_y);
        self.steps.set_size((width - steps_x - MARGIN).max(160) as u32, list_h);

        // Anchor the verdict row to the right edge: fixed-width buttons laid
        // out from the steps list would run past the client area at the
        // default window size. Below the width the row needs, it clamps to the
        // left margin — the best a fixed-width row can do while staying
        // on-screen from the left.
        let total = 6 * BUTTON_W + 5 * BUTTON_GAP;
        let mut x = (width - MARGIN - total).max(MARGIN);
        for button in [
            &self.btn_pass,
            &self.btn_fail,
            &self.btn_blocked,
            &self.btn_skip,
            &self.btn_note,
            &self.btn_evidence,
        ] {
            button.set_position(x, button_y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
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
        // Which rung of the cascade produced the folder is part of the answer
        // to "why am I looking at these suites" — a folder the user picked,
        // `JAFIZ_DIR`, the enclosing project or the central library all look
        // alike otherwise. The wording is the GUI's own: `LocationKind::label`
        // is a pt-BR CLI constant and would leak into an English window.
        let tr = self.tr();
        let source = match kind {
            LocationKind::Explicit => tr.loc_explicit,
            LocationKind::Env => tr.loc_env,
            LocationKind::Project => tr.loc_project,
            LocationKind::Library => tr.loc_library,
        };
        self.location_label
            .set_text(&format!("{} {} ({})", tr.lbl_location, dir.display(), source));
        self.suite_combo.set_selection(Some(0));
        self.load_selected_suite();
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
                    state.run_file = store::load_runs(&dir, &outcome.suite.stem);
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
                }
                None => {
                    state.suite = None;
                    state.diagnostics.clear();
                    state.run_file = RunFile::default();
                }
            }
            state.selected_scenario = None;
            state.selected_step = None;
        }
        self.populate_scenarios();
        self.populate_steps();
        self.update_buttons();
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
        let run = state.run_file.latest();
        for scenario in &suite.scenarios {
            let row = [
                scenario_status(scenario, run).symbol().to_string(),
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
        let run = state.run_file.latest();
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
                (None, Some(suite)) => match state.run_file.latest() {
                    None => format!("{} · {}", suite.title, tr.no_run),
                    Some(run) => report::status_line(suite, Some(run), state.lang),
                },
            };
            // A suite the parser could not fully read shows fewer scenarios
            // than the file has. Say so here rather than let the list quietly
            // lie about what is going to be tested.
            if !state.diagnostics.is_empty() {
                let count = state.diagnostics.len().to_string();
                text.push_str(&format!(" · {}", tr.diagnostics.replace("%N", &count)));
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
    fn update_buttons(&self) {
        let recording = {
            // Refilling a list re-enters the selection handlers while
            // `populate_*` still holds the state borrowed; skipping the update
            // is right, because the refill calls back into here afterwards.
            let Ok(state) = self.state.try_borrow() else { return };
            state.selected_scenario.is_some()
                && state.selected_step.is_some()
                && state.run_file.active().is_some()
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
        self.open_env_dialog(lang, environment, t(lang).run_dlg_title);
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
        self.open_env_dialog(lang, environment, t(lang).menu_run_env.trim_end_matches('…'));
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
        let (environment, note) = {
            let mut shared = self.shared.lock().unwrap();
            (shared.environment.take(), shared.note.take())
        };
        if let Some(Some(text)) = environment {
            // Read out of the borrow before dispatching: both arms borrow the
            // state mutably.
            let target = self.state.borrow().env_target;
            match target {
                EnvTarget::NewRun => self.apply_new_run(&text),
                EnvTarget::ActiveRun => self.apply_environment(&text),
            }
        }
        match note {
            Some(Some(text)) => self.apply_note(&text),
            // Cancelled: forget which step was being annotated, so a later
            // answer can never land on a step the tester has moved off.
            Some(None) => self.state.borrow_mut().note_target = None,
            None => {}
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
        }
        self.save_runs();
        self.refresh_all();
        self.follow_cursor();
    }

    /// Applies a re-typed environment to the run already open.
    fn apply_environment(&self, environment: &str) {
        {
            let mut state = self.state.borrow_mut();
            if !runs::set_environment(&mut state.run_file, environment) {
                return; // the run was finished while the dialog was up
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
    /// into. The state borrow is released before the warning: a modal pumps
    /// messages, and a `Notice` arriving inside one re-enters `drain_dialog`.
    fn recording_target(&self) -> Option<(String, usize)> {
        let (recording, target) = {
            let state = self.state.borrow();
            let target = state
                .suite
                .as_ref()
                .zip(state.selected_scenario)
                .and_then(|(suite, index)| suite.scenarios.get(index))
                .map(|scenario| scenario.id.clone())
                .zip(state.selected_step);
            (state.run_file.active().is_some(), target)
        };
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
    fn show_format_help(&self) {
        let (tr, guide) = {
            let state = self.state.borrow();
            let guide = match state.lang {
                Lang::PtBr => jafiz::parser::GUIDE_PT,
                Lang::En => jafiz::parser::GUIDE_EN,
            };
            (t(state.lang), guide)
        };
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

    /// Menu commands that are routed but not implemented yet: the run history
    /// and the report and preferences actions. Routing them now is what keeps
    /// every menu handle live and the whole menu bar readable in one match.
    fn not_wired_yet(&self) {}
}
