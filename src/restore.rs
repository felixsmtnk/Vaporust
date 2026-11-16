//! "Remove Vaporust changes from Steam": undo all changes Vaporust keeps in Steam files so that
//! the normal Steam works as if Vaporust had never been there (for example before uninstalling it).
//! - hidden appmanifests of unfinished games (.acf.vaporust-paused) are put back -
//!   Steam sees these games and finishes downloading them itself;
//! - pinned versions are unpinned (appmanifest gets the real versions again);
//! - the offline mode flag in loginusers.vdf is cleared if Vaporust set it;
//! - VaporustStaging temporary download folders (unfinished data) go to the Recycle Bin.
//! Launch options, game language and beta, collections and non-Steam games in Steam stay: these are
//! the user's own settings.

use crate::{local, LauncherApp, NetState};
use std::path::PathBuf;

/// What will be done (for the confirmation window).
#[derive(Default, Debug)]
pub struct Plan {
    /// (library, appid) of hidden appmanifests.
    pub stashed: Vec<(PathBuf, u32)>,
    pub pinned: Vec<u32>,
    pub offline_flag: bool,
    /// VaporustStaging folders with their size.
    pub staging: Vec<(PathBuf, u64)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.stashed.is_empty() && self.pinned.is_empty() && !self.offline_flag && self.staging.is_empty()
    }
}

/// Hidden appmanifests in a library: appid from the name appmanifest_<appid>.acf.vaporust-paused.
pub fn stashed_in(library: &std::path::Path) -> Vec<u32> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(library.join("steamapps")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_prefix("appmanifest_").and_then(|s| s.strip_suffix(local::STASH_EXT)) {
                if let Ok(id) = id.parse::<u32>() {
                    out.push(id);
                }
            }
        }
    }
    out.sort_unstable();
    out
}

impl LauncherApp {
    pub(crate) fn restore_plan(&self) -> Plan {
        let mut p = Plan::default();
        let Some(steam) = self.steam.as_ref() else { return p };
        for lib in local::library_dirs(&steam.dir) {
            for id in stashed_in(&lib) {
                p.stashed.push((lib.clone(), id));
            }
            let st = local::staging_dir(&lib);
            if st.is_dir() {
                p.staging.push((st.clone(), crate::storage::dir_size(&st)));
            }
        }
        p.pinned = self.settings.pinned.keys().copied().collect();
        p.offline_flag = self.settings.steam_offline_set;
        p
    }

    /// Run it. Only while Steam is closed and no download is running. Returns a report by lines.
    pub(crate) fn restore_steam(&mut self, net: &NetState) -> Result<Vec<String>, String> {
        if self.steam_running() {
            return Err(tr!("Сначала закройте Steam.", "Close Steam first.").into());
        }
        if net.install.as_ref().is_some_and(|i| i.active()) {
            return Err(tr!("Сейчас идет загрузка или операция с файлами: дождитесь конца или поставьте на паузу.", "A download or a file operation is running: wait for it to finish or pause it.").into());
        }
        let steam = self.steam.clone().ok_or(tr!("Steam не найден.", "Steam not found."))?;
        let plan = self.restore_plan();
        let mut report = Vec::new();
        let mut errors = Vec::new();

        // 1. Hidden appmanifests. First remove the games from "ours", otherwise Vaporust hides them again.
        for (lib, id) in &plan.stashed {
            self.settings.holding.retain(|a| a != id);
            self.settings.downloads.retain(|q| q.appid != *id);
            if self.settings.current_download.as_ref().is_some_and(|q| q.appid == *id) {
                self.settings.current_download = None;
            }
            if local::unstash_manifest(lib, *id) {
                report.push(trf!("Игра {id}: файл игры возвращен Steam, он докачает ее сам.", "Game {id}: the game file is returned to Steam, it finishes the download itself."));
            } else {
                errors.push(trf!("Игра {id}: не удалось вернуть файл игры (возможно, рядом уже есть appmanifest_{id}.acf).", "Game {id}: could not return the game file (maybe appmanifest_{id}.acf is already there)."));
            }
        }
        self.settings.holding.clear();

        // 2. Pinned versions.
        for id in &plan.pinned {
            if !self.games.iter().any(|g| g.appid == *id) {
                self.settings.pinned.remove(id);
                continue;
            }
            match self.unpin_game(*id) {
                Ok(_) => report.push(trf!("{}: версия откреплена.", "{}: version unpinned.", self.restore_game_name(*id))),
                Err(e) => errors.push(trf!("{}: открепить не удалось: {e}", "{}: could not unpin: {e}", self.restore_game_name(*id))),
            }
        }

        // 3. Steam offline mode flag.
        if plan.offline_flag {
            match local::set_steam_offline(&steam.dir, false) {
                Ok(_) => {
                    self.settings.steam_offline_set = false;
                    report.push(tr!("Автономный режим Steam снят.", "Steam offline mode removed.").into());
                }
                Err(e) => errors.push(trf!("Автономный режим Steam снять не удалось: {e}", "Could not remove Steam offline mode: {e}")),
            }
        }

        // 4. Temporary download folders - to the Recycle Bin (recoverable).
        for (dir, _) in &plan.staging {
            match crate::storage::to_recycle_bin(&[dir.as_path()]) {
                Ok(()) => report.push(trf!("Папка {} перемещена в Корзину.", "Folder {} moved to the Recycle Bin.", local::pretty_path(dir))),
                Err(e) => errors.push(trf!("Папку {} убрать не удалось: {e}", "Could not remove folder {}: {e}", local::pretty_path(dir))),
            }
        }
        if !plan.staging.is_empty() {
            for (appid, _) in std::mem::take(&mut self.settings.staged) {
                self.settings.downloads.retain(|q| q.appid != appid);
                if self.settings.current_download.as_ref().is_some_and(|q| q.appid == appid) {
                    self.settings.current_download = None;
                }
            }
        }

        self.rescan();
        if report.is_empty() && errors.is_empty() {
            report.push(tr!("Менять нечего: Vaporust сейчас ничего не держит в файлах Steam.", "Nothing to change: Vaporust keeps nothing in Steam files right now.").into());
        }
        if !errors.is_empty() {
            crate::errlog::write(tr!("вернуть Steam", "restore Steam"), &errors.join("; "));
            report.extend(errors.into_iter().map(|e| trf!("Ошибка. {e}", "Error. {e}")));
        }
        Ok(report)
    }

    fn restore_game_name(&self, appid: u32) -> String {
        self.games.iter().find(|g| g.appid == appid).map(|g| g.name.clone()).unwrap_or_else(|| trf!("Игра {appid}", "Game {appid}"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_stashed() {
        let dir = std::env::temp_dir().join(format!("vaporust_restore_{}", std::process::id()));
        let apps = dir.join("steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("appmanifest_70.acf"), "x").unwrap();
        std::fs::write(apps.join(format!("appmanifest_620{}", crate::local::STASH_EXT)), "x").unwrap();
        std::fs::write(apps.join(format!("appmanifest_abc{}", crate::local::STASH_EXT)), "x").unwrap();
        assert_eq!(super::stashed_in(&dir), vec![620]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

thread_local! {
    /// The confirmation window is open; result of the last run.
    static UI: std::cell::RefCell<(Option<Plan>, Option<Result<Vec<String>, String>>)> = Default::default();
}

impl LauncherApp {
    /// The "Steam" row in "Settings -> About".
    pub(crate) fn restore_row(&mut self, ui: &mut eframe::egui::Ui, net: &NetState) {
        use eframe::egui;
        let (mut plan, mut result) = UI.with(|u| std::mem::take(&mut *u.borrow_mut()));
        crate::settingsui::row(ui, "Steam", |ui| {
            if ui
                .button(tr!("Убрать изменения Vaporust в Steam...", "Remove Vaporust changes in Steam..."))
                .on_hover_text(tr!("Убрать все изменения Vaporust в файлах Steam (например, перед удалением Vaporust)", "Remove all Vaporust changes in Steam files (for example before uninstalling Vaporust)"))
                .clicked()
            {
                plan = Some(self.restore_plan());
                result = None;
            }
        });
        let mut close = false;
        if let Some(p) = &plan {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                if p.is_empty() {
                    ui.label(tr!("Менять нечего: Vaporust сейчас ничего не держит в файлах Steam.", "Nothing to change: Vaporust keeps nothing in Steam files right now."));
                } else {
                    ui.label(tr!("Будет сделано:", "This will be done:"));
                    if !p.stashed.is_empty() {
                        ui.label(trf!(
                            "- недокачанные игры ({}) снова станут видны Steam, и он докачает их сам; очередь Vaporust для них очистится", "- unfinished games ({}) become visible to Steam again, and it finishes them itself; the Vaporust queue for them is cleared",
                            p.stashed.len()
                        ));
                    }
                    if !p.pinned.is_empty() {
                        ui.label(trf!("- закрепленные версии ({}) открепятся, Steam сможет обновлять эти игры", "- pinned versions ({}) are unpinned, Steam can update these games", p.pinned.len()));
                    }
                    if p.offline_flag {
                        ui.label(tr!("- снимется автономный режим Steam, который ставил Vaporust", "- the Steam offline mode set by Vaporust is removed"));
                    }
                    for (d, size) in &p.staging {
                        ui.label(trf!(
                            "- временная папка загрузок {} ({}) уйдет в Корзину", "- the temporary download folder {} ({}) goes to the Recycle Bin",
                            local::pretty_path(d),
                            crate::net::fmt_bytes(*size)
                        ));
                    }
                }
                ui.weak(tr!("Не меняются: параметры запуска, язык и бета игр, коллекции, сторонние игры в Steam. Steam должен быть закрыт.", "Not changed: launch options, game language and beta, collections, non-Steam games in Steam. Steam must be closed."));
                ui.horizontal(|ui| {
                    if !p.is_empty() && ui.button(tr!("Вернуть", "Restore")).clicked() {
                        result = Some(self.restore_steam(net));
                    }
                    if ui.button(tr!("Закрыть", "Close")).clicked() {
                        close = true;
                    }
                });
            });
        }
        if result.is_some() || close {
            plan = None;
        }
        let mut hide = false;
        if let Some(r) = &result {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                match r {
                    Ok(lines) => {
                        for l in lines {
                            ui.label(l);
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::from_rgb(230, 190, 90), e);
                    }
                }
                if ui.small_button(tr!("Скрыть", "Hide")).clicked() {
                    hide = true;
                }
            });
        }
        if hide {
            result = None;
        }
        UI.with(|u| *u.borrow_mut() = (plan, result));
    }
}
