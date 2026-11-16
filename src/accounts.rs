//! Multiple Steam accounts and quick switching (like "Change account" in Steam).
//! Each account has its own Aurelia login folder (the main one uses the usual folder, others use
//! <Vaporust data>\accounts\<number>); the list is in accounts.json. Switching = restarting
//! Vaporust with another folder; if Steam is closed and has a saved login for this account, Steam
//! is switched to it as well (loginusers.vdf flags and AutoLoginUser in the registry, with a restore point
//! before each switch).

use crate::LauncherApp;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Entry {
    /// Folder inside accounts\ (empty - the main account).
    pub dir: String,
    pub login: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Accounts {
    pub active: String,
    pub list: Vec<Entry>,
    /// Steam still has to be switched to the active account: the switch happened while Steam was
    /// open or before the account's login was known (a newly added account). Done after login.
    pub steam_pending: bool,
}

/// steam_pending of the running session (read once at start, so the file is not read every frame).
static STEAM_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Vaporust data folder (%APPDATA%\Vaporust or VaporustData next to the exe).
fn base() -> Option<PathBuf> {
    if crate::portable::enabled() {
        crate::portable::data_dir()
    } else {
        crate::portable::normal_dir().and_then(|d| d.parent().map(|p| p.to_path_buf()))
    }
}

fn file() -> Option<PathBuf> {
    base().map(|b| b.join("accounts.json"))
}

pub fn load() -> Accounts {
    let mut a: Accounts = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if !a.list.iter().any(|e| e.dir.is_empty()) {
        a.list.insert(0, Entry::default());
    }
    a
}

pub fn save(a: &Accounts) {
    if let Some(f) = file() {
        if let Some(p) = f.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(f, serde_json::to_string_pretty(a).unwrap_or_default());
    }
}

/// At the very start of main (after portable::init): point Aurelia to the selected account's folder.
pub fn init() {
    let a = load();
    STEAM_PENDING.store(a.steam_pending, std::sync::atomic::Ordering::Relaxed);
    if a.active.is_empty() {
        // The main account uses the usual folder. A folder of another account inherited from the
        // process that started us (an older Vaporust restarting itself) must not be used.
        if let (Ok(v), Some(b)) = (std::env::var("AURELIA_CONFIG_DIR"), base()) {
            if std::path::Path::new(&v).starts_with(b.join("accounts")) {
                std::env::remove_var("AURELIA_CONFIG_DIR");
            }
        }
        return;
    }
    if let Some(dir) = base().map(|b| b.join("accounts").join(&a.active)) {
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("AURELIA_CONFIG_DIR", &dir);
    }
}

/// Remember the login and name of the current account (for the list). If the login of the
/// active entry changed (a new account, or the entry held a wrong login), Steam may have been
/// switched to a different account: the Steam switch is done again after login.
pub fn note_current(login: &str, name: &str) {
    let mut a = load();
    let active = a.active.clone();
    if let Some(e) = a.list.iter_mut().find(|e| e.dir == active) {
        if e.login == login && (name.is_empty() || e.name == name) {
            return;
        }
        if !e.login.is_empty() && !e.login.eq_ignore_ascii_case(login) {
            a.steam_pending = true;
            STEAM_PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let Some(e) = a.list.iter_mut().find(|e| e.dir == active) else { return };
        e.login = login.to_string();
        if !name.is_empty() {
            e.name = name.to_string();
        }
        save(&a);
    }
}

/// Whether Steam has a saved login for this account name (loginusers.vdf).
fn steam_knows(steam_dir: &std::path::Path, login: &str) -> bool {
    std::fs::read_to_string(steam_dir.join("config").join("loginusers.vdf"))
        .map(|t| flag_login(&t, login).is_some())
        .unwrap_or(false)
}

#[cfg(windows)]
fn set_steam_autologin(login: &str) -> Result<(), String> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (k, _) = hkcu.create_subkey("Software\\Valve\\Steam").map_err(|e| e.to_string())?;
    k.set_value("AutoLoginUser", &login.to_string()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn set_steam_autologin(_login: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn get_steam_autologin() -> String {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .and_then(|k| k.get_value::<String, _>("AutoLoginUser"))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn get_steam_autologin() -> String {
    String::new()
}

/// Mark `login` in loginusers.vdf as the account Steam signs into by itself (the same technique
/// as TcNo Account Switcher and Steam Shadow Launcher): its block gets auto-login, "remember
/// password" and "most recent"; other blocks lose auto-login and "most recent". Every block stays:
/// Steam keeps other data per account name, and a removed block can bring back the password prompt.
/// Only keys that exist are changed; auto-login and "remember password" are added to the target
/// block if missing. Returns None if the login is not in the file. Line endings are kept.
pub(crate) fn flag_login(text: &str, login: &str) -> Option<String> {
    let crlf = text.contains("\r\n");
    let lines: Vec<&str> = text.lines().collect();
    // Account blocks: (line of "{", line of "}", account name).
    let mut blocks: Vec<(usize, usize, String)> = Vec::new();
    let (mut depth, mut start, mut name) = (0, 0, String::new());
    let mut allow_key = false; // older Steam writes "AllowAutoLogin" instead of "AutoLogin"
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t == "{" {
            depth += 1;
            if depth == 2 {
                start = i;
                name.clear();
            }
        } else if t == "}" {
            if depth == 2 {
                blocks.push((start, i, name.clone()));
            }
            depth -= 1;
        } else if depth == 2 {
            if let Some((k, v)) = crate::local::kv_pub(t) {
                if k.eq_ignore_ascii_case("AccountName") {
                    name = v;
                }
                allow_key |= k.eq_ignore_ascii_case("AllowAutoLogin");
            }
        }
    }
    let target = blocks.iter().position(|b| b.2.eq_ignore_ascii_case(login))?;
    let auto_key = if allow_key { "AllowAutoLogin" } else { "AutoLogin" };
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let (mut seen_auto, mut seen_remember) = (false, false);
    for (i, l) in lines.iter().enumerate() {
        let block = blocks.iter().position(|b| i > b.0 && i < b.1);
        let mine = block == Some(target);
        if let (Some(_), Some((k, _))) = (block, crate::local::kv_pub(l.trim())) {
            let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
            let set = |v: &str| format!("{indent}\"{k}\"\t\t\"{v}\"");
            if k.eq_ignore_ascii_case("AutoLogin") || k.eq_ignore_ascii_case("AllowAutoLogin") {
                seen_auto |= mine;
                out.push(set(if mine { "1" } else { "0" }));
                continue;
            }
            if k.eq_ignore_ascii_case("MostRecent") {
                out.push(set(if mine { "1" } else { "0" }));
                continue;
            }
            if mine && k.eq_ignore_ascii_case("RememberPassword") {
                seen_remember = true;
                out.push(set("1"));
                continue;
            }
        }
        if i == blocks[target].1 {
            if !seen_remember {
                out.push(format!("\t\t\"RememberPassword\"\t\t\"1\""));
            }
            if !seen_auto {
                out.push(format!("\t\t\"{auto_key}\"\t\t\"1\""));
            }
        }
        out.push(l.to_string());
    }
    let nl = if crlf { "\r\n" } else { "\n" };
    let mut body = out.join(nl);
    if text.ends_with('\n') {
        body.push_str(nl);
    }
    Some(body)
}

/// Folder with restore points of Steam's login choice: <Vaporust data>\steam_logins\<time>\.
fn points_dir() -> Option<PathBuf> {
    base().map(|b| b.join("steam_logins"))
}

/// How many restore points are kept.
const KEEP_POINTS: usize = 10;

/// Save loginusers.vdf and AutoLoginUser together (one restore point). The same state as the
/// newest point is not saved again. Old points beyond KEEP_POINTS are removed.
fn save_point(dir: &std::path::Path, vdf: &str, autologin: &str) -> Result<(), String> {
    let mut points = list_points(dir);
    if let Some(last) = points.last() {
        let same = std::fs::read_to_string(last.join("loginusers.vdf")).ok().as_deref() == Some(vdf)
            && std::fs::read_to_string(last.join("AutoLoginUser.txt")).unwrap_or_default() == autologin;
        if same {
            return Ok(());
        }
    }
    // Names sort by time: milliseconds plus a zero-padded counter for points within the same
    // millisecond ("-10" would sort before "-2").
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let mut p = dir.join(format!("{stamp}-000"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stamp}-{n:03}"));
        n += 1;
    }
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    std::fs::write(p.join("loginusers.vdf"), vdf).map_err(|e| e.to_string())?;
    std::fs::write(p.join("AutoLoginUser.txt"), autologin).map_err(|e| e.to_string())?;
    points.push(p);
    while points.len() > KEEP_POINTS {
        let old = points.remove(0);
        if old.starts_with(dir) {
            let _ = std::fs::remove_dir_all(old);
        }
    }
    Ok(())
}

/// Restore points, oldest first (folder names sort by time). Only complete points.
fn list_points(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.join("loginusers.vdf").is_file() && p.join("AutoLoginUser.txt").is_file()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Write a file through a temporary one, so a crash cannot leave half a file.
fn write_atomic(path: &std::path::Path, body: &str) -> Result<(), String> {
    let tmp = path.with_extension("vdf.vaporust-tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Switch Steam's own login to `login`: a restore point first, then loginusers.vdf and the
/// AutoLoginUser registry value. Only while Steam is closed (the caller checks).
fn switch_steam_login(steam_dir: &std::path::Path, points: &std::path::Path, login: &str) -> Result<(), String> {
    let path = steam_dir.join("config").join("loginusers.vdf");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let Some(new) = flag_login(&text, login) else {
        return Err(tr!("Steam не помнит вход в этот аккаунт", "Steam does not remember a login for this account").into());
    };
    save_point(points, &text, &get_steam_autologin())?;
    if new != text {
        write_atomic(&path, &new)?;
    }
    set_steam_autologin(login)
}

/// Put back Steam's login choice from the newest restore point (and remove that point).
fn restore_last_point(steam_dir: &std::path::Path, points: &std::path::Path) -> Result<String, String> {
    let Some(p) = list_points(points).pop() else {
        return Err(tr!("Нет сохраненных точек", "No saved points").into());
    };
    let vdf = std::fs::read_to_string(p.join("loginusers.vdf")).map_err(|e| e.to_string())?;
    let auto = std::fs::read_to_string(p.join("AutoLoginUser.txt")).map_err(|e| e.to_string())?;
    write_atomic(&steam_dir.join("config").join("loginusers.vdf"), &vdf)?;
    set_steam_autologin(&auto)?;
    let _ = std::fs::remove_dir_all(&p);
    Ok(auto)
}

impl LauncherApp {
    /// Switch to an account (folder) and restart Vaporust.
    fn switch_account(&mut self, dir: String) {
        let mut a = load();
        let login = a.list.iter().find(|e| e.dir == dir).map(|e| e.login.clone()).unwrap_or_default();
        a.active = dir;
        // Steam: switch to the same account now if it is closed and knows this login; otherwise
        // (Steam open, or a new account whose login is not known yet) after the next login.
        let mut done = false;
        if !login.is_empty() && !crate::local::steam_running() {
            if let (Some(s), Some(points)) = (&self.steam, points_dir()) {
                match switch_steam_login(&s.dir, &points, &login) {
                    Ok(()) => done = true,
                    Err(e) => crate::errlog::write(tr!("аккаунты", "accounts"), &e),
                }
            }
        }
        a.steam_pending = !done;
        save(&a);
        match crate::portable::restart() {
            Ok(()) => crate::tray::request_exit(),
            Err(e) => crate::popup::push(crate::popup::Kind::Info, tr!("Аккаунты", "Accounts"), e),
        }
    }

    pub(crate) fn accounts_window(&mut self, ctx: &egui::Context, net: &crate::net::NetState) {
        // Add the current account to the list (login and name).
        if let Some(login) = &net.account {
            if self.accounts_noted.as_deref() != Some(login.as_str()) {
                let name = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok()).and_then(|p| p.persona_name.clone()).unwrap_or_default();
                note_current(login, &name);
                if !name.is_empty() {
                    self.accounts_noted = Some(login.clone());
                }
            }
            // A pending Steam switch: the login is known now; wait until Steam is closed.
            if STEAM_PENDING.load(std::sync::atomic::Ordering::Relaxed) && !self.steam_running() {
                STEAM_PENDING.store(false, std::sync::atomic::Ordering::Relaxed);
                let mut a = load();
                a.steam_pending = false;
                save(&a);
                if let (Some(s), Some(points)) = (&self.steam, points_dir()) {
                    let text = if !steam_knows(&s.dir, login) {
                        Some(tr!(
                            "Steam не помнит вход в этот аккаунт. Войдите в него в Steam один раз сами (с галочкой «Запомнить меня») - потом переключение будет работать.",
                            "Steam does not remember a login for this account. Sign into it in Steam once yourself (with \"Remember me\" checked) - then switching will work."
                        )
                        .to_string())
                    } else {
                        match switch_steam_login(&s.dir, &points, login) {
                            Ok(()) => Some(tr!("Steam тоже переключен: при следующем запуске он войдет в этот аккаунт.", "Steam is switched too: it signs into this account at the next start.").to_string()),
                            Err(e) => {
                                crate::errlog::write(tr!("аккаунты", "accounts"), &e);
                                Some(trf!("Steam не удалось переключить: {e}", "Could not switch Steam: {e}"))
                            }
                        }
                    };
                    if let Some(t) = text {
                        crate::popup::push(crate::popup::Kind::Info, tr!("Аккаунты", "Accounts"), t);
                    }
                }
            }
        }
        if !self.accounts_open {
            return;
        }
        let a = load();
        let mut open = true;
        let mut switch = None;
        let mut remove = None;
        egui::Window::new(tr!("Аккаунты", "Accounts"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak(tr!("Vaporust помнит вход в каждый аккаунт отдельно. Переключение перезапускает программу.", "Vaporust remembers the login of each account separately. Switching restarts the program."));
                ui.add_space(4.0);
                for e in &a.list {
                    let current = e.dir == a.active;
                    ui.horizontal(|ui| {
                        let title = match (e.name.is_empty(), e.login.is_empty()) {
                            (false, false) => format!("{} ({})", e.name, e.login),
                            (true, false) => e.login.clone(),
                            _ if e.dir.is_empty() => tr!("Основной аккаунт", "Main account").to_string(),
                            _ => tr!("Новый аккаунт (вход еще не выполнен)", "New account (not logged in yet)").to_string(),
                        };
                        let text = egui::RichText::new(crate::richtext::fix(ui, &title));
                        ui.label(if current { text.strong().color(crate::theme::PLAY_TEXT) } else { text });
                        if current {
                            ui.weak(tr!("сейчас", "current"));
                        } else {
                            if ui.button(tr!("Переключиться", "Switch")).clicked() {
                                switch = Some(e.dir.clone());
                            }
                            if !e.dir.is_empty() && ui.small_button(tr!("Убрать", "Remove")).on_hover_text(tr!("Убрать из списка (вход в этот аккаунт забудется)", "Remove from the list (the login to this account will be forgotten)")).clicked() {
                                remove = Some(e.dir.clone());
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button(tr!("Добавить аккаунт...", "Add account...")).on_hover_text(tr!("Vaporust перезапустится с окном входа", "Vaporust will restart with the login window")).clicked() {
                    let dir = format!("{}", chrono::Utc::now().timestamp());
                    let mut a2 = load();
                    a2.list.push(Entry { dir: dir.clone(), ..Default::default() });
                    save(&a2);
                    switch = Some(dir);
                }
                ui.weak(tr!("Если Steam закрыт и в нем сохранен вход того же аккаунта, Steam тоже переключится на него.", "If Steam is closed and remembers the login of the same account, Steam switches to it too."));
                // Roll back Steam's login choice to the state before the last switch.
                let points = points_dir().map(|d| list_points(&d).len()).unwrap_or(0);
                if points > 0 {
                    let running = crate::local::steam_running();
                    let r = ui
                        .add_enabled(!running, egui::Button::new(tr!("Вернуть прошлый вход Steam", "Restore the previous Steam login")))
                        .on_hover_text(if running {
                            tr!("Сначала закройте Steam", "Close Steam first")
                        } else {
                            tr!(
                                "Steam снова будет входить в аккаунт, который был до последнего переключения (Vaporust хранит до 10 таких точек).",
                                "Steam will again sign into the account it used before the last switch (Vaporust keeps up to 10 such points)."
                            )
                        })
                        .on_disabled_hover_text(tr!("Сначала закройте Steam", "Close Steam first"));
                    if r.clicked() {
                        if let (Some(s), Some(d)) = (&self.steam, points_dir()) {
                            let text = match restore_last_point(&s.dir, &d) {
                                Ok(login) if login.is_empty() => tr!("Steam снова спросит, в какой аккаунт войти.", "Steam will ask which account to sign into again.").to_string(),
                                Ok(login) => trf!("Steam снова будет входить в «{login}».", "Steam will sign into \"{login}\" again."),
                                Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                            };
                            crate::popup::push(crate::popup::Kind::Info, tr!("Аккаунты", "Accounts"), text);
                        }
                    }
                }
                ui.add_space(4.0);
                if ui.button(tr!("Закрыть", "Close")).clicked() {
                    open = false;
                }
            });
        if !open {
            self.accounts_open = false;
        }
        if let Some(d) = remove {
            let mut a2 = load();
            a2.list.retain(|e| e.dir != d);
            save(&a2);
            if let Some(b) = base() {
                let p = b.join("accounts").join(&d);
                // Login folder of the account being removed - only inside accounts\ and only a numbered one.
                if d.chars().all(|c| c.is_ascii_digit()) && p.starts_with(b.join("accounts")) {
                    let _ = std::fs::remove_dir_all(p);
                }
            }
        }
        if let Some(d) = switch {
            self.switch_account(d);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{flag_login, list_points, save_point, KEEP_POINTS};

    fn block<'a>(t: &'a str, name: &str) -> &'a str {
        let i = t.find(&format!("\"{name}\"\n")).or_else(|| t.find(&format!("\"{name}\"\r\n"))).unwrap();
        let rest = &t[i..];
        &rest[..rest.find('}').unwrap()]
    }

    /// Current Steam: "AutoLogin", no "MostRecent". The target gets auto-login and remember
    /// password, the other account loses auto-login, nothing else changes.
    #[test]
    fn current_steam_format() {
        let t = "\"users\"\n{\n\t\"1\"\n\t{\n\t\t\"AccountName\"\t\t\"alpha\"\n\t\t\"PersonaName\"\t\t\"A\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"AutoLogin\"\t\t\"1\"\n\t\t\"timestamp\"\t\t\"5\"\n\t}\n\t\"2\"\n\t{\n\t\t\"AccountName\"\t\t\"Beta\"\n\t\t\"RememberPassword\"\t\t\"0\"\n\t\t\"AutoLogin\"\t\t\"0\"\n\t}\n}\n";
        let out = flag_login(t, "beta").unwrap();
        let a = block(&out, "1");
        let b = block(&out, "2");
        assert!(a.contains("\"AutoLogin\"\t\t\"0\"") && a.contains("\"RememberPassword\"\t\t\"1\"") && a.contains("\"timestamp\"\t\t\"5\""));
        assert!(b.contains("\"AutoLogin\"\t\t\"1\"") && b.contains("\"RememberPassword\"\t\t\"1\""));
        assert!(!out.contains("MostRecent"), "a key Steam does not write is not added");
        assert_eq!(out.matches("AccountName").count(), 2, "every account block stays");
        assert!(out.ends_with("}\n"));
        assert_eq!(flag_login(&out, "beta").unwrap(), out, "a second time - no change");
    }

    /// Older Steam: "AllowAutoLogin" and "MostRecent", Windows line endings; missing keys are added.
    #[test]
    fn older_format_and_crlf() {
        let t = "\"users\"\r\n{\r\n\t\"1\"\r\n\t{\r\n\t\t\"AccountName\"\t\t\"a\"\r\n\t\t\"MostRecent\"\t\t\"1\"\r\n\t\t\"AllowAutoLogin\"\t\t\"1\"\r\n\t}\r\n\t\"2\"\r\n\t{\r\n\t\t\"AccountName\"\t\t\"b\"\r\n\t\t\"MostRecent\"\t\t\"0\"\r\n\t}\r\n}\r\n";
        let out = flag_login(t, "b").unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "line endings kept");
        let a = block(&out, "1");
        let b = block(&out, "2");
        assert!(a.contains("\"MostRecent\"\t\t\"0\"") && a.contains("\"AllowAutoLogin\"\t\t\"0\""));
        assert!(b.contains("\"MostRecent\"\t\t\"1\"") && b.contains("\"AllowAutoLogin\"\t\t\"1\"") && b.contains("\"RememberPassword\"\t\t\"1\""));
    }

    #[test]
    fn unknown_login() {
        assert!(flag_login("\"users\"\n{\n\t\"1\"\n\t{\n\t\t\"AccountName\"\t\t\"a\"\n\t}\n}\n", "zzz").is_none());
        assert!(flag_login("garbage", "a").is_none());
    }

    /// Restore points: the same state is not saved twice, only the newest KEEP_POINTS stay.
    #[test]
    fn restore_points() {
        let dir = std::env::temp_dir().join(format!("vaporust_points_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save_point(&dir, "v1", "a").unwrap();
        save_point(&dir, "v1", "a").unwrap();
        assert_eq!(list_points(&dir).len(), 1, "the same state is not saved again");
        for i in 0..KEEP_POINTS + 3 {
            save_point(&dir, &format!("v{}", i + 2), "a").unwrap();
        }
        let points = list_points(&dir);
        assert_eq!(points.len(), KEEP_POINTS);
        let newest = std::fs::read_to_string(points.last().unwrap().join("loginusers.vdf")).unwrap();
        assert_eq!(newest, format!("v{}", KEEP_POINTS + 4));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod live {
    /// Live check on the real loginusers.vdf, in memory only (nothing is written): every account
    /// in the file is found, and switching changes only flag lines. Prints counts, no names.
    #[test]
    #[ignore]
    fn real_loginusers_dry_run() {
        let steam = crate::local::find_steam().expect("Steam");
        let text = std::fs::read_to_string(steam.dir.join("config").join("loginusers.vdf")).unwrap();
        let names: Vec<String> = text.lines().filter_map(|l| crate::local::kv_pub(l.trim())).filter(|(k, _)| k == "AccountName").map(|(_, v)| v).collect();
        println!("accounts in the file: {}", names.len());
        for n in &names {
            let out = super::flag_login(&text, n).expect("found");
            let changed: Vec<String> = text.lines().zip(out.lines()).filter(|(a, b)| a != b).map(|(_, b)| crate::local::kv_pub(b.trim()).map(|(k, v)| format!("{k}={v}")).unwrap_or_default()).collect();
            println!("lines: {} -> {}, changed: {:?}", text.lines().count(), out.lines().count(), changed);
        }
    }
}
