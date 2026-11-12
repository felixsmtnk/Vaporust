//! Закрепление версии игры и откат.
//!
//! Игры всегда запускает настоящий Steam, а он обновляет игру, если версии хранилищ в
//! ее appmanifest (InstalledDepots) отличаются от последних. Поэтому для закрепленной
//! игры Vaporust хранит НАСТОЯЩИЕ версии у себя (Settings.pinned), а в appmanifest
//! записывает последние версии и номер сборки - Steam считает игру обновленной и
//! ничего не качает. Это повторяется перед каждым запуском (вдруг вышло обновление).
//! Открепление возвращает настоящие версии, и Steam обновляет игру как обычно.

use crate::net::{Cmd, NetState};
use crate::{local, LauncherApp};
use eframe::egui;
use std::sync::atomic::Ordering;

/// Ветка игры для поиска последних версий: выбранная бета или public.
pub fn branch_of(acf: &local::AcfInfo) -> String {
    let b = if !acf.mounted_beta.is_empty() { &acf.mounted_beta } else { &acf.user_beta };
    if b.is_empty() { "public".into() } else { b.clone() }
}

impl LauncherApp {
    fn pin_refresh(&self, appid: u32, launch: bool) -> bool {
        let Some(game) = self.games.iter().find(|g| g.appid == appid) else { return false };
        let path = local::appmanifest_path(game);
        let branch = branch_of(&local::read_acf(&path));
        self.tx.send(Cmd::PinRefresh { appid, path, branch, launch }).is_ok()
    }

    /// Перед «Играть»: у закрепленной игры сначала обновить «маскировку» в appmanifest.
    /// true - можно запускать сразу; false - запуск начнется, когда маскировка готова.
    pub(crate) fn pin_gate(&mut self, appid: u32, net: &NetState) -> bool {
        if !self.settings.pinned.contains_key(&appid) || self.pin_ok.remove(&appid) {
            return true;
        }
        let msg = if net.account.is_none() {
            "Версия игры закреплена: войдите в аккаунт, иначе Steam может ее обновить. Запуск отменен."
        } else if self.pin_refresh(appid, true) {
            "Проверяю закрепленную версию игры..."
        } else {
            "Не удалось подготовить закрепленную версию."
        };
        *self.launch_status.lock().unwrap() = msg.into();
        false
    }

    /// Закрепить текущую версию установленной игры.
    pub(crate) fn pin_game(&mut self, appid: u32) -> Result<String, String> {
        let game = self.games.iter().find(|g| g.appid == appid).ok_or("Игра не установлена.")?;
        let real = local::installed_depots(&local::appmanifest_path(game));
        if real.is_empty() {
            return Err("В файле игры нет списка хранилищ - закрепить нельзя.".into());
        }
        self.settings.pinned.insert(appid, real);
        self.pin_refresh(appid, false);
        Ok("Версия закреплена: Steam не будет обновлять эту игру.".into())
    }

    /// Открепить: вернуть в appmanifest настоящие версии. Если игра отстала от последней
    /// версии, помечаем «нужно обновление» - Steam обновит ее как обычно.
    pub(crate) fn unpin_game(&mut self, appid: u32) -> Result<String, String> {
        let game = self.games.iter().find(|g| g.appid == appid).ok_or("Игра не установлена.")?;
        let path = local::appmanifest_path(game);
        let Some(real) = self.settings.pinned.get(&appid).cloned() else { return Ok(String::new()) };
        let outdated = local::installed_depots(&path) != real;
        local::write_depot_versions(&path, &real, None, if outdated { 6 } else { 4 })?;
        self.settings.pinned.remove(&appid);
        self.rescan();
        Ok(if outdated {
            "Версия откреплена. Игра отстала от последней версии - нажмите «Обновить».".into()
        } else {
            "Версия откреплена.".into()
        })
    }

    /// Раз в кадр: итоги PinRefresh, закрепление после отката, устаревшая маскировка.
    pub(crate) fn pin_tick(&mut self, ctx: &egui::Context, net: &mut NetState) {
        let (done, after) = {
            let mut s = self.net.lock().unwrap();
            (s.pin_done.take(), s.pin_after.take())
        };
        if let Some(d) = done {
            match d.result {
                Ok(()) if d.launch => {
                    self.pin_ok.insert(d.appid);
                    self.start(d.appid, ctx);
                }
                Ok(()) => {}
                Err(e) => {
                    *self.launch_status.lock().unwrap() = if d.launch {
                        format!("{e} Запуск отменен, чтобы Steam не обновил закрепленную игру.")
                    } else {
                        e
                    };
                }
            }
        }
        // Откат скачан: закрепляем то, что теперь реально установлено.
        if let Some(appid) = after {
            self.rescan();
            let _ = self.pin_game(appid);
        }
        // Закрепленные игры не показываем как «нужно обновление». Если Steam уже видит у
        // них новую версию (маскировка устарела) - обновляем ее, пока Steam закрыт.
        if let Some(u) = net.updates.as_mut() {
            let stale: Vec<u32> = u.iter().copied().filter(|a| self.settings.pinned.contains_key(a)).collect();
            u.retain(|a| !self.settings.pinned.contains_key(a));
            let idle = !self.launching.load(Ordering::Relaxed) && !self.steam_running();
            for appid in stale {
                if idle && self.pin_stale_sent.insert(appid) {
                    self.pin_refresh(appid, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Живой тест (только чтение): последние версии хранилищ Half-Life против файла игры.
    #[test]
    #[ignore]
    fn latest_versions_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (latest, buildid) = c.latest_depot_versions(70, "public").await.unwrap();
            println!("buildid {buildid:?}");
            let path = std::path::Path::new(r"C:\Program Files (x86)\Steam\steamapps\appmanifest_70.acf");
            for (d, m) in crate::local::installed_depots(path) {
                println!("depot {d}: installed {m}, latest {:?}", latest.get(&d));
            }
        });
    }
}

#[cfg(test)]
mod names_test {
    /// Живой тест (только чтение): названия хранилищ Half-Life.
    #[test]
    #[ignore]
    fn depot_names_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let d = c.depot_descriptions(70).await.unwrap();
            let mut v: Vec<_> = d.into_iter().collect();
            v.sort_by_key(|x| x.0);
            for (id, info) in v {
                println!("depot {id} {info:?}");
            }
        });
    }
}

#[cfg(test)]
mod branches_test {
    /// Живой тест (только чтение): закрытые ветки установленных игр.
    #[test]
    #[ignore]
    fn private_branches_live() {
        let ids = [10u32, 100, 1002300, 105600, 1294760, 130, 1427890, 1451940, 1540210, 206440, 223710, 2280, 250820, 282800, 338300, 354240, 3590, 3620, 368570, 38130, 388080, 438100, 50, 550, 555160, 597170, 620980, 629520, 70, 702050, 823500];
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for id in ids {
                if let Ok(b) = c.branches_detailed(id).await {
                    let p: Vec<_> = b.iter().map(|x| format!("{} ({:?})", x.0, x.1)).collect();
                    if !p.is_empty() {
                        println!("app {id}: {}", p.join(", "));
                    }
                }
            }
        });
    }
}
