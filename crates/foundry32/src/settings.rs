//! Hub preferences persisted under %APPDATA%\Software Imperial\Foundry32.
//! Only the UI language today (the `Lang` type is shared via foundry-common).

use foundry_common::{detect_system_lang, Lang};
use std::fs;
use std::path::PathBuf;

pub struct AppSettings {
    pub lang: Lang,
}

fn settings_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("Software Imperial").join("Foundry32")
}

fn settings_file() -> PathBuf {
    settings_dir().join("settings.json")
}

impl AppSettings {
    pub fn load() -> AppSettings {
        let lang = fs::read_to_string(settings_file())
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| v["lang"].as_str().and_then(Lang::from_code))
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
