//! Bilingual (pt-BR / en) strings for the DAILY window. `%V` (version) and
//! `%S` (an entry's text) are filled in at use. The `Lang` selector and system
//! detection are shared workspace-wide.

pub use foundry_common::lang::Lang;

pub struct T {
    pub app_title: &'static str,
    // Menu bar.
    pub menu_file: &'static str,
    pub menu_open_folder: &'static str,
    pub menu_reload: &'static str,
    pub menu_exit: &'static str,
    pub menu_report: &'static str,
    pub menu_copy: &'static str,
    pub menu_tools: &'static str,
    pub menu_prefs: &'static str,
    pub menu_help: &'static str,
    pub menu_about: &'static str,
    pub about_title: &'static str,
    /// `%V` — the app version.
    pub about_body: &'static str,
    // Day row.
    pub btn_today: &'static str,
    pub today_suffix: &'static str,
    // The form.
    pub lbl_task: &'static str,
    pub lbl_text: &'static str,
    pub btn_add: &'static str,
    pub task_hint: &'static str,
    pub text_hint: &'static str,
    pub no_recent_tasks: &'static str,
    // The list.
    pub col_time: &'static str,
    pub col_task: &'static str,
    pub col_text: &'static str,
    pub col_author: &'static str,
    // Under the list.
    pub btn_edit: &'static str,
    pub btn_delete: &'static str,
    pub btn_copy: &'static str,
    // Status bar and messages.
    pub copied: &'static str,
    pub empty_text: &'static str,
    pub nothing_to_copy: &'static str,
    pub delete_title: &'static str,
    /// `%S` — the entry's text.
    pub delete_body: &'static str,
    pub error_title: &'static str,
    // Edit dialog.
    pub edit_title: &'static str,
    pub dlg_time: &'static str,
    pub dlg_description: &'static str,
    pub dlg_ok: &'static str,
    pub dlg_cancel: &'static str,
    // Preferences.
    pub pref_title: &'static str,
    pub pref_section_interface: &'static str,
    pub pref_lang: &'static str,
    pub pref_hint: &'static str,
}

static PT: T = T {
    app_title: "DAILY — o que fiz hoje",
    menu_file: "&Arquivo",
    menu_open_folder: "Abrir pasta dos dados",
    menu_reload: "Recarregar\tF5",
    menu_exit: "Sair",
    menu_report: "&Relatório",
    menu_copy: "Copiar para a daily",
    menu_tools: "&Ferramentas",
    menu_prefs: "Preferências…",
    menu_help: "Aj&uda",
    menu_about: "Sobre o DAILY",
    about_title: "Sobre o DAILY",
    about_body: "DAILY %V — o que fiz hoje\n\nAnote durante o dia o que você fez e leve para a daily a lista \
                 agrupada por task.\n\nO Claude lê e escreve aqui pelo comando daily (daily --help).\n\n\
                 © Software Imperial · MIT",
    btn_today: "Hoje",
    today_suffix: "  (hoje)",
    lbl_task: "Task:",
    lbl_text: "O que fiz:",
    btn_add: "Anotar",
    task_hint: "código ou nome da task (opcional)",
    text_hint: "o que você fez — Enter anota",
    no_recent_tasks: "(nenhuma task nos últimos dias)",
    col_time: "Hora",
    col_task: "Task",
    col_text: "O que fiz",
    col_author: "Quem",
    btn_edit: "Editar…",
    btn_delete: "Apagar",
    btn_copy: "Copiar para a daily",
    copied: "Copiado. É só colar.",
    empty_text: "Escreva o que você fez antes de anotar.",
    nothing_to_copy: "Nada anotado neste dia.",
    delete_title: "Apagar anotação",
    delete_body: "Apagar esta anotação?\n\n%S",
    error_title: "DAILY",
    edit_title: "Editar anotação",
    dlg_time: "Hora:",
    dlg_description: "Descrição:",
    dlg_ok: "OK",
    dlg_cancel: "Cancelar",
    pref_title: "Preferências",
    pref_section_interface: "Interface",
    pref_lang: "Idioma:",
    pref_hint: "Vale para a janela e para o comando daily.",
};

static EN: T = T {
    app_title: "DAILY — what I did today",
    menu_file: "&File",
    menu_open_folder: "Open data folder",
    menu_reload: "Reload\tF5",
    menu_exit: "Exit",
    menu_report: "&Report",
    menu_copy: "Copy for the stand-up",
    menu_tools: "&Tools",
    menu_prefs: "Preferences…",
    menu_help: "&Help",
    menu_about: "About DAILY",
    about_title: "About DAILY",
    about_body: "DAILY %V — what I did today\n\nWrite down what you did during the day and take the list, \
                 grouped by task, to the stand-up.\n\nClaude reads and writes here through the daily \
                 command (daily --help).\n\n© Software Imperial · MIT",
    btn_today: "Today",
    today_suffix: "  (today)",
    lbl_task: "Task:",
    lbl_text: "What I did:",
    btn_add: "Add",
    task_hint: "task code or name (optional)",
    text_hint: "what you did — Enter adds it",
    no_recent_tasks: "(no task in the last days)",
    col_time: "Time",
    col_task: "Task",
    col_text: "What I did",
    col_author: "Who",
    btn_edit: "Edit…",
    btn_delete: "Delete",
    btn_copy: "Copy for the stand-up",
    copied: "Copied. Just paste it.",
    empty_text: "Write what you did before adding it.",
    nothing_to_copy: "Nothing written down on this day.",
    delete_title: "Delete entry",
    delete_body: "Delete this entry?\n\n%S",
    error_title: "DAILY",
    edit_title: "Edit entry",
    dlg_time: "Time:",
    dlg_description: "Description:",
    dlg_ok: "OK",
    dlg_cancel: "Cancel",
    pref_title: "Preferences",
    pref_section_interface: "Interface",
    pref_lang: "Language:",
    pref_hint: "Applies to the window and to the daily command.",
};

pub fn t(lang: Lang) -> &'static T {
    match lang {
        Lang::PtBr => &PT,
        Lang::En => &EN,
    }
}

/// `4 tasks` / `1 task` — the same word in both languages, only the plural.
pub fn tasks_count(count: usize) -> String {
    if count == 1 {
        "1 task".into()
    } else {
        format!("{count} tasks")
    }
}

/// `2 do Claude` / `2 by Claude`.
pub fn by_claude(count: usize, lang: Lang) -> String {
    match lang {
        Lang::PtBr => format!("{count} do Claude"),
        Lang::En => format!("{count} by Claude"),
    }
}
