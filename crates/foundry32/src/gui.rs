//! Placeholder hub window. The real Windows-Classic hub UI (catalog list,
//! details pane, install/run/uninstall actions, classic progress bar) is built
//! in the next step; for now this confirms the binary launches in GUI mode and
//! respects the saved language.

use crate::settings::AppSettings;
use foundry_common::Lang;
use native_windows_gui as nwg;

pub fn run(settings: AppSettings) {
    let message = match settings.lang {
        Lang::PtBr => "Foundry32 — a janela do hub será construída na próxima etapa.",
        Lang::En => "Foundry32 — the hub window is built in the next step.",
    };
    nwg::simple_message("Foundry32", message);
}
