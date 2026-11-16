//! Multiple Steam accounts and quick switching (like "Change account" in Steam).
//! Each account has its own Aurelia login folder (the main one uses the usual folder, others use
//! <Vaporust data>\accounts\<number>); the list is in accounts.json. Switching = restarting
//! Vaporust with another folder; if Steam is closed and has a saved login for this account, Steam
//! is switched to it as well (AutoLoginUser in the registry, as Steam itself does).

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
}

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
    if a.active.is_empty() {
        return;
    }
    if let Some(dir) = base().map(|b| b.join("accounts").join(&a.active)) {
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("AURELIA_CONFIG_DIR", &dir);
    }
}

/// Remember the login and name of the current account (for the list).
pub fn note_current(login: &str, name: &str) {
    let mut a = load();
    let active = a.active.clone();
    if let Some(e) = a.list.iter_mut().find(|e| e.dir == active) {
        if e.login == login && (name.is_empty() || e.name == name) {
            return;
        }
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
        .map(|t| t.to_lowercase().contains(&format!("\"accountname\"\t\t\"{}\"", login.to_lowercase())))
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

impl LauncherApp {
    /// Switch to an account (folder) and restart Vaporust.
    fn switch_account(&mut self, dir: String) {
        let mut a = load();
        let login = a.list.iter().find(|e| e.dir == dir).map(|e| e.login.clone()).unwrap_or_default();
        a.active = dir;
        save(&a);
        // Steam: switch to the same account if it is closed and knows this login.
        if !login.is_empty() && !crate::local::steam_running() {
            if let Some(s) = &self.steam {
                if steam_knows(&s.dir, &login) {
                    let _ = set_steam_autologin(&login);
                }
            }
        }
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
