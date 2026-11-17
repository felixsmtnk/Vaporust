//! New messages: a Vaporust popup notification with sound (popup.rs), and when no
//! Vaporust window is active - also taskbar button flashing and a dot on the tray icon.
//! Called straight from the network thread.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

/// System identifier (HWND) of the main window.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// Notify about new messages (the general notifications checkbox AND the "Messages" item).
static MESSAGES: AtomicBool = AtomicBool::new(true);
/// The main window is hidden in the tray (flashing in the taskbar is pointless).
static IN_TRAY: AtomicBool = AtomicBool::new(false);

pub fn init(hwnd: isize) {
    MAIN_HWND.store(hwnd, Ordering::Relaxed);
}

pub fn main_hwnd() -> isize {
    MAIN_HWND.load(Ordering::Relaxed)
}

pub fn set_messages(on: bool) {
    MESSAGES.store(on, Ordering::Relaxed);
}

pub fn set_in_tray(on: bool) {
    IN_TRAY.store(on, Ordering::Relaxed);
}

pub fn in_tray() -> bool {
    IN_TRAY.load(Ordering::Relaxed)
}

/// Whether any Vaporust window is active now (main, chat, properties).
#[cfg(windows)]
pub fn app_focused() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, &mut pid);
        pid == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
pub fn app_focused() -> bool {
    true
}

/// Who the chat is open with (SteamID64; 0 - chat closed). Set by the chat window.
static OPEN_CHAT: AtomicU64 = AtomicU64::new(0);

pub fn set_open_chat(friend: u64) {
    OPEN_CHAT.store(friend, Ordering::Relaxed);
}

/// Whether the chat window is open (waking it only makes sense then: a repaint request for
/// a nonexistent window makes eframe briefly create an empty "egui window").
pub fn chat_open() -> bool {
    OPEN_CHAT.load(Ordering::Relaxed) != 0
}

/// The chat is a separate window right now (not docked in the main one): only then it can be woken
/// or focused by its viewport id.
static CHAT_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_chat_window(open: bool) {
    CHAT_WINDOW.store(open, Ordering::Relaxed);
}

pub fn chat_window_open() -> bool {
    CHAT_WINDOW.load(Ordering::Relaxed) && chat_open()
}

/// A new incoming message from `friend` arrived.
/// Notification with sound - unless the chat with exactly this friend is active (as in Steam);
/// taskbar flashing and the tray dot - if no Vaporust window is active.
#[cfg(windows)]
pub fn new_message(friend: u64, name: Option<String>, text: String, accent: Option<eframe::egui::Color32>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY};
    if !MESSAGES.load(Ordering::Relaxed) {
        return;
    }
    let focused = app_focused();
    if !(focused && OPEN_CHAT.load(Ordering::Relaxed) == friend) {
        let title = name.unwrap_or_else(|| tr!("Новое сообщение", "New message").into());
        let (body, image) = crate::chatfmt::preview(&text);
        crate::popup::push_full(crate::popup::Kind::Message(friend), title, body, image, accent);
    }
    if focused {
        return;
    }
    let hwnd = MAIN_HWND.load(Ordering::Relaxed);
    unsafe {
        if hwnd != 0 && !IN_TRAY.load(Ordering::Relaxed) {
            // Flash the taskbar button until the window is opened.
            let info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd: hwnd as _,
                dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                uCount: 0,
                dwTimeout: 0,
            };
            FlashWindowEx(&info);
        }
    }
    crate::tray::set_unread(true);
}

#[cfg(not(windows))]
pub fn new_message(_friend: u64, _name: Option<String>, _text: String, _accent: Option<eframe::egui::Color32>) {}

