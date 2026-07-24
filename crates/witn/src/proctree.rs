//! Whole-process-tree helpers for `kill`: enumerating every process (not just
//! node) and terminating one.
//!
//! Killing a node dev server means killing the workers it spawned too — and
//! those children aren't always node (esbuild, a shell, a native binary) — so
//! the subtree walks *all* processes. It's built purely from one snapshot's
//! parent links, so PID recycling (a between-snapshots hazard) isn't a concern.

use std::collections::{HashMap, HashSet, VecDeque};

use winapi::shared::minwindef::DWORD;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use winapi::um::winnt::PROCESS_TERMINATE;

#[derive(Clone, Debug)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    pub exe_name: String,
}

/// Snapshot of every process on the machine (pid, parent pid, exe file name).
pub fn all_processes() -> Vec<ProcEntry> {
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
                out.push(ProcEntry {
                    pid: entry.th32ProcessID,
                    ppid: entry.th32ParentProcessID,
                    exe_name: wide_to_string(&entry.szExeFile),
                });
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// The `root` process plus all its descendants, breadth-first (parents before
/// children). Terminate in reverse for a children-first kill. Returns empty if
/// `root` isn't in the snapshot.
pub fn subtree(entries: &[ProcEntry], root: u32) -> Vec<ProcEntry> {
    let mut children: HashMap<u32, Vec<&ProcEntry>> = HashMap::new();
    for entry in entries {
        children.entry(entry.ppid).or_default().push(entry);
    }
    let by_pid: HashMap<u32, &ProcEntry> = entries.iter().map(|e| (e.pid, e)).collect();
    let Some(&root_entry) = by_pid.get(&root) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_entry);
    while let Some(entry) = queue.pop_front() {
        if !seen.insert(entry.pid) {
            continue; // guards against cycles / a self-parenting pid
        }
        out.push(entry.clone());
        if let Some(kids) = children.get(&entry.pid) {
            for &kid in kids {
                if kid.pid != entry.pid {
                    queue.push_back(kid);
                }
            }
        }
    }
    out
}

/// Terminates one process by PID. `Err` if it can't be opened or the kill call
/// fails (access denied, or it already exited).
pub fn terminate(pid: u32) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(format!("não foi possível abrir o processo {pid}"));
        }
        let ok = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if ok == 0 {
            return Err(format!("falha ao encerrar o processo {pid}"));
        }
    }
    Ok(())
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}
