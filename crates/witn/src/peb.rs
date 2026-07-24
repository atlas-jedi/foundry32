//! Reading a node process's command line and working directory from its PEB.
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
/// Sanity cap on a UNICODE_STRING byte length before we trust and allocate it.
const MAX_STRING_BYTES: usize = 64 * 1024;

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

/// Command line and working directory of a node process, when readable.
///
/// Implemented for the x64-target case (the dominant one). A target that is
/// itself WOW64 (a 32-bit node) — or the absence of the bridge — yields
/// `(None, None)`, and the caller names the process by its exe/ports instead.
pub fn read_params(handle: HANDLE) -> (Option<String>, Option<PathBuf>) {
    let Some(bridge) = bridge() else {
        return (None, None);
    };
    if is_wow64_target(handle) {
        return (None, None);
    }
    let Some(peb) = query_peb_base(bridge, handle) else {
        return (None, None);
    };
    let Some(params) = read_u64(bridge, handle, peb + PEB64_PROCESS_PARAMETERS) else {
        return (None, None);
    };
    let cmdline = read_unicode_string(bridge, handle, params + RTLUPP64_COMMAND_LINE);
    let cwd =
        read_unicode_string(bridge, handle, params + RTLUPP64_CURRENT_DIRECTORY).map(PathBuf::from);
    (cmdline, cwd)
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
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
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
