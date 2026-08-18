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
use jafiz::settings::AppSettings;
use jafiz::store::{self, LocationKind};
// `jafiz::runs` is imported when recording arrives — an unused import here
// would fail `-D warnings`.

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
const VK_F5: u32 = 0x74;

/// Mailbox the modal dialog threads fill; drained on the UI thread when the
/// Notice fires. `pub(crate)` because the dialog modules write into it.
#[derive(Default)]
pub(crate) struct Shared {
    /// `Some(None)` means the dialog was cancelled.
    pub environment: Option<Option<String>>,
    pub note: Option<Option<String>>,
}

struct UiState {
    lang: Lang,
    settings: AppSettings,
    location: PathBuf,
    location_kind: LocationKind,
    /// Suite files in the current location, in list order.
    suite_paths: Vec<PathBuf>,
    suite: Option<Suite>,
    diagnostics: Vec<Diagnostic>,
    run_file: RunFile,
    /// Index into `suite.scenarios`.
    selected_scenario: Option<usize>,
    /// 1-based step number within the selected scenario.
    selected_step: Option<usize>,
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
    /// The top-level menu-bar entries. Nothing reads them, but `nwg::Menu`'s
    /// `Drop` calls `DestroyMenu` — let these go out of scope at the end of
    /// `build_app` and the whole menu bar, items included, is destroyed before
    /// the window is ever shown.
    _menus: Vec<nwg::Menu>,
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
        _menus: vec![menu_file, menu_run, menu_report, menu_tools, menu_help],
        state: RefCell::new(UiState {
            lang,
            settings,
            location: PathBuf::new(),
            location_kind: LocationKind::Library,
            suite_paths: Vec::new(),
            suite: None,
            diagnostics: Vec::new(),
            run_file: RunFile::default(),
            selected_scenario: None,
            selected_step: None,
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
            E::OnButtonClick if handle == app.btn_note.handle => app.open_note_dialog(),
            E::OnMenuItemSelected => {
                if handle == app.menu_open.handle {
                    app.open_folder();
                } else if handle == app.menu_reload.handle {
                    app.load_selected_suite();
                } else if handle == app.menu_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.menu_run_new.handle {
                    app.open_run_dialog();
                } else if handle == app.menu_help_format.handle {
                    app.show_format_help();
                } else if handle == app.menu_about.handle {
                    app.show_about();
                } else if handle == app.menu_run_env.handle
                    || handle == app.menu_run_finish.handle
                    || handle == app.menu_run_history.handle
                    || handle == app.menu_report_copy.handle
                    || handle == app.menu_report_save.handle
                    || handle == app.menu_prefs.handle
                {
                    app.not_wired_yet();
                }
            }
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_F5) = evt_data {
                    app.load_selected_suite();
                }
            }
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

        let mut x = steps_x;
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

    /// Opens whichever folder the user had last, falling back to the engine's
    /// resolution cascade — so launching from the hub (cwd = the tool folder)
    /// still lands on the library rather than nowhere.
    fn open_initial_location(&self) {
        // Both preferences are read before anything is loaded: opening the
        // folder immediately rewrites `last_suite` to whatever the picker
        // lands on, which would erase the suite we are about to restore.
        let (remembered_dir, remembered_suite) = {
            let state = self.state.borrow();
            (state.settings.last_location.clone(), state.settings.last_suite.clone())
        };
        // A remembered folder that has since been deleted must not strand the
        // window on an empty list — fall through to the cascade instead.
        let remembered_dir = remembered_dir.filter(|dir| dir.is_dir());
        let location = store::resolve_location(remembered_dir.as_deref());
        self.load_location(&location.dir, location.kind);
        if let Some(stem) = remembered_suite {
            self.select_suite_by_stem(&stem);
        }
    }

    /// Reads the folder, fills the suite picker, and opens the first suite.
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
            state.settings.remember_location(dir);
            let _ = state.settings.save();
        }
        self.suite_combo.set_collection(names);
        // Which rung of the cascade produced the folder is part of the answer
        // to "why am I looking at these suites" — a `--dir`, `JAFIZ_DIR`, the
        // enclosing project or the central library all look alike otherwise.
        self.location_label.set_text(&format!(
            "{} {} ({})",
            self.tr().lbl_location,
            dir.display(),
            kind.label()
        ));
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
            match path.as_ref().map(|p| store::load_suite(p)) {
                Some(Ok(outcome)) => {
                    state.run_file = store::load_runs(&dir, &outcome.suite.stem);
                    state.settings.last_suite = Some(outcome.suite.stem.clone());
                    let _ = state.settings.save();
                    state.diagnostics = outcome.diagnostics;
                    state.suite = Some(outcome.suite);
                }
                _ => {
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
        self.refresh_buttons();
        self.update_status();
    }

    /// Points the picker at a named suite, if the current folder has one.
    /// Used to restore the suite the user had open last.
    fn select_suite_by_stem(&self, stem: &str) {
        let index = {
            let state = self.state.borrow();
            state
                .suite_paths
                .iter()
                .position(|p| p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(stem)))
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
            let mut text = match state.suite.as_ref() {
                None => tr.no_suites.replace("%D", &state.location.display().to_string()),
                Some(suite) => match state.run_file.latest() {
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
        self.refresh_buttons();
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
        self.refresh_buttons();
    }

    /// The verdict buttons only mean something with a step to mark, so they
    /// follow the selection.
    fn refresh_buttons(&self) {
        let enabled = {
            let Ok(state) = self.state.try_borrow() else { return };
            state.selected_scenario.is_some() && state.selected_step.is_some()
        };
        for button in [
            &self.btn_pass,
            &self.btn_fail,
            &self.btn_blocked,
            &self.btn_skip,
            &self.btn_note,
            &self.btn_evidence,
        ] {
            button.set_enabled(enabled);
        }
    }

    /// Picks a different suite folder. Chosen by hand, so it enters the
    /// cascade as `Explicit` — the same rung `--dir` uses on the CLI.
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
        self.load_location(&PathBuf::from(chosen), LocationKind::Explicit);
    }

    /// Opens the "new run" dialog. The window is disabled for as long as the
    /// dialog owns the interaction — the dialog runs on its own thread, so
    /// nothing else enforces modality.
    fn open_run_dialog(&self) {
        self.window.set_enabled(false);
        run_dialog::spawn(run_dialog::RunDialogParams {
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Opens the step-note dialog, on the same terms as `open_run_dialog`.
    fn open_note_dialog(&self) {
        self.window.set_enabled(false);
        note_dialog::spawn(note_dialog::NoteDialogParams {
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Drains whatever a dialog thread left behind and hands the window back.
    ///
    /// Recording turns these outcomes into a run and a step note; today both
    /// dialogs are stubs that always report a cancellation, so the mailbox is
    /// read and emptied — leaving a stale entry there would make the next
    /// dialog return someone else's answer.
    fn drain_dialog(&self) {
        let answered = {
            let mut shared = self.shared.lock().unwrap();
            let environment = shared.environment.take();
            let note = shared.note.take();
            environment.is_some() || note.is_some()
        };
        if answered {
            self.window.set_enabled(true);
        }
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

    /// Menu commands that are routed but not implemented yet: recording a
    /// verdict's environment, finishing a run, the run history, and the report
    /// and preferences actions. Routing them now is what keeps every menu
    /// handle live and the whole menu bar readable in one match.
    fn not_wired_yet(&self) {}
}
