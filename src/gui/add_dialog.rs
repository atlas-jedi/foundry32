//! Add/Edit dialog — three-step wizard (identity → scope → connection) with a
//! persistent "what happens when you save" footer that live-updates on every
//! change. Each step is validated on Next. Runs on its own thread with its own
//! window + event loop (nwg multithread dialog pattern) and reports the
//! outcome through `Shared.dialog` + a Notice back to the main window.

use super::{apply_classic_button_theme, Shared};
use crate::i18n::{t, Lang, T};
use crate::model::{Scope, Transport};
use crate::mutation::{split_command, ServerDraft};
use native_windows_gui as nwg;
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 14;
const DIALOG_W: i32 = 620;
const STEP_CONTENT_Y: i32 = 50;
/// Bottom y of each step's content, in client coordinates — the footer and the
/// window height follow the active step so no dead space is left.
const STEP_CONTENT_BOTTOM: [i32; 3] = [116, 216, 354];
const FOOTER_GAP: i32 = 14;
const SUMMARY_H: i32 = 76;
const BUTTON_W: i32 = 110;
const BUTTON_H: i32 = 28;
const LAST_STEP: usize = 2;
const SEPARATOR_COLOR: [u8; 3] = [0xD0, 0xD0, 0xD0];

pub struct DialogParams {
    pub lang: Lang,
    pub known_dirs: Vec<String>,
    pub editing: bool,
    pub prefill: Option<ServerDraft>,
    pub shared: Arc<Mutex<Shared>>,
    pub notify: nwg::NoticeSender,
}

pub fn spawn(params: DialogParams) {
    std::thread::spawn(move || run_dialog(params));
}

struct DialogUi {
    window: nwg::Window,
    section_font: nwg::Font,
    hint_font: nwg::Font,
    step_headers: [nwg::Label; 3],
    /// Handles of every control belonging to each step, for show/hide.
    steps: [Vec<nwg::ControlHandle>; 3],
    current_step: Cell<usize>,
    name_input: nwg::TextInput,
    scope_user: nwg::RadioButton,
    scope_project: nwg::RadioButton,
    scope_local: nwg::RadioButton,
    dir_label: nwg::Label,
    dir_input: nwg::TextInput,
    known_label: nwg::Label,
    known_combo: nwg::ComboBox<String>,
    dir_hint: nwg::Label,
    tr_stdio: nwg::RadioButton,
    tr_http: nwg::RadioButton,
    tr_sse: nwg::RadioButton,
    target_label: nwg::Label,
    target_input: nwg::TextInput,
    target_hint: nwg::Label,
    env_box: nwg::TextBox,
    headers_label: nwg::Label,
    headers_box: nwg::TextBox,
    headers_hint: nwg::Label,
    footer_separator: nwg::Label,
    summary_header: nwg::Label,
    summary: nwg::Label,
    backup_note: nwg::Label,
    back_btn: nwg::Button,
    next_btn: nwg::Button,
    cancel_btn: nwg::Button,
    editing: bool,
    /// Static labels kept alive for the dialog's lifetime — nwg destroys the
    /// native control as soon as the wrapper struct drops.
    _labels: Vec<nwg::Label>,
}

fn run_dialog(params: DialogParams) {
    let tr = t(params.lang);
    let ui = build_ui(&params, tr);

    apply_prefill(&ui, &params);
    sync_field_states(&ui, tr);
    update_summary(&ui, tr);
    switch_step(&ui, tr, 0);
    ui.window.set_visible(true);
    ui.name_input.set_focus();

    let ui = Rc::new(ui);
    let window_handle = ui.window.handle;
    let shared = Arc::clone(&params.shared);
    let notify = params.notify;
    let lang = params.lang;
    let editing = params.editing;
    let known_dirs = params.known_dirs.clone();
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == ui.window.handle => {
                send_outcome(&shared, &notify, None);
            }
            E::OnButtonClick if handle == ui.cancel_btn.handle => {
                send_outcome(&shared, &notify, None);
                ui.window.close();
            }
            E::OnButtonClick if handle == ui.back_btn.handle => {
                let step = ui.current_step.get();
                if step > 0 {
                    switch_step(&ui, t(lang), step - 1);
                }
            }
            E::OnButtonClick if handle == ui.next_btn.handle => {
                advance(&ui, lang, editing, &shared, &notify);
            }
            E::OnButtonClick if is_choice_radio(&ui, handle) => {
                sync_field_states(&ui, t(lang));
                update_summary(&ui, t(lang));
            }
            E::OnComboxBoxSelection if handle == ui.known_combo.handle => {
                if let Some(index) = ui.known_combo.selection() {
                    if let Some(dir) = known_dirs.get(index) {
                        ui.dir_input.set_text(dir);
                    }
                }
                update_summary(&ui, t(lang));
            }
            E::OnTextInput => update_summary(&ui, t(lang)),
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

fn build_ui(params: &DialogParams, tr: &'static T) -> DialogUi {
    // Born hidden: the initial switch_step sizes the window to step 1 before
    // the first paint, so it never flashes at the wrong height.
    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((DIALOG_W, 600))
        .position((360, 110))
        .title(if params.editing { tr.dlg_title_edit } else { tr.dlg_title_add })
        .flags(nwg::WindowFlags::WINDOW)
        .build(&mut window)
        .expect("dialog window");
    let client_w = window.size().0 as i32;
    let field_w = client_w - 2 * MARGIN;

    let mut section_font = nwg::Font::default();
    let _ = nwg::Font::builder().family("Segoe UI").size(16).weight(700).build(&mut section_font);
    let mut hint_font = nwg::Font::default();
    let _ = nwg::Font::builder().family("Segoe UI").size(13).build(&mut hint_font);

    let make_label = |text: &str, x: i32, y: i32, w: i32, h: i32, font: Option<&nwg::Font>| {
        let mut control = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .text(text)
            .position((x, y))
            .size((w, h))
            .build(&mut control)
            .expect("label");
        if font.is_some() {
            control.set_font(font);
        }
        control
    };
    let make_separator = |y: i32| {
        let mut control = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .text("")
            .position((MARGIN, y))
            .size((field_w, 1))
            .background_color(Some(SEPARATOR_COLOR))
            .build(&mut control)
            .expect("separator");
        control
    };
    let make_input = |y: i32| {
        let mut control = nwg::TextInput::default();
        nwg::TextInput::builder()
            .parent(&window)
            .position((MARGIN, y))
            .size((field_w, 24))
            .build(&mut control)
            .expect("input");
        control
    };
    let make_box = |y: i32, h: i32| {
        let mut control = nwg::TextBox::default();
        nwg::TextBox::builder()
            .parent(&window)
            .position((MARGIN, y))
            .size((field_w, h))
            .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL)
            .build(&mut control)
            .expect("box");
        control
    };
    let make_radio = |text: &str, x: i32, y: i32, w: i32, group_start: bool, checked: bool| {
        let mut flags = nwg::RadioButtonFlags::VISIBLE | nwg::RadioButtonFlags::TAB_STOP;
        if group_start {
            flags |= nwg::RadioButtonFlags::GROUP;
        }
        let state = if checked {
            nwg::RadioButtonState::Checked
        } else {
            nwg::RadioButtonState::Unchecked
        };
        let mut control = nwg::RadioButton::default();
        nwg::RadioButton::builder()
            .parent(&window)
            .text(text)
            .position((x, y))
            .size((w, 20))
            .flags(flags)
            .check_state(state)
            .build(&mut control)
            .expect("radio");
        control
    };
    let make_button = |text: &str, x: i32| {
        let mut control = nwg::Button::default();
        nwg::Button::builder()
            .parent(&window)
            .text(text)
            .position((x, 0))
            .size((BUTTON_W, BUTTON_H))
            .build(&mut control)
            .expect("button");
        apply_classic_button_theme(&control);
        control
    };

    let mut labels = Vec::new();

    let header_w = field_w / 3;
    let step_headers = [
        make_label(tr.dlg_step_identity, MARGIN, 12, header_w, 22, Some(&section_font)),
        make_label(tr.dlg_step_scope, MARGIN + header_w, 12, header_w, 22, Some(&hint_font)),
        make_label(tr.dlg_step_connection, MARGIN + 2 * header_w, 12, header_w, 22, Some(&hint_font)),
    ];
    labels.push(make_separator(40));

    // Step 1 — identity.
    let y = STEP_CONTENT_Y;
    let name_label = make_label(tr.dlg_name, MARGIN, y, field_w, 18, None);
    let name_input = make_input(y + 22);
    let name_hint = make_label(tr.dlg_name_hint, MARGIN, y + 50, field_w, 16, Some(&hint_font));
    let step0 = vec![name_label.handle, name_input.handle, name_hint.handle];
    labels.push(name_label);
    labels.push(name_hint);

    // Step 2 — scope.
    let scope_user = make_radio(tr.dlg_scope_user_radio, MARGIN, y, field_w, true, true);
    let scope_project = make_radio(tr.dlg_scope_project_radio, MARGIN, y + 20, field_w, false, false);
    let scope_local = make_radio(tr.dlg_scope_local_radio, MARGIN, y + 40, field_w, false, false);
    let dir_label = make_label(tr.dlg_dir, MARGIN, y + 68, field_w, 18, None);
    let dir_input = make_input(y + 90);
    let known_label = make_label(tr.dlg_known_fill, MARGIN, y + 125, 240, 18, Some(&hint_font));
    let mut known_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((MARGIN + 246, y + 122))
        .size((field_w - 246, 24))
        .collection(params.known_dirs.clone())
        .build(&mut known_combo)
        .expect("known_combo");
    let dir_hint = make_label(tr.dlg_dir_hint, MARGIN, y + 150, field_w, 16, Some(&hint_font));
    let step1 = vec![
        scope_user.handle,
        scope_project.handle,
        scope_local.handle,
        dir_label.handle,
        dir_input.handle,
        known_label.handle,
        known_combo.handle,
        dir_hint.handle,
    ];

    // Step 3 — connection.
    let transport_label = make_label(tr.dlg_transport, MARGIN, y, field_w, 18, None);
    let tr_stdio = make_radio(tr.dlg_tr_stdio_radio, MARGIN, y + 22, 220, true, true);
    let tr_http = make_radio(tr.dlg_tr_http_radio, MARGIN + 226, y + 22, 150, false, false);
    let tr_sse = make_radio(tr.dlg_tr_sse_radio, MARGIN + 382, y + 22, field_w - 382, false, false);
    let target_label = make_label(tr.dlg_target_cmd, MARGIN, y + 50, field_w, 18, None);
    let target_input = make_input(y + 72);
    let target_hint = make_label(tr.dlg_target_cmd_hint, MARGIN, y + 100, field_w, 16, Some(&hint_font));
    let env_label = make_label(tr.dlg_env, MARGIN, y + 124, field_w, 18, None);
    let env_box = make_box(y + 146, 48);
    let env_hint_text = if params.editing { tr.dlg_env_edit_note } else { tr.dlg_env_hint };
    let env_hint = make_label(env_hint_text, MARGIN, y + 198, field_w, 16, Some(&hint_font));
    let headers_label = make_label(tr.dlg_headers, MARGIN, y + 222, field_w, 18, None);
    let headers_box = make_box(y + 244, 40);
    let headers_hint = make_label(tr.dlg_headers_hint, MARGIN, y + 288, field_w, 16, Some(&hint_font));
    let step2 = vec![
        transport_label.handle,
        tr_stdio.handle,
        tr_http.handle,
        tr_sse.handle,
        target_label.handle,
        target_input.handle,
        target_hint.handle,
        env_label.handle,
        env_box.handle,
        env_hint.handle,
        headers_label.handle,
        headers_box.handle,
        headers_hint.handle,
    ];
    labels.push(transport_label);
    labels.push(env_label);
    labels.push(env_hint);

    // Persistent footer — anchored under the active step by `position_footer`.
    let footer_separator = make_separator(0);
    let summary_header = make_label(tr.dlg_sec_summary, MARGIN, 0, field_w, 20, Some(&section_font));
    let summary = make_label("", MARGIN, 0, field_w, SUMMARY_H, None);
    let backup_note = make_label(tr.dlg_backup_note, MARGIN, 0, field_w, 30, Some(&hint_font));

    let cancel_x = client_w - MARGIN - BUTTON_W;
    let next_x = cancel_x - 14 - BUTTON_W;
    let back_x = next_x - 6 - BUTTON_W;
    let back_btn = make_button(tr.dlg_back, back_x);
    let next_btn = make_button(tr.dlg_next, next_x);
    let cancel_btn = make_button(tr.dlg_cancel, cancel_x);

    DialogUi {
        window,
        section_font,
        hint_font,
        step_headers,
        steps: [step0, step1, step2],
        current_step: Cell::new(0),
        name_input,
        scope_user,
        scope_project,
        scope_local,
        dir_label,
        dir_input,
        known_label,
        known_combo,
        dir_hint,
        tr_stdio,
        tr_http,
        tr_sse,
        target_label,
        target_input,
        target_hint,
        env_box,
        headers_label,
        headers_box,
        headers_hint,
        footer_separator,
        summary_header,
        summary,
        backup_note,
        back_btn,
        next_btn,
        cancel_btn,
        editing: params.editing,
        _labels: labels,
    }
}

/// Shows only the given step's controls, highlights its header, re-anchors
/// the footer and adjusts the navigation buttons.
fn switch_step(ui: &DialogUi, tr: &T, step: usize) {
    ui.current_step.set(step);
    for (index, handles) in ui.steps.iter().enumerate() {
        let visible = index == step;
        for handle in handles {
            set_handle_visible(handle, visible);
        }
    }
    for (index, header) in ui.step_headers.iter().enumerate() {
        let active = index == step;
        let font = if active { &ui.section_font } else { &ui.hint_font };
        header.set_font(Some(font));
        header.set_enabled(active);
    }
    position_footer(ui, step);
    ui.back_btn.set_enabled(step > 0);
    ui.next_btn.set_text(if step == LAST_STEP { tr.dlg_ok } else { tr.dlg_next });
    match step {
        0 => ui.name_input.set_focus(),
        LAST_STEP => ui.target_input.set_focus(),
        _ => {}
    }
}

/// Anchors the footer right under the active step's content and resizes the
/// window (client area) to fit, so shorter steps leave no dead space.
fn position_footer(ui: &DialogUi, step: usize) {
    let footer_y = STEP_CONTENT_BOTTOM[step] + FOOTER_GAP;
    ui.footer_separator.set_position(MARGIN, footer_y);
    ui.summary_header.set_position(MARGIN, footer_y + 10);
    ui.summary.set_position(MARGIN, footer_y + 34);
    ui.backup_note.set_position(MARGIN, footer_y + 40 + SUMMARY_H);
    let buttons_y = footer_y + 76 + SUMMARY_H;
    let client_w = ui.window.size().0 as i32;
    let cancel_x = client_w - MARGIN - BUTTON_W;
    let next_x = cancel_x - 14 - BUTTON_W;
    let back_x = next_x - 6 - BUTTON_W;
    ui.back_btn.set_position(back_x, buttons_y);
    ui.next_btn.set_position(next_x, buttons_y);
    ui.cancel_btn.set_position(cancel_x, buttons_y);
    ui.window.set_size(client_w as u32, (buttons_y + BUTTON_H + 12) as u32);
}

/// Next button: validates the current step; on the last step builds the draft
/// and reports it back.
fn advance(
    ui: &Rc<DialogUi>,
    lang: Lang,
    editing: bool,
    shared: &Arc<Mutex<Shared>>,
    notify: &nwg::NoticeSender,
) {
    let tr = t(lang);
    let step = ui.current_step.get();
    let step_check = match step {
        0 => validate_name(ui, tr).map(|_| ()),
        1 => validate_scope(ui, tr).map(|_| ()),
        _ => Ok(()),
    };
    if let Err(message) = step_check {
        nwg::modal_error_message(&ui.window.handle, tr.dlg_err_title, message);
        return;
    }
    if step < LAST_STEP {
        switch_step(ui, tr, step + 1);
        return;
    }
    match build_draft(ui, lang) {
        Ok(draft) => {
            if editing && !confirm_blank_env_values(ui, lang, &draft) {
                return;
            }
            send_outcome(shared, notify, Some(draft));
            ui.window.close();
        }
        Err(message) => {
            nwg::modal_error_message(&ui.window.handle, tr.dlg_err_title, message);
        }
    }
}

fn set_handle_visible(handle: &nwg::ControlHandle, visible: bool) {
    use winapi::um::winuser::{ShowWindow, SW_HIDE, SW_SHOW};
    let Some(hwnd) = handle.hwnd() else { return };
    unsafe {
        ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

fn is_checked(radio: &nwg::RadioButton) -> bool {
    radio.check_state() == nwg::RadioButtonState::Checked
}

/// Programmatic check — BS_AUTORADIOBUTTON only unchecks siblings on user
/// clicks, so the whole group is set explicitly.
fn check_radio(target: &nwg::RadioButton, group: [&nwg::RadioButton; 3]) {
    for radio in group {
        let state = if radio.handle == target.handle {
            nwg::RadioButtonState::Checked
        } else {
            nwg::RadioButtonState::Unchecked
        };
        radio.set_check_state(state);
    }
}

fn is_choice_radio(ui: &DialogUi, handle: nwg::ControlHandle) -> bool {
    [
        &ui.scope_user,
        &ui.scope_project,
        &ui.scope_local,
        &ui.tr_stdio,
        &ui.tr_http,
        &ui.tr_sse,
    ]
    .iter()
    .any(|radio| radio.handle == handle)
}

fn apply_prefill(ui: &DialogUi, params: &DialogParams) {
    let Some(draft) = &params.prefill else {
        if let Some(first) = params.known_dirs.first() {
            ui.dir_input.set_text(first);
        }
        return;
    };
    ui.name_input.set_text(&draft.name);
    let scope_group = [&ui.scope_user, &ui.scope_project, &ui.scope_local];
    let scope_radio = match draft.scope {
        Scope::User => &ui.scope_user,
        Scope::Project { .. } => &ui.scope_project,
        _ => &ui.scope_local,
    };
    check_radio(scope_radio, scope_group);
    if let Some(dir) = draft.scope.project_dir() {
        ui.dir_input.set_text(dir);
    }
    let transport_group = [&ui.tr_stdio, &ui.tr_http, &ui.tr_sse];
    let transport_radio = match draft.transport {
        Transport::Stdio => &ui.tr_stdio,
        Transport::Http => &ui.tr_http,
        Transport::Sse => &ui.tr_sse,
    };
    check_radio(transport_radio, transport_group);
    ui.target_input.set_text(&join_command(&draft.target, &draft.args));
    let env_lines: Vec<String> = draft.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    ui.env_box.set_text(&env_lines.join("\r\n"));
}

fn sync_field_states(ui: &DialogUi, tr: &T) {
    let needs_dir = !is_checked(&ui.scope_user);
    ui.dir_label.set_enabled(needs_dir);
    ui.dir_input.set_enabled(needs_dir);
    ui.known_label.set_enabled(needs_dir);
    ui.known_combo.set_enabled(needs_dir);
    ui.dir_hint.set_enabled(needs_dir);

    let stdio = is_checked(&ui.tr_stdio);
    ui.target_label.set_text(if stdio { tr.dlg_target_cmd } else { tr.dlg_target_url });
    ui.target_hint.set_text(if stdio { tr.dlg_target_cmd_hint } else { tr.dlg_target_url_hint });
    ui.headers_label.set_enabled(!stdio);
    ui.headers_box.set_enabled(!stdio);
    ui.headers_hint.set_enabled(!stdio);
}

/// Rebuilds the plain-language "what happens when you save" summary.
fn update_summary(ui: &DialogUi, tr: &T) {
    let mut parts = Vec::new();

    let name = ui.name_input.text().trim().to_string();
    if name.is_empty() {
        parts.push(tr.dlg_sum_need_name.to_string());
    } else {
        let head = if ui.editing { tr.dlg_sum_edit } else { tr.dlg_sum_add };
        parts.push(head.replace("%S", &name));
    }

    let dir = ui.dir_input.text().trim().to_string();
    parts.push(if is_checked(&ui.scope_user) {
        tr.dlg_sum_scope_user.to_string()
    } else if is_checked(&ui.scope_project) {
        if dir.is_empty() {
            tr.dlg_sum_scope_project_nodir.to_string()
        } else {
            tr.dlg_sum_scope_project.replace("%D", &dir)
        }
    } else if dir.is_empty() {
        tr.dlg_sum_scope_local_nodir.to_string()
    } else {
        tr.dlg_sum_scope_local.replace("%D", &dir)
    });

    let target = ui.target_input.text().trim().to_string();
    let stdio = is_checked(&ui.tr_stdio);
    parts.push(match (stdio, target.is_empty()) {
        (true, true) => tr.dlg_sum_stdio_notarget.to_string(),
        (true, false) => tr.dlg_sum_stdio.replace("%T", &target),
        (false, true) => tr.dlg_sum_remote_notarget.to_string(),
        (false, false) => tr.dlg_sum_remote.replace("%T", &target),
    });

    let env_count = ui.env_box.text().lines().filter(|line| !line.trim().is_empty()).count();
    if env_count > 0 {
        parts.push(tr.dlg_sum_env.replace("%N", &env_count.to_string()));
    }

    ui.summary.set_text(&parts.join(" "));
}

/// When editing, env values are intentionally not read back — warn before
/// saving keys with blank values. Returns false to stay in the dialog.
fn confirm_blank_env_values(ui: &DialogUi, lang: Lang, draft: &ServerDraft) -> bool {
    let blank_keys: Vec<&str> = draft
        .env
        .iter()
        .filter(|(_, value)| value.is_empty())
        .map(|(key, _)| key.as_str())
        .collect();
    if blank_keys.is_empty() {
        return true;
    }
    let tr = t(lang);
    let body = tr.dlg_env_blank_confirm_body.replace("%K", &blank_keys.join(", "));
    let choice = nwg::modal_message(&ui.window.handle, &nwg::MessageParams {
        title: tr.dlg_env_blank_confirm_title,
        content: &body,
        buttons: nwg::MessageButtons::YesNo,
        icons: nwg::MessageIcons::Warning,
    });
    choice == nwg::MessageChoice::Yes
}

fn send_outcome(shared: &Arc<Mutex<Shared>>, notify: &nwg::NoticeSender, outcome: Option<ServerDraft>) {
    let mut guard = shared.lock().unwrap();
    if guard.dialog.is_none() {
        guard.dialog = Some(outcome);
        drop(guard);
        notify.notice();
        nwg::stop_thread_dispatch();
    }
}

fn validate_name(ui: &DialogUi, tr: &'static T) -> Result<String, &'static str> {
    let name = ui.name_input.text().trim().to_string();
    let name_ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if name_ok {
        Ok(name)
    } else {
        Err(tr.dlg_err_name)
    }
}

fn validate_scope(ui: &DialogUi, tr: &'static T) -> Result<Scope, &'static str> {
    if is_checked(&ui.scope_user) {
        return Ok(Scope::User);
    }
    let dir = ui.dir_input.text().trim().to_string();
    if dir.is_empty() || !Path::new(&dir).is_dir() {
        return Err(tr.dlg_err_dir);
    }
    if is_checked(&ui.scope_project) {
        Ok(Scope::Project { project_dir: dir })
    } else {
        Ok(Scope::Local { project_dir: dir })
    }
}

fn build_draft(ui: &DialogUi, lang: Lang) -> Result<ServerDraft, &'static str> {
    let tr = t(lang);
    let name = validate_name(ui, tr)?;
    let scope = validate_scope(ui, tr)?;

    let transport = if is_checked(&ui.tr_stdio) {
        Transport::Stdio
    } else if is_checked(&ui.tr_http) {
        Transport::Http
    } else {
        Transport::Sse
    };

    let target_line = ui.target_input.text().trim().to_string();
    if target_line.is_empty() {
        return Err(tr.dlg_err_target);
    }
    let (target, args) = if transport == Transport::Stdio {
        let mut parts = split_command(&target_line);
        if parts.is_empty() {
            return Err(tr.dlg_err_target);
        }
        let program = parts.remove(0);
        (program, parts)
    } else {
        (target_line, Vec::new())
    };

    let mut env = Vec::new();
    for line in ui.env_box.text().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(tr.dlg_err_env);
        };
        env.push((key.trim().to_string(), value.trim().to_string()));
    }

    let mut headers = Vec::new();
    for line in ui.headers_box.text().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(tr.dlg_err_headers);
        };
        headers.push((key.trim().to_string(), value.trim().to_string()));
    }

    Ok(ServerDraft { name, scope, transport, target, args, env, headers })
}

fn join_command(program: &str, args: &[String]) -> String {
    let quote = |part: &str| {
        if part.contains(' ') {
            format!("\"{part}\"")
        } else {
            part.to_string()
        }
    };
    let mut parts = vec![quote(program)];
    parts.extend(args.iter().map(|a| quote(a)));
    parts.join(" ")
}
