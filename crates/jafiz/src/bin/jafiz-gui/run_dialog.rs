//! "New run" dialog — the modal that asks for the environment (URL, build,
//! browser, test user) before a run starts.
//!
//! Stub: the window itself arrives with recording. What is real already is the
//! contract with the main window — the nwg multithread dialog pattern (see
//! `crates/mcp-console/src/gui/preferences_dialog.rs`): the dialog runs on its
//! own thread with its own event loop and never touches the UI thread's state,
//! reporting its outcome through `Shared` and waking the main window with a
//! `Notice`. Until the window exists the stub reports a cancellation, which is
//! exactly what an empty dialog means and keeps that whole path exercised.

use crate::Shared;
use native_windows_gui as nwg;
use std::sync::{Arc, Mutex};

/// What the main window hands the dialog thread.
pub struct RunDialogParams {
    /// The mailbox the outcome is left in; drained on the UI thread.
    pub shared: Arc<Mutex<Shared>>,
    /// Wakes the main window once the outcome is in the mailbox.
    pub notify: nwg::NoticeSender,
}

/// Starts the dialog thread. Returns immediately — the outcome comes back
/// through the `Notice`, never as a return value.
pub fn spawn(params: RunDialogParams) {
    std::thread::spawn(move || {
        let RunDialogParams { shared, notify } = params;
        // `Some(None)` is the mailbox's spelling of "cancelled".
        shared.lock().unwrap().environment = Some(None);
        notify.notice();
    });
}
