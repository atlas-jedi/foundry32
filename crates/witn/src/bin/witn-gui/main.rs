//! `witn-gui.exe` — the GUI front-end (GUI subsystem, launched by the Foundry32
//! hub). A Windows Classic-styled window over the same engine the CLI uses:
//! a tree-grouped list of node.exe processes that auto-refreshes, with Encerrar
//! (kill the whole tree) and Abrir local (open the app's folder) actions.
//!
//! The scan reads other processes' PEBs (per-process, non-trivial), so it runs
//! on a worker thread and hands results back through a `Notice`; the UI thread
//! never blocks on a scan.

#![windows_subsystem = "windows"]

mod i18n;

use i18n::{detect_system_lang, t, Lang, T};

use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{
    apply_window_icon, insert_report_list_view_column, set_list_view_sort_indicator,
};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use witn::model::{format_uptime, NodeProc};
use witn::{appname, proctree, tree, Scanner};

// Column indices, in the order they are inserted into the list view.
const COL_APP: usize = 0;
const COL_PID: usize = 1;
const COL_PPID: usize = 2;
const COL_PORTS: usize = 3;
const COL_CPU: usize = 4;
const COL_RAM: usize = 5;
const COL_UPTIME: usize = 6;
const COL_PATH: usize = 7;

const MARGIN: i32 = 8;
const BUTTON_W: i32 = 120;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const STATUS_H: i32 = 24;
const REFRESH_SECS: u64 = 3;
const VK_F5: u32 = 0x74;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    /// Index into `procs` (not into the displayed rows) of the selection.
    selected: Option<usize>,
    paused: bool,
}

struct WitnApp {
    window: nwg::Window,
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
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
        cmd_tx,
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
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_F5) = evt_data {
                    let _ = app.cmd_tx.send(Cmd::Scan);
                }
            }
            E::OnListViewColumnClick if handle == app.listview.handle => {
                if let nwg::EventData::OnListViewItemIndex { column_index, .. } = evt_data {
                    app.cycle_sort(column_index);
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
        let list_h = (button_y - 2 * MARGIN).max(60) as u32;
        let list_w = (width - 2 * MARGIN).max(120) as u32;

        self.listview.set_position(MARGIN, MARGIN);
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

    /// Rebuilds the display order from the current sort. `procs` itself is
    /// never reordered, so a row's index into it (what `selected` holds) stays
    /// valid across sorts.
    fn reorder(&self) {
        let mut state = self.state.borrow_mut();
        let mut order: Vec<usize> = (0..state.procs.len()).collect();
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

    fn populate(&self) {
        self.listview.clear();
        {
            let state = self.state.borrow();
            let flattened = state.sort.is_some();
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
                let row = [
                    name,
                    p.pid.to_string(),
                    p.ppid.to_string(),
                    p.ports_label(),
                    format!("{:.0}%", p.cpu_percent),
                    format!("{} MB", p.mem_mib()),
                    format_uptime(p.uptime_secs),
                    app_path(p),
                ];
                self.listview.insert_items_row(None, &row);
            }
            set_list_view_sort_indicator(&self.listview, state.sort.map(|(c, d)| (c as i32, d)));
        }
        self.state.borrow_mut().selected = None;
        self.btn_kill.set_enabled(false);
        self.btn_open.set_enabled(false);
        self.update_status();
    }

    fn select_row(&self, row: usize) {
        // Refilling the list makes Windows emit selection changes of its own.
        // Those land while `populate` still holds the state borrowed — and it
        // clears the selection right after — so dropping them is both safe and
        // the behavior we want.
        let Ok(mut state) = self.state.try_borrow_mut() else { return };
        let selected = state.order.get(row).copied();
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
            let Some(i) = state.selected else { return };
            let Some(p) = state.procs.get(i) else { return };
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

    fn open_selected(&self) {
        let dir = {
            let state = self.state.borrow();
            let Some(i) = state.selected else { return };
            let Some(p) = state.procs.get(i) else { return };
            p.cwd.clone().or_else(|| {
                appname::app_location(p.cmdline.as_deref(), p.cwd.as_deref()).map(|loc| {
                    if loc.is_file() {
                        loc.parent().map(Path::to_path_buf).unwrap_or(loc)
                    } else {
                        loc
                    }
                })
            })
        };
        if let Some(dir) = dir {
            let _ = std::process::Command::new("explorer.exe")
                .arg(&dir)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
    }

    fn update_status(&self) {
        let text = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            let left = if state.procs.is_empty() {
                tr.empty.to_string()
            } else {
                tr.status_count.replace("%N", &state.procs.len().to_string())
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
        _ => "",
    }
}

/// The app's on-disk location (script/project dir), never the shared node.exe.
fn app_path(p: &NodeProc) -> String {
    appname::app_location(p.cmdline.as_deref(), p.cwd.as_deref())
        .or_else(|| p.exe_path.clone())
        .map(|x| x.display().to_string())
        .unwrap_or_else(|| "—".to_string())
}
