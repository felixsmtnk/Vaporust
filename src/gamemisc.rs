//! Мелочи страницы игры: своя заметка к игре, ярлык на рабочем столе и запуск игры по ярлыку
//! (`vaporust.exe --run <appid>`): если Vaporust уже открыт, второй экземпляр только передает
//! ему номер игры через файл-просьбу и сразу закрывается.

use crate::LauncherApp;
use eframe::egui;
use std::path::PathBuf;

/// Аргумент командной строки ярлыка: `--run <appid>`.
pub const RUN_ARG: &str = "--run";

/// Файл-просьба «запусти игру» рядом с журналом Vaporust.
fn request_path() -> Option<PathBuf> {
    Some(crate::errlog::path()?.parent()?.join("run_request.txt"))
}

/// Номер игры из командной строки (`--run 70`).
pub fn run_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == RUN_ARG)?;
    args.get(i + 1)?.parse().ok()
}

/// Вызвать в начале main. Есть `--run`: записать просьбу; если Vaporust уже запущен - true
/// (этому экземпляру надо сразу выйти, игру запустит уже открытый).
pub fn hand_over_run() -> bool {
    let Some(appid) = run_arg() else { return false };
    if let Some(p) = request_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
        let _ = std::fs::write(&p, appid.to_string());
    }
    another_instance()
}

/// Запущен ли уже другой Vaporust (именованный мьютекс на сеанс пользователя).
#[cfg(windows)]
fn another_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\Vaporust-running".encode_utf16().chain(Some(0)).collect();
    // Мьютекс живет до выхода процесса: его не закрываем.
    let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    !h.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS
}

#[cfg(not(windows))]
fn another_instance() -> bool {
    false
}

/// Отметить этот экземпляр запущенным (чтобы следующий с `--run` передал просьбу ему).
pub fn mark_running() {
    let _ = another_instance();
}

impl LauncherApp {
    /// Раз в секунду: есть просьба запустить игру (от ярлыка) - показать окно и запустить.
    pub(crate) fn run_request_tick(&mut self, ctx: &egui::Context) {
        let due = self.run_check.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if !due || self.games.is_empty() {
            return;
        }
        self.run_check = Some(std::time::Instant::now());
        let Some(p) = request_path() else { return };
        let Ok(text) = std::fs::read_to_string(&p) else { return };
        let _ = std::fs::remove_file(&p);
        let Ok(appid) = text.trim().parse::<u32>() else { return };
        crate::tray::show_main();
        self.close_extras(crate::View::Library);
        self.view = crate::View::Library;
        self.lib_page = Some(appid);
        self.start(appid, ctx);
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    /// Окно своей заметки к игре.
    pub(crate) fn note_dialog(&mut self, ctx: &egui::Context) {
        let Some((appid, name, mut text)) = self.note_edit.take() else { return };
        let mut keep = true;
        egui::Window::new(format!("Заметка: {name}"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak("Видна только вам: например, где вы остановились, коды или настройки.");
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(4).desired_width(360.0));
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() {
                        if text.trim().is_empty() {
                            self.settings.game_notes.remove(&appid);
                        } else {
                            self.settings.game_notes.insert(appid, text.trim().to_string());
                        }
                        keep = false;
                    }
                    if ui.button("Отмена").clicked() {
                        keep = false;
                    }
                });
            });
        if keep {
            self.note_edit = Some((appid, name, text));
        }
    }

    /// Ярлык на рабочем столе: запускает игру через Vaporust (`vaporust.exe --run <appid>`),
    /// значок - из exe игры (если найден), иначе Vaporust.
    pub(crate) fn make_shortcut(&mut self, appid: u32, name: &str) {
        let Ok(exe) = std::env::current_exe() else { return };
        // Значок: exe сторонней игры или первый exe в папке игры Steam.
        let icon = if crate::custom::is_custom(appid) {
            crate::custom::exe_of(appid).map(PathBuf::from)
        } else {
            self.games.iter().find(|g| g.appid == appid).and_then(|g| first_exe(&g.dir))
        }
        .unwrap_or_else(|| exe.clone());
        let file_name: String = name
            .chars()
            .map(|c| if "\\/:*?\"<>|".contains(c) { ' ' } else { c })
            .collect::<String>()
            .trim()
            .to_string();
        let text = match create_lnk(&exe, &format!("{RUN_ARG} {appid}"), &icon, &file_name) {
            Ok(()) => format!("Ярлык «{file_name}» создан на рабочем столе."),
            Err(e) => {
                crate::errlog::write("ярлык", &e);
                format!("Не удалось создать ярлык: {e}")
            }
        };
        *self.launch_status.lock().unwrap() = text;
    }
}

/// Первый .exe в корне папки игры (для значка ярлыка), без служебных установщиков.
fn first_exe(dir: &std::path::Path) -> Option<PathBuf> {
    let mut exes: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            !["unins", "setup", "crash", "redist", "vc_redist", "dxsetup", "launcherpatch"].iter().any(|s| n.contains(s))
        })
        .collect();
    // Самый большой exe - обычно сама игра.
    exes.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)));
    exes.into_iter().next()
}

/// Создать .lnk на рабочем столе через WScript.Shell (PowerShell без окна). Пути и имя
/// передаются через переменные окружения - так не нужно экранировать кавычки и русские буквы.
#[cfg(windows)]
fn create_lnk(target: &std::path::Path, args: &str, icon: &std::path::Path, name: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = "$d=[Environment]::GetFolderPath('Desktop'); \
                  $s=(New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $d ($env:VP_NAME + '.lnk'))); \
                  $s.TargetPath=$env:VP_TARGET; $s.Arguments=$env:VP_ARGS; $s.IconLocation=$env:VP_ICON + ',0'; \
                  $s.WorkingDirectory=(Split-Path $env:VP_TARGET); $s.Save()";
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("VP_NAME", name)
        .env("VP_TARGET", target)
        .env("VP_ARGS", args)
        .env("VP_ICON", icon)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect())
    }
}

#[cfg(not(windows))]
fn create_lnk(_t: &std::path::Path, _a: &str, _i: &std::path::Path, _n: &str) -> Result<(), String> {
    Err("только Windows".into())
}
