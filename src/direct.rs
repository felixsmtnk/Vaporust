//! Запуск игр без Steam - для игр без защиты Steam (DRM-free).
//!
//! Что запускать, берем из таблицы запуска Steam (PICS config/launch через Aurelia):
//! exe, аргументы, рабочая папка, система. Таблица запоминается в настройках, чтобы
//! игра запускалась и без интернета. Достижения, облако и время в игре без Steam
//! не работают - об этом предупреждаем в «Свойствах».

use crate::net::{Cmd, NetState};
use crate::{launchopts, LauncherApp};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Как запускать игру напрямую (из таблицы запуска Steam).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LaunchSpec {
    /// exe относительно папки игры.
    pub exe: String,
    pub args: String,
    /// Рабочая папка относительно папки игры (пусто - папка exe).
    pub workdir: String,
}

/// Выбрать запись для Windows из таблицы запуска Steam (сначала основную «0»).
pub fn pick(options: &[aurelia::steam_client::LaunchOptionInfo]) -> Option<LaunchSpec> {
    let mine = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    // Чем меньше, тем лучше: своя разрядность, без указания, 32 бит; 64 бит на 32-битной - нельзя.
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
        // Обычный запуск (type default/пусто) - раньше особых режимов; порядок Steam сохраняется.
        .min_by_key(|(r, o)| (*r, !(o.launch_type.is_empty() || o.launch_type == "default")))
        .map(|(_, o)| o)?;
    Some(LaunchSpec { exe: best.executable.clone(), args: best.arguments.clone(), workdir: best.working_dir.clone() })
}

/// Путь внутри папки игры (Steam пишет пути и с \, и с /).
fn join(dir: &Path, rel: &str) -> PathBuf {
    rel.split(['\\', '/']).filter(|p| !p.is_empty()).fold(dir.to_path_buf(), |p, s| p.join(s))
}

/// Разбить строку аргументов (кавычки учитываем).
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

/// Аргументы: из таблицы Steam плюс параметры запуска пользователя. «%command%»
/// (форма Steam «что-то %command% еще») - берем часть после него.
pub fn build_args(spec_args: &str, user: &str) -> Vec<String> {
    let user = match user.split_once("%command%") {
        Some((_, after)) => after,
        None => user,
    };
    let mut v = split_args(spec_args);
    v.extend(split_args(user));
    v
}

// ---------- Проверка защиты (можно ли запускать без Steam) ----------

/// Что нашлось в файлах игры и на странице магазина.
#[derive(Clone, Debug, Default)]
pub struct DrmReport {
    /// exe, обернутые защитой Steam (SteamStub: секция «.bind» в файле) - без Steam не запустятся.
    pub steamstub: Vec<String>,
    /// В папке есть steam_api(64).dll: игра обращается к Steam (может перезапуститься через
    /// него, работать без достижений или не запуститься).
    pub steam_api: bool,
    /// Сколько exe проверено.
    pub exes: usize,
    /// Пометка магазина о сторонней защите (Denuvo и т.п.).
    pub drm_notice: Option<String>,
    /// Нужен сторонний аккаунт (EA, Ubisoft...).
    pub account_notice: Option<String>,
}

impl DrmReport {
    /// Итог одной строкой и можно ли без Steam (Some(true) - да, Some(false) - нет, None - как повезет).
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

/// Есть ли в exe секция «.bind» (обертка SteamStub). Читается только заголовок файла.
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

/// Проверить файлы игры: все exe на глубину до 4 папок и steam_api.
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

/// Пометки магазина о защите: (сторонняя защита, сторонний аккаунт). Открытый API магазина.
pub fn store_drm_notes(appid: u32) -> (Option<String>, Option<String>) {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=russian&filters=basic");
    let Ok(resp) = ehttp::fetch_blocking(&ehttp::Request::get(url)) else { return (None, None) };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.bytes) else { return (None, None) };
    let data = v.get(appid.to_string()).and_then(|x| x.get("data"));
    let get = |k: &str| {
        data.and_then(|d| d.get(k)).and_then(|x| x.as_str()).map(crate::store::plain_text).filter(|s| !s.trim().is_empty())
    };
    (get("drm_notice"), get("ext_user_account_notice"))
}

/// Проверка целиком (файлы и магазин) - для фонового потока.
pub fn check_drm(appid: u32, dir: &Path) -> DrmReport {
    let mut r = scan_drm(dir);
    let (drm, acc) = store_drm_notes(appid);
    r.drm_notice = drm;
    r.account_notice = acc;
    r
}

impl LauncherApp {
    /// «Играть» для игры с галочкой «без Steam»: спросить таблицу запуска (или взять
    /// запомненную) - запуск продолжится в direct_tick.
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

    /// Запустить exe игры и следить за процессом в отдельном потоке.
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
        // Многие игры без Steam ищут steam_appid.txt; переменная - для тех, кто читает ее.
        cmd.env("SteamAppId", appid.to_string()).env("SteamGameId", appid.to_string());
        let script = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
        let (status, launching, rescan) = (self.launch_status.clone(), self.launching.clone(), self.rescan_needed.clone());
        launching.store(true, Ordering::Relaxed);
        // В отдельном потоке: команда перед запуском может идти долго.
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
                        // exe мог быть лаунчером и запустить саму игру: ждем, пока в папке игры
                        // не останется процессов.
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

    /// Раз в кадр: пришла таблица запуска - запомнить и запустить.
    pub(crate) fn direct_tick(&mut self) {
        let done = self.net.lock().unwrap().direct_spec.take();
        let Some((appid, res)) = done else { return };
        let spec = match res {
            Ok(Some(s)) => {
                self.settings.direct_specs.insert(appid, s.clone());
                Some(s)
            }
            // Нет связи - пробуем запомненную.
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
        // Без указания разрядности - берется она.
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
    /// Живой тест (только чтение): что запустится без Steam у установленных игр.
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
    /// Живой тест (только чтение): защита у установленных игр.
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
