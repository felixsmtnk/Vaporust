//! Uninstalling a game from disk: confirmation (from the game menu and from "Properties") and the removal itself.
//!
//! Fully installed games are uninstalled by Aurelia (as before). Unfinished Vaporust downloads
//! we remove ourselves: their appmanifest is hidden from Steam (see local::stash_manifest),
//! and Aurelia does not find them - previously such a game stayed on disk with a "Resume" button.

use crate::net::{Cmd, NetState};
use crate::{local, LauncherApp};
use eframe::egui;
use std::sync::atomic::Ordering;

impl LauncherApp {
    /// Ask for uninstall confirmation. The size comes from appmanifest, and if it is 0 there
    /// (an unfinished game) - it is counted by the folder.
    pub(crate) fn ask_uninstall(&mut self, appid: u32, name: String) {
        if crate::custom::is_custom(appid) {
            self.uninstall_ask = Some((appid, name, 0));
            return;
        }
        let shared = !self.neighbours(appid).is_empty();
        let Some(g) = self.games.iter().find(|g| g.appid == appid) else { return };
        // A folder shared with other games: its size is not this game's size.
        let size = if (g.size > 0 && g.ready) || shared { g.size } else { local::dir_size(&g.dir) };
        self.uninstall_ask = Some((appid, name, size));
    }

    /// Confirmation window (on top of the main window).
    pub(crate) fn uninstall_dialog(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some((appid, name, size)) = self.uninstall_ask.clone() else { return };
        let custom = crate::custom::is_custom(appid);
        let steam_running = !custom && self.steam_running();
        // With Steam open an installed game can be uninstalled; an unfinished one Steam may be downloading itself.
        // Own unfinished one (started in Vaporust) - Steam does not download it, it can be removed.
        let ready = self.games.iter().find(|g| g.appid == appid).is_some_and(|g| g.ready) || self.settings.holding.contains(&appid);
        // Only this game itself blocks it (another running one does not) and Steam closing.
        let this_running = local::RUNNING.lock().unwrap().as_ref().is_some_and(|r| r.0 == appid);
        let busy = this_running || (!custom && self.closing.load(Ordering::Relaxed));
        let downloading = net.install.as_ref().is_some_and(|i| i.active() && i.appid == appid);
        let neighbours = self.neighbours(appid);
        let mut close = false;
        let mut go = false;
        egui::Window::new(if custom { "Сторонняя игра" } else { "Удаление игры" })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                let title = if custom { format!("Убрать «{name}» из библиотеки?") } else { format!("Удалить «{name}»?") };
                ui.label(egui::RichText::new(title).size(16.0).strong());
                ui.add_space(4.0);
                if custom {
                    ui.label("Игра пропадет из библиотеки Vaporust. Файлы игры на диске не удаляются.");
                } else if !neighbours.is_empty() {
                    let names: Vec<&str> = neighbours.iter().map(|(_, n)| n.as_str()).collect();
                    ui.label(format!(
                        "Эта игра лежит в одной папке с другими: {}. Удалятся только файлы самой «{name}»{}, \n                         остальные игры не пострадают. Нужна связь со Steam: он подскажет, какие файлы ее.",
                        names.join(", "),
                        if size > 0 { format!(" ({})", crate::net::fmt_bytes(size)) } else { String::new() }
                    ));
                } else {
                    ui.label(format!(
                    "С диска будет удалена папка игры ({}). Игра останется на аккаунте, ее можно \
                     установить снова. Сохранения в облаке Steam не пострадают.",
                    crate::net::fmt_bytes(size)
                ));
                }
                let blocked = if steam_running && !ready {
                    Some("Игра не докачана, и открытый Steam может качать ее сам: сначала закройте Steam.")
                } else if busy {
                    Some("Сначала дождитесь закрытия игры.")
                } else if downloading {
                    Some("Игра сейчас качается: сначала нажмите «Пауза» или «Отмена» в «Загрузках».")
                } else {
                    None
                };
                if let Some(b) = blocked {
                    ui.add_space(4.0);
                    ui.colored_label(egui::Color32::from_rgb(230, 170, 90), b);
                }
                if steam_running && ready && blocked.is_none() {
                    ui.weak("Steam открыт: он заметит удаление после перезапуска.");
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let red = egui::Button::new(egui::RichText::new(if custom { "Убрать" } else { "Удалить" }).color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(170, 50, 50));
                    if ui.add_enabled(blocked.is_none(), red).clicked() {
                        go = true;
                    }
                    if ui.button("Отмена").clicked() {
                        close = true;
                    }
                });
            });
        if go && custom {
            self.settings.custom_games.retain(|g| g.id != appid);
            if self.lib_page == Some(appid) {
                self.lib_page = None;
            }
            close = true;
        } else if go {
            self.uninstall_game(appid, name);
            close = true;
        }
        if close {
            self.uninstall_ask = None;
        }
    }

    pub(crate) fn uninstall_game(&mut self, appid: u32, name: String) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        // No longer "our" download: remove from the queue, from the hidden ones and from the interrupted one.
        self.settings.downloads.retain(|q| q.appid != appid);
        self.settings.holding.retain(|a| *a != appid);
        self.settings.staged.remove(&appid);
        if self.settings.current_download.as_ref().is_some_and(|q| q.appid == appid) {
            self.settings.current_download = None;
        }
        // Several games share the folder (Counter-Strike, Half-Life, Blue Shift... - common\Half-Life):
        // delete only this game's files, otherwise the neighbors disappear too.
        let others: Vec<u32> = self.neighbours(appid).into_iter().map(|(a, _)| a).collect();
        if !others.is_empty() {
            let _ = self.tx.send(Cmd::UninstallShared { appid, name, dir: game.dir.clone(), others });
            return;
        }
        if game.ready && !game.stashed {
            let _ = self.tx.send(Cmd::Uninstall { appid, name });
            return;
        }
        *self.launch_status.lock().unwrap() = format!("Удаляю «{name}»...");
        let (status, rescan) = (self.launch_status.clone(), self.rescan_needed.clone());
        std::thread::spawn(move || {
            let text = match local::uninstall_files(&game) {
                Ok(()) => format!("«{name}» удалена."),
                Err(e) => {
                    crate::errlog::write("удаление", &format!("{name}: {e}"));
                    format!("Не удалось удалить «{name}»: {e}")
                }
            };
            *status.lock().unwrap() = text;
            rescan.store(true, Ordering::Relaxed);
        });
    }

    /// Other installed games in the same folder (id, name).
    pub(crate) fn neighbours(&self, appid: u32) -> Vec<(u32, String)> {
        let Some(g) = self.games.iter().find(|g| g.appid == appid) else { return Vec::new() };
        let dir = g.dir.to_string_lossy().to_lowercase();
        self.games
            .iter()
            .filter(|o| o.appid != appid && o.dir.to_string_lossy().to_lowercase() == dir)
            .map(|o| (o.appid, o.name.clone()))
            .collect()
    }
}

impl LauncherApp {
    /// "Install the game?" - after a double click on a non-installed game (as in Steam).
    pub(crate) fn install_ask_dialog(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some((appid, name)) = self.install_ask.clone() else { return };
        let mut close = false;
        let mut go = false;
        let lib = self.libraries.get(self.lib_idx).cloned();
        egui::Window::new("Установка игры")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.label(egui::RichText::new(format!("Установить «{name}»?")).size(16.0).strong());
                ui.label("Игра не установлена. Vaporust скачает ее в библиотеку:");
                if let Some(l) = &lib {
                    let free = local::free_space(l).map(crate::net::fmt_bytes).unwrap_or_default();
                    ui.weak(format!("{}{}", l.display(), if free.is_empty() { String::new() } else { format!(" (свободно {free})") }));
                }
                if net.install.as_ref().is_some_and(|i| i.active()) {
                    ui.weak("Сейчас качается другая игра - эта встанет в очередь.");
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Установить").color(egui::Color32::WHITE)).fill(crate::theme::ACCENT)).clicked() {
                        go = true;
                    }
                    if ui.button("Отмена").clicked() {
                        close = true;
                    }
                });
            });
        if go {
            self.install_ask = None;
            self.enqueue(appid, name, net);
        } else if close {
            self.install_ask = None;
        }
    }
}
