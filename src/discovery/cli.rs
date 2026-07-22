//! Runs `claude mcp list` and parses its plain-text output. Account connectors
//! are the lines whose name starts with "claude.ai ". Output format per line:
//! `<name>: <target> - <status>`, e.g.
//! `claude.ai Example: https://mcp.example.com/mcp - ✔ Connected`

use crate::winproc::run_captured;
use std::path::{Path, PathBuf};

pub struct CliListEntry {
    pub name: String,
    pub target: String,
    pub status: String,
}

/// Finds claude.exe via `where.exe`, falling back to the default native-install path.
pub fn locate_claude_binary() -> Option<PathBuf> {
    let where_exe = Path::new("C:/Windows/System32/where.exe");
    if let Ok(output) = run_captured(where_exe, &["claude".to_string()], None, 10) {
        if output.exit_ok {
            if let Some(first) = output.stdout.lines().next() {
                let candidate = PathBuf::from(first.trim());
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    let home = std::env::var("USERPROFILE").ok()?;
    let fallback = PathBuf::from(home).join(".local").join("bin").join("claude.exe");
    fallback.exists().then_some(fallback)
}

/// Runs `claude mcp list` from the user's home dir (neutral project context).
/// Slow: the CLI health-checks every server (can take tens of seconds).
pub fn run_mcp_list(claude: &Path) -> Result<Vec<CliListEntry>, String> {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    let output = run_captured(
        claude,
        &["mcp".to_string(), "list".to_string()],
        Some(Path::new(&home)),
        120,
    )?;
    if !output.exit_ok && output.stdout.trim().is_empty() {
        return Err(format!("claude mcp list failed: {}", output.stderr.trim()));
    }
    Ok(parse_mcp_list_output(&output.stdout))
}

fn parse_mcp_list_output(output: &str) -> Vec<CliListEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Checking") {
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else { continue };
        let Some((target, status)) = rest.rsplit_once(" - ") else { continue };
        let target = target
            .trim()
            .trim_end_matches(" (HTTP)")
            .trim_end_matches(" (SSE)")
            .trim_end_matches(" (stdio)");
        entries.push(CliListEntry {
            name: name.trim().to_string(),
            target: target.to_string(),
            status: status.trim().to_string(),
        });
    }
    entries
}
