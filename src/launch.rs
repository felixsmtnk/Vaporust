//! Launching games through Steam, updating them, stopping a game, closing Steam.

use crate::*;

impl LauncherApp {
    /// Result messages (uninstalled, closed, launch error) - as a popup notification:
    /// the status bar at the bottom is gone, and launch progress is shown on the game page.
    pub(crate) fn status_popup(&mut self) {
        let st = self.launch_status.lock().unwrap().clone();
        if st == self.status_seen {
            return;
        }
        let busy = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
        // "Удаляю...", "Прошу закрыться..." are progress, not a result: not shown.
        let in_progress = st.ends_with("...") || st.ends_with('…');
        if !busy && !in_progress && !st.is_empty() && st != tr!("Готово.", "Done.") {
            // The game in quotes - its image on the left, a click opens its page.
            // Russian texts use «», English ones "".
            let game = st
                .split_once('«')
                .and_then(|(_, rest)| rest.split_once('»'))
                .or_else(|| st.split_once('"').and_then(|(_, rest)| rest.split_once('"')))
                .map(|(name, _)| name)
                .and_then(|name| {
                    self.games
                        .iter()
                        .find(|g| g.name == name)
                        .map(|g| g.appid)
                });
            let kind = game.map_or(popup::Kind::Info, popup::Kind::Game);
            popup::push(kind, "Vaporust", st.clone());
        }
        self.status_seen = st;
    }

    /// Close the running game: gently (like the window's close button) or forcefully.
    pub(crate) fn stop_running(&mut self, appid: u32, force: bool) {
        let Some((id, name, dir)) = local::RUNNING.lock().unwrap().clone() else { return };
        if id != appid {
            return;
        }
        let n = local::stop_game(&dir, force);
        if !force {
            self.stop_asked = Some((appid, std::time::Instant::now()));
        }
        *self.launch_status.lock().unwrap() = match (force, n > 0) {
            (true, _) => trf!("«{name}» закрыта принудительно.", "\"{name}\" was force-closed."),
            (false, true) => trf!("Прошу «{name}» закрыться...", "Asking \"{name}\" to close..."),
            (false, false) => tr!("Процессы игры не найдены.", "Game processes not found.").into(),
        };
    }

    pub(crate) fn steam_running(&mut self) -> bool {
        let now = std::time::Instant::now();
        match self.steam_check {
            // A process snapshot costs ~5-7 ms - at most once per 5 s (when idle this is the main load).
            Some((t, v)) if now.duration_since(t) < std::time::Duration::from_secs(5) => v,
            _ => {
                let v = local::steam_running();
                self.steam_check = Some((now, v));
                v
            }
        }
    }

    /// Offline mode: clear Steam's offline flag when Steam is closed; without a connection -
    /// try to log in again once a minute.
    pub(crate) fn offline_tick(&mut self, net: &NetState) {
        if self.settings.steam_offline_set
            && !self.launching.load(Ordering::Relaxed)
            && !self.closing.load(Ordering::Relaxed)
            && !self.steam_running()
        {
            if let Some(s) = self.steam.clone() {
                if local::set_steam_offline(&s.dir, false).is_ok() {
                    self.settings.steam_offline_set = false;
                }
            }
        }
        if net.offline && net.account.is_none() && !net.busy && !self.settings.local_mode {
            let due = self.offline_retry.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60));
            if due {
                self.offline_retry = Some(std::time::Instant::now());
                let _ = self.tx.send(Cmd::Restore);
            }
        }
    }

    pub(crate) fn close_steam(&mut self, ctx: &egui::Context) {
        let Some(steam) = self.steam.clone() else {
            return;
        };
        self.closing.store(true, Ordering::Relaxed);
        let (status, closing, ctx) = (self.launch_status.clone(), self.closing.clone(), ctx.clone());
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            local::close_steam(&steam, &set);
            set(tr!("Steam закрыт.", "Steam closed."));
            closing.store(false, Ordering::Relaxed);
        });
        self.steam_check = None;
    }

    pub(crate) fn start(&mut self, appid: u32, ctx: &egui::Context) {
        self.status_app = Some(appid);
        if custom::is_custom(appid) {
            self.launch_custom(appid);
            return;
        }
        self.steam_task(appid, ctx, false);
    }

    /// Update a game via the hidden Steam (without launching the game).
    /// "Update" in the library: without Steam (Aurelia) when possible; otherwise via the hidden Steam.
    pub(crate) fn start_update(&mut self, appid: u32, ctx: &egui::Context) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        let acf = local::appmanifest_path(&game);
        let info = local::read_acf(&acf);
        // Waiting for a language change (chosen in "Properties") - Vaporust switches it itself (switch_language).
        let switching = info.language_pending();
        // Beta change - without Steam: Aurelia downloads the chosen branch versions for installed depots.
        let beta = (info.user_beta != info.mounted_beta).then(|| info.user_beta.clone());
        let logged_in = self.net.lock().unwrap().account.is_some();
        // The game is in two libraries: Aurelia takes the alphabetically first copy - may update the wrong one.
        let duplicate = self
            .steam
            .as_ref()
            .is_some_and(|s| local::find_duplicates(&s.dir).iter().any(|(id, _, _)| *id == appid));
        if self.settings.update_via_steam || duplicate || !logged_in || self.settings.pinned.contains_key(&appid) {
            self.steam_task(appid, ctx, true);
            return;
        }
        if self.steam_running() {
            self.install_note = tr!("Сначала закройте Steam (кнопка «Закрыть Steam» внизу), потом нажмите «Обновить» еще раз.", "Close Steam first (the \"Close Steam\" button at the bottom), then press \"Update\" again.").into();
            return;
        }
        if self.net.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
            self.install_note = tr!("Сейчас идет другая загрузка - обновите игру, когда она закончится.", "Another download is running - update the game when it finishes.").into();
            return;
        }
        self.install_note.clear();
        if switching {
            // Language first (the beta, if also chosen, on the next "Update" click).
            let _ = self.tx.send(Cmd::SetLanguage { appid, name: game.name.clone(), acf, lang: info.user_language.clone() });
            return;
        }
        let _ = self.tx.send(Cmd::UpdateDirect { appid, name: game.name.clone(), acf, beta });
    }

    /// Update via the hidden Steam (as before).
    pub(crate) fn start_update_steam(&mut self, appid: u32, ctx: &egui::Context) {
        self.steam_task(appid, ctx, true);
    }

    /// "Play" or "Update": both tasks start the hidden Steam, so they go one at a time.
    pub(crate) fn steam_task(&mut self, appid: u32, ctx: &egui::Context, update: bool) {
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            return;
        }
        // A game without Steam protection - launch directly (checkbox in "Properties").
        let family = is_family_only(&self.net.lock().unwrap(), appid);
        if !update && self.settings.no_steam.contains(&appid) && !family {
            let net = self.net.lock().unwrap().clone();
            self.direct_launch(appid, &net);
            return;
        }
        // A pinned version cannot be updated, and it must be prepared before launch.
        if self.settings.pinned.contains_key(&appid) {
            if update {
                *self.launch_status.lock().unwrap() = tr!("Версия игры закреплена - открепите ее в «Свойствах», чтобы обновить.", "The game version is pinned - unpin it in \"Properties\" to update.").into();
                return;
            }
            let net = self.net.lock().unwrap().clone();
            if !self.pin_gate(appid, &net) {
                return;
            }
        }
        let Some(steam) = self.steam.clone() else {
            return;
        };
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else {
            return;
        };
        // No connection to Steam: let the hidden Steam start offline right away (without a question window).
        // The flag is cleared when Steam is closed again (offline_tick).
        if self.net.lock().unwrap().offline && !self.steam_running() {
            match local::set_steam_offline(&steam.dir, true) {
                Ok(_) => self.settings.steam_offline_set = true,
                Err(e) => *self.launch_status.lock().unwrap() = trf!("Автономный режим Steam не включен: {e}", "Steam offline mode was not turned on: {e}"),
            }
        }
        let opts = local::LaunchOptions {
            freeze_webhelper: self.freeze_webhelper,
            freeze_soft: self.settings.freeze_soft,
            steam_args: self.settings.steam_args.clone(),
        };
        self.launching.store(true, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        let status = self.launch_status.clone();
        let launching = self.launching.clone();
        let cancel = self.cancel.clone();
        let rescan = self.rescan_needed.clone();
        let script = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            let res = if update {
                local::update_via_steam(&steam, &game, opts, &cancel, &set)
            } else {
                // Launch scripts: a command before launch (we wait for it), after exit (we do not wait).
                if !script.before.trim().is_empty() {
                    set(tr!("Выполняю команду перед запуском...", "Running the pre-launch command..."));
                }
                let warn = scripts::run_before(&script, game.appid, &game.name, &game.dir);
                let res = local::run_chain(&steam, &game, opts, &cancel, &set);
                let after = if res.is_ok() && !cancel.load(Ordering::Relaxed) {
                    scripts::run_after(&script, game.appid, &game.name, &game.dir)
                } else {
                    None
                };
                match (warn.or(after), res) {
                    (Some(w), Ok(msg)) => Ok(format!("{msg} {w}")),
                    (_, r) => r,
                }
            };
            match res {
                Ok(msg) => set(&msg),
                Err(e) => {
                    crate::errlog::write(if update { tr!("обновление", "update") } else { tr!("запуск", "launch") }, &format!("{}: {e}", game.name));
                    set(&e)
                }
            }
            launching.store(false, Ordering::Relaxed);
            // Steam may have updated games - rescan the disk and check for updates again.
            rescan.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }
}
