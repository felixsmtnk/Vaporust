//! Мастерская Steam в интерфейсе: поиск и подписка (на странице игры в библиотеке),
//! включение/выключение подписанных модов (во вкладке «Мастерская» свойств игры).
//! Сами моды Steam скачивает и обновляет при запуске игры.

use crate::net::{self, Cmd};
use crate::social::{SocialCmd, WorkshopState};
use aurelia::core::models::WorkshopItem;
use eframe::egui;

/// Состояние поиска в Мастерской для открытой игры.
#[derive(Default)]
pub struct WsUi {
    pub appid: u32,
    pub text: String,
    /// Тип запроса Steam: 0 популярные, 3 в тренде, 1 новые, 21 обновленные, 9 по подпискам.
    pub sort: u32,
    pub loaded: bool,
    /// Открытая страница мода и выбранная картинка на ней.
    pub item: Option<u64>,
    pub preview: usize,
}

impl WsUi {
    /// Для новой игры - сбросить поиск и один раз запросить подписки и популярное.
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
    ui.hyperlink_to(&it.title, url).on_hover_text("Открыть страницу мода в браузере");
    if it.file_size > 0 {
        ui.weak(net::fmt_bytes(it.file_size));
    }
    if it.time_updated > 0 {
        ui.weak(format!("обновлен {}", fmt_date(it.time_updated as u64)));
    }
}

/// Поиск в Мастерской с кнопками «Подписаться» / «Отписаться» (страница игры).
pub fn browse(ui: &mut egui::Ui, w: &mut WsUi, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let appid = w.appid;
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.strong("Мастерская");
        ui.weak(format!("подписок: {} (включить/выключить - в «Свойствах»)", subs.len()));
    });
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    ui.horizontal_wrapped(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut w.text).hint_text("Поиск модов").desired_width(220.0));
        let sorts = [(0u32, "популярные"), (3, "в тренде"), (1, "новые"), (21, "обновленные"), (9, "по подпискам")];
        let before = w.sort;
        egui::ComboBox::from_id_salt("ws_sort")
            .selected_text(sorts.iter().find(|s| s.0 == w.sort).map_or("популярные", |s| s.1))
            .show_ui(ui, |ui| {
                for (v, l) in sorts {
                    ui.selectable_value(&mut w.sort, v, l);
                }
            });
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || ui.button("Искать").clicked() || before != w.sort {
            cmds.push(Cmd::Social(SocialCmd::WorkshopQuery { appid, text: w.text.clone(), sort: w.sort }));
        }
        if ws.searching.contains(&appid) {
            crate::spin::spinner(ui);
        }
    });
    match ws.results.get(&appid) {
        None => {}
        Some(Err(_)) => {
            ui.weak("У этой игры нет Мастерской.");
        }
        Some(Ok(items)) if items.is_empty() => {
            ui.weak("Ничего не найдено.");
        }
        Some(Ok(items)) => {
            if let Some(t) = ws.total.get(&appid) {
                ui.weak(format!("Всего модов: {t}"));
            }
            // Плитка: картинка, название, подписчики и оценка, кнопка подписки.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                for it in items {
                    ui.allocate_ui(egui::vec2(200.0, 230.0), |ui| {
                        egui::Frame::none()
                            .fill(ui.visuals().faint_bg_color)
                            .rounding(6.0)
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.set_width(188.0);
                                ui.vertical(|ui| {
                                    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", it.id);
                                    let size = egui::vec2(188.0, 106.0);
                                    let img = if it.preview_url.is_empty() {
                                        ui.allocate_exact_size(size, egui::Sense::click()).1
                                    } else {
                                        ui.add(
                                            egui::Image::new(crate::richtext::ugc_small(&it.preview_url, 376))
                                                .fit_to_exact_size(size)
                                                .rounding(4.0)
                                                .sense(egui::Sense::click()),
                                        )
                                    };
                                    let _ = url;
                                    let title = crate::richtext::fix(ui, &it.title);
                                    let t = ui.add(egui::Label::new(egui::RichText::new(title).strong()).truncate().sense(egui::Sense::click()));
                                    if img.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Страница мода").clicked()
                                        || t.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                                    {
                                        w.item = Some(it.id);
                                        w.preview = 0;
                                        if !matches!(ws.details.get(&it.id), Some(Ok(_))) {
                                            cmds.push(Cmd::Social(SocialCmd::WorkshopDetail(it.id)));
                                        }
                                    }
                                    let mut info = Vec::new();
                                    if it.subscriptions > 0 {
                                        info.push(format!("подписчиков {}", crate::store::group_digits(it.subscriptions)));
                                    }
                                    let votes = it.votes_up + it.votes_down;
                                    if votes > 0 {
                                        info.push(format!("{}% хорошо", it.votes_up * 100 / votes));
                                    }
                                    ui.weak(egui::RichText::new(info.join(" · ")).small());
                                    let mut meta = Vec::new();
                                    if it.file_size > 0 {
                                        meta.push(net::fmt_bytes(it.file_size));
                                    }
                                    if it.time_updated > 0 {
                                        meta.push(format!("обновлен {}", fmt_date(it.time_updated as u64)));
                                    }
                                    ui.weak(egui::RichText::new(meta.join(" · ")).small());
                                    if subs.contains(&it.id) {
                                        if ui.button("Отписаться").clicked() {
                                            cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on: false }));
                                        }
                                    } else if ui.button("Подписаться").clicked() {
                                        cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on: true }));
                                    }
                                });
                            });
                    });
                }
            });
        }
    }
}

impl crate::LauncherApp {
    /// Открыть Мастерскую игры: в главном окне или (detached) в отдельном.
    pub(crate) fn open_workshop(&mut self, appid: u32, name: String, detached: bool) {
        self.ws_open = Some((appid, name));
        self.ws_detached = detached;
        if !detached {
            self.view = crate::View::Workshop;
        }
    }

    /// Поиск модов Мастерской (и в главном окне, и в отдельном).
    pub(crate) fn workshop_contents(&mut self, ui: &mut egui::Ui, net: &net::NetState) {
        let Some((appid, name)) = self.ws_open.clone() else { return };
        let mut cmds = Vec::new();
        self.ws_ui.ensure(appid, &mut cmds);
        ui.heading(format!("Мастерская: {name}"));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let ws = net.workshop.lock().unwrap();
            if self.ws_ui.item.is_some() {
                if detail_page(ui, &mut self.ws_ui, &ws, &mut cmds) {
                    self.ws_ui.item = None;
                }
            } else {
                browse(ui, &mut self.ws_ui, &ws, &mut cmds);
            }
        });
        for c in cmds {
            let _ = self.tx.send(c);
        }
    }

    /// Мастерская в отдельном окне (открыта с Ctrl).
    pub(crate) fn workshop_window(&mut self, ctx: &egui::Context, net: &net::NetState) {
        if !self.ws_detached {
            return;
        }
        let Some((_, name)) = self.ws_open.clone() else { return };
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_workshop"),
            egui::ViewportBuilder::default()
                .with_title(format!("Мастерская: {name} - Vaporust"))
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

/// Подписанные моды с галочками «включен» (вкладка свойств). Снятая галочка = отписка;
/// мод остается в списке, пока окно открыто, чтобы его можно было включить обратно.
pub fn subscriptions(ui: &mut egui::Ui, appid: u32, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    ui.weak("Включенные моды Steam скачает и обновит при следующем запуске игры. Найти новые - на странице игры в библиотеке.");
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    match ws.subscribed.get(&appid) {
        None => {
            crate::spin::spinner(ui);
        }
        Some(Err(_)) => {
            ui.weak("У этой игры нет Мастерской.");
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.weak("Подписанных модов нет.");
        }
        Some(Ok(list)) => {
            ui.label(format!("Включено модов: {} из {}", list.iter().filter(|i| subs.contains(&i.id)).count(), list.len()));
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
    /// Адреса превью популярных модов L4D2 (только чтение): cargo test ws_previews_live -- --ignored
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

/// Подробности мода для его страницы внутри программы.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub id: u64,
    pub title: String,
    pub description: String,
    /// Картинки: главная и дополнительные.
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

fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

pub fn parse_detail(d: &serde_json::Value) -> Detail {
    let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
    let mut previews = Vec::new();
    if !s("preview_url").is_empty() {
        previews.push(s("preview_url"));
    }
    for p in d["previews"].as_array().into_iter().flatten() {
        // preview_type 0 - картинка; видео YouTube (1) не показываем.
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

/// Подробности мода: открытый метод Steam ISteamRemoteStorage/GetPublishedFileDetails (без входа:
/// описание, теги, счетчики) и страница мода (дополнительные скриншоты). Два запроса на мод.
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
        return Err(format!("Steam ответил кодом {}.", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("{e:#}"))?;
    let d = &v["response"]["publishedfiledetails"][0];
    if d.is_null() || num(&d["result"]) != 1 {
        return Err("мод не найден или скрыт автором".into());
    }
    let mut det = parse_detail(d);
    // Скриншоты мода - только на его странице (полоса миниатюр под главной картинкой).
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

/// Адреса скриншотов со страницы мода (без параметров уменьшения).
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

/// Страница мода внутри программы. Возвращает true, если нажата «< К модам».
pub fn detail_page(ui: &mut egui::Ui, w: &mut WsUi, ws: &WorkshopState, cmds: &mut Vec<Cmd>) -> bool {
    let Some(id) = w.item else { return false };
    let mut back = false;
    if ui.button("< К модам").clicked() {
        back = true;
    }
    let d = match ws.details.get(&id) {
        None => {
            crate::spin::spinner(ui);
            return back;
        }
        Some(Err(e)) => {
            ui.label(format!("Не удалось загрузить мод: {e}"));
            return back;
        }
        Some(Ok(d)) => d.clone(),
    };
    let appid = w.appid;
    // Оценки открытый метод Steam не отдает - берем из списка модов.
    let mut d = d;
    if d.votes_up + d.votes_down == 0 {
        if let Some(it) = ws.results.get(&appid).and_then(|r| r.as_ref().ok()).and_then(|v| v.iter().find(|i| i.id == id)) {
            d.votes_up = it.votes_up as u64;
            d.votes_down = it.votes_down as u64;
        }
    }
    let subscribed = ws.sub_ids.get(&appid).is_some_and(|s| s.contains(&id));
    ui.label(egui::RichText::new(crate::richtext::fix(ui, &d.title)).size(22.0).strong());
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        // Слева - большая картинка и ряд маленьких, как в Мастерской Steam.
        let big_w = (ui.available_width() - 260.0).clamp(240.0, 560.0);
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
        // Справа - подписка и сведения.
        ui.vertical(|ui| {
            let (text, fill) = if subscribed {
                ("Отписаться", egui::Color32::from_rgb(70, 78, 92))
            } else {
                ("Подписаться", crate::theme::PLAY)
            };
            if crate::theme::big_button(ui, text, fill, true).clicked() {
                cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id, on: !subscribed }));
            }
            ui.weak("Steam скачает мод сам при следующем запуске игры.");
            ui.add_space(8.0);
            let votes = d.votes_up + d.votes_down;
            egui::Grid::new("ws_detail").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.weak(k);
                    ui.label(v);
                    ui.end_row();
                };
                if votes > 0 {
                    row("Оценки", format!("{}% хорошо ({})", d.votes_up * 100 / votes, crate::store::group_digits(votes)));
                }
                row("Подписчиков", crate::store::group_digits(d.subscriptions));
                if d.favorited > 0 {
                    row("В избранном", crate::store::group_digits(d.favorited));
                }
                if d.views > 0 {
                    row("Просмотров", crate::store::group_digits(d.views));
                }
                if d.size > 0 {
                    row("Размер", net::fmt_bytes(d.size));
                }
                if d.created > 0 {
                    row("Опубликован", fmt_date(d.created));
                }
                if d.updated > 0 {
                    row("Обновлен", fmt_date(d.updated));
                }
                if d.comments > 0 {
                    row("Комментариев", d.comments.to_string());
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
            if ui.link("Открыть в браузере (комментарии, обсуждения)").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", d.id)));
            }
        });
    });
    ui.add_space(10.0);
    ui.separator();
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
    back
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
    /// Живой тест (только чтение): страница мода L4D2 (Dust II, 312 тыс. подписчиков).
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
                Ok(d) => println!("ok: {} картинок, {} тегов, описание {} знаков, оценки {}/{}", d.previews.len(), d.tags.len(), d.description.len(), d.votes_up, d.votes_down),
                Err(e) => println!("ошибка: {e}"),
            }
        });
    }
}
