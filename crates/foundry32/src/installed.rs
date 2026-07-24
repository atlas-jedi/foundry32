//! Tracks which tools are installed, in
//! `%LOCALAPPDATA%\Software Imperial\Foundry32\installed.json`.
//!
//! Written atomically (temp file + rename) and always re-read immediately
//! before a mutation, so two hub instances can't lose each other's updates.
//! The invariant `entry in installed.json => a verified exe exists` is upheld
//! by writing this file last during install and by `reconcile()` at startup.

use crate::engine::retry_io;
use crate::paths;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;

#[derive(Clone, Debug)]
pub struct InstalledTool {
    pub version: String,
    pub exe: String,
    /// Extra binaries installed next to `exe` (empty for single-file tools).
    /// Recorded so uninstall can check them for locks and reconcile can tell a
    /// half-deleted install from a complete one.
    pub companions: Vec<String>,
    /// True if installing this tool put its directory on the user's PATH — the
    /// only record of what to undo at uninstall time.
    pub path_exposed: bool,
    /// Unix epoch seconds, best-effort; 0 if the clock was unavailable.
    pub installed_at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct InstalledState {
    tools: BTreeMap<String, InstalledTool>,
}

impl InstalledState {
    pub fn load() -> InstalledState {
        let mut state = InstalledState::default();
        let Some(raw) = fs::read_to_string(paths::installed_json_path()).ok() else {
            return state;
        };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else {
            return state;
        };
        if let Some(map) = root["tools"].as_object() {
            for (id, entry) in map {
                let version = entry["version"].as_str().unwrap_or("").to_string();
                let exe = entry["exe"].as_str().unwrap_or("").to_string();
                let installed_at = entry["installed_at"].as_u64().unwrap_or(0);
                // Fields added with multi-artifact tools: absent in files
                // written by older hubs, which is exactly the default.
                let companions = entry["companions"]
                    .as_array()
                    .map(|items| {
                        items.iter().filter_map(|v| v.as_str()).map(str::to_string).collect()
                    })
                    .unwrap_or_default();
                let path_exposed = entry["path_exposed"].as_bool().unwrap_or(false);
                if !version.is_empty() && !exe.is_empty() {
                    state.tools.insert(
                        id.clone(),
                        InstalledTool { version, exe, companions, path_exposed, installed_at },
                    );
                }
            }
        }
        state
    }

    /// Drops entries whose recorded files no longer exist on disk (self-heal
    /// after a manual delete or an interrupted install) — every artifact must be
    /// present, so a half-deleted multi-binary tool reads as not installed and
    /// gets a clean reinstall. Returns true if it changed anything, so the
    /// caller can persist.
    pub fn reconcile(&mut self) -> bool {
        let before = self.tools.len();
        self.tools.retain(|id, tool| {
            paths::tool_exe(id, &tool.exe).exists()
                && tool.companions.iter().all(|exe| paths::tool_exe(id, exe).exists())
        });
        self.tools.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&InstalledTool> {
        self.tools.get(id)
    }

    pub fn upsert(&mut self, id: &str, tool: InstalledTool) {
        self.tools.insert(id.to_string(), tool);
    }

    pub fn remove(&mut self, id: &str) {
        self.tools.remove(id);
    }

    /// Persists atomically: write a temp file, then rename over the real one
    /// (with retry, since AV/Defender can briefly hold the handle open).
    pub fn save_atomic(&self) -> Result<(), String> {
        let path = paths::installed_json_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut map = serde_json::Map::new();
        for (id, tool) in &self.tools {
            map.insert(
                id.clone(),
                serde_json::json!({
                    "version": tool.version,
                    "exe": tool.exe,
                    "companions": tool.companions,
                    "path_exposed": tool.path_exposed,
                    "installed_at": tool.installed_at,
                }),
            );
        }
        let body = serde_json::json!({ "tools": Value::Object(map) }).to_string();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, body).map_err(|e| e.to_string())?;
        retry_io(|| fs::rename(&tmp, &path)).map_err(|e| e.to_string())
    }
}

/// Best-effort Unix epoch seconds for stamping an install time.
pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
