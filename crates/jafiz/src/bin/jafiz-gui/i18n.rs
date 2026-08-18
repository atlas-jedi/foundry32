//! Bilingual (pt-BR / en) strings for the JAFIZ GUI. `%C`/`%D`/`%I`/`%N`/`%S`/
//! `%V` are filled in at use. The `Lang` selector and system detection are
//! shared workspace-wide.

// `detect_system_lang` is not re-exported here the way WITN's table does it:
// `AppSettings::load` already falls back to it, so the GUI never calls it
// directly and an unused re-export is a hard error under `-D warnings`.
pub use foundry_common::lang::Lang;

// The window is built in layers, so three fields below still carry their own
// `#[allow(dead_code)]`: the recent-locations submenu and the report actions
// get their call sites in the task that adds run browsing and report export.
// Splitting the table across those tasks would mean the pt-BR and en wording of
// one feature no longer sits side by side, which is the only reason this file
// exists — and an unused field is a hard error under `-D warnings`. The
// exemption is per-field on purpose: it covers exactly the strings that have no
// caller yet, and disappears one attribute at a time as each is wired up —
// recording took nine of the original twelve with it.
pub struct T {
    pub app_title: &'static str,
    // Menu bar.
    pub menu_file: &'static str,
    pub menu_open: &'static str,
    #[allow(dead_code)]
    pub menu_recent: &'static str,
    pub menu_reload: &'static str,
    pub menu_exit: &'static str,
    pub menu_run: &'static str,
    pub menu_run_new: &'static str,
    pub menu_run_env: &'static str,
    pub menu_run_finish: &'static str,
    pub menu_run_history: &'static str,
    pub menu_report: &'static str,
    pub menu_report_copy: &'static str,
    pub menu_report_save: &'static str,
    pub menu_tools: &'static str,
    pub menu_prefs: &'static str,
    pub menu_help: &'static str,
    pub menu_help_format: &'static str,
    pub menu_about: &'static str,
    pub about_title: &'static str,
    /// `%V` — the app version.
    pub about_body: &'static str,
    // Header row. The four `loc_*` are the GUI's own wording for a
    // `store::LocationKind`: the engine's `label()` is a pt-BR CLI constant
    // ("projeto", "biblioteca") and would leak into an English window.
    pub lbl_suite: &'static str,
    pub lbl_location: &'static str,
    pub loc_explicit: &'static str,
    pub loc_env: &'static str,
    pub loc_project: &'static str,
    pub loc_library: &'static str,
    // Scenario list columns.
    pub col_status: &'static str,
    pub col_id: &'static str,
    pub col_scenario: &'static str,
    pub col_progress: &'static str,
    // Step list columns.
    pub col_num: &'static str,
    pub col_action: &'static str,
    pub col_expected: &'static str,
    pub col_note: &'static str,
    // Verdict buttons.
    pub btn_pass: &'static str,
    pub btn_fail: &'static str,
    pub btn_blocked: &'static str,
    pub btn_skip: &'static str,
    pub btn_note: &'static str,
    pub btn_evidence: &'static str,
    // Status bar.
    /// `%D` — the folder that has no suites in it.
    pub no_suites: &'static str,
    pub no_run: &'static str,
    /// `%N` — how many problems the parser reported about the open suite.
    pub diagnostics: &'static str,
    /// `%S` the file name, `%E` the OS error — the suite could not be read.
    pub read_error: &'static str,
    /// `%C` scenario, `%S` step.
    #[allow(dead_code)]
    pub testing_now: &'static str,
    // Dialogs.
    pub run_dlg_title: &'static str,
    pub run_dlg_env: &'static str,
    pub run_dlg_hint: &'static str,
    /// `%C` scenario, `%S` step.
    pub note_dlg_title: &'static str,
    pub note_dlg_hint: &'static str,
    #[allow(dead_code)]
    pub copied: &'static str,
    pub finish_title: &'static str,
    /// `%I` — the run id.
    pub finish_body: &'static str,
    /// Appended to a step's status when the suite was edited after the verdict.
    pub stale_marker: &'static str,
    pub dlg_ok: &'static str,
    pub dlg_cancel: &'static str,
}

static PT: T = T {
    app_title: "JAFIZ — já fiz?",
    menu_file: "Arquivo",
    menu_open: "Abrir pasta…",
    menu_recent: "Locais recentes",
    menu_reload: "Recarregar\tF5",
    menu_exit: "Sair",
    menu_run: "Execução",
    menu_run_new: "Nova execução…",
    menu_run_env: "Ambiente…",
    menu_run_finish: "Finalizar execução",
    menu_run_history: "Histórico…",
    menu_report: "Relatório",
    menu_report_copy: "Copiar para o Claude",
    menu_report_save: "Salvar como .md…",
    menu_tools: "Ferramentas",
    menu_prefs: "Preferências…",
    menu_help: "Ajuda",
    menu_help_format: "Formato do arquivo…",
    menu_about: "Sobre",
    about_title: "Sobre o JAFIZ",
    about_body: "JAFIZ %V\r\nJá fiz? — execute cenários de teste manuais passo a passo.\r\n\r\ngithub.com/atlas-jedi/foundry32\r\nLicença MIT — Software Imperial",
    lbl_suite: "Suíte:",
    lbl_location: "Local:",
    loc_explicit: "pasta escolhida",
    loc_env: "JAFIZ_DIR",
    loc_project: "projeto",
    loc_library: "biblioteca",
    col_status: "Status",
    col_id: "Id",
    col_scenario: "Cenário",
    col_progress: "Passos",
    col_num: "#",
    col_action: "Ação",
    col_expected: "Esperado",
    col_note: "Observação",
    btn_pass: "Passou (F2)",
    btn_fail: "Falhou (F3)",
    btn_blocked: "Bloqueado (F4)",
    btn_skip: "Pular (F6)",
    btn_note: "Observação…",
    btn_evidence: "Evidência…",
    no_suites: "Nenhuma suíte em %D — crie uma com: jafiz new <nome>",
    no_run: "Nenhuma execução. Use Execução ▸ Nova execução…",
    diagnostics: "⚠ %N problema(s) no arquivo",
    read_error: "Falha ao ler %S: %E",
    testing_now: "testando agora: %C passo %S",
    run_dlg_title: "Nova execução",
    run_dlg_env: "Ambiente / build:",
    run_dlg_hint: "URL, versão, browser, usuário de teste — entra no relatório.",
    note_dlg_title: "Observação — %C passo %S",
    note_dlg_hint: "O que aconteceu? Entra no relatório abaixo do veredito.",
    copied: "Relatório copiado para a área de transferência.",
    finish_title: "Finalizar execução",
    finish_body: "Encerrar a execução %I? Ela deixa de aceitar novas marcações.",
    stale_marker: "!",
    dlg_ok: "OK",
    dlg_cancel: "Cancelar",
};

static EN: T = T {
    app_title: "JAFIZ — did I test it yet?",
    menu_file: "File",
    menu_open: "Open folder…",
    menu_recent: "Recent locations",
    menu_reload: "Reload\tF5",
    menu_exit: "Exit",
    menu_run: "Run",
    menu_run_new: "New run…",
    menu_run_env: "Environment…",
    menu_run_finish: "Finish run",
    menu_run_history: "History…",
    menu_report: "Report",
    menu_report_copy: "Copy for Claude",
    menu_report_save: "Save as .md…",
    menu_tools: "Tools",
    menu_prefs: "Preferences…",
    menu_help: "Help",
    menu_help_format: "File format…",
    menu_about: "About",
    about_title: "About JAFIZ",
    about_body: "JAFIZ %V\r\nJá fiz? — run manual test scenarios step by step.\r\n\r\ngithub.com/atlas-jedi/foundry32\r\nMIT License — Software Imperial",
    lbl_suite: "Suite:",
    lbl_location: "Location:",
    loc_explicit: "chosen folder",
    loc_env: "JAFIZ_DIR",
    loc_project: "project",
    loc_library: "library",
    col_status: "Status",
    col_id: "Id",
    col_scenario: "Scenario",
    col_progress: "Steps",
    col_num: "#",
    col_action: "Action",
    col_expected: "Expected",
    col_note: "Note",
    btn_pass: "Passed (F2)",
    btn_fail: "Failed (F3)",
    btn_blocked: "Blocked (F4)",
    btn_skip: "Skip (F6)",
    btn_note: "Note…",
    btn_evidence: "Evidence…",
    no_suites: "No suites in %D — create one with: jafiz new <name>",
    no_run: "No run yet. Use Run ▸ New run…",
    diagnostics: "⚠ %N problem(s) in the file",
    read_error: "Could not read %S: %E",
    testing_now: "testing now: %C step %S",
    run_dlg_title: "New run",
    run_dlg_env: "Environment / build:",
    run_dlg_hint: "URL, version, browser, test user — it goes into the report.",
    note_dlg_title: "Note — %C step %S",
    note_dlg_hint: "What happened? It goes into the report under the verdict.",
    copied: "Report copied to the clipboard.",
    finish_title: "Finish run",
    finish_body: "End run %I? It stops accepting new verdicts.",
    stale_marker: "!",
    dlg_ok: "OK",
    dlg_cancel: "Cancel",
};

pub fn t(lang: Lang) -> &'static T {
    match lang {
        Lang::PtBr => &PT,
        Lang::En => &EN,
    }
}
