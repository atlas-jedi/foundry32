//! Bilingual (pt-BR / en) strings for the WITN GUI. `%N`/`%S`/`%A` are filled in
//! at use. The `Lang` selector and system detection are shared workspace-wide.

pub use foundry_common::lang::{detect_system_lang, Lang};

pub struct T {
    pub app_title: &'static str,
    pub col_app: &'static str,
    pub col_pid: &'static str,
    pub col_ppid: &'static str,
    pub col_ports: &'static str,
    pub col_cpu: &'static str,
    pub col_ram: &'static str,
    pub col_uptime: &'static str,
    pub col_path: &'static str,
    pub btn_refresh: &'static str,
    pub btn_pause: &'static str,
    pub btn_resume: &'static str,
    pub btn_kill: &'static str,
    pub btn_open: &'static str,
    pub status_count: &'static str,
    pub status_live: &'static str,
    pub status_paused: &'static str,
    pub kill_title: &'static str,
    pub kill_body: &'static str,
    pub empty: &'static str,
}

static PT: T = T {
    app_title: "WITN — Where Is The Node?",
    col_app: "Aplicação",
    col_pid: "PID",
    col_ppid: "PID pai",
    col_ports: "Porta(s)",
    col_cpu: "CPU",
    col_ram: "RAM",
    col_uptime: "Ativo há",
    col_path: "Local do app",
    btn_refresh: "Atualizar",
    btn_pause: "Pausar",
    btn_resume: "Retomar",
    btn_kill: "Encerrar…",
    btn_open: "Abrir local",
    status_count: "%N processos node",
    status_live: "ao vivo (%Ss)",
    status_paused: "pausado",
    kill_title: "Encerrar processo",
    kill_body: "Encerrar %A e toda a sua árvore de processos (%N no total)?",
    empty: "Nenhum processo node.exe em execução.",
};

static EN: T = T {
    app_title: "WITN — Where Is The Node?",
    col_app: "Application",
    col_pid: "PID",
    col_ppid: "Parent",
    col_ports: "Port(s)",
    col_cpu: "CPU",
    col_ram: "RAM",
    col_uptime: "Uptime",
    col_path: "App location",
    btn_refresh: "Refresh",
    btn_pause: "Pause",
    btn_resume: "Resume",
    btn_kill: "End…",
    btn_open: "Open location",
    status_count: "%N node processes",
    status_live: "live (%Ss)",
    status_paused: "paused",
    kill_title: "End process",
    kill_body: "End %A and its whole process tree (%N total)?",
    empty: "No node.exe processes running.",
};

pub fn t(lang: Lang) -> &'static T {
    match lang {
        Lang::PtBr => &PT,
        Lang::En => &EN,
    }
}
