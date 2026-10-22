//! The domain model: one `node.exe` process as WITN sees it, plus the small
//! human-facing formatting the CLI and GUI share.

use std::path::PathBuf;

pub use foundry_common::lang::Lang;

/// A single `node.exe` process with everything WITN could learn about it.
///
/// `cmdline` and `cwd` are `Option` on purpose: they require reading another
/// process's memory (its PEB), which can be unavailable when the target runs
/// under a different CPU architecture than WITN, or when access is denied.
/// Every other field is obtainable for any of the user's own processes without
/// elevation.
#[derive(Clone, Debug)]
pub struct NodeProc {
    pub pid: u32,
    pub ppid: u32,
    /// Full path to the node.exe image (e.g. `C:\Program Files\nodejs\node.exe`).
    pub exe_path: Option<PathBuf>,
    /// Full command line, when readable — the key to naming the app.
    pub cmdline: Option<String>,
    /// Working directory, when readable — where we look for `package.json`.
    pub cwd: Option<PathBuf>,
    /// Friendly, derived name (`my-api · vite`, `server.js`, …). Never empty.
    pub app_name: String,
    /// TCP ports this PID is LISTENING on — ascending, de-duplicated.
    pub ports: Vec<u16>,
    /// Working-set size in bytes.
    pub mem_bytes: u64,
    /// CPU usage percent across all cores since the previous sample (0 on the
    /// first sample — it needs two points in time).
    pub cpu_percent: f32,
    /// Seconds since the process started.
    pub uptime_secs: u64,
    /// Process creation time as a Windows FILETIME (100 ns since 1601). Used to
    /// order the tree and to reject PID-recycled false parents.
    pub start_filetime: u64,
    /// Depth in the process forest — 0 for a project root, deeper for children
    /// it spawned. Drives the GUI/CLI indentation.
    pub depth: usize,
    /// Environment variables, in block order; empty when the PEB is unreadable
    /// (same conditions as `cmdline`/`cwd`).
    pub env: Vec<(String, String)>,
    /// Who started this process and what it started — see [`Origin`].
    pub origin: Origin,
}

/// Another process (any exe, not only node) as seen from a node's lineage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcRef {
    pub pid: u32,
    pub exe_name: String,
    /// Creation FILETIME, 0 when the process could not be opened.
    pub start_filetime: u64,
    /// Only filled by `details::inspect` — reading it costs a PEB read per
    /// process, which the 3-second scan does not pay for every ancestor.
    pub cmdline: Option<String>,
}

/// Where a node process came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Origin {
    /// Parent first, then the parent's parent, …, up to the first break.
    pub ancestors: Vec<ProcRef>,
    /// The PID of this process's own parent when that parent no longer exists
    /// (or the PID now belongs to a younger process). `Some` = orphan.
    pub parent_gone: Option<u32>,
    /// The program a person would say "started it" — see `origin::launcher`.
    pub launcher: Option<Launcher>,
    /// Every process below this one (breadth-first), node or not.
    pub descendants: Vec<ProcRef>,
}

/// The app behind a node process, e.g. "Claude Code" for a `claude.exe`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    /// Friendly product name (`Claude Code`, `VS Code`) or the exe name.
    pub name: String,
    /// The launcher's exe file name, when known.
    pub exe: Option<String>,
    pub pid: Option<u32>,
    /// Whether that PID is still the launcher's (not exited, not recycled).
    pub alive: bool,
    pub evidence: Evidence,
}

/// How the launcher was established — shown next to it, so a guess is never
/// presented as a fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// It is a live process in the ancestor chain.
    Ancestor,
    /// The chain is broken; the named environment variables point at it.
    Env(Vec<String>),
}

impl NodeProc {
    /// Where the app's code lives (see `appname::app_location`), falling back
    /// to the node.exe it runs on. The one definition the list column, the
    /// "Open location" action, the CLI and the search all share.
    pub fn app_location(&self) -> Option<PathBuf> {
        crate::appname::app_location(self.cmdline.as_deref(), self.cwd.as_deref())
            .or_else(|| self.exe_path.clone())
    }

    /// Working set in whole mebibytes, for compact display.
    pub fn mem_mib(&self) -> u64 {
        self.mem_bytes / (1024 * 1024)
    }

    /// Comma-separated listening ports, or `—` when none.
    pub fn ports_label(&self) -> String {
        if self.ports.is_empty() {
            "—".to_string()
        } else {
            self.ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// Formats an uptime in seconds as a compact human string: `45s`, `12m03s`,
/// `3h07m`, `2d04h`.
pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let s = secs % 60;
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Whether an environment variable is likely to hold a credential, so the
/// details view masks it by default. Judged by the name's `_`-separated words
/// (`GITHUB_TOKEN`, `DB_PASS`, `OPENAI_API_KEY`) — whole words, so
/// `GIT_AUTHOR_NAME` is not caught by `AUTH` — and by a value that is a URL
/// carrying `user:password@`.
pub fn is_sensitive_env(name: &str, value: &str) -> bool {
    const WORDS: [&str; 16] = [
        "TOKEN", "TOKENS", "SECRET", "SECRETS", "PASSWORD", "PASSWD", "PASS", "PASSPHRASE", "KEY",
        "APIKEY", "PRIVATE", "AUTH", "CREDENTIAL", "CREDENTIALS", "COOKIE", "PAT",
    ];
    // Names written as one word (`PGPASSWORD`, `GITHUBTOKEN`) have no `_` to
    // split on; these suffixes catch them.
    const SUFFIXES: [&str; 5] = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "APIKEY"];
    let upper = name.to_ascii_uppercase();
    upper.split('_').any(|word| WORDS.contains(&word))
        || SUFFIXES.iter().any(|suffix| upper.ends_with(suffix))
        || url_has_credentials(value)
}

/// `scheme://user:password@host…` — a `:` and an `@` inside the authority (the
/// part between `://` and the next `/`).
fn url_has_credentials(value: &str) -> bool {
    let Some((_, rest)) = value.split_once("://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    authority
        .split_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'))
}

/// A wall-clock moment in the user's local time zone, already converted from a
/// FILETIME (see `details::local_time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

/// `22/10/2026 07:51:35` in Portuguese, `2026-10-22 07:51:35` in English —
/// each language's everyday order, never a locale-dependent API call.
pub fn format_datetime(t: &LocalTime, lang: Lang) -> String {
    let time = format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
    match lang {
        Lang::PtBr => format!("{:02}/{:02}/{} {time}", t.day, t.month, t.year),
        Lang::En => format!("{}-{:02}-{:02} {time}", t.year, t.month, t.day),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_datetime, is_sensitive_env, Lang, LocalTime};

    #[test]
    fn datetime_follows_each_language_order() {
        let t = LocalTime { year: 2026, month: 10, day: 22, hour: 7, minute: 51, second: 5 };
        assert_eq!(format_datetime(&t, Lang::PtBr), "22/10/2026 07:51:05");
        assert_eq!(format_datetime(&t, Lang::En), "2026-10-22 07:51:05");
    }

    #[test]
    fn credential_words_in_the_name_are_sensitive() {
        for name in [
            "GITHUB_TOKEN",
            "CLAUDE_CODE_MESSAGING_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "OPENAI_API_KEY",
            "DB_PASS",
            "PGPASSWORD",
            "npm_config__auth",
            "GH_PAT",
        ] {
            assert!(is_sensitive_env(name, "x"), "{name} should be masked");
        }
    }

    #[test]
    fn ordinary_names_are_not_sensitive() {
        for name in ["PATH", "NODE_ENV", "GIT_AUTHOR_NAME", "CLAUDE_CODE_SESSION_ID", "MONKEY_COUNT", "PWD"] {
            assert!(!is_sensitive_env(name, "x"), "{name} should be shown");
        }
    }

    #[test]
    fn url_with_inline_credentials_is_sensitive() {
        assert!(is_sensitive_env("DATABASE_URL", "postgres://app:hunter2@db:5432/main"));
        assert!(!is_sensitive_env("DATABASE_URL", "postgres://db:5432/main"));
        assert!(!is_sensitive_env("HOMEPAGE", "https://example.com/a@b"));
    }
}
