//! Портативный режим: если рядом с vaporust.exe лежит файл `portable.txt`, все данные
//! (настройки Vaporust и сессия входа Aurelia) хранятся в папке `VaporustData` рядом с
//! exe, а не в профиле Windows. Удобно для флешки.

use std::path::{Path, PathBuf};

pub const MARKER: &str = "portable.txt";
pub const DIR: &str = "VaporustData";

/// Папка рядом с vaporust.exe.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Включён ли портативный режим (есть portable.txt рядом с exe).
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

/// Обычная папка Aurelia: %USERPROFILE%\.config\Aurelia.
fn normal_aurelia_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".config").join("Aurelia"))
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
    // Переменная окружения читается Aurelia при каждом обращении к её папке.
    std::env::set_var("AURELIA_CONFIG_DIR", &aurelia);
    Some(dir.join("app.ron"))
}

/// Скопировать содержимое папки (без подпапки images — это кэш картинок).
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            if e.file_name() != "images" {
                copy_dir(&p, &to.join(e.file_name()))?;
            }
        } else {
            std::fs::copy(&p, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// Включить портативный режим: перенести (скопировать) настройки и вход в папку
/// рядом с exe и создать portable.txt. Действует после перезапуска.
pub fn enable() -> Result<(), String> {
    let exe = exe_dir().ok_or("Не удалось узнать папку программы.")?;
    let dir = exe.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать папку {}: {e}", dir.display()))?;
    if let Some(src) = normal_dir().map(|d| d.join("app.ron")).filter(|p| p.exists()) {
        let _ = std::fs::copy(src, dir.join("app.ron"));
    }
    if let Some(src) = normal_aurelia_dir().filter(|p| p.exists()) {
        copy_dir(&src, &dir.join("aurelia")).map_err(|e| format!("Не удалось скопировать данные входа: {e}"))?;
    }
    std::fs::write(
        exe.join(MARKER),
        "Портативный режим Vaporust: данные хранятся в папке VaporustData рядом с программой.\n\
         Удалите этот файл, чтобы вернуться к обычному режиму.\n",
    )
    .map_err(|e| format!("Не удалось создать {MARKER} (папка программы доступна для записи?): {e}"))
}

/// Выключить портативный режим: убрать portable.txt (папка VaporustData остаётся).
pub fn disable() -> Result<(), String> {
    let exe = exe_dir().ok_or("Не удалось узнать папку программы.")?;
    std::fs::remove_file(exe.join(MARKER)).map_err(|e| format!("Не удалось удалить {MARKER}: {e}"))
}

/// Перезапустить Vaporust (новый процесс; текущее окно закрывает вызывающий).
pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| format!("Не удалось перезапустить: {e}"))
}
