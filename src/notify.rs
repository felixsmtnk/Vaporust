//! Новые сообщения: всплывающее уведомление Vaporust со звуком (popup.rs), а когда ни одно
//! окно Vaporust не активно - еще мигание кнопки в панели задач и точка на иконке в трее.
//! Вызывается прямо из сетевого потока.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

/// Системный идентификатор (HWND) главного окна.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// Уведомлять о новых сообщениях (общая галочка уведомлений И пункт «Сообщения»).
static MESSAGES: AtomicBool = AtomicBool::new(true);
/// Главное окно спрятано в трей (мигать в панели задач бесполезно).
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

/// С кем открыт чат (SteamID64; 0 - чат закрыт). Ставит окно чата.
static OPEN_CHAT: AtomicU64 = AtomicU64::new(0);

pub fn set_open_chat(friend: u64) {
    OPEN_CHAT.store(friend, Ordering::Relaxed);
}

/// Открыто ли окно чата (будить его есть смысл только тогда: запрос перерисовки
/// несуществующего окна заставляет eframe на миг создать пустое окно «egui window»).
pub fn chat_open() -> bool {
    OPEN_CHAT.load(Ordering::Relaxed) != 0
}

/// Пришло новое входящее сообщение от `friend`.
/// Уведомление со звуком - если не активен чат именно с ним (как в Steam);
/// мигание в панели задач и точка в трее - если ни одно окно Vaporust не активно.
#[cfg(windows)]
pub fn new_message(friend: u64, name: Option<String>, text: String, accent: Option<eframe::egui::Color32>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY};
    if !MESSAGES.load(Ordering::Relaxed) {
        return;
    }
    let focused = app_focused();
    if !(focused && OPEN_CHAT.load(Ordering::Relaxed) == friend) {
        let title = name.unwrap_or_else(|| "Новое сообщение".into());
        let (body, image) = crate::chatfmt::preview(&text);
        crate::popup::push_full(crate::popup::Kind::Message(friend), title, body, image, accent);
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
pub fn new_message(_friend: u64, _name: Option<String>, _text: String, _accent: Option<eframe::egui::Color32>) {}

