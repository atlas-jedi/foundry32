//! "Detalhes" — everything WITN knows about one node process, laid out like
//! Process Explorer's Properties: tabs for General, Origin (who started it,
//! the process tree), Performance, Network and Environment.
//!
//! Not modal: like Properties windows, several can be open beside the main
//! list, each on its own thread with its own event loop (the shape of the
//! jafiz dialogs, minus the answer — nothing is handed back). A second request
//! for a process whose window is open brings that window forward instead.
//!
//! The content comes from `witn::report`, the same rendering `witn info`
//! prints, so the window, "Copiar tudo" and the CLI always agree.

use crate::i18n::{t, Lang, T};
use crate::{key_of, ProcKey};
use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{apply_window_icon, insert_report_list_view_column, popup_menu, MenuEntry};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use witn::details;
use witn::model::NodeProc;
use witn::report::{self, Section, SectionKind};
use witn::Scanner;

const MARGIN: i32 = 8;
const BUTTON_W: i32 = 110;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const BOTTOM_H: i32 = BUTTON_H + 2 * MARGIN;
const CHECK_W: i32 = 210;
const WINDOW_W: i32 = 820;
const WINDOW_H: i32 = 580;
const VK_C: u32 = 0x43;

const CMD_COPY_VALUE: usize = 1;
const CMD_COPY_ROW: usize = 2;

/// Open Details windows by process: the window's HWND (as an integer, so the
/// map can cross threads), or 0 while its thread is still building it.
pub type Registry = Arc<Mutex<HashMap<ProcKey, isize>>>;

pub struct DetailsParams {
    pub lang: Lang,
    /// The process as the last scan saw it.
    pub proc: NodeProc,
    pub registry: Registry,
}

/// Opens the Details window for `params.proc`, or brings its open window to
/// the front. Returns immediately; the window lives on its own thread.
pub fn open(params: DetailsParams) {
    let key = key_of(&params.proc);
    {
        let mut open = params.registry.lock().unwrap();
        if let Some(&hwnd) = open.get(&key) {
            if hwnd != 0 {
                bring_to_front(hwnd);
            }
            return; // either up already, or about to be
        }
        open.insert(key, 0);
    }
    std::thread::spawn(move || run(params));
}

fn bring_to_front(hwnd: isize) {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{SetForegroundWindow, ShowWindow, SW_RESTORE};
    // SAFETY: an HWND this process created; both calls tolerate a stale one.
    unsafe {
        ShowWindow(hwnd as HWND, SW_RESTORE);
        SetForegroundWindow(hwnd as HWND);
    }
}

/// One list in the window and the cells it shows, kept beside the control so
/// copying reads our data rather than text back out of the list view.
struct Pane {
    list: nwg::ListView,
    shown: RefCell<Vec<Vec<String>>>,
}

impl Pane {
    fn new(parent: &nwg::Tab, columns: &[(i32, String)]) -> Pane {
        let mut list = nwg::ListView::default();
        nwg::ListView::builder()
            .parent(parent)
            .list_style(nwg::ListViewStyle::Detailed)
            .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
            .build(&mut list)
            .expect("details list");
        // nwg forces LVS_NOCOLUMNHEADER at creation for backward compatibility.
        list.set_headers_enabled(true);
        apply_explorer_theme(&list.handle);
        for (index, (width, title)) in columns.iter().enumerate() {
            insert_report_list_view_column(&list, index as i32, *width, title);
        }
        Pane { list, shown: RefCell::new(Vec::new()) }
    }

    fn fill(&self, rows: Vec<Vec<String>>) {
        self.list.clear();
        for row in &rows {
            self.list.insert_items_row(None, row);
        }
        *self.shown.borrow_mut() = rows;
    }

    fn selected_row(&self) -> Option<Vec<String>> {
        let index = *self.list.selected_items().first()?;
        self.shown.borrow().get(index).cloned()
    }
}

struct Panes {
    general: Pane,
    origin: Pane,
    lineage: Pane,
    performance: Pane,
    network: Pane,
    environment: Pane,
}

impl Panes {
    fn all(&self) -> [&Pane; 6] {
        [&self.general, &self.origin, &self.lineage, &self.performance, &self.network, &self.environment]
    }

    fn fill(&self, sections: &[Section], show_secrets: bool) {
        for section in sections {
            let rows: Vec<Vec<String>> =
                section.rows.iter().map(|r| vec![r.label.clone(), r.value.clone()]).collect();
            let table: Vec<Vec<String>> = section
                .table
                .iter()
                .flat_map(|table| table.rows.iter().map(|r| report::display_cells(r, show_secrets)))
                .collect();
            match section.kind {
                SectionKind::General => self.general.fill(rows),
                SectionKind::Origin => {
                    self.origin.fill(rows);
                    self.lineage.fill(table);
                }
                SectionKind::Performance => self.performance.fill(rows),
                SectionKind::Network => self.network.fill(table),
                SectionKind::Environment => self.environment.fill(table),
            }
        }
    }
}

/// What the window shows, rebuilt on Atualizar.
struct Model {
    proc: NodeProc,
    sections: Vec<Section>,
    alive: bool,
}

fn inspect(proc: NodeProc, lang: Lang) -> Model {
    let d = details::inspect(&proc);
    let sections = report::build(&proc, &d, lang);
    Model { alive: d.alive, proc, sections }
}

fn header(tr: &T, p: &NodeProc) -> String {
    tr.det_title.replace("%A", &p.app_name).replace("%P", &p.pid.to_string())
}

/// The window's controls and state, shared with its event handler.
struct Ui {
    lang: Lang,
    key: ProcKey,
    registry: Registry,
    window: nwg::Window,
    tabs: nwg::TabsContainer,
    pages: [nwg::Tab; 5],
    panes: Panes,
    show_secrets: nwg::CheckBox,
    status: nwg::Label,
    btn_refresh: nwg::Button,
    btn_copy_all: nwg::Button,
    btn_close: nwg::Button,
    model: RefCell<Model>,
}

fn run(params: DetailsParams) {
    let DetailsParams { lang, proc, registry } = params;
    let key = key_of(&proc);
    let ui = Rc::new(build(lang, key, registry, inspect(proc, lang)));
    ui.panes.fill(&ui.model.borrow().sections, false);
    ui.show_status_for_model();
    ui.layout();
    if let Some(hwnd) = ui.window.handle.hwnd() {
        ui.registry.lock().unwrap().insert(key, hwnd as isize);
    }
    ui.window.set_visible(true);
    ui.tabs.set_focus();

    let window_handle = ui.window.handle;
    let handler_ui = Rc::clone(&ui);
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, evt_data, handle| {
        handler_ui.on_event(evt, evt_data, handle);
    });
    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

fn build(lang: Lang, key: ProcKey, registry: Registry, model: Model) -> Ui {
    let tr = t(lang);
    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((240, 150))
        // Born hidden, shown once the controls exist.
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::RESIZABLE)
        .title(&header(tr, &model.proc))
        .build(&mut window)
        .expect("details window");
    apply_window_icon(&window.handle);

    let mut tabs = nwg::TabsContainer::default();
    nwg::TabsContainer::builder().parent(&window).build(&mut tabs).expect("details tabs");
    let section = |kind: SectionKind| model.sections.iter().find(|s| s.kind == kind);
    let page = |kind: SectionKind| {
        let mut page = nwg::Tab::default();
        let title = section(kind).map_or("", |s| s.title.as_str());
        nwg::Tab::builder().text(title).parent(&tabs).build(&mut page).expect("details tab");
        page
    };
    let pages = [
        page(SectionKind::General),
        page(SectionKind::Origin),
        page(SectionKind::Performance),
        page(SectionKind::Network),
        page(SectionKind::Environment),
    ];

    // Column titles come from the report; widths are this window's business.
    let columns = |kind: SectionKind, widths: &[i32]| -> Vec<(i32, String)> {
        let titles = section(kind).and_then(|s| s.table.as_ref()).map(|t| t.columns.clone()).unwrap_or_default();
        titles.into_iter().zip(widths.iter().copied()).map(|(title, width)| (width, title)).collect()
    };
    let property = vec![(230, tr.det_col_property.to_string()), (540, tr.det_col_value.to_string())];
    let panes = Panes {
        general: Pane::new(&pages[0], &property),
        origin: Pane::new(&pages[1], &property),
        lineage: Pane::new(&pages[1], &columns(SectionKind::Origin, &[110, 60, 150, 460])),
        performance: Pane::new(&pages[2], &property),
        network: Pane::new(&pages[3], &columns(SectionKind::Network, &[70, 240, 240, 130])),
        environment: Pane::new(&pages[4], &columns(SectionKind::Environment, &[240, 520])),
    };

    let mut show_secrets = nwg::CheckBox::default();
    nwg::CheckBox::builder()
        .parent(&window)
        .text(tr.det_show_secrets)
        .build(&mut show_secrets)
        .expect("show secrets");
    let mut status = nwg::Label::default();
    nwg::Label::builder().parent(&window).text("").build(&mut status).expect("details status");
    let button = |text: &str| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(&window).text(text).build(&mut control).expect("details button");
        apply_classic_button_theme(&control);
        control
    };
    let btn_refresh = button(tr.det_refresh);
    let btn_copy_all = button(tr.det_copy_all);
    let btn_close = button(tr.det_close);

    Ui {
        lang,
        key,
        registry,
        window,
        tabs,
        pages,
        panes,
        show_secrets,
        status,
        btn_refresh,
        btn_copy_all,
        btn_close,
        model: RefCell::new(model),
    }
}

impl Ui {
    fn tr(&self) -> &'static T {
        t(self.lang)
    }

    fn secrets_on(&self) -> bool {
        self.show_secrets.check_state() == nwg::CheckBoxState::Checked
    }

    fn show_status_for_model(&self) {
        let alive = self.model.borrow().alive;
        self.status.set_text(if alive { "" } else { self.tr().det_exited });
    }

    fn on_event(&self, evt: nwg::Event, evt_data: nwg::EventData, handle: nwg::ControlHandle) {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == self.window.handle => self.close(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == self.window.handle => self.layout(),
            E::OnButtonClick if handle == self.btn_close.handle => self.close(),
            E::OnButtonClick if handle == self.btn_copy_all.handle => {
                let text = {
                    let m = self.model.borrow();
                    report::to_text(&header(self.tr(), &m.proc), &m.sections, self.secrets_on())
                };
                self.copy(&text);
            }
            E::OnButtonClick if handle == self.show_secrets.handle => {
                self.panes.fill(&self.model.borrow().sections, self.secrets_on());
            }
            E::OnButtonClick if handle == self.btn_refresh.handle => self.refresh(),
            E::OnListViewRightClick => {
                let Some(pane) = self.pane_of(&handle) else { return };
                let nwg::EventData::OnListViewItemIndex { row_index, .. } = evt_data else { return };
                if row_index >= pane.shown.borrow().len() {
                    return;
                }
                pane.list.select_item(row_index, true);
                let tr = self.tr();
                let entries = [
                    MenuEntry::Item { id: CMD_COPY_VALUE, text: tr.det_copy_value, enabled: true, default: true },
                    MenuEntry::Item { id: CMD_COPY_ROW, text: tr.det_copy_row, enabled: true, default: false },
                ];
                match popup_menu(&self.window.handle, &entries) {
                    Some(CMD_COPY_VALUE) => self.copy_from(pane, false),
                    Some(CMD_COPY_ROW) => self.copy_from(pane, true),
                    _ => {}
                }
            }
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_C) = evt_data {
                    if ctrl_down() {
                        if let Some(pane) = self.pane_of(&handle) {
                            self.copy_from(pane, false);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn close(&self) {
        self.registry.lock().unwrap().remove(&self.key);
        nwg::stop_thread_dispatch();
    }

    /// Atualizar: a fresh scan of this process (matched by PID + start time)
    /// and a fresh read of its details. A process that exited keeps its last
    /// reading on screen, flagged as such.
    fn refresh(&self) {
        let current = self.model.borrow().proc.clone();
        let fresh = rescan(&current).unwrap_or(current);
        *self.model.borrow_mut() = inspect(fresh, self.lang);
        self.panes.fill(&self.model.borrow().sections, self.secrets_on());
        self.show_status_for_model();
        self.layout();
    }

    fn copy(&self, text: &str) {
        nwg::Clipboard::set_data_text(&self.window, text);
        self.status.set_text(self.tr().det_copied);
    }

    /// Copies the selected row's value (its last cell — the value of a
    /// property, the command line of a process) or the whole row.
    fn copy_from(&self, pane: &Pane, whole_row: bool) {
        if let Some(row) = pane.selected_row() {
            let text = if whole_row { row.join("\t") } else { row.last().cloned().unwrap_or_default() };
            self.copy(&text);
        }
    }

    fn pane_of(&self, handle: &nwg::ControlHandle) -> Option<&Pane> {
        self.panes.all().into_iter().find(|p| p.list.handle == *handle)
    }

    fn layout(&self) {
        let (w, h) = self.window.size();
        let (w, h) = (w as i32, h as i32);
        if w < 300 || h < 200 {
            return;
        }
        self.tabs.set_position(MARGIN, MARGIN);
        // Resizing the container resizes its pages (nwg does that on WM_SIZE);
        // the lists inside each page are ours to fit.
        self.tabs.set_size((w - 2 * MARGIN) as u32, (h - BOTTOM_H - MARGIN) as u32);
        let (pw, ph) = logical_client_size(&self.pages[0].handle);
        let ph = ph as i32;
        let fit = |pane: &Pane, y: i32, height: i32| {
            pane.list.set_position(0, y);
            pane.list.set_size(pw, height.max(40) as u32);
        };
        fit(&self.panes.general, 0, ph);
        fit(&self.panes.performance, 0, ph);
        fit(&self.panes.network, 0, ph);
        fit(&self.panes.environment, 0, ph);
        // Origin: the summary on top, sized to its rows; the tree below.
        let summary_rows = self.panes.origin.shown.borrow().len() as i32;
        let summary_h = (summary_rows * 19 + 30).min(ph / 2);
        fit(&self.panes.origin, 0, summary_h);
        fit(&self.panes.lineage, summary_h + 6, ph - summary_h - 6);

        let y = h - BOTTOM_H + MARGIN;
        self.show_secrets.set_position(MARGIN, y + 3);
        self.show_secrets.set_size(CHECK_W as u32, 22);
        let buttons_x = w - MARGIN - 3 * BUTTON_W - 2 * BUTTON_GAP;
        self.status.set_position(MARGIN + CHECK_W + BUTTON_GAP, y + 5);
        self.status.set_size((buttons_x - MARGIN - CHECK_W - 2 * BUTTON_GAP).max(10) as u32, 20);
        let mut x = buttons_x;
        for button in [&self.btn_refresh, &self.btn_copy_all, &self.btn_close] {
            button.set_position(x, y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
    }
}

/// The process as a fresh scan sees it, matched by identity (PID + start
/// time); `None` once it has exited.
fn rescan(current: &NodeProc) -> Option<NodeProc> {
    let mut scanner = Scanner::new();
    let _ = scanner.sample();
    std::thread::sleep(Duration::from_millis(350));
    let key = key_of(current);
    witn::tree::build(scanner.sample()).into_iter().find(|p| key_of(p) == key)
}

/// A control's client area in nwg's logical units (what `set_size` takes).
/// `nwg::Tab` has no `size()`, and its pages are sized by the container.
fn logical_client_size(handle: &nwg::ControlHandle) -> (u32, u32) {
    use winapi::shared::windef::RECT;
    use winapi::um::winuser::GetClientRect;
    let Some(hwnd) = handle.hwnd() else { return (0, 0) };
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a live HWND and a RECT to fill.
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    let scale = nwg::scale_factor().max(1.0);
    (((rect.right - rect.left) as f64 / scale) as u32, ((rect.bottom - rect.top) as f64 / scale) as u32)
}

fn ctrl_down() -> bool {
    use winapi::um::winuser::{GetKeyState, VK_CONTROL};
    // SAFETY: GetKeyState takes a virtual-key code and reads thread input state.
    unsafe { GetKeyState(VK_CONTROL) < 0 }
}
