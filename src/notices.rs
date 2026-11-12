//! Уведомления о событиях (кроме сообщений — их показывает сетевой поток, см. notify.rs):
//! друг зашёл в игру / появился в сети, скидка на игру из списка желаемого, игра скачана.
//! Проверка раз в 2 с из главного цикла; включаются галочками в «Настройки» -> «Уведомления».

use crate::net::{Cmd, NetState};
use crate::popup::{self, Kind};
use crate::{store, LauncherApp};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Сначала Steam присылает статусы друзей пачкой — первые секунды после входа молчим.
const FRIENDS_QUIET: Duration = Duration::from_secs(30);
/// Как часто проверять скидки в списке желаемого.
const SALES_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct Notices {
    check: Option<Instant>,
    /// Друзья: SteamID -> (в сети, игра).
    seen: HashMap<u64, (bool, Option<u32>)>,
    /// Когда список друзей впервые оказался непустым.
    since: Option<Instant>,
    /// Когда последний раз просили проверить скидки.
    sales_asked: Option<Instant>,
    sales_gen: u64,
    /// Загрузка: (appid, уже закончилась).
    install: Option<(u32, bool)>,
    /// Друг зашёл в игру, название которой ещё грузится: (друг, имя, игра, с какого момента ждём).
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
            let name = f.persona_name.clone().unwrap_or_else(|| "Друг".into());
            if let Some(game) = now.1.filter(|g| Some(*g) != was_game) {
                if s.notify_friend_game {
                    // Название игры: от Steam, из кэша или запросим и подождём (см. ниже).
                    if let Some(t) =
                        f.game_name.clone().filter(|t| !t.is_empty()).or_else(|| net.app_names.get(&game).cloned())
                    {
                        popup::push_full(Kind::FriendGame(*id), name, format!("играет в {t}"), None, popup::status_color(f.persona_state, true));
                    } else {
                        n.pending.push((*id, name, game, Instant::now()));
                    }
                }
            } else if online && !was_online && s.notify_friend_online {
                popup::push_full(Kind::FriendOnline(*id), name, "в сети".into(), None, popup::status_color(f.persona_state, false));
            }
        }
    }

    /// Друзья в играх, чьё название ещё не известно: запросить и показать, когда придёт (не дольше 15 с).
    fn pending_games(&mut self, net: &NetState) {
        let mut ask = Vec::new();
        self.notices.pending.retain(|(id, name, game, since)| {
            let title = net.app_names.get(game).cloned();
            if title.is_none() && since.elapsed() < Duration::from_secs(15) {
                ask.push(*game);
                return true;
            }
            let body = title.map(|t| format!("играет в {t}")).unwrap_or_else(|| "зашёл в игру".into());
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Закончившиеся скидки забываем, чтобы о следующей скидке на ту же игру сообщить снова.
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
                    format!("Скидка -{}%: {}", s.pct, s.name),
                    format!("Из вашего списка желаемого. Сейчас {}.", s.price),
                );
            }
            k => {
                let names: Vec<String> =
                    fresh.iter().take(4).map(|s| format!("{} (-{}%)", s.name, s.pct)).collect();
                let more = if k > 4 { format!(" и ещё {}", k - 4) } else { String::new() };
                popup::push(Kind::Sale(0), format!("Скидки в списке желаемого: {k}"), format!("{}{more}", names.join(", ")));
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
        // Сообщаем только о переходе «шла -> закончилась успешно» и только для установки.
        if before == Some((i.appid, false))
            && finished_ok
            && i.done_text == "установлена."
            && self.settings.notify_on
            && self.settings.notify_download
            && !crate::notify::app_focused()
        {
            popup::push(Kind::Download(i.appid), "Игра скачана", format!("{} — можно играть.", i.name));
        }
    }
}
