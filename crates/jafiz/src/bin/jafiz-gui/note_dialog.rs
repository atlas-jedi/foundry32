//! "Observação" — the modal that collects what actually happened on a step, so
//! the verdict lands in the report with the tester's own words attached.
//!
//! Same shape as `run_dialog` (its own thread, its own event loop, the answer
//! delivered through `Shared` + a `Notice` exactly once), but over a multi-line
//! `TextBox`: a note is usually a sentence or three, and a one-line field would
//! make the tester type into a keyhole.

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
const WINDOW_W: i32 = 520;
const WINDOW_H: i32 = 300;
const VK_ESCAPE: u32 = 0x1B;

/// What the main window hands the dialog thread.
pub struct NoteParams {
    pub lang: Lang,
    /// The note already recorded on the step. Editing a note must be editing,
    /// not retyping — and re-marking a step keeps its note, so the tester will
    /// come back to this text.
    pub note: String,
    /// `note_dlg_title` with the scenario and step already substituted: only
    /// the caller knows which step is being annotated.
    pub title: String,
    /// The mailbox the answer is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the answer is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the answer comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: NoteParams) {
    std::thread::spawn(move || run_dialog(params));
}

fn run_dialog(params: NoteParams) {
    let NoteParams { lang, note, title, shared, notify } = params;
    let tr = t(lang);

    // See `run_dialog` — the one-Notice flag, armed before the first `.expect`
    // so `DialogGuard` can hand the main window back if construction panics.
    let sent = Rc::new(Cell::new(false));
    let _guard = DialogGuard::new(Rc::clone(&sent), DialogSlot::Note, Arc::clone(&shared), notify);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((400, 240))
        .title(&title)
        // Born hidden, shown once the controls exist — see `run_dialog`.
        .flags(nwg::WindowFlags::WINDOW)
        .build(&mut window)
        .expect("note dialog window");

    let field_w = WINDOW_W - 2 * MARGIN;
    let button_y = WINDOW_H - MARGIN - BUTTON_H;

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.note_dlg_hint)
        .position((MARGIN, MARGIN))
        .size((field_w, 20))
        .build(&mut hint)
        .expect("note hint");

    let box_y = MARGIN + 26;
    let mut input = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        .text(&note)
        .position((MARGIN, box_y))
        .size((field_w, button_y - box_y - MARGIN))
        // Deliberately without HSCROLL/AUTOHSCROLL, which nwg's default flags
        // include: without them the edit control wraps, and a note is prose,
        // not a line of code. VSCROLL keeps a long note reachable, TAB_STOP
        // keeps Tab moving to the buttons instead of typing a tab.
        .flags(
            nwg::TextBoxFlags::VISIBLE
                | nwg::TextBoxFlags::VSCROLL
                | nwg::TextBoxFlags::AUTOVSCROLL
                | nwg::TextBoxFlags::TAB_STOP,
        )
        .build(&mut input)
        .expect("note input");

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
    // Caret at the end of the existing note, not over it: Enter is a newline
    // in this control, so a select-all would be one keystroke away from
    // wiping what the tester wrote earlier.
    input.set_focus();
    let end = input.len();
    input.set_selection(end..end);

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
            // No Enter here: the control is ES_MULTILINE | ES_WANTRETURN, so
            // Enter is a newline and OK is a click or Tab+Space. Escape
            // reaches this handler only when the focus is on a button — nwg
            // drops `VK_ESCAPE` before dispatch for any "Edit"-class control
            // (`is_textbox_control` in native-windows-gui's win32/window.rs).
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_ESCAPE) = evt_data {
                    send_outcome(&sent, &shared, &notify, None);
                    window.close();
                }
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

/// Publishes the answer and wakes the main window — at most once per dialog.
/// See `run_dialog::send_outcome` for why the guard matters.
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
    shared.lock().unwrap().note = Some(outcome);
    notify.notice();
    nwg::stop_thread_dispatch();
}
