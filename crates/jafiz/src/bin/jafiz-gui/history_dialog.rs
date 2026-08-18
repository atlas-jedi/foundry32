//! "Histórico" — every run recorded against the open suite, so a tester can
//! read what an earlier pass found without leaving the window or opening the
//! JSON by hand.
//!
//! Same shape as the other dialogs (its own thread, its own event loop, the
//! answer delivered through `Shared` + a `Notice` exactly once) over a report
//! `ListView`. The rows arrive already rendered: computing a run's progress
//! needs the suite, and the dialog thread has no business borrowing the main
//! window's state.
//!
//! The answer is only ever *which run to display*. Recording keeps going to the
//! active run whatever this dialog returns — see `UiState::viewed_run`.

use crate::i18n::{t, Lang};
use crate::{DialogGuard, DialogSlot, Shared};
use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::insert_report_list_view_column;
use native_windows_gui as nwg;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const BUTTON_W: i32 = 92;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 8;
const WINDOW_W: i32 = 760;
const WINDOW_H: i32 = 400;
const HINT_H: i32 = 20;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;

/// One run as the list shows it, rendered on the UI thread.
pub struct HistoryRow {
    /// The run id the answer is given in terms of. Kept beside the cells rather
    /// than read back out of column 0: cross-process list-view text reads are
    /// unreliable, and the id is what the main window actually needs.
    pub id: String,
    /// Id, started, finished, environment, progress.
    pub cells: [String; 5],
}

/// What the main window hands the dialog thread.
pub struct HistoryParams {
    pub lang: Lang,
    /// Newest first — the run someone goes looking for is nearly always recent.
    pub rows: Vec<HistoryRow>,
    /// Row to start on: whichever run the window is already showing.
    pub selected: Option<usize>,
    /// The mailbox the answer is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the answer is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the answer comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: HistoryParams) {
    std::thread::spawn(move || run_dialog(params));
}

fn run_dialog(params: HistoryParams) {
    let HistoryParams { lang, rows, selected, shared, notify } = params;
    let tr = t(lang);

    // Armed before the first `.expect` below: a panic while the window is being
    // built would otherwise leave the main window disabled with nothing to
    // re-enable it. See `DialogGuard`.
    let sent = Rc::new(Cell::new(false));
    let _guard =
        DialogGuard::new(Rc::clone(&sent), DialogSlot::History, Arc::clone(&shared), notify);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((WINDOW_W, WINDOW_H))
        .position((320, 200))
        // Born hidden, shown once the controls exist — see `run_dialog`.
        .flags(nwg::WindowFlags::WINDOW)
        .title(tr.menu_run_history.trim_end_matches('…'))
        .build(&mut window)
        .expect("history dialog window");

    let field_w = WINDOW_W - 2 * MARGIN;
    let button_y = WINDOW_H - MARGIN - BUTTON_H;

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text(tr.hist_hint)
        .position((MARGIN, MARGIN))
        .size((field_w, HINT_H))
        .build(&mut hint)
        .expect("history hint");

    let list_y = MARGIN + HINT_H + 6;
    let mut list = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .position((MARGIN, list_y))
        .size((field_w, button_y - list_y - MARGIN))
        .build(&mut list)
        .expect("history list");
    // nwg forces LVS_NOCOLUMNHEADER at creation for backward compatibility.
    list.set_headers_enabled(true);
    apply_explorer_theme(&list.handle);
    for (index, width, title) in [
        (0, 132, tr.hist_col_run),
        (1, 148, tr.hist_col_started),
        (2, 148, tr.hist_col_finished),
        (3, 240, tr.hist_col_env),
        (4, 60, tr.hist_col_progress),
    ] {
        insert_report_list_view_column(&list, index, width, title);
    }
    for row in &rows {
        list.insert_items_row(None, &row.cells);
    }
    if let Some(row) = selected {
        list.select_item(row, true);
        crate::ensure_visible(&list, row);
    }

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
    // Focus on the list, not on OK: the whole interaction is "pick a row", and
    // the arrow keys should work the moment the window appears.
    list.set_focus();

    let window_handle = window.handle;
    let list_handle = list.handle;
    let ok_handle = ok_btn.handle;
    let cancel_handle = cancel_btn.handle;
    // The ids in list order, so a row index can be answered as a run id without
    // reading text back out of the control.
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let picked = move || -> Option<String> {
        let row = *list.selected_items().first()?;
        ids.get(row).cloned()
    };
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
            // Nothing selected answers as a cancellation: "OK" over an empty
            // selection is not a request to change what is on screen.
            E::OnButtonClick if handle == ok_handle => {
                send_outcome(&sent, &shared, &notify, picked());
                window.close();
            }
            // Double-clicking a row is the gesture a list invites, and it means
            // the same thing as selecting it and pressing OK.
            E::OnListViewItemActivated if handle == list_handle => {
                send_outcome(&sent, &shared, &notify, picked());
                window.close();
            }
            // A list view keeps arrow keys and Enter to itself but lets both
            // through as key events, so Enter can confirm and Escape can leave.
            E::OnKeyRelease => match evt_data {
                nwg::EventData::OnKey(VK_RETURN) => {
                    send_outcome(&sent, &shared, &notify, picked());
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

/// Publishes the chosen run id and wakes the main window — at most once per
/// dialog. See `run_dialog::send_outcome` for why the guard matters.
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
    shared.lock().unwrap().history = Some(outcome);
    notify.notice();
    nwg::stop_thread_dispatch();
}
