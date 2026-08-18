//! "Nova execução" / "Ambiente" — the modal that asks for the environment or
//! build string that heads the report (URL, version, browser, test user).
//!
//! Its own thread with its own event loop: the nwg multithread dialog pattern
//! (see `crates/mcp-console/src/gui/preferences_dialog.rs`). The dialog never
//! touches the main window's state — it leaves its answer in `Shared` and
//! wakes the main window with a `Notice`, exactly once.

use crate::i18n::{t, Lang};
use crate::{DialogGuard, DialogSlot, Shared};
use foundry_common::theme::apply_classic_button_theme;
use native_windows_gui as nwg;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const BUTTON_W: i32 = 92;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 8;
const WINDOW_W: i32 = 470;
const WINDOW_H: i32 = 176;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;

/// What the main window hands the dialog thread.
pub struct RunParams {
    pub lang: Lang,
    /// Pre-filled when editing an existing run's environment, and seeded from
    /// the last run when starting a new one: a tester usually retests the same
    /// build and should not have to retype it.
    pub environment: String,
    /// Window caption. One dialog serves both "start a run" and "re-describe
    /// the open run's environment", and only the caller knows which it asked
    /// for.
    pub title: &'static str,
    /// The mailbox the answer is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the answer is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the answer comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: RunParams) {
    std::thread::spawn(move || run_dialog(params));
}

fn run_dialog(params: RunParams) {
    // Destructured up front so the builders below can borrow `title` and
    // `environment` while `shared`/`notify` move into the event closure.
    let RunParams { lang, environment, title, shared, notify } = params;
    let tr = t(lang);

    // The one-Notice guard, armed before the first `.expect` below. It is a
    // thread-local `Cell` and not the mailbox slot: the UI thread `take()`s
    // that slot the moment it is woken, so a second event arriving after the
    // drain would find it empty and send again. `DialogGuard` shares this very
    // flag so that a panic during construction still hands the main window
    // back — and so that it stays quiet once the dialog has answered.
    let sent = Rc::new(Cell::new(false));
    let _guard =
        DialogGuard::new(Rc::clone(&sent), DialogSlot::Environment, Arc::clone(&shared), notify);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((430, 280))
        .title(title)
        // Born hidden and shown once every control exists: a window that is
        // visible from creation paints empty for a frame, and anything that
        // reacts to it appearing — a screen reader, an automation script —
        // can look inside before there is anything there.
        .flags(nwg::WindowFlags::WINDOW)
        .build(&mut window)
        .expect("run dialog window");

    let field_w = WINDOW_W - 2 * MARGIN;

    let mut label = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.run_dlg_env)
        .position((MARGIN, MARGIN))
        .size((field_w, 20))
        .build(&mut label)
        .expect("env label");

    let mut input = nwg::TextInput::default();
    nwg::TextInput::builder()
        .parent(&window)
        .text(&environment)
        .position((MARGIN, MARGIN + 22))
        .size((field_w, 24))
        .build(&mut input)
        .expect("env input");

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.run_dlg_hint)
        .position((MARGIN, MARGIN + 56))
        .size((field_w, 36))
        .build(&mut hint)
        .expect("env hint");

    let button_y = WINDOW_H - MARGIN - BUTTON_H;
    let cancel_x = WINDOW_W - MARGIN - BUTTON_W;
    let ok_x = cancel_x - BUTTON_GAP - BUTTON_W;

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
    // Focused and fully selected: the two likely answers are "same build as
    // last time" (press Enter) and "a different one" (type straight over it).
    input.set_focus();
    input.set_selection(0..input.len());

    let window_handle = window.handle;
    let ok_handle = ok_btn.handle;
    let cancel_handle = cancel_btn.handle;
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, evt_data, handle| {
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
                send_outcome(&sent, &shared, &notify, Some(input.text()));
                window.close();
            }
            // A one-line field, so Enter is the answer — measured working with
            // the caret in the field. Escape only reaches here when the focus
            // is on a button: nwg drops `VK_ESCAPE` before dispatch for any
            // "Edit"-class control on purpose (`is_textbox_control` in
            // native-windows-gui's win32/window.rs), so Cancel and the title
            // bar's X are the reliable ways out.
            E::OnKeyRelease => match evt_data {
                nwg::EventData::OnKey(VK_RETURN) => {
                    send_outcome(&sent, &shared, &notify, Some(input.text()));
                    window.close();
                }
                nwg::EventData::OnKey(VK_ESCAPE) => {
                    send_outcome(&sent, &shared, &notify, None);
                    window.close();
                }
                _ => {}
            },
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

/// Publishes the answer and wakes the main window — at most once per dialog.
///
/// Answering with OK closes the window, which raises `OnWindowClose` right
/// behind the click; the guard is what turns that into a no-op. It matters:
/// the main window re-enables itself on every `OnNotice`, so a second ping
/// would unlock it while this modal is still on screen.
fn send_outcome(
    sent: &Cell<bool>,
    shared: &Arc<Mutex<Shared>>,
    notify: &nwg::NoticeSender,
    outcome: Option<String>,
) {
    if sent.replace(true) {
        return;
    }
    // `Some(None)` is the mailbox's spelling of "cancelled".
    shared.lock().unwrap().environment = Some(outcome);
    notify.notice();
    nwg::stop_thread_dispatch();
}
