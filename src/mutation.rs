//! Applies changes through `claude mcp add/remove` — never hand-edits
//! ~/.claude.json. Always snapshots the affected config files first.

use crate::discovery::claude_json::claude_json_path;
use crate::model::{Scope, Transport};
use crate::winproc::run_captured;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ServerDraft {
    pub name: String,
    pub scope: Scope,
    pub transport: Transport,
    /// stdio: program only. http/sse: the URL.
    pub target: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

fn backups_root() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude").join("backups").join("mcp-hangar")
}

/// Copies ~/.claude.json (and the project's .mcp.json for project scope) into a
/// timestamped backup folder. Keeps the 20 newest backups.
pub fn backup_configs(scope: &Scope) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let dir = create_unique_backup_dir(stamp)?;

    let claude_json = claude_json_path();
    if claude_json.exists() {
        fs::copy(&claude_json, dir.join("claude.json"))
            .map_err(|e| format!("backup claude.json: {e}"))?;
    }
    if let Scope::Project { project_dir } = scope {
        let mcp_json = Path::new(project_dir).join(".mcp.json");
        if mcp_json.exists() {
            fs::copy(&mcp_json, dir.join("mcp.json"))
                .map_err(|e| format!("backup .mcp.json: {e}"))?;
        }
    }
    prune_old_backups();
    Ok(())
}

/// Same-second backups get a `-N` suffix instead of overwriting each other.
fn create_unique_backup_dir(stamp: u64) -> Result<PathBuf, String> {
    let root = backups_root();
    fs::create_dir_all(&root).map_err(|e| format!("create {}: {e}", root.display()))?;
    let mut suffix = 0u32;
    loop {
        let name = if suffix == 0 {
            stamp.to_string()
        } else {
            format!("{stamp}-{suffix}")
        };
        let dir = root.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => suffix += 1,
            Err(e) => return Err(format!("create {}: {e}", dir.display())),
        }
    }
}

fn prune_old_backups() {
    let Ok(entries) = fs::read_dir(backups_root()) else { return };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    while dirs.len() > 20 {
        let oldest = dirs.remove(0);
        let _ = fs::remove_dir_all(oldest);
    }
}

/// Splits a command line on whitespace, honoring double quotes.
pub fn split_command(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

pub fn add_server(claude: &Path, draft: &ServerDraft) -> Result<(), String> {
    let scope_flag = draft
        .scope
        .cli_flag()
        .ok_or_else(|| "scope is not editable".to_string())?;

    let mut args: Vec<String> = vec![
        "mcp".into(),
        "add".into(),
        draft.name.clone(),
        "--scope".into(),
        scope_flag.into(),
        "--transport".into(),
        draft.transport.cli_flag().into(),
    ];
    for (key, value) in &draft.env {
        args.push("--env".into());
        args.push(format!("{key}={value}"));
    }
    for (key, value) in &draft.headers {
        args.push("--header".into());
        args.push(format!("{key}: {value}"));
    }
    match draft.transport {
        Transport::Stdio => {
            args.push("--".into());
            args.push(draft.target.clone());
            args.extend(draft.args.iter().cloned());
        }
        Transport::Http | Transport::Sse => args.push(draft.target.clone()),
    }

    run_claude(claude, &args, draft.scope.project_dir())
}

pub fn remove_server(claude: &Path, name: &str, scope: &Scope) -> Result<(), String> {
    let scope_flag = scope
        .cli_flag()
        .ok_or_else(|| "scope is not editable".to_string())?;
    let args: Vec<String> = vec![
        "mcp".into(),
        "remove".into(),
        name.to_string(),
        "--scope".into(),
        scope_flag.into(),
    ];
    run_claude(claude, &args, scope.project_dir())
}

/// local/project scopes are cwd-sensitive in the claude CLI — run from the
/// project dir; user scope runs from the home dir.
fn run_claude(claude: &Path, args: &[String], project_dir: Option<&str>) -> Result<(), String> {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    let cwd = project_dir.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(home));
    let output = run_captured(claude, args, Some(&cwd), 60)?;
    if output.exit_ok {
        Ok(())
    } else {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim().to_string()
        } else {
            output.stderr.trim().to_string()
        };
        Err(detail)
    }
}
