//! The Details report — one rendering of a process's details shared by the
//! GUI's tabs, its "Copy all" text, and `witn info`, so the three can never
//! disagree. Sections and field names follow Process Explorer's Properties
//! and Task Manager's Details (`Current directory`, `<Non-existent process>`,
//! `Below normal`…), in Portuguese and English.

use crate::details::{Details, Priority};
use crate::model::{format_datetime, format_uptime, is_sensitive_env, Evidence, Lang, NodeProc};
use crate::search::Hit;

/// Which tab a section belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    General,
    Origin,
    Performance,
    Network,
    Environment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<String>,
    /// The last cell is a credential, masked unless asked.
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub title: String,
    pub rows: Vec<Row>,
    pub table: Option<Table>,
}

/// What a masked value shows instead.
pub const MASK: &str = "••••••••";

/// Shown for a field that could not be read.
const NONE: &str = "—";

/// Picks the string for `lang`.
fn tr(lang: Lang, pt: &str, en: &str) -> String {
    match lang {
        Lang::PtBr => pt.to_string(),
        Lang::En => en.to_string(),
    }
}

fn row(label: String, value: impl Into<String>) -> Row {
    Row { label, value: value.into() }
}

fn or_none(value: Option<String>) -> String {
    value.unwrap_or_else(|| NONE.to_string())
}

fn yes_no(lang: Lang, value: bool) -> String {
    if value { tr(lang, "Sim", "Yes") } else { tr(lang, "Não", "No") }
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / (1024 * 1024))
}

fn env_value<'a>(p: &'a NodeProc, name: &str) -> Option<&'a str> {
    p.env.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

/// Builds every section of the report.
pub fn build(p: &NodeProc, d: &Details, lang: Lang) -> Vec<Section> {
    vec![general(p, d, lang), origin(p, d, lang), performance(p, d, lang), network(d, lang), environment(p, lang)]
}

fn general(p: &NodeProc, d: &Details, lang: Lang) -> Section {
    let path = |value: &Option<std::path::PathBuf>| or_none(value.as_ref().map(|x| x.display().to_string()));
    let mut rows = Vec::new();
    if !d.alive {
        rows.push(row(
            tr(lang, "Situação", "Status"),
            tr(lang, "encerrado desde a última leitura", "exited since the last scan"),
        ));
    }
    rows.extend([
        row(tr(lang, "Aplicação", "Application"), p.app_name.clone()),
        row("PID".to_string(), p.pid.to_string()),
        row(tr(lang, "Iniciado em", "Started"), or_none(d.started.map(|t| format_datetime(&t, lang)))),
        row(tr(lang, "Ativo há", "Uptime"), format_uptime(p.uptime_secs)),
        row(tr(lang, "Local do app", "App location"), path(&p.app_location())),
        row(tr(lang, "Diretório de trabalho (cwd)", "Current directory (cwd)"), path(&p.cwd)),
        row(tr(lang, "Linha de comando", "Command line"), or_none(p.cmdline.clone())),
        row(tr(lang, "Executável do Node", "Node executable"), path(&p.exe_path)),
        row(tr(lang, "Versão do Node", "Node version"), or_none(d.node_version.clone())),
        row(
            tr(lang, "Arquitetura", "Architecture"),
            or_none(d.is_64bit.map(|is64| match (is64, lang) {
                (true, Lang::PtBr) => "64 bits".to_string(),
                (false, Lang::PtBr) => "32 bits".to_string(),
                (true, Lang::En) => "64-bit".to_string(),
                (false, Lang::En) => "32-bit".to_string(),
            })),
        ),
        row(tr(lang, "Usuário", "User"), or_none(d.user.clone())),
        row(tr(lang, "Elevado (admin)", "Elevated (admin)"), or_none(d.elevated.map(|e| yes_no(lang, e)))),
    ]);
    Section { kind: SectionKind::General, title: tr(lang, "Geral", "General"), rows, table: None }
}

fn origin(p: &NodeProc, d: &Details, lang: Lang) -> Section {
    let o = &p.origin;
    let ended = tr(lang, "encerrado", "exited");
    let mut rows = Vec::new();

    let started_by = match &o.launcher {
        Some(l) => {
            let mut parts = Vec::new();
            if let Some(exe) = &l.exe {
                parts.push(exe.clone());
            }
            if let Some(pid) = l.pid {
                let status = if l.alive { String::new() } else { format!(" ({ended})") };
                parts.push(format!("PID {pid}{status}"));
            }
            if parts.is_empty() { l.name.clone() } else { format!("{} — {}", l.name, parts.join(", ")) }
        }
        None => tr(lang, "desconhecido", "unknown"),
    };
    rows.push(row(tr(lang, "Iniciado por", "Started by"), started_by));
    if let Some(l) = &o.launcher {
        let evidence = match &l.evidence {
            Evidence::Ancestor => tr(lang, "processo ancestral em execução", "running ancestor process"),
            Evidence::Env(vars) => format!("{}: {}", tr(lang, "variáveis de ambiente", "environment variables"), vars.join(", ")),
        };
        rows.push(row(tr(lang, "Evidência", "Evidence"), evidence));
    }

    let parent = match (o.parent_gone, o.ancestors.first()) {
        (Some(pid), _) => format!("{} (PID {pid})", tr(lang, "<encerrado>", "<Non-existent process>")),
        (None, Some(first)) => format!("{} (PID {})", first.exe_name, first.pid),
        (None, None) => NONE.to_string(),
    };
    rows.push(row(tr(lang, "Processo pai", "Parent"), parent));
    rows.push(row(tr(lang, "Órfão", "Orphan"), yes_no(lang, o.parent_gone.is_some())));

    if let Some(event) = env_value(p, "npm_lifecycle_event") {
        let script = env_value(p, "npm_lifecycle_script").map(|s| format!(" → {s}")).unwrap_or_default();
        rows.push(row(tr(lang, "Script npm", "npm script"), format!("{event}{script}")));
    }
    if let Some(name) = env_value(p, "npm_package_name") {
        let version = env_value(p, "npm_package_version").map(|v| format!("@{v}")).unwrap_or_default();
        rows.push(row(tr(lang, "Pacote npm", "npm package"), format!("{name}{version}")));
    }
    if let Some(session) = env_value(p, "CLAUDE_CODE_SESSION_ID") {
        rows.push(row(tr(lang, "Sessão do Claude Code", "Claude Code session"), session.to_string()));
    }

    // Top-down, like a tree: the farthest ancestor first, this process in the
    // middle, then everything it started.
    let cmdline = |c: &Option<String>| c.clone().unwrap_or_default();
    let mut lineage = Vec::new();
    for (index, a) in d.ancestors.iter().enumerate().rev() {
        let relation = if index == 0 {
            tr(lang, "Pai", "Parent")
        } else {
            format!("{} {}", tr(lang, "Ancestral", "Ancestor"), index + 1)
        };
        lineage.push(TableRow {
            cells: vec![relation, a.pid.to_string(), a.exe_name.clone(), cmdline(&a.cmdline)],
            sensitive: false,
        });
    }
    let exe_name = p
        .exe_path
        .as_ref()
        .and_then(|x| x.file_name())
        .map_or_else(|| "node.exe".to_string(), |n| n.to_string_lossy().into_owned());
    lineage.push(TableRow {
        cells: vec![tr(lang, "Este processo", "This process"), p.pid.to_string(), exe_name, cmdline(&p.cmdline)],
        sensitive: false,
    });
    for child in &d.descendants {
        lineage.push(TableRow {
            cells: vec![
                tr(lang, "Descendente", "Descendant"),
                child.pid.to_string(),
                child.exe_name.clone(),
                cmdline(&child.cmdline),
            ],
            sensitive: false,
        });
    }
    let table = Table {
        title: tr(lang, "Árvore de processos", "Process tree"),
        columns: vec![
            tr(lang, "Relação", "Relation"),
            "PID".to_string(),
            tr(lang, "Processo", "Process"),
            tr(lang, "Linha de comando", "Command line"),
        ],
        rows: lineage,
    };
    Section { kind: SectionKind::Origin, title: tr(lang, "Origem", "Origin"), rows, table: Some(table) }
}

fn performance(p: &NodeProc, d: &Details, lang: Lang) -> Section {
    let priority = d.priority.map(|priority| match priority {
        Priority::Realtime => tr(lang, "Tempo real", "Realtime"),
        Priority::High => tr(lang, "Alta", "High"),
        Priority::AboveNormal => tr(lang, "Acima do normal", "Above normal"),
        Priority::Normal => "Normal".to_string(),
        Priority::BelowNormal => tr(lang, "Abaixo do normal", "Below normal"),
        Priority::Idle => tr(lang, "Baixa", "Low"),
    });
    let rows = vec![
        row("CPU".to_string(), format!("{:.0}%", p.cpu_percent)),
        row(tr(lang, "Tempo de CPU", "CPU time"), or_none(d.cpu_time_secs.map(format_uptime))),
        row(tr(lang, "Memória (working set)", "Memory (working set)"), megabytes(p.mem_bytes)),
        row(tr(lang, "Pico de memória", "Peak working set"), or_none(d.peak_working_set.map(megabytes))),
        row(tr(lang, "Memória privada", "Private bytes"), or_none(d.private_bytes.map(megabytes))),
        row("Handles".to_string(), or_none(d.handles.map(|h| h.to_string()))),
        row("Threads".to_string(), or_none(d.threads.map(|t| t.to_string()))),
        row(tr(lang, "Prioridade", "Priority"), or_none(priority)),
    ];
    Section { kind: SectionKind::Performance, title: tr(lang, "Desempenho", "Performance"), rows, table: None }
}

fn network(d: &Details, lang: Lang) -> Section {
    let rows = d
        .endpoints
        .iter()
        .map(|e| TableRow {
            cells: vec![
                e.protocol.to_string(),
                e.local.clone(),
                e.remote.clone().unwrap_or_else(|| "*:*".to_string()),
                e.state.unwrap_or("").to_string(),
            ],
            sensitive: false,
        })
        .collect();
    let table = Table {
        title: tr(lang, "Conexões", "Connections"),
        columns: vec![
            tr(lang, "Protocolo", "Protocol"),
            tr(lang, "Endereço local", "Local address"),
            tr(lang, "Endereço remoto", "Remote address"),
            tr(lang, "Estado", "State"),
        ],
        rows,
    };
    Section { kind: SectionKind::Network, title: tr(lang, "Rede", "Network"), rows: Vec::new(), table: Some(table) }
}

fn environment(p: &NodeProc, lang: Lang) -> Section {
    let mut vars: Vec<&(String, String)> = p.env.iter().collect();
    vars.sort_by_key(|(name, _)| name.to_lowercase());
    let rows = vars
        .into_iter()
        .map(|(name, value)| TableRow {
            cells: vec![name.clone(), value.clone()],
            sensitive: is_sensitive_env(name, value),
        })
        .collect();
    let table = Table {
        title: tr(lang, "Variáveis", "Variables"),
        columns: vec![tr(lang, "Variável", "Variable"), tr(lang, "Valor", "Value")],
        rows,
    };
    Section { kind: SectionKind::Environment, title: tr(lang, "Ambiente", "Environment"), rows: Vec::new(), table: Some(table) }
}

/// A table row's cells for display: the last one masked when it is a
/// credential and secrets are hidden.
pub fn display_cells(row: &TableRow, show_secrets: bool) -> Vec<String> {
    let mut cells = row.cells.clone();
    if row.sensitive && !show_secrets {
        if let Some(last) = cells.last_mut() {
            *last = MASK.to_string();
        }
    }
    cells
}

/// The report as plain text (for the clipboard and the CLI).
pub fn to_text(header: &str, sections: &[Section], show_secrets: bool) -> String {
    let mut out = format!("{header}\r\n");
    for section in sections {
        out.push_str(&format!("\r\n{}\r\n", section.title.to_uppercase()));
        let width = section.rows.iter().map(|r| r.label.chars().count()).max().unwrap_or(0);
        for r in &section.rows {
            let pad = width - r.label.chars().count();
            out.push_str(&format!("  {}{}  {}\r\n", r.label, " ".repeat(pad), r.value));
        }
        if let Some(table) = &section.table {
            if !section.rows.is_empty() {
                out.push_str(&format!("  {}:\r\n", table.title));
            }
            if table.rows.is_empty() {
                out.push_str(&format!("  {NONE}\r\n"));
            }
            for table_row in &table.rows {
                let cells = display_cells(table_row, show_secrets);
                let line = if section.kind == SectionKind::Environment {
                    cells.join("=")
                } else {
                    cells.into_iter().filter(|c| !c.is_empty()).collect::<Vec<_>>().join("  ")
                };
                out.push_str(&format!("    {line}\r\n"));
            }
        }
    }
    out
}

/// The "Found in" text for one process's hits.
///
/// Environment hits collapse into a single `Ambiente: A, B, C (+N)` at the
/// first one's position: a term like `claude` matches a dozen variables, and
/// listing them all would bury the hits that answer the question.
pub fn hits_summary(hits: &[Hit], lang: Lang) -> String {
    const SHOWN_VARS: usize = 3;
    let mut vars: Vec<&str> = hits
        .iter()
        .filter_map(|hit| match hit {
            Hit::Env(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    // PATH first: "is it on some node's PATH?" is the question people ask.
    vars.sort_by_key(|name| !name.eq_ignore_ascii_case("PATH"));
    let mut parts = Vec::new();
    let mut env_done = false;
    for hit in hits {
        match hit {
            Hit::Env(_) if env_done => {}
            Hit::Env(_) => {
                env_done = true;
                let mut text = format!(
                    "{}: {}",
                    tr(lang, "Ambiente", "Environment"),
                    vars.iter().take(SHOWN_VARS).copied().collect::<Vec<_>>().join(", ")
                );
                if vars.len() > SHOWN_VARS {
                    text.push_str(&format!(" (+{})", vars.len() - SHOWN_VARS));
                }
                parts.push(text);
            }
            other => parts.push(hit_label(other, lang)),
        }
    }
    parts.join(", ")
}

/// Where a search hit was found, as the "Found in" column says it.
pub fn hit_label(hit: &Hit, lang: Lang) -> String {
    match hit {
        Hit::App => tr(lang, "Aplicação", "Application"),
        Hit::Pid => "PID".to_string(),
        Hit::ParentPid => tr(lang, "PID pai", "Parent PID"),
        Hit::Port => tr(lang, "Porta", "Port"),
        Hit::AppLocation => tr(lang, "Local do app", "App location"),
        Hit::Cwd => tr(lang, "Diretório de trabalho", "Current directory"),
        Hit::CommandLine => tr(lang, "Linha de comando", "Command line"),
        Hit::NodeExe => tr(lang, "Executável do Node", "Node executable"),
        Hit::Env(name) => format!("{}: {name}", tr(lang, "Ambiente", "Environment")),
        Hit::Launcher => tr(lang, "Iniciado por", "Started by"),
        Hit::Ancestor(exe) => format!("{}: {exe}", tr(lang, "Origem", "Origin")),
        Hit::Descendant(exe) => format!("{}: {exe}", tr(lang, "Filho", "Child")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Launcher, Origin, ProcRef};
    use std::path::PathBuf;

    fn orphan() -> NodeProc {
        NodeProc {
            pid: 16836,
            ppid: 17616,
            exe_path: Some(PathBuf::from(r"C:\nvm4w\nodejs\node.exe")),
            cmdline: Some(r"node C:\app\server.js".to_string()),
            cwd: Some(PathBuf::from(r"C:\Users\Pichau\sandbox\game-visual")),
            app_name: "server.js".to_string(),
            ports: vec![3939],
            mem_bytes: 14 * 1024 * 1024,
            cpu_percent: 0.0,
            uptime_secs: 3600,
            start_filetime: 1,
            depth: 0,
            env: vec![
                ("CLAUDE_CODE_SESSION_ID".to_string(), "abc-123".to_string()),
                ("GITHUB_TOKEN".to_string(), "ghp_secret".to_string()),
                ("npm_lifecycle_event".to_string(), "dev".to_string()),
                ("npm_lifecycle_script".to_string(), "vite".to_string()),
            ],
            origin: Origin {
                ancestors: Vec::new(),
                parent_gone: Some(17616),
                launcher: Some(Launcher {
                    name: "Claude Code".to_string(),
                    exe: Some("claude.exe".to_string()),
                    pid: Some(17616),
                    alive: false,
                    evidence: Evidence::Env(vec!["CLAUDE_PID".to_string(), "CLAUDECODE".to_string()]),
                }),
                descendants: vec![ProcRef { pid: 5, exe_name: "esbuild.exe".to_string(), ..ProcRef::default() }],
            },
        }
    }

    fn value(sections: &[Section], kind: SectionKind, label: &str) -> Option<String> {
        sections
            .iter()
            .find(|s| s.kind == kind)?
            .rows
            .iter()
            .find(|r| r.label == label)
            .map(|r| r.value.clone())
    }

    fn alive() -> Details {
        Details { alive: true, ..Details::default() }
    }

    #[test]
    fn report_has_the_five_tabs_in_order() {
        let kinds: Vec<SectionKind> = build(&orphan(), &alive(), Lang::PtBr).iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SectionKind::General,
                SectionKind::Origin,
                SectionKind::Performance,
                SectionKind::Network,
                SectionKind::Environment
            ]
        );
    }

    #[test]
    fn working_directory_is_a_general_field() {
        let sections = build(&orphan(), &alive(), Lang::PtBr);
        assert_eq!(
            value(&sections, SectionKind::General, "Diretório de trabalho (cwd)").as_deref(),
            Some(r"C:\Users\Pichau\sandbox\game-visual")
        );
    }

    #[test]
    fn orphan_origin_names_the_dead_parent_and_the_evidence() {
        let sections = build(&orphan(), &alive(), Lang::PtBr);
        let get = |label| value(&sections, SectionKind::Origin, label);
        assert_eq!(get("Iniciado por").as_deref(), Some("Claude Code — claude.exe, PID 17616 (encerrado)"));
        assert_eq!(get("Evidência").as_deref(), Some("variáveis de ambiente: CLAUDE_PID, CLAUDECODE"));
        assert_eq!(get("Processo pai").as_deref(), Some("<encerrado> (PID 17616)"));
        assert_eq!(get("Órfão").as_deref(), Some("Sim"));
        assert_eq!(get("Script npm").as_deref(), Some("dev → vite"));
        assert_eq!(get("Sessão do Claude Code").as_deref(), Some("abc-123"));
    }

    #[test]
    fn english_uses_process_explorer_wording() {
        let sections = build(&orphan(), &alive(), Lang::En);
        assert_eq!(
            value(&sections, SectionKind::Origin, "Parent").as_deref(),
            Some("<Non-existent process> (PID 17616)")
        );
        assert_eq!(
            value(&sections, SectionKind::General, "Current directory (cwd)").as_deref(),
            Some(r"C:\Users\Pichau\sandbox\game-visual")
        );
    }

    #[test]
    fn lineage_table_reads_top_down() {
        let mut p = orphan();
        p.origin.parent_gone = None;
        p.origin.ancestors = vec![
            ProcRef { pid: 90, exe_name: "cmd.exe".to_string(), ..ProcRef::default() },
            ProcRef { pid: 80, exe_name: "claude.exe".to_string(), ..ProcRef::default() },
        ];
        let d = Details { ancestors: p.origin.ancestors.clone(), descendants: p.origin.descendants.clone(), ..alive() };
        let sections = build(&p, &d, Lang::PtBr);
        let table = sections.iter().find(|s| s.kind == SectionKind::Origin).unwrap().table.clone().unwrap();
        let firsts: Vec<(String, String)> =
            table.rows.iter().map(|r| (r.cells[0].clone(), r.cells[1].clone())).collect();
        assert_eq!(
            firsts,
            vec![
                ("Ancestral 2".to_string(), "80".to_string()),
                ("Pai".to_string(), "90".to_string()),
                ("Este processo".to_string(), "16836".to_string()),
                ("Descendente".to_string(), "5".to_string()),
            ]
        );
    }

    #[test]
    fn priority_uses_task_manager_names() {
        let d = Details { priority: Some(Priority::BelowNormal), ..alive() };
        let sections = build(&orphan(), &d, Lang::PtBr);
        assert_eq!(value(&sections, SectionKind::Performance, "Prioridade").as_deref(), Some("Abaixo do normal"));
    }

    #[test]
    fn text_masks_credentials_unless_asked() {
        let sections = build(&orphan(), &alive(), Lang::PtBr);
        let masked = to_text("x", &sections, false);
        assert!(masked.contains(MASK));
        assert!(!masked.contains("ghp_secret"));
        assert!(masked.contains("abc-123"), "a session id is not a credential");
        assert!(to_text("x", &sections, true).contains("ghp_secret"));
    }

    #[test]
    fn many_environment_hits_collapse_into_one_label() {
        let env = |name: &str| Hit::Env(name.to_string());
        let hits = vec![Hit::Cwd, env("A"), env("B"), env("C"), env("D"), env("E"), Hit::Launcher];
        assert_eq!(
            hits_summary(&hits, Lang::PtBr),
            "Diretório de trabalho, Ambiente: A, B, C (+2), Iniciado por"
        );
        assert_eq!(hits_summary(&[env("PATH")], Lang::En), "Environment: PATH");
    }

    #[test]
    fn path_leads_the_environment_label() {
        let env = |name: &str| Hit::Env(name.to_string());
        let hits = vec![env("NODE"), env("NODE_EXE"), env("npm_config_prefix"), env("Path")];
        assert_eq!(hits_summary(&hits, Lang::PtBr), "Ambiente: Path, NODE, NODE_EXE (+1)");
    }

    #[test]
    fn hit_labels_in_both_languages() {
        assert_eq!(hit_label(&Hit::Cwd, Lang::PtBr), "Diretório de trabalho");
        assert_eq!(hit_label(&Hit::Env("PATH".to_string()), Lang::PtBr), "Ambiente: PATH");
        assert_eq!(hit_label(&Hit::Ancestor("claude.exe".to_string()), Lang::En), "Origin: claude.exe");
        assert_eq!(hit_label(&Hit::Descendant("esbuild.exe".to_string()), Lang::PtBr), "Filho: esbuild.exe");
    }
}
