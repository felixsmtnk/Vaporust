//! Fallback fonts from Windows: egui's built-in fonts lack many characters (arrows, stars,
//! icons, emoji, letters from nicknames like "Ɐ", Chinese, Japanese and Korean titles), and
//! squares were drawn instead. Font files are memory-mapped from disk, so even
//! a 20 MB Chinese font takes almost no program memory. If a font is missing on the PC,
//! it is simply skipped.

use eframe::egui;

pub fn install(ctx: &egui::Context) {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let mut fonts = egui::FontDefinitions::default();
    let mut added = Vec::new();
    for (name, file) in [
        ("segoe_ui", "segoeui.ttf"),
        ("segoe_symbol", "seguisym.ttf"),
        ("segoe_emoji", "seguiemj.ttf"),
        ("yahei", "msyh.ttc"),
        ("yu_gothic", "YuGothM.ttc"),
        ("malgun", "malgun.ttf"),
    ] {
        // The font file is used straight from disk (memory mapping): it is not copied into
        // program memory, Windows reads only the needed parts from disk and shares them with other
        // programs. If that fails - read it whole, as before.
        let data = match map_file(&dir.join(file)) {
            Some(bytes) => egui::FontData::from_static(bytes),
            // Large fonts (CJK) are never read into memory whole.
            None => match std::fs::read(dir.join(file)) {
                Ok(bytes) if bytes.len() < 4 << 20 => egui::FontData::from_owned(bytes),
                _ => continue,
            },
        };
        fonts.font_data.insert(name.to_string(), data);
        added.push(name.to_string());
    }
    if added.is_empty() {
        return;
    }
    // At the end of the lists: the main font stays the same, system ones only for missing characters.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.extend(added.iter().cloned());
        }
    }
    ctx.set_fonts(fonts);
}

/// Map a file into memory read-only for the lifetime of the program.
fn map_file(path: &std::path::Path) -> Option<&'static [u8]> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, GetFileSize, FILE_SHARE_READ, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL};
    use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_READ, PAGE_READONLY};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let file = CreateFileW(wide.as_ptr(), GENERIC_READ, FILE_SHARE_READ, std::ptr::null(), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut());
        if file == INVALID_HANDLE_VALUE {
            return None;
        }
        let size = GetFileSize(file, std::ptr::null_mut());
        if size == 0 || size == u32::MAX {
            CloseHandle(file);
            return None;
        }
        let map = CreateFileMappingW(file, std::ptr::null(), PAGE_READONLY, 0, 0, std::ptr::null());
        CloseHandle(file);
        if map.is_null() {
            return None;
        }
        let view = MapViewOfFile(map, FILE_MAP_READ, 0, 0, 0);
        // The mapping lives while the view is open; the handle can be closed.
        CloseHandle(map);
        if view.Value.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(view.Value as *const u8, size as usize))
    }
}
