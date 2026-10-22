//! The "search everything" box: one term, looked up in every place a node
//! process can be tied to it — where its code lives, the folder it is sitting
//! in, its command line and script, its node.exe, its environment (PATH and
//! the rest), who started it, and the processes above and below it.
//!
//! Matching is case-insensitive and treats `/` and `\` as the same character,
//! so a path pasted from either world matches. `*` and `?` are wildcards over
//! the whole field (`*server.js` = ends with `server.js`); without them the
//! term matches anywhere inside a field, so `*game-visual*` and `game-visual`
//! find the same processes. A bare number also matches the PID, the parent
//! PID and the listening ports exactly; `:3939` asks for a port.
//!
//! Each hit says *where* the term was found, because "it matched" is useless
//! when the question is why a folder is locked.

use crate::model::NodeProc;

/// Where in a process the term was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    App,
    Pid,
    ParentPid,
    Port,
    AppLocation,
    Cwd,
    CommandLine,
    NodeExe,
    /// An environment variable, by name, whose name or value matched.
    Env(String),
    Launcher,
    /// An ancestor process, by exe name.
    Ancestor(String),
    /// A descendant process, by exe name.
    Descendant(String),
}

/// A parsed search term.
#[derive(Clone, Debug)]
pub struct Query {
    text: String,
    /// The term normalized for comparison (lowercase, `/` → `\`).
    pattern: Vec<char>,
    /// Whether `pattern` holds `*`/`?` and must match a whole field.
    wildcard: bool,
    /// The term as a number, for the exact PID / parent / port comparison.
    number: Option<u32>,
    /// `:3939` — the number is only a port.
    port_only: bool,
}

impl Query {
    /// `None` for a blank term — no search, the full list.
    pub fn parse(text: &str) -> Option<Query> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let port_only = text.starts_with(':');
        let number = text.trim_start_matches(':').parse::<u32>().ok();
        let pattern: Vec<char> = normalize(text).chars().collect();
        let wildcard = pattern.iter().any(|c| matches!(c, '*' | '?'));
        Some(Query { text: text.to_string(), pattern, wildcard, number, port_only })
    }

    /// The term as typed (trimmed), for the status bar.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether a text field matches: a whole-field wildcard match, or the
    /// term found anywhere inside it.
    fn hits_text(&self, field: &str) -> bool {
        let field: Vec<char> = normalize(field).chars().collect();
        if self.wildcard {
            wildcard_match(&self.pattern, &field)
        } else {
            field.windows(self.pattern.len()).any(|window| window == self.pattern.as_slice())
        }
    }
}

/// Every place in `p` the query matches, in a fixed order (identity first,
/// then location, then environment and lineage), without repeats.
pub fn matches(p: &NodeProc, q: &Query) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut add = |hit: Hit| {
        if !hits.contains(&hit) {
            hits.push(hit);
        }
    };
    let path_hit = |path: &Option<std::path::PathBuf>| {
        path.as_ref().is_some_and(|path| q.hits_text(&path.to_string_lossy()))
    };

    if q.hits_text(&p.app_name) {
        add(Hit::App);
    }
    if let Some(number) = q.number {
        if !q.port_only && number == p.pid {
            add(Hit::Pid);
        }
        if !q.port_only && number == p.ppid {
            add(Hit::ParentPid);
        }
        if p.ports.iter().any(|&port| u32::from(port) == number) {
            add(Hit::Port);
        }
    }
    if path_hit(&p.app_location()) {
        add(Hit::AppLocation);
    }
    if path_hit(&p.cwd) {
        add(Hit::Cwd);
    }
    if p.cmdline.as_deref().is_some_and(|cmdline| q.hits_text(cmdline)) {
        add(Hit::CommandLine);
    }
    if path_hit(&p.exe_path) {
        add(Hit::NodeExe);
    }
    for (name, value) in &p.env {
        if q.hits_text(name) || q.hits_text(value) {
            add(Hit::Env(name.clone()));
        }
    }
    if let Some(launcher) = &p.origin.launcher {
        if q.hits_text(&launcher.name) || launcher.exe.as_deref().is_some_and(|exe| q.hits_text(exe)) {
            add(Hit::Launcher);
        }
    }
    let lineage_hit = |r: &crate::model::ProcRef| {
        q.hits_text(&r.exe_name) || r.cmdline.as_deref().is_some_and(|cmdline| q.hits_text(cmdline))
    };
    for ancestor in p.origin.ancestors.iter().filter(|r| lineage_hit(r)) {
        add(Hit::Ancestor(ancestor.exe_name.clone()));
    }
    for descendant in p.origin.descendants.iter().filter(|r| lineage_hit(r)) {
        add(Hit::Descendant(descendant.exe_name.clone()));
    }
    hits
}

/// Lowercase, with `/` folded onto `\` — Windows paths compare either way.
fn normalize(text: &str) -> String {
    text.to_lowercase().replace('/', "\\")
}

/// `*` = any run of characters (including none), `?` = exactly one; the
/// pattern must cover the whole text. Greedy with a single backtrack point,
/// linear in practice.
fn wildcard_match(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((star_p, star_t)) => {
                    p = star_p + 1;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Launcher, Evidence, Origin, ProcRef};
    use std::path::PathBuf;

    /// The process from the 22/10 incident: an orphaned MCP server sitting in
    /// the folder that could not be deleted.
    fn excalidraw() -> NodeProc {
        NodeProc {
            pid: 16836,
            ppid: 17616,
            exe_path: Some(PathBuf::from(r"C:\nvm4w\nodejs\node.exe")),
            cmdline: Some(
                r"C:\nvm4w\nodejs\node.exe C:\Users\Pichau\AppData\Local\npm-cache\_npx\1149d391a9f6528a\node_modules\mcp-excalidraw-server\dist\server.js"
                    .to_string(),
            ),
            cwd: Some(PathBuf::from(r"C:\Users\Pichau\sandbox\Game-Visual")),
            app_name: "mcp-excalidraw-server".to_string(),
            ports: vec![3939],
            mem_bytes: 0,
            cpu_percent: 0.0,
            uptime_secs: 0,
            start_filetime: 1,
            depth: 0,
            env: vec![
                ("PATH".to_string(), r"C:\Windows;C:\nvm4w\nodejs".to_string()),
                ("CLAUDECODE".to_string(), "1".to_string()),
            ],
            origin: Origin {
                ancestors: Vec::new(),
                parent_gone: Some(17616),
                launcher: Some(Launcher {
                    name: "Claude Code".to_string(),
                    exe: Some("claude.exe".to_string()),
                    pid: Some(17616),
                    alive: false,
                    evidence: Evidence::Env(vec!["CLAUDECODE".to_string()]),
                }),
                descendants: vec![ProcRef { pid: 5, exe_name: "esbuild.exe".to_string(), ..ProcRef::default() }],
            },
        }
    }

    fn hits(term: &str) -> Vec<Hit> {
        matches(&excalidraw(), &Query::parse(term).expect("a query"))
    }

    #[test]
    fn blank_term_is_no_search() {
        assert!(Query::parse("   ").is_none());
        assert!(Query::parse("").is_none());
    }

    #[test]
    fn folder_name_finds_the_working_directory_case_insensitively() {
        assert_eq!(hits("game-visual"), vec![Hit::Cwd]);
    }

    #[test]
    fn surrounding_stars_mean_anywhere() {
        assert_eq!(hits("*game-visual*"), vec![Hit::Cwd]);
    }

    #[test]
    fn forward_slashes_match_backslashes() {
        assert_eq!(hits("sandbox/game-visual"), vec![Hit::Cwd]);
    }

    #[test]
    fn wildcards_match_the_whole_field() {
        assert_eq!(hits("*server.js"), vec![Hit::AppLocation, Hit::CommandLine]);
        assert!(hits("server.*").is_empty());
        assert_eq!(hits("mcp-excalidraw-?erver"), vec![Hit::App]);
    }

    #[test]
    fn script_name_finds_location_and_command_line() {
        assert_eq!(hits("server.js"), vec![Hit::AppLocation, Hit::CommandLine]);
    }

    #[test]
    fn environment_hits_name_the_variable() {
        assert_eq!(hits("nvm4w"), vec![Hit::CommandLine, Hit::NodeExe, Hit::Env("PATH".to_string())]);
        assert_eq!(hits("claudecode"), vec![Hit::Env("CLAUDECODE".to_string())]);
    }

    #[test]
    fn numbers_match_pid_parent_and_port_exactly() {
        assert_eq!(hits("16836"), vec![Hit::Pid]);
        assert_eq!(hits("17616"), vec![Hit::ParentPid]);
        assert_eq!(hits("3939"), vec![Hit::Port]);
        assert_eq!(hits(":3939"), vec![Hit::Port]);
        assert!(hits("1683").is_empty());
    }

    #[test]
    fn launcher_and_lineage_are_searched() {
        assert_eq!(hits("claude code"), vec![Hit::Launcher]);
        assert_eq!(hits("esbuild"), vec![Hit::Descendant("esbuild.exe".to_string())]);
    }

    #[test]
    fn ancestors_are_searched_by_exe_name() {
        let mut p = excalidraw();
        p.origin.ancestors =
            vec![ProcRef { pid: 9, exe_name: "WindowsTerminal.exe".to_string(), ..ProcRef::default() }];
        let found = matches(&p, &Query::parse("terminal").unwrap());
        assert_eq!(found, vec![Hit::Ancestor("WindowsTerminal.exe".to_string())]);
    }
}
