//! Parses ~/.claude.json: top-level `mcpServers` (user scope) and
//! `projects.<dir>.mcpServers` (local scope). Project dir keys can repeat with
//! different casing — they are deduplicated case-insensitively.

use super::parse_server_definition;
use crate::model::{McpServer, Scope};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub struct ClaudeConfig {
    pub user_servers: Vec<McpServer>,
    pub local_servers: Vec<McpServer>,
    pub project_dirs: Vec<String>,
}

pub fn claude_json_path() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude.json")
}

fn normalize_dir(dir: &str) -> String {
    dir.replace('\\', "/").to_lowercase()
}

pub fn read_claude_config() -> Result<ClaudeConfig, String> {
    let path = claude_json_path();
    if !path.exists() {
        return Err(format!("{} not found (is Claude Code installed?)", path.display()));
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let root: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let source = path.display().to_string();

    let mut user_servers = Vec::new();
    if let Some(map) = root["mcpServers"].as_object() {
        for (name, definition) in map {
            user_servers.push(parse_server_definition(name, definition, Scope::User, &source));
        }
    }

    // key: (normalized dir, server name) → last definition wins, like Claude itself.
    let mut local_by_key: BTreeMap<(String, String), McpServer> = BTreeMap::new();
    let mut dirs_by_norm: BTreeMap<String, String> = BTreeMap::new();
    if let Some(projects) = root["projects"].as_object() {
        for (dir, project) in projects {
            let norm = normalize_dir(dir);
            dirs_by_norm.entry(norm.clone()).or_insert_with(|| dir.clone());
            if let Some(map) = project["mcpServers"].as_object() {
                for (name, definition) in map {
                    let display_dir = dirs_by_norm[&norm].clone();
                    let server = parse_server_definition(
                        name,
                        definition,
                        Scope::Local { project_dir: display_dir },
                        &source,
                    );
                    local_by_key.insert((norm.clone(), name.clone()), server);
                }
            }
        }
    }

    Ok(ClaudeConfig {
        user_servers,
        local_servers: local_by_key.into_values().collect(),
        project_dirs: dirs_by_norm.into_values().collect(),
    })
}
