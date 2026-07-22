//! Add/Edit dialog. Runs on its own thread with its own window + event loop
//! (nwg multithread dialog pattern) and reports the outcome through
//! `Shared.dialog` + a Notice back to the main window.

use super::Shared;
use crate::i18n::{t, Lang};
use crate::model::{Scope, Transport};
use crate::mutation::{split_command, ServerDraft};
use native_windows_gui as nwg;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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
    name_input: nwg::TextInput,
    scope_combo: nwg::ComboBox<String>,
    known_combo: nwg::ComboBox<String>,
    dir_input: nwg::TextInput,
    transport_combo: nwg::ComboBox<String>,
    target_label: nwg::Label,
    target_input: nwg::TextInput,
    env_box: nwg::TextBox,
    headers_box: nwg::TextBox,
    ok_btn: nwg::Button,
    cancel_btn: nwg::Button,
}

fn run_dialog(params: DialogParams) {
    let tr = t(params.lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((540, 560))
        .position((360, 160))
        .title(if params.editing { tr.dlg_title_edit } else { tr.dlg_title_add })
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .expect("dialog window");

    let label = |text: &str, x: i32, y: i32, w: i32| {
        let mut control = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .text(text)
            .position((x, y))
            .size((w, 20))
            .build(&mut control)
            .expect("label");
        control
    };
    let input = |x: i32, y: i32, w: i32| {
        let mut control = nwg::TextInput::default();
        nwg::TextInput::builder()
            .parent(&window)
            .position((x, y))
            .size((w, 24))
            .build(&mut control)
            .expect("input");
        control
    };

    label(tr.dlg_name, 12, 16, 160);
    let name_input = input(180, 12, 340);

    label(tr.dlg_scope, 12, 48, 160);
    let mut scope_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((180, 44))
        .size((340, 24))
        .collection(vec![
            tr.scope_user.to_string(),
            tr.scope_project.to_string(),
            tr.scope_local.to_string(),
        ])
        .selected_index(Some(0))
        .build(&mut scope_combo)
        .expect("scope_combo");

    label(tr.dlg_known_dirs, 12, 80, 160);
    let mut known_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((180, 76))
        .size((340, 24))
        .collection(params.known_dirs.clone())
        .build(&mut known_combo)
        .expect("known_combo");

    label(tr.dlg_dir, 12, 112, 160);
    let dir_input = input(180, 108, 340);

    label(tr.dlg_transport, 12, 144, 160);
    let mut transport_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((180, 140))
        .size((340, 24))
        .collection(vec!["stdio".to_string(), "HTTP".to_string(), "SSE".to_string()])
        .selected_index(Some(0))
        .build(&mut transport_combo)
        .expect("transport_combo");

    let target_label = label(tr.dlg_target_cmd, 12, 176, 160);
    let target_input = input(180, 172, 340);

    label(tr.dlg_env, 12, 208, 160);
    let mut env_box = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        .position((180, 204))
        .size((340, 90))
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL)
        .build(&mut env_box)
        .expect("env_box");
    label(tr.dlg_env_hint, 180, 298, 340);

    label(tr.dlg_headers, 12, 324, 160);
    let mut headers_box = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        .position((180, 320))
        .size((340, 70))
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL)
        .build(&mut headers_box)
        .expect("headers_box");

    if params.editing {
        label(tr.dlg_env_edit_note, 12, 402, 508);
    }

    let mut ok_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.dlg_ok)
        .position((310, 470))
        .size((100, 30))
        .build(&mut ok_btn)
        .expect("ok_btn");
    let mut cancel_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.dlg_cancel)
        .position((420, 470))
        .size((100, 30))
        .build(&mut cancel_btn)
        .expect("cancel_btn");

    let ui = DialogUi {
        window,
        name_input,
        scope_combo,
        known_combo,
        dir_input,
        transport_combo,
        target_label,
        target_input,
        env_box,
        headers_box,
        ok_btn,
        cancel_btn,
    };

    apply_prefill(&ui, &params);
    sync_field_states(&ui, tr);

    // `ui` must be moved whole into the handler below (it is passed by reference
    // to helpers like `ui_handle_window`/`build_draft`), but the same call also
    // borrows `ui.window.handle` for the first argument. Rc lets both live
    // together: `window_handle` is copied out (`ControlHandle` is `Copy`) so it
    // has no lingering borrow on `ui`, then `ui` is reshadowed as a fresh clone
    // that is what actually gets moved into the `Fn` closure below — the closure
    // body keeps referring to an owned `ui`, unchanged from the brief
    // (fix-forward per brief note: wrap `ui` in `Rc`).
    let ui = Rc::new(ui);
    let window_handle = ui.window.handle;
    let ui = Rc::clone(&ui);
    let shared = Arc::clone(&params.shared);
    let notify = params.notify;
    let lang = params.lang;
    let editing = params.editing;
    let known_dirs = params.known_dirs.clone();
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == ui_handle_window(&ui) => {
                send_outcome(&shared, &notify, None);
            }
            E::OnButtonClick if handle == ui.cancel_btn.handle => {
                send_outcome(&shared, &notify, None);
                ui.window.close();
            }
            E::OnButtonClick if handle == ui.ok_btn.handle => {
                match build_draft(&ui, lang) {
                    Ok(draft) => {
                        if editing {
                            let blank_keys: Vec<&str> = draft
                                .env
                                .iter()
                                .filter(|(_, value)| value.is_empty())
                                .map(|(key, _)| key.as_str())
                                .collect();
                            if !blank_keys.is_empty() {
                                let tr = t(lang);
                                let body = tr.dlg_env_blank_confirm_body.replace("%K", &blank_keys.join(", "));
                                let choice = nwg::modal_message(&ui.window.handle, &nwg::MessageParams {
                                    title: tr.dlg_env_blank_confirm_title,
                                    content: &body,
                                    buttons: nwg::MessageButtons::YesNo,
                                    icons: nwg::MessageIcons::Warning,
                                });
                                if choice != nwg::MessageChoice::Yes {
                                    return; // stay in the dialog
                                }
                            }
                        }
                        send_outcome(&shared, &notify, Some(draft));
                        ui.window.close();
                    }
                    Err(message) => {
                        nwg::modal_error_message(&ui.window.handle, t(lang).dlg_err_title, message);
                    }
                }
            }
            E::OnComboxBoxSelection if handle == ui.known_combo.handle => {
                if let Some(index) = ui.known_combo.selection() {
                    if let Some(dir) = known_dirs.get(index) {
                        ui.dir_input.set_text(dir);
                    }
                }
            }
            E::OnComboxBoxSelection
                if handle == ui.scope_combo.handle || handle == ui.transport_combo.handle =>
            {
                sync_field_states(&ui, t(lang));
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

// The closure moves `ui` in; this helper keeps the OnWindowClose guard readable.
fn ui_handle_window(ui: &DialogUi) -> nwg::ControlHandle {
    ui.window.handle
}

fn apply_prefill(ui: &DialogUi, params: &DialogParams) {
    let Some(draft) = &params.prefill else {
        if let Some(first) = params.known_dirs.first() {
            ui.dir_input.set_text(first);
        }
        return;
    };
    ui.name_input.set_text(&draft.name);
    let scope_index = match draft.scope {
        Scope::User => 0,
        Scope::Project { .. } => 1,
        _ => 2,
    };
    ui.scope_combo.set_selection(Some(scope_index));
    if let Some(dir) = draft.scope.project_dir() {
        ui.dir_input.set_text(dir);
    }
    let transport_index = match draft.transport {
        Transport::Stdio => 0,
        Transport::Http => 1,
        Transport::Sse => 2,
    };
    ui.transport_combo.set_selection(Some(transport_index));
    ui.target_input.set_text(&join_command(&draft.target, &draft.args));
    let env_lines: Vec<String> = draft.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    ui.env_box.set_text(&env_lines.join("\r\n"));
}

fn sync_field_states(ui: &DialogUi, tr: &crate::i18n::T) {
    let scope_needs_dir = ui.scope_combo.selection().unwrap_or(0) != 0;
    ui.dir_input.set_enabled(scope_needs_dir);
    ui.known_combo.set_enabled(scope_needs_dir);

    let transport = ui.transport_combo.selection().unwrap_or(0);
    ui.target_label.set_text(if transport == 0 { tr.dlg_target_cmd } else { tr.dlg_target_url });
    ui.headers_box.set_enabled(transport != 0);
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

fn build_draft(ui: &DialogUi, lang: Lang) -> Result<ServerDraft, &'static str> {
    let tr = t(lang);

    let name = ui.name_input.text().trim().to_string();
    let name_ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !name_ok {
        return Err(tr.dlg_err_name);
    }

    let scope = match ui.scope_combo.selection().unwrap_or(0) {
        0 => Scope::User,
        selection => {
            let dir = ui.dir_input.text().trim().to_string();
            if dir.is_empty() || !Path::new(&dir).is_dir() {
                return Err(tr.dlg_err_dir);
            }
            if selection == 1 {
                Scope::Project { project_dir: dir }
            } else {
                Scope::Local { project_dir: dir }
            }
        }
    };

    let transport = match ui.transport_combo.selection().unwrap_or(0) {
        1 => Transport::Http,
        2 => Transport::Sse,
        _ => Transport::Stdio,
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
