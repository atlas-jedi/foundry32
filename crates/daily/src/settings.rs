//! Preferences persisted under `%APPDATA%\Software Imperial\DAILY`.
//!
//! Shared by both front-ends: the GUI writes them, and the CLI reads `lang` so
//! a report comes back in the language the user picked rather than whatever
//! Windows is set to.

use foundry_common::lang::{detect_system_lang, Lang};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct AppSettings {
    pub lang: Lang,
}

fn settings_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("Software Imperial").join("DAILY")
}

fn settings_file() -> PathBuf {
    settings_dir().join("settings.json")
}

impl AppSettings {
    /// Reads preferences from disk, falling back to the detected system
    /// language when the file is missing or unreadable — a fresh install must
    /// still produce a usable `AppSettings`.
    pub fn load() -> AppSettings {
        let value = std::fs::read_to_string(settings_file())
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or(Value::Null);
        AppSettings {
            lang: value["lang"].as_str().and_then(Lang::from_code).unwrap_or_else(detect_system_lang),
        }
    }

    /// Writes preferences to disk, creating the settings folder on first use.
    pub fn save(&self) -> Result<(), String> {
        let dir = settings_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let body = json!({ "lang": self.lang.code() }).to_string();
        std::fs::write(settings_file(), body).map_err(|e| e.to_string())
    }
}
