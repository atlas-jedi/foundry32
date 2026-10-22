//! `witn-gui.exe` — the GUI front-end (GUI subsystem, launched by the Foundry32
//! hub). A Windows Classic-styled window over the same engine the CLI uses:
//! a tree-grouped list of node.exe processes that auto-refreshes, a search box
//! that looks a term up everywhere a process can be tied to it, and the
//! actions Encerrar (kill the whole tree), Abrir local (open where the app's
//! code lives) and — on right-click or double-click — Detalhes.
//!
//! The scan reads other processes' PEBs (per-process, non-trivial), so it runs
//! on a worker thread and hands results back through a `Notice`; the UI thread
//! never blocks on a scan.

#![windows_subsystem = "windows"]

mod details_window;
mod i18n;

use i18n::{detect_system_lang, t, Lang, T};

use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{
    apply_window_icon, ensure_visible, insert_report_list_view_column, popup_menu, select_only,
    set_list_view_sort_indicator, MenuEntry,
};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use witn::model::{format_uptime, NodeProc};
use witn::search::{self, Hit, Query};
use witn::{proctree, report, tree, Scanner};

// Column indices, in the order they are inserted into the list view.
const COL_APP: usize = 0;
const COL_PID: usize = 1;
const COL_PPID: usize = 2;
const COL_PORTS: usize = 3;
const COL_CPU: usize = 4;
const COL_RAM: usize = 5;
const COL_UPTIME: usize = 6;
const COL_PATH: usize = 7;
/// Only present while a search is active.
const COL_FOUND: usize = 8;

const MARGIN: i32 = 8;
const SEARCH_H: i32 = 24;
const BUTTON_W: i32 = 120;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const STATUS_H: i32 = 24;
const REFRESH_SECS: u64 = 3;
const VK_F5: u32 = 0x74;
const VK_DELETE: u32 = 0x2E;
const VK_ESCAPE: u32 = 0x1B;
const VK_F: u32 = 0x46;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Context-menu command ids (non-zero: 0 means the menu was dismissed).
const CMD_DETAILS: usize = 1;
const CMD_OPEN_APP: usize = 2;
const CMD_OPEN_CWD: usize = 3;
const CMD_COPY_PID: usize = 4;
const CMD_COPY_CMDLINE: usize = 5;
const CMD_COPY_APP: usize = 6;
const CMD_COPY_CWD: usize = 7;
const CMD_KILL: usize = 8;

/// Commands from the UI thread to the scanner thread.
enum Cmd {
    Scan,
    Pause,
    Resume,
    Quit,
}

/// Mailbox the scanner thread fills; drained on the UI thread when the Notice
/// fires.
#[derive(Default)]
struct Shared {
    result: Option<Vec<NodeProc>>,
}

struct UiState {
    lang: Lang,
    /// The scan result in tree order (each parent followed by its children) —
    /// the source of truth. `order` alone decides how it is displayed.
    procs: Vec<NodeProc>,
    /// Indices into `procs`, in display order.
    order: Vec<usize>,
    /// Column the list is sorted by and whether it is descending; `None` is
    /// the default process tree.
    sort: Option<(usize, bool)>,
    /// The selected process, held by identity rather than by position. Every
    /// scan builds a fresh `procs`, so an index into it survives no refresh:
    /// it would land on whatever process the new scan happens to put there.
    selected: Option<ProcKey>,
    paused: bool,
    /// The search box's term; `None` shows every process.
    query: Option<Query>,
    /// Where the term was found, per process — only the processes that match.
    hits: HashMap<ProcKey, Vec<Hit>>,
    /// Whether the "Found in" column is currently in the list view.
    found_column: bool,
}

/// What identifies a process across refreshes. The PID alone does not —
/// Windows recycles them — which is why `tree` already pairs a PID with the
/// process's start time to reject PID-recycled parents; the selection uses
/// the same identity.
pub(crate) type ProcKey = (u32, u64);

pub(crate) fn key_of(p: &NodeProc) -> ProcKey {
    (p.pid, p.start_filetime)
}

impl UiState {
    /// The displayed row of the selected process, if the last scan still
    /// lists it.
    fn selected_row(&self) -> Option<usize> {
        let key = self.selected?;
        self.order.iter().position(|&i| self.procs.get(i).map(key_of) == Some(key))
    }

    /// The selected process itself, if the last scan still lists it.
    fn selected_proc(&self) -> Option<&NodeProc> {
        let key = self.selected?;
        self.procs.iter().find(|p| key_of(p) == key)
    }
}

struct WitnApp {
    window: nwg::Window,
    search: nwg::TextInput,
    listview: nwg::ListView,
    btn_refresh: nwg::Button,
    btn_pause: nwg::Button,
    btn_kill: nwg::Button,
    btn_open: nwg::Button,
    status_bar: nwg::StatusBar,
    notice: nwg::Notice,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
    cmd_tx: Sender<Cmd>,
    /// The Details windows that are open, so a second request for the same
    /// process brings its window forward instead of opening another.
    details_open: details_window::Registry,
}

fn main() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let app = Rc::new(build_app(detect_system_lang(), cmd_tx));
    spawn_scanner(cmd_rx, Arc::clone(&app.shared), app.notice.sender());
    wire_events(&app);
    app.layout();
    let _ = app.cmd_tx.send(Cmd::Scan); // first fill, right away
    nwg::dispatch_thread_events();
    let _ = app.cmd_tx.send(Cmd::Quit);
}

/// The scanner loop: `recv_timeout` gives both the auto-refresh tick (on
/// timeout) and the UI's explicit commands. Each scan is self-contained
/// (`scan_twice` computes CPU% from its own two samples), so no state persists
/// between refreshes.
fn spawn_scanner(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>, sender: nwg::NoticeSender) {
    std::thread::spawn(move || {
        let interval = Duration::from_secs(REFRESH_SECS);
        let mut paused = false;
        loop {
            let scan_now = match rx.recv_timeout(interval) {
                Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Cmd::Scan) => true,
                Ok(Cmd::Pause) => {
                    paused = true;
                    false
                }
                Ok(Cmd::Resume) => {
                    paused = false;
                    true
                }
                Err(RecvTimeoutError::Timeout) => !paused,
            };
            if scan_now {
                let procs = tree::build(scan_twice());
                shared.lock().unwrap().result = Some(procs);
                sender.notice();
            }
        }
    });
}

/// Two samples ~350 ms apart so the second carries a real CPU% reading.
fn scan_twice() -> Vec<NodeProc> {
    let mut scanner = Scanner::new();
    let _ = scanner.sample();
    std::thread::sleep(Duration::from_millis(350));
    scanner.sample()
}

fn build_app(lang: Lang, cmd_tx: Sender<Cmd>) -> WitnApp {
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((980, 600))
        .position((180, 110))
        .title(tr.app_title)
        .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::RESIZABLE)
        .build(&mut window)
        .expect("window");

    let mut search = nwg::TextInput::default();
    nwg::TextInput::builder()
        .parent(&window)
        .placeholder_text(Some(tr.search_placeholder))
        .build(&mut search)
        .expect("search");

    let mut listview = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .build(&mut listview)
        .expect("listview");
    listview.set_headers_enabled(true);
    let columns = [
        (210, tr.col_app),
        (55, tr.col_pid),
        (55, tr.col_ppid),
        (105, tr.col_ports),
        (48, tr.col_cpu),
        (70, tr.col_ram),
        (78, tr.col_uptime),
        (300, tr.col_path),
    ];
    for (i, (width, title)) in columns.iter().enumerate() {
        insert_report_list_view_column(&listview, i as i32, *width, title);
    }
    apply_explorer_theme(&listview.handle);

    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let btn_refresh = button(tr.btn_refresh, &window);
    let btn_pause = button(tr.btn_pause, &window);
    let btn_kill = button(tr.btn_kill, &window);
    let btn_open = button(tr.btn_open, &window);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    // Title bar, taskbar and Alt+Tab icons (absent on plain GNU dev builds).
    apply_window_icon(&window.handle);

    btn_kill.set_enabled(false);
    btn_open.set_enabled(false);

    WitnApp {
        window,
        search,
        listview,
        btn_refresh,
        btn_pause,
        btn_kill,
        btn_open,
        status_bar,
        notice,
        state: RefCell::new(UiState {
            lang,
            procs: Vec::new(),
            order: Vec::new(),
            sort: None,
            selected: None,
            paused: false,
            query: None,
            hits: HashMap::new(),
            found_column: false,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
        cmd_tx,
        details_open: details_window::Registry::default(),
    }
}

fn wire_events(app: &Rc<WitnApp>) {
    let weak = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = weak.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => {
                app.layout();
            }
            E::OnButtonClick => {
                if handle == app.btn_refresh.handle {
                    let _ = app.cmd_tx.send(Cmd::Scan);
                } else if handle == app.btn_pause.handle {
                    app.toggle_pause();
                } else if handle == app.btn_kill.handle {
                    app.kill_selected();
                } else if handle == app.btn_open.handle {
                    app.open_selected();
                }
            }
            E::OnKeyRelease => match evt_data {
                nwg::EventData::OnKey(VK_F5) => {
                    let _ = app.cmd_tx.send(Cmd::Scan);
                }
                nwg::EventData::OnKey(VK_F) if ctrl_down() => {
                    app.search.set_focus();
                    app.search.set_selection(0..app.search.text().encode_utf16().count() as u32);
                }
                // Delete in the search box edits the text; only the list's
                // Delete ends a process (with the usual confirmation).
                nwg::EventData::OnKey(VK_DELETE) if handle == app.listview.handle => app.kill_selected(),
                nwg::EventData::OnKey(VK_ESCAPE) if handle == app.search.handle => {
                    app.search.set_text("");
                }
                _ => {}
            },
            E::OnTextInput if handle == app.search.handle => app.apply_search(),
            E::OnListViewColumnClick if handle == app.listview.handle => {
                if let nwg::EventData::OnListViewItemIndex { column_index, .. } = evt_data {
                    app.cycle_sort(column_index);
                }
            }
            // Double-click or Enter on a row: Details, as in Process Explorer.
            E::OnListViewItemActivated if handle == app.listview.handle => app.show_details(),
            E::OnListViewRightClick if handle == app.listview.handle => {
                // A right-click on empty space reports row -1 (usize::MAX here).
                if let nwg::EventData::OnListViewItemIndex { row_index, .. } = evt_data {
                    if row_index < app.listview.len() {
                        select_only(&app.listview, row_index);
                        app.select_row(row_index);
                        app.context_menu();
                    }
                }
            }
            E::OnListViewItemChanged | E::OnListViewClick if handle == app.listview.handle => {
                if let nwg::EventData::OnListViewItemIndex { row_index, .. } = evt_data {
                    app.select_row(row_index);
                } else if let nwg::EventData::OnListViewItemChanged { row_index, selected: true, .. } = evt_data {
                    app.select_row(row_index);
                }
            }
            E::OnNotice if handle == app.notice.handle => app.drain(),
            _ => {}
        }
    });
    std::mem::forget(handler); // lives for the whole process (single window)
}

impl WitnApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < 360 || height < 220 {
            return;
        }
        let button_y = height - STATUS_H - BUTTON_H - MARGIN;
        let list_y = MARGIN + SEARCH_H + 6;
        let list_h = (button_y - list_y - MARGIN).max(60) as u32;
        let list_w = (width - 2 * MARGIN).max(120) as u32;

        self.search.set_position(MARGIN, MARGIN);
        self.search.set_size(list_w, SEARCH_H as u32);
        self.listview.set_position(MARGIN, list_y);
        self.listview.set_size(list_w, list_h);

        let mut x = MARGIN;
        for button in [&self.btn_refresh, &self.btn_pause, &self.btn_kill, &self.btn_open] {
            button.set_position(x, button_y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
    }

    fn drain(&self) {
        let result = self.shared.lock().unwrap().result.take();
        if let Some(procs) = result {
            self.state.borrow_mut().procs = procs;
            self.reorder();
            self.populate();
        }
    }

    /// Cycles a column the way a click on its header should: ascending, then
    /// descending, then back to the process tree.
    ///
    /// Sorting flattens the tree — a list ordered by CPU cannot also keep every
    /// child under its parent — so that third click is the only way back to the
    /// hierarchy.
    fn cycle_sort(&self, column: usize) {
        {
            let mut state = self.state.borrow_mut();
            state.sort = match state.sort {
                Some((sorted, false)) if sorted == column => Some((column, true)),
                Some((sorted, true)) if sorted == column => None,
                _ => Some((column, false)),
            };
        }
        self.reorder();
        self.populate();
    }

    /// Re-reads the search box and refilters the list.
    fn apply_search(&self) {
        self.state.borrow_mut().query = Query::parse(&self.search.text());
        self.reorder();
        self.populate();
    }

    /// Rebuilds the display order from the current search and sort. `procs`
    /// itself is never reordered, so a row's index into it (what `selected`
    /// holds) stays valid across sorts.
    fn reorder(&self) {
        let mut state = self.state.borrow_mut();
        let mut hits = HashMap::new();
        if let Some(query) = &state.query {
            for p in &state.procs {
                let found = search::matches(p, query);
                if !found.is_empty() {
                    hits.insert(key_of(p), found);
                }
            }
        }
        let searching = state.query.is_some();
        let mut order: Vec<usize> = (0..state.procs.len())
            .filter(|&i| !searching || hits.contains_key(&key_of(&state.procs[i])))
            .collect();
        state.hits = hits;
        if let Some((column, descending)) = state.sort {
            let procs = &state.procs;
            order.sort_by(|&a, &b| {
                let ordering = compare_by_column(&procs[a], &procs[b], column);
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
        state.order = order;
    }

    /// Refills the list from `order`, then puts the selection back on the row
    /// holding the same process.
    ///
    /// The state stays borrowed for the whole refill on purpose: emptying and
    /// refilling the list makes Windows emit selection changes of its own, and
    /// `select_row` drops the ones that arrive while the borrow is held (see
    /// there). They describe a half-rebuilt list; the selection restored at the
    /// end of this function is the one that counts.
    fn populate(&self) {
        self.sync_found_column();
        let restored = {
            let state = self.state.borrow();
            self.listview.clear();
            // A search, like a sort, shows rows whose neighbors are not their
            // parents — so neither indents.
            let flattened = state.sort.is_some() || state.query.is_some();
            for &index in &state.order {
                let Some(p) = state.procs.get(index) else { continue };
                // Only the tree view indents: under a sort, the row above is no
                // longer the parent, so indentation would claim a relationship
                // the order does not reflect.
                let name = if flattened {
                    p.app_name.clone()
                } else {
                    format!("{}{}", "  ".repeat(p.depth), p.app_name)
                };
                let mut row = vec![
                    name,
                    p.pid.to_string(),
                    p.ppid.to_string(),
                    p.ports_label(),
                    format!("{:.0}%", p.cpu_percent),
                    format!("{} MB", p.mem_mib()),
                    format_uptime(p.uptime_secs),
                    app_path(p),
                ];
                if let Some(found) = state.hits.get(&key_of(p)) {
                    row.push(report::hits_summary(found, state.lang));
                }
                self.listview.insert_items_row(None, &row);
            }
            set_list_view_sort_indicator(&self.listview, state.sort.map(|(c, d)| (c as i32, d)));
            match state.selected_row() {
                Some(row) => {
                    select_only(&self.listview, row);
                    // The refill scrolled back to the top, so the restored row
                    // has to be brought back into view to be a selection the
                    // user can still see.
                    ensure_visible(&self.listview, row);
                    true
                }
                None => false,
            }
        };
        if !restored {
            // Either nothing was selected, or the selected process is gone —
            // it exited, or this scan no longer sees it. Nothing left to act on.
            self.state.borrow_mut().selected = None;
        }
        self.btn_kill.set_enabled(restored);
        self.btn_open.set_enabled(restored);
        self.update_status();
    }

    fn select_row(&self, row: usize) {
        // Refilling the list makes Windows emit selection changes of its own.
        // Those land while `populate` still holds the state borrowed, and they
        // describe rows of a list that is halfway rebuilt — populate restores
        // the real selection itself, so dropping them is the behavior we want.
        let Ok(mut state) = self.state.try_borrow_mut() else { return };
        let selected = state.order.get(row).and_then(|&i| state.procs.get(i)).map(key_of);
        state.selected = selected;
        drop(state);
        self.btn_kill.set_enabled(selected.is_some());
        self.btn_open.set_enabled(selected.is_some());
    }

    fn toggle_pause(&self) {
        let now_paused = {
            let mut state = self.state.borrow_mut();
            state.paused = !state.paused;
            state.paused
        };
        let _ = self.cmd_tx.send(if now_paused { Cmd::Pause } else { Cmd::Resume });
        self.btn_pause.set_text(if now_paused { self.tr().btn_resume } else { self.tr().btn_pause });
        self.update_status();
    }

    fn kill_selected(&self) {
        let (pid, label) = {
            let state = self.state.borrow();
            let Some(p) = state.selected_proc() else { return };
            (p.pid, p.app_name.clone())
        };
        let all = proctree::all_processes();
        let subtree = proctree::subtree(&all, pid);
        if subtree.is_empty() {
            return;
        }
        let tr = self.tr();
        let listing = subtree
            .iter()
            .map(|e| format!("  PID {} — {}", e.pid, e.exe_name))
            .collect::<Vec<_>>()
            .join("\r\n");
        let body = format!(
            "{}\r\n\r\n{}",
            tr.kill_body.replace("%A", &label).replace("%N", &subtree.len().to_string()),
            listing
        );
        let choice = nwg::modal_message(&self.window.handle, &nwg::MessageParams {
            title: tr.kill_title,
            content: &body,
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Warning,
        });
        if choice == nwg::MessageChoice::Yes {
            for entry in subtree.iter().rev() {
                let _ = proctree::terminate(entry.pid);
            }
            let _ = self.cmd_tx.send(Cmd::Scan);
        }
    }

    /// Abrir local: where the app's code lives — the same place the "App
    /// location" column shows — with the file selected when it is one, like
    /// Task Manager's "Open file location". Falls back to the working
    /// directory only when that location no longer exists on disk.
    fn open_selected(&self) {
        let target = {
            let state = self.state.borrow();
            let Some(p) = state.selected_proc() else { return };
            p.app_location().filter(|loc| loc.exists()).or_else(|| p.cwd.clone())
        };
        if let Some(target) = target {
            open_in_explorer(&target);
        }
    }

    fn open_selected_cwd(&self) {
        let cwd = self.state.borrow().selected_proc().and_then(|p| p.cwd.clone());
        if let Some(cwd) = cwd {
            open_in_explorer(&cwd);
        }
    }

    /// Opens the Details window for the selected process (or brings an open
    /// one forward).
    fn show_details(&self) {
        let (proc, lang) = {
            let state = self.state.borrow();
            let Some(p) = state.selected_proc() else { return };
            (p.clone(), state.lang)
        };
        details_window::open(details_window::DetailsParams {
            lang,
            proc,
            registry: self.details_open.clone(),
        });
    }

    /// The right-click menu on a row, in Task Manager's order: the default
    /// action first, then navigation, copying, and the destructive one last.
    fn context_menu(&self) {
        let (has_cwd, has_cmdline) = {
            let state = self.state.borrow();
            let Some(p) = state.selected_proc() else { return };
            (p.cwd.is_some(), p.cmdline.is_some())
        };
        let tr = self.tr();
        let item = |id, text, enabled| MenuEntry::Item { id, text, enabled, default: false };
        let entries = [
            MenuEntry::Item { id: CMD_DETAILS, text: tr.menu_details, enabled: true, default: true },
            MenuEntry::Separator,
            item(CMD_OPEN_APP, tr.menu_open_app, true),
            item(CMD_OPEN_CWD, tr.menu_open_cwd, has_cwd),
            MenuEntry::Separator,
            MenuEntry::Submenu {
                text: tr.menu_copy,
                items: vec![
                    item(CMD_COPY_PID, tr.menu_copy_pid, true),
                    item(CMD_COPY_CMDLINE, tr.menu_copy_cmdline, has_cmdline),
                    item(CMD_COPY_APP, tr.menu_copy_app, true),
                    item(CMD_COPY_CWD, tr.menu_copy_cwd, has_cwd),
                ],
            },
            MenuEntry::Separator,
            item(CMD_KILL, tr.menu_kill, true),
        ];
        match popup_menu(&self.window.handle, &entries) {
            Some(CMD_DETAILS) => self.show_details(),
            Some(CMD_OPEN_APP) => self.open_selected(),
            Some(CMD_OPEN_CWD) => self.open_selected_cwd(),
            Some(CMD_COPY_PID) => self.copy_selected(|p| Some(p.pid.to_string())),
            Some(CMD_COPY_CMDLINE) => self.copy_selected(|p| p.cmdline.clone()),
            Some(CMD_COPY_APP) => self.copy_selected(|p| Some(app_path(p))),
            Some(CMD_COPY_CWD) => self.copy_selected(|p| p.cwd.as_ref().map(|c| c.display().to_string())),
            Some(CMD_KILL) => self.kill_selected(),
            _ => {}
        }
    }

    fn copy_selected(&self, field: impl Fn(&NodeProc) -> Option<String>) {
        let text = self.state.borrow().selected_proc().and_then(field);
        if let Some(text) = text {
            nwg::Clipboard::set_data_text(&self.window, &text);
        }
    }

    /// Adds the "Found in" column when a search starts and removes it when
    /// the search is cleared, so the plain list keeps its usual columns.
    fn sync_found_column(&self) {
        let mut state = self.state.borrow_mut();
        let wanted = state.query.is_some();
        if wanted && !state.found_column {
            insert_report_list_view_column(&self.listview, COL_FOUND as i32, 320, t(state.lang).col_found);
        } else if !wanted && state.found_column {
            self.listview.remove_column(COL_FOUND);
        }
        state.found_column = wanted;
    }

    fn update_status(&self) {
        let text = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            let total = state.procs.len().to_string();
            let left = if state.procs.is_empty() {
                tr.empty.to_string()
            } else if let Some(query) = &state.query {
                let shown = tr.status_filtered.replace("%M", &state.order.len().to_string()).replace("%N", &total);
                format!("{shown}   ·   {}", tr.status_query.replace("%Q", query.text()))
            } else {
                tr.status_count.replace("%N", &total)
            };
            let mode = if state.paused {
                tr.status_paused.to_string()
            } else {
                tr.status_live.replace("%S", &REFRESH_SECS.to_string())
            };
            let mut text = format!("{left}   ·   {mode}");
            if let Some((column, descending)) = state.sort {
                let sorted = tr
                    .status_sorted
                    .replace("%C", column_title(tr, column))
                    .replace("%D", if descending { "↓" } else { "↑" });
                text.push_str(&format!("   ·   {sorted}"));
            }
            text
        };
        self.status_bar.set_text(0, &text);
    }
}

/// Orders two processes by what a given column shows. Every column falls back
/// to the PID so equal values keep a stable, reproducible order between scans.
fn compare_by_column(a: &NodeProc, b: &NodeProc, column: usize) -> Ordering {
    let by_pid = a.pid.cmp(&b.pid);
    match column {
        COL_APP => a.app_name.to_lowercase().cmp(&b.app_name.to_lowercase()).then(by_pid),
        COL_PID => by_pid,
        COL_PPID => a.ppid.cmp(&b.ppid).then(by_pid),
        COL_PORTS => port_key(a).cmp(&port_key(b)).then(by_pid),
        COL_CPU => a.cpu_percent.total_cmp(&b.cpu_percent).then(by_pid),
        COL_RAM => a.mem_bytes.cmp(&b.mem_bytes).then(by_pid),
        COL_UPTIME => a.uptime_secs.cmp(&b.uptime_secs).then(by_pid),
        COL_PATH => app_path(a).to_lowercase().cmp(&app_path(b).to_lowercase()).then(by_pid),
        _ => by_pid,
    }
}

/// Sorts by the lowest listening port, with the processes that listen on none
/// pushed to the end of the ascending order — those are never what someone
/// sorting by port is looking for.
fn port_key(p: &NodeProc) -> (bool, u16) {
    (p.ports.is_empty(), p.ports.first().copied().unwrap_or(0))
}

/// The header caption of a column, for the status bar's sort indicator.
fn column_title(tr: &'static T, column: usize) -> &'static str {
    match column {
        COL_APP => tr.col_app,
        COL_PID => tr.col_pid,
        COL_PPID => tr.col_ppid,
        COL_PORTS => tr.col_ports,
        COL_CPU => tr.col_cpu,
        COL_RAM => tr.col_ram,
        COL_UPTIME => tr.col_uptime,
        COL_PATH => tr.col_path,
        COL_FOUND => tr.col_found,
        _ => "",
    }
}

/// The app's on-disk location (script/project dir), see
/// `NodeProc::app_location`.
fn app_path(p: &NodeProc) -> String {
    p.app_location().map_or_else(|| "—".to_string(), |x| x.display().to_string())
}

/// Shows a path in Explorer: a file with its folder open and the file
/// selected, a folder opened.
fn open_in_explorer(path: &Path) {
    let mut command = std::process::Command::new("explorer.exe");
    if path.is_file() {
        // Explorer parses `/select,"<path>"` itself; Rust's own argument
        // quoting would wrap the whole switch in quotes and break it on any
        // path with a space, hence `raw_arg`.
        command.raw_arg(format!("/select,\"{}\"", path.display()));
    } else {
        command.arg(path);
    }
    let _ = command.creation_flags(CREATE_NO_WINDOW).spawn();
}

/// Whether Ctrl is held right now (for Ctrl+F, read at key release).
fn ctrl_down() -> bool {
    use winapi::um::winuser::{GetKeyState, VK_CONTROL};
    // SAFETY: GetKeyState takes a virtual-key code and reads thread input state.
    unsafe { GetKeyState(VK_CONTROL) < 0 }
}
