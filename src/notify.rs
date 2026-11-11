//! Уведомления о новых сообщениях, когда ни одно окно Vaporust не активно:
//! мигание кнопки в панели задач и короткий системный звук.
//! Вызывается прямо из сетевого потока, поэтому работает и при свёрнутом окне.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

/// Системный идентификатор (HWND) главного окна.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static SOUND: AtomicBool = AtomicBool::new(true);
/// Главное окно спрятано в трей (мигать в панели задач бесполезно).
static IN_TRAY: AtomicBool = AtomicBool::new(false);

pub fn init(hwnd: isize) {
    MAIN_HWND.store(hwnd, Ordering::Relaxed);
}

pub fn main_hwnd() -> isize {
    MAIN_HWND.load(Ordering::Relaxed)
}

pub fn set_sound(on: bool) {
    SOUND.store(on, Ordering::Relaxed);
}

pub fn set_in_tray(on: bool) {
    IN_TRAY.store(on, Ordering::Relaxed);
}

/// Активно ли сейчас какое-нибудь окно Vaporust (главное, чат, свойства).
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

/// С кем открыт чат (SteamID64; 0 — чат закрыт). Ставит окно чата.
static OPEN_CHAT: AtomicU64 = AtomicU64::new(0);

pub fn set_open_chat(friend: u64) {
    OPEN_CHAT.store(friend, Ordering::Relaxed);
}

/// Звук «новое сообщение» из звуковой схемы Windows (Notification.IM),
/// если его нет — обычный системный сигнал.
#[cfg(windows)]
pub fn play_message_sound() -> bool {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
    let alias: Vec<u16> = "Notification.IM".encode_utf16().chain(Some(0)).collect();
    unsafe {
        if PlaySoundW(alias.as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC | SND_NODEFAULT) != 0 {
            return true;
        }
        MessageBeep(MB_ICONASTERISK) != 0
    }
}

/// Пришло новое входящее сообщение от `friend`.
/// Звук — если не открыт и не активен чат именно с ним (как в Steam);
/// мигание в панели задач и точка в трее — если ни одно окно Vaporust не активно.
#[cfg(windows)]
pub fn new_message(friend: u64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY};
    let focused = app_focused();
    if SOUND.load(Ordering::Relaxed) && !(focused && OPEN_CHAT.load(Ordering::Relaxed) == friend) {
        play_message_sound();
    }
    if focused {
        return;
    }
    let hwnd = MAIN_HWND.load(Ordering::Relaxed);
    unsafe {
        if hwnd != 0 && !IN_TRAY.load(Ordering::Relaxed) {
            // Мигать кнопкой в панели задач, пока окно не откроют.
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
pub fn new_message(_friend: u64) {}

#[cfg(test)]
mod tests {
    /// cargo test sound_plays -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sound_plays() {
        let ok = super::play_message_sound();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        println!("sound played: {ok}");
        assert!(ok);
    }
}
