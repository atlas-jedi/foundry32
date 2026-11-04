//! "Editar anotação" — the modal that changes an entry's task, text, time or
//! description. The description lives here and nowhere else on screen: the
//! list and the stand-up report show the one-line text alone.
//!
//! Same shape as JAFIZ's note dialog: its own thread, its own event loop, the
//! answer delivered through `Shared` + a `Notice` exactly once. Enter is OK and
//! Esc is Cancel — nwg's message loop runs `IsDialogMessage`, which turns them
//! into IDOK/IDCANCEL on this window. Inside the description box Enter is a
//! line break instead (it is ES_MULTILINE | ES_WANTRETURN), and Esc needs the
//! hook in `cancel_on_escape`.

use crate::i18n::{t, Lang};
use crate::{DialogGuard, DialogSlot, Shared};
use foundry_common::theme::apply_classic_button_theme;
use native_windows_gui as nwg;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const LABEL_W: i32 = 86;
const ROW_H: i32 = 24;
const ROW_GAP: i32 = 10;
const BUTTON_W: i32 = 92;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 8;
const TIME_W: i32 = 70;
const WINDOW_W: i32 = 620;
const WINDOW_H: i32 = 330;
/// Raw-subclass id of the description box's Esc hook (nwg reserves <= 0xFFFF).
const ESC_HANDLER_ID: usize = 0x0D41_1E5C;

/// What the dialog answers with: every field as it was left.
pub struct EntryEdit {
    pub task: String,
    pub text: String,
    pub time: String,
    /// As the edit control hands it back, `\r\n` and all.
    pub description: String,
}

/// What the main window hands the dialog thread.
pub struct EntryParams {
    pub lang: Lang,
    pub task: String,
    pub text: String,
    pub time: String,
    /// `\n` line breaks, as stored.
    pub description: String,
    pub shared: Arc<Mutex<Shared>>,
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the answer comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: EntryParams) {
    std::thread::spawn(move || run_dialog(params));
}

fn run_dialog(params: EntryParams) {
    let EntryParams { lang, task, text, time, description, shared, notify } = params;
    let tr = t(lang);

    // Armed before the first `.expect` so `DialogGuard` can hand the main
    // window back if construction panics.
    let sent = Rc::new(Cell::new(false));
    let _guard = DialogGuard::new(Rc::clone(&sent), DialogSlot::Edit, Arc::clone(&shared), notify);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((420, 260))
        .title(tr.edit_title)
        // Born hidden, shown once the controls exist.
        .flags(nwg::WindowFlags::WINDOW)
        .build(&mut window)
        .expect("entry dialog window");

    let field_x = MARGIN + LABEL_W;
    let field_w = WINDOW_W - field_x - MARGIN;
    let row = |index: i32| MARGIN + index * (ROW_H + ROW_GAP);

    let label = |caption: &str, y: i32, window: &nwg::Window| {
        let mut control = nwg::Label::default();
        nwg::Label::builder()
            .parent(window)
            .text(caption)
            .position((MARGIN, y + 4))
            .size((LABEL_W, 20))
            .build(&mut control)
            .expect("label");
        control
    };
    let input = |value: &str, y: i32, width: i32, window: &nwg::Window| {
        let mut control = nwg::TextInput::default();
        nwg::TextInput::builder()
            .parent(window)
            .text(value)
            .position((field_x, y))
            .size((width, ROW_H))
            .build(&mut control)
            .expect("input");
        control
    };

    let _task_label = label(tr.lbl_task, row(0), &window);
    let task_input = input(&task, row(0), field_w, &window);
    let _text_label = label(tr.lbl_text, row(1), &window);
    let text_input = input(&text, row(1), field_w, &window);
    let _time_label = label(tr.dlg_time, row(2), &window);
    let time_input = input(&time, row(2), TIME_W, &window);

    let button_y = WINDOW_H - MARGIN - BUTTON_H;
    let _desc_label = label(tr.dlg_description, row(3), &window);
    let mut desc_input = nwg::TextBox::default();
    nwg::TextBox::builder()
        .parent(&window)
        // The edit control breaks lines on `\r\n` only.
        .text(&description.replace('\n', "\r\n"))
        .position((field_x, row(3)))
        .size((field_w, button_y - MARGIN - row(3)))
        // Without nwg's default HSCROLL/AUTOHSCROLL, so the text wraps — a
        // description is prose. TAB_STOP keeps Tab moving on to the buttons.
        .flags(
            nwg::TextBoxFlags::VISIBLE
                | nwg::TextBoxFlags::VSCROLL
                | nwg::TextBoxFlags::AUTOVSCROLL
                | nwg::TextBoxFlags::TAB_STOP,
        )
        .build(&mut desc_input)
        .expect("description input");
    let cancel_x = WINDOW_W - MARGIN - BUTTON_W;
    let ok_x = cancel_x - BUTTON_GAP - BUTTON_W;
    let button = |caption: &str, x: i32, window: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder()
            .parent(window)
            .text(caption)
            .position((x, button_y))
            .size((BUTTON_W, BUTTON_H))
            .build(&mut control)
            .expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let ok_btn = button(tr.dlg_ok, ok_x, &window);
    let cancel_btn = button(tr.dlg_cancel, cancel_x, &window);

    window.set_visible(true);
    // The text is what gets corrected most; the caret goes to its end.
    text_input.set_focus();
    let end = text_input.len();
    text_input.set_selection(end..end);

    let window_handle = window.handle;
    let ok_handle = ok_btn.handle;
    let cancel_handle = cancel_btn.handle;
    let esc_hook = cancel_on_escape(&desc_input, &window);
    let answer = move || EntryEdit {
        task: task_input.text(),
        text: text_input.text(),
        time: time_input.text(),
        description: desc_input.text(),
    };
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == window_handle => send_outcome(&sent, &shared, &notify, None),
            E::OnButtonClick if handle == cancel_handle => {
                send_outcome(&sent, &shared, &notify, None);
                window.close();
            }
            E::OnKeyEsc => {
                send_outcome(&sent, &shared, &notify, None);
                window.close();
            }
            E::OnButtonClick if handle == ok_handle => {
                send_outcome(&sent, &shared, &notify, Some(answer()));
                window.close();
            }
            E::OnKeyEnter => {
                send_outcome(&sent, &shared, &notify, Some(answer()));
                window.close();
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    // The hook first: the description box still exists until the handler
    // below goes, taking the controls it captured along with it.
    if let Some(hook) = esc_hook {
        let _ = nwg::unbind_raw_event_handler(&hook);
    }
    nwg::unbind_event_handler(&handler);
}

/// Makes Esc in the description box cancel, as it does in the other fields.
/// A multi-line edit control wants every key, so `IsDialogMessage` hands it
/// the Esc instead of turning it into IDCANCEL — and nwg then drops Esc for any
/// "Edit"-class control before an event is raised. This subclass runs ahead of
/// nwg's and posts the IDCANCEL itself, which reaches the handler as
/// `OnKeyEsc`. `None` only if the hook could not be installed: Esc then simply
/// does nothing there, and Cancel still works.
fn cancel_on_escape(desc_input: &nwg::TextBox, window: &nwg::Window) -> Option<nwg::RawEventHandler> {
    use winapi::shared::minwindef::WPARAM;
    use winapi::um::winuser::{PostMessageW, IDCANCEL, VK_ESCAPE, WM_CHAR, WM_COMMAND, WM_KEYDOWN};

    let dialog = window.handle.hwnd()?;
    nwg::bind_raw_event_handler(&desc_input.handle, ESC_HANDLER_ID, move |_hwnd, msg, wparam, _lparam| {
        match msg {
            WM_KEYDOWN if wparam == VK_ESCAPE as WPARAM => {
                // SAFETY: `dialog` is this thread's dialog window, alive for
                // as long as the control it parents — the only one that can
                // receive the message this runs on.
                unsafe { PostMessageW(dialog, WM_COMMAND, IDCANCEL as WPARAM, 0) };
                Some(0)
            }
            // The character the key press would also type: swallowed, or the
            // control beeps on its way out.
            WM_CHAR if wparam == VK_ESCAPE as WPARAM => Some(0),
            _ => None,
        }
    })
    .ok()
}

/// Publishes the answer and wakes the main window — at most once per dialog.
fn send_outcome(
    sent: &Cell<bool>,
    shared: &Arc<Mutex<Shared>>,
    notify: &nwg::NoticeSender,
    outcome: Option<EntryEdit>,
) {
    if sent.replace(true) {
        return;
    }
    // `Some(None)` is the mailbox's spelling of "cancelled".
    shared.lock().unwrap().edit = Some(outcome);
    notify.notice();
    nwg::stop_thread_dispatch();
}
