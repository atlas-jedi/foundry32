//! Everything the Details view shows beyond the list's columns, read on demand
//! for one process: who runs it (user, elevation), what runs (64/32-bit, the
//! Node version stamped in `node.exe`), how it is doing (peak and private
//! memory, handles, threads, priority, CPU time), its sockets, and its
//! lineage with every command line.
//!
//! Field names follow the conventions of Process Explorer's Properties and of
//! Task Manager's Details tab, so they read the same as the tools people
//! already know. Every field is optional: a process can exit, or be elevated
//! above us, between the scan and this read.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use winapi::ctypes::c_void;
use winapi::shared::minwindef::FILETIME;
use winapi::um::handleapi::CloseHandle;
use winapi::um::minwinbase::SYSTEMTIME;
use winapi::um::processthreadsapi::{
    GetCurrentProcess, GetPriorityClass, GetProcessHandleCount, OpenProcess, OpenProcessToken,
};
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use winapi::um::securitybaseapi::GetTokenInformation;
use winapi::um::timezoneapi::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use winapi::um::winbase::{
    LookupAccountSidW, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, REALTIME_PRIORITY_CLASS,
};
use winapi::um::winnt::{
    TokenElevation, TokenUser, HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use winapi::um::winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use winapi::um::wow64apiset::IsWow64Process;

use crate::model::{LocalTime, NodeProc, ProcRef};
use crate::{peb, ports, procscan, proctree};

/// `VS_FIXEDFILEINFO` (verrsrc.h) — the root block of a version resource.
/// Defined locally because winapi 0.3 does not export it.
#[repr(C)]
#[allow(non_snake_case)]
struct VS_FIXEDFILEINFO {
    dwSignature: u32,
    dwStrucVersion: u32,
    dwFileVersionMS: u32,
    dwFileVersionLS: u32,
    dwProductVersionMS: u32,
    dwProductVersionLS: u32,
    dwFileFlagsMask: u32,
    dwFileFlags: u32,
    dwFileOS: u32,
    dwFileType: u32,
    dwFileSubtype: u32,
    dwFileDateMS: u32,
    dwFileDateLS: u32,
}

/// A process's base priority, named as Task Manager names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

/// Maps a `GetPriorityClass` value; `None` for anything unexpected.
pub fn priority_from_class(class: u32) -> Option<Priority> {
    match class {
        IDLE_PRIORITY_CLASS => Some(Priority::Idle),
        BELOW_NORMAL_PRIORITY_CLASS => Some(Priority::BelowNormal),
        NORMAL_PRIORITY_CLASS => Some(Priority::Normal),
        ABOVE_NORMAL_PRIORITY_CLASS => Some(Priority::AboveNormal),
        HIGH_PRIORITY_CLASS => Some(Priority::High),
        REALTIME_PRIORITY_CLASS => Some(Priority::Realtime),
        _ => None,
    }
}

/// `22.22.0` from a VS_FIXEDFILEINFO file version — node.exe's file version
/// is its release, and the fourth part is always 0.
pub fn version_string(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16)
}

#[derive(Clone, Debug, Default)]
pub struct Details {
    /// Whether the PID still belongs to the process that was scanned.
    pub alive: bool,
    pub user: Option<String>,
    pub elevated: Option<bool>,
    pub is_64bit: Option<bool>,
    pub node_version: Option<String>,
    pub started: Option<LocalTime>,
    pub cpu_time_secs: Option<u64>,
    pub peak_working_set: Option<u64>,
    pub private_bytes: Option<u64>,
    pub handles: Option<u32>,
    pub threads: Option<u32>,
    pub priority: Option<Priority>,
    pub endpoints: Vec<ports::Endpoint>,
    /// The scan's ancestors, with their command lines read.
    pub ancestors: Vec<ProcRef>,
    /// The scan's descendants, with their command lines read.
    pub descendants: Vec<ProcRef>,
}

/// Reads the details of a scanned process. A process that exited (or whose
/// PID was recycled) since the scan comes back with `alive: false` and only
/// what needs no handle — never another process's data under its name.
pub fn inspect(p: &NodeProc) -> Details {
    let mut details = Details {
        started: local_time(p.start_filetime),
        node_version: p.exe_path.as_deref().and_then(file_version),
        ancestors: with_command_lines(&p.origin.ancestors),
        descendants: with_command_lines(&p.origin.descendants),
        ..Details::default()
    };
    let Some(handle) = open(p.pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return details;
    };
    let (start_ft, busy_100ns) = procscan::process_times(handle);
    details.alive = p.start_filetime == 0 || start_ft == p.start_filetime;
    if details.alive {
        details.cpu_time_secs = Some(busy_100ns / 10_000_000);
        details.is_64bit = is_64bit(handle);
        (details.user, details.elevated) = token_facts(handle);
        (details.peak_working_set, details.private_bytes) = memory(handle);
        details.handles = handle_count(handle);
        details.priority = priority_from_class(unsafe { GetPriorityClass(handle) });
        details.endpoints = ports::endpoints_of(p.pid);
        // Read only once the PID is known to still be this process.
        details.threads = proctree::all_processes().iter().find(|e| e.pid == p.pid).map(|e| e.threads);
    }
    unsafe {
        CloseHandle(handle);
    }
    details
}

/// A FILETIME (UTC) as local wall-clock time, honoring the DST rule in force
/// on that date rather than today's.
pub fn local_time(ft: u64) -> Option<LocalTime> {
    if ft == 0 {
        return None;
    }
    unsafe {
        let file_time = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&file_time, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(null_mut(), &utc, &mut local) == 0
        {
            return None;
        }
        Some(LocalTime {
            year: local.wYear,
            month: local.wMonth,
            day: local.wDay,
            hour: local.wHour,
            minute: local.wMinute,
            second: local.wSecond,
        })
    }
}

/// Copies of `refs` with each command line read from its PEB — only when the
/// PID is provably still the same process (same start time). A process that
/// exited, or whose start time was never known, keeps no command line rather
/// than borrowing the one of whoever holds its PID now.
fn with_command_lines(refs: &[ProcRef]) -> Vec<ProcRef> {
    refs.iter()
        .map(|r| {
            let mut copy = r.clone();
            if r.start_filetime == 0 {
                return copy;
            }
            if let Some(handle) = open(r.pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ) {
                let (start_ft, _) = procscan::process_times(handle);
                if start_ft == r.start_filetime {
                    copy.cmdline = peb::read_params(handle).cmdline;
                }
                unsafe {
                    CloseHandle(handle);
                }
            }
            copy
        })
        .collect()
}

fn open(pid: u32, access: u32) -> Option<HANDLE> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    (!handle.is_null()).then_some(handle)
}

/// 64-bit unless the target runs under WOW64 — on a 64-bit Windows, which
/// WITN (itself a WOW64 process) can tell by asking about its own process.
fn is_64bit(handle: HANDLE) -> Option<bool> {
    let wow = |h: HANDLE| -> Option<bool> {
        let mut flag = 0;
        (unsafe { IsWow64Process(h, &mut flag) } != 0).then_some(flag != 0)
    };
    let os_is_64 = cfg!(target_pointer_width = "64") || wow(unsafe { GetCurrentProcess() })?;
    Some(os_is_64 && !wow(handle)?)
}

/// `DOMAIN\user` and whether the token is elevated.
fn token_facts(process: HANDLE) -> (Option<String>, Option<bool>) {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return (None, None);
    }
    let user = token_user(token);
    let elevated = unsafe {
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0;
        let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        (GetTokenInformation(token, TokenElevation, (&mut elevation as *mut TOKEN_ELEVATION).cast(), size, &mut len)
            != 0)
            .then_some(elevation.TokenIsElevated != 0)
    };
    unsafe {
        CloseHandle(token);
    }
    (user, elevated)
}

fn token_user(token: HANDLE) -> Option<String> {
    unsafe {
        let mut len = 0;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
        if len == 0 {
            return None;
        }
        // u64-aligned storage: TOKEN_USER holds a pointer.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
            return None;
        }
        let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        let mut name = [0u16; 256];
        let mut domain = [0u16; 256];
        let (mut name_len, mut domain_len) = (name.len() as u32, domain.len() as u32);
        let mut use_kind = 0;
        if LookupAccountSidW(
            null_mut(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_kind,
        ) == 0
        {
            return None;
        }
        let name = String::from_utf16_lossy(&name[..name_len as usize]);
        let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
        Some(if domain.is_empty() { name } else { format!("{domain}\\{name}") })
    }
}

/// (peak working set, private bytes).
fn memory(handle: HANDLE) -> (Option<u64>, Option<u64>) {
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if GetProcessMemoryInfo(handle, (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(), size) == 0 {
            return (None, None);
        }
        (Some(counters.PeakWorkingSetSize as u64), Some(counters.PrivateUsage as u64))
    }
}

fn handle_count(handle: HANDLE) -> Option<u32> {
    let mut count = 0;
    (unsafe { GetProcessHandleCount(handle, &mut count) } != 0).then_some(count)
}

/// The file version stamped in an executable's version resource.
fn file_version(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut ignored = 0;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut ignored);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        let mut info: *mut c_void = null_mut();
        let mut info_len = 0;
        if VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut info, &mut info_len) == 0
            || info.is_null()
            || (info_len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let fixed = &*(info as *const VS_FIXEDFILEINFO);
        Some(version_string(fixed.dwFileVersionMS, fixed.dwFileVersionLS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_classes_map_to_task_manager_names() {
        assert_eq!(priority_from_class(0x20), Some(Priority::Normal));
        assert_eq!(priority_from_class(0x80), Some(Priority::High));
        assert_eq!(priority_from_class(0x4000), Some(Priority::BelowNormal));
        assert_eq!(priority_from_class(0x8000), Some(Priority::AboveNormal));
        assert_eq!(priority_from_class(0x40), Some(Priority::Idle));
        assert_eq!(priority_from_class(0x100), Some(Priority::Realtime));
        assert_eq!(priority_from_class(0x1), None);
    }

    #[test]
    fn node_version_drops_the_fourth_part() {
        assert_eq!(version_string((22 << 16) | 22, 0), "22.22.0");
        assert_eq!(version_string((20 << 16) | 11, (1 << 16) | 7), "20.11.1");
    }
}
