//! «Вернуть Steam как было»: снять все изменения, которые Vaporust держит в файлах Steam, чтобы
//! обычный Steam работал так, будто Vaporust не было (например, перед удалением программы).
//! - спрятанные appmanifest недокачанных игр (.acf.vaporust-paused) возвращаются на место -
//!   Steam увидит эти игры и докачает их сам;
//! - закрепленные версии открепляются (в appmanifest снова настоящие версии);
//! - снимается флаг автономного режима в loginusers.vdf, если его ставил Vaporust;
//! - временные папки загрузок VaporustStaging (недокачанное) уходят в Корзину.
//! Параметры запуска, язык и бета игр, коллекции и сторонние игры в Steam остаются: это
//! настройки самого пользователя.

use crate::{local, LauncherApp, NetState};
use std::path::PathBuf;

/// Что будет сделано (для окна подтверждения).
#[derive(Default, Debug)]
pub struct Plan {
    /// (библиотека, appid) спрятанных appmanifest.
    pub stashed: Vec<(PathBuf, u32)>,
    pub pinned: Vec<u32>,
    pub offline_flag: bool,
    /// Папки VaporustStaging с размером.
    pub staging: Vec<(PathBuf, u64)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.stashed.is_empty() && self.pinned.is_empty() && !self.offline_flag && self.staging.is_empty()
    }
}

/// Спрятанные appmanifest в библиотеке: appid из имени appmanifest_<appid>.acf.vaporust-paused.
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

    /// Выполнить. Только при закрытом Steam и без идущей загрузки. Возвращает отчет по строкам.
    pub(crate) fn restore_steam(&mut self, net: &NetState) -> Result<Vec<String>, String> {
        if self.steam_running() {
            return Err("Сначала закройте Steam.".into());
        }
        if net.install.as_ref().is_some_and(|i| i.active()) {
            return Err("Сейчас идет загрузка или операция с файлами: дождитесь конца или поставьте на паузу.".into());
        }
        let steam = self.steam.clone().ok_or("Steam не найден.")?;
        let plan = self.restore_plan();
        let mut report = Vec::new();
        let mut errors = Vec::new();

        // 1. Спрятанные appmanifest. Сначала убираем игры из «своих», иначе Vaporust спрячет их снова.
        for (lib, id) in &plan.stashed {
            self.settings.holding.retain(|a| a != id);
            self.settings.downloads.retain(|q| q.appid != *id);
            if self.settings.current_download.as_ref().is_some_and(|q| q.appid == *id) {
                self.settings.current_download = None;
            }
            if local::unstash_manifest(lib, *id) {
                report.push(format!("Игра {id}: файл игры возвращен Steam, он докачает ее сам."));
            } else {
                errors.push(format!("Игра {id}: не удалось вернуть файл игры (возможно, рядом уже есть appmanifest_{id}.acf)."));
            }
        }
        self.settings.holding.clear();

        // 2. Закрепленные версии.
        for id in &plan.pinned {
            if !self.games.iter().any(|g| g.appid == *id) {
                self.settings.pinned.remove(id);
                continue;
            }
            match self.unpin_game(*id) {
                Ok(_) => report.push(format!("{}: версия откреплена.", self.restore_game_name(*id))),
                Err(e) => errors.push(format!("{}: открепить не удалось: {e}", self.restore_game_name(*id))),
            }
        }

        // 3. Флаг автономного режима Steam.
        if plan.offline_flag {
            match local::set_steam_offline(&steam.dir, false) {
                Ok(_) => {
                    self.settings.steam_offline_set = false;
                    report.push("Автономный режим Steam снят.".into());
                }
                Err(e) => errors.push(format!("Автономный режим Steam снять не удалось: {e}")),
            }
        }

        // 4. Временные папки загрузок - в Корзину (можно восстановить).
        for (dir, _) in &plan.staging {
            match crate::storage::to_recycle_bin(&[dir.as_path()]) {
                Ok(()) => report.push(format!("Папка {} перемещена в Корзину.", local::pretty_path(dir))),
                Err(e) => errors.push(format!("Папку {} убрать не удалось: {e}", local::pretty_path(dir))),
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
            report.push("Менять нечего: Vaporust сейчас ничего не держит в файлах Steam.".into());
        }
        if !errors.is_empty() {
            crate::errlog::write("вернуть Steam", &errors.join("; "));
            report.extend(errors.into_iter().map(|e| format!("Ошибка. {e}")));
        }
        Ok(report)
    }

    fn restore_game_name(&self, appid: u32) -> String {
        self.games.iter().find(|g| g.appid == appid).map(|g| g.name.clone()).unwrap_or_else(|| format!("Игра {appid}"))
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
    /// Окно подтверждения открыто; итог последнего запуска.
    static UI: std::cell::RefCell<(Option<Plan>, Option<Result<Vec<String>, String>>)> = Default::default();
}

impl LauncherApp {
    /// Строка «Steam» в «Настройки -> О программе».
    pub(crate) fn restore_row(&mut self, ui: &mut eframe::egui::Ui, net: &NetState) {
        use eframe::egui;
        let (mut plan, mut result) = UI.with(|u| std::mem::take(&mut *u.borrow_mut()));
        crate::settingsui::row(ui, "Steam", |ui| {
            if ui
                .button("Вернуть Steam как было...")
                .on_hover_text("Убрать все изменения Vaporust в файлах Steam (например, перед удалением Vaporust)")
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
                    ui.label("Менять нечего: Vaporust сейчас ничего не держит в файлах Steam.");
                } else {
                    ui.label("Будет сделано:");
                    if !p.stashed.is_empty() {
                        ui.label(format!(
                            "- недокачанные игры ({}) снова станут видны Steam, и он докачает их сам; очередь Vaporust для них очистится",
                            p.stashed.len()
                        ));
                    }
                    if !p.pinned.is_empty() {
                        ui.label(format!("- закрепленные версии ({}) открепятся, Steam сможет обновлять эти игры", p.pinned.len()));
                    }
                    if p.offline_flag {
                        ui.label("- снимется автономный режим Steam, который ставил Vaporust");
                    }
                    for (d, size) in &p.staging {
                        ui.label(format!(
                            "- временная папка загрузок {} ({}) уйдет в Корзину",
                            local::pretty_path(d),
                            crate::net::fmt_bytes(*size)
                        ));
                    }
                }
                ui.weak("Не меняются: параметры запуска, язык и бета игр, коллекции, сторонние игры в Steam. Steam должен быть закрыт.");
                ui.horizontal(|ui| {
                    if !p.is_empty() && ui.button("Вернуть").clicked() {
                        result = Some(self.restore_steam(net));
                    }
                    if ui.button("Закрыть").clicked() {
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
                if ui.small_button("Скрыть").clicked() {
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
