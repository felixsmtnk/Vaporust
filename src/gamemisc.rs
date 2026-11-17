//! Game page extras: own note for a game, a desktop shortcut and launching a game from the shortcut
//! (`vaporust.exe --run <appid>`): if Vaporust is already open, the second instance only passes
//! the game id to it via a request file and exits right away.

use crate::LauncherApp;
use eframe::egui;
use std::path::PathBuf;

/// Shortcut command-line argument: `--run <appid>`.
pub const RUN_ARG: &str = "--run";

/// "Launch the game" request file next to the Vaporust log.
fn request_path() -> Option<PathBuf> {
    Some(crate::errlog::path()?.parent()?.join("run_request.txt"))
}

/// Game id from the command line (`--run 70`).
pub fn run_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == RUN_ARG)?;
    args.get(i + 1)?.parse().ok()
}

/// Call at the start of main. With `--run`: write the request; if Vaporust is already running - true
/// (this instance must exit right away, the open one will launch the game).
pub fn hand_over_run() -> bool {
    let Some(appid) = run_arg() else { return false };
    if let Some(p) = request_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
        let _ = std::fs::write(&p, appid.to_string());
    }
    another_instance()
}

/// Whether another Vaporust is already running (named mutex per user session).
#[cfg(windows)]
fn another_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    // One name per data folder: a portable copy has its own run_request.txt, so it must not hand a
    // shortcut request to a normal copy that reads another folder (and the other way round).
    let tag = crate::errlog::path()
        .map(|p| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            p.to_string_lossy().to_lowercase().hash(&mut h);
            format!("{:x}", h.finish())
        })
        .unwrap_or_default();
    let name: Vec<u16> = format!("Local\\Vaporust-running-{tag}").encode_utf16().chain(Some(0)).collect();
    // The mutex lives until the process exits: we do not close it.
    let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    !h.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS
}

#[cfg(not(windows))]
fn another_instance() -> bool {
    false
}

/// Mark this instance as running (so the next one with `--run` passes the request to it).
pub fn mark_running() {
    let _ = another_instance();
}

impl LauncherApp {
    /// Once per second: there is a request to launch a game (from a shortcut) - show the window and launch.
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

    /// Window for the own note to a game.
    pub(crate) fn note_dialog(&mut self, ctx: &egui::Context) {
        let Some((appid, name, mut text)) = self.note_edit.take() else { return };
        let mut keep = true;
        egui::Window::new(trf!("Заметка: {name}", "Note: {name}"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak(tr!("Видна только вам: например, где вы остановились или настройки. Хранится открытым текстом в файле настроек - не пишите сюда пароли.", "Only you see it: for example where you stopped or settings. Kept as plain text in the settings file - do not put passwords here."));
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(4).desired_width(360.0));
                ui.horizontal(|ui| {
                    if ui.button(tr!("Сохранить", "Save")).clicked() {
                        if text.trim().is_empty() {
                            self.settings.game_notes.remove(&appid);
                        } else {
                            self.settings.game_notes.insert(appid, text.trim().to_string());
                        }
                        keep = false;
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
        if keep {
            self.note_edit = Some((appid, name, text));
        }
    }

    /// Desktop shortcut: launches the game through Vaporust (`vaporust.exe --run <appid>`),
    /// the icon comes from the game's exe (if found), otherwise Vaporust's.
    pub(crate) fn make_shortcut(&mut self, appid: u32, name: &str) {
        let Ok(exe) = std::env::current_exe() else { return };
        // Icon: exe of a non-Steam game or the first exe in the Steam game folder.
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
            Ok(()) => trf!("Ярлык «{file_name}» создан на рабочем столе.", "Shortcut \"{file_name}\" created on the desktop."),
            Err(e) => {
                crate::errlog::write(tr!("ярлык", "shortcut"), &e);
                trf!("Не удалось создать ярлык: {e}", "Could not create the shortcut: {e}")
            }
        };
        *self.launch_status.lock().unwrap() = text;
    }
}

/// First .exe in the root of the game folder (for the shortcut icon), excluding service installers.
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
    // The largest exe is usually the game itself.
    exes.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)));
    exes.into_iter().next()
}

/// Create a .lnk on the desktop via WScript.Shell (PowerShell without a window). Paths and the name
/// are passed via environment variables - so quotes and Russian letters need no escaping.
#[cfg(windows)]
fn create_lnk(target: &std::path::Path, args: &str, icon: &std::path::Path, name: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = "$d=[Environment]::GetFolderPath('Desktop'); \
                  $s=(New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $d ($env:VP_NAME + '.lnk'))); \
                  $s.TargetPath=$env:VP_TARGET; $s.Arguments=$env:VP_ARGS; $s.IconLocation=$env:VP_ICON + ',0'; \
                  $s.WorkingDirectory=(Split-Path $env:VP_TARGET); $s.Save()";
    let out = std::process::Command::new(crate::util::system_exe("powershell.exe"))
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
    Err(tr!("только Windows", "Windows only").into())
}
