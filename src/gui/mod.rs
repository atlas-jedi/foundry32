//! Main window: server list with scope/reach columns, details pane, action buttons.

pub mod add_dialog;

use crate::discovery::{self, cli::CliListEntry, Discovery};
use crate::i18n::{t, Lang, T};
use crate::model::{McpServer, Scope, Transport};
use crate::mutation::{self, ServerDraft};
use crate::settings::AppSettings;
use crate::update_check::{self, UpdateInfo};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const CONNECTORS_URL: &str = "https://claude.ai/settings/connectors";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) struct MutationFailure {
    pub message: String,
    pub original_removed: bool,
}

/// Mailbox written by worker threads (CLI listing, update check, mutations,
/// dialog outcome), drained on the UI thread when the Notice fires.
#[derive(Default)]
pub(crate) struct Shared {
    pub cli: Option<Result<Vec<CliListEntry>, String>>,
    pub update: Option<Result<Option<UpdateInfo>, String>>,
    pub mutation: Option<Result<(), MutationFailure>>,
    pub dialog: Option<Option<ServerDraft>>,
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
    update_url: Option<String>,
}

pub struct HangarApp {
    window: nwg::Window,
    _grid: nwg::GridLayout,
    legend: nwg::Label,
    update_btn: nwg::Button,
    listview: nwg::ListView,
    details: nwg::TextBox,
    btn_add: nwg::Button,
    btn_edit: nwg::Button,
    btn_remove: nwg::Button,
    btn_refresh: nwg::Button,
    btn_connectors: nwg::Button,
    lang_label: nwg::Label,
    lang_combo: nwg::ComboBox<String>,
    status_bar: nwg::StatusBar,
    notice: nwg::Notice,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
}

pub fn run() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let app = Rc::new(build_app(AppSettings::load()));
    wire_events(&app);
    app.refresh();
    app.spawn_update_check();
    nwg::dispatch_thread_events();
}

fn build_app(settings: AppSettings) -> HangarApp {
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

    let mut legend = nwg::Label::default();
    nwg::Label::builder().parent(&window).text(tr.legend).build(&mut legend).expect("legend");

    let mut update_btn = nwg::Button::default();
    nwg::Button::builder().parent(&window).text("").build(&mut update_btn).expect("update_btn");

    let mut listview = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .build(&mut listview)
        .expect("listview");
    let widths = [170, 130, 190, 60, 240, 130];
    let titles = [tr.col_name, tr.col_scope, tr.col_reach, tr.col_type, tr.col_target, tr.col_status];
    for (i, (w, title)) in widths.iter().zip(titles.iter()).enumerate() {
        insert_report_list_view_column(&listview, i as i32, *w, title);
    }

    let mut details = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        .text(tr.details_placeholder)
        .readonly(true)
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL)
        .build(&mut details)
        .expect("details");

    let mut btn_add = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_add).build(&mut btn_add).expect("btn_add");
    let mut btn_edit = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_edit).build(&mut btn_edit).expect("btn_edit");
    let mut btn_remove = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_remove).build(&mut btn_remove).expect("btn_remove");
    let mut btn_refresh = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_refresh).build(&mut btn_refresh).expect("btn_refresh");
    let mut btn_connectors = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_connectors).build(&mut btn_connectors).expect("btn_connectors");

    let mut lang_label = nwg::Label::default();
    nwg::Label::builder().parent(&window).text(tr.lang_label).build(&mut lang_label).expect("lang_label");
    let mut lang_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .collection(vec!["Português (BR)".to_string(), "English".to_string()])
        .selected_index(Some(match lang { Lang::PtBr => 0, Lang::En => 1 }))
        .build(&mut lang_combo)
        .expect("lang_combo");

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

    let grid = nwg::GridLayout::default();
    nwg::GridLayout::builder()
        .parent(&window)
        .spacing(4)
        .child_item(nwg::GridLayoutItem::new(&legend, 0, 0, 10, 1))
        .child_item(nwg::GridLayoutItem::new(&update_btn, 10, 0, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&listview, 0, 1, 8, 11))
        .child_item(nwg::GridLayoutItem::new(&details, 8, 1, 4, 11))
        .child_item(nwg::GridLayoutItem::new(&btn_add, 0, 12, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&btn_edit, 1, 12, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&btn_remove, 2, 12, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&btn_refresh, 3, 12, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&btn_connectors, 5, 12, 3, 1))
        .child_item(nwg::GridLayoutItem::new(&lang_label, 9, 12, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&lang_combo, 10, 12, 2, 1))
        .build(&grid)
        .expect("grid");

    update_btn.set_visible(false);
    btn_edit.set_enabled(false);
    btn_remove.set_enabled(false);

    HangarApp {
        window,
        _grid: grid,
        legend,
        update_btn,
        listview,
        details,
        btn_add,
        btn_edit,
        btn_remove,
        btn_refresh,
        btn_connectors,
        lang_label,
        lang_combo,
        status_bar,
        notice,
        state: RefCell::new(UiState {
            lang,
            discovery: Discovery { servers: Vec::new(), warnings: Vec::new(), project_dirs: Vec::new() },
            claude: None,
            cli_running: false,
            mutating: false,
            selected_row: None,
            editing: None,
            update_url: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
    }
}

fn wire_events(app: &Rc<HangarApp>) {
    let evt_app = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = evt_app.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnButtonClick => {
                if handle == app.btn_add.handle {
                    app.open_dialog(None);
                } else if handle == app.btn_edit.handle {
                    app.edit_selected();
                } else if handle == app.btn_remove.handle {
                    app.remove_selected();
                } else if handle == app.btn_refresh.handle {
                    app.refresh();
                } else if handle == app.btn_connectors.handle {
                    open_in_browser(CONNECTORS_URL);
                } else if handle == app.update_btn.handle {
                    let url = app.state.borrow().update_url.clone();
                    if let Some(url) = url {
                        open_in_browser(&url);
                    }
                }
            }
            E::OnComboxBoxSelection if handle == app.lang_combo.handle => app.change_language(),
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

impl HangarApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
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
                self.set_status(&self.status_ready_text());
                return;
            };
            state.cli_running = true;
            claude
        };
        self.set_status(self.tr().status_cli_running);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let result = discovery::cli::run_mcp_list(&claude);
            shared.lock().unwrap().cli = Some(result);
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
        self.set_status(self.tr().status_mutating);
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
        let (cli, update, mutation_result, dialog) = {
            let mut shared = self.shared.lock().unwrap();
            (shared.cli.take(), shared.update.take(), shared.mutation.take(), shared.dialog.take())
        };

        if let Some(result) = cli {
            self.state.borrow_mut().cli_running = false;
            match result {
                Ok(entries) => {
                    discovery::merge_cli_entries(&mut self.state.borrow_mut().discovery, entries);
                    self.populate_list();
                }
                Err(error) => self.state.borrow_mut().discovery.warnings.push(error),
            }
            self.set_status(&self.status_ready_text());
        }

        if let Some(Ok(Some(info))) = update {
            let label = self.tr().update_available.replace("%V", &info.latest_version);
            self.update_btn.set_text(&label);
            self.update_btn.set_visible(true);
            self.state.borrow_mut().update_url = Some(info.html_url);
        }

        if let Some(result) = mutation_result {
            self.state.borrow_mut().mutating = false;
            self.set_action_buttons_enabled(true);
            match result {
                Ok(()) => {
                    self.set_status(self.tr().op_done);
                    self.refresh();
                }
                Err(failure) => {
                    let mut message = failure.message;
                    if failure.original_removed {
                        message = format!("{}\r\n\r\n{}", self.tr().replace_removed_warning, message);
                    }
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &message);
                    self.set_status(&self.tr().status_error.replace("%E", &message));
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
    }

    fn populate_list(&self) {
        self.listview.clear();
        let state = self.state.borrow();
        let tr = t(state.lang);
        for server in &state.discovery.servers {
            let target = if server.target.chars().count() > 70 {
                let head: String = server.target.chars().take(69).collect();
                format!("{head}…")
            } else {
                server.target.clone()
            };
            let row = [
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
        self.set_status(&self.status_ready_text());
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

    fn change_language(&self) {
        let lang = match self.lang_combo.selection() {
            Some(1) => Lang::En,
            _ => Lang::PtBr,
        };
        self.state.borrow_mut().lang = lang;
        let _ = AppSettings { lang }.save();
        self.relabel_all();
    }

    fn relabel_all(&self) {
        let tr = self.tr();
        self.window.set_text(tr.app_title);
        self.legend.set_text(tr.legend);
        self.btn_add.set_text(tr.btn_add);
        self.btn_edit.set_text(tr.btn_edit);
        self.btn_remove.set_text(tr.btn_remove);
        self.btn_refresh.set_text(tr.btn_refresh);
        self.btn_connectors.set_text(tr.btn_connectors);
        self.lang_label.set_text(tr.lang_label);
        let titles = [tr.col_name, tr.col_scope, tr.col_reach, tr.col_type, tr.col_target, tr.col_status];
        for (i, title) in titles.iter().enumerate() {
            self.listview.update_column(i, nwg::InsertListViewColumn {
                index: Some(i as i32),
                fmt: None,
                width: None,
                text: Some((*title).into()),
            });
        }
        self.populate_list();
    }

    fn set_action_buttons_enabled(&self, enabled: bool) {
        self.btn_add.set_enabled(enabled);
        self.btn_edit.set_enabled(enabled && self.state.borrow().selected_row.is_some());
        self.btn_remove.set_enabled(enabled && self.state.borrow().selected_row.is_some());
    }

    fn status_ready_text(&self) -> String {
        let state = self.state.borrow();
        let tr = t(state.lang);
        let mut text = tr.status_ready.replace("%N", &state.discovery.servers.len().to_string());
        if !state.discovery.warnings.is_empty() {
            text.push_str("   ⚠ ");
            text.push_str(&state.discovery.warnings.join(" | "));
        }
        text
    }

    fn set_status(&self, text: &str) {
        self.status_bar.set_text(0, text);
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

/// Inserts a report-view list view column via a direct `LVM_INSERTCOLUMNW` call.
///
/// `nwg::ListView::insert_column` unconditionally probes the existing column
/// count first by sending `LVM_GETCOLUMNWIDTH` in a loop until it returns 0 —
/// a message Microsoft documents as valid only for LVS_LIST/LVS_ICON views.
/// Sent against our LVS_REPORT (Detailed) list view, it never returns 0,
/// spinning the UI thread at 100% CPU forever before the message pump even
/// starts. We always supply an explicit column index, so that probed count
/// is never used — this reimplements only the needed subset of the call.
fn insert_report_list_view_column(listview: &nwg::ListView, index: i32, width: i32, text: &str) {
    use winapi::um::commctrl::{LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVM_INSERTCOLUMNW};
    use winapi::um::winuser::SendMessageW;

    let Some(handle) = listview.handle.hwnd() else { return };
    let scaled_width = (width as f64 * nwg::scale_factor()) as i32;
    let mut wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();

    let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
    column.mask = LVCF_TEXT | LVCF_WIDTH;
    column.cx = scaled_width;
    column.pszText = wide_text.as_mut_ptr();
    column.cchTextMax = wide_text.len() as i32;

    unsafe {
        SendMessageW(handle, LVM_INSERTCOLUMNW, index as usize, &mut column as *mut LVCOLUMNW as isize);
    }
}
