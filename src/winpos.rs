//! Размер и положение главного окна между запусками.
//!
//! eframe умеет сохранять окно сам, но у нас оно открывалось то крошечным, то на весь экран
//! (особенно при смене монитора или масштаба). Поэтому храним сами, в пикселях экрана, и при
//! запуске проверяем: окно должно помещаться на один из подключенных мониторов. Если нет -
//! открываемся как раньше: 960x630 посередине.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WinPos {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Окно было развернуто на весь экран (x/y/w/h - обычный размер до разворачивания).
    pub max: bool,
}

/// Наименьший размер, который восстанавливаем (меньше - считаем ошибкой и не трогаем окно).
const MIN_W: i32 = 500;
const MIN_H: i32 = 320;

/// Прямоугольник окна, подогнанный под рабочую область монитора `work` (left, top, right,
/// bottom): не больше нее и целиком внутри. None - окно на этом мониторе почти не видно.
pub fn fit(p: WinPos, work: (i32, i32, i32, i32)) -> Option<WinPos> {
    let (l, t, r, b) = work;
    if p.w < MIN_W || p.h < MIN_H || r - l < MIN_W || b - t < MIN_H {
        return None;
    }
    // Сколько окна на этом мониторе: хотя бы кусок 200x100, иначе окно «потерялось» (монитор отключили).
    let iw = (p.x + p.w).min(r) - p.x.max(l);
    let ih = (p.y + p.h).min(b) - p.y.max(t);
    if iw < 200 || ih < 100 {
        return None;
    }
    let w = p.w.min(r - l);
    let h = p.h.min(b - t);
    let x = p.x.clamp(l, r - w);
    let y = p.y.clamp(t, b - h);
    Some(WinPos { x, y, w, h, max: p.max })
}

/// Текущее положение окна (только обычное, не свернутое и не спрятанное в трей). Развернутое -
/// флаг max с прежним обычным размером из `prev`.
#[cfg(windows)]
pub fn read(prev: Option<WinPos>) -> Option<WinPos> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsZoomed};
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() {
        return None;
    }
    unsafe {
        if IsIconic(hwnd as _) != 0 {
            return None;
        }
        if IsZoomed(hwnd as _) != 0 {
            return prev.map(|p| WinPos { max: true, ..p });
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd as _, &mut r) == 0 {
            return None;
        }
        let p = WinPos { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top, max: false };
        (p.w >= MIN_W && p.h >= MIN_H && p.x > -30000).then_some(p)
    }
}

/// Поставить окно на сохраненное место (один раз, сразу после появления окна).
/// Возвращает, нужно ли развернуть окно.
#[cfg(windows)]
pub fn apply(p: WinPos) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 {
        return false;
    }
    unsafe {
        let rect = RECT { left: p.x, top: p.y, right: p.x + p.w, bottom: p.y + p.h };
        let mon = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
        if mon.is_null() {
            return false; // монитора, где было окно, больше нет
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut info) == 0 {
            return false;
        }
        let w = info.rcWork;
        let Some(f) = fit(p, (w.left, w.top, w.right, w.bottom)) else { return false };
        SetWindowPos(hwnd as _, std::ptr::null_mut(), f.x, f.y, f.w, f.h, SWP_NOZORDER | SWP_NOACTIVATE);
        f.max
    }
}

#[cfg(not(windows))]
pub fn read(_prev: Option<WinPos>) -> Option<WinPos> {
    None
}
#[cfg(not(windows))]
pub fn apply(_p: WinPos) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: (i32, i32, i32, i32) = (0, 0, 1920, 1040);

    fn p(x: i32, y: i32, w: i32, h: i32) -> WinPos {
        WinPos { x, y, w, h, max: false }
    }

    #[test]
    fn fits_screen() {
        // Обычное окно - как было.
        assert_eq!(fit(p(100, 100, 960, 630), SCREEN), Some(p(100, 100, 960, 630)));
        // Чуть вылезает за край - подвигаем внутрь.
        assert_eq!(fit(p(1500, 700, 960, 630), SCREEN), Some(p(960, 410, 960, 630)));
        // Больше экрана (был монитор крупнее) - уменьшаем до экрана.
        assert_eq!(fit(p(-10, -10, 2560, 1400), SCREEN), Some(p(0, 0, 1920, 1040)));
        // На отключенном втором мониторе - не восстанавливаем.
        assert_eq!(fit(p(2000, 100, 960, 630), SCREEN), None);
        assert_eq!(fit(p(1800, 100, 960, 630), SCREEN), None, "видно меньше 200 точек");
        // Крошечное окно или спрятанное в трей - не восстанавливаем.
        assert_eq!(fit(p(100, 100, 120, 80), SCREEN), None);
        assert_eq!(fit(p(-32000, -32000, 960, 630), SCREEN), None);
        // Флаг «развернуто» сохраняется.
        assert!(fit(WinPos { max: true, ..p(100, 100, 960, 630) }, SCREEN).unwrap().max);
    }
}
