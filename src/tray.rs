//! Icon in the notification area (tray).
//!
//! Runs in a separate thread with its own Windows message loop: this way it responds to
//! clicks while the main window is hidden in the tray. The window is shown and hidden directly via WinAPI.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

enum TrayCmd {
    Unread(bool),
    /// Download progress: (name, fraction 0..1); None - no download.
    Download(Option<(String, f32)>),
}

static SENDER: OnceLock<Mutex<mpsc::Sender<TrayCmd>>> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static UNREAD: AtomicBool = AtomicBool::new(false);
/// The user chose "Exit" in the tray menu: the close button no longer hides the window.
static EXIT: AtomicBool = AtomicBool::new(false);
/// The window close button hides Vaporust to the tray (checkbox in the tray menu).
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);

/// Initial value of the checkbox (from settings) - before `start`.
pub fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

pub fn close_to_tray() -> bool {
    CLOSE_TO_TRAY.load(Ordering::Relaxed)
}

pub fn exit_requested() -> bool {
    EXIT.load(Ordering::Relaxed)
}

/// Really close the program (the close button does not hide to the tray), for example for a restart.
pub fn request_exit() {
    EXIT.store(true, Ordering::Relaxed);
}

/// Show a red dot on the icon (or remove it).
pub fn set_unread(on: bool) {
    if UNREAD.swap(on, Ordering::Relaxed) == on {
        return;
    }
    send(TrayCmd::Unread(on));
}

/// Download progress on the tray icon: a bar at the bottom of the icon and a tooltip. Sent only
/// when it changes by a whole percent (or when a download starts and ends).
pub fn set_download(d: Option<(String, f32)>) {
    static LAST: Mutex<Option<(String, i32)>> = Mutex::new(None);
    let key = d.as_ref().map(|(n, f)| (n.clone(), (f * 100.0) as i32));
    {
        let mut last = LAST.lock().unwrap();
        if *last == key {
            return;
        }
        *last = key;
    }
    send(TrayCmd::Download(d));
}

/// Icon with a download progress bar at the bottom (on top of the usual icon).
fn with_progress(base: &eframe::egui::IconData, frac: f32) -> Vec<u8> {
    let (w, h) = (base.width as usize, base.height as usize);
    let mut px = base.rgba.clone();
    let bar_h = (h / 8).max(2);
    let filled = ((w as f32) * frac.clamp(0.0, 1.0)).round() as usize;
    for y in h - bar_h..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let c: [u8; 4] = if x < filled { [26, 159, 255, 255] } else { [30, 36, 46, 255] };
            px[i..i + 4].copy_from_slice(&c);
        }
    }
    px
}

fn send(cmd: TrayCmd) {
    let Some(tx) = SENDER.get() else { return };
    let _ = tx.lock().unwrap().send(cmd);
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};
        // Wake the tray thread's message loop.
        PostThreadMessageW(THREAD_ID.load(Ordering::Relaxed), WM_APP, 0, 0);
    }
}

/// Type of the GetWindowLongPtrW value: isize on 64-bit Windows, i32 on 32-bit
/// (there it is just GetWindowLongW).
#[cfg(all(windows, target_pointer_width = "64"))]
type LongPtr = isize;
#[cfg(all(windows, target_pointer_width = "32"))]
type LongPtr = i32;

/// Where the window was before hiding to the tray (top left corner, physical pixels).
#[cfg(windows)]
static SAVED_POS: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// The window had WS_EX_APPWINDOW before going to the tray (it is put back on return).
static HAD_APPWINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hide the main window to the tray.
///
/// The window is NOT really hidden: a hidden eframe window stops updating, and the whole
/// program would fall asleep with it (queue, schedule, popup notifications). Instead the window
/// moves off-screen and becomes a "tool window" - without a taskbar button and not in Alt+Tab.
#[cfg(windows)]
pub fn hide_main() {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsIconic, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WINDOWPLACEMENT,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() {
        return;
    }
    unsafe {
        // A minimized window reports (-32000, -32000): its normal place is taken from the placement.
        if IsIconic(hwnd as _) != 0 {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(hwnd as _, &mut wp) != 0 {
                let r = wp.rcNormalPosition;
                *SAVED_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some((r.left, r.top));
            }
        } else {
            let mut r: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd as _, &mut r) != 0 {
                *SAVED_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some((r.left, r.top));
            }
        }
        // The window style changes only on a hidden window, so: hide -> change -> show off-screen.
        ShowWindow(hwnd as _, SW_HIDE);
        let ex = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
        HAD_APPWINDOW.store(ex & WS_EX_APPWINDOW as LongPtr != 0, std::sync::atomic::Ordering::Relaxed);
        SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW as LongPtr) & !(WS_EX_APPWINDOW as LongPtr));
        SetWindowPos(hwnd as _, std::ptr::null_mut(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
    }
    crate::notify::set_in_tray(true);
}

/// Show the main window from the tray and bring it to the front.
#[cfg(windows)]
pub fn show_main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsIconic, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
        SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 {
        return;
    }
    unsafe {
        if crate::notify::in_tray() {
            ShowWindow(hwnd as _, SW_HIDE);
            let ex = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
            let app = if HAD_APPWINDOW.load(std::sync::atomic::Ordering::Relaxed) { WS_EX_APPWINDOW as LongPtr } else { 0 };
            SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, (ex & !(WS_EX_TOOLWINDOW as LongPtr)) | app);
            if let Some((x, y)) = SAVED_POS.lock().unwrap().take() {
                SetWindowPos(hwnd as _, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            }
        }
        ShowWindow(hwnd as _, SW_SHOW);
        if IsIconic(hwnd as _) != 0 {
            ShowWindow(hwnd as _, SW_RESTORE);
        }
        SetForegroundWindow(hwnd as _);
    }
    crate::notify::set_in_tray(false);
}

#[cfg(not(windows))]
pub fn hide_main() {}
#[cfg(not(windows))]
pub fn show_main() {}

/// Start the tray icon (once, at program start).
#[cfg(windows)]
pub fn start() {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PeekMessageW, PostMessageW, TranslateMessage, MSG,
        PM_NOREMOVE, WM_APP, WM_CLOSE,
    };

    let (tx, rx) = mpsc::channel::<TrayCmd>();
    if SENDER.set(Mutex::new(tx)).is_err() {
        return; // already started
    }
    std::thread::spawn(move || {
        let to_icon = |kind| {
            let d = crate::icons::icon(kind);
            Icon::from_rgba(d.rgba.clone(), d.width, d.height).ok()
        };
        let (normal, unread) = (to_icon(crate::icons::Kind::Tray), to_icon(crate::icons::Kind::TrayUnread));
        let base = crate::icons::icon(crate::icons::Kind::Tray);
        // Current state: unread and download - the icon and tooltip are built from both.
        let mut is_unread = false;
        let mut download: Option<(String, f32)> = None;

        unsafe {
            // Create the thread's message queue before anyone starts writing to it.
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            THREAD_ID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
        }

        let open = MenuItem::new(tr!("Открыть Vaporust", "Open Vaporust"), true, None);
        let tray_close = CheckMenuItem::new(tr!("Крестик сворачивает в трей", "Close button minimizes to the tray"), true, close_to_tray(), None);
        let quit = MenuItem::new(tr!("Выход", "Exit"), true, None);
        let menu = Menu::new();
        let _ = menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &tray_close,
            &PredefinedMenuItem::separator(),
            &quit,
        ]);
        let (open_id, quit_id, close_id) = (open.id().clone(), quit.id().clone(), tray_close.id().clone());

        let mut builder = TrayIconBuilder::new()
            .with_tooltip("Vaporust")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false);
        if let Some(i) = normal.clone() {
            builder = builder.with_icon(i);
        }
        let Ok(tray) = builder.build() else { return };

        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == open_id {
                show_main();
            } else if e.id == close_id {
                // Windows toggles the menu checkbox itself; here only the setting.
                CLOSE_TO_TRAY.fetch_xor(true, Ordering::Relaxed);
            } else if e.id == quit_id {
                // Really close the program: the window receives a normal close request.
                EXIT.store(true, Ordering::Relaxed);
                let hwnd = crate::notify::main_hwnd();
                show_main();
                if hwnd != 0 {
                    unsafe { PostMessageW(hwnd as _, WM_CLOSE, 0, 0) };
                }
            }
        }));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show_main();
                set_unread(false);
            }
        }));

        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_APP && msg.hwnd.is_null() {
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            TrayCmd::Unread(on) => is_unread = on,
                            TrayCmd::Download(d) => download = d,
                        }
                        let icon = match (&download, is_unread) {
                            (_, true) => unread.clone(),
                            (Some((_, f)), false) => Icon::from_rgba(with_progress(&base, *f), base.width, base.height).ok(),
                            (None, false) => normal.clone(),
                        };
                        let _ = tray.set_icon(icon);
                        let mut tip = String::from(if is_unread { tr!("Vaporust - новые сообщения", "Vaporust - new messages") } else { "Vaporust" });
                        if let Some((name, f)) = &download {
                            tip.push_str(&trf!("
Загрузка: {name} - {:.0}%", "\nDownloading: {name} - {:.0}%", f * 100.0));
                        }
                        let _ = tray.set_tooltip(Some(tip));
                    }
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                // The menu checkbox always matches the setting.
                if tray_close.is_checked() != close_to_tray() {
                    tray_close.set_checked(close_to_tray());
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start() {}
