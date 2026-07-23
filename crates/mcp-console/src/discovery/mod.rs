//! Discovers MCP servers from config files (fast) and the claude CLI (slow, adds
//! account connectors + health status).

pub mod claude_json;
pub mod cli;
pub mod mcp_json;

use crate::model::{McpServer, Scope, Transport};
use cli::CliListEntry;

pub struct Discovery {
    pub servers: Vec<McpServer>,
    pub warnings: Vec<String>,
    /// Project directories known to ~/.claude.json (feeds the dialog's list).
    pub project_dirs: Vec<String>,
}

/// Reads ~/.claude.json (user + local scopes) and each known project's .mcp.json.
pub fn discover_file_servers() -> Discovery {
    let mut servers = Vec::new();
    let mut warnings = Vec::new();
    let mut project_dirs = Vec::new();

    match claude_json::read_claude_config() {
        Ok(config) => {
            servers.extend(config.user_servers);
            servers.extend(config.local_servers);
            for project_dir in &config.project_dirs {
                match mcp_json::read_project_servers(project_dir) {
                    Ok(project_servers) => servers.extend(project_servers),
                    Err(warning) => warnings.push(warning),
                }
            }
            project_dirs = config.project_dirs;
        }
        Err(warning) => warnings.push(warning),
    }

    sort_servers(&mut servers);
    Discovery { servers, warnings, project_dirs }
}

/// Merges `claude mcp list` entries: claude.ai connectors and plugin servers are
/// added as new rows; file-based rows get their health status attached.
pub fn merge_cli_entries(discovery: &mut Discovery, entries: Vec<CliListEntry>) {
    for entry in entries {
        if let Some(short_name) = entry.name.strip_prefix("claude.ai ") {
            discovery.servers.push(cli_only_server(
                short_name.to_string(),
                Scope::Account,
                &entry,
            ));
        } else if entry.name.starts_with("plugin:") {
            discovery.servers.push(cli_only_server(
                entry.name.clone(),
                Scope::Plugin,
                &entry,
            ));
        } else if let Some(existing) = discovery
            .servers
            .iter_mut()
            .find(|s| s.name == entry.name)
        {
            existing.status = Some(entry.status);
        } else {
            discovery.servers.push(cli_only_server(
                entry.name.clone(),
                Scope::Unknown,
                &entry,
            ));
        }
    }
    sort_servers(&mut discovery.servers);
}

fn cli_only_server(name: String, scope: Scope, entry: &CliListEntry) -> McpServer {
    let transport = if entry.target.starts_with("http://") || entry.target.starts_with("https://") {
        Transport::Http
    } else {
        Transport::Stdio
    };
    McpServer {
        name,
        scope,
        transport,
        target: entry.target.clone(),
        env_keys: Vec::new(),
        status: Some(entry.status.clone()),
        source_file: None,
    }
}

fn scope_rank(scope: &Scope) -> u8 {
    match scope {
        Scope::Account => 0,
        Scope::User => 1,
        Scope::Project { .. } => 2,
        Scope::Local { .. } => 3,
        Scope::Plugin => 4,
        Scope::Unknown => 5,
    }
}

pub fn sort_servers(servers: &mut [McpServer]) {
    servers.sort_by(|a, b| {
        scope_rank(&a.scope)
            .cmp(&scope_rank(&b.scope))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Shared parser for a server definition object from any JSON config file.
/// Reads env var NAMES only — never the values.
pub(crate) fn parse_server_definition(
    name: &str,
    definition: &serde_json::Value,
    scope: Scope,
    source_file: &str,
) -> McpServer {
    let type_hint = definition["type"].as_str().unwrap_or("");
    let url = definition["url"].as_str().unwrap_or("");
    let command = definition["command"].as_str().unwrap_or("");

    let transport = match type_hint {
        "http" => Transport::Http,
        "sse" => Transport::Sse,
        "stdio" => Transport::Stdio,
        _ if !url.is_empty() => Transport::Http,
        _ => Transport::Stdio,
    };

    let target = if transport == Transport::Stdio {
        let args: Vec<String> = definition["args"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|a| a.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if args.is_empty() {
            command.to_string()
        } else {
            format!("{command} {}", args.join(" "))
        }
    } else {
        url.to_string()
    };

    let mut env_keys: Vec<String> = definition["env"]
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    env_keys.sort();

    McpServer {
        name: name.to_string(),
        scope,
        transport,
        target,
        env_keys,
        status: None,
        source_file: Some(source_file.to_string()),
    }
}
