//! Локальный Steam: поиск через реестр, сканирование библиотек, запуск игры
//! через урезанный Steam.
//!
//! Версия 0.3: Steam и игра определяются по процессам, а не по реестру
//! (ActiveUser и RunningAppID в реестре оказались ненадёжными).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

// Флаги урезанного запуска Steam. Valve может менять их поддержку между версиями.
const STEAM_ARGS: [&str; 4] = ["-silent", "-no-browser", "-cef-disable-gpu", "-nofriendsui"];

#[derive(Clone)]
pub struct Game {
    pub appid: u32,
    pub name: String,
    /// Папка игры: <библиотека>\steamapps\common\<installdir>
    pub dir: PathBuf,
    /// Полностью установлена (StateFlags содержит 4). false = недокачана.
    pub ready: bool,
    /// Steam пометил игру как требующую обновления (StateFlags != 4).
    pub needs_update: bool,
}

#[derive(Clone)]
pub struct SteamInfo {
    pub dir: PathBuf,
    pub exe: PathBuf,
}

#[derive(Clone, Copy, Default)]
pub struct LaunchOptions {
    /// Эксперимент: замораживать steamwebhelper во время игры.
    /// Закрывать его бесполезно: Steam сразу запускает его заново (проверено в 0.3).
    pub freeze_webhelper: bool,
}

// ---------- Реестр ----------

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

/// Сбрасывает залипший ActiveUser: после ручного закрытия Steam он может остаться != 0.
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

// ---------- Разбор VDF/ACF (минимальный, построчный) ----------

fn kv(line: &str) -> Option<(String, String)> {
    let mut parts = line.trim().split('"').filter(|s| !s.trim().is_empty());
    let k = parts.next()?;
    let v = parts.next()?;
    Some((k.to_string(), v.replace("\\\\", "\\")))
}

/// Библиотеки Steam без повторов (главная папка Steam первой).
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

/// Свободное место на диске, где лежит папка.
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

/// Сколько файлов игры уже лежит в библиотеке (для продолжения прерванной установки).
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

/// Запущен ли сейчас steam.exe.
pub fn steam_running() -> bool {
    Procs::new().steam_running()
}

pub fn scan_games(steam_dir: &Path) -> Vec<Game> {
    let mut games: Vec<Game> = Vec::new();
    for lib in library_dirs(steam_dir) {
        let Ok(entries) = fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else {
                continue;
            };
            let (mut appid, mut name, mut installdir, mut flags) = (None, None, None, 0u32);
            for line in text.lines() {
                if let Some((k, v)) = kv(line) {
                    match k.as_str() {
                        "appid" if appid.is_none() => appid = v.parse::<u32>().ok(),
                        "name" if name.is_none() => name = Some(v),
                        "installdir" if installdir.is_none() => installdir = Some(v),
                        "StateFlags" => flags = v.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            if let (Some(appid), Some(name), Some(installdir)) = (appid, name, installdir) {
                // 228980 = Steamworks Common Redistributables, не игра
                if appid != 228980 && !games.iter().any(|g| g.appid == appid) {
                    let dir = lib.join("steamapps").join("common").join(installdir);
                    let ready = flags & 4 != 0;
                    let needs_update = ready && flags != 4;
                    games.push(Game { appid, name, dir, ready, needs_update });
                }
            }
        }
    }
    games
}

// ---------- appmanifest игры: состояние, язык, бета ----------

/// steamapps\appmanifest_<appid>.acf игры (рядом с папкой common).
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
    /// UserConfig — что выбрал пользователь; MountedConfig — что сейчас установлено.
    pub user_language: String,
    pub user_beta: String,
    pub mounted_language: String,
    pub mounted_beta: String,
}

impl AcfInfo {
    /// Steam что-то докачивает / должен обновить (4 = полностью установлена и актуальна).
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
            (2, "UserConfig", "betakey") => info.user_beta = v,
            (2, "MountedConfig", "language") => info.mounted_language = v,
            (2, "MountedConfig", "betakey") => info.mounted_beta = v,
            _ => {}
        }
    }
    info
}

/// Записать UserConfig.<key> (пустое значение — удалить ключ) и пометить игру как
/// требующую обновления: Steam докачает нужное при следующем запуске. Только при закрытом Steam.
pub fn set_user_config(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Не удалось прочитать файл игры: {e}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    let (mut depth, mut in_user, mut done, mut pending) = (0, false, false, String::new());
    let mut user_seen = false;
    for line in &lines {
        let t = line.trim();
        if t == "{" {
            depth += 1;
            in_user = depth == 2 && pending.eq_ignore_ascii_case("UserConfig");
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
                // Блока UserConfig нет — добавляем перед концом файла.
                out.push("\t\"UserConfig\"".into());
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
            if depth == 1 && k.eq_ignore_ascii_case("StateFlags") {
                // 2 = UpdateRequired: Steam сверит конфигурацию и докачает файлы.
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
    fs::write(path, body).map_err(|e| format!("Не удалось записать файл игры: {e}"))
}

/// Открыть папку в Проводнике.
pub fn open_folder(dir: &Path) {
    let _ = Command::new("explorer.exe").arg(dir).spawn();
}

// ---------- Процессы ----------

/// Путь в единый вид для сравнения: нижний регистр, обратные слэши.
fn norm(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('/', "\\")
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

    /// Игра запущена = есть процесс, чей exe лежит в папке игры.
    fn game_running(&mut self, game_dir: &str) -> bool {
        self.refresh();
        self.sys
            .processes()
            .values()
            .any(|p| p.exe().is_some_and(|e| norm(e).starts_with(game_dir)))
    }

    fn pids(&mut self, name: &str) -> Vec<u32> {
        self.refresh();
        self.named(name).map(|p| p.pid().as_u32()).collect()
    }

    /// Убивает все процессы с этими именами, возвращает, сколько убито.
    fn kill_all(&mut self, names: &[&str]) -> usize {
        self.refresh();
        let mut n = 0;
        for name in names {
            for p in self.named(name) {
                if p.kill() {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------- Заморозка процессов ----------

/// Приостановка процесса целиком (NtSuspendProcess) и сброс его памяти в файл подкачки.
/// Процесс остаётся жив, поэтому Steam не перезапускает его.
#[cfg(windows)]
mod freeze {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SUSPEND_RESUME,
    };

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSuspendProcess(h: HANDLE) -> i32;
        fn NtResumeProcess(h: HANDLE) -> i32;
    }

    fn with_process(pid: u32, f: impl FnOnce(HANDLE) -> bool) -> bool {
        unsafe {
            let access = PROCESS_SUSPEND_RESUME | PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION;
            let h = OpenProcess(access, 0, pid);
            if h.is_null() {
                return false;
            }
            let ok = f(h);
            CloseHandle(h);
            ok
        }
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
}

#[cfg(not(windows))]
mod freeze {
    pub fn suspend(_pid: u32) -> bool {
        false
    }
    pub fn resume(_pid: u32) -> bool {
        false
    }
}

/// Список замороженных процессов. Размораживает всё при выходе из run_chain
/// любым путём (конец игры, ошибка, «Перестать ждать»).
#[derive(Default)]
struct Frozen(Vec<u32>);

impl Frozen {
    fn freeze_new(&mut self, pids: Vec<u32>) {
        for pid in pids {
            if !self.0.contains(&pid) && freeze::suspend(pid) {
                self.0.push(pid);
            }
        }
    }

    fn resume_all(&mut self) {
        for pid in self.0.drain(..) {
            freeze::resume(pid);
        }
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        self.resume_all();
    }
}

// ---------- Запуск игры через урезанный Steam ----------

/// Ждёт условия; false при тайм-ауте или отмене.
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

fn shutdown_steam(steam: &SteamInfo, procs: &mut Procs, set: &dyn Fn(&str)) {
    set("Закрываю Steam...");
    let _ = Command::new(&steam.exe).arg("-shutdown").spawn();
    let never = AtomicBool::new(false);
    if !wait_until(Duration::from_secs(30), &never, || !procs.steam_running()) {
        set("Steam не закрылся за 30 с, завершаю принудительно...");
    }
    // Добиваем остатки в любом случае (webhelper и оверлей иногда остаются висеть).
    procs.kill_all(&["steam.exe", "steamwebhelper.exe", "gameoverlayui.exe"]);
    reset_active_user();
}

/// Закрыть Steam по кнопке (например, после «Не следить за игрой»).
pub fn close_steam(steam: &SteamInfo, set: &dyn Fn(&str)) {
    let mut procs = Procs::new();
    shutdown_steam(steam, &mut procs, set);
}

/// Поднять Steam в урезанном режиме и дождаться входа. Ok(Some(true)) — Steam запустили мы
/// (значит, и закрываем мы), Ok(Some(false)) — он уже был открыт пользователем, Ok(None) — отмена.
fn start_hidden_steam(
    steam: &SteamInfo,
    procs: &mut Procs,
    cancel: &AtomicBool,
    set: &dyn Fn(&str),
) -> Result<Option<bool>, String> {
    // Если Steam уже запущен пользователем, мы его не трогаем и не закрываем.
    if procs.steam_running() {
        return Ok(Some(false));
    }
    reset_active_user();
    set("Запускаю Steam в урезанном режиме...");
    Command::new(&steam.exe)
        .args(STEAM_ARGS)
        .spawn()
        .map_err(|e| format!("Не удалось запустить Steam: {e}"))?;
    if !wait_until(Duration::from_secs(90), cancel, || active_user() != 0) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        shutdown_steam(steam, procs, set);
        return Err("Steam не залогинился за 90 с. Войдите один раз через обычный интерфейс \
                    с галочкой «Запомнить пароль»."
            .into());
    }
    Ok(Some(true))
}

/// Обновить игру настоящим Steam без запуска игры (новая версия, смена языка или беты).
/// Steam поднимается незаметно, лаунчер показывает прогресс из appmanifest и закрывает Steam.
pub fn update_via_steam(
    steam: &SteamInfo,
    game: &Game,
    opts: LaunchOptions,
    cancel: &AtomicBool,
    set: &dyn Fn(&str),
) -> Result<String, String> {
    const CANCEL_MSG: &str =
        "Лаунчер больше не следит за обновлением. Steam останется открытым и докачает игру сам.";
    let mut procs = Procs::new();
    let acf = appmanifest_path(game);
    let we_started = match start_hidden_steam(steam, &mut procs, cancel, set)? {
        Some(w) => w,
        None => return Ok(CANCEL_MSG.into()),
    };

    set("Жду, пока Steam начнёт обновление...");
    let started = Instant::now();
    let mut nudged = false;
    let mut done_since: Option<Instant> = None;
    let mut last_progress = (0u64, Instant::now());
    let mut frozen = Frozen::default();
    let mut last_freeze: Option<Instant> = None;
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(CANCEL_MSG.into());
        }
        thread::sleep(Duration::from_secs(1));
        let info = read_acf(&acf);
        if info.updating() {
            done_since = None;
            if info.bytes_downloaded != last_progress.0 {
                last_progress = (info.bytes_downloaded, Instant::now());
            }
            set(&if info.bytes_to_download > 0 {
                format!(
                    "Steam обновляет игру: {} из {}",
                    crate::net::fmt_bytes(info.bytes_downloaded),
                    crate::net::fmt_bytes(info.bytes_to_download)
                )
            } else {
                "Steam обновляет игру...".into()
            });
            // Сам Steam может не начать загрузку (например, если в его настройках «обновлять
            // только при запуске») — через 45 с просим проверить файлы игры, это докачает недостающее.
            if !nudged && started.elapsed() > Duration::from_secs(45) && info.bytes_downloaded == 0 {
                nudged = true;
                set("Steam не начал обновление сам — запускаю проверку файлов игры...");
                let _ = Command::new(&steam.exe)
                    .arg(format!("steam://validate/{}", game.appid))
                    .spawn();
            }
            if last_progress.1.elapsed() > Duration::from_secs(600) {
                break Err("Обновление не двигается 10 минут. Steam оставлен открытым.".to_string());
            }
        } else if done_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(5) {
            break Ok(if started.elapsed() < Duration::from_secs(10) {
                "Игра уже обновлена.".to_string()
            } else {
                "Игра обновлена.".to_string()
            });
        }
        // Пока качает, steamwebhelper не нужен — замораживаем, как во время игры.
        if opts.freeze_webhelper
            && started.elapsed() >= Duration::from_secs(20)
            && last_freeze.map_or(true, |t| t.elapsed() >= Duration::from_secs(10))
        {
            last_freeze = Some(Instant::now());
            frozen.freeze_new(procs.pids("steamwebhelper.exe"));
        }
    };
    frozen.resume_all();
    if we_started && result.is_ok() {
        set("Обновление готово, закрываю Steam...");
        thread::sleep(Duration::from_secs(3));
        shutdown_steam(steam, &mut procs, set);
    }
    result
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
    const CANCEL_MSG: &str =
        "Лаунчер больше не следит за игрой. Steam останется открытым — закройте его кнопкой «Закрыть Steam».";

    let we_started = match start_hidden_steam(steam, &mut procs, cancel, set)? {
        Some(w) => w,
        None => return Ok(CANCEL_MSG.into()),
    };

    set("Запускаю игру...");
    Command::new(&steam.exe)
        .arg(format!("steam://rungameid/{}", game.appid))
        .spawn()
        .map_err(|e| format!("Не удалось отправить команду запуска: {e}"))?;

    // Ждём процесс игры. Если Steam сначала обновляет игру (новая версия, сменили язык или
    // бету), показываем прогресс из appmanifest и не считаем это зависанием.
    let acf = appmanifest_path(game);
    let mut deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if cancelled() {
            return Ok(CANCEL_MSG.into());
        }
        if procs.game_running(&game_dir) {
            break;
        }
        let info = read_acf(&acf);
        if info.updating() {
            deadline = Instant::now() + Duration::from_secs(120);
            let progress = if info.bytes_to_download > 0 {
                format!(
                    ": {} из {}",
                    crate::net::fmt_bytes(info.bytes_downloaded),
                    crate::net::fmt_bytes(info.bytes_to_download)
                )
            } else {
                "...".into()
            };
            set(&format!("Steam обновляет игру перед запуском{progress}"));
        }
        if Instant::now() > deadline {
            if we_started {
                shutdown_steam(steam, &mut procs, set);
            }
            return Err("Игра не запустилась за 120 с.".into());
        }
        thread::sleep(Duration::from_secs(1));
    }

    set("Игра запущена, жду выхода...");
    let started = Instant::now();
    let mut gone_since: Option<Instant> = None;
    let mut frozen = Frozen::default();
    let mut last_freeze: Option<Instant> = None;
    loop {
        thread::sleep(Duration::from_secs(1));
        if cancelled() {
            return Ok(CANCEL_MSG.into()); // Frozen размораживает при выходе
        }
        if procs.game_running(&game_dir) {
            gone_since = None;
        } else if gone_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(6) {
            break;
        }
        // Через 20 с после старта игры замораживаем webhelper; раз в 10 с
        // подхватываем новые его процессы, если Steam их создал.
        if opts.freeze_webhelper
            && started.elapsed() >= Duration::from_secs(20)
            && last_freeze.map_or(true, |t| t.elapsed() >= Duration::from_secs(10))
        {
            last_freeze = Some(Instant::now());
            let before = frozen.0.len();
            frozen.freeze_new(procs.pids("steamwebhelper.exe"));
            if frozen.0.len() != before {
                set(&format!(
                    "Игра запущена, жду выхода... (steamwebhelper заморожен, процессов: {})",
                    frozen.0.len()
                ));
            }
        }
    }
    // Размораживаем до закрытия Steam, иначе он не сможет нормально завершиться.
    frozen.resume_all();

    if we_started {
        set("Игра закрыта, даю Steam синхронизировать сохранения...");
        thread::sleep(Duration::from_secs(8));
        shutdown_steam(steam, &mut procs, set);
    }
    Ok("Готово.".into())
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
        assert_eq!(i.mounted_language, "english"); // MountedConfig не тронут
        assert_eq!(i.state_flags, 6);
        assert!(i.updating());

        set_user_config(&p, "BetaKey", "").unwrap(); // public = ключ убран
        assert_eq!(read_acf(&p).user_beta, "");
    }

    #[test]
    fn no_user_config_block() {
        let p = tmp("nouser", "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"StateFlags\"\t\t\"4\"\n}\n");
        set_user_config(&p, "language", "german").unwrap();
        assert_eq!(read_acf(&p).user_language, "german");
    }
}
