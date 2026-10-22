//! Reading a process's command line, working directory and environment from
//! its PEB.
//!
//! WITN ships as an x86 (WOW64) process; the node.exe processes it inspects are
//! almost always x64. A 32-bit process can't read a 64-bit process's PEB with
//! the ordinary `Nt*` calls — the pointers are 64-bit — so we use the WOW64
//! bridges `NtWow64QueryInformationProcess64` and `NtWow64ReadVirtualMemory64`,
//! resolved dynamically from ntdll (they exist only in a WOW64 process). This
//! is the same technique 32-bit Process Explorer / Process Hacker use.
//!
//! The offsets below are the stable, long-documented x64 layouts of PEB and
//! RTL_USER_PROCESS_PARAMETERS. If a target turns out NOT to be x64 (a rare
//! 32-bit node, or a 32-bit OS), we return `None` — the caller falls back to
//! naming by exe/ports, exactly as in Phase 1. A wrong offset can only yield a
//! failed read (→ `None`), never a crash: every read is bounds-checked.

use std::path::PathBuf;
use std::sync::OnceLock;

use winapi::ctypes::c_void;
use winapi::shared::minwindef::{FALSE, FARPROC};
use winapi::um::libloaderapi::{GetModuleHandleW, GetProcAddress};
use winapi::um::winnt::HANDLE;
use winapi::um::wow64apiset::IsWow64Process;

/// PEB (x64): offset of the `ProcessParameters` pointer.
const PEB64_PROCESS_PARAMETERS: u64 = 0x20;
/// RTL_USER_PROCESS_PARAMETERS (x64): `CurrentDirectory` (CURDIR.DosPath).
const RTLUPP64_CURRENT_DIRECTORY: u64 = 0x38;
/// RTL_USER_PROCESS_PARAMETERS (x64): `CommandLine`.
const RTLUPP64_COMMAND_LINE: u64 = 0x70;
/// RTL_USER_PROCESS_PARAMETERS (x64): `Environment` (pointer to the block).
const RTLUPP64_ENVIRONMENT: u64 = 0x80;
/// RTL_USER_PROCESS_PARAMETERS (x64): `EnvironmentSize` (bytes, Windows 7+).
const RTLUPP64_ENVIRONMENT_SIZE: u64 = 0x3F0;
/// Sanity cap on a UNICODE_STRING byte length before we trust and allocate it.
const MAX_STRING_BYTES: usize = 64 * 1024;
/// Sanity cap on an environment block — real ones are a few KB to a few dozen.
const MAX_ENV_BYTES: u64 = 1024 * 1024;

/// What a process's PEB tells us, each part independently optional.
#[derive(Default)]
pub struct ProcParams {
    pub cmdline: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

type NtWow64QueryInformationProcess64 =
    unsafe extern "system" fn(HANDLE, u32, *mut c_void, u32, *mut u32) -> i32;
type NtWow64ReadVirtualMemory64 =
    unsafe extern "system" fn(HANDLE, u64, *mut c_void, u64, *mut u64) -> i32;

struct Wow64Bridge {
    query: NtWow64QueryInformationProcess64,
    read: NtWow64ReadVirtualMemory64,
}

/// Resolves the WOW64 ntdll bridges once. `None` when WITN isn't a WOW64
/// process (e.g. a real 32-bit OS), in which case x64 PEBs can't be read.
fn bridge() -> Option<&'static Wow64Bridge> {
    static CACHE: OnceLock<Option<Wow64Bridge>> = OnceLock::new();
    CACHE
        .get_or_init(|| unsafe {
            let name = wide("ntdll.dll");
            let ntdll = GetModuleHandleW(name.as_ptr());
            if ntdll.is_null() {
                return None;
            }
            let query_ptr = GetProcAddress(ntdll, c"NtWow64QueryInformationProcess64".as_ptr());
            let read_ptr = GetProcAddress(ntdll, c"NtWow64ReadVirtualMemory64".as_ptr());
            if query_ptr.is_null() || read_ptr.is_null() {
                return None;
            }
            Some(Wow64Bridge {
                query: std::mem::transmute::<FARPROC, NtWow64QueryInformationProcess64>(query_ptr),
                read: std::mem::transmute::<FARPROC, NtWow64ReadVirtualMemory64>(read_ptr),
            })
        })
        .as_ref()
}

/// Command line, working directory and environment of a process, when
/// readable. The handle needs `PROCESS_QUERY_LIMITED_INFORMATION |
/// PROCESS_VM_READ`.
///
/// Implemented for the x64-target case (the dominant one). A target that is
/// itself WOW64 (a 32-bit node) — or the absence of the bridge — yields an
/// empty `ProcParams`, and the caller names the process by its exe/ports.
pub fn read_params(handle: HANDLE) -> ProcParams {
    let Some(bridge) = bridge() else {
        return ProcParams::default();
    };
    if is_wow64_target(handle) {
        return ProcParams::default();
    }
    let Some(peb) = query_peb_base(bridge, handle) else {
        return ProcParams::default();
    };
    let Some(params) = read_u64(bridge, handle, peb + PEB64_PROCESS_PARAMETERS) else {
        return ProcParams::default();
    };
    ProcParams {
        cmdline: read_unicode_string(bridge, handle, params + RTLUPP64_COMMAND_LINE),
        cwd: read_unicode_string(bridge, handle, params + RTLUPP64_CURRENT_DIRECTORY)
            .map(PathBuf::from),
        env: read_environment(bridge, handle, params).unwrap_or_default(),
    }
}

/// The environment block: a pointer and a byte size side by side in the
/// process parameters, read whole and handed to [`parse_env_block`].
fn read_environment(
    bridge: &Wow64Bridge,
    handle: HANDLE,
    params: u64,
) -> Option<Vec<(String, String)>> {
    let block = read_u64(bridge, handle, params + RTLUPP64_ENVIRONMENT)?;
    let size = read_u64(bridge, handle, params + RTLUPP64_ENVIRONMENT_SIZE)?;
    if block == 0 || size == 0 || size > MAX_ENV_BYTES {
        return None;
    }
    let mut bytes = vec![0u8; size as usize];
    read_mem(bridge, handle, block, &mut bytes)?;
    let units: Vec<u16> =
        bytes.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect();
    Some(parse_env_block(&units))
}

fn is_wow64_target(handle: HANDLE) -> bool {
    let mut wow = FALSE;
    unsafe {
        IsWow64Process(handle, &mut wow);
    }
    wow != FALSE
}

/// `PebBaseAddress` from PROCESS_BASIC_INFORMATION64 (48 bytes; the pointer is
/// at offset 8). Read as raw bytes to sidestep 32-bit struct-alignment traps.
fn query_peb_base(bridge: &Wow64Bridge, handle: HANDLE) -> Option<u64> {
    let mut buf = [0u8; 48];
    let mut ret_len: u32 = 0;
    let status = unsafe {
        (bridge.query)(
            handle,
            0, // ProcessBasicInformation
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut ret_len,
        )
    };
    if status < 0 {
        return None;
    }
    Some(u64::from_le_bytes(buf[8..16].try_into().ok()?))
}

fn read_u64(bridge: &Wow64Bridge, handle: HANDLE, addr: u64) -> Option<u64> {
    let mut buf = [0u8; 8];
    read_mem(bridge, handle, addr, &mut buf)?;
    Some(u64::from_le_bytes(buf))
}

fn read_mem(bridge: &Wow64Bridge, handle: HANDLE, addr: u64, buf: &mut [u8]) -> Option<()> {
    let mut read: u64 = 0;
    let status = unsafe {
        (bridge.read)(
            handle,
            addr,
            buf.as_mut_ptr().cast(),
            buf.len() as u64,
            &mut read,
        )
    };
    if status < 0 || (read as usize) < buf.len() {
        return None;
    }
    Some(())
}

/// Reads a UNICODE_STRING (x64 layout: `Length` u16 @0, `Buffer` u64 @8) and
/// then the wide characters it points at, as a `String`.
fn read_unicode_string(bridge: &Wow64Bridge, handle: HANDLE, addr: u64) -> Option<String> {
    let mut hdr = [0u8; 16];
    read_mem(bridge, handle, addr, &mut hdr)?;
    let length = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
    let buffer = u64::from_le_bytes(hdr[8..16].try_into().ok()?);
    if length == 0 || buffer == 0 || length > MAX_STRING_BYTES {
        return None;
    }
    let mut bytes = vec![0u8; length];
    read_mem(bridge, handle, buffer, &mut bytes)?;
    // `as_chunks`, not `chunks_exact(2)`: the pair arrives as a `[u8; 2]` the
    // conversion takes whole, with no indexing and no length to get wrong. A
    // trailing odd byte cannot happen — `length` counts UTF-16 units — and the
    // remainder half of the pair is dropped for exactly that reason.
    let utf16: Vec<u16> =
        bytes.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect();
    let text = String::from_utf16_lossy(&utf16);
    let trimmed = text.trim_end_matches('\0').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Splits a Windows environment block — `NAME=value` entries, each ending in a
/// NUL, the whole block ending in an empty entry — into `(name, value)` pairs.
/// Entries whose name starts with `=` (the hidden per-drive `=C:=C:\…` cwd
/// records cmd keeps) are skipped: they are not variables anyone set.
pub fn parse_env_block(units: &[u16]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in units.split(|&unit| unit == 0) {
        if entry.is_empty() {
            break; // the block's terminating empty entry
        }
        let text = String::from_utf16_lossy(entry);
        if text.starts_with('=') {
            continue;
        }
        if let Some((name, value)) = text.split_once('=') {
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_env_block;

    fn block(entries: &[&str]) -> Vec<u16> {
        let mut units = Vec::new();
        for entry in entries {
            units.extend(entry.encode_utf16());
            units.push(0);
        }
        units.push(0);
        units
    }

    #[test]
    fn env_block_yields_name_value_pairs_in_order() {
        let parsed = parse_env_block(&block(&[r"PATH=C:\nvm4w\nodejs", "NODE_ENV=development"]));
        assert_eq!(
            parsed,
            vec![
                ("PATH".to_string(), r"C:\nvm4w\nodejs".to_string()),
                ("NODE_ENV".to_string(), "development".to_string()),
            ]
        );
    }

    #[test]
    fn env_block_skips_hidden_drive_entries() {
        let parsed = parse_env_block(&block(&[r"=C:=C:\Users\Pichau", r"HOME=C:\Users\Pichau"]));
        assert_eq!(parsed, vec![("HOME".to_string(), r"C:\Users\Pichau".to_string())]);
    }

    #[test]
    fn env_block_stops_at_the_empty_entry() {
        let mut units = block(&["A=1"]);
        units.extend("GARBAGE=after-the-end".encode_utf16());
        assert_eq!(parse_env_block(&units), vec![("A".to_string(), "1".to_string())]);
    }

    #[test]
    fn env_value_keeps_its_own_equals_signs() {
        let parsed = parse_env_block(&block(&["NODE_OPTIONS=--max-old-space-size=4096"]));
        assert_eq!(parsed, vec![("NODE_OPTIONS".to_string(), "--max-old-space-size=4096".to_string())]);
    }

    #[test]
    fn env_block_without_terminator_still_parses() {
        let units: Vec<u16> = "A=1\0B=2".encode_utf16().collect();
        assert_eq!(
            parse_env_block(&units),
            vec![("A".to_string(), "1".to_string()), ("B".to_string(), "2".to_string())]
        );
    }
}
