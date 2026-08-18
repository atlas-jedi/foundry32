//! "Note" dialog — the modal that collects what actually happened on a step,
//! so the verdict lands in the report with the tester's own words attached.
//!
//! Stub, on the same terms as `run_dialog`: the window arrives with recording,
//! while the thread/`Shared`/`Notice` contract with the main window is already
//! the real one. Until then it reports a cancellation.

use crate::Shared;
use native_windows_gui as nwg;
use std::sync::{Arc, Mutex};

/// What the main window hands the dialog thread.
pub struct NoteDialogParams {
    /// The mailbox the outcome is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the outcome is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the outcome comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: NoteDialogParams) {
    std::thread::spawn(move || {
        let NoteDialogParams { shared, notify } = params;
        // `Some(None)` is the mailbox's spelling of "cancelled".
        shared.lock().unwrap().note = Some(None);
        notify.notice();
    });
}
