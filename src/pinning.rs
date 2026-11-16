//! Pinning a game version and rollback.
//!
//! Games are always launched by the real Steam, and it updates a game if the depot versions in
//! its appmanifest (InstalledDepots) differ from the latest. So for a pinned
//! game Vaporust keeps the REAL versions itself (Settings.pinned), and writes the latest
//! versions and build number into appmanifest - Steam considers the game up to date and
//! downloads nothing. This is repeated before every launch (in case an update came out).
//! Unpinning restores the real versions, and Steam updates the game as usual.

use crate::net::{Cmd, NetState};
use crate::{local, LauncherApp};
use eframe::egui;
use std::sync::atomic::Ordering;

/// Game branch for looking up the latest versions: the chosen beta or public.
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

    /// Before "Play": for a pinned game first refresh the "masking" in appmanifest.
    /// true - can launch right away; false - the launch starts when the masking is ready.
    pub(crate) fn pin_gate(&mut self, appid: u32, net: &NetState) -> bool {
        if !self.settings.pinned.contains_key(&appid) || self.pin_ok.remove(&appid) {
            return true;
        }
        let msg = if net.account.is_none() {
            tr!("Версия игры закреплена: войдите в аккаунт, иначе Steam может ее обновить. Запуск отменен.", "The game version is pinned: log in, otherwise Steam may update it. Launch cancelled.")
        } else if self.pin_refresh(appid, true) {
            tr!("Проверяю закрепленную версию игры...", "Checking the pinned game version...")
        } else {
            tr!("Не удалось подготовить закрепленную версию.", "Could not prepare the pinned version.")
        };
        *self.launch_status.lock().unwrap() = msg.into();
        false
    }

    /// Pin the current version of an installed game.
    pub(crate) fn pin_game(&mut self, appid: u32) -> Result<String, String> {
        let game = self.games.iter().find(|g| g.appid == appid).ok_or(tr!("Игра не установлена.", "The game is not installed."))?;
        let real = local::installed_depots(&local::appmanifest_path(game));
        if real.is_empty() {
            return Err(tr!("В файле игры нет списка хранилищ - закрепить нельзя.", "The game file has no depot list - it cannot be pinned.").into());
        }
        self.settings.pinned.insert(appid, real);
        self.pin_refresh(appid, false);
        Ok(tr!("Версия закреплена: Steam не будет обновлять эту игру.", "Version pinned: Steam will not update this game.").into())
    }

    /// Unpin: restore the real versions in appmanifest. If the game is behind the latest
    /// version, mark "update needed" - Steam updates it as usual.
    pub(crate) fn unpin_game(&mut self, appid: u32) -> Result<String, String> {
        let game = self.games.iter().find(|g| g.appid == appid).ok_or(tr!("Игра не установлена.", "The game is not installed."))?;
        let path = local::appmanifest_path(game);
        let Some(real) = self.settings.pinned.get(&appid).cloned() else { return Ok(String::new()) };
        let outdated = local::installed_depots(&path) != real;
        local::write_depot_versions(&path, &real, None, if outdated { 6 } else { 4 })?;
        self.settings.pinned.remove(&appid);
        self.rescan();
        Ok(if outdated {
            tr!("Версия откреплена. Игра отстала от последней версии - нажмите «Обновить».", "Version unpinned. The game is behind the latest version - press \"Update\".").into()
        } else {
            tr!("Версия откреплена.", "Version unpinned.").into()
        })
    }

    /// Once per frame: PinRefresh results, pinning after a rollback, outdated masking.
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
                        trf!("{e} Запуск отменен, чтобы Steam не обновил закрепленную игру.", "{e} Launch cancelled so that Steam does not update the pinned game.")
                    } else {
                        e
                    };
                }
            }
        }
        // The rollback is downloaded: pin what is actually installed now.
        if let Some(appid) = after {
            self.rescan();
            let _ = self.pin_game(appid);
        }
        // Pinned games are not shown as "update needed". If Steam already sees a new
        // version for them (the masking is outdated) - refresh it while Steam is closed.
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
    /// Live test (read only): latest Half-Life depot versions versus the game file.
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
    /// Live test (read only): Half-Life depot names.
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
    /// Live test (read only): private branches of installed games.
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
