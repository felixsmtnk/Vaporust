//! «Активность друзей»: во что друзья играли последние две недели. Для каждого друга -
//! Web API IPlayerService/GetRecentlyPlayedGames (у кого данные об играх открыты вам).
//! Сверху - игры, популярные у друзей.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Recent {
    pub appid: u32,
    pub name: String,
    /// Минут за две недели и всего.
    pub two_weeks: u32,
    pub forever: u32,
}

#[derive(Clone, Debug, Default)]
pub struct FriendRecent {
    pub steamid: u64,
    pub name: String,
    pub games: Vec<Recent>,
}

fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}

pub fn parse_recent(v: &serde_json::Value) -> Vec<Recent> {
    v["games"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| Recent {
            appid: num(&g["appid"]) as u32,
            name: g["name"].as_str().unwrap_or("").to_string(),
            two_weeks: num(&g["playtime_2weeks"]) as u32,
            forever: num(&g["playtime_forever"]) as u32,
        })
        .collect()
}

/// Недавние игры друзей (по 6 запросов сразу). Друзья со скрытыми данными пропускаются.
pub async fn load(client: &SteamClient, friends: Vec<(u64, String)>) -> Result<Vec<FriendRecent>, String> {
    let mut set = tokio::task::JoinSet::new();
    let mut out = Vec::new();
    let mut queue = friends.into_iter();
    let mut ok = 0;
    loop {
        while set.len() < 6 {
            let Some((id, name)) = queue.next() else { break };
            let c = client.clone();
            set.spawn(async move {
                let r = webapi::get(&c, "IPlayerService/GetRecentlyPlayedGames/v1", &[("steamid", id.to_string())]).await;
                (id, name, r)
            });
        }
        let Some(done) = set.join_next().await else { break };
        if let Ok((steamid, name, Ok(v))) = done {
            ok += 1;
            let games = parse_recent(&v);
            if !games.is_empty() {
                out.push(FriendRecent { steamid, name, games });
            }
        }
    }
    if ok == 0 {
        return Err("Steam не ответил.".into());
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.games.iter().map(|g| g.two_weeks).sum::<u32>()));
    Ok(out)
}

/// Игры, популярные у друзей: (appid, название, сколько друзей, минут за 2 недели у всех).
pub fn popular(list: &[FriendRecent]) -> Vec<(u32, String, usize, u32)> {
    let mut m: HashMap<u32, (String, usize, u32)> = HashMap::new();
    for f in list {
        for g in &f.games {
            let e = m.entry(g.appid).or_insert((g.name.clone(), 0, 0));
            e.1 += 1;
            e.2 += g.two_weeks;
        }
    }
    let mut v: Vec<_> = m.into_iter().map(|(a, (n, c, t))| (a, n, c, t)).collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)));
    v
}

#[derive(Default)]
pub struct ActivityUi {
    pub open: bool,
    pub asked: bool,
    /// 0 - лента событий, 1 - во что играли.
    pub tab: u8,
    pub feed_asked: bool,
}

fn hours(min: u32) -> String {
    format!("{:.1} ч", min as f32 / 60.0)
}

impl LauncherApp {
    pub(crate) fn open_activity(&mut self) {
        self.close_extras(crate::View::Activity);
        self.activity_ui.open = true;
        self.activity_ui.asked = false;
        self.view = crate::View::Activity;
    }

    pub(crate) fn ask_activity(&mut self, net: &NetState) {
        let friends: Vec<(u64, String)> = net
            .roster
            .read()
            .unwrap()
            .values()
            .filter(|f| f.relationship == 3)
            .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string())))
            .collect();
        let _ = self.tx.send(Cmd::Activity(friends));
    }

    /// Вкладка «Лента событий» в «Активности».
    fn feed_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.activity_ui.feed_asked && net.account.is_some() {
            self.activity_ui.feed_asked = true;
            let _ = self.tx.send(Cmd::Feed { start: 0 });
        }
        ui.horizontal(|ui| {
            if ui.button("Обновить").clicked() {
                let _ = self.tx.send(Cmd::Feed { start: 0 });
            }
            if net.feed_busy {
                crate::spin::spinner(ui);
            }
        });
        ui.weak("События друзей, как на главной странице сообщества Steam.");
        let page = match &net.feed {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(p)) => p.clone(),
        };
        let mut open_store = None;
        let mut more = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut day = String::new();
            for e in &page.entries {
                if e.day != day {
                    day = e.day.clone();
                    ui.add_space(6.0);
                    ui.strong(&day);
                }
                ui.horizontal_top(|ui| {
                    if !e.avatar.is_empty() {
                        ui.add(egui::Image::new(e.avatar.as_str()).fit_to_exact_size(egui::vec2(24.0, 24.0)));
                    }
                    ui.vertical(|ui| {
                        let r = ui.add(egui::Label::new(&e.text).wrap().sense(egui::Sense::click()));
                        if let Some(a) = e.appid {
                            if r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Страница игры в магазине").clicked() {
                                open_store = Some(a);
                            }
                        }
                        if !e.images.is_empty() {
                            ui.horizontal(|ui| {
                                for img in &e.images {
                                    ui.add(egui::Image::new(crate::richtext::ugc_small(img, 320)).max_height(90.0).max_width(160.0));
                                }
                            });
                        }
                    });
                });
            }
            if page.entries.is_empty() {
                ui.weak("Событий нет.");
            }
            if let Some(n) = page.next {
                if ui.add_enabled(!net.feed_busy, egui::Button::new("Показать раньше")).clicked() {
                    more = Some(n);
                }
            }
        });
        if let Some(a) = open_store {
            self.open_store(Some(a), false);
        }
        if let Some(n) = more {
            let _ = self.tx.send(Cmd::Feed { start: n });
        }
    }

    pub(crate) fn activity_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        ui.horizontal(|ui| {
            ui.heading("Активность друзей");
            ui.separator();
            ui.selectable_value(&mut self.activity_ui.tab, 0, "Лента событий");
            ui.selectable_value(&mut self.activity_ui.tab, 1, "Во что играли");
        });
        if self.activity_ui.tab == 0 {
            self.feed_contents(ui, net);
            return;
        }
        if !self.activity_ui.asked && net.account.is_some() && !net.roster.read().unwrap().is_empty() {
            self.activity_ui.asked = true;
            self.ask_activity(net);
        }
        let mut refresh = false;
        ui.horizontal(|ui| {
            if ui.button("Обновить").clicked() {
                refresh = true;
            }
            if net.activity_busy {
                crate::spin::spinner(ui);
            }
        });
        if refresh {
            self.ask_activity(net);
        }
        ui.weak("Во что друзья играли за последние две недели (у кого данные об играх открыты вам).");
        let list = match &net.activity {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(v)) => v.clone(),
        };
        let mut open_store = None;
        let mut open_chat = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.strong("Популярно у друзей");
            egui::Grid::new("act_popular").num_columns(3).striped(true).show(ui, |ui| {
                for (appid, name, count, total) in popular(&list).into_iter().take(15) {
                    if ui.link(&name).on_hover_text("Страница в магазине").clicked() {
                        open_store = Some(appid);
                    }
                    ui.label(format!("друзей: {count}"));
                    ui.weak(format!("всего {}", hours(total)));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.strong("По друзьям");
            for f in &list {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.link(egui::RichText::new(&f.name).strong()).on_hover_text("Написать").clicked() {
                        open_chat = Some((f.steamid, f.name.clone()));
                    }
                });
                for g in &f.games {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if ui.link(&g.name).clicked() {
                            open_store = Some(g.appid);
                        }
                        ui.weak(format!("{} за 2 недели, всего {}", hours(g.two_weeks), hours(g.forever)));
                    });
                }
            }
            if list.is_empty() {
                ui.weak("Никто из друзей не играл последние две недели (или скрыл данные об играх).");
            }
        });
        if let Some(a) = open_store {
            self.open_store(Some(a), false);
        }
        if let Some((id, name)) = open_chat {
            let ctx = ui.ctx().clone();
            self.open_chat(&ctx, id, name, net);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popular_counts() {
        let f = |id: u64, games: &[(u32, u32)]| FriendRecent {
            steamid: id,
            name: String::new(),
            games: games.iter().map(|(a, m)| Recent { appid: *a, name: format!("g{a}"), two_weeks: *m, forever: *m }).collect(),
        };
        let p = popular(&[f(1, &[(10, 60), (20, 5)]), f(2, &[(10, 30)]), f(3, &[(30, 900)])]);
        assert_eq!(p[0], (10, "g10".into(), 2, 90));
        assert_eq!(p[1].0, 30);
    }

    #[test]
    #[ignore]
    fn activity_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            let ids: Vec<(u64, String)> = friends.iter().filter(|f| f.relationship == 3).map(|f| (f.steam_id, String::new())).collect();
            let n = ids.len();
            let v = load(&c, ids).await.unwrap();
            println!("друзей {n}, играли за 2 недели {}, популярных игр {}", v.len(), popular(&v).len());
        });
    }
}
