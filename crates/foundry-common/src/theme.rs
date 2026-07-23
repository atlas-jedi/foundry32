//! Classic (Windows 32-bit) control theming and colored glyph icons.
//!
//! These are the pieces that give the workspace its old-school chrome: the
//! `SetWindowTheme` fallback-to-classic trick for 3D buttons, Explorer-style
//! list-view highlighting, and Segoe MDL2 glyphs rendered as colored ARGB icons
//! for status accents. All are self-contained (nwg + winapi only).

use native_windows_gui as nwg;

/// Renders a themed (comctl32 v6) button with the classic 3D bevel look by
/// pointing the theme manager at a non-existent theme — the documented
/// `SetWindowTheme(hwnd, L" ", L" ")` fallback-to-classic trick.
pub fn apply_classic_button_theme(button: &nwg::Button) {
    set_window_theme(&button.handle, " ", Some(" "));
}

/// Explorer item styling for the list view (hover highlight, softer selection),
/// matching what Explorer/Wireshark report views look like.
pub fn apply_explorer_theme(handle: &nwg::ControlHandle) {
    set_window_theme(handle, "Explorer", None);
}

/// Renders one Segoe MDL2 Assets glyph (the native Windows 10/11 icon font)
/// into a colored ARGB icon: the glyph is drawn white on black, the coverage
/// becomes the alpha channel, and the fill color is premultiplied in. Returns
/// null on failure — callers treat that as "no icon".
pub fn create_glyph_icon(glyph: u16, (r, g, b): (u8, u8, u8), size: i32) -> winapi::shared::windef::HICON {
    use winapi::shared::windef::RECT;
    use winapi::um::wingdi::{
        CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
        GdiFlush, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, FW_NORMAL, OUT_DEFAULT_PRECIS, TRANSPARENT,
    };
    use winapi::um::winuser::{
        CreateIconIndirect, DrawTextW, GetDC, ReleaseDC, DT_CENTER, DT_NOCLIP, DT_SINGLELINE,
        DT_VCENTER, ICONINFO,
    };

    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return std::ptr::null_mut();
        }

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size;
        info.bmiHeader.biHeight = -size; // top-down
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut winapi::ctypes::c_void = std::ptr::null_mut();
        let color_bitmap = CreateDIBSection(memory_dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if color_bitmap.is_null() || bits.is_null() {
            DeleteDC(memory_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return std::ptr::null_mut();
        }
        let previous_bitmap = SelectObject(memory_dc, color_bitmap as _);

        let face: Vec<u16> = "Segoe MDL2 Assets\0".encode_utf16().collect();
        let font = CreateFontW(
            -size, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, DEFAULT_PITCH, face.as_ptr(),
        );
        let previous_font = SelectObject(memory_dc, font as _);
        SetTextColor(memory_dc, 0x00FF_FFFF);
        SetBkMode(memory_dc, TRANSPARENT as i32);
        let mut rect = RECT { left: 0, top: 0, right: size, bottom: size };
        let text = [glyph];
        DrawTextW(memory_dc, text.as_ptr(), 1, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP);
        GdiFlush();

        let pixels = bits as *mut u32;
        for offset in 0..(size * size) as usize {
            let coverage = *pixels.add(offset) & 0xFF;
            let premultiply = |channel: u8| (channel as u32 * coverage) / 255;
            *pixels.add(offset) =
                (coverage << 24) | (premultiply(r) << 16) | (premultiply(g) << 8) | premultiply(b);
        }

        SelectObject(memory_dc, previous_font);
        DeleteObject(font as _);
        SelectObject(memory_dc, previous_bitmap);

        let mask_bits = vec![0u8; (size as usize).div_ceil(8) * 2 * size as usize];
        let mask_bitmap = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as _);
        let mut icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = CreateIconIndirect(&mut icon_info);

        DeleteObject(mask_bitmap as _);
        DeleteObject(color_bitmap as _);
        DeleteDC(memory_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);
        icon
    }
}

fn set_window_theme(handle: &nwg::ControlHandle, app_name: &str, id_list: Option<&str>) {
    use winapi::um::uxtheme::SetWindowTheme;
    let Some(hwnd) = handle.hwnd() else { return };
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let app_name_wide = wide(app_name);
    let id_list_wide = id_list.map(wide);
    let id_list_ptr = id_list_wide.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    unsafe {
        SetWindowTheme(hwnd, app_name_wide.as_ptr(), id_list_ptr);
    }
}
