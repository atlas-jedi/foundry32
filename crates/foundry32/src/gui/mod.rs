//! Main hub window. Windows-Classic chrome — classic 3D buttons, sunken-edged
//! catalog and details panels under section labels, a classic segmented
//! progress bar — with modern accents (colored status glyphs, Explorer list
//! hover, DPI awareness, threaded install with live progress and cancel).

pub mod preferences_dialog;

use crate::i18n::{t, Lang, T};
use crate::installed::InstalledState;
use crate::model::{self, ToolStatus, ToolView};
use crate::registry::{self, Catalog, Source};
use crate::settings::AppSettings;
use crate::update_check::{self, UpdateInfo};
use crate::{download, engine, paths};
use foundry_common::theme::{apply_classic_button_theme, apply_classic_theme, apply_explorer_theme, create_glyph_icon};
use foundry_common::ui::{insert_report_list_view_column, set_menu_item_text, set_submenu_text};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const REPO_URL: &str = "https://github.com/atlas-jedi/foundry32";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Layout metrics in logical pixels (nwg setters apply the DPI scale).
const MARGIN: i32 = 10;
const HEADER_H: i32 = 18;
const BUTTON_W: i32 = 104;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const PROGRESS_H: i32 = 18;
const DETAILS_W: i32 = 310;
const STATUS_TOOLS_W: i32 = 120;
const STATUS_VERSION_W: i32 = 80;
const VK_F5: u32 = 0x74;

/// Segoe MDL2 Assets glyphs (the system icon font on Windows 10/11).
const GLYPH_CHECKMARK: u16 = 0xE73E;
const GLYPH_WARNING: u16 = 0xE7BA;
const GLYPH_ERROR: u16 = 0xE783;
/// Fluent state colors as (r, g, b).
const COLOR_OK: (u8, u8, u8) = (0x10, 0x7C, 0x10);
const COLOR_WARNING: (u8, u8, u8) = (0x9D, 0x5D, 0x00);
const COLOR_ERROR: (u8, u8, u8) = (0xC4, 0x2B, 0x1C);

#[derive(Clone, Copy)]
enum StatusTone {
    Busy,
    Ok,
    Warning,
    Error,
}

struct StatusIcons {
    ok: winapi::shared::windef::HICON,
    warning: winapi::shared::windef::HICON,
    error: winapi::shared::windef::HICON,
}

pub(crate) struct ProgressMsg {
    pub tool: String,
    pub done: u64,
    pub total: Option<u64>,
}

pub(crate) enum OpResult {
    Done,
    InUse,
    Failed { message: String },
    Cancelled,
}

/// Mailbox written by worker threads, drained on the UI thread when the Notice
/// fires.
#[derive(Default)]
pub(crate) struct Shared {
    pub catalog: Option<(Catalog, Source)>,
    pub progress: Option<ProgressMsg>,
    pub op_result: Option<OpResult>,
    pub update: Option<Result<Option<UpdateInfo>, String>>,
    pub preferences: Option<Option<Lang>>,
}

struct UiState {
    lang: Lang,
    catalog: Catalog,
    source: Source,
    views: Vec<ToolView>,
    selected: Option<usize>,
    busy: bool,
    update_url: Option<String>,
}

pub struct HubApp {
    window: nwg::Window,
    lbl_catalog: nwg::Label,
    lbl_details: nwg::Label,
    listview: nwg::ListView,
    details: nwg::TextBox,
    progress: nwg::ProgressBar,
    btn_install: nwg::Button,
    btn_run: nwg::Button,
    btn_uninstall: nwg::Button,
    btn_update: nwg::Button,
    btn_cancel: nwg::Button,
    menu_file: nwg::Menu,
    mi_refresh: nwg::MenuItem,
    mi_prefs: nwg::MenuItem,
    mi_exit: nwg::MenuItem,
    menu_tools: nwg::Menu,
    mi_install: nwg::MenuItem,
    mi_run: nwg::MenuItem,
    mi_uninstall: nwg::MenuItem,
    mi_update: nwg::MenuItem,
    menu_help: nwg::Menu,
    mi_site: nwg::MenuItem,
    mi_about: nwg::MenuItem,
    _menu_seps: Vec<nwg::MenuSeparator>,
    status_bar: nwg::StatusBar,
    status_icons: StatusIcons,
    notice: nwg::Notice,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
}

pub fn run(settings: AppSettings) {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let app = Rc::new(build_app(settings));
    wire_events(&app);
    app.redraw_menu_bar();
    app.layout();
    app.set_version_text();
    app.set_status(app.tr().status_fetching, StatusTone::Busy);
    app.spawn_fetch_catalog();
    app.spawn_update_check();
    nwg::dispatch_thread_events();
}

fn build_app(settings: AppSettings) -> HubApp {
    let lang = settings.lang;
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((1040, 660))
        .position((200, 110))
        .title(tr.app_title)
        .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::RESIZABLE)
        .build(&mut window)
        .expect("window");

    let mut menu_seps = Vec::new();
    let mut separator = |parent: &nwg::Menu| {
        let mut sep = nwg::MenuSeparator::default();
        nwg::MenuSeparator::builder().parent(parent).build(&mut sep).expect("menu separator");
        menu_seps.push(sep);
    };
    let menu = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Menu::default();
        nwg::Menu::builder().text(text).parent(parent).build(&mut control).expect("menu");
        control
    };
    let item = |text: &str, parent: &nwg::Menu| {
        let mut control = nwg::MenuItem::default();
        nwg::MenuItem::builder().text(text).parent(parent).build(&mut control).expect("menu item");
        control
    };

    let menu_file = menu(tr.menu_file, &window);
    let mi_refresh = item(tr.menu_file_refresh, &menu_file);
    let mi_prefs = item(tr.menu_file_prefs, &menu_file);
    separator(&menu_file);
    let mi_exit = item(tr.menu_file_exit, &menu_file);

    let menu_tools = menu(tr.menu_tools, &window);
    let mi_install = item(tr.menu_tools_install, &menu_tools);
    let mi_run = item(tr.menu_tools_run, &menu_tools);
    let mi_uninstall = item(tr.menu_tools_uninstall, &menu_tools);
    let mi_update = item(tr.menu_tools_update, &menu_tools);

    let menu_help = menu(tr.menu_help, &window);
    let mi_site = item(tr.menu_help_site, &menu_help);
    let mi_about = item(tr.menu_help_about, &menu_help);

    let label = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Label::default();
        nwg::Label::builder().parent(parent).text(text).build(&mut control).expect("label");
        control
    };
    let lbl_catalog = label(tr.group_catalog, &window);
    let lbl_details = label(tr.group_details, &window);

    let mut listview = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .build(&mut listview)
        .expect("listview");
    listview.set_headers_enabled(true);
    let widths = [34, 200, 90, 90, 160, 84];
    let titles = [tr.col_num, tr.col_tool, tr.col_installed, tr.col_available, tr.col_status, tr.col_size];
    for (i, (w, title)) in widths.iter().zip(titles.iter()).enumerate() {
        insert_report_list_view_column(&listview, i as i32, *w, title);
    }
    apply_explorer_theme(&listview.handle);

    let mut details = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        .text(tr.details_placeholder)
        .readonly(true)
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL)
        .build(&mut details)
        .expect("details");

    let mut progress = nwg::ProgressBar::default();
    nwg::ProgressBar::builder()
        .parent(&window)
        .range(0..100)
        .build(&mut progress)
        .expect("progress");
    apply_classic_theme(&progress.handle); // classic segmented block style
    progress.set_visible(false);

    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let btn_install = button(tr.btn_install, &window);
    let btn_run = button(tr.btn_run, &window);
    let btn_uninstall = button(tr.btn_uninstall, &window);
    let btn_update = button(tr.btn_update, &window);
    let btn_cancel = button(tr.btn_cancel, &window);
    btn_cancel.set_visible(false);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    if let Ok(embed) = nwg::EmbedResource::load(None) {
        if let Some(icon) = embed.icon(1, None) {
            window.set_icon(Some(&icon));
        }
    }

    btn_install.set_enabled(false);
    btn_run.set_enabled(false);
    btn_uninstall.set_enabled(false);
    btn_update.set_enabled(false);
    mi_install.set_enabled(false);
    mi_run.set_enabled(false);
    mi_uninstall.set_enabled(false);
    mi_update.set_enabled(false);

    HubApp {
        window,
        lbl_catalog,
        lbl_details,
        listview,
        details,
        progress,
        btn_install,
        btn_run,
        btn_uninstall,
        btn_update,
        btn_cancel,
        menu_file,
        mi_refresh,
        mi_prefs,
        mi_exit,
        menu_tools,
        mi_install,
        mi_run,
        mi_uninstall,
        mi_update,
        menu_help,
        mi_site,
        mi_about,
        _menu_seps: menu_seps,
        status_bar,
        status_icons: create_status_icons(),
        notice,
        state: RefCell::new(UiState {
            lang,
            catalog: Catalog::default(),
            source: Source::Embedded,
            views: Vec::new(),
            selected: None,
            busy: false,
            update_url: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn wire_events(app: &Rc<HubApp>) {
    let evt_app = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = evt_app.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => app.layout(),
            E::OnButtonClick => {
                if handle == app.btn_install.handle {
                    app.do_install_or_update(false);
                } else if handle == app.btn_update.handle {
                    app.do_install_or_update(true);
                } else if handle == app.btn_run.handle {
                    app.do_run();
                } else if handle == app.btn_uninstall.handle {
                    app.do_uninstall();
                } else if handle == app.btn_cancel.handle {
                    app.cancel.store(true, Ordering::Relaxed);
                }
            }
            E::OnMenuItemSelected => {
                if handle == app.mi_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.mi_refresh.handle {
                    app.refresh();
                } else if handle == app.mi_prefs.handle {
                    app.open_preferences();
                } else if handle == app.mi_install.handle {
                    app.do_install_or_update(false);
                } else if handle == app.mi_update.handle {
                    app.do_install_or_update(true);
                } else if handle == app.mi_run.handle {
                    app.do_run();
                } else if handle == app.mi_uninstall.handle {
                    app.do_uninstall();
                } else if handle == app.mi_site.handle {
                    open_in_browser(REPO_URL);
                } else if handle == app.mi_about.handle {
                    app.show_about();
                }
            }
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_F5) = evt_data {
                    if !app.state.borrow().busy {
                        app.refresh();
                    }
                }
            }
            E::OnListViewItemChanged | E::OnListViewClick if handle == app.listview.handle => {
                if let nwg::EventData::OnListViewItemIndex { row_index, .. } = evt_data {
                    app.select_row(row_index);
                }
            }
            E::OnNotice if handle == app.notice.handle => app.drain_shared(),
            _ => {}
        }
    });
    std::mem::forget(handler);
}

impl HubApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < 360 || height < 260 {
            return;
        }
        let status_h = self.status_bar_height();
        let button_y = height - status_h - BUTTON_H - MARGIN;
        let progress_y = button_y - PROGRESS_H - 6;
        let content_top = MARGIN + HEADER_H;
        let content_h = (progress_y - MARGIN - content_top).max(80) as u32;
        let details_w = DETAILS_W.min(width / 3);
        let details_x = width - MARGIN - details_w;
        let list_w = (details_x - 2 * MARGIN).max(140) as u32;

        self.lbl_catalog.set_position(MARGIN, MARGIN);
        self.lbl_catalog.set_size(list_w, HEADER_H as u32);
        self.listview.set_position(MARGIN, content_top);
        self.listview.set_size(list_w, content_h);

        self.lbl_details.set_position(details_x, MARGIN);
        self.lbl_details.set_size(details_w as u32, HEADER_H as u32);
        self.details.set_position(details_x, content_top);
        self.details.set_size(details_w as u32, content_h);

        // Progress bar spans the content width, just above the button row.
        self.progress.set_position(MARGIN, progress_y);
        self.progress.set_size((width - 2 * MARGIN) as u32, PROGRESS_H as u32);

        let mut x = MARGIN;
        for button in [&self.btn_install, &self.btn_run, &self.btn_uninstall, &self.btn_update] {
            button.set_position(x, button_y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
        self.btn_cancel.set_position(width - MARGIN - BUTTON_W, button_y);
        self.btn_cancel.set_size(BUTTON_W as u32, BUTTON_H as u32);

        self.set_status_parts();
    }

    fn status_bar_height(&self) -> i32 {
        use winapi::um::winuser::GetWindowRect;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return 23 };
        let mut rect: winapi::shared::windef::RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(hwnd, &mut rect) };
        ((rect.bottom - rect.top) as f64 / nwg::scale_factor()) as i32
    }

    fn set_status_parts(&self) {
        use winapi::um::commctrl::SB_SETPARTS;
        use winapi::um::winuser::SendMessageW;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return };
        let scale = nwg::scale_factor();
        let px = |v: i32| (v as f64 * scale) as i32;
        let width = px(self.window.size().0 as i32);
        let right = px(STATUS_VERSION_W);
        let edges = [width - px(STATUS_TOOLS_W) - right, width - right, -1i32];
        unsafe {
            SendMessageW(hwnd, SB_SETPARTS, edges.len(), edges.as_ptr() as isize);
        }
    }

    fn refresh(&self) {
        if self.state.borrow().busy {
            return;
        }
        self.set_status(self.tr().status_fetching, StatusTone::Busy);
        self.spawn_fetch_catalog();
    }

    fn spawn_fetch_catalog(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.busy = true;
        }
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let loaded = registry::load();
            shared.lock().unwrap().catalog = Some(loaded);
            sender.notice();
        });
    }

    fn spawn_update_check(&self) {
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let result = update_check::check_for_update();
            shared.lock().unwrap().update = Some(result);
            sender.notice();
        });
    }

    fn selected_view(&self) -> Option<ToolView> {
        let state = self.state.borrow();
        state.selected.and_then(|row| state.views.get(row).cloned())
    }

    fn do_install_or_update(&self, is_update: bool) {
        let Some(view) = self.selected_view() else { return };
        if self.state.borrow().busy {
            return;
        }
        if !view.entry.is_installable() {
            nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, self.tr().err_not_installable);
            return;
        }
        self.begin_op(true);
        let status = if is_update { self.tr().status_updating } else { self.tr().status_installing };
        self.set_status(&status.replace("%S", &view.entry.name), StatusTone::Busy);

        self.cancel.store(false, Ordering::Relaxed);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        let cancel = Arc::clone(&self.cancel);
        let entry = view.entry.clone();
        let tool = view.entry.name.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            let mut first = true;
            let result = engine::install(&entry, &cancel, |p| {
                if first || last.elapsed() >= Duration::from_millis(150) {
                    first = false;
                    last = Instant::now();
                    shared.lock().unwrap().progress =
                        Some(ProgressMsg { tool: tool.clone(), done: p.done, total: p.total });
                    sender.notice();
                }
            });
            shared.lock().unwrap().op_result = Some(op_outcome(result));
            sender.notice();
        });
    }

    fn do_uninstall(&self) {
        let Some(view) = self.selected_view() else { return };
        if self.state.borrow().busy {
            return;
        }
        let tr = self.tr();
        let choice = nwg::modal_message(&self.window.handle, &nwg::MessageParams {
            title: tr.confirm_uninstall_title,
            content: &tr.confirm_uninstall_body.replace("%S", &view.entry.name),
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Warning,
        });
        if choice != nwg::MessageChoice::Yes {
            return;
        }
        self.begin_op(false);
        self.set_status(&tr.status_uninstalling.replace("%S", &view.entry.name), StatusTone::Busy);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        let id = view.entry.id.clone();
        std::thread::spawn(move || {
            shared.lock().unwrap().op_result = Some(op_outcome(engine::uninstall(&id)));
            sender.notice();
        });
    }

    fn do_run(&self) {
        let Some(view) = self.selected_view() else { return };
        if let Err(error) = engine::launch(&view.entry.id) {
            nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &error.to_string());
        }
    }

    /// Enters the busy state: disable actions, show the progress bar (for
    /// downloads) and the cancel button.
    fn begin_op(&self, cancellable: bool) {
        self.state.borrow_mut().busy = true;
        self.set_actions_enabled(false);
        if cancellable {
            self.progress.set_visible(true);
            self.progress.set_pos(0);
            self.btn_cancel.set_visible(true);
            self.btn_cancel.set_enabled(true);
        }
    }

    fn end_op(&self) {
        self.state.borrow_mut().busy = false;
        self.progress.set_visible(false);
        self.btn_cancel.set_visible(false);
        self.reload_installed();
        self.update_buttons();
    }

    fn drain_shared(&self) {
        let (catalog, progress, op_result, update, preferences) = {
            let mut shared = self.shared.lock().unwrap();
            (
                shared.catalog.take(),
                shared.progress.take(),
                shared.op_result.take(),
                shared.update.take(),
                shared.preferences.take(),
            )
        };

        if let Some((catalog, source)) = catalog {
            {
                let mut state = self.state.borrow_mut();
                state.catalog = catalog;
                state.source = source;
                state.busy = false;
            }
            self.reload_installed();
            self.populate_list();
            self.update_buttons();
            match source {
                Source::Embedded => self.set_status(self.tr().status_offline, StatusTone::Warning),
                _ => self.set_status(self.tr().status_ready, StatusTone::Ok),
            }
        }

        if let Some(p) = progress {
            self.apply_progress(&p);
        }

        if let Some(result) = op_result {
            self.end_op();
            match result {
                OpResult::Done => {
                    self.populate_list();
                    self.set_status(self.tr().status_done, StatusTone::Ok);
                }
                OpResult::Cancelled => self.set_status(self.tr().status_ready, StatusTone::Ok),
                OpResult::InUse => {
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, self.tr().err_in_use);
                    self.set_status(self.tr().status_ready, StatusTone::Warning);
                }
                OpResult::Failed { message } => {
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &message);
                    self.set_status(self.tr().status_ready, StatusTone::Error);
                }
            }
        }

        if let Some(Ok(Some(info))) = update {
            self.state.borrow_mut().update_url = Some(info.html_url);
        }

        if let Some(outcome) = preferences {
            self.window.set_enabled(true);
            if let Some(lang) = outcome {
                let changed = {
                    let mut state = self.state.borrow_mut();
                    let changed = state.lang != lang;
                    state.lang = lang;
                    changed
                };
                if changed {
                    let _ = AppSettings { lang }.save();
                    self.relabel_all();
                }
            }
        }
    }

    fn apply_progress(&self, p: &ProgressMsg) {
        match p.total {
            Some(total) if total > 0 => {
                let pct = ((p.done.min(total) as f64 / total as f64) * 100.0) as u32;
                self.progress.set_pos(pct);
                let text = self.tr().status_downloading.replace("%S", &p.tool).replace("%P", &pct.to_string());
                self.set_status(&text, StatusTone::Busy);
            }
            _ => {
                // Unknown size: keep the bar moving, show bytes so far.
                self.progress.set_pos((p.done / 65536 % 100) as u32);
                let text = self.tr().status_installing.replace("%S", &p.tool);
                self.set_status(&text, StatusTone::Busy);
            }
        }
    }

    fn reload_installed(&self) {
        let mut installed = InstalledState::load();
        installed.reconcile();
        let mut state = self.state.borrow_mut();
        state.views = model::merge(&state.catalog, &installed);
    }

    fn populate_list(&self) {
        self.listview.clear();
        let state = self.state.borrow();
        let tr = t(state.lang);
        for (index, view) in state.views.iter().enumerate() {
            let installed = view.installed.as_ref().map(|t| t.version.clone()).unwrap_or_else(|| tr.d_none.into());
            let size = format_size(view.entry.size_bytes, tr);
            let row = [
                (index + 1).to_string(),
                view.entry.name.clone(),
                installed,
                view.entry.version.clone(),
                status_label(view.status, tr).to_string(),
                size,
            ];
            self.listview.insert_items_row(None, &row);
        }
        drop(state);
        self.details.set_text(self.tr().details_placeholder);
        self.set_tools_count_text();
    }

    fn select_row(&self, row: usize) {
        let text = {
            let mut state = self.state.borrow_mut();
            if state.views.get(row).is_none() {
                return;
            }
            state.selected = Some(row);
            let lang = state.lang;
            let view = &state.views[row];
            details_text(view, lang, t(lang))
        };
        self.details.set_text(&text);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let (installable, status, has_sel, busy) = {
            let state = self.state.borrow();
            match state.selected.and_then(|r| state.views.get(r)) {
                Some(view) => (view.entry.is_installable(), Some(view.status), true, state.busy),
                None => (false, None, false, state.busy),
            }
        };
        let installed = matches!(status, Some(ToolStatus::Installed) | Some(ToolStatus::UpdateAvailable));
        let can_install = has_sel && !busy && status == Some(ToolStatus::NotInstalled) && installable;
        let can_update = has_sel && !busy && status == Some(ToolStatus::UpdateAvailable) && installable;
        let can_run = has_sel && !busy && installed;
        let can_uninstall = has_sel && !busy && installed;

        self.btn_install.set_enabled(can_install);
        self.btn_update.set_enabled(can_update);
        self.btn_run.set_enabled(can_run);
        self.btn_uninstall.set_enabled(can_uninstall);
        self.mi_install.set_enabled(can_install);
        self.mi_update.set_enabled(can_update);
        self.mi_run.set_enabled(can_run);
        self.mi_uninstall.set_enabled(can_uninstall);
    }

    fn set_actions_enabled(&self, enabled: bool) {
        for button in [&self.btn_install, &self.btn_run, &self.btn_uninstall, &self.btn_update] {
            button.set_enabled(enabled);
        }
        for item in [&self.mi_install, &self.mi_run, &self.mi_uninstall, &self.mi_update] {
            item.set_enabled(enabled);
        }
    }

    fn open_preferences(&self) {
        let params = preferences_dialog::PreferencesParams {
            lang: self.state.borrow().lang,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        };
        self.window.set_enabled(false);
        preferences_dialog::spawn(params);
    }

    fn show_about(&self) {
        let tr = self.tr();
        let body = tr.about_body.replace("%V", CURRENT_VERSION);
        nwg::modal_info_message(&self.window.handle, tr.about_title, &body);
    }

    fn relabel_all(&self) {
        let tr = self.tr();
        self.window.set_text(tr.app_title);
        self.lbl_catalog.set_text(tr.group_catalog);
        self.lbl_details.set_text(tr.group_details);
        self.btn_install.set_text(tr.btn_install);
        self.btn_run.set_text(tr.btn_run);
        self.btn_uninstall.set_text(tr.btn_uninstall);
        self.btn_update.set_text(tr.btn_update);
        self.btn_cancel.set_text(tr.btn_cancel);
        let titles = [tr.col_num, tr.col_tool, tr.col_installed, tr.col_available, tr.col_status, tr.col_size];
        for (i, title) in titles.iter().enumerate() {
            self.listview.update_column(i, nwg::InsertListViewColumn {
                index: Some(i as i32),
                fmt: None,
                width: None,
                text: Some((*title).into()),
            });
        }
        set_submenu_text(&self.menu_file, tr.menu_file);
        set_submenu_text(&self.menu_tools, tr.menu_tools);
        set_submenu_text(&self.menu_help, tr.menu_help);
        set_menu_item_text(&self.mi_refresh, tr.menu_file_refresh);
        set_menu_item_text(&self.mi_prefs, tr.menu_file_prefs);
        set_menu_item_text(&self.mi_exit, tr.menu_file_exit);
        set_menu_item_text(&self.mi_install, tr.menu_tools_install);
        set_menu_item_text(&self.mi_run, tr.menu_tools_run);
        set_menu_item_text(&self.mi_uninstall, tr.menu_tools_uninstall);
        set_menu_item_text(&self.mi_update, tr.menu_tools_update);
        set_menu_item_text(&self.mi_site, tr.menu_help_site);
        set_menu_item_text(&self.mi_about, tr.menu_help_about);
        self.redraw_menu_bar();
        self.set_version_text();
        self.populate_list();
        self.update_buttons();
    }

    fn redraw_menu_bar(&self) {
        if let Some(hwnd) = self.window.handle.hwnd() {
            unsafe { winapi::um::winuser::DrawMenuBar(hwnd) };
        }
    }

    fn set_status(&self, text: &str, tone: StatusTone) {
        use winapi::um::commctrl::SB_SETICON;
        use winapi::um::winuser::SendMessageW;
        self.status_bar.set_text(0, text);
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return };
        let icon = match tone {
            StatusTone::Busy => std::ptr::null_mut(),
            StatusTone::Ok => self.status_icons.ok,
            StatusTone::Warning => self.status_icons.warning,
            StatusTone::Error => self.status_icons.error,
        };
        unsafe {
            SendMessageW(hwnd, SB_SETICON, 0, icon as isize);
        }
    }

    fn set_tools_count_text(&self) {
        let count = self.state.borrow().views.len().to_string();
        let text = self.tr().status_tools.replace("%N", &count);
        self.status_bar.set_text(1, &text);
    }

    fn set_version_text(&self) {
        self.status_bar.set_text(2, &format!("v{CURRENT_VERSION}"));
    }
}

/// Maps an engine result to the UI outcome, keeping the "in use" case typed so
/// the UI can show a localized message instead of the engine's English string.
fn op_outcome(result: Result<(), engine::EngineError>) -> OpResult {
    match result {
        Ok(()) => OpResult::Done,
        Err(engine::EngineError::InUse) => OpResult::InUse,
        Err(engine::EngineError::Download(download::DlError::Cancelled)) => OpResult::Cancelled,
        Err(error) => OpResult::Failed { message: error.to_string() },
    }
}

fn status_label(status: ToolStatus, tr: &T) -> &'static str {
    match status {
        ToolStatus::NotInstalled => tr.st_not_installed,
        ToolStatus::Installed => tr.st_installed,
        ToolStatus::UpdateAvailable => tr.st_update,
    }
}

fn details_text(view: &ToolView, lang: Lang, tr: &T) -> String {
    let description = match lang {
        Lang::PtBr => &view.entry.description_pt,
        Lang::En => &view.entry.description_en,
    };
    let installed = view
        .installed
        .as_ref()
        .map(|t| t.version.clone())
        .unwrap_or_else(|| tr.d_none.to_string());
    let path = view
        .installed
        .as_ref()
        .map(|t| paths::tool_exe(&view.entry.id, &t.exe).display().to_string())
        .unwrap_or_else(|| tr.d_none.to_string());
    let mut lines = vec![
        view.entry.name.clone(),
        String::new(),
        format!("{}: {}", tr.d_publisher, view.entry.publisher),
        format!("{}: {}", tr.d_version, view.entry.version),
        format!("{}: {}", tr.d_installed, installed),
        format!("{}: {}", tr.d_homepage, view.entry.homepage),
        format!("{}: {}", tr.d_path, path),
        String::new(),
        description.clone(),
    ];
    if description.is_empty() {
        lines.pop();
    }
    lines.join("\r\n")
}

fn format_size(bytes: u64, tr: &T) -> String {
    if bytes == 0 {
        return tr.size_unknown.to_string();
    }
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn create_status_icons() -> StatusIcons {
    let size = (16.0 * nwg::scale_factor()) as i32;
    StatusIcons {
        ok: create_glyph_icon(GLYPH_CHECKMARK, COLOR_OK, size),
        warning: create_glyph_icon(GLYPH_WARNING, COLOR_WARNING, size),
        error: create_glyph_icon(GLYPH_ERROR, COLOR_ERROR, size),
    }
}

fn open_in_browser(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
