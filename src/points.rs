//! «Очки Steam»: баланс и магазин очков (эффекты чата, стикеры, рамки и фоны профиля).
//! Web API ILoyaltyRewardsService: GetSummary, QueryRewardItems, RedeemPoints.
//! Покупка тратит очки (не деньги) и только после подтверждения.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// Виды предметов: (community_item_class, название).
pub const CLASSES: [(u32, &str); 7] = [
    (12, "Эффекты чата"),
    (11, "Стикеры"),
    (14, "Рамки аватара"),
    (15, "Анимированные аватары"),
    (3, "Фоны профиля"),
    (13, "Мини-фоны профиля"),
    (4, "Смайлики"),
];

#[derive(Clone, Debug, Default)]
pub struct RewardItem {
    pub defid: u64,
    pub appid: u32,
    /// Вид предмета (community_item_class) и его номер внутри игры (community_item_type).
    pub class: u32,
    pub item_type: u32,
    /// Внутреннее имя (item_name), например «Snowball».
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

fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}

/// Баланс очков.
pub async fn balance(client: &SteamClient, me: u64) -> Result<u64, String> {
    let v = webapi::get(client, "ILoyaltyRewardsService/GetSummary/v1", &[("steamid", me.to_string())]).await?;
    Ok(num(&v["summary"]["points"]))
}

fn parse_item(d: &serde_json::Value) -> Option<RewardItem> {
    let data = d.get("community_item_data")?;
    let appid = num(&d["appid"]) as u32;
    let s = |k: &str| data.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let file = if s("item_image_small").is_empty() { s("item_image_large") } else { s("item_image_small") };
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

/// Страница каталога: вид предметов, поиск, курсор следующей страницы.
pub async fn query(client: &SteamClient, class: u32, search: &str, cursor: Option<&str>) -> Result<Page, String> {
    let mut p = vec![
        ("count", "60".to_string()),
        ("language", "russian".to_string()),
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

/// Что уже есть на аккаунте: предметы профиля (appid, вид, номер) и предметы инвентаря Steam
/// (стикеры, эффекты чата, смайлики) по имени.
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

/// Купленное: IPlayerService/GetProfileItemsOwned + инвентарь Steam (753/6).
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

/// Купить предмет за очки.
pub async fn redeem(client: &SteamClient, defid: u64) -> Result<(), String> {
    let v = webapi::post(client, "ILoyaltyRewardsService/RedeemPoints/v1", &[("defid", defid.to_string())]).await?;
    if v.get("communityitemid").is_some() || v.get("bundle_community_item_ids").is_some() {
        Ok(())
    } else {
        Err("Steam не выдал предмет (уже есть или не хватает очков?).".into())
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
        ui.horizontal(|ui| {
            ui.heading("Очки Steam");
            match net.points_balance {
                Some(b) => ui.label(format!("на счету: {}", crate::store::group_digits(b))),
                None => crate::spin::spinner(ui),
            };
            ui.hyperlink_to("Магазин очков на сайте", "https://store.steampowered.com/points/shop");
        });
        if let Some(m) = &net.points_msg {
            ui.label(m);
        }
        ui.horizontal_wrapped(|ui| {
            for (c, name) in CLASSES {
                ui.selectable_value(&mut st.class, c, name);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.search).hint_text("Поиск по названию или игре").desired_width(260.0));
            if net.points_busy {
                crate::spin::spinner(ui);
            }
        });
        ui.weak("Очки дают за покупки в Steam (100 очков за каждый доллар). Купленное появится в профиле и в чате.");
        ui.separator();
        let page = match &net.points_page {
            Some(Ok(p)) if p.class == st.class && p.search == st.search.trim() => p.clone(),
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            _ => {
                crate::spin::spinner(ui);
                return;
            }
        };
        let balance = net.points_balance.unwrap_or(0);
        let mut buy: Option<RewardItem> = None;
        let owned = net.points_owned.clone().unwrap_or_default();
        let mut more = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.weak(format!("Найдено: {}", page.total));
            ui.horizontal_wrapped(|ui| {
                for it in &page.items {
                    ui.allocate_ui(egui::vec2(150.0, 190.0), |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(150.0);
                            if !it.image.is_empty() {
                                ui.add(egui::Image::new(crate::imgload::still(&it.image)).fit_to_exact_size(egui::vec2(110.0, 110.0)));
                            }
                            ui.add(egui::Label::new(egui::RichText::new(&it.title).small()).truncate());
                            ui.label(format!("{} очков", crate::store::group_digits(it.cost)));
                            if owned.has(it) {
                                ui.colored_label(egui::Color32::from_rgb(110, 200, 110), "Уже есть");
                            } else if st.confirm == Some(it.defid) {
                                ui.horizontal(|ui| {
                                    if ui.button("Купить").clicked() {
                                        buy = Some(it.clone());
                                    }
                                    if ui.button("Нет").clicked() {
                                        st.confirm = None;
                                    }
                                });
                            } else if ui
                                .add_enabled(it.cost <= balance, egui::Button::new("Купить..."))
                                .on_disabled_hover_text("Не хватает очков")
                                .clicked()
                            {
                                st.confirm = Some(it.defid);
                            }
                        });
                    });
                }
            });
            if page.cursor.is_some() && (page.items.len() as u64) < page.total && ui.button("Показать еще").clicked() {
                more = true;
            }
        });
        if let Some(it) = buy {
            st.confirm = None;
            let _ = self.tx.send(Cmd::PointsRedeem(it));
        }
        if more {
            let _ = self.tx.send(Cmd::PointsQuery { class: page.class, search: page.search.clone(), cursor: page.cursor.clone() });
        }
    }
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
            println!("очков: {}", balance(&c, me).await.unwrap());
            for (cl, name) in CLASSES {
                let p = query(&c, cl, "", None).await.unwrap();
                println!("{name}: {} всего, на странице {}, курсор {}", p.total, p.items.len(), p.cursor.is_some());
                for it in p.items.iter().take(2) {
                    let r = ehttp::fetch_blocking(&ehttp::Request::get(&it.image));
                    println!("  {} -> {:?}", it.image, r.map(|r| (r.status, r.content_type().map(str::to_owned), r.bytes.len(), r.bytes.iter().take(8).copied().collect::<Vec<u8>>())));
                }
            }
            let p = query(&c, 11, "cat", None).await.unwrap();
            println!("стикеры «cat»: {}", p.total);
        });
    }
}

#[cfg(test)]
mod owned_live {
    /// Живой тест (только чтение): какие эффекты чата и рамки отмечены «уже есть».
    #[test]
    #[ignore]
    fn owned_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = super::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let o = super::owned(&c).await.unwrap();
            println!("предметов профиля {}, в инвентаре {}", o.profile.len(), o.names.len());
            let frames: Vec<(u32, u32, u32)> = o.profile.iter().filter(|k| k.1 == 14).copied().collect();
            for (appid, _, _) in frames.iter().take(3) {
                let v = crate::webapi::get(&c, "ILoyaltyRewardsService/QueryRewardItems/v1", &[("count", "50".into()), ("community_item_classes[0]", "14".into()), ("appids[0]", appid.to_string())]).await.unwrap();
                let items: Vec<super::RewardItem> = v["definitions"].as_array().map(|a| a.iter().filter_map(super::parse_item).collect()).unwrap_or_default();
                println!("рамки игры: {} в каталоге, отмечено {}; мои типы {:?}, в каталоге {:?}", items.len(), items.iter().filter(|i| o.has(i)).count(), frames.iter().filter(|f| f.0 == *appid).map(|f| f.2).collect::<Vec<_>>(), items.iter().map(|i| (i.class, i.item_type)).collect::<Vec<_>>());
            }
            for cl in [12u32, 14] {
                let p = super::query(&c, cl, "", None).await.unwrap();
                let have: Vec<&str> = p.items.iter().filter(|i| o.has(i)).map(|i| i.title.as_str()).collect();
                println!("класс {cl}: есть {} из {}: {:?}", have.len(), p.items.len(), have);
            }
        });
    }
}
