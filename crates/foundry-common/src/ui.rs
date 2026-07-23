//! Small Win32 shims that native-windows-gui lacks or gets wrong: in-place menu
//! caption updates (for runtime language switching) and a report-view list
//! column insert that sidesteps an nwg bug. Self-contained (nwg + winapi only).

use native_windows_gui as nwg;

/// Updates a menu item's caption in place (nwg has no set_text for menu items).
pub fn set_menu_item_text(item: &nwg::MenuItem, text: &str) {
    use winapi::um::winuser::{SetMenuItemInfoW, MENUITEMINFOW, MIIM_STRING};
    let nwg::ControlHandle::MenuItem(parent, id) = item.handle else { return };
    let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    info.fMask = MIIM_STRING;
    info.dwTypeData = wide.as_mut_ptr();
    unsafe {
        SetMenuItemInfoW(parent, id, 0, &info);
    }
}

/// Updates a top-level menu caption by locating its position in the parent
/// menu bar via its HMENU (submenus have no command id to address them by).
pub fn set_submenu_text(menu: &nwg::Menu, text: &str) {
    use winapi::um::winuser::{
        GetMenuItemCount, GetMenuItemInfoW, SetMenuItemInfoW, MENUITEMINFOW, MIIM_STRING, MIIM_SUBMENU,
    };
    let nwg::ControlHandle::Menu(parent, own) = menu.handle else { return };
    let count = unsafe { GetMenuItemCount(parent) }.max(0) as u32;
    for position in 0..count {
        let mut probe: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        probe.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
        probe.fMask = MIIM_SUBMENU;
        let found = unsafe { GetMenuItemInfoW(parent, position, 1, &mut probe) };
        if found == 0 || probe.hSubMenu != own {
            continue;
        }
        let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
        info.fMask = MIIM_STRING;
        info.dwTypeData = wide.as_mut_ptr();
        unsafe {
            SetMenuItemInfoW(parent, position, 1, &info);
        }
        return;
    }
}

/// Inserts a report-view list view column via a direct `LVM_INSERTCOLUMNW` call.
///
/// `nwg::ListView::insert_column` unconditionally probes the existing column
/// count first by sending `LVM_GETCOLUMNWIDTH` in a loop until it returns 0 —
/// a message Microsoft documents as valid only for LVS_LIST/LVS_ICON views.
/// Sent against our LVS_REPORT (Detailed) list view, it never returns 0,
/// spinning the UI thread at 100% CPU forever before the message pump even
/// starts. We always supply an explicit column index, so that probed count
/// is never used — this reimplements only the needed subset of the call.
pub fn insert_report_list_view_column(listview: &nwg::ListView, index: i32, width: i32, text: &str) {
    use winapi::um::commctrl::{LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVM_INSERTCOLUMNW};
    use winapi::um::winuser::SendMessageW;

    let Some(handle) = listview.handle.hwnd() else { return };
    let scaled_width = (width as f64 * nwg::scale_factor()) as i32;
    let mut wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();

    let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
    column.mask = LVCF_TEXT | LVCF_WIDTH;
    column.cx = scaled_width;
    column.pszText = wide_text.as_mut_ptr();
    column.cchTextMax = wide_text.len() as i32;

    unsafe {
        SendMessageW(handle, LVM_INSERTCOLUMNW, index as usize, &mut column as *mut LVCOLUMNW as isize);
    }
}
