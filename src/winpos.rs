//! Main window size and position between runs.
//!
//! eframe can save the window itself, but for us it opened either tiny or full screen
//! (especially after a monitor or scale change). So we store it ourselves, in screen pixels, and at
//! startup check that the window fits on one of the connected monitors. If not -
//! we open as before: 960x630 in the middle.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WinPos {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// The window was maximized (x/y/w/h - the normal size before maximizing).
    pub max: bool,
}

/// The smallest size we restore (smaller is treated as an error and the window is left alone).
const MIN_W: i32 = 500;
const MIN_H: i32 = 320;

/// Window rectangle fitted into the monitor work area `work` (left, top, right,
/// bottom): not larger than it and fully inside. None - the window is barely visible on this monitor.
pub fn fit(p: WinPos, work: (i32, i32, i32, i32)) -> Option<WinPos> {
    let (l, t, r, b) = work;
    if p.w < MIN_W || p.h < MIN_H || r - l < MIN_W || b - t < MIN_H {
        return None;
    }
    // How much of the window is on this monitor: at least a 200x100 piece, otherwise the window is "lost" (monitor disconnected).
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

/// Current window position (only a normal one, not minimized and not hidden in the tray). Maximized -
/// the max flag with the previous normal size from `prev`.
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

/// Put the window at the saved position (once, right after the window appears).
/// Returns whether the window should be maximized.
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
            return false; // the monitor the window was on is gone
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
        // A normal window - as it was.
        assert_eq!(fit(p(100, 100, 960, 630), SCREEN), Some(p(100, 100, 960, 630)));
        // Slightly beyond the edge - move it inside.
        assert_eq!(fit(p(1500, 700, 960, 630), SCREEN), Some(p(960, 410, 960, 630)));
        // Larger than the screen (the monitor used to be bigger) - shrink to the screen.
        assert_eq!(fit(p(-10, -10, 2560, 1400), SCREEN), Some(p(0, 0, 1920, 1040)));
        // On a disconnected second monitor - not restored.
        assert_eq!(fit(p(2000, 100, 960, 630), SCREEN), None);
        assert_eq!(fit(p(1800, 100, 960, 630), SCREEN), None, "less than 200 points visible");
        // A tiny window or one hidden in the tray - not restored.
        assert_eq!(fit(p(100, 100, 120, 80), SCREEN), None);
        assert_eq!(fit(p(-32000, -32000, 960, 630), SCREEN), None);
        // The "maximized" flag is kept.
        assert!(fit(WinPos { max: true, ..p(100, 100, 960, 630) }, SCREEN).unwrap().max);
    }
}
