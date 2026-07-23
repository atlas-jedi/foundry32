//! App preferences persisted under %APPDATA%\Software Imperial\MCP Console.
//! On first launch, a one-time migration copies the preference across from the
//! former MCP Hangar 1.x location so the user's language choice carries over.

use crate::i18n::{detect_system_lang, Lang};
use std::fs;
use std::path::{Path, PathBuf};

pub struct AppSettings {
    pub lang: Lang,
}

fn settings_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("Software Imperial").join("MCP Console")
}

fn legacy_settings_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("Software Imperial").join("MCP Hangar")
}

fn settings_file() -> PathBuf {
    settings_dir().join("settings.json")
}

fn read_lang(path: &Path) -> Option<Lang> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v["lang"].as_str().and_then(Lang::from_code))
}

/// One-time migration: if the new settings file is absent but the MCP Hangar
/// 1.x file exists, copy it across so the language preference carries over.
/// Best-effort — a copy failure still returns the legacy value.
fn migrate_from_legacy() -> Option<Lang> {
    let legacy = legacy_settings_dir().join("settings.json");
    let lang = read_lang(&legacy)?;
    let dir = settings_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let _ = fs::copy(&legacy, dir.join("settings.json"));
    }
    Some(lang)
}

impl AppSettings {
    pub fn load() -> AppSettings {
        let lang = read_lang(&settings_file())
            .or_else(migrate_from_legacy)
            .unwrap_or_else(detect_system_lang);
        AppSettings { lang }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = settings_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let body = serde_json::json!({ "lang": self.lang.code() }).to_string();
        fs::write(settings_file(), body).map_err(|e| e.to_string())
    }
}
