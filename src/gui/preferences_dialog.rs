//! Preferences window: section sidebar on the left (currently only
//! "Interface"), settings panel on the right. Runs on its own thread with its
//! own event loop (nwg multithread dialog pattern) and reports the chosen
//! language through `Shared.preferences` + a Notice back to the main window.

use super::{apply_classic_button_theme, Shared};
use crate::i18n::{t, Lang};
use native_windows_gui as nwg;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const SIDEBAR_W: i32 = 150;
const PANEL_X: i32 = MARGIN + SIDEBAR_W + 14;
const BUTTON_W: i32 = 85;
const BUTTON_H: i32 = 26;

pub struct PreferencesParams {
    pub lang: Lang,
    pub shared: Arc<Mutex<Shared>>,
    pub notify: nwg::NoticeSender,
}

pub fn spawn(params: PreferencesParams) {
    std::thread::spawn(move || run_preferences(params));
}

fn run_preferences(params: PreferencesParams) {
    let tr = t(params.lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((560, 360))
        .position((400, 220))
        .title(tr.pref_title)
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .expect("preferences window");
    let (client_w, client_h) = window.size();
    let (client_w, client_h) = (client_w as i32, client_h as i32);
    let button_y = client_h - MARGIN - BUTTON_H;
    let sidebar_h = (button_y - 2 * MARGIN) as u32;

    let mut sections = nwg::ListBox::default();
    nwg::ListBox::builder()
        .parent(&window)
        .position((MARGIN, MARGIN))
        .size((SIDEBAR_W, sidebar_h as i32))
        .collection(vec![tr.pref_section_interface.to_string()])
        .build(&mut sections)
        .expect("sections");
    sections.set_selection(Some(0));

    let mut heading_font = nwg::Font::default();
    let _ = nwg::Font::builder()
        .family("Segoe UI")
        .size(16)
        .weight(700)
        .build(&mut heading_font);

    let mut heading = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.pref_section_interface)
        .position((PANEL_X, MARGIN))
        .size((client_w - PANEL_X - MARGIN, 22))
        .build(&mut heading)
        .expect("heading");
    heading.set_font(Some(&heading_font));

    let mut lang_label = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.lang_label)
        .position((PANEL_X, 56))
        .size((80, 20))
        .build(&mut lang_label)
        .expect("lang_label");

    let mut lang_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((PANEL_X + 88, 52))
        .size((210, 24))
        .collection(vec!["Português (BR)".to_string(), "English".to_string()])
        .selected_index(Some(match params.lang { Lang::PtBr => 0, Lang::En => 1 }))
        .build(&mut lang_combo)
        .expect("lang_combo");

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.pref_hint)
        .position((PANEL_X, 88))
        .size((client_w - PANEL_X - MARGIN, 20))
        .build(&mut hint)
        .expect("hint");

    let cancel_x = client_w - MARGIN - BUTTON_W;
    let ok_x = cancel_x - 8 - BUTTON_W;

    let mut ok_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.pref_ok)
        .position((ok_x, button_y))
        .size((BUTTON_W, BUTTON_H))
        .build(&mut ok_btn)
        .expect("ok_btn");
    apply_classic_button_theme(&ok_btn);

    let mut cancel_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.dlg_cancel)
        .position((cancel_x, button_y))
        .size((BUTTON_W, BUTTON_H))
        .build(&mut cancel_btn)
        .expect("cancel_btn");
    apply_classic_button_theme(&cancel_btn);

    let window_handle = window.handle;
    let ok_handle = ok_btn.handle;
    let cancel_handle = cancel_btn.handle;
    let combo_handle = lang_combo.handle;
    let shared = Arc::clone(&params.shared);
    let notify = params.notify;
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == window_handle => {
                send_outcome(&shared, &notify, None);
            }
            E::OnButtonClick if handle == cancel_handle => {
                send_outcome(&shared, &notify, None);
                window.close();
            }
            E::OnButtonClick if handle == ok_handle => {
                let chosen = match combo_selection(&combo_handle) {
                    Some(1) => Lang::En,
                    _ => Lang::PtBr,
                };
                send_outcome(&shared, &notify, Some(chosen));
                window.close();
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

/// Reads the combo selection through the raw handle — only the `Copy` handle
/// is moved into the event closure, keeping the control's ownership simple.
fn combo_selection(handle: &nwg::ControlHandle) -> Option<usize> {
    use winapi::um::winuser::{SendMessageW, CB_GETCURSEL};
    let hwnd = handle.hwnd()?;
    let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

fn send_outcome(shared: &Arc<Mutex<Shared>>, notify: &nwg::NoticeSender, outcome: Option<Lang>) {
    let mut guard = shared.lock().unwrap();
    if guard.preferences.is_none() {
        guard.preferences = Some(outcome);
        drop(guard);
        notify.notice();
        nwg::stop_thread_dispatch();
    }
}
