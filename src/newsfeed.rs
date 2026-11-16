//! "News" tab: news of all own games in one feed (like Steam's news section).
//! Installed games and the most played ones are taken, a few news items each
//! (public Steam news API), sorted by date.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use std::collections::HashSet;

/// A news item in the feed.
#[derive(Clone, Debug)]
pub struct FeedItem {
    pub appid: u32,
    pub game: String,
    pub title: String,
    pub url: String,
    pub date: u64,
    /// Start of the text without formatting.
    pub excerpt: String,
    /// Full text (Steam BBCode).
    pub body: String,
}

/// How many games to query (3 news items each).
const MAX_GAMES: usize = 60;

/// Download the feed: game news in parallel (8 requests at a time), by date descending.
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
        return Err(tr!("Steam не ответил (нет связи?).", "Steam did not answer (no connection?).").into());
    }
    let mut seen = HashSet::new();
    out.retain(|n| seen.insert((n.appid, n.title.clone())));
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(200);
    Ok(out)
}

/// Game announcements from Steam "events" (the same as the game news section in the Steam client):
/// with l=russian Steam returns the Russian version if the developer wrote one, and the full text.
async fn events(appid: u32, count: u32) -> Result<Vec<FeedItem>, String> {
    let url = format!(
        "https://store.steampowered.com/events/ajaxgetpartnereventspageable/?clan_accountid=0&appid={appid}&offset=0&count={count}&l={lang}&origin=https:%2F%2Fstore.steampowered.com",
        lang = crate::lang::steam()
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


/// The first ~300 characters of the news text without markup.
fn excerpt(contents: &str) -> String {
    // In Steam news text square brackets may be escaped with a backslash.
    let raw = contents.replace("\\[", "[").replace("\\]", "]");
    let text = crate::store::plain_text(&raw).replace('\\', "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 300 {
        format!("{}...", text.chars().take(300).collect::<String>())
    } else {
        text
    }
}

/// Tab state.
#[derive(Default)]
pub struct NewsUi {
    pub asked: bool,
    /// Filter by game (None - all).
    pub game: Option<u32>,
    pub only_installed: bool,
    /// Open news item: (appid, date, title).
    pub open: Option<(u32, u64, String)>,
}

impl LauncherApp {
    /// Which games to query: installed ones, then the most played.
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
        v.retain(|(a, _)| *a != 228980); // Steam service libraries
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
            ui.heading(tr!("Новости ваших игр", "News of your games"));
            if ui.button(tr!("Обновить", "Refresh")).clicked() {
                let _ = self.tx.send(Cmd::NewsFeed(self.news_games(net)));
            }
            if net.news_busy {
                crate::spin::spinner(ui);
            }
        });
        let items = match &net.news_feed {
            None => {
                ui.weak(tr!("Загружаю новости...", "Loading news..."));
                return;
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                return;
            }
            Some(Ok(v)) => v,
        };
        // Filters: game, installed only.
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
                .unwrap_or_else(|| tr!("Все игры", "All games").into());
            egui::ComboBox::from_id_salt("news_game").selected_text(current).height(360.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut st.game, None, tr!("Все игры", "All games"));
                for (id, name) in &games {
                    ui.selectable_value(&mut st.game, Some(*id), name);
                }
            });
            ui.checkbox(&mut st.only_installed, tr!("Только установленные", "Installed only"));
        });
        ui.separator();
        // Open news item - in full.
        if let Some(n) = st.open.as_ref().and_then(|key| items.iter().find(|n| (n.appid, n.date, &n.title) == (key.0, key.1, &key.2))) {
            let mut back = false;
            ui.horizontal(|ui| {
                if ui.button(tr!("< К новостям", "< Back to news")).clicked() {
                    back = true;
                }
                if ui.button(tr!("Открыть в браузере", "Open in browser")).clicked() {
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
            ui.weak(tr!("Новостей нет.", "No news."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for n in shown {
                ui.horizontal(|ui| {
                    ui.weak(crate::socialwin::fmt_date(n.date));
                    if ui.link(&n.game).on_hover_text(tr!("Страница игры в библиотеке", "Game page in the library")).clicked() {
                        open_game = Some(n.appid);
                    }
                });
                if ui.add(egui::Label::new(egui::RichText::new(&n.title).strong()).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tr!("Читать целиком", "Read in full"))
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

impl LauncherApp {
    /// "What's new" shelf above the games on the library home: 4 fresh news items of own games.
    pub(crate) fn whats_new_shelf(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.settings.whats_new || crate::lite::on() {
            return;
        }
        if !self.news_ui.asked && (!net.owned.is_empty() || !self.games.is_empty()) {
            self.news_ui.asked = true;
            let _ = self.tx.send(Cmd::NewsFeed(self.news_games(net)));
        }
        let items: Vec<FeedItem> = match &net.news_feed {
            Some(Ok(v)) => v.iter().take(4).cloned().collect(),
            _ => return,
        };
        if items.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr!("ЧТО НОВОГО", "WHAT'S NEW")).size(13.0).color(crate::theme::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(tr!("Скрыть", "Hide")).on_hover_text(tr!("Вернуть - Настройки, раздел «Библиотека»", "Bring it back - Settings, \"Library\" section")).clicked() {
                    self.settings.whats_new = false;
                }
                if ui.link(tr!("Все новости", "All news")).clicked() {
                    self.close_extras(crate::View::News);
                    self.view = crate::View::News;
                }
            });
        });
        let gap = 10.0;
        // A card is at least 250 points wide: in a narrow window there are fewer cards.
        let fit = (((ui.available_width() + gap) / (250.0 + gap)).floor() as usize).clamp(1, items.len());
        let items = &items[..fit];
        let n = fit as f32;
        let w = ((ui.available_width() + gap) / n - gap).max(120.0);
        let mut open = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for it in items {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 64.0), egui::Sense::click());
                let bg = if resp.hovered() { egui::Color32::from_rgb(52, 66, 86) } else { crate::theme::CARD };
                ui.painter().rect_filled(rect, 3.0, bg);
                let img = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 12.5), egui::vec2(104.0, 39.0));
                egui::Image::new(format!(
                    "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/capsule_231x87.jpg",
                    it.appid
                ))
                .rounding(2.0)
                .paint_at(ui, img);
                let text = egui::Rect::from_min_max(egui::pos2(img.right() + 8.0, rect.top() + 4.0), rect.max - egui::vec2(6.0, 4.0));
                let mut c = ui.new_child(egui::UiBuilder::new().max_rect(text).layout(egui::Layout::top_down(egui::Align::Min)));
                use chrono::TimeZone;
                let date = chrono::Local
                    .timestamp_opt(it.date as i64, 0)
                    .single()
                    .map(|t| t.format("%d.%m").to_string())
                    .unwrap_or_default();
                c.add(egui::Label::new(egui::RichText::new(format!("{} · {date}", it.game)).small().weak()).truncate());
                // Title - up to two lines, then an ellipsis.
                let mut job = egui::text::LayoutJob::single_section(
                    it.title.clone(),
                    egui::TextFormat::simple(egui::FontId::proportional(12.5), egui::Color32::WHITE),
                );
                job.wrap = egui::text::TextWrapping { max_width: text.width(), max_rows: 2, break_anywhere: false, overflow_character: Some('…') };
                c.label(job);
                if resp.on_hover_text(&it.title).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    open = Some((it.appid, it.date, it.title.clone()));
                }
            }
        });
        ui.add_space(6.0);
        if open.is_some() {
            self.close_extras(crate::View::News);
            self.view = crate::View::News;
            self.news_ui.open = open;
        }
    }
}
