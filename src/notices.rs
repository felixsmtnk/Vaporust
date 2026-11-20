//! Event notifications (except messages - those are shown by the network thread, see notify.rs):
//! a friend started a game / came online, a discount on a wishlisted game, a game is downloaded.
//! Checked every 2 s from the main loop; enabled by checkboxes in "Settings" -> "Notifications".

use crate::net::{Cmd, NetState};
use crate::popup::{self, Kind};
use crate::{store, LauncherApp};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// At first Steam sends friend statuses in a batch - we stay quiet for the first seconds after login.
const FRIENDS_QUIET: Duration = Duration::from_secs(30);
/// How often to check wishlist discounts.
const SALES_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct Notices {
    check: Option<Instant>,
    /// Friends: SteamID -> (online, game).
    seen: HashMap<u64, (bool, Option<u32>)>,
    /// When the friend list first became non-empty.
    since: Option<Instant>,
    /// When we last asked to check discounts.
    sales_asked: Option<Instant>,
    sales_gen: u64,
    /// Download: (appid, already finished).
    install: Option<(u32, bool)>,
    /// A friend started a game whose name is still loading: (friend, name, game, waiting since).
    pending: Vec<(u64, String, u32, Instant)>,
}

impl LauncherApp {
    pub(crate) fn notices_tick(&mut self, net: &NetState) {
        let s = &self.settings;
        crate::notify::set_messages(s.notify_on && s.notify_msg);
        popup::set_sound(s.notify_sound);
        if self.notices.check.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.notices.check = Some(Instant::now());
        self.friends_notices(net);
        self.pending_games(net);
        self.sales_notices(net);
        self.install_notice(net);
    }

    fn friends_notices(&mut self, net: &NetState) {
        let Ok(roster) = net.roster.read() else { return };
        let n = &mut self.notices;
        if net.account.is_none() || roster.is_empty() {
            n.seen.clear();
            n.since = None;
            return;
        }
        let since = *n.since.get_or_insert_with(Instant::now);
        let quiet = since.elapsed() < FRIENDS_QUIET;
        let s = &self.settings;
        for (id, f) in roster.iter().filter(|(_, f)| f.relationship == 3) {
            let online = f.persona_state.is_some_and(|p| p != 0);
            let now = (online, f.game_app_id.filter(|g| *g != 0));
            let before = n.seen.insert(*id, now);
            if quiet || !s.notify_on {
                continue;
            }
            let Some((was_online, was_game)) = before else { continue };
            let name = f.persona_name.clone().unwrap_or_else(|| tr!("Друг", "Friend").into());
            if let Some(game) = now.1.filter(|g| Some(*g) != was_game) {
                if s.notify_friend_game {
                    // Game name: from Steam, from the cache, or request it and wait (see below).
                    if let Some(t) =
                        f.game_name.clone().filter(|t| !t.is_empty()).or_else(|| net.app_names.get(&game).cloned())
                    {
                        popup::push_full(Kind::FriendGame(*id), name, trf!("играет в {t}", "is playing {t}"), None, popup::status_color(f.persona_state, true));
                    } else {
                        n.pending.push((*id, name, game, Instant::now()));
                    }
                }
            } else if online && !was_online && s.notify_friend_online {
                popup::push_full(Kind::FriendOnline(*id), name, tr!("в сети", "online").into(), None, popup::status_color(f.persona_state, false));
            }
        }
    }

    /// Friends in games whose names are not known yet: request and show when it arrives (at most 15 s).
    fn pending_games(&mut self, net: &NetState) {
        let mut ask = Vec::new();
        self.notices.pending.retain(|(id, name, game, since)| {
            let title = net.app_names.get(game).cloned();
            if title.is_none() && since.elapsed() < Duration::from_secs(15) {
                ask.push(*game);
                return true;
            }
            let body = title.map(|t| trf!("играет в {t}", "is playing {t}")).unwrap_or_else(|| tr!("зашел в игру", "started a game").into());
            popup::push_full(Kind::FriendGame(*id), name.clone(), body, None, popup::status_color(Some(1), true));
            false
        });
        ask.retain(|a| self.requested_apps.insert(*a));
        if !ask.is_empty() {
            let _ = self.tx.send(Cmd::ResolveApps(ask));
        }
    }

    fn sales_notices(&mut self, net: &NetState) {
        if net.account.is_none() || !self.settings.notify_on || !self.settings.notify_wishlist {
            return;
        }
        if self.notices.sales_asked.is_none_or(|t| t.elapsed() > SALES_EVERY) {
            self.notices.sales_asked = Some(Instant::now());
            let _ = self.tx.send(Cmd::Store(store::StoreCmd::WishlistSales));
        }
        let st = net.store.lock().unwrap();
        if st.sales_gen == self.notices.sales_gen {
            return;
        }
        self.notices.sales_gen = st.sales_gen;
        // A wishlisted game was released: it was "coming soon", now it is out.
        for (appid, name, soon) in &st.wish_status {
            if *soon {
                self.settings.wish_upcoming.insert(*appid);
            } else if self.settings.wish_upcoming.remove(appid) {
                popup::push(Kind::Sale(*appid), trf!("Вышла игра: {name}", "Released: {name}"), tr!("Из вашего списка желаемого.", "From your wishlist."));
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Forget ended discounts so the next discount on the same game is reported again.
        let current: std::collections::HashSet<u32> = st.sales.iter().map(|s| s.appid).collect();
        self.settings
            .notified_sales
            .retain(|appid, end| current.contains(appid) && (*end == 0 || *end > now));
        let fresh: Vec<&store::Sale> =
            st.sales.iter().filter(|s| !self.settings.notified_sales.contains_key(&s.appid)).collect();
        match fresh.len() {
            0 => {}
            1 => {
                let s = fresh[0];
                popup::push(
                    Kind::Sale(s.appid),
                    trf!("Скидка -{}%: {}", "-{}% off: {}", s.pct, s.name),
                    trf!("Из вашего списка желаемого. Сейчас {}.", "From your wishlist. Now {}.", s.price),
                );
            }
            k => {
                let names: Vec<String> =
                    fresh.iter().take(4).map(|s| format!("{} (-{}%)", s.name, s.pct)).collect();
                let more = if k > 4 { trf!(" и еще {}", " and {} more", k - 4) } else { String::new() };
                popup::push(Kind::Sale(0), trf!("Скидки в списке желаемого: {k}", "Discounts in your wishlist: {k}"), format!("{}{more}", names.join(", ")));
            }
        }
        for s in fresh {
            self.settings.notified_sales.insert(s.appid, s.end);
        }
    }

    fn install_notice(&mut self, net: &NetState) {
        let Some(i) = net.install.as_ref() else {
            self.notices.install = None;
            return;
        };
        let finished_ok = matches!(i.result, Some(Ok(())));
        let before = self.notices.install.replace((i.appid, i.result.is_some()));
        // Report only the "running -> finished successfully" transition and only for an install.
        // Downloaded to the temp folder while Steam was open: Steam sees it only after a restart.
        let staged = (self.unstaged_open.contains(&i.appid) || self.settings.staged.contains_key(&i.appid)) && crate::local::steam_running();
        if before == Some((i.appid, false))
            && finished_ok
            && i.download
            && self.settings.notify_on
            && (staged || (self.settings.notify_download && !crate::notify::app_focused()))
        {
            let body = if staged { tr!("Перезапустите Steam, чтобы играть.", "Restart Steam to play.").to_string() } else { trf!("{} - можно играть.", "{} - ready to play.", i.name) };
            popup::push(Kind::Download(i.appid), tr!("Игра скачана", "Game downloaded"), body);
        }
    }
}
