//! Иконка в области уведомлений (трей).
//!
//! Работает в отдельном потоке со своим циклом сообщений Windows: так она отвечает на
//! щелчки и показывает непрочитанные сообщения, даже когда главное окно спрятано
//! (спрятанное окно eframe само не перерисовывается). Окно показываем и прячем
//! напрямую через WinAPI по его HWND.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

enum TrayCmd {
    Unread(bool),
}

static SENDER: OnceLock<Mutex<mpsc::Sender<TrayCmd>>> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static UNREAD: AtomicBool = AtomicBool::new(false);
/// Пользователь выбрал «Выход» в меню трея: крестик больше не прячет окно.
static EXIT: AtomicBool = AtomicBool::new(false);

pub fn exit_requested() -> bool {
    EXIT.load(Ordering::Relaxed)
}

/// Показать красную точку на иконке (или убрать её).
pub fn set_unread(on: bool) {
    if UNREAD.swap(on, Ordering::Relaxed) == on {
        return;
    }
    send(TrayCmd::Unread(on));
}

fn send(cmd: TrayCmd) {
    let Some(tx) = SENDER.get() else { return };
    let _ = tx.lock().unwrap().send(cmd);
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};
        // Разбудить цикл сообщений потока трея.
        PostThreadMessageW(THREAD_ID.load(Ordering::Relaxed), WM_APP, 0, 0);
    }
}

/// Спрятать главное окно в трей.
#[cfg(windows)]
pub fn hide_main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    let hwnd = crate::notify::main_hwnd();
    if hwnd != 0 {
        unsafe { ShowWindow(hwnd as _, SW_HIDE) };
        crate::notify::set_in_tray(true);
    }
}

/// Показать главное окно из трея и вывести на передний план.
#[cfg(windows)]
pub fn show_main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 {
        return;
    }
    unsafe {
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

/// Запустить иконку в трее (один раз, при старте программы).
#[cfg(windows)]
pub fn start() {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PeekMessageW, PostMessageW, TranslateMessage, MSG,
        PM_NOREMOVE, WM_APP, WM_CLOSE,
    };

    let (tx, rx) = mpsc::channel::<TrayCmd>();
    if SENDER.set(Mutex::new(tx)).is_err() {
        return; // уже запущено
    }
    std::thread::spawn(move || {
        let to_icon = |kind| {
            let d = crate::icons::icon(kind);
            Icon::from_rgba(d.rgba.clone(), d.width, d.height).ok()
        };
        let (normal, unread) = (to_icon(crate::icons::Kind::Launcher), to_icon(crate::icons::Kind::LauncherUnread));

        unsafe {
            // Создать очередь сообщений потока до того, как в неё начнут писать.
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            THREAD_ID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
        }

        let open = MenuItem::new("Открыть Vaporust", true, None);
        let quit = MenuItem::new("Выход", true, None);
        let menu = Menu::new();
        let _ = menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit]);
        let (open_id, quit_id) = (open.id().clone(), quit.id().clone());

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
            } else if e.id == quit_id {
                // Закрыть программу по-настоящему: окно получит обычный запрос закрытия.
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
                            TrayCmd::Unread(on) => {
                                let icon = if on { unread.clone() } else { normal.clone() };
                                let _ = tray.set_icon(icon);
                                let _ = tray.set_tooltip(Some(if on {
                                    "Vaporust — новые сообщения"
                                } else {
                                    "Vaporust"
                                }));
                            }
                        }
                    }
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start() {}
