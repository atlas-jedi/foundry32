//! Parses <project>/.mcp.json (project scope — shared with the team via git).

use super::parse_server_definition;
use crate::model::{McpServer, Scope};
use std::fs;
use std::path::Path;

pub fn read_project_servers(project_dir: &str) -> Result<Vec<McpServer>, String> {
    let path = Path::new(project_dir).join(".mcp.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let root: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let source = path.display().to_string();

    let mut servers = Vec::new();
    if let Some(map) = root["mcpServers"].as_object() {
        for (name, definition) in map {
            servers.push(parse_server_definition(
                name,
                definition,
                Scope::Project { project_dir: project_dir.to_string() },
                &source,
            ));
        }
    }
    Ok(servers)
}
