//! Small Win32 shims that native-windows-gui lacks or gets wrong: in-place menu
//! caption updates (for runtime language switching), a report-view list column
//! insert that sidesteps an nwg bug, the window icon nwg only half-sets, and
//! list-view header sort arrows, and single-row list-view selection.
//! Self-contained (nwg + winapi only).

use native_windows_gui as nwg;

/// Resource id of the app icon in every crate's `app.rc` (`1 ICON "…"`).
const APP_ICON_RESOURCE_ID: u16 = 1;

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

/// Gives a window both icon sizes Windows actually asks for.
///
/// `nwg::Window::set_icon` sends `WM_SETICON` with `wParam = 0` — that is
/// `ICON_SMALL`, the title-bar icon — and never sets `ICON_BIG`; the window
/// class nwg registers carries `hIcon: null` too. The taskbar button and
/// Alt+Tab ask for `ICON_BIG`, find nothing, and fall back to a generic icon,
/// which is why the app looks right in its own title bar and wrong on the
/// taskbar. This loads the embedded app icon at each metric's own size and
/// sets both, plus the class icons, so dialogs created later inherit them.
///
/// Does nothing when the icon resource is absent — plain GNU dev builds skip
/// the resource compile, so the icon only shows up in release (MSVC) builds.
pub fn apply_window_icon(handle: &nwg::ControlHandle) {
    use winapi::shared::windef::HICON;
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::winuser::{
        GetSystemMetrics, LoadImageW, SendMessageW, SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM,
        ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LR_SHARED, MAKEINTRESOURCEW, SM_CXICON,
        SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    let Some(hwnd) = handle.hwnd() else { return };
    unsafe {
        // LR_SHARED: the system caches and owns these handles, so nothing here
        // has to be destroyed — and repeated calls hand back the same icons.
        let load = |cx, cy| {
            LoadImageW(
                GetModuleHandleW(std::ptr::null()),
                MAKEINTRESOURCEW(APP_ICON_RESOURCE_ID),
                IMAGE_ICON,
                GetSystemMetrics(cx),
                GetSystemMetrics(cy),
                LR_DEFAULTCOLOR | LR_SHARED,
            ) as HICON
        };
        let big = load(SM_CXICON, SM_CYICON);
        let small = load(SM_CXSMICON, SM_CYSMICON);
        if big.is_null() && small.is_null() {
            return;
        }
        for (which, icon) in [(ICON_BIG, big), (ICON_SMALL, small)] {
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, which as usize, icon as isize);
            }
        }
        for (which, icon) in [(GCLP_HICON, big), (GCLP_HICONSM, small)] {
            if !icon.is_null() {
                SetClassLongPtrW(hwnd, which, icon as isize as _);
            }
        }
    }
}

/// Draws (or clears) the sort arrow on a report list view's header, the way
/// Explorer marks the column a list is ordered by.
///
/// `sort` is the zero-based column and whether the order is descending;
/// `None` clears every column's arrow.
pub fn set_list_view_sort_indicator(listview: &nwg::ListView, sort: Option<(i32, bool)>) {
    use winapi::um::commctrl::{
        HDF_SORTDOWN, HDF_SORTUP, HDITEMW, HDI_FORMAT, HDM_GETITEMCOUNT, HDM_GETITEMW,
        HDM_SETITEMW, LVM_GETHEADER,
    };
    use winapi::um::winuser::SendMessageW;

    let Some(handle) = listview.handle.hwnd() else { return };
    unsafe {
        let header = SendMessageW(handle, LVM_GETHEADER, 0, 0) as winapi::shared::windef::HWND;
        if header.is_null() {
            return;
        }
        let count = SendMessageW(header, HDM_GETITEMCOUNT, 0, 0) as i32;
        for column in 0..count {
            let mut item: HDITEMW = std::mem::zeroed();
            item.mask = HDI_FORMAT;
            if SendMessageW(header, HDM_GETITEMW, column as usize, &mut item as *mut HDITEMW as isize) == 0 {
                continue;
            }
            item.fmt &= !(HDF_SORTUP | HDF_SORTDOWN);
            if let Some((sorted_column, descending)) = sort {
                if sorted_column == column {
                    item.fmt |= if descending { HDF_SORTDOWN } else { HDF_SORTUP };
                }
            }
            SendMessageW(header, HDM_SETITEMW, column as usize, &mut item as *mut HDITEMW as isize);
        }
    }
}

/// Moves a list view's selection to exactly one row.
///
/// `nwg::ListView::select_item` only *adds* to the selection — our lists are
/// not `LVS_SINGLESEL` — so the rows the selection moved off are cleared
/// first. Otherwise every move would leave another row highlighted behind it.
pub fn select_only(listview: &nwg::ListView, row: usize) {
    for selected in listview.selected_items() {
        if selected != row {
            listview.select_item(selected, false);
        }
    }
    listview.select_item(row, true);
}

/// Scrolls a row into view. nwg has no wrapper for this one.
pub fn ensure_visible(listview: &nwg::ListView, row: usize) {
    use winapi::um::commctrl::LVM_ENSUREVISIBLE;
    use winapi::um::winuser::SendMessageW;
    let Some(handle) = listview.handle.hwnd() else { return };
    // SAFETY: a live list-view HWND, and LVM_ENSUREVISIBLE takes no pointer —
    // wParam is the row index and lParam a "partial is enough" flag.
    unsafe {
        SendMessageW(handle, LVM_ENSUREVISIBLE, row, 0);
    }
}
