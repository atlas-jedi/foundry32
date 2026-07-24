//! Deriving a human name and an on-disk location for a node process from what
//! we could read: the `package.json` in its working directory (the *project*)
//! and the script/tool on its command line (the *task*). Falls back gracefully
//! and never returns an empty name.
//!
//! Two subtleties this handles:
//! - A tool run out of `node_modules` is named by its package (`@playwright/mcp`,
//!   `vite`), not by the shim file the path points at — after lexically
//!   resolving `.`/`..` (npx paths look like `.../node_modules/.bin/../@scope/x`).
//! - A process often *inherits* its cwd from whatever launched it — e.g. the MCP
//!   servers Claude Code spawns inherit its cwd, which happens to be a Rust repo
//!   with no `package.json`. So the cwd counts as a project ONLY when it has a
//!   `package.json`; its bare folder name is never used as a project, since that
//!   is exactly what made an inherited cwd look like a JS app it isn't.

use std::path::{Path, PathBuf};

/// Best available human name: `project · task`, then project, then task, then
/// the exe file name, then a literal `node.exe`.
pub fn derive(exe_path: Option<&Path>, cmdline: Option<&str>, cwd: Option<&Path>) -> String {
    let script = cmdline.and_then(script_arg).map(|s| normalize(&s));
    let project = cwd.and_then(project_name);
    let task = script
        .as_deref()
        .and_then(|s| package_in_node_modules(s).or_else(|| task_from_script(s)));

    match (project, task) {
        (Some(p), Some(t)) => format!("{p} · {t}"),
        (Some(p), None) => p,
        (None, Some(t)) => t,
        (None, None) => exe_path
            .and_then(Path::file_name)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "node.exe".to_string()),
    }
}

/// Where the app actually lives on disk — the thing you'd want to open, not the
/// shared `node.exe`.
///
/// - A user's own script (`node dist/main.js`) → that file, resolved absolute.
/// - A tool from a project's `node_modules` (`next`, `vite`) or `node .` → the
///   project directory (the cwd).
/// - A tool from a global / npx-cache `node_modules` under an inherited cwd →
///   where the tool's code actually sits (the cwd would be misleading).
pub fn app_location(cmdline: Option<&str>, cwd: Option<&Path>) -> Option<PathBuf> {
    let script = cmdline.and_then(script_arg).map(|s| normalize(&s));
    let has_project = cwd.is_some_and(|c| project_name(c).is_some());

    if let Some(script) = script.as_deref() {
        let is_pkg = package_in_node_modules(script).is_some();
        let is_dot = script == "." || script == "..";
        if !is_pkg && !is_dot {
            // A concrete script file: show it, resolved to an absolute path.
            return Some(resolve(script, cwd));
        }
        if is_pkg && !has_project {
            // A tool under an inherited cwd → where its code sits.
            return Some(to_pathbuf(script));
        }
    }
    cwd.map(Path::to_path_buf)
}

/// Project name: `name` from `<cwd>/package.json`. Deliberately does NOT fall
/// back to the folder name — a directory without a `package.json` is not a
/// JS project, and pretending it is was the whole bug.
fn project_name(cwd: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cwd.join("package.json")).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    let name = json.get("name")?.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// The script/entry argument of a node command line: the first non-flag token
/// after node itself, skipping flags — including the ones (`-e`, `-r`, …) whose
/// value is inline code or a module, not a script. `None` for a bare `node`
/// (REPL) or a `node -e '<code>'` with no script.
fn script_arg(cmdline: &str) -> Option<String> {
    let tokens = tokenize(cmdline);
    let mut i = 1; // skip node itself
    while let Some(tok) = tokens.get(i) {
        if is_value_flag(tok) {
            i += 2; // skip the flag AND the code/module it carries
        } else if tok.starts_with('-') {
            i += 1; // a valueless flag
        } else {
            return Some(tok.clone());
        }
    }
    None
}

/// node flags whose following token is inline code or a module — not the app's
/// script: `-e`/`--eval`, `-p`/`--print`, `-r`/`--require`, `--import`.
fn is_value_flag(tok: &str) -> bool {
    matches!(
        tok,
        "-e" | "--eval" | "-p" | "--print" | "-r" | "--require" | "--import"
    )
}

/// Package name if the (normalized) script path runs through a `node_modules`:
/// `@scope/name` for scoped packages, the binary name for a `.bin/<x>` entry,
/// and the plain package name otherwise.
fn package_in_node_modules(script: &str) -> Option<String> {
    let segments: Vec<&str> = script.split('/').filter(|s| !s.is_empty()).collect();
    let nm = segments
        .iter()
        .rposition(|&s| s.eq_ignore_ascii_case("node_modules"))?;
    let first = *segments.get(nm + 1)?;
    if first.eq_ignore_ascii_case(".bin") {
        return segments.get(nm + 2).map(|s| strip_ext(s));
    }
    if first.starts_with('@') {
        let second = segments.get(nm + 2)?;
        return Some(format!("{first}/{second}"));
    }
    Some(first.to_string())
}

/// Task label for a plain user script: its file name without extension. `None`
/// for `.`/`..` (a bare `node .`), where the project name already suffices.
fn task_from_script(script: &str) -> Option<String> {
    let base = script.rsplit('/').next().unwrap_or(script);
    if base.is_empty() || base == "." || base == ".." {
        None
    } else {
        Some(strip_ext(base))
    }
}

/// Resolves a script path to an absolute `PathBuf` (a relative one against the
/// cwd), with Windows separators.
fn resolve(script: &str, cwd: Option<&Path>) -> PathBuf {
    let path = to_pathbuf(script);
    if path.is_absolute() {
        path
    } else if let Some(dir) = cwd {
        dir.join(script.replace('/', "\\"))
    } else {
        path
    }
}

/// Lexically resolves `.` and `..` in a path and returns it with `/`
/// separators. No filesystem access — the path may belong to another process.
fn normalize(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    for seg in unified.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(seg),
        }
    }
    out.join("/")
}

fn to_pathbuf(normalized: &str) -> PathBuf {
    PathBuf::from(normalized.replace('/', "\\"))
}

fn strip_ext(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _ext)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

/// Minimal command-line tokenizer: splits on whitespace, honoring double
/// quotes. Enough to pull the script argument out of a node command line.
fn tokenize(cmdline: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in cmdline.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}
