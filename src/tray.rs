//! Иконка в области уведомлений (трей).
//!
//! Работает в отдельном потоке со своим циклом сообщений Windows: так она отвечает на
//! щелчки, пока главное окно спрятано в трей. Окно показываем и прячем напрямую через WinAPI.

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
/// Крестик окна прячет Vaporust в трей (галочка в меню трея).
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);

/// Начальное значение галочки (из настроек) - до `start`.
pub fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

pub fn close_to_tray() -> bool {
    CLOSE_TO_TRAY.load(Ordering::Relaxed)
}

pub fn exit_requested() -> bool {
    EXIT.load(Ordering::Relaxed)
}

/// Закрыть программу по-настоящему (крестик не прячет в трей), например для перезапуска.
pub fn request_exit() {
    EXIT.store(true, Ordering::Relaxed);
}

/// Показать красную точку на иконке (или убрать ее).
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

/// Тип значения GetWindowLongPtrW: на 64-битной Windows isize, на 32-битной - i32
/// (там это просто GetWindowLongW).
#[cfg(all(windows, target_pointer_width = "64"))]
type LongPtr = isize;
#[cfg(all(windows, target_pointer_width = "32"))]
type LongPtr = i32;

/// Где было окно до прятанья в трей (левый верхний угол, физические пиксели).
#[cfg(windows)]
static SAVED_POS: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// Спрятать главное окно в трей.
///
/// Окно НЕ скрывается по-настоящему: скрытое окно eframe перестает обновляться, и вместе с ним
/// засыпала бы вся программа (очередь, расписание, всплывающие уведомления). Вместо этого окно
/// уезжает за пределы экрана и становится «инструментальным» - без кнопки на панели задач и в Alt+Tab.
#[cfg(windows)]
pub fn hide_main() {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() {
        return;
    }
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd as _, &mut r) != 0 {
            *SAVED_POS.lock().unwrap() = Some((r.left, r.top));
        }
        // Стиль окна меняется только у скрытого окна, поэтому: спрятать -> поменять -> показать вне экрана.
        ShowWindow(hwnd as _, SW_HIDE);
        let ex = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW as LongPtr) & !(WS_EX_APPWINDOW as LongPtr));
        SetWindowPos(hwnd as _, std::ptr::null_mut(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
    }
    crate::notify::set_in_tray(true);
}

/// Показать главное окно из трея и вывести на передний план.
#[cfg(windows)]
pub fn show_main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsIconic, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
        SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, WS_EX_TOOLWINDOW,
    };
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 {
        return;
    }
    unsafe {
        if crate::notify::in_tray() {
            ShowWindow(hwnd as _, SW_HIDE);
            let ex = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, ex & !(WS_EX_TOOLWINDOW as LongPtr));
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

/// Запустить иконку в трее (один раз, при старте программы).
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
        return; // уже запущено
    }
    std::thread::spawn(move || {
        let to_icon = |kind| {
            let d = crate::icons::icon(kind);
            Icon::from_rgba(d.rgba.clone(), d.width, d.height).ok()
        };
        let (normal, unread) = (to_icon(crate::icons::Kind::Launcher), to_icon(crate::icons::Kind::LauncherUnread));

        unsafe {
            // Создать очередь сообщений потока до того, как в нее начнут писать.
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            THREAD_ID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
        }

        let open = MenuItem::new("Открыть Vaporust", true, None);
        let tray_close = CheckMenuItem::new("Крестик сворачивает в трей", true, close_to_tray(), None);
        let quit = MenuItem::new("Выход", true, None);
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
                // Галочку меню переключает сама Windows; здесь - только настройка.
                CLOSE_TO_TRAY.fetch_xor(true, Ordering::Relaxed);
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
                                    "Vaporust - новые сообщения"
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
                // Галочка меню всегда совпадает с настройкой.
                if tray_close.is_checked() != close_to_tray() {
                    tray_close.set_checked(close_to_tray());
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start() {}
