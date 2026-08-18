//! Local wall-clock time, via Win32 `GetLocalTime`.
//!
//! The workspace has no date/time crate and does not want one: `std::time`
//! only offers an epoch offset, and turning that into a local calendar date
//! means reimplementing timezone and leap-year rules. `GetLocalTime` already
//! did it, is one call, and matches what the user's clock shows.

/// Local time as `(year, month, day, hour, minute, second)`.
fn local_parts() -> (u16, u16, u16, u16, u16, u16) {
    use winapi::um::minwinbase::SYSTEMTIME;
    use winapi::um::sysinfoapi::GetLocalTime;
    // SAFETY: GetLocalTime only writes the SYSTEMTIME we hand it.
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    (now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond)
}

/// `2026-08-17 14:32:05` — sorts lexicographically, reads as a human date.
pub fn timestamp() -> String {
    let (y, mo, d, h, mi, s) = local_parts();
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// `20260817-143205` — a run id: filename-safe and chronologically sortable.
pub fn run_id() -> String {
    let (y, mo, d, h, mi, s) = local_parts();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}
