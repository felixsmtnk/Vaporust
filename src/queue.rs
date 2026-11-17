//! The download queue: starting an installation, the queue, finishing, unfinished downloads.

use crate::*;

impl LauncherApp {
    /// An unfinished game started in Vaporust (Steam does not consider it its own): its library.
    pub(crate) fn movable_partial(&self, appid: u32) -> Option<std::path::PathBuf> {
        if !self.settings.holding.contains(&appid) {
            return None;
        }
        self.games
            .iter()
            .find(|g| g.appid == appid && !g.ready)
            .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
    }

    pub(crate) fn install(&mut self, appid: u32, name: String) {
        self.install_note.clear();
        let steam_open = local::steam_running();
        // Started in the temporary library (while Steam was open) - continue there.
        if let Some(real) = self.settings.staged.get(&appid).cloned() {
            if !self.settings.holding.contains(&appid) {
                self.settings.holding.push(appid);
            }
            self.settings.current_download = Some(settings::QueueItem { appid, name: name.clone(), paused: false });
            let staging = local::staging_dir(std::path::Path::new(&real));
            let _ = std::fs::create_dir_all(staging.join("steamapps").join("common"));
            let _ = self.tx.send(Cmd::Install { appid, name, library: staging });
            return;
        }
        // Steam is open: an unfinished game in the real library must not be touched (Steam could
        // start downloading it itself), and a new one goes to the temporary library Steam does not see.
        let partial_real = self.games.iter().any(|g| g.appid == appid);
        // Own unfinished one (started in Vaporust) - move it to the temporary folder and finish it there.
        if steam_open && self.settings.download_with_steam && partial_real {
            if let Some(lib) = self.movable_partial(appid) {
                match local::to_staging(&lib, appid) {
                    Ok(()) => {
                        self.settings.staged.insert(appid, lib.to_string_lossy().into_owned());
                        self.rescan();
                        return self.install(appid, name);
                    }
                    Err(e) => {
                        self.install_note = trf!("Не удалось продолжить при открытом Steam: {e}. Закройте Steam.", "Could not continue with Steam open: {e}. Close Steam.");
                        return;
                    }
                }
            }
        }
        if steam_open && (!self.settings.download_with_steam || partial_real) {
            self.install_note = tr!("Сначала закройте Steam (и игру, если она запущена), потом нажмите «Установить» еще раз.", "Close Steam first (and the game if it is running), then press \"Install\" again.").into();
            return;
        }
        // An unfinished game is resumed in the same library where it lies.
        let library = self
            .games
            .iter()
            .find(|g| g.appid == appid)
            .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
            .or_else(|| self.libraries.get(self.lib_idx).cloned());
        let Some(library) = library else {
            self.install_note = tr!("Не найдена папка библиотеки Steam.", "Steam library folder not found.").into();
            return;
        };
        // While the game is unfinished it is "ours": when paused, its appmanifest is hidden from Steam.
        if !self.settings.holding.contains(&appid) {
            self.settings.holding.push(appid);
        }
        // If Vaporust is closed mid-download, it resumes on the next run.
        self.settings.current_download = Some(settings::QueueItem { appid, name: name.clone(), paused: false });
        let library = if steam_open {
            self.settings.staged.insert(appid, library.to_string_lossy().into_owned());
            let staging = local::staging_dir(&library);
            let _ = std::fs::create_dir_all(staging.join("steamapps").join("common"));
            staging
        } else {
            library
        };
        let _ = self.tx.send(Cmd::Install {
            appid,
            name,
            library,
        });
    }

    /// Hide unfinished Vaporust downloads from Steam (paused, cancelled, interrupted),
    /// otherwise the Steam started for a game would begin downloading them itself. Checked every 3 s.
    pub(crate) fn guard_partials(&mut self, net: &NetState) {
        // Finished ones are no longer "ours", Steam takes care of them from now on.
        self.settings.holding.retain(|a| !self.games.iter().any(|g| g.appid == *a && g.ready));
        if self.settings.holding.is_empty()
            || self.stash_check.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3))
        {
            return;
        }
        self.stash_check = Some(std::time::Instant::now());
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) || self.steam_running() {
            return; // with Steam open, the files must not be touched
        }
        let active = net.install.as_ref().filter(|i| i.active()).map(|i| i.appid);
        let mut changed = false;
        for &appid in &self.settings.holding {
            if Some(appid) == active {
                continue;
            }
            let lib = self
                .games
                .iter()
                .find(|g| g.appid == appid && !g.ready && !g.stashed)
                .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()));
            if let Some(lib) = lib {
                changed |= local::stash_manifest(&lib, appid);
            }
        }
        if changed {
            self.rescan();
        }
    }

    /// Put a game into the install queue (or start right away if nothing is downloading).
    pub(crate) fn enqueue(&mut self, appid: u32, name: String, net: &NetState) {
        if let Some(q) = self.settings.downloads.iter_mut().find(|q| q.appid == appid) {
            q.paused = false; // already queued and paused - "Resume" unpauses it
            return;
        }
        // While Steam is open, Vaporust does not download (Steam could start downloading the same game):
        // the game is queued and starts by itself as soon as Steam is closed. Previously clicking
        // "Install" in this case did nothing visible.
        let partial_real = self.games.iter().any(|g| g.appid == appid) && !self.settings.staged.contains_key(&appid) && self.movable_partial(appid).is_none();
        if self.steam_running() && !self.launching.load(Ordering::Relaxed) && (!self.settings.download_with_steam || partial_real) {
            self.settings.downloads.push(settings::QueueItem { appid, name: name.clone(), paused: false });
            popup::push(
                popup::Kind::Game(appid),
                tr!("Загрузка ждет закрытия Steam", "The download waits for Steam to close"),
                trf!("«{name}» в очереди: начнется сама, когда вы закроете Steam (меню «Vaporust» - «Закрыть Steam»).", "\"{name}\" is queued: it starts by itself when you close Steam (\"Vaporust\" menu - \"Close Steam\")."),
            );
            return;
        }
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        let waiting = self.settings.downloads.iter().any(|q| !q.paused);
        let allowed = schedule::may_download(&self.settings);
        if !allowed {
            self.install_note = trf!(
                "«{name}» в очереди: по расписанию загрузки идут с {} до {}.", "\"{name}\" is queued: by schedule, downloads run from {} to {}.",
                schedule::hh(self.settings.sched_from),
                schedule::hh(self.settings.sched_to)
            );
        }
        if !busy && !waiting && !self.settings.queue_paused && allowed {
            self.install(appid, name);
            self.queue_started = Some(std::time::Instant::now());
        } else {
            self.settings.downloads.push(settings::QueueItem { appid, name, paused: false });
        }
    }

    /// The download is over: no longer "current". If the network broke it (not "Cancel" or
    /// "Pause"), it goes first in the queue and resumes when the connection is back (up to 3 times in a row).
    pub(crate) fn finish_tick(&mut self, net: &NetState) {
        let Some(cur) = self.settings.current_download.clone() else { return };
        let Some(inst) = net.install.as_ref().filter(|i| i.appid == cur.appid) else { return };
        let Some(res) = &inst.result else { return };
        self.settings.current_download = None;
        let user_stop = inst.abort.load(Ordering::Relaxed);
        if let (Err(e), false) = (res, user_stop) {
            crate::errlog::write(tr!("загрузка", "download"), &format!("{}: {e}", cur.name));
        }
        match res {
            Err(e) if should_resume(e, user_stop, self.resume_tries) => {
                self.resume_tries += 1;
                if !self.settings.downloads.iter().any(|q| q.appid == cur.appid) {
                    self.settings.downloads.insert(0, cur.clone());
                }
                self.install_note = trf!(
                    "Загрузка «{}» прервалась из-за связи: продолжится сама, когда Steam снова будет доступен.", "The download of \"{}\" was interrupted by the connection: it continues by itself when Steam is available again.",
                    cur.name
                );
                // Not right away: give the network time to come back.
                self.queue_started = Some(std::time::Instant::now() + std::time::Duration::from_secs(27));
            }
            Ok(()) => {
                self.resume_tries = 0;
                if let Some(real) = self.settings.staged.remove(&cur.appid) {
                    match local::unstage(std::path::Path::new(&real), cur.appid) {
                        Ok(()) => {
                            self.rescan();
                        }
                        Err(e) => {
                            crate::errlog::write(tr!("загрузка", "download"), &trf!("перенос из временной папки: {e}", "moving from the temporary folder: {e}"));
                            popup::push(popup::Kind::Info, tr!("Загрузка", "Download"), trf!("«{}» скачана, но не перенесена в библиотеку: {e}", "\"{}\" is downloaded but not moved to the library: {e}", cur.name));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Start the next game from the queue when the downloader is free.
    pub(crate) fn run_queue(&mut self, net: &NetState) {
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        // The install command does not reach the network thread instantly: a 3 s pause between starts.
        let cooling = self.queue_started.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
        // The first game in the queue that is not paused.
        let next = self.settings.downloads.iter().position(|q| !q.paused);
        if busy
            || cooling
            || self.settings.queue_paused
            || !schedule::may_download(&self.settings)
            || next.is_none()
            || net.account.is_none()
            || self.launching.load(Ordering::Relaxed)
        {
            return;
        }
        if self.steam_running() {
            let q = &self.settings.downloads[next.unwrap()];
            let real_partial = self.games.iter().any(|g| g.appid == q.appid) && !self.settings.staged.contains_key(&q.appid) && self.movable_partial(q.appid).is_none();
            if !self.settings.download_with_steam || real_partial {
                return;
            }
        }
        let q = self.settings.downloads.remove(next.unwrap());
        self.install(q.appid, q.name);
        self.queue_started = Some(std::time::Instant::now());
    }

    /// "Already downloaded...": pick the game folder in steamapps\common and add it to Steam.
    pub(crate) fn import_game(&mut self, appid: u32, name: String) {
        if self.steam_running() {
            *self.launch_status.lock().unwrap() = tr!("Закройте Steam, чтобы добавить игру (кнопка «Закрыть Steam» внизу).", "Close Steam to add the game (the \"Close Steam\" button at the bottom).").into();
            return;
        }
        let Some(dir) = rfd::FileDialog::new()
            .set_title(trf!("Папка с файлами «{name}» (внутри steamapps\\common)", "Folder with the files of \"{name}\" (inside steamapps\\common)"))
            .pick_folder()
        else {
            return;
        };
        // We expect <library>\steamapps\common\<game folder>.
        let library = dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("common")))
            .and_then(|p| p.parent())
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps")))
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        match library {
            Some(library) => {
                let _ = self.tx.send(Cmd::Import { appid, name, library });
            }
            None => {
                *self.launch_status.lock().unwrap() = tr!("Папка игры должна лежать в библиотеке Steam: …\\steamapps\\common\\<папка игры>. \
                     Перенесите ее туда и попробуйте снова.", "The game folder must be in a Steam library: …\\steamapps\\common\\<game folder>. Move it there and try again.")
                    .into();
            }
        }
    }
}
