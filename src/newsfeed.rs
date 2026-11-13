//! Вкладка «Новости»: новости всех своих игр одной лентой (как раздел новостей Steam).
//! Берутся установленные игры и самые игранные, по несколько новостей у каждой
//! (открытый API новостей Steam), и сортируются по дате.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use std::collections::HashSet;

/// Новость в ленте.
#[derive(Clone, Debug)]
pub struct FeedItem {
    pub appid: u32,
    pub game: String,
    pub title: String,
    pub url: String,
    pub date: u64,
    /// Начало текста без оформления.
    pub excerpt: String,
    /// Полный текст (BBCode Steam).
    pub body: String,
}

/// Сколько игр опрашивать (по 3 новости).
const MAX_GAMES: usize = 60;

/// Скачать ленту: новости игр параллельно (по 8 запросов), по убыванию даты.
pub async fn load(games: Vec<(u32, String)>) -> Result<Vec<FeedItem>, String> {
    let mut set = tokio::task::JoinSet::new();
    let mut out = Vec::new();
    let mut errors = 0;
    let mut queue = games.into_iter();
    loop {
        while set.len() < 8 {
            let Some((appid, name)) = queue.next() else { break };
            set.spawn(async move { (appid, name, events(appid, 3).await) });
        }
        let Some(done) = set.join_next().await else { break };
        match done {
            Ok((appid, game, Ok(items))) => {
                for mut n in items {
                    n.game = game.clone();
                    n.appid = appid;
                    out.push(n);
                }
            }
            _ => errors += 1,
        }
    }
    if out.is_empty() && errors > 0 {
        return Err("Steam не ответил (нет связи?).".into());
    }
    let mut seen = HashSet::new();
    out.retain(|n| seen.insert((n.appid, n.title.clone())));
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(200);
    Ok(out)
}

/// Объявления игры из «событий» Steam (то же, что раздел новостей игры в клиенте Steam):
/// с l=russian Steam отдает русскую версию, если разработчик ее написал, и полный текст.
async fn events(appid: u32, count: u32) -> Result<Vec<FeedItem>, String> {
    let url = format!(
        "https://store.steampowered.com/events/ajaxgetpartnereventspageable/?clan_accountid=0&appid={appid}&offset=0&count={count}&l=russian&origin=https:%2F%2Fstore.steampowered.com"
    );
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let r = http.get(url).send().await.map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = r.json().await.map_err(|e| format!("{e:#}"))?;
    Ok(v["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let body = e.get("announcement_body")?;
            let raw = body["body"].as_str().unwrap_or("").replace(r"\[", "[").replace(r"\]", "]");
            let gid = body["gid"].as_str().unwrap_or("").to_string();
            Some(FeedItem {
                appid,
                game: String::new(),
                title: body["headline"].as_str().or(e["event_name"].as_str()).unwrap_or("").to_string(),
                url: format!("https://store.steampowered.com/news/app/{appid}/view/{gid}"),
                date: body["posttime"].as_u64().unwrap_or(0),
                excerpt: excerpt(&raw),
                body: raw,
            })
        })
        .collect())
}


/// Первые ~300 знаков текста новости без разметки.
fn excerpt(contents: &str) -> String {
    // В тексте новостей Steam квадратные скобки бывают экранированы обратной чертой.
    let raw = contents.replace("\\[", "[").replace("\\]", "]");
    let text = crate::store::plain_text(&raw).replace('\\', "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 300 {
        format!("{}...", text.chars().take(300).collect::<String>())
    } else {
        text
    }
}

/// Состояние вкладки.
#[derive(Default)]
pub struct NewsUi {
    pub asked: bool,
    /// Фильтр по игре (None - все).
    pub game: Option<u32>,
    pub only_installed: bool,
    /// Открытая новость: (appid, дата, заголовок).
    pub open: Option<(u32, u64, String)>,
}

impl LauncherApp {
    /// Какие игры опрашивать: установленные, затем самые игранные.
    fn news_games(&self, net: &NetState) -> Vec<(u32, String)> {
        let mut v: Vec<(u32, String)> = self.games.iter().map(|g| (g.appid, g.name.clone())).collect();
        let mut owned: Vec<_> = net.owned.iter().collect();
        owned.sort_by_key(|g| std::cmp::Reverse(g.playtime_forever_minutes));
        for g in owned {
            if v.len() >= MAX_GAMES {
                break;
            }
            if !v.iter().any(|(a, _)| *a == g.app_id) {
                v.push((g.app_id, g.name.clone()));
            }
        }
        v.retain(|(a, _)| *a != 228980); // служебные библиотеки Steam
        v.truncate(MAX_GAMES);
        v
    }

    pub(crate) fn news_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.news_ui.asked && (!net.owned.is_empty() || !self.games.is_empty()) {
            self.news_ui.asked = true;
            let _ = self.tx.send(Cmd::NewsFeed(self.news_games(net)));
        }
        let installed: HashSet<u32> = self.games.iter().map(|g| g.appid).collect();
        let mut open_game = None;
        ui.horizontal(|ui| {
            ui.heading("Новости ваших игр");
            if ui.button("Обновить").clicked() {
                let _ = self.tx.send(Cmd::NewsFeed(self.news_games(net)));
            }
            if net.news_busy {
                crate::spin::spinner(ui);
            }
        });
        let items = match &net.news_feed {
            None => {
                ui.weak("Загружаю новости...");
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(v)) => v,
        };
        // Фильтры: игра, только установленные.
        let mut games: Vec<(u32, String)> = Vec::new();
        for n in items {
            if !games.iter().any(|(a, _)| *a == n.appid) {
                games.push((n.appid, n.game.clone()));
            }
        }
        games.sort_by_key(|g| g.1.to_lowercase());
        let st = &mut self.news_ui;
        ui.horizontal(|ui| {
            let current = st
                .game
                .and_then(|id| games.iter().find(|g| g.0 == id))
                .map(|g| g.1.clone())
                .unwrap_or_else(|| "Все игры".into());
            egui::ComboBox::from_id_salt("news_game").selected_text(current).height(360.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut st.game, None, "Все игры");
                for (id, name) in &games {
                    ui.selectable_value(&mut st.game, Some(*id), name);
                }
            });
            ui.checkbox(&mut st.only_installed, "Только установленные");
        });
        ui.separator();
        // Открытая новость - целиком.
        if let Some(n) = st.open.as_ref().and_then(|key| items.iter().find(|n| (n.appid, n.date, &n.title) == (key.0, key.1, &key.2))) {
            let mut back = false;
            ui.horizontal(|ui| {
                if ui.button("< К новостям").clicked() {
                    back = true;
                }
                if ui.button("Открыть в браузере").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&n.url));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(820.0);
                ui.horizontal(|ui| {
                    ui.weak(crate::socialwin::fmt_date(n.date));
                    if ui.link(&n.game).clicked() {
                        open_game = Some(n.appid);
                    }
                });
                ui.label(egui::RichText::new(&n.title).size(20.0).strong());
                ui.add_space(8.0);
                for b in crate::store::description_blocks(&n.body) {
                    match b {
                        crate::store::Block::Heading(t) => {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(t).size(16.0).strong());
                        }
                        crate::store::Block::Para(t) => {
                            ui.add(egui::Label::new(t).wrap());
                            ui.add_space(4.0);
                        }
                        crate::store::Block::Bullet(t) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("•");
                                ui.add(egui::Label::new(t).wrap());
                            });
                        }
                    }
                }
            });
            if back {
                st.open = None;
            }
            if let Some(a) = open_game {
                self.lib_page = Some(a);
                self.view = crate::View::Library;
            }
            return;
        }
        let mut open: Option<(u32, u64, String)> = None;
        let shown: Vec<&FeedItem> = items
            .iter()
            .filter(|n| st.game.is_none_or(|g| g == n.appid))
            .filter(|n| !st.only_installed || installed.contains(&n.appid))
            .collect();
        if shown.is_empty() {
            ui.weak("Новостей нет.");
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for n in shown {
                ui.horizontal(|ui| {
                    ui.weak(crate::socialwin::fmt_date(n.date));
                    if ui.link(&n.game).on_hover_text("Страница игры в библиотеке").clicked() {
                        open_game = Some(n.appid);
                    }
                });
                if ui.add(egui::Label::new(egui::RichText::new(&n.title).strong()).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Читать целиком")
                    .clicked()
                {
                    open = Some((n.appid, n.date, n.title.clone()));
                }
                if !n.excerpt.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&n.excerpt).weak()).wrap());
                }
                ui.add_space(10.0);
            }
        });
        if open.is_some() {
            self.news_ui.open = open;
        }
        if let Some(a) = open_game {
            self.lib_page = Some(a);
            self.view = crate::View::Library;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn feed_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let v = super::load(vec![(70, "Half-Life".into()), (550, "L4D2".into()), (730, "CS2".into())]).await.unwrap();
            println!("новостей: {}", v.len());
            for n in v.iter().take(3) {
                println!("{} | {} | {}", n.game, n.title, n.excerpt.chars().take(60).collect::<String>());
            }
            assert!(v.len() >= 3);
            assert!(v.windows(2).all(|w| w[0].date >= w[1].date));
        });
    }
}
