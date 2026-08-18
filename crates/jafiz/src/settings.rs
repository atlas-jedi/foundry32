//! Preferences persisted under `%APPDATA%\Software Imperial\JAFIZ`.
//!
//! Shared by both front-ends: the GUI writes them, and the CLI reads `lang` so
//! a report comes back in the language the user picked rather than whatever
//! Windows is set to.

use foundry_common::lang::{detect_system_lang, Lang};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct AppSettings {
    pub lang: Lang,
    /// Suite folders the user added in the GUI, most recent first.
    pub locations: Vec<PathBuf>,
    /// The folder the GUI had open last.
    pub last_location: Option<PathBuf>,
    /// The suite stem the GUI had selected last.
    pub last_suite: Option<String>,
}

fn settings_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("Software Imperial").join("JAFIZ")
}

fn settings_file() -> PathBuf {
    settings_dir().join("settings.json")
}

impl AppSettings {
    pub fn load() -> AppSettings {
        let value = std::fs::read_to_string(settings_file())
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or(Value::Null);
        AppSettings {
            lang: value["lang"].as_str().and_then(Lang::from_code).unwrap_or_else(detect_system_lang),
            locations: value["locations"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect())
                .unwrap_or_default(),
            last_location: value["last_location"].as_str().map(PathBuf::from),
            last_suite: value["last_suite"].as_str().map(str::to_string),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = settings_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let body = json!({
            "lang": self.lang.code(),
            "locations": self.locations.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "last_location": self.last_location.as_ref().map(|p| p.display().to_string()),
            "last_suite": self.last_suite,
        })
        .to_string();
        std::fs::write(settings_file(), body).map_err(|e| e.to_string())
    }

    /// Adds a folder to the recent list, most recent first, without duplicates.
    pub fn remember_location(&mut self, dir: &std::path::Path) {
        self.locations.retain(|p| p != dir);
        self.locations.insert(0, dir.to_path_buf());
        self.locations.truncate(8);
        self.last_location = Some(dir.to_path_buf());
    }
}
