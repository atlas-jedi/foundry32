//! Enumerating `node.exe` processes and reading the per-process facts that do
//! NOT require cross-process memory reads: image path, times (uptime + CPU),
//! and working-set memory. Command line and cwd need `ReadProcessMemory` of the
//! target's PEB and live in a later phase (they are architecture-sensitive), so
//! `cmdline`/`cwd` come back `None` here.

use crate::appname;
use crate::model::NodeProc;
use crate::ports;
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use winapi::shared::minwindef::{DWORD, FILETIME};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::processthreadsapi::{GetProcessTimes, OpenProcess};
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::sysinfoapi::{GetSystemInfo, GetSystemTimeAsFileTime, SYSTEM_INFO};
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::winnt::{HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

/// Samples node.exe processes, keeping just enough state between calls to turn
/// raw CPU times into a percentage. Reuse one `Scanner` across refreshes (GUI)
/// or call twice with a short gap for a one-shot CPU reading (CLI).
pub struct Scanner {
    /// Logical processor count, for normalizing CPU% to 0–100 across all cores.
    ncores: f64,
    /// Last sample's CPU point per PID, to diff against on the next sample.
    prev: HashMap<u32, CpuPoint>,
}

#[derive(Clone, Copy)]
struct CpuPoint {
    /// Guards against PID recycling — a reused PID with a different start time
    /// is a different process, so its CPU delta must not be computed.
    start_ft: u64,
    busy_100ns: u64,
    wall_100ns: u64,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        let ncores = unsafe {
            let mut si: SYSTEM_INFO = std::mem::zeroed();
            GetSystemInfo(&mut si);
            f64::from(si.dwNumberOfProcessors.max(1))
        };
        Scanner {
            ncores,
            prev: HashMap::new(),
        }
    }

    /// One snapshot of all `node.exe` processes, flat. Call [`crate::tree::build`]
    /// to nest them into the display forest.
    pub fn sample(&mut self) -> Vec<NodeProc> {
        let now = now_100ns();
        let pairs = enumerate_node_pids();
        let port_map = ports::listening_by_pid();

        let mut next_prev = HashMap::with_capacity(pairs.len());
        let mut out = Vec::with_capacity(pairs.len());

        for (pid, ppid) in pairs {
            let ports = port_map.get(&pid).cloned().unwrap_or_default();

            let Some(handle) = open_query(pid) else {
                out.push(minimal(pid, ppid, ports));
                continue;
            };
            let exe_path = image_path(handle);
            let (start_ft, busy_100ns) = process_times(handle);
            let mem_bytes = working_set(handle);
            let (cmdline, cwd) = crate::peb::read_params(handle);
            unsafe {
                CloseHandle(handle);
            }

            next_prev.insert(
                pid,
                CpuPoint {
                    start_ft,
                    busy_100ns,
                    wall_100ns: now,
                },
            );
            let cpu_percent = match self.prev.get(&pid) {
                Some(p) if p.start_ft == start_ft && now > p.wall_100ns => {
                    let busy_delta = busy_100ns.saturating_sub(p.busy_100ns) as f64;
                    let wall_delta = (now - p.wall_100ns) as f64;
                    ((busy_delta / wall_delta) / self.ncores * 100.0) as f32
                }
                _ => 0.0,
            };

            let uptime_secs = if start_ft > 0 {
                now.saturating_sub(start_ft) / 10_000_000
            } else {
                0
            };
            let app_name = appname::derive(exe_path.as_deref(), cmdline.as_deref(), cwd.as_deref());

            out.push(NodeProc {
                pid,
                ppid,
                exe_path,
                cmdline,
                cwd,
                app_name,
                ports,
                mem_bytes,
                cpu_percent,
                uptime_secs,
                start_filetime: start_ft,
                depth: 0,
            });
        }

        self.prev = next_prev;
        out
    }
}

/// A process we could see in the snapshot but not open (rare for one's own
/// processes) — still worth listing by PID and whatever ports it owns.
fn minimal(pid: u32, ppid: u32, ports: Vec<u16>) -> NodeProc {
    NodeProc {
        pid,
        ppid,
        exe_path: None,
        cmdline: None,
        cwd: None,
        app_name: "node.exe".to_string(),
        ports,
        mem_bytes: 0,
        cpu_percent: 0.0,
        uptime_secs: 0,
        start_filetime: 0,
        depth: 0,
    }
}

/// All `node.exe` PIDs with their parent PID, via a process snapshot.
fn enumerate_node_pids() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                if exe_name_is_node(&entry.szExeFile) {
                    out.push((entry.th32ProcessID, entry.th32ParentProcessID));
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// Case-insensitive match of a NUL-terminated wide exe name against `node.exe`.
fn exe_name_is_node(wide: &[u16]) -> bool {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let name = String::from_utf16_lossy(&wide[..len]);
    name.eq_ignore_ascii_case("node.exe")
}

/// Opens a process for the limited queries we need. Works for the user's own
/// processes without elevation.
fn open_query(pid: u32) -> Option<HANDLE> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        None
    } else {
        Some(handle)
    }
}

/// Full image path via `QueryFullProcessImageNameW` (works across bitness).
fn image_path(handle: HANDLE) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32_768];
    let mut size = buf.len() as DWORD;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
    if ok == 0 || size == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..size as usize])))
}

/// Returns `(creation_filetime, kernel+user busy time)` in 100 ns units.
fn process_times(handle: HANDLE) -> (u64, u64) {
    unsafe {
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return (0, 0);
        }
        (
            ft_to_u64(&creation),
            ft_to_u64(&kernel).wrapping_add(ft_to_u64(&user)),
        )
    }
}

/// Working-set size in bytes, or 0 if it can't be read.
fn working_set(handle: HANDLE) -> u64 {
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as DWORD;
        if GetProcessMemoryInfo(handle, &mut pmc, cb) == 0 {
            return 0;
        }
        pmc.WorkingSetSize as u64
    }
}

fn ft_to_u64(ft: &FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

fn now_100ns() -> u64 {
    unsafe {
        let mut ft: FILETIME = std::mem::zeroed();
        GetSystemTimeAsFileTime(&mut ft);
        ft_to_u64(&ft)
    }
}
