//! Portable mode: if a `portable.txt` file lies next to vaporust.exe, all data
//! (Vaporust settings and Aurelia's login session) is kept in the `VaporustData` folder next to
//! the exe, not in the Windows profile. There is no switch in settings yet (postponed, 2026-11-12):
//! the mode is enabled only manually with portable.txt; the login is encrypted with a Windows key as usual.

use std::path::{Path, PathBuf};

pub const MARKER: &str = "portable.txt";
pub const DIR: &str = "VaporustData";

/// Folder next to vaporust.exe.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Whether portable mode is on (portable.txt next to the exe).
pub fn enabled() -> bool {
    exe_dir().is_some_and(|d| d.join(MARKER).exists())
}

/// Portable mode data folder.
pub fn data_dir() -> Option<PathBuf> {
    exe_dir().map(|d| d.join(DIR))
}

/// Normal Vaporust settings folder (eframe): %APPDATA%\Vaporust\data.
pub fn normal_dir() -> Option<PathBuf> {
    eframe::storage_dir("Vaporust")
}

/// Call at the very start of main: in portable mode point Aurelia to its own folder.
/// Returns the path to the Vaporust settings file (app.ron) for eframe, or None.
pub fn init() -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    let dir = data_dir()?;
    let aurelia = dir.join("aurelia");
    let _ = std::fs::create_dir_all(&aurelia);
    // Aurelia reads the environment variable on every access to its folder.
    std::env::set_var("AURELIA_CONFIG_DIR", &aurelia);
    Some(dir.join("app.ron"))
}

/// Started by restart(): wait (at most 15 s) until the previous process has exited.
pub fn wait_for_previous() {
    let args: Vec<String> = std::env::args().collect();
    let Some(pid) = args.iter().position(|a| a == "--wait-pid").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u32>().ok()) else {
        return;
    };
    #[cfg(windows)]
    // SAFETY: the handle is checked and closed.
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 15_000);
            CloseHandle(h);
        }
    }
}

/// Restart Vaporust (a new process; the caller closes the current window).
pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // The login folder is chosen again at start (portable::init, accounts::init). An inherited
    // AURELIA_CONFIG_DIR would keep the previous account's folder: switching back to the main
    // account then signed into the other one.
    // --wait-pid: the new process waits until this one has exited and written its settings
    // (otherwise it could read the settings file first and the last changes would be lost).
    std::process::Command::new(exe)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .env_remove("AURELIA_CONFIG_DIR")
        .spawn()
        .map(|_| ())
        .map_err(|e| trf!("Не удалось перезапустить: {e}", "Could not restart: {e}"))
}
