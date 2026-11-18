//! "Badges and cards": Steam level, how many trading cards will still drop in each
//! game, sets that can be crafted into a badge. Level - Web API IPlayerService/GetBadges,
//! cards - the community badges page (where the Steam site shows them).

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// A game with trading cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameCards {
    pub appid: u32,
    pub name: String,
    /// How many cards will still drop for the game.
    pub drops: u32,
    pub owned: u32,
    pub total: u32,
    /// Set is complete - a badge can be crafted.
    pub ready: bool,
    pub foil: bool,
    /// «8.8 hrs on record».
    pub hours: String,
}

#[derive(Clone, Debug, Default)]
pub struct Badges {
    pub level: u32,
    pub xp: u32,
    /// XP at which the current level started, and how much is needed for the next one.
    pub xp_level_start: u32,
    pub xp_to_next: u32,
    pub badges_count: usize,
    pub games: Vec<GameCards>,
}

use crate::util::between;

fn clean(s: &str) -> String {
    s.replace("&nbsp;", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse the badges page (l=english).
pub fn parse_badges_page(html: &str) -> Vec<GameCards> {
    let mut out = Vec::new();
    for row in html.split("class=\"badge_row_overlay\" href=\"").skip(1) {
        let href = row.split('"').next().unwrap_or("");
        let Some(id) = href.split("/gamecards/").nth(1) else { continue };
        let Ok(appid) = id.split(['/', '?']).next().unwrap_or("").parse::<u32>() else { continue };
        let foil = href.contains("border=1");
        // A badge row ends where the next one begins.
        let row = row.split("badge_row is_link").next().unwrap_or(row);
        let name = between(row, "<div class=\"badge_title\">", "<").map(clean).unwrap_or_default();
        let drops_text = between(row, "progress_info_bold\">", "<").map(clean).unwrap_or_default();
        let drops = drops_text.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let hours = between(row, "badge_title_stats_playtime\">", "</div>").map(clean).unwrap_or_default();
        let progress = between(row, "class=\"badge_progress_info\">", "<").map(clean).unwrap_or_default();
        let (owned, total) = match progress.split_once(" of ") {
            Some((a, b)) => (a.trim().parse().unwrap_or(0), b.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0)),
            None => (0, 0),
        };
        let ready = row.contains("badge_craft_button");
        out.push(GameCards { appid, name, drops, owned, total, ready, foil, hours });
    }
    out
}

/// Level, XP and all card sets (badge pages one by one while there is a next one).
pub async fn load(client: &SteamClient, me: u64) -> Result<Badges, String> {
    let v = webapi::get(client, "IPlayerService/GetBadges/v1", &[("steamid", me.to_string())]).await?;
    let num = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let mut b = Badges {
        level: num("player_level"),
        xp: num("player_xp"),
        xp_level_start: num("player_xp_needed_current_level"),
        xp_to_next: num("player_xp_needed_to_level_up"),
        badges_count: v.get("badges").and_then(|x| x.as_array()).map_or(0, |a| a.len()),
        games: Vec::new(),
    };
    let web = webapi::web_session(client).await?;
    for page in 1..=10 {
        let url = format!("https://steamcommunity.com/profiles/{me}/badges/?l=english&p={page}");
        let html = web.get_text(&url).await.map_err(|e| format!("{e:#}"))?;
        let rows = parse_badges_page(&html);
        let more = html.contains(&format!("?p={}", page + 1));
        b.games.extend(rows);
        if !more {
            break;
        }
    }
    Ok(b)
}

/// Craft a badge from a complete set (spends the set's cards).
pub async fn craft(client: &SteamClient, me: u64, appid: u32, foil: bool) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let fields = [
        ("appid", appid.to_string()),
        ("series", "1".to_string()),
        ("border_color", if foil { "1" } else { "0" }.to_string()),
        ("sessionid", web.sessionid().to_string()),
    ];
    let (status, body) = web
        .post_form(
            &format!("https://steamcommunity.com/profiles/{me}/ajaxcraftbadge/"),
            &fields,
            &format!("https://steamcommunity.com/profiles/{me}/gamecards/{appid}/"),
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    let ok = status == 200
        && serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v.get("success")?.as_i64()) == Some(1);
    if ok {
        Ok(())
    } else {
        Err(trf!("Steam не создал значок (код {status}).", "Steam did not create the badge (code {status})."))
    }
}

/// Tab state.
#[derive(Default)]
pub struct BadgesUi {
    pub open: bool,
    pub asked: bool,
    /// Set for which badge crafting is being confirmed.
    pub confirm: Option<(u32, bool)>,
}

impl LauncherApp {
    pub(crate) fn open_badges(&mut self) {
        self.close_extras(crate::View::Badges);
        self.badges_ui.open = true;
        self.badges_ui.asked = false;
        self.view = crate::View::Badges;
    }

    pub(crate) fn badges_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.badges_ui.asked && net.account.is_some() {
            self.badges_ui.asked = true;
            let _ = self.tx.send(Cmd::Badges);
        }
        ui.horizontal(|ui| {
            crate::theme::page_title(ui, tr!("Значки и карточки", "Badges and cards"));
            ui.add_space(8.0);
            if crate::theme::text_link(ui, tr!("Обновить", "Refresh")).clicked() {
                let _ = self.tx.send(Cmd::Badges);
            }
        });
        if let Some(m) = &net.badges_msg {
            ui.label(m);
        }
        let b = match &net.badges {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                return;
            }
            Some(Ok(b)) => b.clone(),
        };
        let mut open_url: Option<String> = None;
        let me = net.my_id.unwrap_or(0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Level and XP to the next one.
            let span = (b.xp + b.xp_to_next).saturating_sub(b.xp_level_start).max(1);
            let done = b.xp.saturating_sub(b.xp_level_start);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(trf!("Уровень Steam {}", "Steam level {}", b.level)).font(crate::theme::semibold(18.0)).color(egui::Color32::WHITE));
                crate::theme::progress(
                    ui,
                    done as f32 / span as f32,
                    220.0,
                    Some(trf!("{} XP, до уровня {} еще {} XP", "{} XP, to level {}: {} XP more", b.xp, b.level + 1, b.xp_to_next)),
                );
            });
            ui.weak(trf!("Значков: {}", "Badges: {}", b.badges_count));
            ui.add_space(8.0);

            // Game badges as the rows of the Steam badges page: the game capsule, the name, hours,
            // drops left and the collected cards; games with drops left first.
            let mut games: Vec<&GameCards> = b.games.iter().filter(|g| g.drops > 0 || g.total > 0 || g.ready).collect();
            games.sort_by_key(|g| (std::cmp::Reverse(g.drops), std::cmp::Reverse(g.owned)));
            let total: u32 = games.iter().map(|g| g.drops).sum();
            ui.horizontal(|ui| {
                crate::theme::caps_title(ui, tr!("Значки игр", "Game badges"), Some(games.len()));
                ui.add_space(12.0);
                ui.label(egui::RichText::new(trf!("еще выпадет карточек: {total}", "card drops remaining: {total}")).size(12.0).color(crate::theme::WEAK));
            });
            ui.label(
                egui::RichText::new(tr!(
                    "Карточки выпадают, пока игра запущена через Steam (обычное «Играть» в Vaporust). При запуске без Steam они не выпадают.",
                    "Cards drop while the game runs through Steam (the normal \"Play\" in Vaporust). When launched without Steam they do not drop."
                ))
                .size(11.5)
                .color(crate::theme::DIM),
            );
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            for g in games {
                crate::theme::block(ui, |ui| {
                    ui.horizontal(|ui| {
                        let cap = egui::Image::new(format!("https://cdn.akamai.steamstatic.com/steam/apps/{}/capsule_184x69.jpg", g.appid))
                            .fit_to_exact_size(egui::vec2(150.0, 56.0))
                            .sense(egui::Sense::click());
                        if ui.add(cap).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr!("Страница значка в Steam", "Badge page on Steam")).clicked() {
                            open_url = Some(format!("https://steamcommunity.com/profiles/{me}/gamecards/{}/", g.appid));
                        }
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(egui::RichText::new(format!("{}{}", g.name, if g.foil { tr!(" (металлический)", " (foil)") } else { "" })).size(14.5).color(egui::Color32::WHITE));
                            let hours = hours_ru(&g.hours);
                            if !hours.is_empty() {
                                ui.label(egui::RichText::new(hours).size(11.5).color(crate::theme::DIM));
                            }
                            if g.drops > 0 {
                                ui.label(egui::RichText::new(trf!("Еще выпадет карточек: {}", "{} card drops remaining", g.drops)).size(12.0).color(crate::theme::LIGHT_BLUE));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if g.ready {
                                if self.badges_ui.confirm == Some((g.appid, g.foil)) {
                                    if ui.button(tr!("Нет", "No")).clicked() {
                                        self.badges_ui.confirm = None;
                                    }
                                    if crate::theme::accent_button(ui, tr!("Создать", "Craft")).on_hover_text(tr!("Карточки набора будут израсходованы", "The set's cards will be used up")).clicked() {
                                        let _ = self.tx.send(Cmd::CraftBadge { appid: g.appid, foil: g.foil });
                                        self.badges_ui.confirm = None;
                                    }
                                } else if ui
                                    .add_enabled(crate::exp::on(), egui::Button::new(tr!("Создать значок...", "Craft badge...")))
                                    .on_disabled_hover_text(crate::exp::HINT())
                                    .on_hover_text(tr!("Из полного набора карточек: значок, опыт Steam, фон и смайлик игры", "From a full card set: a badge, Steam XP, a background and an emoticon of the game"))
                                    .clicked()
                                {
                                    self.badges_ui.confirm = Some((g.appid, g.foil));
                                }
                            }
                            if g.total > 0 {
                                let total = g.total.max(1);
                                crate::theme::progress(ui, g.owned as f32 / total as f32, 140.0, Some(trf!("{} из {} карточек", "{} of {} cards", g.owned, g.total)));
                            }
                        });
                    });
                });
            }
        });
        if let Some(u) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(u));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rows() {
        let html = r#"
        <div class="badge_row is_link"><a class="badge_row_overlay" href="https://steamcommunity.com/profiles/1/badges/2"></a>
        <div class="badge_title">Years of Service</div></div>
        <div class="badge_row is_link"><a class="badge_row_overlay" href="https://steamcommunity.com/profiles/1/gamecards/702050/"></a>
        <div class="badge_title_stats_playtime">&nbsp; 8.8 hrs on record </div>
        <span class="progress_info_bold">3 card drops remaining</span>
        <div class="badge_title"> The Song of Saya &nbsp;<span>View details</span></div>
        <div class="badge_progress_info"> 2 of 5 cards collected </div></div>
        <div class="badge_row is_link"><a class="badge_row_overlay" href="https://steamcommunity.com/profiles/1/gamecards/70/?border=1"></a>
        <span class="progress_info_bold">No card drops remaining</span>
        <div class="badge_title">Half-Life</div>
        <a class="badge_craft_button" href="x">Craft</a></div>"#;
        let v = parse_badges_page(html);
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            GameCards { appid: 702050, name: "The Song of Saya".into(), drops: 3, owned: 2, total: 5, ready: false, foil: false, hours: "8.8 hrs on record".into() }
        );
        assert!(v[1].ready && v[1].foil && v[1].drops == 0);
    }

    #[test]
    #[ignore]
    fn badges_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let b = load(&c, me).await.unwrap();
            println!(
                "level {}, xp {}, to next {}, badges {}, games with cards {}, drops left {}",
                b.level,
                b.xp,
                b.xp_to_next,
                b.badges_count,
                b.games.len(),
                b.games.iter().map(|g| g.drops).sum::<u32>()
            );
            assert!(!b.games.is_empty());
        });
    }
}

/// "8.8 hrs on record" -> "8.8 ч в игре".
fn hours_ru(s: &str) -> String {
    match s.trim().strip_suffix("hrs on record").or_else(|| s.trim().strip_suffix("hr on record")) {
        Some(n) => trf!("{} ч в игре", "{} h on record", n.trim()),
        None => s.to_string(),
    }
}
