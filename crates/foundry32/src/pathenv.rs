//! Publishing a tool's directory on the user's PATH, and taking it back off.
//!
//! Only `HKCU\Environment` is touched — the per-user PATH, writable asInvoker;
//! the machine PATH (which needs elevation) is never read or written. Two
//! things make this safe to do to a value the user also edits by hand:
//!
//! * the value is read and written **raw**, so a `REG_EXPAND_SZ` PATH full of
//!   `%USERPROFILE%`-style references keeps its type and its literal text — the
//!   classic way to corrupt a PATH is to read it expanded and write it back;
//! * entries we don't own are passed through byte-for-byte (including empty
//!   ones), so nothing is normalised, reordered or dropped behind the user's
//!   back.
//!
//! After a successful write, `WM_SETTINGCHANGE` is broadcast so already-running
//! shells and Explorer pick the change up without a logout.

use std::path::Path;
use winapi::shared::minwindef::LPARAM;
use winapi::um::winuser::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

/// The per-user environment key. Overridable so the PATH plumbing can be
/// exercised end-to-end (see `--dump-path`) without touching the real PATH.
const DEFAULT_SUBKEY: &str = "Environment";
const PATH_VALUE: &str = "Path";

/// Refuse to write a PATH longer than this. The registry would take more, but a
/// runaway value is far likelier to be a bug than a legitimate PATH, and a
/// corrupted PATH is painful to recover from.
const MAX_PATH_CHARS: usize = 30_000;

fn subkey() -> String {
    match std::env::var("FOUNDRY32_ENV_KEY") {
        Ok(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => DEFAULT_SUBKEY.to_string(),
    }
}

/// The user's PATH exactly as stored (unexpanded), with its registry type.
/// `None` when the value doesn't exist yet.
pub fn read_raw() -> Result<Option<RegValue>, String> {
    let key = open_key()?;
    match key.get_raw_value(PATH_VALUE) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("reading PATH: {error}")),
    }
}

/// Appends `dir` to the user's PATH. Returns whether anything changed — an
/// entry that's already there (any casing, trailing separator or not) is left
/// alone and reported as `false`.
pub fn add(dir: &Path) -> Result<bool, String> {
    let dir_text = dir.to_string_lossy().to_string();
    let current = read_raw()?;
    let (text, vtype) = match &current {
        Some(value) => (decode(value)?, value.vtype.clone()),
        // A user PATH that doesn't exist yet is created as a plain string: our
        // own path is fully expanded, so nothing would need REG_EXPAND_SZ.
        None => (String::new(), winreg::enums::RegType::REG_SZ),
    };
    if entries(&text).any(|entry| same_dir(entry, &dir_text)) {
        return Ok(false);
    }
    // A PATH ending in `;` keeps ending in `;` — the empty trailing entry is the
    // user's formatting, and uninstall then restores the value byte-for-byte.
    let updated = if text.is_empty() {
        dir_text
    } else if text.ends_with(';') {
        format!("{text}{dir_text};")
    } else {
        format!("{text};{dir_text}")
    };
    write(&updated, vtype)?;
    Ok(true)
}

/// Removes every PATH entry pointing at `dir`, leaving all other entries — even
/// empty ones — exactly as they were. Returns whether anything changed.
pub fn remove(dir: &Path) -> Result<bool, String> {
    let dir_text = dir.to_string_lossy().to_string();
    let Some(current) = read_raw()? else { return Ok(false) };
    let text = decode(&current)?;
    let kept: Vec<&str> = entries(&text).filter(|entry| !same_dir(entry, &dir_text)).collect();
    if kept.len() == entries(&text).count() {
        return Ok(false);
    }
    write(&kept.join(";"), current.vtype.clone())?;
    Ok(true)
}

fn open_key() -> Result<RegKey, String> {
    RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(subkey(), KEY_READ | KEY_WRITE)
        .map(|(key, _)| key)
        .map_err(|error| format!("opening HKCU\\{}: {error}", subkey()))
}

fn decode(value: &RegValue) -> Result<String, String> {
    String::from_reg_value(value).map_err(|error| format!("decoding PATH: {error}"))
}

fn entries(text: &str) -> impl Iterator<Item = &str> + '_ {
    text.split(';')
}

/// Compares two PATH entries as directories: quotes, surrounding whitespace,
/// trailing separators and case are all noise here.
fn same_dir(entry: &str, dir: &str) -> bool {
    fn normalise(text: &str) -> String {
        text.trim().trim_matches('"').trim_end_matches(['\\', '/']).to_lowercase()
    }
    let entry = normalise(entry);
    !entry.is_empty() && entry == normalise(dir)
}

/// Writes the value back with the type it already had, then tells the system.
fn write(text: &str, vtype: winreg::enums::RegType) -> Result<(), String> {
    if text.chars().count() > MAX_PATH_CHARS {
        return Err("refusing to write an implausibly long PATH".to_string());
    }
    let bytes: Vec<u8> = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    open_key()?
        .set_raw_value(PATH_VALUE, &RegValue { bytes, vtype })
        .map_err(|error| format!("writing PATH: {error}"))?;
    broadcast_change();
    Ok(())
}

/// Best-effort: a shell that misses the broadcast just needs to be restarted.
/// `SMTO_ABORTIFHUNG` plus a short timeout keeps one wedged top-level window
/// from blocking an install.
fn broadcast_change() {
    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            3000,
            &mut result,
        );
    }
}
