//! Несколько аккаунтов Steam и быстрое переключение (как «Сменить аккаунт» в Steam).
//! У каждого аккаунта своя папка входа Aurelia (основной - обычная папка, остальные -
//! <данные Vaporust>\accounts\<номер>), список - в accounts.json. Переключение = перезапуск
//! Vaporust с другой папкой; если Steam закрыт и в нем сохранен вход этого аккаунта, Steam тоже
//! переключается на него (AutoLoginUser в реестре, как делает сам Steam).

use crate::LauncherApp;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Entry {
    /// Папка в accounts\ (пусто - основной аккаунт).
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

/// Папка данных Vaporust (%APPDATA%\Vaporust или VaporustData рядом с exe).
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

/// В самом начале main (после portable::init): направить Aurelia в папку выбранного аккаунта.
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

/// Запомнить логин и имя текущего аккаунта (для списка).
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

/// Есть ли в Steam сохраненный вход этого логина (loginusers.vdf).
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
    /// Переключиться на аккаунт (папку) и перезапустить Vaporust.
    fn switch_account(&mut self, dir: String) {
        let mut a = load();
        let login = a.list.iter().find(|e| e.dir == dir).map(|e| e.login.clone()).unwrap_or_default();
        a.active = dir;
        save(&a);
        // Steam: на тот же аккаунт, если он закрыт и знает этот вход.
        if !login.is_empty() && !crate::local::steam_running() {
            if let Some(s) = &self.steam {
                if steam_knows(&s.dir, &login) {
                    let _ = set_steam_autologin(&login);
                }
            }
        }
        match crate::portable::restart() {
            Ok(()) => crate::tray::request_exit(),
            Err(e) => crate::popup::push(crate::popup::Kind::Info, "Аккаунты", e),
        }
    }

    pub(crate) fn accounts_window(&mut self, ctx: &egui::Context, net: &crate::net::NetState) {
        // Текущий аккаунт - в список (логин и имя).
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
        egui::Window::new("Аккаунты")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak("Vaporust помнит вход в каждый аккаунт отдельно. Переключение перезапускает программу.");
                ui.add_space(4.0);
                for e in &a.list {
                    let current = e.dir == a.active;
                    ui.horizontal(|ui| {
                        let title = match (e.name.is_empty(), e.login.is_empty()) {
                            (false, false) => format!("{} ({})", e.name, e.login),
                            (true, false) => e.login.clone(),
                            _ if e.dir.is_empty() => "Основной аккаунт".to_string(),
                            _ => "Новый аккаунт (вход еще не выполнен)".to_string(),
                        };
                        let text = egui::RichText::new(crate::richtext::fix(ui, &title));
                        ui.label(if current { text.strong().color(crate::theme::PLAY_TEXT) } else { text });
                        if current {
                            ui.weak("сейчас");
                        } else {
                            if ui.button("Переключиться").clicked() {
                                switch = Some(e.dir.clone());
                            }
                            if !e.dir.is_empty() && ui.small_button("Убрать").on_hover_text("Убрать из списка (вход в этот аккаунт забудется)").clicked() {
                                remove = Some(e.dir.clone());
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button("Добавить аккаунт...").on_hover_text("Vaporust перезапустится с окном входа").clicked() {
                    let dir = format!("{}", chrono::Utc::now().timestamp());
                    let mut a2 = load();
                    a2.list.push(Entry { dir: dir.clone(), ..Default::default() });
                    save(&a2);
                    switch = Some(dir);
                }
                ui.weak("Если Steam закрыт и в нем сохранен вход того же аккаунта, Steam тоже переключится на него.");
                ui.add_space(4.0);
                if ui.button("Закрыть").clicked() {
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
                // Папка входа удаляемого аккаунта - только внутри accounts\ и только с номером.
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
