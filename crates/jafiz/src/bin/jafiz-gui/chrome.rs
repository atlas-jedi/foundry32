//! The window itself: what it is made of, where the pieces sit, and what they
//! are called. Everything here is chrome — construction, geometry and captions
//! — so `main.rs` is left holding behaviour alone.
//!
//! `JafizApp` and `UiState` stay declared in the crate root because that is
//! where the borrow rules are simplest, and Rust makes their private fields
//! visible to this module anyway: an item without `pub` is reachable from the
//! module that declares it *and its descendants*, and `chrome` is a child of
//! the root.

use crate::i18n::{t, Lang};
use crate::{EnvTarget, JafizApp, Shared, UiState};

use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{
    apply_window_icon, insert_report_list_view_column, set_menu_item_text, set_submenu_text,
};
use native_windows_gui as nwg;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jafiz::model::RunFile;
use jafiz::settings::AppSettings;
use jafiz::store::LocationKind;

// Scenario list columns.
pub(crate) const SC_STATUS: usize = 0;
pub(crate) const SC_ID: usize = 1;
pub(crate) const SC_TITLE: usize = 2;
pub(crate) const SC_PROGRESS: usize = 3;

// Step list columns.
pub(crate) const ST_NUM: usize = 0;
pub(crate) const ST_STATUS: usize = 1;
pub(crate) const ST_ACTION: usize = 2;
pub(crate) const ST_EXPECTED: usize = 3;
pub(crate) const ST_NOTE: usize = 4;

const MARGIN: i32 = 8;
const HEADER_H: i32 = 26;
const ROW_GAP: i32 = 6;
const TOOLBAR_H: i32 = 26;
const BUTTON_W: i32 = 116;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const STATUS_H: i32 = 24;

// Run toolbar. Fixed widths, sized to the longest caption either language puts
// on them; only the environment picker stretches with the window.
const RUN_NEW_W: i32 = 140;
const RUN_FINISH_W: i32 = 110;
const ENV_LABEL_W: i32 = 84;
const HISTORY_W: i32 = 110;

/// The draggable gap between the two lists — the same 8px the fixed layout
/// used to leave there, so nothing looks different until someone grabs it.
pub(crate) const SPLITTER_W: i32 = 8;

/// Where the divider sits until the tester moves it.
pub(crate) const SCENARIOS_W_DEFAULT: i32 = 330;

// How far the divider can travel. Past either end one of the two lists stops
// being readable, which is the very problem the divider exists to fix.
const SCENARIOS_W_MIN: i32 = 140;
const STEPS_W_MIN: i32 = 220;

// Below this there is no room left to lay anything out: `layout` bails, and the
// divider reports no rectangle, so nothing can be dragged either.
const MIN_W: i32 = 560;
const MIN_H: i32 = 360;

/// Builds every control the window owns and hands back the assembled app.
pub(crate) fn build_app(settings: AppSettings) -> JafizApp {
    let lang = settings.lang;
    let tr = t(lang);
    let scenarios_w = settings.scenarios_width.unwrap_or(SCENARIOS_W_DEFAULT);

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
    //
    // There is deliberately no "Execução" menu: everything a run needs — start,
    // finish, environment, history — sits on the toolbar under the header,
    // where each control being enabled or greyed out says whether a run is open
    // at all. The same four commands behind a menu read as available always.
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

    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };

    // Run toolbar, second row: start a run, finish it, say where it is being
    // run, and reach the ones already recorded.
    let btn_run_new = button(tr.btn_run_new, &window);
    let btn_run_finish = button(tr.btn_run_finish, &window);
    let mut env_label = nwg::Label::default();
    nwg::Label::builder().parent(&window).text(tr.lbl_env).build(&mut env_label).expect("env_label");
    let mut env_combo: nwg::ComboBox<String> = nwg::ComboBox::default();
    nwg::ComboBox::builder().parent(&window).collection(Vec::new()).build(&mut env_combo).expect("env_combo");
    let btn_history = button(tr.btn_history, &window);

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
    for (index, width, title) in scenario_columns(lang) {
        insert_report_list_view_column(&scenarios, index as i32, width, title);
    }
    let steps = list(&window);
    for (index, width, title) in step_columns(lang) {
        insert_report_list_view_column(&steps, index as i32, width, title);
    }

    // Verdict buttons, under the step list — where the tester's eyes already
    // are while reading the step they just executed.
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

    // Every verdict button starts disabled — nothing is selected yet — and so
    // does everything on the toolbar that acts on a run, because there is none.
    for control in [&btn_pass, &btn_fail, &btn_blocked, &btn_skip, &btn_note, &btn_evidence] {
        control.set_enabled(false);
    }
    for control in [&btn_run_new, &btn_run_finish, &btn_history] {
        control.set_enabled(false);
    }
    env_combo.set_enabled(false);

    JafizApp {
        window,
        suite_label,
        suite_combo,
        location_label,
        btn_run_new,
        btn_run_finish,
        env_label,
        env_combo,
        btn_history,
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
        menu_report_copy,
        menu_report_save,
        menu_prefs,
        menu_help_format,
        menu_about,
        menu_file,
        menu_report,
        menu_tools,
        menu_help,
        scenarios_w: Cell::new(scenarios_w),
        dragging: Cell::new(false),
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
            runs_damage: None,
            selected_scenario: None,
            selected_step: None,
            viewed_run: None,
            env_target: EnvTarget::NewRun,
            env_choices: Vec::new(),
            note_target: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
    }
}

/// Index, width and caption of each scenario-list column, in one place so
/// building the list and re-captioning it after a language change can never
/// disagree about how many columns there are.
fn scenario_columns(lang: Lang) -> [(usize, i32, &'static str); 4] {
    let tr = t(lang);
    [
        (SC_STATUS, 46, tr.col_status),
        (SC_ID, 60, tr.col_id),
        (SC_TITLE, 170, tr.col_scenario),
        (SC_PROGRESS, 52, tr.col_progress),
    ]
}

/// Index, width and caption of each step-list column. See `scenario_columns`.
fn step_columns(lang: Lang) -> [(usize, i32, &'static str); 5] {
    let tr = t(lang);
    [
        (ST_NUM, 32, tr.col_num),
        (ST_STATUS, 46, tr.col_status),
        (ST_ACTION, 250, tr.col_action),
        (ST_EXPECTED, 250, tr.col_expected),
        (ST_NOTE, 200, tr.col_note),
    ]
}

/// The horizontal bands the window is cut into, top to bottom. `layout` and the
/// divider's hit test both read them from here, so a drag can never grab a
/// strip of window the lists do not actually occupy.
struct Rows {
    header_y: i32,
    toolbar_y: i32,
    list_y: i32,
    list_h: i32,
    button_y: i32,
}

fn rows(height: i32) -> Rows {
    let header_y = MARGIN;
    let toolbar_y = header_y + HEADER_H + ROW_GAP;
    let list_y = toolbar_y + TOOLBAR_H + MARGIN;
    let button_y = height - STATUS_H - BUTTON_H - MARGIN;
    Rows { header_y, toolbar_y, list_y, list_h: (button_y - list_y - MARGIN).max(80), button_y }
}

impl JafizApp {
    /// Header row across the top, the run toolbar under it, scenarios on the
    /// left, steps on the right with the verdict buttons under them, status bar
    /// at the bottom.
    pub(crate) fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < MIN_W || height < MIN_H {
            return;
        }
        let rows = rows(height);

        self.suite_label.set_position(MARGIN, rows.header_y + 4);
        self.suite_label.set_size(44, 20);
        self.suite_combo.set_position(MARGIN + 48, rows.header_y);
        self.suite_combo.set_size(280, 24);
        self.location_label.set_position(MARGIN + 340, rows.header_y + 4);
        self.location_label.set_size((width - MARGIN - 348).max(80) as u32, 20);

        // The run toolbar: a run's whole life cycle in one row.
        let mut x = MARGIN;
        for (control, control_w) in
            [(&self.btn_run_new, RUN_NEW_W), (&self.btn_run_finish, RUN_FINISH_W)]
        {
            control.set_position(x, rows.toolbar_y);
            control.set_size(control_w as u32, TOOLBAR_H as u32);
            x += control_w + BUTTON_GAP;
        }
        // The two verbs are one group and the environment another; the extra
        // gap is what says so.
        x += BUTTON_GAP;
        self.env_label.set_position(x, rows.toolbar_y + 4);
        self.env_label.set_size(ENV_LABEL_W as u32, 20);
        x += ENV_LABEL_W;
        // Histórico is anchored right and the environment picker takes what is
        // between: it is the one control on this row whose content has no
        // length limit, so it is the one that should get the spare width.
        let history_x = width - MARGIN - HISTORY_W;
        self.env_combo.set_position(x, rows.toolbar_y);
        self.env_combo.set_size((history_x - BUTTON_GAP - x).max(80) as u32, 24);
        self.btn_history.set_position(history_x, rows.toolbar_y);
        self.btn_history.set_size(HISTORY_W as u32, TOOLBAR_H as u32);

        let scenarios_w = self.scenarios_width(width);
        self.scenarios.set_position(MARGIN, rows.list_y);
        self.scenarios.set_size(scenarios_w as u32, rows.list_h as u32);
        let steps_x = MARGIN + scenarios_w + SPLITTER_W;
        self.steps.set_position(steps_x, rows.list_y);
        self.steps.set_size((width - steps_x - MARGIN).max(160) as u32, rows.list_h as u32);

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
            button.set_position(x, rows.button_y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
    }

    /// How wide the scenario list is in a window this wide: where the tester
    /// left the divider, clamped so neither list can be squeezed out of
    /// existence. The stored width is left alone — narrowing the window and
    /// widening it again puts the divider back where it was put.
    pub(crate) fn scenarios_width(&self, width: i32) -> i32 {
        self.scenarios_w.get().clamp(SCENARIOS_W_MIN, splitter_max(width))
    }

    /// The divider's rectangle in client coordinates: the gap between the two
    /// lists, which is what the drag grabs and what the sizing cursor covers.
    /// `None` when the window is too small to have been laid out at all.
    fn splitter_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < MIN_W || height < MIN_H {
            return None;
        }
        let rows = rows(height);
        let left = MARGIN + self.scenarios_width(width);
        Some((left, left + SPLITTER_W, rows.list_y, rows.list_y + rows.list_h))
    }

    /// Whether a point in client coordinates is on the divider.
    pub(crate) fn on_splitter(&self, x: i32, y: i32) -> bool {
        self.splitter_rect().is_some_and(|(left, right, top, bottom)| {
            x >= left && x < right && y >= top && y < bottom
        })
    }

    /// Moves the divider so its left edge follows the cursor and re-lays the
    /// window out around it. Clamped to the same range `scenarios_width` reads,
    /// so what is stored is always a width the window can honour.
    pub(crate) fn drag_splitter_to(&self, x: i32) {
        let width = self.window.size().0 as i32;
        let wanted = (x - MARGIN).clamp(SCENARIOS_W_MIN, splitter_max(width));
        if wanted == self.scenarios_w.get() {
            return; // same pixel: re-laying out would only cost a repaint
        }
        self.scenarios_w.set(wanted);
        self.layout();
    }

    /// Re-captions every visible string after the language changed, the way
    /// MCP Console does it: the window keeps its state and its selection, and
    /// nothing has to be restarted. Both lists are refilled at the end because
    /// their cells carry translated text too (the location line, the status
    /// bar, and the progress wording all come out of the same table).
    pub(crate) fn relabel_all(&self) {
        let (tr, lang) = {
            let state = self.state.borrow();
            (t(state.lang), state.lang)
        };
        self.window.set_text(tr.app_title);
        self.suite_label.set_text(tr.lbl_suite);
        self.btn_run_new.set_text(tr.btn_run_new);
        self.btn_run_finish.set_text(tr.btn_run_finish);
        self.env_label.set_text(tr.lbl_env);
        self.btn_history.set_text(tr.btn_history);
        self.btn_pass.set_text(tr.btn_pass);
        self.btn_fail.set_text(tr.btn_fail);
        self.btn_blocked.set_text(tr.btn_blocked);
        self.btn_skip.set_text(tr.btn_skip);
        self.btn_note.set_text(tr.btn_note);
        self.btn_evidence.set_text(tr.btn_evidence);
        for (index, _, title) in scenario_columns(lang) {
            relabel_column(&self.scenarios, index, title);
        }
        for (index, _, title) in step_columns(lang) {
            relabel_column(&self.steps, index, title);
        }
        self.relabel_menus();
        self.set_location_label();
        // Refills the environment picker too, whose last row is translated.
        self.refresh_all();
    }

    /// Menu captions, which nwg cannot set after creation — the two Win32
    /// shims in `foundry_common::ui` do it in place.
    fn relabel_menus(&self) {
        let tr = self.tr();
        set_submenu_text(&self.menu_file, tr.menu_file);
        set_submenu_text(&self.menu_report, tr.menu_report);
        set_submenu_text(&self.menu_tools, tr.menu_tools);
        set_submenu_text(&self.menu_help, tr.menu_help);
        set_menu_item_text(&self.menu_open, tr.menu_open);
        set_menu_item_text(&self.menu_reload, tr.menu_reload);
        set_menu_item_text(&self.menu_exit, tr.menu_exit);
        set_menu_item_text(&self.menu_report_copy, tr.menu_report_copy);
        set_menu_item_text(&self.menu_report_save, tr.menu_report_save);
        set_menu_item_text(&self.menu_prefs, tr.menu_prefs);
        set_menu_item_text(&self.menu_help_format, tr.menu_help_format);
        set_menu_item_text(&self.menu_about, tr.menu_about);
        // The bar keeps painting the old captions — and their old widths —
        // until it is told to measure itself again.
        if let Some(hwnd) = self.window.handle.hwnd() {
            // SAFETY: a live top-level HWND; DrawMenuBar takes nothing else.
            unsafe { winapi::um::winuser::DrawMenuBar(hwnd) };
        }
    }
}

/// The furthest right the divider may sit in a window this wide — never less
/// than its left stop, so the clamp range stays valid in a window too narrow to
/// give both lists their minimum.
fn splitter_max(width: i32) -> i32 {
    (width - MARGIN * 2 - SPLITTER_W - STEPS_W_MIN).max(SCENARIOS_W_MIN)
}

/// Rewrites one column header, leaving its width alone — the user may have
/// dragged it, and a language change is no reason to undo that.
fn relabel_column(list: &nwg::ListView, index: usize, title: &str) {
    list.update_column(
        index,
        nwg::InsertListViewColumn {
            index: Some(index as i32),
            fmt: None,
            width: None,
            text: Some(title.into()),
        },
    );
}
