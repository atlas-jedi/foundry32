//! "Preferências" — section sidebar on the left (currently only "Interface"),
//! settings panel on the right.
//!
//! Adapted from `crates/mcp-console/src/gui/preferences_dialog.rs`, which is
//! the window this workspace's tools share; only the string table it reads and
//! the mailbox slot it answers through are JAFIZ's own. The language change is
//! applied in place, the way MCP Console applies it — the main window
//! re-captions itself in `relabel_all` rather than asking for a restart, which
//! for JAFIZ would mean abandoning the run being recorded.

use crate::i18n::{t, Lang};
use crate::{DialogGuard, DialogSlot, Shared};
use foundry_common::theme::apply_classic_button_theme;
use native_windows_gui as nwg;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const SIDEBAR_W: i32 = 150;
const PANEL_X: i32 = MARGIN + SIDEBAR_W + 14;
const BUTTON_W: i32 = 85;
const BUTTON_H: i32 = 26;
const WINDOW_W: i32 = 560;
const WINDOW_H: i32 = 360;

/// What the main window hands the dialog thread.
pub struct PreferencesParams {
    /// The language currently in force — what the combo starts on.
    pub lang: Lang,
    /// The mailbox the answer is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the answer is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the answer comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: PreferencesParams) {
    std::thread::spawn(move || run_preferences(params));
}

fn run_preferences(params: PreferencesParams) {
    let PreferencesParams { lang, shared, notify } = params;
    let tr = t(lang);

    // Armed before the first `.expect` below — see `DialogGuard`.
    let sent = Rc::new(Cell::new(false));
    let _guard =
        DialogGuard::new(Rc::clone(&sent), DialogSlot::Preferences, Arc::clone(&shared), notify);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((400, 220))
        .title(tr.pref_title)
        // Born hidden, shown once the controls exist — see `run_dialog`.
        .flags(nwg::WindowFlags::WINDOW)
        .build(&mut window)
        .expect("preferences window");
    let button_y = WINDOW_H - MARGIN - BUTTON_H;
    let sidebar_h = button_y - 2 * MARGIN;

    let mut sections = nwg::ListBox::default();
    nwg::ListBox::builder()
        .parent(&window)
        .position((MARGIN, MARGIN))
        .size((SIDEBAR_W, sidebar_h))
        .collection(vec![tr.pref_section_interface.to_string()])
        .build(&mut sections)
        .expect("sections");
    sections.set_selection(Some(0));

    let mut heading_font = nwg::Font::default();
    let _ = nwg::Font::builder().family("Segoe UI").size(16).weight(700).build(&mut heading_font);

    let mut heading = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.pref_section_interface)
        .position((PANEL_X, MARGIN))
        .size((WINDOW_W - PANEL_X - MARGIN, 22))
        .build(&mut heading)
        .expect("heading");
    heading.set_font(Some(&heading_font));

    let mut lang_label = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.pref_lang)
        .position((PANEL_X, 56))
        .size((80, 20))
        .build(&mut lang_label)
        .expect("lang_label");

    let mut lang_combo = nwg::ComboBox::default();
    nwg::ComboBox::builder()
        .parent(&window)
        .position((PANEL_X + 88, 52))
        .size((210, 24))
        // Each language names itself: someone who has landed in the wrong one
        // has to recognise their own, not read the current one's word for it.
        .collection(vec!["Português (BR)".to_string(), "English".to_string()])
        .selected_index(Some(match lang {
            Lang::PtBr => 0,
            Lang::En => 1,
        }))
        .build(&mut lang_combo)
        .expect("lang_combo");

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.pref_hint)
        .position((PANEL_X, 88))
        .size((WINDOW_W - PANEL_X - MARGIN, 20))
        .build(&mut hint)
        .expect("hint");

    let cancel_x = WINDOW_W - MARGIN - BUTTON_W;
    let ok_x = cancel_x - 8 - BUTTON_W;

    let mut ok_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.dlg_ok)
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

    window.set_visible(true);

    let window_handle = window.handle;
    let ok_handle = ok_btn.handle;
    let cancel_handle = cancel_btn.handle;
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == window_handle => {
                send_outcome(&sent, &shared, &notify, None);
            }
            E::OnButtonClick if handle == cancel_handle => {
                send_outcome(&sent, &shared, &notify, None);
                window.close();
            }
            E::OnButtonClick if handle == ok_handle => {
                let chosen = match lang_combo.selection() {
                    Some(1) => Lang::En,
                    _ => Lang::PtBr,
                };
                send_outcome(&sent, &shared, &notify, Some(chosen));
                window.close();
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

/// Publishes the chosen language and wakes the main window — at most once per
/// dialog. See `run_dialog::send_outcome` for why the guard matters.
fn send_outcome(
    sent: &Cell<bool>,
    shared: &Arc<Mutex<Shared>>,
    notify: &nwg::NoticeSender,
    outcome: Option<Lang>,
) {
    if sent.replace(true) {
        return;
    }
    // `Some(None)` is the mailbox's spelling of "cancelled".
    shared.lock().unwrap().preferences = Some(outcome);
    notify.notice();
    nwg::stop_thread_dispatch();
}
