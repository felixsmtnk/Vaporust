//! Launching games without Steam - for games without Steam protection (DRM-free).
//!
//! What to launch comes from Steam's launch table (PICS config/launch via Aurelia):
//! exe, arguments, working folder, OS. The table is remembered in settings so that
//! the game also starts without internet. Achievements, cloud and playtime do not work
//! without Steam - the "Properties" window warns about this.

use crate::net::{Cmd, NetState};
use crate::{launchopts, LauncherApp};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// How to launch the game directly (from Steam's launch table).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LaunchSpec {
    /// Exe relative to the game folder.
    pub exe: String,
    pub args: String,
    /// Working folder relative to the game folder (empty - the exe folder).
    pub workdir: String,
}

/// Pick the Windows entry from Steam's launch table (the main "0" first).
pub fn pick(options: &[aurelia::steam_client::LaunchOptionInfo]) -> Option<LaunchSpec> {
    let mine = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    // Smaller is better: own bitness, unspecified, 32 bit; 64 bit on a 32-bit system is not allowed.
    let arch_rank = |a: &str| match a {
        a if a == mine => Some(0),
        "" => Some(1),
        "32" => Some(2),
        _ => None,
    };
    let best = options
        .iter()
        .filter(|o| {
            !o.executable.is_empty()
                && (o.oslist.is_empty() || o.oslist.contains("windows"))
                && o.executable.to_lowercase().ends_with(".exe")
        })
        .filter_map(|o| arch_rank(&o.osarch).map(|r| (r, o)))
        // Normal launch (type default/empty) comes before special modes; Steam's order is kept.
        .min_by_key(|(r, o)| (*r, !(o.launch_type.is_empty() || o.launch_type == "default")))
        .map(|(_, o)| o)?;
    Some(LaunchSpec { exe: best.executable.clone(), args: best.arguments.clone(), workdir: best.working_dir.clone() })
}

/// Path inside the game folder (Steam writes paths with both \ and /).
fn join(dir: &Path, rel: &str) -> PathBuf {
    rel.split(['\\', '/']).filter(|p| !p.is_empty()).fold(dir.to_path_buf(), |p, s| p.join(s))
}

/// Split an argument string (quotes are respected).
pub(crate) fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quoted) = (String::new(), false);
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Arguments: from Steam's table plus the user's launch options. "%command%"
/// (Steam's form "something %command% more") - we take the part after it.
pub fn build_args(spec_args: &str, user: &str) -> Vec<String> {
    let user = match user.split_once("%command%") {
        Some((_, after)) => after,
        None => user,
    };
    let mut v = split_args(spec_args);
    v.extend(split_args(user));
    v
}

// ---------- Protection check (can it run without Steam) ----------

/// What was found in the game files and on the store page.
#[derive(Clone, Debug, Default)]
pub struct DrmReport {
    /// Exes wrapped with Steam protection (SteamStub: a ".bind" section in the file) - will not run without Steam.
    pub steamstub: Vec<String>,
    /// The folder has steam_api(64).dll: the game talks to Steam (may relaunch through
    /// it, run without achievements, or not start).
    pub steam_api: bool,
    /// How many exes were checked.
    pub exes: usize,
    /// Store note about third-party protection (Denuvo and the like).
    pub drm_notice: Option<String>,
    /// Requires a third-party account (EA, Ubisoft...).
    pub account_notice: Option<String>,
}

impl DrmReport {
    /// Summary in one line and whether it works without Steam (Some(true) - yes, Some(false) - no, None - maybe).
    pub fn verdict(&self) -> (Option<bool>, String) {
        if !self.steamstub.is_empty() {
            return (Some(false), "Защита Steam (SteamStub): без Steam игра не запустится.".into());
        }
        if self.drm_notice.is_some() {
            return (Some(false), "Сторонняя защита: она проверяет покупку через Steam, без Steam игра не запустится.".into());
        }
        if self.account_notice.is_some() {
            return (None, "Игре нужен сторонний аккаунт или лаунчер: без Steam может не запуститься. Попробуйте.".into());
        }
        if self.steam_api {
            return (
                None,
                "Защиты Steam нет, но игра обращается к Steam (steam_api.dll): может перезапуститься \
                 через Steam, запуститься без достижений или не запуститься. Попробуйте."
                    .into(),
            );
        }
        (Some(true), "Защиты не найдено: игра должна запускаться без Steam.".into())
    }
}

/// Whether the exe has a ".bind" section (SteamStub wrapper). Only the file header is read.
pub fn has_steamstub(exe: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(exe) else { return false };
    let mut buf = vec![0u8; 8192];
    let Ok(n) = f.read(&mut buf) else { return false };
    let b = &buf[..n];
    let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize);
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize);
    if b.get(0..2) != Some(b"MZ") {
        return false;
    }
    let Some(pe) = u32_at(0x3C) else { return false };
    if b.get(pe..pe + 4) != Some(b"PE\0\0") {
        return false;
    }
    let (Some(count), Some(opt)) = (u16_at(pe + 6), u16_at(pe + 20)) else { return false };
    let table = pe + 24 + opt;
    (0..count.min(96)).any(|i| b.get(table + i * 40..table + i * 40 + 8).is_some_and(|name| name.starts_with(b".bind")))
}

/// Check the game files: all exes up to 4 folders deep and steam_api.
pub fn scan_drm(dir: &Path) -> DrmReport {
    fn walk(base: &Path, d: &Path, depth: u32, r: &mut DrmReport) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if p.is_dir() {
                if depth < 4 {
                    walk(base, &p, depth + 1, r);
                }
            } else if name == "steam_api.dll" || name == "steam_api64.dll" {
                r.steam_api = true;
            } else if name.ends_with(".exe") && r.exes < 400 {
                r.exes += 1;
                if has_steamstub(&p) {
                    r.steamstub.push(p.strip_prefix(base).unwrap_or(&p).display().to_string());
                }
            }
        }
    }
    let mut r = DrmReport::default();
    walk(dir, dir, 0, &mut r);
    r
}

/// Store notes about protection: (third-party DRM, third-party account). Public store API.
pub fn store_drm_notes(appid: u32) -> (Option<String>, Option<String>) {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l={lang}&filters=basic", lang = crate::lang::STEAM);
    let Ok(resp) = ehttp::fetch_blocking(&ehttp::Request::get(url)) else { return (None, None) };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.bytes) else { return (None, None) };
    let data = v.get(appid.to_string()).and_then(|x| x.get("data"));
    let get = |k: &str| {
        data.and_then(|d| d.get(k)).and_then(|x| x.as_str()).map(crate::store::plain_text).filter(|s| !s.trim().is_empty())
    };
    (get("drm_notice"), get("ext_user_account_notice"))
}

/// Full check (files and store) - for a background thread.
pub fn check_drm(appid: u32, dir: &Path) -> DrmReport {
    let mut r = scan_drm(dir);
    let (drm, acc) = store_drm_notes(appid);
    r.drm_notice = drm;
    r.account_notice = acc;
    r
}

impl LauncherApp {
    /// "Play" for a game with the "without Steam" checkbox: ask for the launch table (or use
    /// the remembered one) - the launch continues in direct_tick.
    pub(crate) fn direct_launch(&mut self, appid: u32, net: &NetState) {
        if self.launching.load(Ordering::Relaxed) {
            return;
        }
        if net.account.is_some() {
            *self.launch_status.lock().unwrap() = "Готовлю запуск без Steam...".into();
            let _ = self.tx.send(Cmd::DirectSpec(appid));
        } else if let Some(spec) = self.settings.direct_specs.get(&appid).cloned() {
            self.direct_run(appid, spec);
        } else {
            *self.launch_status.lock().unwrap() =
                "Для первого запуска без Steam нужен вход в аккаунт (узнать, какой файл запускать).".into();
        }
    }

    /// Start the game exe and watch the process in a separate thread.
    fn direct_run(&mut self, appid: u32, spec: LaunchSpec) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        let exe = join(&game.dir, &spec.exe);
        if !exe.exists() {
            *self.launch_status.lock().unwrap() = format!("Не найден файл игры: {}", exe.display());
            return;
        }
        let cwd = if spec.workdir.is_empty() {
            exe.parent().map(Path::to_path_buf).unwrap_or(game.dir.clone())
        } else {
            join(&game.dir, &spec.workdir)
        };
        let user = self
            .steam
            .as_ref()
            .and_then(|s| launchopts::find_localconfig(&s.dir))
            .and_then(|p| launchopts::read(&p, appid).ok())
            .unwrap_or_default();
        let args = build_args(&spec.args, &user);
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(&args).current_dir(&cwd);
        // Many games without Steam look for steam_appid.txt; the variable is for those that read it.
        cmd.env("SteamAppId", appid.to_string()).env("SteamGameId", appid.to_string());
        let script = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
        let (status, launching, rescan) = (self.launch_status.clone(), self.launching.clone(), self.rescan_needed.clone());
        launching.store(true, Ordering::Relaxed);
        // In a separate thread: the pre-launch command may take a long time.
        std::thread::spawn(move || {
            if !script.before.trim().is_empty() {
                *status.lock().unwrap() = "Выполняю команду перед запуском...".into();
            }
            let warn = crate::scripts::run_before(&script, appid, &game.name, &game.dir);
            match cmd.spawn() {
                Ok(mut child) => {
                    *status.lock().unwrap() = format!("Игра запущена без Steam: {}", game.name);
                    {
                        let _running = crate::local::RunningGuard::set(appid, &game.name, &game.dir);
                        let _ = child.wait();
                        // The exe may be a launcher that starts the game itself: wait until no processes
                        // are left in the game folder.
                        while crate::local::game_alive(&game.dir) {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                    let after = crate::scripts::run_after(&script, appid, &game.name, &game.dir);
                    *status.lock().unwrap() = match warn.or(after) {
                        Some(w) => format!("Готово. {w}"),
                        None => "Готово.".into(),
                    };
                }
                Err(e) => *status.lock().unwrap() = format!("Не удалось запустить игру: {e}"),
            }
            launching.store(false, Ordering::Relaxed);
            rescan.store(true, Ordering::Relaxed);
        });
    }

    /// Once per frame: the launch table arrived - remember it and launch.
    pub(crate) fn direct_tick(&mut self) {
        let done = self.net.lock().unwrap().direct_spec.take();
        let Some((appid, res)) = done else { return };
        let spec = match res {
            Ok(Some(s)) => {
                self.settings.direct_specs.insert(appid, s.clone());
                Some(s)
            }
            // No connection - try the remembered one.
            _ => self.settings.direct_specs.get(&appid).cloned(),
        };
        match spec {
            Some(s) => self.direct_run(appid, s),
            None => {
                *self.launch_status.lock().unwrap() =
                    "Steam не сообщил, какой файл запускать у этой игры. Запустите ее через Steam.".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurelia::steam_client::LaunchOptionInfo;

    fn opt(id: &str, exe: &str, os: &str, arch: &str) -> LaunchOptionInfo {
        LaunchOptionInfo { id: id.into(), executable: exe.into(), oslist: os.into(), osarch: arch.into(), ..Default::default() }
    }

    #[test]
    fn picks_windows_exe() {
        let v = vec![opt("0", "game.sh", "linux", ""), opt("1", "bin\\win32\\game.exe", "windows", "32"), opt("2", "bin\\win64\\game.exe", "windows", "64")];
        let s = pick(&v).unwrap();
        let want = if cfg!(target_pointer_width = "64") { "bin\\win64\\game.exe" } else { "bin\\win32\\game.exe" };
        assert_eq!(s.exe, want);
        // Without bitness specified - that one is taken.
        assert_eq!(pick(&[opt("0", "hl.exe", "", "")]).unwrap().exe, "hl.exe");
        assert!(pick(&[opt("0", "game.sh", "linux", "")]).is_none());
    }

    #[test]
    fn args() {
        assert_eq!(build_args("-game cstrike", "-novid \"-w 1280\""), vec!["-game", "cstrike", "-novid", "-w 1280"]);
        assert_eq!(build_args("", "PROTON=1 %command% -fullscreen"), vec!["-fullscreen"]);
        assert_eq!(join(Path::new("C:\\g"), "bin\\x/y.exe"), Path::new("C:\\g").join("bin").join("x").join("y.exe"));
    }
}

#[cfg(test)]
mod live {
    /// Live test (read only): what installed games will start without Steam.
    #[test]
    #[ignore]
    fn direct_specs_live() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for g in games.iter().filter(|g| g.ready) {
                let spec = c.fetch_launch_options(g.appid).await.ok().and_then(|o| super::pick(&o));
                let exists = spec.as_ref().map(|s| super::join(&g.dir, &s.exe).exists());
                println!("{} | steam_api: {} | {:?} | exe exists: {:?}", g.name, super::scan_drm(&g.dir).steam_api, spec.map(|s| s.exe), exists);
            }
        });
    }
}

#[cfg(test)]
mod drm_live {
    /// Live test (read only): protection of installed games.
    #[test]
    #[ignore]
    fn drm_scan_live() {
        let steam = crate::local::find_steam().unwrap();
        for g in crate::local::scan_games(&steam.dir).iter().filter(|g| g.ready) {
            let r = super::check_drm(g.appid, &g.dir);
            println!("{} | exe {} | stub {:?} | api {} | {:?} | {:?} | {}", g.name, r.exes, r.steamstub, r.steam_api, r.drm_notice, r.account_notice, r.verdict().1);
        }
    }
}
