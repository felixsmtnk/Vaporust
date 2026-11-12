//! Портативный режим: если рядом с vaporust.exe лежит файл `portable.txt`, все данные
//! (настройки Vaporust и сессия входа Aurelia) хранятся в папке `VaporustData` рядом с
//! exe, а не в профиле Windows. Переключателя в настройках пока нет (отложено, 2026-11-12):
//! режим включается только вручную файлом portable.txt; вход шифруется ключом Windows, как обычно.

use std::path::{Path, PathBuf};

pub const MARKER: &str = "portable.txt";
pub const DIR: &str = "VaporustData";

/// Папка рядом с vaporust.exe.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Включен ли портативный режим (есть portable.txt рядом с exe).
pub fn enabled() -> bool {
    exe_dir().is_some_and(|d| d.join(MARKER).exists())
}

/// Папка данных портативного режима.
pub fn data_dir() -> Option<PathBuf> {
    exe_dir().map(|d| d.join(DIR))
}

/// Обычная папка настроек Vaporust (eframe): %APPDATA%\Vaporust\data.
pub fn normal_dir() -> Option<PathBuf> {
    eframe::storage_dir("Vaporust")
}

/// Вызвать в самом начале main: в портативном режиме направить Aurelia в свою папку.
/// Возвращает путь к файлу настроек Vaporust (app.ron) для eframe или None.
pub fn init() -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    let dir = data_dir()?;
    let aurelia = dir.join("aurelia");
    let _ = std::fs::create_dir_all(&aurelia);
    // Переменная окружения читается Aurelia при каждом обращении к ее папке.
    std::env::set_var("AURELIA_CONFIG_DIR", &aurelia);
    Some(dir.join("app.ron"))
}

/// Перезапустить Vaporust (новый процесс; текущее окно закрывает вызывающий).
pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| format!("Не удалось перезапустить: {e}"))
}
