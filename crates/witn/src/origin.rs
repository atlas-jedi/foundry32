//! Where a node process came from: the chain of processes above it, the one a
//! person would say "started it", and everything it started in turn.
//!
//! Built from one process snapshot (`proctree::all_processes`) plus each
//! ancestor's start time, which is what separates a real parent from a PID
//! Windows recycled after the real parent exited — the same rule `tree::build`
//! applies between node processes.
//!
//! When the chain is broken (the parent exited — an orphan, like an MCP server
//! that outlived the Claude Code session that spawned it), the environment
//! still remembers who started it: launchers stamp variables such as
//! `CLAUDE_PID` into their children. Those are reported as evidence of a
//! different kind, so a clue is never shown as if it were a live parent.

use std::collections::{HashMap, HashSet};

use crate::model::{Evidence, Launcher, Origin, ProcRef};
use crate::proctree::{self, ProcEntry};

/// Longest ancestor chain followed. Real chains are 3–6 deep; the cap only
/// guards against a pathological snapshot.
const MAX_DEPTH: usize = 12;

/// The System process: everything kernel-started hangs off it, and it is never
/// what a person means by "who started this".
const SYSTEM_PID: u32 = 4;

/// Walks parent links up from `pid`. Returns the ancestors (parent first) and,
/// when the process's own parent is missing or recycled, that parent's PID.
/// `start_of` gives a process's creation FILETIME, 0 when unknown — an
/// unknown time is accepted (it is usually an elevated or system process, older
/// than anything the user runs).
pub fn ancestors(
    entries: &[ProcEntry],
    pid: u32,
    start_ft: u64,
    start_of: &dyn Fn(u32) -> u64,
) -> (Vec<ProcRef>, Option<u32>) {
    let by_pid: HashMap<u32, &ProcEntry> = entries.iter().map(|e| (e.pid, e)).collect();
    let mut chain = Vec::new();
    let mut seen = HashSet::from([pid]);
    let (mut current, mut current_start) = (pid, start_ft);
    while chain.len() < MAX_DEPTH {
        let Some(entry) = by_pid.get(&current) else { break };
        let parent = entry.ppid;
        if parent == 0 || parent == SYSTEM_PID || !seen.insert(parent) {
            break;
        }
        // A missing or recycled parent ends the walk. It makes the process an
        // orphan only at the first step: higher up, a dead parent is routine
        // (Explorer's own parent exits right after logon).
        let orphaned = |chain: Vec<ProcRef>| {
            let gone = chain.is_empty().then_some(parent);
            (chain, gone)
        };
        let Some(parent_entry) = by_pid.get(&parent) else {
            return orphaned(chain);
        };
        let parent_start = start_of(parent);
        if parent_start != 0 && current_start != 0 && parent_start > current_start {
            return orphaned(chain); // the PID now belongs to a younger process
        }
        chain.push(ProcRef {
            pid: parent,
            exe_name: parent_entry.exe_name.clone(),
            start_filetime: parent_start,
            cmdline: None,
        });
        current = parent;
        current_start = parent_start;
    }
    (chain, None)
}

/// The first ancestor that is not a shell, console host or another node
/// (those only relay a launch), named the way its user knows it. Without one,
/// launcher variables left in the environment are used as evidence.
pub fn launcher(
    ancestors: &[ProcRef],
    env: &[(String, String)],
    is_alive: &dyn Fn(u32) -> bool,
) -> Option<Launcher> {
    if let Some(found) = ancestors.iter().find(|p| !is_relay(&p.exe_name)) {
        return Some(Launcher {
            name: friendly_name(&found.exe_name),
            exe: Some(found.exe_name.clone()),
            pid: Some(found.pid),
            alive: true,
            evidence: Evidence::Ancestor,
        });
    }
    from_environment(env, is_alive)
}

/// Processes that only pass a launch along: shells, console hosts, and node
/// itself (npm, npx and every dev-server wrapper are node).
fn is_relay(exe: &str) -> bool {
    const RELAYS: [&str; 9] = [
        "node.exe", "cmd.exe", "conhost.exe", "openconsole.exe", "powershell.exe", "pwsh.exe",
        "bash.exe", "sh.exe", "wsl.exe",
    ];
    RELAYS.iter().any(|relay| exe.eq_ignore_ascii_case(relay))
}

/// The product name people use for a launcher exe; unknown exes keep their
/// file name.
fn friendly_name(exe: &str) -> String {
    const KNOWN: [(&str, &str); 11] = [
        ("claude.exe", "Claude Code"),
        ("code.exe", "VS Code"),
        ("code - insiders.exe", "VS Code Insiders"),
        ("cursor.exe", "Cursor"),
        ("windsurf.exe", "Windsurf"),
        ("antigravity.exe", "Antigravity"),
        ("windowsterminal.exe", "Windows Terminal"),
        ("explorer.exe", "Explorer"),
        ("idea64.exe", "IntelliJ IDEA"),
        ("webstorm64.exe", "WebStorm"),
        ("witn-gui.exe", "WITN"),
    ];
    KNOWN
        .iter()
        .find(|(known, _)| exe.eq_ignore_ascii_case(known))
        .map_or_else(|| exe.to_string(), |(_, name)| name.to_string())
}

/// Launcher clues left in the environment, innermost launcher first: Claude
/// Code usually runs inside a terminal or an editor, and is the nearer cause.
fn from_environment(env: &[(String, String)], is_alive: &dyn Fn(u32) -> bool) -> Option<Launcher> {
    let get = |name: &str| {
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    // (product, exe, variable holding its PID, every variable that betrays it)
    let rules: [(&str, &str, Option<&str>, &[&str]); 3] = [
        ("Claude Code", "claude.exe", Some("CLAUDE_PID"), &["CLAUDE_PID", "CLAUDECODE"]),
        ("VS Code", "Code.exe", Some("VSCODE_PID"), &["VSCODE_PID", "TERM_PROGRAM"]),
        ("Windows Terminal", "WindowsTerminal.exe", None, &["WT_SESSION"]),
    ];
    for (name, exe, pid_var, markers) in rules {
        let present: Vec<String> = markers
            .iter()
            .filter(|marker| match get(marker) {
                // TERM_PROGRAM names whichever terminal; only `vscode` is a clue.
                Some(value) if **marker == "TERM_PROGRAM" => value.eq_ignore_ascii_case("vscode"),
                Some(_) => true,
                None => false,
            })
            .map(|marker| marker.to_string())
            .collect();
        if present.is_empty() {
            continue;
        }
        let pid = pid_var.and_then(get).and_then(|value| value.trim().parse::<u32>().ok());
        return Some(Launcher {
            name: name.to_string(),
            exe: Some(exe.to_string()),
            pid,
            alive: pid.is_some_and(is_alive),
            evidence: Evidence::Env(present),
        });
    }
    None
}

/// Every process below `pid`, breadth-first, without `pid` itself.
pub fn descendants(entries: &[ProcEntry], pid: u32) -> Vec<ProcRef> {
    proctree::subtree(entries, pid)
        .into_iter()
        .skip(1)
        .map(|entry| ProcRef {
            pid: entry.pid,
            exe_name: entry.exe_name,
            ..ProcRef::default()
        })
        .collect()
}

/// The whole [`Origin`] of one process, from a snapshot taken once per scan.
pub fn origin_of(
    entries: &[ProcEntry],
    pid: u32,
    start_ft: u64,
    env: &[(String, String)],
    start_of: &dyn Fn(u32) -> u64,
) -> Origin {
    let (chain, parent_gone) = ancestors(entries, pid, start_ft, start_of);
    let present: HashSet<u32> = entries.iter().map(|e| e.pid).collect();
    // A launcher PID counts as alive only if it still belongs to a process at
    // least as old as this one — otherwise it is someone else's recycled PID.
    let is_alive = |candidate: u32| {
        present.contains(&candidate) && {
            let started = start_of(candidate);
            started == 0 || start_ft == 0 || started <= start_ft
        }
    };
    Origin {
        launcher: launcher(&chain, env, &is_alive),
        ancestors: chain,
        parent_gone,
        descendants: descendants(entries, pid),
    }
}

/// Builds a memoizing `start_of` for one scan: many node processes share the
/// same ancestors, and each lookup opens the process.
pub fn start_time_cache(lookup: impl Fn(u32) -> u64) -> impl Fn(u32) -> u64 {
    let cache = std::cell::RefCell::new(HashMap::new());
    move |pid| *cache.borrow_mut().entry(pid).or_insert_with(|| lookup(pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pid: u32, ppid: u32, exe: &str) -> ProcEntry {
        ProcEntry { pid, ppid, exe_name: exe.to_string(), threads: 1 }
    }

    fn starts(table: &[(u32, u64)]) -> impl Fn(u32) -> u64 + '_ {
        move |pid| table.iter().find(|(p, _)| *p == pid).map_or(0, |(_, t)| *t)
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn lineage() -> Vec<ProcEntry> {
        vec![
            entry(70, 1, "WindowsTerminal.exe"),
            entry(80, 70, "claude.exe"),
            entry(90, 80, "cmd.exe"),
            entry(100, 90, "node.exe"),
            entry(110, 100, "esbuild.exe"),
        ]
    }

    #[test]
    fn ancestors_follow_parents_up_to_the_root() {
        let times = [(70, 10), (80, 20), (90, 30), (100, 40)];
        let (chain, gone) = ancestors(&lineage(), 100, 40, &starts(&times));
        let pids: Vec<u32> = chain.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![90, 80, 70]);
        assert_eq!(chain[1].exe_name, "claude.exe");
        assert_eq!(gone, None);
    }

    #[test]
    fn ancestors_stop_at_a_parent_that_exited() {
        let entries = vec![entry(100, 17616, "node.exe")];
        let (chain, gone) = ancestors(&entries, 100, 40, &starts(&[(100, 40)]));
        assert!(chain.is_empty());
        assert_eq!(gone, Some(17616));
    }

    #[test]
    fn ancestors_reject_a_recycled_parent_pid() {
        // PID 90 now belongs to a process younger than the node it "parents".
        let times = [(90, 50), (100, 40)];
        let entries = vec![entry(90, 1, "notepad.exe"), entry(100, 90, "node.exe")];
        let (chain, gone) = ancestors(&entries, 100, 40, &starts(&times));
        assert!(chain.is_empty());
        assert_eq!(gone, Some(90));
    }

    #[test]
    fn ancestors_stop_before_the_system_process() {
        let entries = vec![entry(4, 0, "System"), entry(600, 4, "smss.exe"), entry(100, 600, "node.exe")];
        let (chain, gone) = ancestors(&entries, 100, 40, &starts(&[]));
        let pids: Vec<u32> = chain.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![600]);
        assert_eq!(gone, None);
    }

    #[test]
    fn launcher_skips_shells_and_names_claude_code() {
        let times = [(70, 10), (80, 20), (90, 30), (100, 40)];
        let (chain, _) = ancestors(&lineage(), 100, 40, &starts(&times));
        let found = launcher(&chain, &[], &|_| true).expect("a launcher");
        assert_eq!(found.name, "Claude Code");
        assert_eq!(found.exe.as_deref(), Some("claude.exe"));
        assert_eq!(found.pid, Some(80));
        assert!(found.alive);
        assert_eq!(found.evidence, Evidence::Ancestor);
    }

    #[test]
    fn orphan_launcher_comes_from_claude_environment() {
        let vars = env(&[("PATH", "C:\\x"), ("CLAUDECODE", "1"), ("CLAUDE_PID", "17616")]);
        let found = launcher(&[], &vars, &|_| false).expect("a launcher");
        assert_eq!(found.name, "Claude Code");
        assert_eq!(found.pid, Some(17616));
        assert!(!found.alive);
        assert_eq!(
            found.evidence,
            Evidence::Env(vec!["CLAUDE_PID".to_string(), "CLAUDECODE".to_string()])
        );
    }

    #[test]
    fn vscode_environment_is_a_clue_when_no_ancestor_helps() {
        let chain = vec![ProcRef { pid: 90, exe_name: "cmd.exe".to_string(), ..ProcRef::default() }];
        let vars = env(&[("TERM_PROGRAM", "vscode"), ("VSCODE_PID", "5150")]);
        let found = launcher(&chain, &vars, &|pid| pid == 5150).expect("a launcher");
        assert_eq!(found.name, "VS Code");
        assert_eq!(found.pid, Some(5150));
        assert!(found.alive);
    }

    #[test]
    fn only_relays_and_no_clues_means_unknown() {
        let chain = vec![
            ProcRef { pid: 90, exe_name: "cmd.exe".to_string(), ..ProcRef::default() },
            ProcRef { pid: 91, exe_name: "node.exe".to_string(), ..ProcRef::default() },
        ];
        assert_eq!(launcher(&chain, &env(&[("PATH", "C:\\x")]), &|_| true), None);
    }

    #[test]
    fn unknown_launcher_keeps_its_exe_name() {
        let chain = vec![ProcRef { pid: 50, exe_name: "MyTool.exe".to_string(), ..ProcRef::default() }];
        let found = launcher(&chain, &[], &|_| true).expect("a launcher");
        assert_eq!(found.name, "MyTool.exe");
    }

    #[test]
    fn descendants_exclude_the_process_itself() {
        let kids = descendants(&lineage(), 100);
        assert_eq!(kids.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![110]);
    }
}
