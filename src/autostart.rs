//! Автозапуск с Windows: значение «Vaporust» в HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! (как у других программ, без прав администратора). Запуск с ключом --tray - сразу в трей.

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "Vaporust";
pub const TRAY_ARG: &str = "--tray";

/// Запущен ли Vaporust с ключом «сразу в трей».
pub fn started_in_tray() -> bool {
    std::env::args().any(|a| a == TRAY_ARG)
}

#[cfg(windows)]
pub fn enabled() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN)
        .and_then(|k| k.get_value::<String, _>(NAME))
        .is_ok_and(|v| current_exe_quoted().is_some_and(|exe| v.starts_with(&exe)))
}

#[cfg(windows)]
fn current_exe_quoted() -> Option<String> {
    std::env::current_exe().ok().map(|p| format!("\"{}\"", p.display()))
}

/// Включить или выключить автозапуск (в трей). Пишется путь к этому exe.
#[cfg(windows)]
pub fn set(on: bool) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(|e| e.to_string())?;
    if on {
        let exe = current_exe_quoted().ok_or("не найден путь к программе")?;
        key.set_value(NAME, &format!("{exe} {TRAY_ARG}")).map_err(|e| e.to_string())
    } else {
        match key.delete_value(NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(windows))]
pub fn enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set(_on: bool) -> Result<(), String> {
    Err("Только для Windows".into())
}
