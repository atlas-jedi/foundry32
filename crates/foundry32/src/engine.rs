//! Install / update / uninstall / launch, with the Windows file-locking care
//! the design review flagged.
//!
//! Commit order upholds the invariant *entry in installed.json => verified exes
//! exist*: download every artifact to a temp file and verify its SHA-256, only
//! then rename them onto the final exes, and write installed.json last. A tool
//! with companions is therefore all-or-nothing — a failure partway through
//! leaves temp files (swept later) and no catalog entry. A running old exe
//! can't be overwritten but CAN be renamed aside, so updates move it out of the
//! way. installed.json is always re-read fresh right before it's mutated, so
//! two hub instances never lose each other's writes.

use crate::download::{self, DlError, Progress};
use crate::installed::{now_epoch, InstalledState, InstalledTool};
use crate::paths;
use crate::pathenv;
use crate::registry::ToolEntry;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const DETACHED_PROCESS: u32 = 0x0000_0008;

#[derive(Debug)]
pub enum EngineError {
    /// The catalog entry has no verifiable download yet (empty/other host).
    NotInstallable,
    /// The tool's exe is currently running.
    InUse,
    Download(DlError),
    Io(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotInstallable => write!(f, "this tool has no verifiable download yet"),
            EngineError::InUse => write!(f, "the tool is running — close it and try again"),
            EngineError::Download(e) => write!(f, "{e}"),
            EngineError::Io(m) => write!(f, "{m}"),
        }
    }
}

/// Installs a fresh tool or updates an existing one (same path — an already
/// present exe is moved aside first, so a running old version never blocks the
/// swap). Every artifact is downloaded and verified before any of them is
/// placed. Writes installed.json last.
pub fn install(
    entry: &ToolEntry,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), EngineError> {
    if !entry.is_installable() {
        return Err(EngineError::NotInstallable);
    }
    let dir = paths::tool_dir(&entry.id);
    fs::create_dir_all(&dir).map_err(|e| EngineError::Io(e.to_string()))?;
    paths::sweep_stale_tmp();

    // Progress is reported for the install as a whole: bytes already finished
    // plus what the current artifact has done. `total_bytes()` is 0 when the
    // catalog doesn't size every file, in which case we fall back to whatever
    // Content-Length the current download reports.
    let artifacts = entry.artifacts();
    let grand_total = entry.total_bytes();
    let mut finished: u64 = 0;
    let mut staged: Vec<(std::path::PathBuf, String)> = Vec::new();

    for artifact in &artifacts {
        let tmp = paths::tmp_path_for(&entry.id, &artifact.exe);
        let mut this_artifact: u64 = 0;
        download::download_verify(&artifact.download_url, &artifact.sha256, &tmp, cancel, |p| {
            this_artifact = p.done;
            let total = if grand_total > 0 { Some(grand_total) } else { p.total };
            on_progress(Progress { done: finished + p.done, total });
        })
        .map_err(EngineError::Download)?;
        finished += this_artifact;
        staged.push((tmp, artifact.exe.clone()));
    }

    for (tmp, exe_name) in &staged {
        place_exe(tmp, &paths::tool_exe(&entry.id, exe_name), &dir, exe_name)?;
    }
    expose_on_path(entry.expose_on_path, &dir)?;
    record_installed(entry)
}

/// Dev/offline install from a local file (exercises the install path without a
/// download). Used by the `--install-local` headless flag; installs the primary
/// exe only, but honours the catalog's PATH exposure so that plumbing can be
/// verified without a release.
pub fn install_from_file(
    id: &str,
    src: &Path,
    version: &str,
    exe_name: &str,
    expose_path: bool,
) -> Result<(), EngineError> {
    let dir = paths::tool_dir(id);
    fs::create_dir_all(&dir).map_err(|e| EngineError::Io(e.to_string()))?;
    let exe = paths::tool_exe(id, exe_name);
    if exe.exists() {
        let aside = dir.join(format!("{exe_name}.{}.old", now_epoch()));
        retry_io(|| fs::rename(&exe, &aside)).map_err(|e| EngineError::Io(e.to_string()))?;
    }
    retry_io(|| fs::copy(src, &exe)).map_err(|e| EngineError::Io(e.to_string()))?;
    expose_on_path(expose_path, &dir)?;
    let tool = InstalledTool {
        version: version.to_string(),
        exe: exe_name.to_string(),
        companions: Vec::new(),
        path_exposed: expose_path,
        installed_at: now_epoch(),
    };
    save_installed(id, tool)
}

/// Publishes the tool's directory on the user's PATH when the catalog asks for
/// it. Done after the exes are in place, so a shell that reacts to the
/// broadcast finds a working command.
fn expose_on_path(wanted: bool, dir: &Path) -> Result<(), EngineError> {
    if !wanted {
        return Ok(());
    }
    pathenv::add(dir).map(|_| ()).map_err(EngineError::Io)
}

/// Renames the verified temp file onto the final exe. If an exe is already
/// there (update/reinstall), it is moved aside first under a unique `.old` name
/// — permitted even while the old image runs; swept on a later launch.
fn place_exe(tmp: &Path, exe: &Path, dir: &Path, exe_name: &str) -> Result<(), EngineError> {
    if exe.exists() {
        let aside = dir.join(format!("{exe_name}.{}.old", now_epoch()));
        retry_io(|| fs::rename(exe, &aside)).map_err(|e| EngineError::Io(e.to_string()))?;
    }
    retry_io(|| fs::rename(tmp, exe)).map_err(|e| EngineError::Io(e.to_string()))
}

fn record_installed(entry: &ToolEntry) -> Result<(), EngineError> {
    let tool = InstalledTool {
        version: entry.version.clone(),
        exe: entry.exe.clone(),
        companions: entry.companions.iter().map(|a| a.exe.clone()).collect(),
        path_exposed: entry.expose_on_path,
        installed_at: now_epoch(),
    };
    save_installed(&entry.id, tool)
}

fn save_installed(id: &str, tool: InstalledTool) -> Result<(), EngineError> {
    // Re-read fresh so a concurrent hub instance's changes aren't clobbered.
    let mut state = InstalledState::load();
    state.upsert(id, tool);
    state.save_atomic().map_err(EngineError::Io)
}

/// Removes an installed tool's directory, and its PATH entry if it had one.
/// Refuses if any of its exes is running. The PATH is cleaned before the files
/// go, so a failure there never leaves an entry pointing at a deleted folder.
pub fn uninstall(id: &str) -> Result<(), EngineError> {
    let state = InstalledState::load();
    let Some(tool) = state.get(id) else { return Ok(()) };
    let exes = std::iter::once(&tool.exe).chain(tool.companions.iter());
    if exes.map(|exe| paths::tool_exe(id, exe)).any(|exe| exe_locked(&exe)) {
        return Err(EngineError::InUse);
    }
    let dir = paths::tool_dir(id);
    if tool.path_exposed {
        pathenv::remove(&dir).map_err(EngineError::Io)?;
    }
    if dir.exists() {
        retry_io(|| fs::remove_dir_all(&dir)).map_err(|e| EngineError::Io(e.to_string()))?;
    }
    let mut state = InstalledState::load();
    state.remove(id);
    state.save_atomic().map_err(EngineError::Io)
}

/// Launches an installed tool, detached, with its own directory as the working
/// dir (resolves relative assets and closes the DLL search order to a directory
/// we control). No pipes — it's a GUI launch, not a captured subprocess.
pub fn launch(id: &str) -> Result<(), EngineError> {
    let state = InstalledState::load();
    let Some(tool) = state.get(id) else {
        return Err(EngineError::Io("tool is not installed".into()));
    };
    let exe = paths::tool_exe(id, &tool.exe);
    let dir = paths::tool_dir(id);
    Command::new(&exe)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|e| EngineError::Io(format!("launch {}: {e}", exe.display())))
}

/// True if `exe` can't be opened exclusively for writing — i.e. it's running.
/// The image loader keeps a running exe open with share READ|DELETE (not
/// WRITE), so our exclusive write-open fails with a sharing violation.
pub fn exe_locked(exe: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    if !exe.exists() {
        return false;
    }
    const ERROR_SHARING_VIOLATION: i32 = 32;
    match fs::OpenOptions::new().write(true).share_mode(0).open(exe) {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}

/// Retries a filesystem op a few times over ~1s. Antivirus (Defender included)
/// opens a freshly closed file to scan it, so a rename right after a download
/// can transiently fail with a sharing violation.
pub fn retry_io<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    const DELAYS_MS: [u64; 5] = [0, 60, 120, 250, 500];
    let mut last: Option<std::io::Error> = None;
    for delay in DELAYS_MS {
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }
        match op() {
            Ok(value) => return Ok(value),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("retry_io: no attempt made")))
}
