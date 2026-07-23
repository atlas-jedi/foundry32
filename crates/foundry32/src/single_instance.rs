//! One hub instance at a time, via a named mutex. A second launch focuses the
//! existing window (looked up by its "Foundry32" title) instead of racing on
//! installed.json, then bows out.

use winapi::shared::ntdef::HANDLE;
use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::CreateMutexW;

const MUTEX_NAME: &str = "Local\\Foundry32.SingleInstance";

/// Held for the process lifetime; releases the mutex on drop.
pub struct InstanceGuard(HANDLE);

// SAFETY: the guard only owns a mutex HANDLE, closed once on drop.
unsafe impl Send for InstanceGuard {}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// `Some(guard)` if this is the only instance, `None` if one already runs.
/// Fails open: if the mutex can't be created at all, startup is not blocked.
pub fn acquire() -> Option<InstanceGuard> {
    let name: Vec<u16> = MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Some(InstanceGuard(std::ptr::null_mut()));
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return None;
    }
    Some(InstanceGuard(handle))
}

/// Bring the already-running hub window to the front; if it can't be found,
/// show a small message so the second launch isn't a silent no-op.
pub fn notify_already_running() {
    use winapi::um::winuser::{
        FindWindowW, MessageBoxW, SetForegroundWindow, ShowWindow, MB_ICONINFORMATION, MB_OK, SW_RESTORE,
    };
    let title: Vec<u16> = "Foundry32\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
            return;
        }
        let text: Vec<u16> = "Foundry32 is already running.\0".encode_utf16().collect();
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
