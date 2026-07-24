//! The domain model: one `node.exe` process as WITN sees it, plus the small
//! human-facing formatting the CLI and GUI share.

use std::path::PathBuf;

/// A single `node.exe` process with everything WITN could learn about it.
///
/// `cmdline` and `cwd` are `Option` on purpose: they require reading another
/// process's memory (its PEB), which can be unavailable when the target runs
/// under a different CPU architecture than WITN, or when access is denied.
/// Every other field is obtainable for any of the user's own processes without
/// elevation.
#[derive(Clone, Debug)]
pub struct NodeProc {
    pub pid: u32,
    pub ppid: u32,
    /// Full path to the node.exe image (e.g. `C:\Program Files\nodejs\node.exe`).
    pub exe_path: Option<PathBuf>,
    /// Full command line, when readable — the key to naming the app.
    pub cmdline: Option<String>,
    /// Working directory, when readable — where we look for `package.json`.
    pub cwd: Option<PathBuf>,
    /// Friendly, derived name (`my-api · vite`, `server.js`, …). Never empty.
    pub app_name: String,
    /// TCP ports this PID is LISTENING on — ascending, de-duplicated.
    pub ports: Vec<u16>,
    /// Working-set size in bytes.
    pub mem_bytes: u64,
    /// CPU usage percent across all cores since the previous sample (0 on the
    /// first sample — it needs two points in time).
    pub cpu_percent: f32,
    /// Seconds since the process started.
    pub uptime_secs: u64,
    /// Process creation time as a Windows FILETIME (100 ns since 1601). Used to
    /// order the tree and to reject PID-recycled false parents.
    pub start_filetime: u64,
    /// Depth in the process forest — 0 for a project root, deeper for children
    /// it spawned. Drives the GUI/CLI indentation.
    pub depth: usize,
}

impl NodeProc {
    /// Working set in whole mebibytes, for compact display.
    pub fn mem_mib(&self) -> u64 {
        self.mem_bytes / (1024 * 1024)
    }

    /// Comma-separated listening ports, or `—` when none.
    pub fn ports_label(&self) -> String {
        if self.ports.is_empty() {
            "—".to_string()
        } else {
            self.ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// Formats an uptime in seconds as a compact human string: `45s`, `12m03s`,
/// `3h07m`, `2d04h`.
pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let s = secs % 60;
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{s:02}s")
    } else {
        format!("{s}s")
    }
}
