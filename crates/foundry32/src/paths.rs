//! Filesystem layout for installed tools and hub state, all under
//! `%LOCALAPPDATA%\Software Imperial\Foundry32`. Runs asInvoker — no elevation,
//! only the user's own profile is written. A deliberately shallow tree
//! (`tools\<id>\<exe>`) keeps paths well clear of MAX_PATH.

use std::path::PathBuf;

const VENDOR: &str = "Software Imperial";
const APP: &str = "Foundry32";

/// `%LOCALAPPDATA%`, with a best-effort fallback if the env var is missing or
/// blank. Resolved purely through `std::env` / `std::fs` (wide APIs under the
/// hood) so spaces and Unicode in the path are handled — never via a shell.
pub fn local_appdata() -> PathBuf {
    if let Ok(dir) = std::env::var("LOCALAPPDATA") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("AppData").join("Local")
}

pub fn app_root() -> PathBuf {
    local_appdata().join(VENDOR).join(APP)
}

pub fn install_root() -> PathBuf {
    app_root().join("tools")
}

pub fn tool_dir(id: &str) -> PathBuf {
    install_root().join(id)
}

pub fn tool_exe(id: &str, exe: &str) -> PathBuf {
    tool_dir(id).join(exe)
}

/// Temp file a download streams into before it is verified and committed. One
/// per artifact, since a tool may ship several binaries (`exe` is validated as
/// a plain file name by the catalog parser, so it can't escape the directory).
pub fn tmp_path_for(id: &str, exe: &str) -> PathBuf {
    tool_dir(id).join(format!("{TMP_PREFIX}{exe}{TMP_SUFFIX}"))
}

const TMP_PREFIX: &str = ".download.";
const TMP_SUFFIX: &str = ".tmp";

pub fn installed_json_path() -> PathBuf {
    app_root().join("installed.json")
}

/// Last-known-good copy of the fetched catalog.
pub fn registry_cache_path() -> PathBuf {
    app_root().join("registry.cache.json")
}

/// Deletes leftover `.download.*.tmp` files from interrupted installs. Called at
/// startup and before each install so a crash never strands a partial download.
pub fn sweep_stale_tmp() {
    let Ok(tool_dirs) = std::fs::read_dir(install_root()) else { return };
    for tool_dir in tool_dirs.flatten() {
        let Ok(files) = std::fs::read_dir(tool_dir.path()) else { continue };
        for file in files.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(TMP_PREFIX) && name.ends_with(TMP_SUFFIX) {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }
}
