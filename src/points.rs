//! "Steam Points": balance and the points shop (chat effects, stickers, profile frames and backgrounds).
//! Web API ILoyaltyRewardsService: GetSummary, QueryRewardItems, RedeemPoints.
//! A purchase spends points (not money) and only after confirmation.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// Item kinds: (community_item_class, name).
#[allow(non_snake_case)]
pub fn CLASSES() -> [(u32, &'static str); 7] {
    [
    (12, tr!("Эффекты чата", "Chat effects")),
    (11, tr!("Стикеры", "Stickers")),
    (14, tr!("Рамки аватара", "Avatar frames")),
    (15, tr!("Анимированные аватары", "Animated avatars")),
    (3, tr!("Фоны профиля", "Profile backgrounds")),
    (13, tr!("Мини-фоны профиля", "Mini profile backgrounds")),
    (4, tr!("Смайлики", "Emoticons")),
]
}

#[derive(Clone, Debug, Default)]
pub struct RewardItem {
    pub defid: u64,
    pub appid: u32,
    /// Item kind (community_item_class) and its number within the game (community_item_type).
    pub class: u32,
    pub item_type: u32,
    /// Internal name (item_name), for example "Snowball".
    pub name: String,
    pub title: String,
    pub cost: u64,
    pub image: String,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub class: u32,
    pub search: String,
    pub items: Vec<RewardItem>,
    pub total: u64,
    pub cursor: Option<String>,
}

use crate::util::num;

/// Points balance.
pub async fn balance(client: &SteamClient, me: u64) -> Result<u64, String> {
    let v = webapi::get(client, "ILoyaltyRewardsService/GetSummary/v1", &[("steamid", me.to_string())]).await?;
    Ok(num(&v["summary"]["points"]))
}

fn parse_item(d: &serde_json::Value) -> Option<RewardItem> {
    let data = d.get("community_item_data")?;
    let appid = num(&d["appid"]) as u32;
    let s = |k: &str| data.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    // Which image to show: for animated items the "small" one is the animation (APNG ~1 MB), and the "large" one
    // is still (~30 KB); for emoticons the small one is only 18x18, the large one 54x54. Otherwise - the small one.
    let class = num(&d["community_item_class"]) as u32;
    let animated = data.get("animated").and_then(|a| a.as_bool()).unwrap_or(false);
    let (small, large) = (s("item_image_small"), s("item_image_large"));
    let file = if (animated || class == 4) && !large.is_empty() || small.is_empty() { large } else { small };
    let title = if s("item_title").is_empty() { s("item_name") } else { s("item_title") };
    Some(RewardItem {
        defid: num(&d["defid"]),
        appid,
        class: num(&d["community_item_class"]) as u32,
        item_type: num(&d["community_item_type"]) as u32,
        name: s("item_name"),
        title,
        cost: num(&d["point_cost"]),
        image: if file.is_empty() {
            String::new()
        } else if file.starts_with("http") {
            file
        } else {
            format!("https://shared.fastly.steamstatic.com/community_assets/images/items/{appid}/{file}")
        },
    })
}

/// Catalog page: item kind, search, next page cursor.
pub async fn query(client: &SteamClient, class: u32, search: &str, cursor: Option<&str>) -> Result<Page, String> {
    let mut p = vec![
        ("count", "60".to_string()),
        ("language", crate::lang::steam().to_string()),
        ("community_item_classes[0]", class.to_string()),
    ];
    if !search.trim().is_empty() {
        p.push(("search_term", search.trim().to_string()));
    }
    if let Some(c) = cursor {
        p.push(("cursor", c.to_string()));
    }
    let v = webapi::get(client, "ILoyaltyRewardsService/QueryRewardItems/v1", &p).await?;
    let items = v["definitions"].as_array().map(|a| a.iter().filter_map(parse_item).collect()).unwrap_or_default();
    let cursor = v.get("next_cursor").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(String::from);
    Ok(Page { class, search: search.trim().to_string(), items, total: num(&v["total_count"]), cursor })
}

/// What the account already has: profile items (appid, kind, number) and Steam inventory items
/// (stickers, chat effects, emoticons) by name.
#[derive(Clone, Debug, Default)]
pub struct Owned {
    pub profile: std::collections::HashSet<(u32, u32, u32)>,
    pub names: std::collections::HashSet<String>,
}

impl Owned {
    pub fn has(&self, it: &RewardItem) -> bool {
        match it.class {
            4 | 11 | 12 => self.names.contains(&it.name.to_lowercase()),
            _ => self.profile.contains(&(it.appid, it.class, it.item_type)),
        }
    }

    pub fn add(&mut self, it: &RewardItem) {
        self.profile.insert((it.appid, it.class, it.item_type));
        self.names.insert(it.name.to_lowercase());
    }
}

/// Owned: IPlayerService/GetProfileItemsOwned + the Steam inventory (753/6).
pub async fn owned(client: &SteamClient) -> Result<Owned, String> {
    let v = webapi::get(client, "IPlayerService/GetProfileItemsOwned/v1", &[]).await?;
    let mut o = Owned::default();
    for arr in v.as_object().into_iter().flat_map(|m| m.values()) {
        for i in arr.as_array().into_iter().flatten() {
            o.profile.insert((num(&i["appid"]) as u32, num(&i["item_class"]) as u32, num(&i["item_type"]) as u32));
        }
    }
    let web = webapi::web_session(client).await?;
    if let Ok(items) = aurelia::steam_client::inventory_via_lang(&web, 753, 6, "english").await {
        for i in items {
            o.names.insert(i.name.to_lowercase());
        }
    }
    Ok(o)
}

/// Buy an item for points.
pub async fn redeem(client: &SteamClient, defid: u64) -> Result<(), String> {
    let v = webapi::post(client, "ILoyaltyRewardsService/RedeemPoints/v1", &[("defid", defid.to_string())]).await?;
    if v.get("communityitemid").is_some() || v.get("bundle_community_item_ids").is_some() {
        Ok(())
    } else {
        Err(tr!("Steam не выдал предмет (уже есть или не хватает очков?).", "Steam did not give the item (already owned or not enough points?).").into())
    }
}

#[derive(Default)]
pub struct PointsUi {
    pub open: bool,
    pub class: u32,
    pub search: String,
    pub asked: Option<(u32, String)>,
    pub confirm: Option<u64>,
}

impl LauncherApp {
    pub(crate) fn open_points(&mut self) {
        self.close_extras(crate::View::Points);
        self.points_ui.open = true;
        if self.points_ui.class == 0 {
            self.points_ui.class = 12;
        }
        self.points_ui.asked = None;
        let _ = self.tx.send(Cmd::PointsBalance);
        let _ = self.tx.send(Cmd::PointsOwned);
        self.view = crate::View::Points;
    }

    pub(crate) fn points_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let st = &mut self.points_ui;
        let want = (st.class, st.search.trim().to_string());
        if st.asked.as_ref() != Some(&want) && net.account.is_some() {
            st.asked = Some(want.clone());
            let _ = self.tx.send(Cmd::PointsQuery { class: want.0, search: want.1, cursor: None });
        }
        use crate::theme;
        let shown_list = matches!(&net.points_page, Some(Ok(p)) if p.class == st.class && p.search == st.search.trim());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            theme::page_title(ui, tr!("Магазин очков", "Points Shop"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The balance, as the Steam points chip: a coin and the number.
                match net.points_balance {
                    Some(b) => {
                        ui.label(egui::RichText::new(crate::store::group_digits(b)).font(theme::semibold(17.0)).color(egui::Color32::WHITE));
                        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        coin(ui.painter(), r);
                        ui.label(egui::RichText::new(tr!("ВАШИ ОЧКИ", "YOUR POINTS")).size(11.5).color(theme::WEAK));
                    }
                    None => {
                        crate::spin::spinner(ui);
                    }
                }
            });
        });
        if let Some(m) = &net.points_msg {
            ui.label(m);
        }
        ui.add_space(8.0);
        let page = match &net.points_page {
            Some(Ok(p)) if shown_list => Some(p.clone()),
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                return;
            }
            _ => None,
        };
        let balance = net.points_balance.unwrap_or(0);
        let mut buy: Option<RewardItem> = None;
        let owned = net.points_owned.clone().unwrap_or_default();
        let mut more = false;
        ui.horizontal_top(|ui| {
            // Categories on the left, as the Points Shop menu.
            ui.vertical(|ui| {
                ui.set_width(190.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(tr!("ПРЕДМЕТЫ ПРОФИЛЯ И ЧАТА", "PROFILE AND CHAT ITEMS")).size(11.0).color(theme::DIM));
                ui.add_space(4.0);
                for (c, name) in CLASSES() {
                    if theme::side_item(ui, st.class == c, name, None).clicked() {
                        st.class = c;
                    }
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(tr!(
                        "Очки дают за покупки в Steam (100 очков за каждый доллар). Купленное появится в профиле и в чате.",
                        "Points are earned for purchases on Steam (100 points for every dollar). Purchases appear in the profile and in chat."
                    ))
                    .size(11.5)
                    .color(theme::DIM),
                );
                ui.add_space(6.0);
                if theme::text_link(ui, tr!("Магазин очков на сайте", "Points Shop on the website")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/points/shop"));
                }
            });
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(ui.available_size(), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::search_field(ui, &mut st.search, tr!("Поиск по названию или игре", "Search by name or game"), 300.0);
                    if net.points_busy && shown_list {
                        crate::spin::spinner(ui);
                    }
                    if let Some(p) = &page {
                        ui.label(egui::RichText::new(trf!("Найдено: {}", "Found: {}", p.total)).size(12.0).color(theme::DIM));
                    }
                });
                ui.add_space(8.0);
                let Some(page) = page.as_ref() else {
                    crate::spin::spinner(ui);
                    return;
                };
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        for it in &page.items {
                            ui.allocate_ui(egui::vec2(166.0, 222.0), |ui| {
                            egui::Frame::none().fill(egui::Color32::from_rgb(36, 43, 53)).rounding(3.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                                ui.set_width(150.0);
                                ui.set_height(206.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 3.0;
                                    // An image loads only when visible (the catalog has tens of thousands of items), and
                                    // downscaled: profile backgrounds come in 1920x1080.
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(150.0, 112.0), egui::Sense::hover());
                                    theme::vgradient_rect(ui.painter(), r, egui::Color32::from_rgb(52, 62, 76), egui::Color32::from_rgb(30, 36, 45));
                                    if !it.image.is_empty() && ui.is_rect_visible(r) {
                                        let url = crate::imgload::sized(&crate::imgload::still(&it.image), 240);
                                        let img = egui::Image::new(url).maintain_aspect_ratio(true);
                                        // Emoticons are small (54x54) - not stretched so they do not blur.
                                        let target = if it.class == 4 { r.shrink2(egui::vec2(48.0, 29.0)) } else { r.shrink(6.0) };
                                        ui.put(target, img.max_size(target.size()));
                                    }
                                    ui.add_space(2.0);
                                    ui.add(egui::Label::new(egui::RichText::new(&it.title).size(13.0).color(egui::Color32::WHITE)).truncate());
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
                                        coin(ui.painter(), r);
                                        ui.label(egui::RichText::new(crate::store::group_digits(it.cost)).size(12.5).color(theme::TEXT2));
                                    });
                                    ui.add_space(2.0);
                                    if owned.has(it) {
                                        ui.label(egui::RichText::new(tr!("Уже есть", "Owned")).size(12.5).color(theme::PLAY));
                                    } else if st.confirm == Some(it.defid) {
                                        ui.horizontal(|ui| {
                                            if theme::accent_button(ui, tr!("Купить", "Buy")).clicked() {
                                                buy = Some(it.clone());
                                            }
                                            if ui.button(tr!("Нет", "No")).clicked() {
                                                st.confirm = None;
                                            }
                                        });
                                    } else if ui
                                        .add_enabled(it.cost <= balance, egui::Button::new(tr!("Купить...", "Buy...")))
                                        .on_disabled_hover_text(tr!("Не хватает очков", "Not enough points"))
                                        .clicked()
                                    {
                                        st.confirm = Some(it.defid);
                                    }
                                });
                            });
                            });
                        }
                    });
                    ui.add_space(8.0);
                    if page.cursor.is_some() && (page.items.len() as u64) < page.total && ui.button(tr!("Показать еще", "Show more")).clicked() {
                        more = true;
                    }
                });
            });
        });
        if let Some(it) = buy {
            st.confirm = None;
            let _ = self.tx.send(Cmd::PointsRedeem(it));
        }
        if more {
            if let Some(Ok(page)) = &net.points_page {
                let _ = self.tx.send(Cmd::PointsQuery { class: page.class, search: page.search.clone(), cursor: page.cursor.clone() });
            }
        }
    }
}

/// The Steam points coin: a blue circle with a lighter ring.
fn coin(p: &egui::Painter, r: egui::Rect) {
    let c = r.center();
    let rad = r.width().min(r.height()) / 2.0;
    p.circle_filled(c, rad, egui::Color32::from_rgb(26, 159, 255));
    p.circle_stroke(c, rad * 0.62, egui::Stroke::new(1.4, egui::Color32::from_rgb(190, 228, 255)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_parse() {
        let d: serde_json::Value = serde_json::from_str(
            r#"{"defid":"123","appid":"2861720","point_cost":"1500","community_item_data":{"item_name":"x","item_title":"Snowball","item_image_small":"abc.png"}}"#,
        )
        .unwrap();
        let it = parse_item(&d).unwrap();
        assert_eq!((it.defid, it.cost, it.title.as_str()), (123, 1500, "Snowball"));
        assert!(it.image.ends_with("/items/2861720/abc.png"));
    }

    #[test]
    #[ignore]
    fn points_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            println!("points: {}", balance(&c, me).await.unwrap());
            for (cl, name) in CLASSES() {
                let p = query(&c, cl, "", None).await.unwrap();
                println!("{name}: {} total, on the page {}, cursor {}", p.total, p.items.len(), p.cursor.is_some());
                for it in p.items.iter().take(2) {
                    let r = ehttp::fetch_blocking(&ehttp::Request::get(&it.image));
                    println!("  {} -> {:?}", it.image, r.map(|r| (r.status, r.content_type().map(str::to_owned), r.bytes.len(), r.bytes.iter().take(8).copied().collect::<Vec<u8>>())));
                }
            }
            let p = query(&c, 11, "cat", None).await.unwrap();
            println!("stickers \"cat\": {}", p.total);
        });
    }
}

#[cfg(test)]
mod owned_live {
    /// Live test (read only): which chat effects and frames are marked "already owned".
    #[test]
    #[ignore]
    fn owned_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = super::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let o = super::owned(&c).await.unwrap();
            println!("profile items {}, in inventory {}", o.profile.len(), o.names.len());
            let frames: Vec<(u32, u32, u32)> = o.profile.iter().filter(|k| k.1 == 14).copied().collect();
            for (appid, _, _) in frames.iter().take(3) {
                let v = crate::webapi::get(&c, "ILoyaltyRewardsService/QueryRewardItems/v1", &[("count", "50".into()), ("community_item_classes[0]", "14".into()), ("appids[0]", appid.to_string())]).await.unwrap();
                let items: Vec<super::RewardItem> = v["definitions"].as_array().map(|a| a.iter().filter_map(super::parse_item).collect()).unwrap_or_default();
                println!("game frames: {} in the catalog, marked {}; my types {:?}, in the catalog {:?}", items.len(), items.iter().filter(|i| o.has(i)).count(), frames.iter().filter(|f| f.0 == *appid).map(|f| f.2).collect::<Vec<_>>(), items.iter().map(|i| (i.class, i.item_type)).collect::<Vec<_>>());
            }
            for cl in [12u32, 14] {
                let p = super::query(&c, cl, "", None).await.unwrap();
                let have: Vec<&str> = p.items.iter().filter(|i| o.has(i)).map(|i| i.title.as_str()).collect();
                println!("class {cl}: owned {} of {}: {:?}", have.len(), p.items.len(), have);
            }
        });
    }
}

#[cfg(test)]
mod tag_live {
    /// Stickers "Тег «...»" without an image: what Steam returns about them (raw first page data).
    #[test]
    #[ignore]
    fn tag_stickers_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let cl: u32 = std::env::var("VAPORUST_CLASS").ok().and_then(|v| v.parse().ok()).unwrap_or(11);
            let p = super::query(&c, cl, "", None).await.unwrap();
            for it in p.items.iter().take(3) {
                println!("{} | {} | {}", it.title, it.name, it.image);
            }
            let v = crate::webapi::get(
                &c,
                "ILoyaltyRewardsService/QueryRewardItems/v1",
                &[("community_item_classes[0]", std::env::var("VAPORUST_CLASS").unwrap_or("11".into())), ("count", "3".into()), ("language", "russian".into())],
            )
            .await
            .unwrap();
            println!("{}", serde_json::to_string_pretty(&v["definitions"][0]).unwrap_or_default());
        });
    }
}

