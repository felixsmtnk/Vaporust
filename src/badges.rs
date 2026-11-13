//! «Значки и карточки»: уровень Steam, сколько коллекционных карточек еще выпадет в каждой
//! игре, наборы, из которых можно создать значок. Уровень - Web API IPlayerService/GetBadges,
//! карточки - страница значков сообщества (там же, где их видит сайт Steam).

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// Игра с коллекционными карточками.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameCards {
    pub appid: u32,
    pub name: String,
    /// Сколько карточек еще выпадет за игру.
    pub drops: u32,
    pub owned: u32,
    pub total: u32,
    /// Набор собран - можно создать значок.
    pub ready: bool,
    pub foil: bool,
    /// «8.8 hrs on record».
    pub hours: String,
}

#[derive(Clone, Debug, Default)]
pub struct Badges {
    pub level: u32,
    pub xp: u32,
    /// Опыт, с которого начался текущий уровень, и сколько нужно до следующего.
    pub xp_level_start: u32,
    pub xp_to_next: u32,
    pub badges_count: usize,
    pub games: Vec<GameCards>,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn clean(s: &str) -> String {
    s.replace("&nbsp;", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Разобрать страницу значков (l=english).
pub fn parse_badges_page(html: &str) -> Vec<GameCards> {
    let mut out = Vec::new();
    for row in html.split("class=\"badge_row_overlay\" href=\"").skip(1) {
        let href = row.split('"').next().unwrap_or("");
        let Some(id) = href.split("/gamecards/").nth(1) else { continue };
        let Ok(appid) = id.split(['/', '?']).next().unwrap_or("").parse::<u32>() else { continue };
        let foil = href.contains("border=1");
        // Строка значка кончается там, где начинается следующая.
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

/// Уровень, опыт и все наборы карточек (страницы значков по очереди, пока есть следующая).
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

/// Создать значок из собранного набора (тратит карточки набора).
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
        Err(format!("Steam не создал значок (код {status})."))
    }
}

/// Состояние вкладки.
#[derive(Default)]
pub struct BadgesUi {
    pub open: bool,
    pub asked: bool,
    /// Набор, для которого подтверждаем создание значка.
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
            ui.heading("Значки и карточки");
            if ui.button("Обновить").clicked() {
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
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(b)) => b.clone(),
        };
        let mut open_url: Option<String> = None;
        let me = net.my_id.unwrap_or(0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Уровень и опыт до следующего.
            let span = (b.xp + b.xp_to_next).saturating_sub(b.xp_level_start).max(1);
            let done = b.xp.saturating_sub(b.xp_level_start);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Уровень Steam {}", b.level)).size(18.0).strong());
                ui.add(
                    egui::ProgressBar::new(done as f32 / span as f32)
                        .desired_width(260.0)
                        .text(format!("{} XP, до уровня {} еще {} XP", b.xp, b.level + 1, b.xp_to_next)),
                );
            });
            ui.weak(format!("Значков: {}", b.badges_count));
            ui.add_space(8.0);

            // Готовые наборы.
            let ready: Vec<&GameCards> = b.games.iter().filter(|g| g.ready).collect();
            if !ready.is_empty() {
                ui.strong("Можно создать значок");
                for g in ready {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}{}", g.name, if g.foil { " (металлический)" } else { "" }));
                        if self.badges_ui.confirm == Some((g.appid, g.foil)) {
                            ui.label("Карточки набора будут израсходованы. Создать?");
                            if ui.button("Да").clicked() {
                                let _ = self.tx.send(Cmd::CraftBadge { appid: g.appid, foil: g.foil });
                                self.badges_ui.confirm = None;
                            }
                            if ui.button("Нет").clicked() {
                                self.badges_ui.confirm = None;
                            }
                        } else if ui
                            .button("Создать значок...")
                            .on_hover_text("Из полного набора карточек: значок, опыт Steam, фон и смайлик игры")
                            .clicked()
                        {
                            self.badges_ui.confirm = Some((g.appid, g.foil));
                        }
                    });
                }
                ui.add_space(8.0);
            }

            // Карточки, которые еще выпадут.
            let mut drops: Vec<&GameCards> = b.games.iter().filter(|g| g.drops > 0).collect();
            drops.sort_by_key(|g| std::cmp::Reverse(g.drops));
            let total: u32 = drops.iter().map(|g| g.drops).sum();
            ui.strong(format!("Еще выпадет карточек: {total} (в играх: {})", drops.len()));
            ui.weak(
                "Карточки выпадают, пока игра запущена через Steam (обычное «Играть» в Vaporust). При запуске без \
                 Steam они не выпадают.",
            );
            egui::Grid::new("drops").num_columns(3).striped(true).show(ui, |ui| {
                for g in &drops {
                    if ui.link(&g.name).on_hover_text("Страница значка в Steam").clicked() {
                        open_url = Some(format!("https://steamcommunity.com/profiles/{me}/gamecards/{}/", g.appid));
                    }
                    ui.label(format!("еще {}", g.drops));
                    ui.weak(hours_ru(&g.hours));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);

            // Все наборы: сколько собрано.
            ui.strong("Наборы карточек");
            egui::Grid::new("sets").num_columns(3).striped(true).show(ui, |ui| {
                for g in b.games.iter().filter(|g| g.total > 0 || g.ready) {
                    if ui.link(&g.name).clicked() {
                        open_url = Some(format!("https://steamcommunity.com/profiles/{me}/gamecards/{}/", g.appid));
                    }
                    let total = g.total.max(1);
                    ui.add(
                        egui::ProgressBar::new(g.owned as f32 / total as f32)
                            .desired_width(140.0)
                            .text(format!("{} из {}", g.owned, g.total)),
                    );
                    ui.weak(if g.ready { "готов" } else { "" });
                    ui.end_row();
                }
            });
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
                "уровень {}, xp {}, до след. {}, значков {}, игр с карточками {}, выпадет {}",
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

/// «8.8 hrs on record» -> «8.8 ч в игре».
fn hours_ru(s: &str) -> String {
    match s.trim().strip_suffix("hrs on record").or_else(|| s.trim().strip_suffix("hr on record")) {
        Some(n) => format!("{} ч в игре", n.trim()),
        None => s.to_string(),
    }
}
