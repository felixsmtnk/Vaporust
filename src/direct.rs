//! Запуск игр без Steam — для игр без защиты Steam (DRM-free).
//!
//! Что запускать, берём из таблицы запуска Steam (PICS config/launch через Aurelia):
//! exe, аргументы, рабочая папка, система. Таблица запоминается в настройках, чтобы
//! игра запускалась и без интернета. Достижения, облако и время в игре без Steam
//! не работают — об этом предупреждаем в «Свойствах».

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
    /// Рабочая папка относительно папки игры (пусто — папка exe).
    pub workdir: String,
}

/// Выбрать запись для Windows из таблицы запуска Steam (сначала основную «0»).
pub fn pick(options: &[aurelia::steam_client::LaunchOptionInfo]) -> Option<LaunchSpec> {
    let mine = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    // Чем меньше, тем лучше: своя разрядность, без указания, 32 бит; 64 бит на 32-битной — нельзя.
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
        // Обычный запуск (type default/пусто) — раньше особых режимов; порядок Steam сохраняется.
        .min_by_key(|(r, o)| (*r, !(o.launch_type.is_empty() || o.launch_type == "default")))
        .map(|(_, o)| o)?;
    Some(LaunchSpec { exe: best.executable.clone(), args: best.arguments.clone(), workdir: best.working_dir.clone() })
}

/// Путь внутри папки игры (Steam пишет пути и с \, и с /).
fn join(dir: &Path, rel: &str) -> PathBuf {
    rel.split(['\\', '/']).filter(|p| !p.is_empty()).fold(dir.to_path_buf(), |p, s| p.join(s))
}

/// Есть ли в папке игры steam_api(64).dll (на глубину до 4 папок) — тогда игре, скорее
/// всего, нужен Steam.
pub fn uses_steam_api(dir: &Path) -> bool {
    fn walk(d: &Path, depth: u32) -> bool {
        let Ok(rd) = std::fs::read_dir(d) else { return false };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name == "steam_api.dll" || name == "steam_api64.dll" {
                return true;
            }
            if depth < 4 && p.is_dir() && walk(&p, depth + 1) {
                return true;
            }
        }
        false
    }
    walk(dir, 0)
}

/// Разбить строку аргументов (кавычки учитываем).
fn split_args(s: &str) -> Vec<String> {
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
/// (форма Steam «что-то %command% ещё») — берём часть после него.
pub fn build_args(spec_args: &str, user: &str) -> Vec<String> {
    let user = match user.split_once("%command%") {
        Some((_, after)) => after,
        None => user,
    };
    let mut v = split_args(spec_args);
    v.extend(split_args(user));
    v
}

impl LauncherApp {
    /// «Играть» для игры с галочкой «без Steam»: спросить таблицу запуска (или взять
    /// запомненную) — запуск продолжится в direct_tick.
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
        // Многие игры без Steam ищут steam_appid.txt; переменная — для тех, кто читает её.
        cmd.env("SteamAppId", appid.to_string()).env("SteamGameId", appid.to_string());
        match cmd.spawn() {
            Ok(mut child) => {
                let (status, launching, rescan) = (self.launch_status.clone(), self.launching.clone(), self.rescan_needed.clone());
                launching.store(true, Ordering::Relaxed);
                *status.lock().unwrap() = format!("Игра запущена без Steam: {}", game.name);
                std::thread::spawn(move || {
                    let _ = child.wait();
                    *status.lock().unwrap() = "Готово.".into();
                    launching.store(false, Ordering::Relaxed);
                    rescan.store(true, Ordering::Relaxed);
                });
            }
            Err(e) => *self.launch_status.lock().unwrap() = format!("Не удалось запустить игру: {e}"),
        }
    }

    /// Раз в кадр: пришла таблица запуска — запомнить и запустить.
    pub(crate) fn direct_tick(&mut self) {
        let done = self.net.lock().unwrap().direct_spec.take();
        let Some((appid, res)) = done else { return };
        let spec = match res {
            Ok(Some(s)) => {
                self.settings.direct_specs.insert(appid, s.clone());
                Some(s)
            }
            // Нет связи — пробуем запомненную.
            _ => self.settings.direct_specs.get(&appid).cloned(),
        };
        match spec {
            Some(s) => self.direct_run(appid, s),
            None => {
                *self.launch_status.lock().unwrap() =
                    "Steam не сообщил, какой файл запускать у этой игры. Запустите её через Steam.".into()
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
        // Без указания разрядности — берётся она.
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
                println!("{} | steam_api: {} | {:?} | exe exists: {:?}", g.name, super::uses_steam_api(&g.dir), spec.map(|s| s.exe), exists);
            }
        });
    }
}
