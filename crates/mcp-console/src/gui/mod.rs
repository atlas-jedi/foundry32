//! Main window: menu bar, server list with scope/reach columns, details pane,
//! classic-style action buttons and a three-part status bar.

pub mod add_dialog;
pub mod manual_dialog;
pub mod preferences_dialog;

use crate::discovery::{self, cli::CliListEntry, Discovery};
use crate::i18n::{t, Lang, T};
use crate::model::{McpServer, Scope, Transport};
use crate::mutation::{self, ServerDraft};
use crate::settings::AppSettings;
use foundry_common::theme::{apply_explorer_theme, create_glyph_icon};
use foundry_common::ui::{insert_report_list_view_column, set_menu_item_text, set_submenu_text};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// Re-exported so the dialog submodules keep using `super::apply_classic_button_theme`.
pub(crate) use foundry_common::theme::apply_classic_button_theme;

const CONNECTORS_URL: &str = "https://claude.ai/settings/connectors";
const REPO_URL: &str = "https://github.com/atlas-jedi/mcp-hangar";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// This tool's own version, shown in the status bar and About box. Update
/// checking itself is owned by the Foundry32 hub, not by the tool.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Layout metrics in logical pixels (nwg setters apply the DPI scale).
const MARGIN: i32 = 8;
const BUTTON_W: i32 = 110;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const GROUP_GAP: i32 = 18;
const DETAILS_W: i32 = 330;
const STATUS_SERVERS_W: i32 = 130;
const STATUS_VERSION_W: i32 = 90;
const VK_F5: u32 = 0x74;

/// Segoe MDL2 Assets glyphs (the system icon font on Windows 10/11).
const GLYPH_CHECKMARK: u16 = 0xE73E;
const GLYPH_WARNING: u16 = 0xE7BA;
const GLYPH_ERROR: u16 = 0xE783;
/// Fluent state colors as (r, g, b).
const COLOR_OK: (u8, u8, u8) = (0x10, 0x7C, 0x10);
const COLOR_WARNING: (u8, u8, u8) = (0x9D, 0x5D, 0x00);
const COLOR_ERROR: (u8, u8, u8) = (0xC4, 0x2B, 0x1C);

/// Which state icon the status bar message part shows.
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

pub(crate) struct MutationFailure {
    pub message: String,
    pub original_removed: bool,
}

/// Mailbox written by worker threads (CLI listing, mutations, dialog outcomes),
/// drained on the UI thread when the Notice fires.
#[derive(Default)]
pub(crate) struct Shared {
    pub cli: Option<Result<Vec<CliListEntry>, String>>,
    pub mutation: Option<Result<(), MutationFailure>>,
    pub dialog: Option<Option<ServerDraft>>,
    pub preferences: Option<Option<Lang>>,
    pub manual: Option<()>,
}

enum MutationOp {
    Add(ServerDraft),
    Remove { name: String, scope: Scope },
    Replace { original_name: String, original_scope: Scope, draft: ServerDraft },
}

struct UiState {
    lang: Lang,
    discovery: Discovery,
    claude: Option<PathBuf>,
    cli_running: bool,
    mutating: bool,
    selected_row: Option<usize>,
    editing: Option<(String, Scope)>,
}

pub struct ConsoleApp {
    window: nwg::Window,
    listview: nwg::ListView,
    details: nwg::TextBox,
    btn_add: nwg::Button,
    btn_edit: nwg::Button,
    btn_remove: nwg::Button,
    btn_refresh: nwg::Button,
    menu_file: nwg::Menu,
    mi_prefs: nwg::MenuItem,
    mi_exit: nwg::MenuItem,
    menu_servers: nwg::Menu,
    mi_add: nwg::MenuItem,
    mi_edit: nwg::MenuItem,
    mi_remove: nwg::MenuItem,
    mi_refresh: nwg::MenuItem,
    mi_connectors: nwg::MenuItem,
    menu_help: nwg::Menu,
    mi_site: nwg::MenuItem,
    mi_manual: nwg::MenuItem,
    mi_about: nwg::MenuItem,
    _menu_seps: Vec<nwg::MenuSeparator>,
    status_bar: nwg::StatusBar,
    status_icons: StatusIcons,
    notice: nwg::Notice,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
}

pub fn run() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let app = Rc::new(build_app(AppSettings::load()));
    wire_events(&app);
    app.redraw_menu_bar();
    app.layout();
    app.set_version_text();
    app.refresh();
    nwg::dispatch_thread_events();
}

fn build_app(settings: AppSettings) -> ConsoleApp {
    let lang = settings.lang;
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((1020, 620))
        .position((200, 120))
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
    let mi_prefs = item(tr.menu_file_prefs, &menu_file);
    separator(&menu_file);
    let mi_exit = item(tr.menu_file_exit, &menu_file);

    let menu_servers = menu(tr.menu_servers, &window);
    let mi_add = item(tr.menu_srv_add, &menu_servers);
    let mi_edit = item(tr.menu_srv_edit, &menu_servers);
    let mi_remove = item(tr.menu_srv_remove, &menu_servers);
    separator(&menu_servers);
    let mi_refresh = item(tr.menu_srv_refresh, &menu_servers);
    separator(&menu_servers);
    let mi_connectors = item(tr.menu_srv_connectors, &menu_servers);

    let menu_help = menu(tr.menu_help, &window);
    let mi_site = item(tr.menu_help_site, &menu_help);
    separator(&menu_help);
    let mi_manual = item(tr.menu_help_manual, &menu_help);
    let mi_about = item(tr.menu_help_about, &menu_help);

    let mut listview = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .build(&mut listview)
        .expect("listview");
    // nwg forces LVS_NOCOLUMNHEADER at creation for backward compatibility —
    // re-enable the column header (Wireshark-style report view).
    listview.set_headers_enabled(true);
    let widths = [36, 170, 130, 190, 60, 240, 130];
    let titles = [tr.col_num, tr.col_name, tr.col_scope, tr.col_reach, tr.col_type, tr.col_target, tr.col_status];
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

    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let btn_add = button(tr.btn_add, &window);
    let btn_edit = button(tr.btn_edit, &window);
    let btn_remove = button(tr.btn_remove, &window);
    let btn_refresh = button(tr.btn_refresh, &window);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    // Window icon from embedded resource id 1 (absent on plain GNU dev builds).
    if let Ok(embed) = nwg::EmbedResource::load(None) {
        if let Some(icon) = embed.icon(1, None) {
            window.set_icon(Some(&icon));
        }
    }

    mi_edit.set_enabled(false);
    mi_remove.set_enabled(false);
    btn_edit.set_enabled(false);
    btn_remove.set_enabled(false);

    ConsoleApp {
        window,
        listview,
        details,
        btn_add,
        btn_edit,
        btn_remove,
        btn_refresh,
        menu_file,
        mi_prefs,
        mi_exit,
        menu_servers,
        mi_add,
        mi_edit,
        mi_remove,
        mi_refresh,
        mi_connectors,
        menu_help,
        mi_site,
        mi_manual,
        mi_about,
        _menu_seps: menu_seps,
        status_bar,
        status_icons: create_status_icons(),
        notice,
        state: RefCell::new(UiState {
            lang,
            discovery: Discovery { servers: Vec::new(), warnings: Vec::new(), project_dirs: Vec::new() },
            claude: None,
            cli_running: false,
            mutating: false,
            selected_row: None,
            editing: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
    }
}

fn wire_events(app: &Rc<ConsoleApp>) {
    let evt_app = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = evt_app.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => {
                app.layout();
            }
            E::OnButtonClick => {
                if handle == app.btn_add.handle {
                    app.open_dialog(None);
                } else if handle == app.btn_edit.handle {
                    app.edit_selected();
                } else if handle == app.btn_remove.handle {
                    app.remove_selected();
                } else if handle == app.btn_refresh.handle {
                    app.refresh();
                }
            }
            E::OnMenuItemSelected => {
                if handle == app.mi_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.mi_add.handle {
                    app.open_dialog(None);
                } else if handle == app.mi_edit.handle {
                    app.edit_selected();
                } else if handle == app.mi_remove.handle {
                    app.remove_selected();
                } else if handle == app.mi_refresh.handle {
                    app.refresh();
                } else if handle == app.mi_connectors.handle {
                    open_in_browser(CONNECTORS_URL);
                } else if handle == app.mi_prefs.handle {
                    app.open_preferences();
                } else if handle == app.mi_site.handle {
                    open_in_browser(REPO_URL);
                } else if handle == app.mi_manual.handle {
                    app.open_manual();
                } else if handle == app.mi_about.handle {
                    app.show_about();
                }
            }
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_F5) = evt_data {
                    if !app.state.borrow().mutating {
                        app.refresh();
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
            E::OnNotice if handle == app.notice.handle => app.drain_shared(),
            _ => {}
        }
    });
    // Handler lives as long as the process; leak intentionally (single window app).
    std::mem::forget(handler);
}

impl ConsoleApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    /// Manual layout: list + details side by side, button row bottom-left,
    /// status bar parts right-aligned. Replaces GridLayout, which stretched
    /// every control to its cell.
    fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < 320 || height < 220 {
            return;
        }
        let status_h = self.status_bar_height();
        let button_y = height - status_h - BUTTON_H - MARGIN;
        let content_h = (button_y - 2 * MARGIN).max(60) as u32;
        let details_w = DETAILS_W.min(width / 3);
        let details_x = width - MARGIN - details_w;
        let list_w = (details_x - 2 * MARGIN).max(120) as u32;

        self.listview.set_position(MARGIN, MARGIN);
        self.listview.set_size(list_w, content_h);
        self.details.set_position(details_x, MARGIN);
        self.details.set_size(details_w as u32, content_h);

        let mut x = MARGIN;
        for button in [&self.btn_add, &self.btn_edit, &self.btn_remove] {
            button.set_position(x, button_y);
            button.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
        x += GROUP_GAP;
        self.btn_refresh.set_position(x, button_y);
        self.btn_refresh.set_size(BUTTON_W as u32, BUTTON_H as u32);

        self.set_status_parts();
    }

    /// Status bar height in logical pixels (nwg's StatusBar exposes no size getter).
    fn status_bar_height(&self) -> i32 {
        use winapi::um::winuser::GetWindowRect;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return 23 };
        let mut rect: winapi::shared::windef::RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(hwnd, &mut rect) };
        ((rect.bottom - rect.top) as f64 / nwg::scale_factor()) as i32
    }

    /// Splits the status bar into message | server count | version parts.
    fn set_status_parts(&self) {
        use winapi::um::commctrl::SB_SETPARTS;
        use winapi::um::winuser::SendMessageW;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return };
        let scale = nwg::scale_factor();
        let px = |v: i32| (v as f64 * scale) as i32;
        let width = px(self.window.size().0 as i32);
        let right_w = px(STATUS_VERSION_W);
        let edges = [width - px(STATUS_SERVERS_W) - right_w, width - right_w, -1i32];
        unsafe {
            SendMessageW(hwnd, SB_SETPARTS, edges.len(), edges.as_ptr() as isize);
        }
    }

    fn refresh(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.discovery = discovery::discover_file_servers();
            state.claude = discovery::cli::locate_claude_binary();
            if state.claude.is_none() {
                let warning = t(state.lang).warn_claude_missing.to_string();
                state.discovery.warnings.push(warning);
            }
            state.selected_row = None;
        }
        self.populate_list();
        self.spawn_cli_list();
    }

    fn spawn_cli_list(&self) {
        let claude = {
            let mut state = self.state.borrow_mut();
            if state.cli_running {
                return;
            }
            let Some(claude) = state.claude.clone() else {
                drop(state);
                self.set_ready_status();
                return;
            };
            state.cli_running = true;
            claude
        };
        self.set_status(self.tr().status_cli_running, StatusTone::Busy);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let result = discovery::cli::run_mcp_list(&claude);
            shared.lock().unwrap().cli = Some(result);
            sender.notice();
        });
    }

    fn spawn_mutation(&self, op: MutationOp) {
        let claude = {
            let state = self.state.borrow();
            state.claude.clone()
        };
        let Some(claude) = claude else {
            nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, self.tr().warn_claude_missing);
            return;
        };
        self.state.borrow_mut().mutating = true;
        self.set_action_buttons_enabled(false);
        self.set_status(self.tr().status_mutating, StatusTone::Busy);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let result: Result<(), MutationFailure> = (|| match &op {
                MutationOp::Add(draft) => {
                    mutation::backup_configs(&draft.scope).map_err(plain)?;
                    mutation::add_server(&claude, draft).map_err(plain)
                }
                MutationOp::Remove { name, scope } => {
                    mutation::backup_configs(scope).map_err(plain)?;
                    mutation::remove_server(&claude, name, scope).map_err(plain)
                }
                MutationOp::Replace { original_name, original_scope, draft } => {
                    mutation::backup_configs(original_scope).map_err(plain)?;
                    if draft.scope != *original_scope {
                        mutation::backup_configs(&draft.scope).map_err(plain)?;
                    }
                    mutation::remove_server(&claude, original_name, original_scope).map_err(plain)?;
                    mutation::add_server(&claude, draft)
                        .map_err(|message| MutationFailure { message, original_removed: true })
                }
            })();
            shared.lock().unwrap().mutation = Some(result);
            sender.notice();
        });
    }

    fn drain_shared(&self) {
        let (cli, mutation_result, dialog, preferences, manual) = {
            let mut shared = self.shared.lock().unwrap();
            (
                shared.cli.take(),
                shared.mutation.take(),
                shared.dialog.take(),
                shared.preferences.take(),
                shared.manual.take(),
            )
        };

        if let Some(result) = cli {
            self.state.borrow_mut().cli_running = false;
            match result {
                Ok(entries) => {
                    discovery::merge_cli_entries(&mut self.state.borrow_mut().discovery, entries);
                    self.populate_list();
                }
                Err(error) => {
                    self.state.borrow_mut().discovery.warnings.push(error);
                    self.set_ready_status();
                }
            }
        }

        if let Some(result) = mutation_result {
            self.state.borrow_mut().mutating = false;
            self.set_action_buttons_enabled(true);
            match result {
                Ok(()) => {
                    self.set_status(self.tr().op_done, StatusTone::Ok);
                    self.refresh();
                }
                Err(failure) => {
                    let mut message = failure.message;
                    if failure.original_removed {
                        message = format!("{}\r\n\r\n{}", self.tr().replace_removed_warning, message);
                    }
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &message);
                    self.set_status(&self.tr().status_error.replace("%E", &message), StatusTone::Error);
                }
            }
        }

        if let Some(outcome) = dialog {
            self.window.set_enabled(true);
            let editing = self.state.borrow_mut().editing.take();
            if let Some(draft) = outcome {
                let op = match editing {
                    Some((original_name, original_scope)) => {
                        MutationOp::Replace { original_name, original_scope, draft }
                    }
                    None => MutationOp::Add(draft),
                };
                self.spawn_mutation(op);
            }
        }

        if manual.is_some() {
            self.window.set_enabled(true);
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

    fn populate_list(&self) {
        self.listview.clear();
        let state = self.state.borrow();
        let tr = t(state.lang);
        for (index, server) in state.discovery.servers.iter().enumerate() {
            let target = if server.target.chars().count() > 70 {
                let head: String = server.target.chars().take(69).collect();
                format!("{head}…")
            } else {
                server.target.clone()
            };
            let row = [
                (index + 1).to_string(),
                server.name.clone(),
                scope_label(&server.scope, tr).to_string(),
                reach_label(&server.scope, tr).to_string(),
                server.transport.label().to_string(),
                target,
                server.status.clone().unwrap_or_else(|| "—".into()),
            ];
            self.listview.insert_items_row(None, &row);
        }
        drop(state);
        self.state.borrow_mut().selected_row = None;
        self.details.set_text(self.tr().details_placeholder);
        self.btn_edit.set_enabled(false);
        self.btn_remove.set_enabled(false);
        self.mi_edit.set_enabled(false);
        self.mi_remove.set_enabled(false);
        self.set_ready_status();
        self.set_server_count_text();
    }

    fn select_row(&self, row: usize) {
        let state = self.state.borrow();
        let Some(server) = state.discovery.servers.get(row) else { return };
        let tr = t(state.lang);
        let text = details_text(server, tr);
        let editable = server.scope.is_editable() && !state.mutating;
        drop(state);
        self.state.borrow_mut().selected_row = Some(row);
        self.details.set_text(&text);
        self.btn_edit.set_enabled(editable);
        self.btn_remove.set_enabled(editable);
        self.mi_edit.set_enabled(editable);
        self.mi_remove.set_enabled(editable);
    }

    fn edit_selected(&self) {
        let (draft, original) = {
            let state = self.state.borrow();
            let Some(row) = state.selected_row else { return };
            let Some(server) = state.discovery.servers.get(row) else { return };
            if !server.scope.is_editable() {
                return;
            }
            (draft_from_server(server), (server.name.clone(), server.scope.clone()))
        };
        self.state.borrow_mut().editing = Some(original);
        self.open_dialog(Some(draft));
    }

    fn remove_selected(&self) {
        let target = {
            let state = self.state.borrow();
            let Some(row) = state.selected_row else { return };
            let Some(server) = state.discovery.servers.get(row) else { return };
            if !server.scope.is_editable() {
                return;
            }
            (server.name.clone(), server.scope.clone())
        };
        let tr = self.tr();
        let choice = nwg::modal_message(&self.window.handle, &nwg::MessageParams {
            title: tr.confirm_remove_title,
            content: &tr.confirm_remove_body.replace("%S", &target.0),
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Warning,
        });
        if choice == nwg::MessageChoice::Yes {
            self.spawn_mutation(MutationOp::Remove { name: target.0, scope: target.1 });
        }
    }

    fn open_dialog(&self, prefill: Option<ServerDraft>) {
        let params = {
            let state = self.state.borrow();
            add_dialog::DialogParams {
                lang: state.lang,
                known_dirs: state.discovery.project_dirs.clone(),
                editing: state.editing.is_some(),
                prefill,
                shared: Arc::clone(&self.shared),
                notify: self.notice.sender(),
            }
        };
        self.window.set_enabled(false);
        add_dialog::spawn(params);
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

    fn open_manual(&self) {
        let params = manual_dialog::ManualParams {
            lang: self.state.borrow().lang,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        };
        self.window.set_enabled(false);
        manual_dialog::spawn(params);
    }

    fn show_about(&self) {
        let tr = self.tr();
        let body = tr.about_body.replace("%V", CURRENT_VERSION);
        nwg::modal_info_message(&self.window.handle, tr.about_title, &body);
    }

    fn relabel_all(&self) {
        let tr = self.tr();
        self.window.set_text(tr.app_title);
        self.btn_add.set_text(tr.btn_add);
        self.btn_edit.set_text(tr.btn_edit);
        self.btn_remove.set_text(tr.btn_remove);
        self.btn_refresh.set_text(tr.btn_refresh);
        let titles = [tr.col_num, tr.col_name, tr.col_scope, tr.col_reach, tr.col_type, tr.col_target, tr.col_status];
        for (i, title) in titles.iter().enumerate() {
            self.listview.update_column(i, nwg::InsertListViewColumn {
                index: Some(i as i32),
                fmt: None,
                width: None,
                text: Some((*title).into()),
            });
        }
        self.relabel_menus();
        self.set_version_text();
        self.populate_list();
    }

    fn relabel_menus(&self) {
        let tr = self.tr();
        set_submenu_text(&self.menu_file, tr.menu_file);
        set_submenu_text(&self.menu_servers, tr.menu_servers);
        set_submenu_text(&self.menu_help, tr.menu_help);
        set_menu_item_text(&self.mi_prefs, tr.menu_file_prefs);
        set_menu_item_text(&self.mi_exit, tr.menu_file_exit);
        set_menu_item_text(&self.mi_add, tr.menu_srv_add);
        set_menu_item_text(&self.mi_edit, tr.menu_srv_edit);
        set_menu_item_text(&self.mi_remove, tr.menu_srv_remove);
        set_menu_item_text(&self.mi_refresh, tr.menu_srv_refresh);
        set_menu_item_text(&self.mi_connectors, tr.menu_srv_connectors);
        set_menu_item_text(&self.mi_site, tr.menu_help_site);
        set_menu_item_text(&self.mi_manual, tr.menu_help_manual);
        set_menu_item_text(&self.mi_about, tr.menu_help_about);
        self.redraw_menu_bar();
    }

    /// Repaints the menu bar frame — needed after attaching menus to an
    /// already-visible window and after in-place caption updates.
    fn redraw_menu_bar(&self) {
        if let Some(hwnd) = self.window.handle.hwnd() {
            unsafe { winapi::um::winuser::DrawMenuBar(hwnd) };
        }
    }

    fn set_action_buttons_enabled(&self, enabled: bool) {
        let has_selection = self.state.borrow().selected_row.is_some();
        self.btn_add.set_enabled(enabled);
        self.btn_edit.set_enabled(enabled && has_selection);
        self.btn_remove.set_enabled(enabled && has_selection);
        self.mi_add.set_enabled(enabled);
        self.mi_edit.set_enabled(enabled && has_selection);
        self.mi_remove.set_enabled(enabled && has_selection);
    }

    /// "Ready" or the joined warnings, with the matching state icon.
    fn set_ready_status(&self) {
        let (text, tone) = {
            let state = self.state.borrow();
            let tr = t(state.lang);
            if state.discovery.warnings.is_empty() {
                (tr.status_left_ready.to_string(), StatusTone::Ok)
            } else {
                (state.discovery.warnings.join("  |  "), StatusTone::Warning)
            }
        };
        self.set_status(&text, tone);
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

    fn set_server_count_text(&self) {
        let count = self.state.borrow().discovery.servers.len().to_string();
        let text = self.tr().status_servers.replace("%N", &count);
        self.status_bar.set_text(1, &text);
    }

    fn set_version_text(&self) {
        self.status_bar.set_text(2, &format!("v{CURRENT_VERSION}"));
    }
}

fn plain(message: String) -> MutationFailure {
    MutationFailure { message, original_removed: false }
}

fn scope_label(scope: &Scope, tr: &T) -> &'static str {
    match scope {
        Scope::Account => tr.scope_account,
        Scope::Plugin => tr.scope_plugin,
        Scope::User => tr.scope_user,
        Scope::Project { .. } => tr.scope_project,
        Scope::Local { .. } => tr.scope_local,
        Scope::Unknown => tr.scope_unknown,
    }
}

fn reach_label(scope: &Scope, tr: &T) -> &'static str {
    match scope {
        Scope::Account => tr.reach_account,
        Scope::Project { .. } => tr.reach_repo,
        Scope::Plugin | Scope::User | Scope::Local { .. } => tr.reach_machine,
        Scope::Unknown => tr.reach_unknown,
    }
}

fn details_text(server: &McpServer, tr: &T) -> String {
    let reach_detail = match &server.scope {
        Scope::Account => tr.detail_reach_account,
        Scope::Project { .. } => tr.detail_reach_repo,
        Scope::Plugin | Scope::User | Scope::Local { .. } => tr.detail_reach_machine,
        Scope::Unknown => tr.detail_reach_unknown,
    };
    let env = if server.env_keys.is_empty() {
        tr.d_none.to_string()
    } else {
        server.env_keys.join(", ")
    };
    let mut lines = vec![
        server.name.clone(),
        String::new(),
        format!("{}: {}", tr.d_scope, scope_label(&server.scope, tr)),
        format!("{}: {}", tr.d_reach, reach_label(&server.scope, tr)),
        format!("{}: {}", tr.d_type, server.transport.label()),
        format!("{}: {}", tr.d_target, server.target),
        format!("{}: {}", tr.d_env, env),
    ];
    if let Some(source) = &server.source_file {
        lines.push(format!("{}: {}", tr.d_source, source));
    }
    if let Some(dir) = server.scope.project_dir() {
        lines.push(format!("{}: {}", tr.d_source, dir));
    }
    if let Some(status) = &server.status {
        lines.push(format!("{}: {}", tr.d_status, status));
    }
    lines.push(String::new());
    lines.push(reach_detail.to_string());
    lines.join("\r\n")
}

fn draft_from_server(server: &McpServer) -> ServerDraft {
    let (target, args) = match server.transport {
        Transport::Stdio => {
            let mut parts = mutation::split_command(&server.target);
            if parts.is_empty() {
                (String::new(), Vec::new())
            } else {
                let program = parts.remove(0);
                (program, parts)
            }
        }
        _ => (server.target.clone(), Vec::new()),
    };
    ServerDraft {
        name: server.name.clone(),
        scope: server.scope.clone(),
        transport: server.transport,
        target,
        args,
        env: server.env_keys.iter().map(|k| (k.clone(), String::new())).collect(),
        headers: Vec::new(),
    }
}

fn open_in_browser(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn create_status_icons() -> StatusIcons {
    let size = (16.0 * nwg::scale_factor()) as i32;
    StatusIcons {
        ok: create_glyph_icon(GLYPH_CHECKMARK, COLOR_OK, size),
        warning: create_glyph_icon(GLYPH_WARNING, COLOR_WARNING, size),
        error: create_glyph_icon(GLYPH_ERROR, COLOR_ERROR, size),
    }
}

// Classic theming (apply_classic_button_theme / apply_explorer_theme /
// create_glyph_icon) and the menu/list-view shims (set_menu_item_text /
// set_submenu_text / insert_report_list_view_column) now live in
// `foundry_common::{theme, ui}` and are imported at the top of this module.
