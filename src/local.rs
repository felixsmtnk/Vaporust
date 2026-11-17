//! Local Steam: finding it via the registry, scanning libraries, launching a game
//! through a stripped-down Steam.
//!
//! Version 0.3: Steam and the game are detected by processes, not by the registry
//! (ActiveUser and RunningAppID in the registry turned out to be unreliable).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::{AtomicBool, Ordering}, Mutex},
    thread,
    time::{Duration, Instant},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

// Flags for the stripped-down Steam launch. Valve may change their support between versions.
/// Default flags of the stripped-down Steam (changed in "Settings").
pub const DEFAULT_STEAM_ARGS: &str = "-silent -no-browser -cef-disable-gpu -nofriendsui";

#[derive(Clone)]
pub struct Game {
    pub appid: u32,
    pub name: String,
    /// Game folder: <library>\steamapps\common\<installdir>
    pub dir: PathBuf,
    /// Fully installed (StateFlags contains 4). false = not finished downloading.
    pub ready: bool,
    /// Steam marked the game as needing an update (StateFlags != 4).
    pub needs_update: bool,
    /// Size on disk (SizeOnDisk from appmanifest), bytes.
    pub size: u64,
    /// An unfinished Vaporust download hidden from Steam (appmanifest renamed).
    pub stashed: bool,
}

#[derive(Clone)]
pub struct SteamInfo {
    pub dir: PathBuf,
    pub exe: PathBuf,
}

#[derive(Clone, Default)]
pub struct LaunchOptions {
    /// Experiment: freeze steamwebhelper while playing.
    /// Closing it is useless: Steam restarts it right away (checked in 0.3).
    pub freeze_webhelper: bool,
    /// Soft freezing (priority and efficiency mode) instead of suspending the process.
    pub freeze_soft: bool,
    /// Launch flags of the hidden Steam; empty - DEFAULT_STEAM_ARGS.
    pub steam_args: String,
}

// ---------- Registry ----------

#[cfg(windows)]
pub fn find_steam() -> Option<SteamInfo> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()?;
    let dir: String = key.get_value("SteamPath").ok()?;
    let exe: String = key.get_value("SteamExe").ok()?;
    Some(SteamInfo {
        dir: PathBuf::from(dir),
        exe: PathBuf::from(exe),
    })
}

#[cfg(not(windows))]
pub fn find_steam() -> Option<SteamInfo> {
    None
}

#[cfg(windows)]
fn active_user() -> u32 {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam\\ActiveProcess")
        .and_then(|k| k.get_value::<u32, _>("ActiveUser"))
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn active_user() -> u32 {
    0
}

/// Resets a stuck ActiveUser: after Steam is closed manually it may stay != 0.
#[cfg(windows)]
fn reset_active_user() {
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
        RegKey,
    };
    if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Software\\Valve\\Steam\\ActiveProcess", KEY_SET_VALUE)
    {
        let _ = k.set_value("ActiveUser", &0u32);
    }
}

#[cfg(not(windows))]
fn reset_active_user() {}

// ---------- VDF/ACF parsing (minimal, line by line) ----------

/// A `"key"  "value"` line. The value may be empty (`"BetaKey"  ""`) and may contain
/// escaped quotes and backslashes (\" and \\), as Steam writes them.
fn kv(line: &str) -> Option<(String, String)> {
    let mut chars = line.trim().chars().peekable();
    let mut quoted = || -> Option<String> {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.next()? != '"' {
            return None;
        }
        let mut s = String::new();
        loop {
            match chars.next()? {
                '"' => return Some(s),
                '\\' => match chars.peek() {
                    Some('\\') | Some('"') => s.push(chars.next()?),
                    _ => s.push('\\'),
                },
                c => s.push(c),
            }
        }
    };
    let k = quoted()?;
    let v = quoted()?;
    if k.is_empty() {
        return None;
    }
    Some((k, v))
}

#[cfg(test)]
mod kv_tests {
    #[test]
    fn values() {
        assert_eq!(super::kv("\t\t\"BetaKey\"\t\t\"\""), Some(("BetaKey".into(), String::new())));
        assert_eq!(super::kv("\"name\"\t\t\"Say \\\"hi\\\"\""), Some(("name".into(), "Say \"hi\"".into())));
        assert_eq!(super::kv("\"path\"\t\t\"D:\\\\Games\\\\Steam\""), Some(("path".into(), "D:\\Games\\Steam".into())));
        assert_eq!(super::kv("\t\"apps\""), None);
        assert_eq!(super::kv("{"), None);
    }
}

/// A "key" "value" line from VDF (for other modules).
pub fn kv_pub(line: &str) -> Option<(String, String)> {
    kv(line)
}

/// Steam libraries without duplicates (the main Steam folder first).
pub fn library_dirs(steam_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![steam_dir.to_path_buf()];
    let vdf = steam_dir.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = fs::read_to_string(vdf) {
        for line in text.lines() {
            if let Some((k, v)) = kv(line) {
                if k == "path" {
                    let p = PathBuf::from(v);
                    let n = norm(&p);
                    if !dirs.iter().any(|d| norm(d).trim_end_matches('\\') == n.trim_end_matches('\\')) {
                        dirs.push(p);
                    }
                }
            }
        }
    }
    dirs
}

/// Free space on the disk where the folder is.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// How many game files already exist in the library (for resuming an interrupted install).
pub fn installed_size(library: &Path, appid: u32) -> u64 {
    let acf = library
        .join("steamapps")
        .join(format!("appmanifest_{appid}.acf"));
    let Ok(text) = fs::read_to_string(acf) else {
        return 0;
    };
    let Some(dir) = text
        .lines()
        .filter_map(kv)
        .find(|(k, _)| k == "installdir")
        .map(|(_, v)| v)
    else {
        return 0;
    };
    fn walk(p: &Path) -> u64 {
        let Ok(rd) = fs::read_dir(p) else { return 0 };
        rd.flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path()),
                Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => 0,
            })
            .sum()
    }
    walk(&library.join("steamapps").join("common").join(dir))
}

/// Whether steam.exe is running now.
pub fn steam_running() -> bool {
    let t = std::time::Instant::now();
    let v = process_named("steam.exe");
    crate::stats::proc_scan(t);
    v
}

/// Whether a process with this name exists. A ToolHelp snapshot - names only, no exe paths (sysinfo
/// asked every process for its path: ~10 ms versus ~1 ms).
#[cfg(windows)]
pub fn process_named(name: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Procs::new().named_exists(name);
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(name) {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
pub fn process_named(name: &str) -> bool {
    Procs::new().named_exists(name)
}

pub fn scan_games(steam_dir: &Path) -> Vec<Game> {
    let mut games: Vec<Game> = Vec::new();
    for lib in library_dirs(steam_dir) {
        let Ok(entries) = fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            // Unfinished Vaporust downloads hidden from Steam (see stash_manifest) are ours too.
            let stashed = fname.starts_with("appmanifest_") && fname.ends_with(STASH_EXT);
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) && !stashed {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else {
                continue;
            };
            let (mut appid, mut name, mut installdir, mut flags, mut size) = (None, None, None, 0u32, 0u64);
            for line in text.lines() {
                if let Some((k, v)) = kv(line) {
                    match k.as_str() {
                        "appid" if appid.is_none() => appid = v.parse::<u32>().ok(),
                        "name" if name.is_none() => name = Some(v),
                        "installdir" if installdir.is_none() => installdir = Some(v),
                        "StateFlags" => flags = v.parse().unwrap_or(0),
                        "SizeOnDisk" => size = v.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            if let (Some(appid), Some(name), Some(installdir)) = (appid, name, installdir) {
                // 228980 = Steamworks Common Redistributables, not a game
                if appid != 228980 && !games.iter().any(|g| g.appid == appid) {
                    let dir = lib.join("steamapps").join("common").join(installdir);
                    let ready = !stashed && flags & 4 != 0;
                    let needs_update = ready && flags != 4;
                    games.push(Game { appid, name, dir, ready, needs_update, size, stashed });
                }
            }
        }
    }
    games
}

// ---------- Game appmanifest: state, language, beta ----------

/// Suffix of a hidden appmanifest. Steam reads only appmanifest_*.acf, so it does not see
/// an unfinished game paused in Vaporust and does not resume it by itself.
pub const STASH_EXT: &str = ".acf.vaporust-paused";

fn manifest_paths(library: &Path, appid: u32) -> (PathBuf, PathBuf) {
    let dir = library.join("steamapps");
    (
        dir.join(format!("appmanifest_{appid}.acf")),
        dir.join(format!("appmanifest_{appid}{STASH_EXT}")),
    )
}

/// Hide the appmanifest of an unfinished game from Steam. Only while Steam is closed.
/// Fully installed games (StateFlags == 4) are not touched.
pub fn stash_manifest(library: &Path, appid: u32) -> bool {
    let (acf, hidden) = manifest_paths(library, appid);
    let info = read_acf(&acf);
    if !acf.exists() || info.state_flags == 4 {
        return false;
    }
    fs::rename(&acf, &hidden).is_ok()
}

/// Put the appmanifest back before resuming the download.
pub fn unstash_manifest(library: &Path, appid: u32) -> bool {
    let (acf, hidden) = manifest_paths(library, appid);
    hidden.exists() && !acf.exists() && fs::rename(&hidden, &acf).is_ok()
}

/// The game's steamapps\appmanifest_<appid>.acf (next to the common folder).
pub fn appmanifest_path(game: &Game) -> PathBuf {
    game.dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new(""))
        .join(format!("appmanifest_{}.acf", game.appid))
}

#[derive(Default, Clone)]
pub struct AcfInfo {
    pub state_flags: u32,
    pub size_on_disk: u64,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
    pub last_updated: u64,
    /// UserConfig - what the user chose; MountedConfig - what is installed now.
    pub user_language: String,
    pub user_beta: String,
    pub mounted_language: String,
    pub mounted_beta: String,
}

impl AcfInfo {
    /// Installed language: MountedConfig, and when Steam did not write it - English (the default).
    pub fn installed_language(&self) -> &str {
        if self.mounted_language.is_empty() { "english" } else { &self.mounted_language }
    }

    /// Another language was chosen and is not installed yet.
    pub fn language_pending(&self) -> bool {
        !self.user_language.is_empty() && !self.user_language.eq_ignore_ascii_case(self.installed_language())
    }

    /// Steam is downloading something / must update (4 = fully installed and up to date).
    pub fn updating(&self) -> bool {
        self.state_flags != 0 && self.state_flags != 4
    }
}

pub fn read_acf(path: &Path) -> AcfInfo {
    let mut info = AcfInfo::default();
    let Ok(text) = fs::read_to_string(path) else {
        return info;
    };
    let mut section = String::new();
    let mut pending = String::new();
    let mut depth = 0;
    for line in text.lines() {
        let t = line.trim();
        if t == "{" {
            depth += 1;
            if depth == 2 {
                section = pending.clone();
            }
            continue;
        }
        if t == "}" {
            if depth == 2 {
                section.clear();
            }
            depth -= 1;
            continue;
        }
        let Some((k, v)) = kv(t) else {
            if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                pending = k.to_string();
            }
            continue;
        };
        let num = || v.parse::<u64>().unwrap_or(0);
        match (depth, section.as_str(), k.to_lowercase().as_str()) {
            (1, _, "stateflags") => info.state_flags = num() as u32,
            (1, _, "sizeondisk") => info.size_on_disk = num(),
            (1, _, "bytestodownload") => info.bytes_to_download = num(),
            (1, _, "bytesdownloaded") => info.bytes_downloaded = num(),
            (1, _, "lastupdated") => info.last_updated = num(),
            (2, "UserConfig", "language") => info.user_language = v,
            // "public" is the main branch, the same as an empty key (Steam writes both).
            (2, "UserConfig", "betakey") => info.user_beta = if v.eq_ignore_ascii_case("public") { String::new() } else { v },
            (2, "MountedConfig", "language") => info.mounted_language = v,
            (2, "MountedConfig", "betakey") => info.mounted_beta = if v.eq_ignore_ascii_case("public") { String::new() } else { v },
            _ => {}
        }
    }
    info
}

/// Write UserConfig.<key> (empty value - remove the key) and mark the game as
/// needing an update: Steam downloads what is needed on the next launch. Only while Steam is closed.
pub fn set_user_config(path: &Path, key: &str, value: &str) -> Result<(), String> {
    set_config_block(path, "UserConfig", key, value, true)
}

/// The same in MountedConfig ("what is installed now") - without the "update needed" mark.
/// Needed after changing the beta without Steam: the new branch files are already downloaded.
pub fn set_mounted_config(path: &Path, key: &str, value: &str) -> Result<(), String> {
    set_config_block(path, "MountedConfig", key, value, false)
}

fn set_config_block(path: &Path, block: &str, key: &str, value: &str, mark_update: bool) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    let (mut depth, mut in_user, mut done, mut pending) = (0, false, false, String::new());
    let mut user_seen = false;
    for line in &lines {
        let t = line.trim();
        if t == "{" {
            depth += 1;
            in_user = depth == 2 && pending.eq_ignore_ascii_case(block);
            user_seen |= in_user;
            out.push(line.to_string());
            continue;
        }
        if t == "}" {
            if in_user && depth == 2 && !done && !value.is_empty() {
                out.push(format!("\t\t\"{key}\"\t\t\"{value}\""));
                done = true;
            }
            if depth == 2 {
                in_user = false;
            }
            if depth == 1 && !user_seen && !value.is_empty() {
                // No block - add it before the end of the file.
                out.push(format!("\t\"{block}\""));
                out.push("\t{".into());
                out.push(format!("\t\t\"{key}\"\t\t\"{value}\""));
                out.push("\t}".into());
                user_seen = true;
            }
            depth -= 1;
            out.push(line.to_string());
            continue;
        }
        if let Some((k, _)) = kv(t) {
            if in_user && k.eq_ignore_ascii_case(key) {
                if !value.is_empty() {
                    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    out.push(format!("{indent}\"{k}\"\t\t\"{value}\""));
                }
                done = true;
                continue;
            }
            if mark_update && depth == 1 && k.eq_ignore_ascii_case("StateFlags") {
                // 2 = UpdateRequired: Steam checks the configuration and downloads the files.
                let flags = kv(t).and_then(|(_, v)| v.parse::<u32>().ok()).unwrap_or(4) | 2;
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                out.push(format!("{indent}\"StateFlags\"\t\t\"{flags}\""));
                continue;
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            pending = k.to_string();
        }
        out.push(line.to_string());
    }
    let backup = path.with_extension("acf.vaporust.bak");
    if !backup.exists() {
        let _ = fs::copy(path, &backup);
    }
    let mut body = out.join("\n");
    body.push('\n');
    crate::util::write_atomic(path, body.as_bytes()).map_err(|e| trf!("Не удалось записать файл игры: {e}", "Could not write the game file: {e}"))
}

/// Games installed in several Steam libraries at once: (appid, name, library
/// folders). Because of such duplicates Steam sees two different game files and may keep
/// "updating" it endlessly.
pub fn find_duplicates(steam_dir: &Path) -> Vec<(u32, String, Vec<PathBuf>)> {
    let mut map: std::collections::BTreeMap<u32, (String, Vec<PathBuf>)> = Default::default();
    for lib in library_dirs(steam_dir) {
        let Ok(rd) = fs::read_dir(lib.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_prefix("appmanifest_").and_then(|s| s.strip_suffix(".acf")) else { continue };
            let Ok(appid) = id.parse::<u32>() else { continue };
            let title = fs::read_to_string(e.path())
                .ok()
                .and_then(|t| t.lines().find_map(|l| kv(l).filter(|(k, _)| k.eq_ignore_ascii_case("name")).map(|(_, v)| v)))
                .unwrap_or_else(|| appid.to_string());
            let entry = map.entry(appid).or_insert_with(|| (title, Vec::new()));
            if !entry.1.contains(&lib) {
                entry.1.push(lib.clone());
            }
        }
    }
    map.into_iter().filter(|(_, (_, libs))| libs.len() > 1).map(|(id, (n, l))| (id, n, l)).collect()
}

/// Installed depot versions of a game from appmanifest: InstalledDepots, depot -> manifest ID.
pub fn installed_depots(path: &Path) -> std::collections::BTreeMap<u32, u64> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else { return out };
    let mut stack: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t == "{" {
            stack.push(std::mem::take(&mut pending));
            continue;
        }
        if t == "}" {
            stack.pop();
            continue;
        }
        if let Some((k, v)) = kv(t) {
            if stack.len() == 3 && stack[1].eq_ignore_ascii_case("InstalledDepots") && k.eq_ignore_ascii_case("manifest") {
                if let (Ok(d), Ok(m)) = (stack[2].parse(), v.parse()) {
                    out.insert(d, m);
                }
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            pending = k.to_string();
        }
    }
    out
}

/// Sizes of installed depots from appmanifest (InstalledDepots: depot -> size).
pub fn installed_depot_sizes(path: &Path) -> std::collections::BTreeMap<u32, u64> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else { return out };
    let mut stack: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t == "{" {
            stack.push(std::mem::take(&mut pending));
        } else if t == "}" {
            stack.pop();
        } else if let Some((k, v)) = kv(t) {
            if stack.len() == 3 && stack[1].eq_ignore_ascii_case("InstalledDepots") && k.eq_ignore_ascii_case("size") {
                if let (Ok(d), Ok(s)) = (stack[2].parse(), v.parse()) {
                    out.insert(d, s);
                }
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            pending = k.to_string();
        }
    }
    out
}

/// Write depot versions into appmanifest (manifest ID in InstalledDepots), the build number
/// (buildid/TargetBuildID, if given) and StateFlags. Everything else - byte for byte.
/// Needed for version pinning: Steam compares these numbers with the latest ones and, if they
/// match, updates nothing. A backup - once, *.acf.vaporust-pin.bak.
pub fn write_depot_versions(
    path: &Path,
    manifests: &std::collections::BTreeMap<u32, u64>,
    buildid: Option<&str>,
    state_flags: u32,
) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let t = line.trim();
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        if t == "{" {
            stack.push(std::mem::take(&mut pending));
        } else if t == "}" {
            stack.pop();
        } else if let Some((k, _)) = kv(t) {
            let top = stack.len() == 1;
            if top && k.eq_ignore_ascii_case("StateFlags") {
                out.push(format!("{indent}\"{k}\"\t\t\"{state_flags}\""));
                continue;
            }
            if let Some(b) = buildid.filter(|_| top && k.eq_ignore_ascii_case("buildid")) {
                out.push(format!("{indent}\"{k}\"\t\t\"{b}\""));
                continue;
            }
            // For an up-to-date game Steam keeps TargetBuildID = 0 (nothing to download).
            if top && state_flags == 4 && k.eq_ignore_ascii_case("TargetBuildID") {
                out.push(format!("{indent}\"{k}\"\t\t\"0\""));
                continue;
            }
            if stack.len() == 3 && stack[1].eq_ignore_ascii_case("InstalledDepots") && k.eq_ignore_ascii_case("manifest") {
                if let Some(m) = stack[2].parse::<u32>().ok().and_then(|d| manifests.get(&d)) {
                    out.push(format!("{indent}\"{k}\"\t\t\"{m}\""));
                    continue;
                }
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            pending = k.to_string();
        }
        out.push(line.to_string());
    }
    let backup = path.with_extension("acf.vaporust-pin.bak");
    if !backup.exists() {
        let _ = fs::copy(path, &backup);
    }
    let mut body = out.join("\n");
    body.push('\n');
    let tmp = path.with_extension("acf.tmp");
    fs::write(&tmp, body).map_err(|e| trf!("Не удалось записать файл игры: {e}", "Could not write the game file: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| trf!("Не удалось записать файл игры: {e}", "Could not write the game file: {e}"))
}

/// Open a folder in Explorer.
pub fn open_folder(dir: &Path) {
    // Explorer does not understand paths with "/" (that is how Steam writes its folder to the registry)
    // and opens "Documents" instead - convert to backslashes.
    let p = PathBuf::from(dir.to_string_lossy().replace('/', "\\"));
    let _ = Command::new(crate::util::system_exe("explorer.exe")).arg(p).spawn();
}

// ---------- Processes ----------

/// Path in a uniform form for comparison: lowercase, backslashes.
fn norm(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('/', "\\")
}

/// A normalized folder path ending with a backslash, for "is inside" checks.
fn with_slash(p: &Path) -> String {
    let mut s = norm(p);
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// Memory of the Steam processes right now (Task Manager numbers), bytes: steam.exe,
/// steamwebhelper (process count and total), the overlay. Only processes from the Steam folder.
#[derive(Clone, Copy, Default, Debug)]
pub struct SteamMem {
    pub steam: u64,
    pub web: (usize, u64),
    pub overlay: u64,
}

/// What freezing gave during the last game (shown in "Settings -> Launching games").
#[derive(Clone, Copy, Debug)]
pub struct FreezeResult {
    pub before: u64,
    pub during: u64,
    pub processes: usize,
    pub soft: bool,
}

pub static LAST_FREEZE: std::sync::Mutex<Option<FreezeResult>> = std::sync::Mutex::new(None);

pub fn steam_memory(steam_dir: &Path) -> SteamMem {
    let mut procs = Procs::new();
    let sum = |pids: &[u32]| pids.iter().filter_map(|p| crate::stats::memory_of(*p)).sum::<u64>();
    let web = procs.pids_in("steamwebhelper.exe", steam_dir);
    SteamMem {
        steam: sum(&procs.pids_in("steam.exe", steam_dir)),
        web: (web.len(), sum(&web)),
        overlay: sum(&procs.pids_in("gameoverlayui.exe", steam_dir)),
    }
}

struct Procs {
    sys: System,
}

impl Procs {
    fn new() -> Self {
        Self { sys: System::new() }
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
    }

    fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a sysinfo::Process> + 'a {
        self.sys
            .processes()
            .values()
            .filter(move |p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
    }

    fn steam_running(&mut self) -> bool {
        self.refresh();
        self.named("steam.exe").next().is_some()
    }

    fn named_exists(&mut self, name: &str) -> bool {
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        self.named(name).next().is_some()
    }

    /// The game is running = there is a process whose exe lies in the game folder.
    fn game_running(&mut self, game_dir: &str) -> bool {
        self.refresh();
        self.sys
            .processes()
            .values()
            .any(|p| p.exe().is_some_and(|e| norm(e).starts_with(game_dir)))
    }

    /// Processes with this name whose exe is known to lie in `root` (the Steam folder). A process
    /// of another program with the same name, or one whose path cannot be read, is never chosen.
    fn pids_in(&mut self, name: &str, root: &Path) -> Vec<u32> {
        self.refresh();
        let root = with_slash(root);
        self.named(name).filter(|p| p.exe().is_some_and(|e| norm(e).starts_with(&root))).map(|p| p.pid().as_u32()).collect()
    }

    /// Kills all processes with these names whose exe lies in `root` (the Steam folder: another
    /// program's "steam.exe" elsewhere is not touched). Returns how many were killed.
    fn kill_all(&mut self, names: &[&str], root: &Path) -> usize {
        self.refresh();
        let root = with_slash(root);
        let mut n = 0;
        for name in names {
            for p in self.named(name) {
                // Only processes whose exe is known to be in the Steam folder: an unknown path (no access,
                // another user's or an elevated process) is left alone.
                if !p.exe().is_some_and(|e| norm(e).starts_with(&root)) {
                    continue;
                }
                if p.kill() {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------- Freezing processes ----------

/// How steamwebhelper is held down while a game runs.
///  - full: the whole process is suspended (NtSuspendProcess) and its memory goes to the page file.
///    The process stays alive, so Steam does not restart it. The most effect, but the Steam
///    overlay and other browser windows cannot open.
///  - soft (the way NoSteamWebHelper-Reloaded does it): low priority, Windows efficiency mode,
///    very low memory priority and the memory moved to the page file again every 10 s.
///    Nothing is stopped, so Steam never waits for a frozen process.
#[cfg(windows)]
mod freeze {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, ProcessMemoryPriority, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
        BELOW_NORMAL_PRIORITY_CLASS, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_NORMAL, MEMORY_PRIORITY_VERY_LOW,
        NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
        PROCESS_SUSPEND_RESUME,
    };

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSuspendProcess(h: HANDLE) -> i32;
        fn NtResumeProcess(h: HANDLE) -> i32;
    }

    fn with_process<T: Default>(pid: u32, f: impl FnOnce(HANDLE) -> T) -> T {
        // SAFETY: the handle is checked and closed; the functions get valid pointers to local data.
        unsafe {
            let access = PROCESS_SUSPEND_RESUME | PROCESS_SET_QUOTA | PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION;
            let h = OpenProcess(access, 0, pid);
            if h.is_null() {
                return T::default();
            }
            let r = f(h);
            CloseHandle(h);
            r
        }
    }

    /// Process start time (100 ns units): together with the number it tells one process from
    /// another that later got the same number.
    pub fn started(pid: u32) -> u64 {
        with_process(pid, |h| unsafe {
            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut c, mut e, mut k, mut u) = (z, z, z, z);
            if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 {
                return 0;
            }
            ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64
        })
    }

    pub fn suspend(pid: u32) -> bool {
        with_process(pid, |h| unsafe {
            let ok = NtSuspendProcess(h) >= 0;
            if ok {
                K32EmptyWorkingSet(h);
            }
            ok
        })
    }

    pub fn resume(pid: u32) -> bool {
        with_process(pid, |h| unsafe { NtResumeProcess(h) >= 0 })
    }

    fn set_soft(h: HANDLE, on: bool) -> bool {
        unsafe {
            let ok = SetPriorityClass(h, if on { BELOW_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS }) != 0;
            // Efficiency mode and memory priority: Windows 10 and newer; on older systems simply not applied.
            let thr = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
            };
            SetProcessInformation(h, ProcessPowerThrottling, &thr as *const _ as _, std::mem::size_of_val(&thr) as u32);
            let mem = MEMORY_PRIORITY_INFORMATION { MemoryPriority: if on { MEMORY_PRIORITY_VERY_LOW } else { MEMORY_PRIORITY_NORMAL } };
            SetProcessInformation(h, ProcessMemoryPriority, &mem as *const _ as _, std::mem::size_of_val(&mem) as u32);
            if on {
                K32EmptyWorkingSet(h);
            }
            ok
        }
    }

    pub fn soften(pid: u32) -> bool {
        with_process(pid, |h| set_soft(h, true))
    }

    pub fn unsoften(pid: u32) -> bool {
        with_process(pid, |h| set_soft(h, false))
    }

    /// Move the memory of a softened process to the page file again (it grows back while it works).
    pub fn trim(pid: u32) {
        with_process(pid, |h| unsafe { K32EmptyWorkingSet(h) });
    }
}

#[cfg(not(windows))]
mod freeze {
    pub fn started(_pid: u32) -> u64 {
        0
    }
    pub fn suspend(_pid: u32) -> bool {
        false
    }
    pub fn resume(_pid: u32) -> bool {
        false
    }
    pub fn soften(_pid: u32) -> bool {
        false
    }
    pub fn unsoften(_pid: u32) -> bool {
        false
    }
    pub fn trim(_pid: u32) {}
}

/// File with the processes Vaporust froze: if Vaporust is closed or crashes during a game, the next
/// start (or the exit itself) unfreezes them, otherwise Steam would stay hidden and "hung".
fn frozen_file() -> Option<PathBuf> {
    crate::errlog::path().map(|p| p.with_file_name("frozen_webhelper.txt"))
}

fn write_frozen(list: &[(u32, u64, bool)]) {
    let Some(p) = frozen_file() else { return };
    if list.is_empty() {
        let _ = fs::remove_file(&p);
        return;
    }
    let text: String = list.iter().map(|(pid, t, soft)| format!("{pid} {t} {}\n", if *soft { "soft" } else { "full" })).collect();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = crate::util::write_atomic(&p, text.as_bytes());
}

/// Unfreeze what a previous run (or this one, before exiting) left frozen.
/// Only the same processes: the number and the start time must both match.
pub fn thaw_leftovers() {
    let Some(p) = frozen_file() else { return };
    let Ok(text) = fs::read_to_string(&p) else { return };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let pid = it.next().and_then(|s| s.parse::<u32>().ok());
        let t = it.next().and_then(|s| s.parse::<u64>().ok());
        let (Some(pid), Some(t)) = (pid, t) else { continue };
        if t == 0 || freeze::started(pid) != t {
            continue; // the process is gone, or the number now belongs to another one
        }
        if it.next() == Some("soft") {
            freeze::unsoften(pid);
        } else {
            freeze::resume(pid);
        }
    }
    let _ = fs::remove_file(&p);
}

/// List of frozen processes. Unfreezes all of them when run_chain exits by any path
/// (game over, error, "Stop waiting"); the list is also kept in a file (see thaw_leftovers).
#[derive(Default)]
struct Frozen {
    list: Vec<(u32, u64, bool)>,
    soft: bool,
}

impl Frozen {
    fn new(soft: bool) -> Self {
        Self { list: Vec::new(), soft }
    }

    fn len(&self) -> usize {
        self.list.len()
    }

    fn freeze_new(&mut self, pids: Vec<u32>) {
        for pid in pids {
            let t = freeze::started(pid);
            // The same process = the same number AND start time; a new process with a reused number
            // is a different one (the old record is dropped: that process is gone).
            if let Some(i) = self.list.iter().position(|(p, _, _)| *p == pid) {
                if self.list[i].1 == t && t != 0 {
                    if self.soft {
                        freeze::trim(pid);
                    }
                    continue;
                }
                self.list.remove(i);
            }
            // The file first: even if Vaporust dies right after freezing, the next start knows about it.
            self.list.push((pid, t, self.soft));
            write_frozen(&self.list);
            let ok = if self.soft { freeze::soften(pid) } else { freeze::suspend(pid) };
            if !ok {
                self.list.pop();
                write_frozen(&self.list);
            }
        }
    }

    fn resume_all(&mut self) {
        for (pid, t, soft) in self.list.drain(..) {
            // Only the process that was frozen: its number may belong to another one by now.
            if t == 0 || freeze::started(pid) != t {
                continue;
            }
            if soft {
                freeze::unsoften(pid);
            } else {
                freeze::resume(pid);
            }
        }
        write_frozen(&[]);
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        if !self.list.is_empty() {
            self.resume_all();
        }
    }
}

// ---------- Launching a game through the stripped-down Steam ----------

/// Waits for a condition; false on timeout or cancel.
fn wait_until(timeout: Duration, cancel: &AtomicBool, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    false
}

/// Steam wrote something in its folder within the last `secs` seconds: updates itself (package),
/// syncs the cloud (logs/cloud_log.txt) and the like. Then it is busy and must not be killed.
fn steam_busy(steam: &SteamInfo, secs: u64) -> bool {
    let fresh = |p: PathBuf| {
        fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|e| e < Duration::from_secs(secs))
    };
    let package = steam.dir.join("package");
    fresh(package.clone())
        || fs::read_dir(&package).into_iter().flatten().flatten().any(|e| fresh(e.path()))
        || fresh(steam.dir.join("logs").join("cloud_log.txt"))
        || fresh(steam.dir.join("logs").join("bootstrap_log.txt"))
}

fn shutdown_steam(steam: &SteamInfo, procs: &mut Procs, set: &dyn Fn(&str)) {
    set(tr!("Закрываю Steam...", "Closing Steam..."));
    let _ = Command::new(&steam.exe).arg("-shutdown").spawn();
    let never = AtomicBool::new(false);
    let mut closed = wait_until(Duration::from_secs(30), &never, || !procs.steam_running());
    // Still sending saves to the cloud or updating itself: wait up to 3 more minutes while it is busy.
    let extra = Instant::now();
    while !closed && extra.elapsed() < Duration::from_secs(180) && steam_busy(steam, 15) {
        set(tr!("Steam еще занят (облако или обновление), жду...", "Steam is still busy (cloud or update), waiting..."));
        closed = wait_until(Duration::from_secs(5), &never, || !procs.steam_running());
    }
    if !closed {
        set(tr!("Steam не закрылся за 30 с, завершаю принудительно...", "Steam did not close within 30 s, forcing it to quit..."));
    }
    // Finish off leftovers in any case (webhelper and the overlay sometimes stay hanging).
    procs.kill_all(&["steam.exe", "steamwebhelper.exe", "gameoverlayui.exe"], &steam.dir);
    reset_active_user();
}

/// Close Steam by button (for example after "Stop tracking the game").
pub fn close_steam(steam: &SteamInfo, set: &dyn Fn(&str)) {
    let mut procs = Procs::new();
    shutdown_steam(steam, &mut procs, set);
}

/// Start Steam in stripped-down mode and wait for login. Ok(Some(true)) - we started Steam
/// (so we close it too), Ok(Some(false)) - the user had it open already, Ok(None) - cancelled.
fn start_hidden_steam(
    steam: &SteamInfo,
    args: &str,
    procs: &mut Procs,
    cancel: &AtomicBool,
    set: &dyn Fn(&str),
) -> Result<Option<bool>, String> {
    // If the user already runs Steam, we do not touch it and do not close it.
    if procs.steam_running() {
        return Ok(Some(false));
    }
    reset_active_user();
    set(tr!("Запускаю Steam в урезанном режиме...", "Starting Steam in reduced mode..."));
    Command::new(&steam.exe)
        .args(if args.trim().is_empty() { DEFAULT_STEAM_ARGS } else { args }.split_whitespace())
        .spawn()
        .map_err(|e| trf!("Не удалось запустить Steam: {e}", "Could not start Steam: {e}"))?;
    if !wait_until(Duration::from_secs(90), cancel, || active_user() != 0) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        // Steam may be updating itself first (often after a long break, and slow on an old PC):
        // killing it in the middle can break the Steam installation. Wait while it is busy.
        let extra = Instant::now();
        while extra.elapsed() < Duration::from_secs(900) && procs.steam_running() && steam_busy(steam, 60) {
            set(tr!("Steam обновляет сам себя, жду...", "Steam is updating itself, waiting..."));
            if wait_until(Duration::from_secs(5), cancel, || active_user() != 0) {
                return Ok(Some(true));
            }
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
        }
        shutdown_steam(steam, procs, set);
        return Err(tr!("Steam не залогинился за 90 с. Войдите один раз через обычный интерфейс \
                    с галочкой «Запомнить пароль».", "Steam did not log in within 90 s. Log in once through the normal interface with \"Remember password\" checked.")
            .into());
    }
    Ok(Some(true))
}

/// Update a game with the real Steam without launching it (new version, language or beta change).
/// Steam starts invisibly, the launcher shows progress from appmanifest and closes Steam.
pub fn update_via_steam(
    steam: &SteamInfo,
    game: &Game,
    opts: LaunchOptions,
    cancel: &AtomicBool,
    set: &dyn Fn(&str),
) -> Result<String, String> {
    #[allow(non_snake_case)]
    let CANCEL_MSG: &str =
        tr!("Лаунчер больше не следит за обновлением. Steam останется открытым и докачает игру сам.", "The launcher no longer follows the update. Steam stays open and finishes the game download itself.");
    let mut procs = Procs::new();
    let acf = appmanifest_path(game);
    let we_started = match start_hidden_steam(steam, &opts.steam_args, &mut procs, cancel, set)? {
        Some(w) => w,
        None => return Ok(CANCEL_MSG.into()),
    };

    set(tr!("Жду, пока Steam начнет обновление...", "Waiting for Steam to start the update..."));
    let started = Instant::now();
    let mut nudged = false;
    let mut done_since: Option<Instant> = None;
    let mut last_progress = (0u64, Instant::now());
    let mut frozen = Frozen::new(opts.freeze_soft);
    let mut last_freeze: Option<Instant> = None;
    let mut steam_gone: Option<Instant> = None;
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(CANCEL_MSG.into());
        }
        thread::sleep(Duration::from_secs(1));
        if steam_closed(&mut steam_gone) {
            break Err(tr!("Steam закрыт, обновление прервано.", "Steam closed, the update was interrupted.").to_string());
        }
        let info = read_acf(&acf);
        if info.updating() {
            done_since = None;
            if info.bytes_downloaded != last_progress.0 {
                last_progress = (info.bytes_downloaded, Instant::now());
            }
            set(&if info.bytes_to_download > 0 {
                trf!(
                    "Steam обновляет игру: {} из {}", "Steam is updating the game: {} of {}",
                    crate::net::fmt_bytes(info.bytes_downloaded),
                    crate::net::fmt_bytes(info.bytes_to_download)
                )
            } else {
                tr!("Steam обновляет игру...", "Steam is updating the game...").into()
            });
            // Steam itself may not start downloading (for example if its settings say "update
            // only on launch") - after 45 s we ask it to verify the game files, which downloads what is missing.
            if !nudged && started.elapsed() > Duration::from_secs(45) && info.bytes_downloaded == 0 {
                nudged = true;
                set(tr!("Steam не начал обновление сам - запускаю проверку файлов игры...", "Steam did not start the update itself - starting a file verification..."));
                let _ = Command::new(&steam.exe)
                    .arg(format!("steam://validate/{}", game.appid))
                    .spawn();
            }
            if last_progress.1.elapsed() > Duration::from_secs(600) {
                break Err(tr!("Обновление не двигается 10 минут. Steam оставлен открытым.", "The update has not moved for 10 minutes. Steam is left open.").to_string());
            }
        } else if done_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(5) {
            break Ok(if started.elapsed() < Duration::from_secs(10) {
                tr!("Игра уже обновлена.", "The game is already up to date.").to_string()
            } else {
                tr!("Игра обновлена.", "The game is updated.").to_string()
            });
        }
        // While downloading, steamwebhelper is not needed - freeze it, as during play.
        if opts.freeze_webhelper
            && started.elapsed() >= Duration::from_secs(20)
            && last_freeze.map_or(true, |t| t.elapsed() >= Duration::from_secs(10))
        {
            last_freeze = Some(Instant::now());
            frozen.freeze_new(procs.pids_in("steamwebhelper.exe", &steam.dir));
        }
    };
    frozen.resume_all();
    if we_started && result.is_ok() {
        set(tr!("Обновление готово, закрываю Steam...", "Update finished, closing Steam..."));
        thread::sleep(Duration::from_secs(3));
        shutdown_steam(steam, &mut procs, set);
    }
    result
}

/// Steam has not been running for 10 s in a row (short gaps, e.g. Steam restarting after
/// updating itself, do not count).
fn steam_closed(gone_since: &mut Option<Instant>) -> bool {
    if steam_running() {
        *gone_since = None;
        return false;
    }
    gone_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(10)
}

pub fn run_chain(
    steam: &SteamInfo,
    game: &Game,
    opts: LaunchOptions,
    cancel: &AtomicBool,
    set: &dyn Fn(&str),
) -> Result<String, String> {
    let mut procs = Procs::new();
    let game_dir = {
        let mut d = norm(&game.dir);
        if !d.ends_with('\\') {
            d.push('\\');
        }
        d
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    #[allow(non_snake_case)]
    let CANCEL_MSG: &str =
        tr!("Лаунчер больше не следит за игрой. Steam останется открытым - закройте его кнопкой «Закрыть Steam».", "The launcher no longer follows the game. Steam stays open - close it with the \"Close Steam\" button.");

    let we_started = match start_hidden_steam(steam, &opts.steam_args, &mut procs, cancel, set)? {
        Some(w) => w,
        None => return Ok(CANCEL_MSG.into()),
    };

    set(tr!("Запускаю игру...", "Starting the game..."));
    Command::new(&steam.exe)
        .arg(format!("steam://rungameid/{}", game.appid))
        .spawn()
        .map_err(|e| trf!("Не удалось отправить команду запуска: {e}", "Could not send the launch command: {e}"))?;

    // Waiting for the game process. If Steam first updates the game (new version, changed language or
    // beta), show progress from appmanifest and do not treat it as a hang.
    let acf = appmanifest_path(game);
    let mut deadline = Instant::now() + Duration::from_secs(120);
    let mut steam_gone: Option<Instant> = None;
    loop {
        if cancelled() {
            return Ok(CANCEL_MSG.into());
        }
        if procs.game_running(&game_dir) {
            break;
        }
        // Steam was closed (for example with the "Close Steam" button) while it was updating the game or
        // had not launched it yet: nothing more to wait for.
        if steam_closed(&mut steam_gone) {
            return Err(tr!("Steam закрыт, запуск отменен.", "Steam closed, launch cancelled.").into());
        }
        let info = read_acf(&acf);
        if info.updating() {
            deadline = Instant::now() + Duration::from_secs(120);
            let progress = if info.bytes_to_download > 0 {
                trf!(
                    ": {} из {}", ": {} of {}",
                    crate::net::fmt_bytes(info.bytes_downloaded),
                    crate::net::fmt_bytes(info.bytes_to_download)
                )
            } else {
                "...".into()
            };
            set(&trf!("Steam обновляет игру перед запуском{progress}", "Steam is updating the game before launch{progress}"));
        }
        if Instant::now() > deadline {
            // The game may wait for a Steam window that is hidden (an agreement, a launch option,
            // a cloud conflict) or start through its own launcher from another folder. Closing Steam
            // would break both, so the Steam window is shown instead.
            let _ = Command::new(&steam.exe).arg("steam://open/main").spawn();
            return Err(tr!(
                "Игра не появилась за 120 с. Открыл окно Steam: возможно, он что-то спрашивает. Steam оставлен открытым - закройте его кнопкой «Закрыть Steam», когда закончите.",
                "The game did not appear within 120 s. The Steam window is opened: it may be asking something. Steam is left open - close it with the \"Close Steam\" button when you are done."
            )
            .into());
        }
        thread::sleep(Duration::from_secs(1));
    }

    let _running = RunningGuard::set(game.appid, &game.name, &game.dir);
    set(tr!("Игра запущена, жду выхода...", "The game is running, waiting for it to exit..."));
    let started = Instant::now();
    let mut gone_since: Option<Instant> = None;
    let mut frozen = Frozen::new(opts.freeze_soft);
    let mut last_freeze: Option<Instant> = None;
    let mut web_before: u64 = 0;
    loop {
        thread::sleep(Duration::from_secs(1));
        if cancelled() {
            return Ok(CANCEL_MSG.into()); // Frozen unfreezes on drop
        }
        if procs.game_running(&game_dir) {
            gone_since = None;
        } else if gone_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(6) {
            break;
        }
        // 20 s after the game starts we freeze the webhelper; every 10 s
        // we pick up its new processes if Steam created them.
        if opts.freeze_webhelper
            && started.elapsed() >= Duration::from_secs(20)
            && last_freeze.map_or(true, |t| t.elapsed() >= Duration::from_secs(10))
        {
            last_freeze = Some(Instant::now());
            let before = frozen.len();
            let pids = procs.pids_in("steamwebhelper.exe", &steam.dir);
            // Memory of the processes that are about to be frozen for the first time: the "before" number.
            let fresh: Vec<u32> = pids.iter().copied().filter(|p| !frozen.list.iter().any(|(q, _, _)| q == p)).collect();
            web_before += fresh.iter().filter_map(|p| crate::stats::memory_of(*p)).sum::<u64>();
            frozen.freeze_new(pids);
            if frozen.len() != before {
                set(&trf!(
                    "Игра запущена, жду выхода... (steamwebhelper заморожен, процессов: {})", "The game is running, waiting for it to exit... (steamwebhelper frozen, processes: {})",
                    frozen.len()
                ));
            }
        }
    }
    // The result of freezing for "Settings": what the browser took before and while frozen.
    if web_before > 0 {
        let during = frozen.list.iter().filter_map(|(p, _, _)| crate::stats::memory_of(*p)).sum::<u64>();
        *LAST_FREEZE.lock().unwrap() = Some(FreezeResult { before: web_before, during, processes: frozen.len(), soft: opts.freeze_soft });
    }
    // Unfreeze before closing Steam, otherwise it cannot shut down properly.
    frozen.resume_all();

    if we_started {
        set(tr!("Игра закрыта, даю Steam синхронизировать сохранения...", "The game closed, letting Steam sync the saves..."));
        thread::sleep(Duration::from_secs(8));
        shutdown_steam(steam, &mut procs, set);
    }
    Ok(tr!("Готово.", "Done.").into())
}

#[cfg(test)]
mod acf_tests {
    use super::*;

    const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"SizeOnDisk\"\t\t\"1000\"\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n\t\"MountedConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t\t\"BetaKey\"\t\t\"beta\"\n\t}\n}\n";

    fn tmp(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vaporust_acf_{name}_{}.acf", std::process::id()));
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn depot_versions() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"StateFlags\"\t\t\"6\"\n\t\"buildid\"\t\t\"100\"\n\t\"TargetBuildID\"\t\t\"200\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"1\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"111\"\n\t\t\t\"size\"\t\t\"5\"\n\t\t}\n\t\t\"2\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"222\"\n\t\t\t\"size\"\t\t\"6\"\n\t\t}\n\t}\n\t\"SharedDepots\"\n\t{\n\t\t\"228988\"\t\t\"228980\"\n\t}\n}\n";
        let p = tmp("depots", acf);
        let d = installed_depots(&p);
        assert_eq!(d.into_iter().collect::<Vec<_>>(), vec![(1, 111), (2, 222)]);
        let new: std::collections::BTreeMap<u32, u64> = [(1, 999)].into_iter().collect();
        write_depot_versions(&p, &new, Some("300"), 4).unwrap();
        let d = installed_depots(&p);
        assert_eq!(d.into_iter().collect::<Vec<_>>(), vec![(1, 999), (2, 222)]);
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"buildid\"\t\t\"300\""));
        assert!(text.contains("\"TargetBuildID\"\t\t\"0\""));
        assert!(text.contains("\"StateFlags\"\t\t\"4\""));
        assert!(text.contains("\"228988\"\t\t\"228980\""));
        assert_eq!(read_acf(&p).state_flags, 4);
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(p.with_extension("acf.vaporust-pin.bak"));
    }

    #[test]
    fn mounted_config() {
        let p = tmp("mounted", ACF);
        set_mounted_config(&p, "BetaKey", "").unwrap();
        let i = read_acf(&p);
        assert_eq!((i.mounted_beta.as_str(), i.state_flags), ("", 4));
        set_mounted_config(&p, "BetaKey", "previous").unwrap();
        let i = read_acf(&p);
        assert_eq!((i.mounted_beta.as_str(), i.mounted_language.as_str(), i.state_flags), ("previous", "english", 4));
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(p.with_extension("acf.vaporust.bak"));
    }

    #[test]
    fn read_and_edit() {
        let p = tmp("edit", ACF);
        let i = read_acf(&p);
        assert_eq!((i.state_flags, i.size_on_disk), (4, 1000));
        assert_eq!((i.user_language.as_str(), i.mounted_beta.as_str()), ("english", "beta"));
        assert!(!i.updating());

        set_user_config(&p, "language", "russian").unwrap();
        set_user_config(&p, "BetaKey", "beta").unwrap();
        let i = read_acf(&p);
        assert_eq!((i.user_language.as_str(), i.user_beta.as_str()), ("russian", "beta"));
        assert_eq!(i.mounted_language, "english"); // MountedConfig is untouched
        assert_eq!(i.state_flags, 6);
        assert!(i.updating());

        set_user_config(&p, "BetaKey", "").unwrap(); // public = the key is removed
        assert_eq!(read_acf(&p).user_beta, "");
    }

    #[test]
    fn no_user_config_block() {
        let p = tmp("nouser", "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"StateFlags\"\t\t\"4\"\n}\n");
        set_user_config(&p, "language", "german").unwrap();
        assert_eq!(read_acf(&p).user_language, "german");
    }
}

/// Build number (buildid) from appmanifest.
pub fn acf_buildid(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut depth = 0;
    for line in text.lines() {
        let t = line.trim();
        match t {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ if depth == 1 => {
                if let Some((k, v)) = kv(t) {
                    if k.eq_ignore_ascii_case("buildid") {
                        return Some(v.to_string()).filter(|v| !v.is_empty() && v != "0");
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Shared depots from appmanifest (SharedDepots: depot -> the game it belongs to).
pub fn shared_depots(path: &Path) -> std::collections::BTreeMap<u32, u32> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else { return out };
    let mut stack: Vec<String> = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t == "{" {
            stack.push(std::mem::take(&mut pending));
            continue;
        }
        if t == "}" {
            stack.pop();
            continue;
        }
        if let Some((k, v)) = kv(t) {
            if stack.len() == 2 && stack[1].eq_ignore_ascii_case("SharedDepots") {
                if let (Ok(d), Ok(a)) = (k.parse(), v.parse()) {
                    out.insert(d, a);
                }
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            pending = k.to_string();
        }
    }
    out
}

/// Changes to the depot lists in appmanifest (for changing the language without Steam).
#[derive(Default, Debug)]
pub struct DepotEdit {
    /// Remove from InstalledDepots.
    pub remove_installed: Vec<u32>,
    /// Add to InstalledDepots: (depot, manifest, size).
    pub add_installed: Vec<(u32, u64, u64)>,
    /// Remove from SharedDepots.
    pub remove_shared: Vec<u32>,
    /// Add to SharedDepots: (depot, owner game).
    pub add_shared: Vec<(u32, u32)>,
}

fn installed_entry(d: u32, m: u64, s: u64) -> Vec<String> {
    vec![
        format!("\t\t\"{d}\""),
        "\t\t{".to_string(),
        format!("\t\t\t\"manifest\"\t\t\"{m}\""),
        format!("\t\t\t\"size\"\t\t\"{s}\""),
        "\t\t}".to_string(),
    ]
}

/// Fix InstalledDepots and SharedDepots, the rest - byte for byte. A missing block is created.
pub fn edit_depot_lists(path: &Path, e: &DepotEdit) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| trf!("Не удалось прочитать файл игры: {err}", "Could not read the game file: {err}"))?;
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending = String::new();
    // Skipping a removed depot inside InstalledDepots: the depth up to which we skip.
    let mut skip_to: Option<usize> = None;
    let (mut seen_inst, mut seen_shared) = (false, false);
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        i += 1;
        if let Some(depth) = skip_to {
            if t == "{" {
                stack.push(String::new());
            } else if t == "}" {
                stack.pop();
                if stack.len() == depth {
                    skip_to = None;
                }
            }
            continue;
        }
        if t == "{" {
            let name = std::mem::take(&mut pending);
            seen_inst |= stack.len() == 1 && name.eq_ignore_ascii_case("InstalledDepots");
            seen_shared |= stack.len() == 1 && name.eq_ignore_ascii_case("SharedDepots");
            stack.push(name);
            out.push(line.to_string());
            continue;
        }
        if t == "}" {
            if stack.len() == 2 && stack[1].eq_ignore_ascii_case("InstalledDepots") {
                for &(d, m, s) in &e.add_installed {
                    out.extend(installed_entry(d, m, s));
                }
            }
            if stack.len() == 2 && stack[1].eq_ignore_ascii_case("SharedDepots") {
                for &(d, a) in &e.add_shared {
                    out.push(format!("\t\t\"{d}\"\t\t\"{a}\""));
                }
            }
            if stack.len() == 1 {
                if !seen_inst && !e.add_installed.is_empty() {
                    out.push("\t\"InstalledDepots\"".into());
                    out.push("\t{".into());
                    for &(d, m, s) in &e.add_installed {
                        out.extend(installed_entry(d, m, s));
                    }
                    out.push("\t}".into());
                }
                if !seen_shared && !e.add_shared.is_empty() {
                    out.push("\t\"SharedDepots\"".into());
                    out.push("\t{".into());
                    for &(d, a) in &e.add_shared {
                        out.push(format!("\t\t\"{d}\"\t\t\"{a}\""));
                    }
                    out.push("\t}".into());
                }
            }
            stack.pop();
            out.push(line.to_string());
            continue;
        }
        if let Some((k, _)) = kv(t) {
            if stack.len() == 2 && stack[1].eq_ignore_ascii_case("SharedDepots") {
                let d: Option<u32> = k.parse().ok();
                // Removed ones and those that will be added again (without duplicates).
                if d.is_some_and(|d| e.remove_shared.contains(&d) || e.add_shared.iter().any(|x| x.0 == d)) {
                    continue;
                }
            }
        } else if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            if stack.len() == 2 && stack[1].eq_ignore_ascii_case("InstalledDepots") {
                let d: Option<u32> = k.parse().ok();
                if d.is_some_and(|d| e.remove_installed.contains(&d) || e.add_installed.iter().any(|x| x.0 == d)) {
                    // Skip the whole depot block.
                    if lines.get(i).is_some_and(|l| l.trim() == "{") {
                        skip_to = Some(stack.len());
                        stack.push(String::new());
                        i += 1;
                    }
                    continue;
                }
            }
            pending = k.to_string();
        }
        out.push(line.to_string());
    }
    let mut body = out.join("\n");
    body.push('\n');
    let tmp = path.with_extension("acf.tmp");
    fs::write(&tmp, body).map_err(|err| trf!("Не удалось записать файл игры: {err}", "Could not write the game file: {err}"))?;
    fs::rename(&tmp, path).map_err(|err| trf!("Не удалось записать файл игры: {err}", "Could not write the game file: {err}"))
}

#[cfg(test)]
mod depot_edit_tests {
    use super::*;

    #[test]
    fn edit_lists() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"60\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"61\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"1\"\n\t\t\t\"size\"\t\t\"10\"\n\t\t}\n\t\t\"62\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"2\"\n\t\t\t\"size\"\t\t\"20\"\n\t\t}\n\t}\n\t\"SharedDepots\"\n\t{\n\t\t\"228988\"\t\t\"228980\"\n\t\t\"72\"\t\t\"70\"\n\t}\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"french\"\n\t}\n}\n";
        let p = std::env::temp_dir().join(format!("vaporust_edit_{}.acf", std::process::id()));
        fs::write(&p, acf).unwrap();
        let e = DepotEdit {
            remove_installed: vec![62],
            add_installed: vec![(63, 3, 30)],
            remove_shared: vec![72],
            add_shared: vec![(73, 70)],
        };
        edit_depot_lists(&p, &e).unwrap();
        assert_eq!(installed_depots(&p).into_iter().collect::<Vec<_>>(), vec![(61, 1), (63, 3)]);
        assert_eq!(shared_depots(&p).into_iter().collect::<Vec<_>>(), vec![(73, 70), (228988, 228980)]);
        assert_eq!(read_acf(&p).user_language, "french");
        // No blocks - they are created.
        fs::write(&p, "\"AppState\"\n{\n\t\"appid\"\t\t\"60\"\n}\n").unwrap();
        edit_depot_lists(&p, &DepotEdit { add_installed: vec![(62, 2, 20)], add_shared: vec![(141, 70)], ..Default::default() })
            .unwrap();
        assert_eq!(installed_depots(&p).into_iter().collect::<Vec<_>>(), vec![(62, 2)]);
        assert_eq!(shared_depots(&p).into_iter().collect::<Vec<_>>(), vec![(141, 70)]);
        let _ = fs::remove_file(&p);
    }
}

/// Steam offline mode: in config\loginusers.vdf, for the account Steam logs into by itself
/// (AutoLogin "1", otherwise MostRecent "1"), set WantsOfflineMode and SkipOfflineModeWarning.
/// Then Steam without network starts offline right away, without the "Could not connect" window.
/// This is the same setting Steam itself sets with the "Go Offline" button.
/// Only while Steam is closed (it rewrites the file on exit). Ok(true) - the file was changed.
pub fn set_steam_offline(steam_dir: &Path, on: bool) -> Result<bool, String> {
    let path = steam_dir.join("config").join("loginusers.vdf");
    let text = fs::read_to_string(&path).map_err(|e| trf!("Не удалось прочитать loginusers.vdf: {e}", "Could not read loginusers.vdf: {e}"))?;
    let lines: Vec<&str> = text.lines().collect();
    // Account blocks: (start line "{", end line "}", AutoLogin, MostRecent).
    let mut blocks: Vec<(usize, usize, bool, bool)> = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let (mut auto, mut recent) = (false, false);
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t == "{" {
            depth += 1;
            if depth == 2 {
                start = i;
                auto = false;
                recent = false;
            }
        } else if t == "}" {
            if depth == 2 {
                blocks.push((start, i, auto, recent));
            }
            depth -= 1;
        } else if depth == 2 {
            if let Some((k, v)) = kv(t) {
                auto |= k.eq_ignore_ascii_case("AutoLogin") && v == "1";
                recent |= k.eq_ignore_ascii_case("MostRecent") && v == "1";
            }
        }
    }
    let Some(&(s, e, _, _)) = blocks.iter().find(|b| b.2).or_else(|| blocks.iter().find(|b| b.3)).or(blocks.first())
    else {
        return Err(tr!("В loginusers.vdf нет аккаунтов", "loginusers.vdf has no accounts").into());
    };
    let want = if on { "1" } else { "0" };
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let (mut seen_want, mut seen_skip, mut changed) = (false, false, false);
    for (i, l) in lines.iter().enumerate() {
        if i > s && i < e {
            if let Some((k, v)) = kv(l.trim()) {
                let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
                if k.eq_ignore_ascii_case("WantsOfflineMode") || k.eq_ignore_ascii_case("SkipOfflineModeWarning") {
                    seen_want |= k.eq_ignore_ascii_case("WantsOfflineMode");
                    seen_skip |= k.eq_ignore_ascii_case("SkipOfflineModeWarning");
                    changed |= v != want;
                    out.push(format!("{indent}\"{k}\"\t\t\"{want}\""));
                    continue;
                }
            }
        }
        if i == e && on {
            // The keys were missing - add them before the end of the account block.
            if !seen_want {
                out.push(format!("\t\t\"WantsOfflineMode\"\t\t\"{want}\""));
                changed = true;
            }
            if !seen_skip {
                out.push(format!("\t\t\"SkipOfflineModeWarning\"\t\t\"{want}\""));
                changed = true;
            }
        }
        out.push(l.to_string());
    }
    if !changed {
        return Ok(false);
    }
    let backup = path.with_extension("vdf.vaporust.bak");
    if !backup.exists() {
        let _ = fs::copy(&path, &backup);
    }
    let mut body = out.join("\n");
    body.push('\n');
    let tmp = path.with_extension("vdf.tmp");
    fs::write(&tmp, body).map_err(|e| trf!("Не удалось записать loginusers.vdf: {e}", "Could not write loginusers.vdf: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| trf!("Не удалось записать loginusers.vdf: {e}", "Could not write loginusers.vdf: {e}"))?;
    Ok(true)
}

#[cfg(test)]
mod offline_tests {
    use super::*;

    #[test]
    fn steam_offline_flags() {
        let dir = std::env::temp_dir().join(format!("vaporust_offline_{}", std::process::id()));
        fs::create_dir_all(dir.join("config")).unwrap();
        let p = dir.join("config").join("loginusers.vdf");
        let body = "\"users\"\n{\n\t\"1\"\n\t{\n\t\t\"AccountName\"\t\t\"a\"\n\t\t\"WantsOfflineMode\"\t\t\"0\"\n\t\t\"SkipOfflineModeWarning\"\t\t\"0\"\n\t\t\"AutoLogin\"\t\t\"0\"\n\t}\n\t\"2\"\n\t{\n\t\t\"AccountName\"\t\t\"b\"\n\t\t\"AutoLogin\"\t\t\"1\"\n\t}\n}\n";
        fs::write(&p, body).unwrap();
        assert!(set_steam_offline(&dir, true).unwrap());
        let t = fs::read_to_string(&p).unwrap();
        // Only the auto-login account (b) was changed: the keys were added to its block.
        let b = t.split("\"2\"").nth(1).unwrap();
        assert!(b.contains("\"WantsOfflineMode\"\t\t\"1\"") && b.contains("\"SkipOfflineModeWarning\"\t\t\"1\""));
        assert!(t.split("\"2\"").next().unwrap().contains("\"WantsOfflineMode\"\t\t\"0\""));
        assert!(!set_steam_offline(&dir, true).unwrap(), "a second time - no change");
        assert!(set_steam_offline(&dir, false).unwrap());
        assert!(!fs::read_to_string(&p).unwrap().contains("\"1\"\n\t\t\"SkipOfflineModeWarning\"\t\t\"1\""));
        let _ = fs::remove_dir_all(&dir);
    }
}

// ---------- Stopping a game ----------

/// Running game (appid, name, folder): set when Vaporust sees the game process,
/// and cleared after it exits. The "Close game" buttons work from it.
pub static RUNNING: Mutex<Option<(u32, String, PathBuf)>> = Mutex::new(None);

/// Clears RUNNING when leaving run_chain or the launch-without-Steam thread.
pub struct RunningGuard;

impl RunningGuard {
    pub fn set(appid: u32, name: &str, dir: &Path) -> Self {
        *RUNNING.lock().unwrap() = Some((appid, name.to_string(), dir.to_path_buf()));
        RunningGuard
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        *RUNNING.lock().unwrap() = None;
    }
}

/// Processes whose exe lies in the game folder.
fn game_pids(dir: &Path) -> Vec<u32> {
    let mut d = norm(dir);
    if !d.ends_with('\\') {
        d.push('\\');
    }
    let mut procs = Procs::new();
    procs.refresh();
    let me = std::process::id();
    let mut pids: Vec<u32> = procs
        .sys
        .processes()
        .values()
        .filter(|p| p.pid().as_u32() != me && p.exe().is_some_and(|e| norm(e).starts_with(&d)))
        .map(|p| p.pid().as_u32())
        .collect();
    // And their child processes: many games start the actual game outside their folder (Plants vs. Zombies
    // unpacks and starts popcapgame1.exe in a temp folder, and PlantsVsZombies.exe only waits).
    loop {
        let more: Vec<u32> = procs
            .sys
            .processes()
            .values()
            .filter(|p| {
                let pid = p.pid().as_u32();
                // Windows reuses process numbers and keeps the parent number after the parent died:
                // a real child cannot have started before its parent.
                pid != me
                    && !pids.contains(&pid)
                    && p.parent().is_some_and(|pp| {
                        pids.contains(&pp.as_u32())
                            && procs.sys.process(pp).is_some_and(|parent| parent.start_time() <= p.start_time())
                    })
            })
            .map(|p| p.pid().as_u32())
            .collect();
        if more.is_empty() {
            break;
        }
        pids.extend(more);
    }
    pids
}

/// Stop a game. Gently: WM_CLOSE is sent to the game windows (like the window's close button or
/// Alt+F4), the game may save and exit by itself. Forcefully: processes are terminated
/// immediately (unsaved progress is lost). Returns the number of game processes.
pub fn stop_game(dir: &Path, force: bool) -> usize {
    let pids = game_pids(dir);
    if pids.is_empty() {
        return 0;
    }
    if force {
        let mut procs = Procs::new();
        procs.refresh();
        for pid in &pids {
            if let Some(p) = procs.sys.process(sysinfo::Pid::from_u32(*pid)) {
                p.kill();
            }
        }
    } else {
        close_windows_of(&pids);
    }
    pids.len()
}

/// The game is still running (there are processes in its folder).
pub fn game_alive(dir: &Path) -> bool {
    !game_pids(dir).is_empty()
}

#[cfg(windows)]
fn close_windows_of(pids: &[u32]) {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };
    unsafe extern "system" fn each(hwnd: HWND, l: LPARAM) -> BOOL {
        let pids = &*(l as *const Vec<u32>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        1
    }
    let v: Vec<u32> = pids.to_vec();
    unsafe {
        EnumWindows(Some(each), &v as *const Vec<u32> as LPARAM);
    }
}

#[cfg(not(windows))]
fn close_windows_of(_pids: &[u32]) {}

#[cfg(test)]
mod stop_tests {
    use super::*;

    /// Live test: a copy of Notepad closes gently, a copy of cmd (it has no window) - only
    /// forcefully.
    #[test]
    #[ignore]
    fn stop_game_live() {
        let dir = std::env::temp_dir().join(format!("vaporust_stop_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let sys = PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32");
        fs::copy(sys.join("notepad.exe"), dir.join("notepad.exe")).unwrap();
        fs::copy(sys.join("cmd.exe"), dir.join("cmd.exe")).unwrap();
        for lang in ["en-US", "ru-RU"] {
            if sys.join(lang).join("notepad.exe.mui").exists() {
                fs::create_dir_all(dir.join(lang)).unwrap();
                for f in ["notepad.exe.mui", "cmd.exe.mui"] {
                    let _ = fs::copy(sys.join(lang).join(f), dir.join(lang).join(f));
                }
            }
        }
        let mut a = Command::new(dir.join("notepad.exe")).spawn().unwrap();
        thread::sleep(Duration::from_secs(2));
        assert_eq!(stop_game(&dir, false), 1);
        thread::sleep(Duration::from_secs(2));
        assert!(a.try_wait().unwrap().is_some(), "Notepad closed gently");
        let mut b = {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the process has no windows at all.
            Command::new(dir.join("cmd.exe")).args(["/c", "ping -n 60 127.0.0.1 >nul"]).creation_flags(0x0800_0000).spawn().unwrap()
        };
        thread::sleep(Duration::from_secs(2));
        stop_game(&dir, false);
        thread::sleep(Duration::from_secs(2));
        assert!(b.try_wait().unwrap().is_none(), "cmd has no window, a gentle close does not work");
        assert_eq!(stop_game(&dir, true), 1);
        thread::sleep(Duration::from_secs(1));
        assert!(b.try_wait().unwrap().is_some(), "closed by force");
        assert!(game_pids(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}

// ---------- Relink ----------

/// Other libraries that already contain this game's folder (steamapps\common\<folder>): for example,
/// files were moved manually or Windows was reinstalled.
pub fn relink_candidates(steam_dir: &Path, game: &Game) -> Vec<PathBuf> {
    let Some(installdir) = game.dir.file_name() else { return Vec::new() };
    library_dirs(steam_dir)
        .into_iter()
        .filter(|l| !game.dir.starts_with(l))
        .filter(|l| l.join("steamapps").join("common").join(installdir).is_dir())
        .collect()
}

/// Attach the game to another library where its files already are: move the appmanifest and
/// the libraryfolders.vdf entry. Game files are not touched. Only while Steam is closed.
pub fn relink_game(steam_dir: &Path, game: &Game, dest_lib: &Path) -> Result<PathBuf, String> {
    let installdir = game.dir.file_name().ok_or(tr!("Не удалось определить папку игры.", "Could not determine the game folder."))?;
    let dest_common = dest_lib.join("steamapps").join("common").join(installdir);
    if !dest_common.is_dir() {
        return Err(trf!("В {} нет папки игры.", "{} has no game folder.", dest_lib.display()));
    }
    let src = appmanifest_path(game);
    if !src.exists() {
        return Err(tr!("Не найден appmanifest игры (загрузка на паузе? продолжите ее сначала).", "The game's appmanifest was not found (download paused? resume it first).").into());
    }
    let dest = dest_lib.join("steamapps").join(format!("appmanifest_{}.acf", game.appid));
    if dest.exists() {
        return Err(tr!("В той библиотеке у игры уже есть свой appmanifest.", "The game already has its own appmanifest in that library.").into());
    }
    let tmp = dest.with_extension("acf.tmp");
    fs::copy(&src, &tmp).map_err(|e| trf!("Не удалось записать appmanifest: {e}", "Could not write appmanifest: {e}"))?;
    fs::rename(&tmp, &dest).map_err(|e| trf!("Не удалось записать appmanifest: {e}", "Could not write appmanifest: {e}"))?;
    fs::remove_file(&src).map_err(|e| trf!("Не удалось убрать старый appmanifest: {e}", "Could not remove the old appmanifest: {e}"))?;
    // Library game lists: Steam will recount them anyway, but let them be right from the start.
    let _ = move_app_in_libraryfolders(steam_dir, game.appid, dest_lib);
    Ok(dest_common)
}

/// In libraryfolders.vdf move the game line into the "apps" block of the dest_lib library.
fn move_app_in_libraryfolders(steam_dir: &Path, appid: u32, dest_lib: &Path) -> Result<(), String> {
    let path = steam_dir.join("steamapps").join("libraryfolders.vdf");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    let want = norm(dest_lib);
    let want = want.trim_end_matches('\\');
    let key = appid.to_string();
    // Pass state: depth, path of the current library, whether we are inside the "apps" block.
    let (mut depth, mut lib_path, mut in_apps, mut pending_apps) = (0, String::new(), false, false);
    let mut size = "0".to_string();
    let mut dest_apps_open: Option<usize> = None;
    let mut remove: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t == "{" {
            depth += 1;
            in_apps = pending_apps && depth == 3;
            if in_apps && norm(Path::new(&lib_path)).trim_end_matches('\\') == want {
                dest_apps_open = Some(i);
            }
            pending_apps = false;
        } else if t == "}" {
            if depth == 3 {
                in_apps = false;
            }
            depth -= 1;
        } else if let Some((k, v)) = kv(t) {
            if depth == 2 && k == "path" {
                lib_path = v;
            } else if in_apps && k == key {
                size = v;
                remove = Some(i);
            }
        } else if depth == 2 && t.trim_matches('"').eq_ignore_ascii_case("apps") {
            pending_apps = true;
        }
    }
    let open = dest_apps_open.ok_or(tr!("библиотеки нет в libraryfolders.vdf", "the library is not in libraryfolders.vdf"))?;
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for (i, l) in lines.iter().enumerate() {
        if Some(i) != remove {
            out.push(l.to_string());
        }
        if i == open {
            out.push(format!("\t\t\t\"{appid}\"\t\t\"{size}\""));
        }
    }
    let backup = path.with_extension("vdf.vaporust.bak");
    if !backup.exists() {
        let _ = fs::copy(&path, &backup);
    }
    let tmp = path.with_extension("vdf.tmp");
    fs::write(&tmp, out.join("\n") + "\n").map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod relink_tests {
    use super::*;

    #[test]
    fn relink_moves_manifest_and_entry() {
        let root = std::env::temp_dir().join(format!("vaporust_relink_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (a, b) = (root.join("A"), root.join("B"));
        for l in [&a, &b] {
            fs::create_dir_all(l.join("steamapps").join("common").join("Game")).unwrap();
        }
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"70\"\t\t\"555\"\n\t\t\t\"10\"\t\t\"1\"\n\t\t}}\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n}}\n",
            a.display().to_string().replace('\\', "\\\\"),
            b.display().to_string().replace('\\', "\\\\")
        );
        fs::write(a.join("steamapps").join("libraryfolders.vdf"), vdf).unwrap();
        fs::write(a.join("steamapps").join("appmanifest_70.acf"), "\"AppState\"\n{\n}\n").unwrap();
        let game = Game {
            appid: 70,
            name: "HL".into(),
            dir: a.join("steamapps").join("common").join("Game"),
            ready: true,
            needs_update: false,
            size: 0,
            stashed: false,
        };
        assert_eq!(relink_candidates(&a, &game), vec![b.clone()]);
        relink_game(&a, &game, &b).unwrap();
        assert!(!a.join("steamapps").join("appmanifest_70.acf").exists());
        assert!(b.join("steamapps").join("appmanifest_70.acf").exists());
        let t = fs::read_to_string(a.join("steamapps").join("libraryfolders.vdf")).unwrap();
        let (first, second) = t.split_once("\"1\"").unwrap();
        assert!(!first.contains("\"70\"") && first.contains("\"10\""));
        assert!(second.contains("\"70\"\t\t\"555\""));
        let _ = fs::remove_dir_all(&root);
    }
}

/// Path for display: as it is written on disk (Steam stores its folder in lowercase
/// and with "/": "c:/program files (x86)/steam" -> "C:\Program Files (x86)\Steam").
pub fn pretty_path(p: &std::path::Path) -> String {
    match std::fs::canonicalize(p) {
        Ok(c) => {
            // canonicalize returns a "long" Windows path: \\?\C:\... or \\?\UNC\server\folder.
            let s = c.display().to_string();
            if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{unc}")
            } else {
                s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
            }
        }
        Err(_) => p.display().to_string().replace('/', "\\"),
    }
}

#[cfg(test)]
mod pretty_path_tests {
    #[cfg(windows)]
    #[test]
    fn no_long_path_prefix() {
        let dir = std::env::temp_dir();
        let s = super::pretty_path(&dir);
        assert!(!s.starts_with(r"\\?\"), "{s}");
        assert!(s.contains(':'), "{s}");
    }
}

/// How much space a folder takes (with all subfolders), bytes.
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Uninstall a game from disk by ourselves: the game folder, the unfinished download
/// (steamapps\downloading\<appid>) and the appmanifest - including one hidden from Steam
/// (unfinished Vaporust downloads that Aurelia does not find). Only while Steam is closed.
pub fn uninstall_files(game: &Game) -> Result<(), String> {
    // Protection against a path error: we delete only <library>\steamapps\common\<game folder>.
    let common = game.dir.parent().ok_or(tr!("неверная папка игры", "wrong game folder"))?;
    let steamapps = common.parent().ok_or(tr!("неверная папка игры", "wrong game folder"))?;
    let name_is = |p: &Path, n: &str| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(n));
    let leaf = game.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if !name_is(common, "common") || !name_is(steamapps, "steamapps") || leaf.trim().is_empty() || leaf == "." || leaf == ".." {
        return Err(trf!("папка игры выглядит неправильно: {}", "the game folder looks wrong: {}", game.dir.display()));
    }
    // Several games share the folder (common\Half-Life for Counter-Strike, Half-Life, Blue Shift...):
    // it must not be deleted as a whole.
    if let Some(other) = other_owner(steamapps, game.appid, &leaf) {
        return Err(trf!("эту папку использует и другая игра ({other}), целиком ее удалять нельзя", "this folder is also used by another game ({other}), it cannot be deleted entirely"));
    }
    if game.dir.exists() {
        fs::remove_dir_all(&game.dir).map_err(|e| trf!("не удалось удалить папку игры: {e}", "could not delete the game folder: {e}"))?;
    }
    let downloading = steamapps.join("downloading").join(game.appid.to_string());
    if downloading.exists() {
        let _ = fs::remove_dir_all(&downloading);
    }
    let (acf, hidden) = (
        steamapps.join(format!("appmanifest_{}.acf", game.appid)),
        steamapps.join(format!("appmanifest_{}{STASH_EXT}", game.appid)),
    );
    for f in [acf, hidden] {
        if f.exists() {
            fs::remove_file(&f).map_err(|e| trf!("не удалось удалить {}: {e}", "could not delete {}: {e}", f.display()))?;
        }
    }
    Ok(())
}

/// Another game (name) in this library with the same installdir folder.
fn other_owner(steamapps: &Path, appid: u32, installdir: &str) -> Option<String> {
    for e in fs::read_dir(steamapps).ok()?.flatten() {
        let f = e.file_name().to_string_lossy().to_string();
        if !f.starts_with("appmanifest_") || !(f.ends_with(".acf") || f.ends_with(STASH_EXT)) {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path()) else { continue };
        let (mut id, mut dir, mut name) = (None, None, None);
        for (k, v) in text.lines().filter_map(kv) {
            match k.as_str() {
                "appid" if id.is_none() => id = v.parse::<u32>().ok(),
                "installdir" if dir.is_none() => dir = Some(v),
                "name" if name.is_none() => name = Some(v),
                _ => {}
            }
        }
        if id.is_some_and(|i| i != appid) && dir.is_some_and(|d| d.eq_ignore_ascii_case(installdir)) {
            return Some(name.unwrap_or_default());
        }
    }
    None
}

#[cfg(test)]
mod uninstall_tests {
    use super::*;

    #[test]
    fn uninstall_partial_stashed() {
        let root = std::env::temp_dir().join(format!("vaporust_uninst_{}", std::process::id()));
        let steamapps = root.join("steamapps");
        let dir = steamapps.join("common").join("Игра тест");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("a.bin"), vec![0u8; 1000]).unwrap();
        fs::create_dir_all(steamapps.join("downloading").join("777")).unwrap();
        fs::write(steamapps.join(format!("appmanifest_777{STASH_EXT}")), "\"AppState\"\n{\n}\n").unwrap();
        fs::write(steamapps.join("appmanifest_778.acf"), "чужая игра").unwrap();
        let game = Game {
            appid: 777,
            name: "Игра тест".into(),
            dir: dir.clone(),
            ready: false,
            needs_update: false,
            size: 0,
            stashed: true,
        };
        assert_eq!(dir_size(&dir), 1000);
        uninstall_files(&game).unwrap();
        assert!(!dir.exists());
        assert!(!steamapps.join("downloading").join("777").exists());
        assert!(!steamapps.join(format!("appmanifest_777{STASH_EXT}")).exists());
        assert!(steamapps.join("appmanifest_778.acf").exists(), "other games are not touched");
        assert!(steamapps.join("common").exists());
        // Shared folder (like common\Half-Life of Counter-Strike and Half-Life) - refused, files intact.
        let shared = steamapps.join("common").join("Half-Life");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("hl.exe"), b"x").unwrap();
        fs::write(steamapps.join("appmanifest_10.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"10\"\n\t\"name\"\t\t\"Counter-Strike\"\n\t\"installdir\"\t\t\"Half-Life\"\n}\n").unwrap();
        fs::write(steamapps.join("appmanifest_70.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"name\"\t\t\"Half-Life\"\n\t\"installdir\"\t\t\"Half-Life\"\n}\n").unwrap();
        let cs = Game { appid: 10, name: "Counter-Strike".into(), dir: shared.clone(), ready: true, needs_update: false, size: 0, stashed: false };
        let err = uninstall_files(&cs).unwrap_err();
        assert!(err.contains("Half-Life"), "{err}");
        assert!(shared.join("hl.exe").exists());
        // Wrong path - refused.
        let bad = Game { dir: root.join("x"), ..game };
        assert!(uninstall_files(&bad).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}

/// Top-level appmanifest value (for example AutoUpdateBehavior).
pub fn acf_top(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut depth = 0;
    for line in text.lines() {
        let t = line.trim();
        match t {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ => {
                if depth == 1 {
                    if let Some((k, v)) = kv(t) {
                        if k.eq_ignore_ascii_case(key) {
                            return Some(v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Change (or add) a top-level appmanifest value. Only while Steam is closed.
/// The rest of the file - byte for byte; written via .tmp.
pub fn set_acf_top(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
    let mut out: Vec<String> = Vec::new();
    let mut depth = 0;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "{" {
            depth += 1;
        } else if t == "}" {
            // The key was missing - add it at the end of the top level.
            if depth == 1 && !done {
                out.push(format!("\t\"{key}\"\t\t\"{value}\""));
                done = true;
            }
            depth -= 1;
        } else if depth == 1 && !done {
            if let Some((k, _)) = kv(t) {
                if k.eq_ignore_ascii_case(key) {
                    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    out.push(format!("{indent}\"{k}\"\t\t\"{value}\""));
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    let mut body = out.join("\n");
    body.push('\n');
    let tmp = path.with_extension("acf.tmp");
    fs::write(&tmp, body).map_err(|e| trf!("Не удалось записать файл игры: {e}", "Could not write the game file: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| trf!("Не удалось записать файл игры: {e}", "Could not write the game file: {e}"))
}

#[cfg(test)]
mod acf_top_tests {
    use super::*;

    #[test]
    fn read_and_set() {
        let p = std::env::temp_dir().join(format!("vaporust_acftop_{}.acf", std::process::id()));
        fs::write(
            &p,
            "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"AutoUpdateBehavior\"\t\t\"0\"\n\t\"UserConfig\"\n\t{\n\t\t\"AutoUpdateBehavior\"\t\t\"9\"\n\t}\n}\n",
        )
        .unwrap();
        assert_eq!(acf_top(&p, "AutoUpdateBehavior").as_deref(), Some("0"));
        set_acf_top(&p, "AutoUpdateBehavior", "1").unwrap();
        assert_eq!(acf_top(&p, "AutoUpdateBehavior").as_deref(), Some("1"));
        let t = fs::read_to_string(&p).unwrap();
        assert!(t.contains("\t\t\"AutoUpdateBehavior\"\t\t\"9\""), "the nested one is not touched");
        // No key - it is added.
        set_acf_top(&p, "AllowOtherDownloadsWhileRunning", "1").unwrap();
        assert_eq!(acf_top(&p, "AllowOtherDownloadsWhileRunning").as_deref(), Some("1"));
        fs::remove_file(&p).unwrap();
    }
}

/// Add a new Steam library folder (for example on another disk), like "Settings ->
/// Storage -> Add drive" in Steam. Only while Steam is closed. Creates steamapps and
/// libraryfolder.vdf in the folder, appends an entry to steamapps\libraryfolders.vdf (backup .vdf.vaporust.bak).
pub fn add_library(steam_dir: &Path, folder: &Path) -> Result<PathBuf, String> {
    if !folder.is_absolute() {
        return Err(tr!("нужен полный путь к папке", "a full folder path is needed").into());
    }
    let n = norm(folder);
    let n = n.trim_end_matches('\\');
    for d in library_dirs(steam_dir) {
        let dn = norm(&d);
        let dn = dn.trim_end_matches('\\');
        if dn == n {
            return Err(tr!("эта папка уже есть в списке библиотек", "this folder is already in the library list").into());
        }
        if n.starts_with(&format!("{dn}\\steamapps")) {
            return Err(tr!("папку нельзя добавить внутри другой библиотеки", "a folder cannot be added inside another library").into());
        }
    }
    fs::create_dir_all(folder.join("steamapps")).map_err(|e| trf!("не удалось создать папку: {e}", "could not create the folder: {e}"))?;
    // Library id, as in Steam: a random large number.
    let contentid = {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        ((t ^ (std::process::id() as u128) << 40) % 9_000_000_000_000_000_000 + 1_000_000_000_000_000_000) as u64
    };
    let marker = folder.join("libraryfolder.vdf");
    if !marker.exists() {
        fs::write(&marker, format!("\"libraryfolder\"\n{{\n\t\"contentid\"\t\t\"{contentid}\"\n\t\"label\"\t\t\"\"\n}}\n"))
            .map_err(|e| trf!("не удалось записать libraryfolder.vdf: {e}", "could not write libraryfolder.vdf: {e}"))?;
    }
    let vdf = steam_dir.join("steamapps").join("libraryfolders.vdf");
    let text = fs::read_to_string(&vdf).map_err(|e| trf!("не удалось прочитать libraryfolders.vdf: {e}", "could not read libraryfolders.vdf: {e}"))?;
    // Next entry number: one more than the largest.
    let mut next = 0u32;
    let mut depth = 0;
    for line in text.lines() {
        let t = line.trim();
        match t {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ => {
                if depth == 1 {
                    if let Some(k) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                        if let Ok(i) = k.parse::<u32>() {
                            next = next.max(i + 1);
                        }
                    }
                }
            }
        }
    }
    let esc = folder.to_string_lossy().replace('\\', "\\\\");
    let entry = format!(
        "\t\"{next}\"\n\t{{\n\t\t\"path\"\t\t\"{esc}\"\n\t\t\"label\"\t\t\"\"\n\t\t\"contentid\"\t\t\"{contentid}\"\n\t\t\"totalsize\"\t\t\"0\"\n\t\t\"update_clean_bytes_tally\"\t\t\"0\"\n\t\t\"time_last_update_verified\"\t\t\"0\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n"
    );
    // Insert before the last "}" of the file.
    let end = text.rfind('}').ok_or(tr!("libraryfolders.vdf поврежден", "libraryfolders.vdf is damaged"))?;
    let mut body = String::with_capacity(text.len() + entry.len());
    body.push_str(text[..end].trim_end_matches(['\r', '\n']));
    body.push('\n');
    body.push_str(&entry);
    body.push_str(&text[end..]);
    let backup = vdf.with_extension("vdf.vaporust.bak");
    if !backup.exists() {
        let _ = fs::copy(&vdf, &backup);
    }
    let tmp = vdf.with_extension("vdf.tmp");
    fs::write(&tmp, body).map_err(|e| trf!("не удалось записать libraryfolders.vdf: {e}", "could not write libraryfolders.vdf: {e}"))?;
    fs::rename(&tmp, &vdf).map_err(|e| trf!("не удалось записать libraryfolders.vdf: {e}", "could not write libraryfolders.vdf: {e}"))?;
    Ok(folder.to_path_buf())
}

#[cfg(test)]
mod add_library_tests {
    use super::*;

    #[test]
    fn adds_entry() {
        let root = std::env::temp_dir().join(format!("vaporust_addlib_{}", std::process::id()));
        let steam = root.join("Steam");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::write(
            steam.join("steamapps").join("libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"c:\\steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"70\"\t\t\"1\"\n\t\t}\n\t}\n}\n",
        )
        .unwrap();
        let lib = root.join("Новая библиотека");
        add_library(&steam, &lib).unwrap();
        assert!(lib.join("steamapps").is_dir());
        assert!(lib.join("libraryfolder.vdf").is_file());
        let dirs = library_dirs(&steam);
        assert!(dirs.iter().any(|d| norm(d).trim_end_matches('\\') == norm(&lib).trim_end_matches('\\')), "{dirs:?}");
        let t = fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).unwrap();
        assert!(t.contains("\t\"1\"\n\t{"), "{t}");
        assert!(t.trim_end().ends_with('}'));
        // A repeat - refused; inside another library - refused.
        assert!(add_library(&steam, &lib).is_err());
        assert!(add_library(&steam, &lib.join("steamapps").join("x")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}

/// Clear Steam's temporary download files (appcache\httpcache): helps when Steam downloads
/// hang. Downloaded games and unfinished downloads are not touched. Only while Steam is closed.
/// Returns how many bytes were freed.
pub fn clear_download_cache(steam_dir: &Path) -> Result<u64, String> {
    let dir = steam_dir.join("appcache").join("httpcache");
    if !dir.is_dir() {
        return Ok(0);
    }
    let size = dir_size(&dir);
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let r = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        if let Err(err) = r {
            return Err(trf!("не удалось удалить {}: {err}", "could not delete {}: {err}", p.display()));
        }
    }
    Ok(size.saturating_sub(dir_size(&dir)))
}

/// Download region chosen in Steam settings (config.vdf: CellIDServerOverride), if set.
pub fn steam_cell_override(steam_dir: &Path) -> Option<u32> {
    let t = std::fs::read_to_string(steam_dir.join("config").join("config.vdf")).ok()?;
    let line = t.lines().find(|l| l.contains("\"CellIDServerOverride\""))?;
    let v = line.split('"').nth(3)?.trim().parse::<u32>().ok()?;
    (v > 0).then_some(v)
}

/// Temporary library folder for downloads while Steam is open (Steam does not see it: it is not in
/// libraryfolders.vdf). It lives in the same library so the move is a simple rename.
pub fn staging_dir(library: &Path) -> PathBuf {
    library.join("VaporustStaging")
}

/// Move a game finished in the temporary library to the real one: the common\<game> folder and
/// appmanifest (by rename, on the same disk). Steam sees a ready game.
pub fn unstage(library: &Path, appid: u32) -> Result<(), String> {
    let staging = staging_dir(library);
    let acf = staging.join("steamapps").join(format!("appmanifest_{appid}.acf"));
    let installdir = acf_top(&acf, "installdir").ok_or(tr!("во временной папке нет appmanifest", "the temporary folder has no appmanifest"))?;
    if installdir.is_empty() || installdir.contains("..") || installdir.contains(['\\', '/']) {
        return Err(tr!("странное имя папки игры", "strange game folder name").into());
    }
    let from = staging.join("steamapps").join("common").join(&installdir);
    let to = library.join("steamapps").join("common").join(&installdir);
    if to.exists() {
        return Err(trf!("папка {} уже есть в библиотеке", "folder {} is already in the library", to.display()));
    }
    fs::create_dir_all(library.join("steamapps").join("common")).map_err(|e| e.to_string())?;
    if from.exists() {
        fs::rename(&from, &to).map_err(|e| trf!("перенос папки игры: {e}", "moving the game folder: {e}"))?;
    }
    let acf_to = library.join("steamapps").join(format!("appmanifest_{appid}.acf"));
    fs::rename(&acf, &acf_to).map_err(|e| trf!("перенос appmanifest: {e}", "moving appmanifest: {e}"))?;
    let _ = fs::remove_dir_all(staging.join("steamapps").join("downloading").join(appid.to_string()));
    // An empty temporary library is no longer needed.
    let left = fs::read_dir(staging.join("steamapps").join("common")).map(|mut d| d.next().is_none()).unwrap_or(true);
    let acfs = fs::read_dir(staging.join("steamapps"))
        .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("appmanifest_")))
        .unwrap_or(false);
    // "old" holds pieces of earlier attempts: the folder stays (they go with "Remove Vaporust changes").
    if left && !acfs && !staging.join("old").exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(())
}

#[cfg(test)]
mod staging_tests {
    #[test]
    fn unstage_moves_game() {
        let lib = std::env::temp_dir().join(format!("vaporust_stage_{}", std::process::id()));
        let st = super::staging_dir(&lib).join("steamapps");
        std::fs::create_dir_all(st.join("common").join("Игра")).unwrap();
        std::fs::write(st.join("common").join("Игра").join("game.exe"), b"x").unwrap();
        std::fs::write(st.join("appmanifest_60.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"60\"\n\t\"installdir\"\t\t\"Игра\"\n}\n").unwrap();
        super::unstage(&lib, 60).unwrap();
        assert!(lib.join("steamapps").join("common").join("Игра").join("game.exe").exists());
        assert!(lib.join("steamapps").join("appmanifest_60.acf").exists());
        assert!(!super::staging_dir(&lib).exists(), "the empty temporary folder is removed");
        let _ = std::fs::remove_dir_all(&lib);
    }
}

/// An unfinished Vaporust game from the library goes to the temporary folder (to finish it while Steam is open):
/// appmanifest (or the hidden .acf.vaporust-paused), the common\<game> folder and steamapps\downloading\<appid>.
pub fn to_staging(library: &Path, appid: u32) -> Result<(), String> {
    let apps = library.join("steamapps");
    let st = staging_dir(library).join("steamapps");
    fs::create_dir_all(st.join("common")).map_err(|e| e.to_string())?;
    let acf = apps.join(format!("appmanifest_{appid}.acf"));
    let paused = apps.join(format!("appmanifest_{appid}.acf.vaporust-paused"));
    let src = if acf.exists() { acf } else if paused.exists() { paused } else { return Err(tr!("нет appmanifest игры", "the game has no appmanifest").into()) };
    let installdir = acf_top(&src, "installdir").unwrap_or_default();
    if installdir.is_empty() || installdir.contains("..") || installdir.contains(['\\', '/']) {
        return Err(tr!("странное имя папки игры", "strange game folder name").into());
    }
    let from = apps.join("common").join(&installdir);
    let to = st.join("common").join(&installdir);
    if from.exists() {
        if to.exists() {
            // A piece of an earlier attempt. If the temporary folder has no appmanifest of this game, the
            // piece belongs to nothing: it is moved aside (VaporustStaging\old), not deleted, and the
            // download goes on. With an appmanifest it is a real unfinished copy - then refuse.
            if st.join(format!("appmanifest_{appid}.acf")).exists() {
                return Err(trf!("во временной папке уже есть {}", "the temporary folder already has {}", to.display()));
            }
            let old = staging_dir(library).join("old");
            fs::create_dir_all(&old).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            fs::rename(&to, old.join(format!("{installdir}-{stamp}")))
                .map_err(|e| trf!("старый остаток во временной папке: {e}", "an old piece in the temporary folder: {e}"))?;
        }
        fs::rename(&from, &to).map_err(|e| trf!("перенос папки игры: {e}", "moving the game folder: {e}"))?;
    }
    let dl = apps.join("downloading").join(appid.to_string());
    if dl.exists() {
        let _ = fs::create_dir_all(st.join("downloading"));
        let _ = fs::rename(&dl, st.join("downloading").join(appid.to_string()));
    }
    fs::rename(&src, st.join(format!("appmanifest_{appid}.acf"))).map_err(|e| trf!("перенос appmanifest: {e}", "moving appmanifest: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod to_staging_tests {
    #[test]
    fn move_partial_to_staging_and_back() {
        let lib = std::env::temp_dir().join(format!("vaporust_tostage_{}", std::process::id()));
        let apps = lib.join("steamapps");
        std::fs::create_dir_all(apps.join("common").join("Игра")).unwrap();
        std::fs::write(apps.join("common").join("Игра").join("a.bin"), b"x").unwrap();
        std::fs::write(apps.join("appmanifest_60.acf.vaporust-paused"), "\"AppState\"\n{\n\t\"installdir\"\t\t\"Игра\"\n}\n").unwrap();
        super::to_staging(&lib, 60).unwrap();
        let st = super::staging_dir(&lib).join("steamapps");
        assert!(st.join("common").join("Игра").join("a.bin").exists());
        assert!(st.join("appmanifest_60.acf").exists());
        assert!(!apps.join("common").join("Игра").exists());
        super::unstage(&lib, 60).unwrap();
        assert!(apps.join("common").join("Игра").join("a.bin").exists());
        let _ = std::fs::remove_dir_all(&lib);
    }
}
