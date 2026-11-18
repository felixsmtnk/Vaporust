//! Steam Workshop in the UI: search and subscribing (on the game page in the library),
//! enabling/disabling subscribed mods (in the "Workshop" tab of game properties).
//! Steam itself downloads and updates the mods when the game launches.

use crate::net::{self, Cmd};
use crate::social::{SocialCmd, WorkshopState};
use aurelia::core::models::WorkshopItem;
use eframe::egui;

/// Workshop search state for the open game.
#[derive(Default)]
pub struct WsUi {
    pub appid: u32,
    pub text: String,
    /// Steam query type: 0 popular, 3 trending, 1 newest, 21 updated, 9 by subscriptions.
    pub sort: u32,
    pub loaded: bool,
    /// Open mod page and the selected image on it.
    pub item: Option<u64>,
    pub preview: usize,
    /// Show collections (not mods); the open collection; requested collections and mods.
    pub collections: bool,
    pub coll_open: Option<u64>,
    pub coll_asked: std::collections::HashSet<(u32, String)>,
    pub items_asked: std::collections::HashSet<u64>,
    /// Confirmation of "subscribe to the whole collection".
    pub sub_all: bool,
}

impl WsUi {
    /// For a new game - reset the search and request subscriptions and popular items once.
    pub fn ensure(&mut self, appid: u32, cmds: &mut Vec<Cmd>) {
        if self.appid != appid {
            *self = WsUi { appid, ..Default::default() };
        }
        if !self.loaded {
            self.loaded = true;
            cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribed(appid)));
            cmds.push(Cmd::Social(SocialCmd::WorkshopQuery { appid, text: String::new(), sort: self.sort }));
        }
    }
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

fn item_info(ui: &mut egui::Ui, it: &WorkshopItem) {
    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", it.id);
    ui.hyperlink_to(&it.title, url).on_hover_text(tr!("Открыть страницу мода в браузере", "Open the mod page in the browser"));
    if it.file_size > 0 {
        ui.weak(net::fmt_bytes(it.file_size));
    }
    if it.time_updated > 0 {
        ui.weak(trf!("обновлен {}", "updated {}", fmt_date(it.time_updated as u64)));
    }
}

/// Workshop search with "Subscribe" / "Unsubscribe" buttons (game page).
pub fn browse(ui: &mut egui::Ui, w: &mut WsUi, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let appid = w.appid;
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    // The Workshop banner, as the top of a Steam Workshop page: the game header darkened, "WORKSHOP".
    let (banner, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 74.0), egui::Sense::hover());
    let img = egui::Image::new(format!("https://cdn.akamai.steamstatic.com/steam/apps/{appid}/header.jpg"))
        .uv(egui::Rect::from_min_max(egui::pos2(0.0, 0.3), egui::pos2(1.0, 0.62)))
        .tint(egui::Color32::from_gray(110));
    img.paint_at(ui, banner);
    crate::theme::gradient_rect(ui.painter(), banner, egui::Color32::from_black_alpha(200), egui::Color32::from_black_alpha(40));
    ui.painter().text(banner.left_center() + egui::vec2(16.0, -8.0), egui::Align2::LEFT_CENTER, tr!("МАСТЕРСКАЯ", "WORKSHOP"), crate::theme::semibold(22.0), egui::Color32::WHITE);
    ui.painter().text(
        banner.left_center() + egui::vec2(16.0, 16.0),
        egui::Align2::LEFT_CENTER,
        trf!("подписок: {} (включить/выключить - в «Свойствах»)", "subscriptions: {} (enable/disable in \"Properties\")", subs.len()),
        egui::FontId::proportional(12.5),
        crate::theme::TEXT2,
    );
    ui.add_space(8.0);
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    ui.horizontal_wrapped(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut w.text).hint_text(tr!("Поиск модов", "Search mods")).desired_width(220.0));
        let sorts = [(0u32, tr!("популярные", "popular")), (3, tr!("в тренде", "trending")), (1, tr!("новые", "newest")), (21, tr!("обновленные", "updated")), (9, tr!("по подпискам", "by subscriptions"))];
        let before = w.sort;
        egui::ComboBox::from_id_salt("ws_sort")
            .selected_text(sorts.iter().find(|s| s.0 == w.sort).map_or(tr!("популярные", "popular"), |s| s.1))
            .show_ui(ui, |ui| {
                for (v, l) in sorts {
                    ui.selectable_value(&mut w.sort, v, l);
                }
            });
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || ui.button(tr!("Искать", "Search")).clicked() || before != w.sort {
            cmds.push(Cmd::Social(SocialCmd::WorkshopQuery { appid, text: w.text.clone(), sort: w.sort }));
        }
        if ws.searching.contains(&appid) {
            crate::spin::spinner(ui);
        }
    });
    match ws.results.get(&appid) {
        None => {}
        Some(Err(_)) => {
            ui.weak(tr!("У этой игры нет Мастерской.", "This game has no Workshop."));
        }
        Some(Ok(items)) if items.is_empty() => {
            ui.weak(tr!("Ничего не найдено.", "Nothing found."));
        }
        Some(Ok(items)) => {
            if let Some(t) = ws.total.get(&appid) {
                ui.weak(trf!("Всего модов: {t}", "Total mods: {t}"));
            }
            // A grid as in the Steam Workshop: the whole square image (on a dark background), under it
            // the name on two lines and a star rating; subscribing - a "+" button on the image on hover.
            let gap = 14.0;
            let avail = ui.available_width() - 4.0;
            let cols = (((avail + gap) / (170.0 + gap)).floor() as usize).max(1);
            let tile_w = ((avail - gap * (cols - 1) as f32) / cols as f32).min(230.0);
            let mut open = None;
            for row in items.chunks(cols) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for it in row {
                        match tile(ui, it, tile_w, subs.contains(&it.id)) {
                            Some(TileAct::Open) => open = Some(it.id),
                            Some(TileAct::Subscribe(on)) => {
                                cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on }));
                            }
                            None => {}
                        }
                    }
                });
                ui.add_space(gap);
            }
            if let Some(id) = open {
                w.item = Some(id);
                w.preview = 0;
                if !matches!(ws.details.get(&id), Some(Ok(_))) {
                    cmds.push(Cmd::Social(SocialCmd::WorkshopDetail(id)));
                }
            }
        }
    }
}

enum TileAct {
    Open,
    Subscribe(bool),
}

/// Mod tile: the whole image, name, stars and subscribers. Click - the mod page.
fn tile(ui: &mut egui::Ui, it: &aurelia::core::models::WorkshopItem, w: f32, subscribed: bool) -> Option<TileAct> {
    let mut act = None;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w + 58.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let hovered = resp.hovered();
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, w));
    let p = ui.painter_at(rect);
    p.rect_filled(img_rect, 3.0, egui::Color32::from_rgb(16, 20, 26));
    if !it.preview_url.is_empty() {
        // The whole image, centered in the square (not cropped).
        let url = crate::richtext::ugc_small(&it.preview_url, 400);
        let img = egui::Image::new(url).max_size(img_rect.size()).maintain_aspect_ratio(true);
        ui.put(img_rect, img);
    }
    if hovered {
        p.rect_stroke(img_rect, 3.0, egui::Stroke::new(2.0, crate::theme::ACCENT));
    }
    // Subscription: a green check mark if subscribed; "+" on hover.
    if subscribed || hovered {
        let b = egui::Rect::from_min_size(egui::pos2(img_rect.right() - 34.0, img_rect.top() + 6.0), egui::vec2(28.0, 28.0));
        let br = ui.interact(b, ui.id().with(("ws_sub", it.id)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let fill = if subscribed { crate::theme::PLAY } else if br.hovered() { crate::theme::ACCENT } else { egui::Color32::from_rgba_unmultiplied(20, 26, 34, 220) };
        p.rect_filled(b, 4.0, fill);
        let c = b.center();
        let s = egui::Stroke::new(2.2, egui::Color32::WHITE);
        if subscribed {
            p.line_segment([c + egui::vec2(-6.0, 0.0), c + egui::vec2(-2.0, 4.5)], s);
            p.line_segment([c + egui::vec2(-2.0, 4.5), c + egui::vec2(6.5, -5.0)], s);
        } else {
            p.line_segment([c + egui::vec2(-6.0, 0.0), c + egui::vec2(6.0, 0.0)], s);
            p.line_segment([c + egui::vec2(0.0, -6.0), c + egui::vec2(0.0, 6.0)], s);
        }
        let br = br.on_hover_text(if subscribed { tr!("Отписаться", "Unsubscribe") } else { tr!("Подписаться (Steam скачает мод при запуске игры)", "Subscribe (Steam downloads the mod when the game starts)") });
        if br.clicked() {
            act = Some(TileAct::Subscribe(!subscribed));
        }
    }
    // Name (up to two lines).
    let title = crate::richtext::fix(ui, &it.title);
    let mut job = egui::text::LayoutJob::simple(title, egui::FontId::proportional(13.5), egui::Color32::WHITE, w);
    job.wrap.max_rows = 2;
    let galley = ui.fonts(|f| f.layout_job(job));
    let ty = img_rect.bottom() + 5.0;
    let th = galley.size().y;
    p.galley(egui::pos2(rect.left(), ty), galley, egui::Color32::WHITE);
    // Star rating and subscribers.
    let votes = it.votes_up + it.votes_down;
    let sy = ty + th.max(17.0) + 10.0;
    if votes > 0 {
        let share = it.votes_up as f32 / votes as f32;
        stars(&p, egui::pos2(rect.left() + 6.0, sy), share);
    }
    if it.subscriptions > 0 {
        p.text(
            egui::pos2(rect.right(), sy),
            egui::Align2::RIGHT_CENTER,
            crate::store::group_digits(it.subscriptions),
            egui::FontId::proportional(11.5),
            crate::theme::WEAK,
        );
    }
    if act.is_none() && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        act = Some(TileAct::Open);
    }
    act
}

/// Five stars filled by the share of positive votes (like the Steam Workshop rating).
fn stars(p: &egui::Painter, left_center: egui::Pos2, share: f32) {
    let filled = (share * 5.0).round() as usize;
    for i in 0..5 {
        let c = left_center + egui::vec2(i as f32 * 13.0, 0.0);
        let pts: Vec<egui::Pos2> = (0..10)
            .map(|k| {
                let a = std::f32::consts::PI * (k as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
                let r = if k % 2 == 0 { 5.5 } else { 2.4 };
                c + egui::vec2(a.cos() * r, a.sin() * r)
            })
            .collect();
        let col = if i < filled { egui::Color32::from_rgb(230, 190, 70) } else { egui::Color32::from_rgb(70, 78, 92) };
        // A star is not convex: draw it as a fan of triangles from the center.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, col);
        for pt in &pts {
            mesh.colored_vertex(*pt, col);
        }
        for k in 0..10u32 {
            mesh.add_triangle(0, 1 + k, 1 + (k + 1) % 10);
        }
        p.add(egui::Shape::mesh(mesh));
    }
}

impl crate::LauncherApp {
    /// Open the game's Workshop: in the main window or (detached) in a separate one.
    pub(crate) fn open_workshop(&mut self, appid: u32, name: String, detached: bool) {
        self.ws_open = Some((appid, name));
        self.ws_detached = detached;
        if !detached {
            self.view = crate::View::Workshop;
        }
    }

    /// Workshop mod search (both in the main window and in the separate one).
    pub(crate) fn workshop_contents(&mut self, ui: &mut egui::Ui, net: &net::NetState) {
        let Some((appid, name)) = self.ws_open.clone() else { return };
        let mut cmds = Vec::new();
        self.ws_ui.ensure(appid, &mut cmds);
        let in_item = self.ws_ui.item.is_some();
        ui.horizontal(|ui| {
            if in_item {
                if ui.button(tr!("< Все моды", "< All mods")).clicked() {
                    self.ws_ui.item = None;
                }
            } else if !self.ws_detached && ui.button(tr!("< К игре", "< Back to game")).clicked() {
                self.view = crate::View::Library;
            }
            ui.label(egui::RichText::new(trf!("Мастерская  ·  {name}", "Workshop  ·  {name}")).size(15.0).color(crate::theme::WEAK));
            if !in_item {
                ui.add_space(12.0);
                if crate::theme::tab(ui, !self.ws_ui.collections, tr!("Моды", "Mods")).clicked() {
                    self.ws_ui.collections = false;
                }
                if crate::theme::tab(ui, self.ws_ui.collections, tr!("Коллекции", "Collections")).on_hover_text(tr!("Наборы модов, собранные игроками", "Mod sets collected by players")).clicked() {
                    self.ws_ui.collections = true;
                }
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let ws = net.workshop.lock().unwrap();
            if self.ws_ui.item.is_some() {
                crate::theme::centered(ui, 1100.0, |ui| detail_page(ui, &mut self.ws_ui, &ws, &mut cmds));
            } else if self.ws_ui.collections {
                collections_view(ui, &mut self.ws_ui, net, &ws, &mut cmds);
            } else {
                browse(ui, &mut self.ws_ui, &ws, &mut cmds);
            }
        });
        for c in cmds {
            let _ = self.tx.send(c);
        }
    }

    /// The Workshop in a separate window (opened with Ctrl).
    pub(crate) fn workshop_window(&mut self, ctx: &egui::Context, net: &net::NetState) {
        if !self.ws_detached {
            return;
        }
        let Some((_, name)) = self.ws_open.clone() else { return };
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_workshop"),
            egui::ViewportBuilder::default()
                .with_title(trf!("Мастерская: {name} - Vaporust", "Workshop: {name} - Vaporust"))
                .with_inner_size([720.0, 560.0])
                .with_min_inner_size([480.0, 320.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
                .with_decorations(crate::winframe::system()),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| self.workshop_contents(ui, net));
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.ws_open = None;
            self.ws_detached = false;
        }
    }
}

/// Subscribed mods with "enabled" checkboxes (properties tab). An unchecked box = unsubscribe;
/// the mod stays in the list while the window is open so it can be turned back on.
pub fn subscriptions(ui: &mut egui::Ui, appid: u32, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    ui.weak(tr!("Включенные моды Steam скачает и обновит при следующем запуске игры. Найти новые - на странице игры в библиотеке.", "Steam downloads and updates the enabled mods the next time the game starts. Find new ones on the game page in the library."));
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    match ws.subscribed.get(&appid) {
        None => {
            crate::spin::spinner(ui);
        }
        Some(Err(_)) => {
            ui.weak(tr!("У этой игры нет Мастерской.", "This game has no Workshop."));
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.weak(tr!("Подписанных модов нет.", "No subscribed mods."));
        }
        Some(Ok(list)) => {
            ui.label(trf!("Включено модов: {} из {}", "Mods enabled: {} of {}", list.iter().filter(|i| subs.contains(&i.id)).count(), list.len()));
            ui.separator();
            for it in list {
                ui.horizontal_wrapped(|ui| {
                    let mut on = subs.contains(&it.id);
                    if ui.checkbox(&mut on, "").changed() {
                        cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on }));
                    }
                    item_info(ui, it);
                });
            }
        }
    }
}

#[cfg(test)]
mod live {
    /// Preview URLs of popular L4D2 mods (read only): cargo test ws_previews_live -- --ignored
    #[test]
    #[ignore]
    fn ws_previews_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let p = c.query_workshop_files(550, None, 0, "*", 8, &[]).await.unwrap();
            for it in p.items.iter().take(8) {
                println!("{} | {}", it.title, it.preview_url);
            }
        });
    }
}

/// Mod details for its page inside the program.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub id: u64,
    pub title: String,
    pub description: String,
    /// Images: the main one and additional ones.
    pub previews: Vec<String>,
    pub tags: Vec<String>,
    pub subscriptions: u64,
    pub favorited: u64,
    pub views: u64,
    pub created: u64,
    pub updated: u64,
    pub size: u64,
    pub votes_up: u64,
    pub votes_down: u64,
    pub comments: u64,
}

use crate::util::num;

pub fn parse_detail(d: &serde_json::Value) -> Detail {
    let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
    let mut previews = Vec::new();
    if !s("preview_url").is_empty() {
        previews.push(s("preview_url"));
    }
    for p in d["previews"].as_array().into_iter().flatten() {
        // preview_type 0 - an image; YouTube videos (1) are not shown.
        if num(&p["preview_type"]) == 0 {
            if let Some(u) = p["url"].as_str().filter(|u| !u.is_empty() && !previews.iter().any(|x| x == u)) {
                previews.push(u.to_string());
            }
        }
    }
    Detail {
        id: num(&d["publishedfileid"]),
        title: s("title"),
        description: if s("file_description").is_empty() { s("description") } else { s("file_description") },
        previews,
        tags: d["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["display_name"].as_str().or(t["tag"].as_str()).map(str::to_string))
            .collect(),
        subscriptions: num(&d["subscriptions"]),
        favorited: num(&d["favorited"]),
        views: num(&d["views"]),
        created: num(&d["time_created"]),
        updated: num(&d["time_updated"]),
        size: num(&d["file_size"]),
        votes_up: num(&d["vote_data"]["votes_up"]),
        votes_down: num(&d["vote_data"]["votes_down"]),
        comments: num(&d["num_comments_public"]),
    }
}

/// Mod details: the public Steam method ISteamRemoteStorage/GetPublishedFileDetails (no login:
/// description, tags, counters) and the mod page (extra screenshots). Two requests per mod.
pub async fn load_detail(_c: &aurelia::steam_client::SteamClient, id: u64) -> Result<Detail, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let resp = http
        .post("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("itemcount=1&publishedfileids%5B0%5D={id}"))
        .send()
        .await
        .map_err(|e| format!("{e:#}"))?;
    if !resp.status().is_success() {
        return Err(trf!("Steam ответил кодом {}.", "Steam answered with code {}.", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("{e:#}"))?;
    let d = &v["response"]["publishedfiledetails"][0];
    if d.is_null() || num(&d["result"]) != 1 {
        return Err(tr!("мод не найден или скрыт автором", "the mod was not found or is hidden by the author").into());
    }
    let mut det = parse_detail(d);
    // Mod screenshots - only on its page (a strip of thumbnails under the main image).
    let page = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}&l=russian");
    if let Ok(r) = http.get(page).send().await {
        if let Ok(html) = r.text().await {
            for u in page_screenshots(&html) {
                if !det.previews.contains(&u) {
                    det.previews.push(u);
                }
            }
        }
    }
    Ok(det)
}

/// Short details of up to 100 mods in one request (the same public method, without the mod pages).
pub async fn load_brief(ids: &[u64]) -> Result<std::collections::HashMap<u64, Detail>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let mut body = format!("itemcount={}", ids.len());
    for (i, id) in ids.iter().enumerate() {
        body.push_str(&format!("&publishedfileids%5B{i}%5D={id}"));
    }
    let resp = http
        .post("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("{e:#}"))?;
    if !resp.status().is_success() {
        return Err(trf!("Steam ответил кодом {}.", "Steam answered with code {}.", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("{e:#}"))?;
    Ok(v["response"]["publishedfiledetails"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| num(&d["result"]) == 1)
        .map(|d| {
            let det = parse_detail(d);
            (det.id, det)
        })
        .collect())
}

/// Screenshot URLs from the mod page (without downscaling parameters).
pub fn page_screenshots(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in html.split("highlight_strip_screenshot").skip(1) {
        let Some(i) = part.find("src=\"") else { continue };
        let rest = &part[i + 5..];
        let Some(end) = rest.find('"') else { continue };
        let url = rest[..end].split('?').next().unwrap_or("").replace("&amp;", "&");
        if url.starts_with("http") && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Mod page inside the program ("back" - in the Workshop header).
pub fn detail_page(ui: &mut egui::Ui, w: &mut WsUi, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let Some(id) = w.item else { return };
    let d = match ws.details.get(&id) {
        None => {
            crate::spin::spinner(ui);
            return;
        }
        Some(Err(e)) => {
            ui.label(trf!("Не удалось загрузить мод: {e}", "Could not load the mod: {e}"));
            return;
        }
        Some(Ok(d)) => d.clone(),
    };
    let appid = w.appid;
    // The public Steam method does not return votes - take them from the mod list.
    let mut d = d;
    if d.votes_up + d.votes_down == 0 {
        if let Some(it) = ws.results.get(&appid).and_then(|r| r.as_ref().ok()).and_then(|v| v.iter().find(|i| i.id == id)) {
            d.votes_up = it.votes_up as u64;
            d.votes_down = it.votes_down as u64;
        }
    }
    let subscribed = ws.sub_ids.get(&appid).is_some_and(|s| s.contains(&id));
    ui.label(egui::RichText::new(crate::richtext::fix(ui, &d.title)).font(crate::theme::semibold(22.0)).color(egui::Color32::WHITE));
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        // On the left - a large image and a row of small ones, as in the Steam Workshop.
        // As in Steam: large images on the left (60% of the width), the subscription and info next to them on the right.
        let big_w = (ui.available_width() * 0.6).clamp(240.0, 640.0);
        ui.vertical(|ui| {
            let big = egui::vec2(big_w, big_w * 9.0 / 16.0);
            ui.set_width(big.x);
            let cur = w.preview.min(d.previews.len().saturating_sub(1));
            match d.previews.get(cur) {
                Some(u) => {
                    ui.add(egui::Image::new(crate::richtext::ugc_small(u, 800)).fit_to_exact_size(big).rounding(4.0));
                }
                None => {
                    ui.allocate_exact_size(big, egui::Sense::hover());
                }
            }
            if d.previews.len() > 1 {
                ui.horizontal_wrapped(|ui| {
                    for (i, u) in d.previews.iter().enumerate().take(12) {
                        let r = ui.add(
                            egui::Image::new(crate::richtext::ugc_small(u, 160))
                                .fit_to_exact_size(egui::vec2(80.0, 45.0))
                                .rounding(3.0)
                                .sense(egui::Sense::click()),
                        );
                        if i == cur {
                            ui.painter().rect_stroke(r.rect, 3.0, egui::Stroke::new(2.0, crate::theme::ACCENT));
                        }
                        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            w.preview = i;
                        }
                    }
                });
            }
        });
        // On the right - the subscription and info.
        ui.vertical(|ui| {
            // Star rating, as in the Steam Workshop.
            let votes_all = d.votes_up + d.votes_down;
            if votes_all > 0 {
                let (r, _) = ui.allocate_exact_size(egui::vec2(160.0, 20.0), egui::Sense::hover());
                stars(ui.painter(), r.left_center() + egui::vec2(7.0, 0.0), d.votes_up as f32 / votes_all as f32);
                ui.painter().text(
                    r.left_center() + egui::vec2(72.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    trf!("оценок: {}", "ratings: {}", crate::store::group_digits(votes_all)),
                    egui::FontId::proportional(12.0),
                    crate::theme::WEAK,
                );
                ui.add_space(4.0);
            }
            let (text, fill) = if subscribed {
                (tr!("Отписаться", "Unsubscribe"), egui::Color32::from_rgb(70, 78, 92))
            } else {
                (tr!("Подписаться", "Subscribe"), crate::theme::PLAY)
            };
            if crate::theme::big_button(ui, text, fill, true).clicked() {
                cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id, on: !subscribed }));
            }
            ui.weak(tr!("Steam скачает мод сам при следующем запуске игры.", "Steam downloads the mod itself the next time the game starts."));
            ui.horizontal_wrapped(|ui| {
                if crate::exp::button(ui, tr!("Нравится", "Like")).on_hover_text(tr!("Оценить мод (видно автору)", "Rate the mod (the author sees it)")).clicked() {
                    cmds.push(Cmd::WsVote { id, up: true });
                }
                if crate::exp::button(ui, tr!("Не нравится", "Dislike")).clicked() {
                    cmds.push(Cmd::WsVote { id, up: false });
                }
                if crate::exp::button(ui, tr!("В избранное", "Add to favorites")).on_hover_text(tr!("Избранное Мастерской в вашем профиле", "Workshop favorites in your profile")).clicked() {
                    cmds.push(Cmd::WsFavorite { id, appid, add: true });
                }
                if crate::exp::button(ui, tr!("Наградить...", "Award...")).on_hover_text(tr!("Наградить мод очками Steam", "Award the mod with Steam Points")).clicked() {
                    crate::storewin::UI_REQ.lock().unwrap().push(crate::storewin::UiReq::Award(2, id.to_string(), trf!("мод «{}»", "mod \"{}\"", d.title)));
                }
                if ui.button(tr!("Комментарии", "Comments")).on_hover_text(tr!("Откроются на сайте Steam", "Open on the Steam website")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://steamcommunity.com/sharedfiles/filedetails/comments/{id}")));
                }
            });
            ui.add_space(8.0);
            let votes = d.votes_up + d.votes_down;
            egui::Grid::new("ws_detail").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.weak(k);
                    ui.label(v);
                    ui.end_row();
                };
                if votes > 0 {
                    row(tr!("Оценки", "Ratings"), trf!("{}% хорошо ({})", "{}% good ({})", d.votes_up * 100 / votes, crate::store::group_digits(votes)));
                }
                row(tr!("Подписчиков", "Subscribers"), crate::store::group_digits(d.subscriptions));
                if d.favorited > 0 {
                    row(tr!("В избранном", "Favorited"), crate::store::group_digits(d.favorited));
                }
                if d.views > 0 {
                    row(tr!("Просмотров", "Views"), crate::store::group_digits(d.views));
                }
                if d.size > 0 {
                    row(tr!("Размер", "Size"), net::fmt_bytes(d.size));
                }
                if d.created > 0 {
                    row(tr!("Опубликован", "Published"), fmt_date(d.created));
                }
                if d.updated > 0 {
                    row(tr!("Обновлен", "Updated"), fmt_date(d.updated));
                }
                if d.comments > 0 {
                    row(tr!("Комментариев", "Comments"), d.comments.to_string());
                }
            });
            if !d.tags.is_empty() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for t in &d.tags {
                        egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::symmetric(6.0, 2.0)).show(ui, |ui| {
                            ui.label(egui::RichText::new(t).size(12.0));
                        });
                    }
                });
            }
            ui.add_space(6.0);
            if ui.link(tr!("Открыть в браузере (комментарии, обсуждения)", "Open in the browser (comments, discussions)")).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", d.id)));
            }
        });
    });
    ui.add_space(12.0);
    // Description - in a card, in normal text size.
    egui::Frame::none().fill(crate::theme::CARD).rounding(4.0).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(tr!("Описание", "Description")).size(15.0).strong());
        ui.add_space(6.0);
        ui.style_mut().override_text_style = Some(egui::TextStyle::Body);
        for b in crate::store::description_blocks(&d.description) {
        match b {
            crate::store::Block::Heading(t) => {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(crate::richtext::fix(ui, &t)).size(16.0).strong());
            }
            crate::store::Block::Para(t) => {
                ui.add(egui::Label::new(crate::richtext::fix(ui, &t)).wrap());
                ui.add_space(4.0);
            }
            crate::store::Block::Bullet(t) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("•");
                    ui.add(egui::Label::new(crate::richtext::fix(ui, &t)).wrap());
                });
            }
        }
        }
    });
    ui.add_space(12.0);
}

#[cfg(test)]
mod detail_tests {
    #[test]
    fn parses_detail() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"publishedfileid":"42","result":1,"title":"Map","file_description":"[h1]Hi[/h1]","preview_url":"https://x/a.jpg",
            "previews":[{"preview_type":0,"url":"https://x/b.jpg"},{"preview_type":1,"youtubevideoid":"q"}],
            "tags":[{"tag":"Maps","display_name":"Карты"}],"subscriptions":10,"file_size":"2048","vote_data":{"votes_up":9,"votes_down":1}}"#,
        )
        .unwrap();
        let d = super::parse_detail(&v);
        assert_eq!(d.id, 42);
        assert_eq!(d.previews, vec!["https://x/a.jpg", "https://x/b.jpg"]);
        assert_eq!(d.tags, vec!["Карты"]);
        assert_eq!((d.size, d.votes_up, d.votes_down), (2048, 9, 1));
    }
}

#[cfg(test)]
mod detail_live {
    /// Live test (read only): an L4D2 mod page (Dust II, 312 thousand subscribers).
    #[test]
    #[ignore]
    fn detail_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let p = c.query_workshop_files(550, None, 0, "*", 1, &[]).await.unwrap();
            let id = p.items[0].id;
            match super::load_detail(&c, id).await {
                Ok(d) => println!("ok: {} images, {} tags, description {} chars, votes {}/{}", d.previews.len(), d.tags.len(), d.description.len(), d.votes_up, d.votes_down),
                Err(e) => println!("error: {e}"),
            }
        });
    }
}

/// Workshop collections of a game: list, collection contents, "subscribe to all".
fn collections_view(ui: &mut egui::Ui, w: &mut WsUi, net: &net::NetState, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let appid = w.appid;
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    if let Some(cid) = w.coll_open {
        if ui.button(tr!("< Все коллекции", "< All collections")).clicked() {
            w.coll_open = None;
            w.sub_all = false;
            return;
        }
        if w.items_asked.insert(cid) {
            cmds.push(Cmd::WsCollectionItems(cid));
        }
        let title = net
            .ws_collections
            .iter()
            .filter(|((a, _), _)| *a == appid)
            .filter_map(|(_, r)| r.as_ref().ok())
            .flatten()
            .find(|c| c.id == cid)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        ui.label(egui::RichText::new(crate::richtext::fix(ui, &title)).size(17.0).strong());
        match net.ws_collection_items.get(&cid) {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
            }
            Some(Ok(items)) => {
                let missing: Vec<u64> = items.iter().copied().filter(|i| !subs.contains(i)).collect();
                ui.horizontal(|ui| {
                    ui.weak(trf!("Модов: {}, из них вы подписаны на {}", "Mods: {}, you are subscribed to {}", items.len(), items.len() - missing.len()));
                    if !missing.is_empty() {
                        if !w.sub_all {
                            if ui.button(trf!("Подписаться на все ({})", "Subscribe to all ({})", missing.len())).clicked() {
                                w.sub_all = true;
                            }
                        } else {
                            ui.label(trf!("Подписаться на {} модов? Steam скачает их при запуске игры.", "Subscribe to {} mods? Steam downloads them when the game starts.", missing.len()));
                            if ui.button(tr!("Да", "Yes")).clicked() {
                                for id in &missing {
                                    cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: *id, on: true }));
                                }
                                w.sub_all = false;
                            }
                            if ui.button(tr!("Нет", "No")).clicked() {
                                w.sub_all = false;
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                // Names of all mods of the collection - in batches of 100 (was two requests per mod).
                let missing: Vec<u64> = items
                    .iter()
                    .take(200)
                    .copied()
                    .filter(|id| !ws.details.contains_key(id) && !ws.brief.contains_key(id) && w.items_asked.insert(*id))
                    .collect();
                if !missing.is_empty() {
                    cmds.push(Cmd::Social(SocialCmd::WorkshopBrief(missing)));
                }
                for id in items.iter().take(200) {
                    let name = match ws.details.get(id).or_else(|| ws.brief.get(id)) {
                        Some(Ok(d)) => d.title.clone(),
                        Some(Err(_)) => trf!("мод {id} (недоступен)", "mod {id} (unavailable)"),
                        None => trf!("мод {id}", "mod {id}"),
                    };
                    ui.horizontal(|ui| {
                        let on = subs.contains(id);
                        if ui.link(crate::richtext::fix(ui, &name)).clicked() {
                            w.item = Some(*id);
                        }
                        if on {
                            ui.label(egui::RichText::new(tr!("подписаны", "subscribed")).small().color(crate::theme::PLAY_TEXT));
                        }
                    });
                }
            }
        }
        return;
    }
    let key = (appid, w.text.trim().to_string());
    ui.horizontal(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut w.text).hint_text(tr!("Поиск коллекций", "Search collections")).desired_width(220.0));
        if (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button(tr!("Искать", "Search")).clicked() {
            w.coll_asked.remove(&(appid, w.text.trim().to_string()));
        }
    });
    if w.coll_asked.insert(key.clone()) {
        cmds.push(Cmd::WsCollections { appid, text: key.1.clone() });
    }
    match net.ws_collections.get(&key) {
        None => {
            crate::spin::spinner(ui);
        }
        Some(Err(e)) => {
            ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.weak(tr!("Коллекций нет.", "No collections."));
        }
        Some(Ok(list)) => {
            for c in list {
                let r = egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width().min(900.0));
                    ui.horizontal(|ui| {
                        if !c.preview.is_empty() {
                            ui.add(egui::Image::new(crate::richtext::ugc_small(&c.preview, 128)).fit_to_exact_size(egui::vec2(64.0, 64.0)).rounding(2.0));
                        }
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(crate::richtext::fix(ui, &c.title)).size(15.0).color(egui::Color32::WHITE));
                            ui.weak(trf!("модов: {} · следят: {}", "mods: {} · followers: {}", c.items, crate::store::group_digits(c.subscriptions)));
                        });
                    });
                });
                if r.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    w.coll_open = Some(c.id);
                }
                ui.add_space(3.0);
            }
        }
    }
}
