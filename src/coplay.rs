//! «Недавние игроки» (как в Steam: Друзья -> Недавние игроки): с кем вы играли в последнее
//! время в сетевых играх. Источник - страница steamcommunity.com/profiles/<я>/friends/coplay.

use aurelia::steam_client::SteamClient;

/// Игра и люди, с которыми в нее играли.
#[derive(Clone, Debug, Default)]
pub struct CoplayGame {
    pub appid: u32,
    pub name: String,
    /// «Играли 3 ноя» и т.п. (как пишет Steam).
    pub when: String,
    pub players: Vec<CoplayPlayer>,
}

#[derive(Clone, Debug, Default)]
pub struct CoplayPlayer {
    pub steamid: u64,
    pub name: String,
    pub avatar: String,
}

pub async fn load(client: &SteamClient, me: u64) -> Result<Vec<CoplayGame>, String> {
    let web = crate::webapi::web_session(client).await?;
    let t = web
        .get_text(&format!("https://steamcommunity.com/profiles/{me}/friends/coplay?l={lang}", lang = crate::lang::STEAM))
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(parse(&t))
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn attr(s: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    between(s, &key, "\"").map(|v| v.to_string())
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in s.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    crate::store::plain_text(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Разбор страницы: блоки `coplayGroup` (игра + список `friend_block_v2`).
pub fn parse(html: &str) -> Vec<CoplayGame> {
    let mut out = Vec::new();
    for block in html.split("class=\"coplayGroup\"").skip(1) {
        let mut g = CoplayGame::default();
        // Ссылка на игру: .../app/<id>... и название в gameListRowItemName / coplayAppInfo.
        if let Some(a) = between(block, "/app/", "\"") {
            g.appid = a.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
        if g.appid == 0 {
            if let Some(a) = between(block, "/apps/", "/") {
                g.appid = a.parse().unwrap_or(0);
            }
        }
        let info = between(block, "class=\"gameListRowItemName\"", "</div>")
            .or_else(|| between(block, "class=\"coplayAppInfo\"", "</div>"))
            .unwrap_or("");
        g.name = strip(info.split_once('>').map_or(info, |x| x.1));
        if let Some(w) = between(block, "class=\"gameListRowItemTopSecondary\"", "</div>")
            .or_else(|| between(block, "class=\"coplayAppInfo_date\"", "</div>"))
        {
            g.when = strip(w.split_once('>').map_or(w, |x| x.1));
        }
        for p in block.split("class=\"friend_block_v2").skip(1) {
            let head = &p[..p.find('>').unwrap_or(p.len())];
            let steamid = attr(head, "data-steamid").and_then(|v| v.parse().ok()).unwrap_or(0);
            if steamid == 0 {
                continue;
            }
            let name = between(p, "class=\"friend_block_content\">", "<").map(|s| s.trim().to_string()).unwrap_or_default();
            let avatar = between(p, "<img src=\"", "\"").unwrap_or("").to_string();
            g.players.push(CoplayPlayer { steamid, name: crate::store::plain_text(&name), avatar });
        }
        if !g.players.is_empty() {
            out.push(g);
        }
    }
    out
}

impl crate::LauncherApp {
    /// Вкладка «Недавние игроки» на странице «Друзья».
    pub(crate) fn coplay_contents(&mut self, ui: &mut eframe::egui::Ui, net: &crate::net::NetState) {
        use eframe::egui;
        if !self.friends_page.coplay_asked {
            self.friends_page.coplay_asked = true;
            let _ = self.tx.send(crate::net::Cmd::Coplay);
        }
        ui.weak("Люди, с которыми вы недавно играли в сетевых играх (не все игры сообщают об этом Steam).");
        ui.add_space(4.0);
        let games = match &net.coplay {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(v)) => v,
        };
        if games.is_empty() {
            ui.weak("Пока никого: Steam не запомнил, с кем вы играли.");
            return;
        }
        let friends: std::collections::HashSet<u64> =
            net.roster.read().unwrap().values().filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)).map(|f| f.steam_id).collect();
        let mut profile = None;
        let mut add = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for g in games {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if g.appid != 0 {
                        ui.add(
                            egui::Image::new(format!(
                                "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{}/capsule_231x87.jpg",
                                g.appid
                            ))
                            .fit_to_exact_size(egui::vec2(120.0, 45.0))
                            .rounding(2.0),
                        );
                    }
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&g.name).size(15.0).color(egui::Color32::WHITE));
                        if !g.when.is_empty() {
                            ui.weak(&g.when);
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    for p in &g.players {
                        egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::same(5.0)).show(ui, |ui| {
                            ui.set_width(220.0);
                            ui.horizontal(|ui| {
                                if !p.avatar.is_empty() {
                                    ui.add(egui::Image::new(p.avatar.as_str()).fit_to_exact_size(egui::vec2(32.0, 32.0)).rounding(2.0));
                                }
                                ui.vertical(|ui| {
                                    let name = if p.name.is_empty() { p.steamid.to_string() } else { p.name.clone() };
                                    if ui.add(egui::Label::new(name).truncate().sense(egui::Sense::click()))
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        profile = Some(p.steamid);
                                    }
                                    if friends.contains(&p.steamid) {
                                        ui.label(egui::RichText::new("в друзьях").small().color(crate::theme::PLAY_TEXT));
                                    } else if ui.small_button("Добавить в друзья").clicked() {
                                        add = Some(p.steamid);
                                    }
                                });
                            });
                        });
                    }
                });
            }
        });
        if let Some(id) = profile {
            self.open_profile(id, net);
        }
        if let Some(id) = add {
            let _ = self.tx.send(crate::net::Cmd::Social(crate::social::SocialCmd::AddFriend(id)));
        }
    }
}

#[cfg(test)]
mod live {
    /// VAPORUST_DUMP=<файл> cargo test coplay_dump -- --ignored --nocapture: сырой HTML и разбор.
    #[test]
    #[ignore]
    fn coplay_dump() {
        let path = std::env::var("VAPORUST_DUMP").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let t = web.get_text(&format!("https://steamcommunity.com/profiles/{me}/friends/coplay?l=russian")).await.unwrap();
            std::fs::write(&path, &t).unwrap();
            let v = super::parse(&t);
            println!("игр {}, людей {}", v.len(), v.iter().map(|g| g.players.len()).sum::<usize>());
            for g in v.iter().take(5) {
                println!("{} {} [{}] - {} чел.", g.appid, g.name, g.when, g.players.len());
            }
        });
    }
}
