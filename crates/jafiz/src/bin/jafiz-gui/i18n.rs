//! Bilingual (pt-BR / en) strings for the JAFIZ GUI. `%C`/`%D`/`%I`/`%N`/`%P`/
//! `%S`/`%V` are filled in at use. The `Lang` selector and system detection are
//! shared workspace-wide.

// `detect_system_lang` is not re-exported here the way WITN's table does it:
// `AppSettings::load` already falls back to it, so the GUI never calls it
// directly and an unused re-export is a hard error under `-D warnings`.
pub use foundry_common::lang::Lang;

// One field below still carries its own `#[allow(dead_code)]`: `menu_recent`
// names the recent-locations submenu, a feature the settings file already
// records the data for (`AppSettings::locations`) but no menu shows yet.
// Deleting the wording would mean the pt-BR and en halves of that feature stop
// sitting side by side, which is the only reason this file exists — and an
// unused field is a hard error under `-D warnings`. The exemption is per-field
// on purpose, and every string with a caller has had it removed.
pub struct T {
    pub app_title: &'static str,
    // Menu bar.
    pub menu_file: &'static str,
    pub menu_open: &'static str,
    #[allow(dead_code)]
    pub menu_recent: &'static str,
    pub menu_reload: &'static str,
    pub menu_exit: &'static str,
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
    // Run toolbar. There is no "Execução" menu: these four controls are the
    // whole life cycle of a run, and being greyed out is how they say there is
    // no run open — which a menu item cannot.
    pub btn_run_new: &'static str,
    pub btn_run_finish: &'static str,
    pub btn_history: &'static str,
    pub lbl_env: &'static str,
    /// Last row of the environment picker: opens the dialog to type a new one.
    pub env_other: &'static str,
    /// Stands in for a run whose environment was left blank, which a combo box
    /// would otherwise show as an empty row nobody can tell from a bug.
    pub env_unset: &'static str,
    /// Title of the environment dialog when it edits the open run rather than
    /// starting one (`run_dlg_title` is the other case).
    pub env_dlg_title: &'static str,
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
    /// `%E` — the run file's path and the underlying error. A history that
    /// exists but cannot be read is not the same as a suite never run, and the
    /// window must not show the second when it found the first.
    pub runs_damaged: &'static str,
    /// `%I` — the run being read instead of the one recording goes to.
    pub viewing_run: &'static str,
    /// `%I` — said out loud when a verdict key is pressed while reading an old
    /// run, which the disabled buttons alone cannot prevent.
    pub read_only: &'static str,
    // Dialogs.
    pub run_dlg_title: &'static str,
    pub run_dlg_env: &'static str,
    pub run_dlg_hint: &'static str,
    /// `%C` scenario, `%S` step.
    pub note_dlg_title: &'static str,
    pub note_dlg_hint: &'static str,
    // Run history.
    pub hist_hint: &'static str,
    pub hist_col_run: &'static str,
    pub hist_col_started: &'static str,
    pub hist_col_finished: &'static str,
    pub hist_col_env: &'static str,
    pub hist_col_progress: &'static str,
    /// Shown in the "finished" column of a run that has not been closed.
    pub hist_running: &'static str,
    // Preferences.
    pub pref_title: &'static str,
    pub pref_section_interface: &'static str,
    pub pref_lang: &'static str,
    pub pref_hint: &'static str,
    // Report export.
    pub copied: &'static str,
    /// `%P` — where the report was written.
    pub saved: &'static str,
    /// File-dialog filter labels.
    pub filter_md: &'static str,
    pub filter_all: &'static str,
    pub finish_title: &'static str,
    /// `%I` — the run id.
    pub finish_body: &'static str,
    /// Asked before starting a new run over a run history the loader could not
    /// read: silently proceeding would replace the damaged-but-maybe-repairable
    /// file with a history holding only the new run, the exact data loss the
    /// atomic save was written to prevent, reached through a different door.
    pub new_run_damaged_title: &'static str,
    /// `%E` — the same damage reason the status bar already shows.
    pub new_run_damaged_body: &'static str,
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
    btn_run_new: "▶ Nova execução…",
    btn_run_finish: "■ Finalizar",
    btn_history: "Histórico…",
    lbl_env: "Ambiente:",
    env_other: "Outro…",
    env_unset: "(sem ambiente)",
    env_dlg_title: "Ambiente",
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
    no_run: "Nenhuma execução. Clique em ▶ Nova execução… na barra acima.",
    diagnostics: "⚠ %N problema(s) no arquivo",
    read_error: "Falha ao ler %S: %E",
    runs_damaged: "⚠ histórico de execuções danificado — %E",
    viewing_run: "lendo a execução %I — a gravação continua na execução ativa",
    read_only: "Você está lendo a execução %I, que é só leitura.\r\n\r\nPara marcar passos, volte à execução ativa em Histórico… na barra acima.",
    run_dlg_title: "Nova execução",
    run_dlg_env: "Ambiente / build:",
    run_dlg_hint: "URL, versão, browser, usuário de teste — entra no relatório.",
    note_dlg_title: "Observação — %C passo %S",
    note_dlg_hint: "O que aconteceu? Entra no relatório abaixo do veredito.",
    hist_hint: "Escolha uma execução para ler. Marcar passos continua indo para a execução ativa.",
    hist_col_run: "Execução",
    hist_col_started: "Início",
    hist_col_finished: "Fim",
    hist_col_env: "Ambiente",
    hist_col_progress: "Passos",
    hist_running: "em andamento",
    pref_title: "Preferências",
    pref_section_interface: "Interface",
    pref_lang: "Idioma:",
    pref_hint: "A alteração é aplicada imediatamente.",
    copied: "Relatório copiado para a área de transferência.",
    saved: "Relatório salvo em %P",
    filter_md: "Markdown (*.md)",
    filter_all: "Todos os arquivos (*.*)",
    finish_title: "Finalizar execução",
    finish_body: "Encerrar a execução %I? Ela deixa de aceitar novas marcações.",
    new_run_damaged_title: "Histórico de execuções danificado",
    new_run_damaged_body: "O histórico de execuções desta suíte não pôde ser lido (%E).\r\n\r\nIniciar uma nova execução agora vai SUBSTITUIR o arquivo danificado por um histórico contendo só a execução nova — o que já estava gravado se perde.\r\n\r\nIniciar mesmo assim?",
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
    btn_run_new: "▶ New run…",
    btn_run_finish: "■ Finish",
    btn_history: "History…",
    lbl_env: "Environment:",
    env_other: "Other…",
    env_unset: "(no environment)",
    env_dlg_title: "Environment",
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
    no_run: "No run yet. Click ▶ New run… on the toolbar above.",
    diagnostics: "⚠ %N problem(s) in the file",
    read_error: "Could not read %S: %E",
    runs_damaged: "⚠ run history is damaged — %E",
    viewing_run: "reading run %I — recording still goes to the active run",
    read_only: "You are reading run %I, which is read-only.\r\n\r\nTo mark steps, go back to the active run under History… on the toolbar above.",
    run_dlg_title: "New run",
    run_dlg_env: "Environment / build:",
    run_dlg_hint: "URL, version, browser, test user — it goes into the report.",
    note_dlg_title: "Note — %C step %S",
    note_dlg_hint: "What happened? It goes into the report under the verdict.",
    hist_hint: "Pick a run to read. Marking steps still goes to the active run.",
    hist_col_run: "Run",
    hist_col_started: "Started",
    hist_col_finished: "Finished",
    hist_col_env: "Environment",
    hist_col_progress: "Steps",
    hist_running: "in progress",
    pref_title: "Preferences",
    pref_section_interface: "Interface",
    pref_lang: "Language:",
    pref_hint: "The change is applied immediately.",
    copied: "Report copied to the clipboard.",
    saved: "Report saved to %P",
    filter_md: "Markdown (*.md)",
    filter_all: "All files (*.*)",
    finish_title: "Finish run",
    finish_body: "End run %I? It stops accepting new verdicts.",
    new_run_damaged_title: "Run history is damaged",
    new_run_damaged_body: "This suite's run history could not be read (%E).\r\n\r\nStarting a new run now will REPLACE the damaged file with a history holding only the new run — whatever was recorded before is lost.\r\n\r\nStart anyway?",
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
