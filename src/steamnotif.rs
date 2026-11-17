//! Steam notifications (the bell in the Steam client): gifts, comments, new items,
//! friend requests, wishlist discounts, trade offers. Fetched via the Web API method
//! ISteamNotificationService/GetSteamNotifications with the login token, every 5 minutes.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// Where a click on a notification leads.
#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Url(String),
    /// Game page in the store.
    Store(u32),
    Wishlist,
    Inventory,
    /// Friends panel (requests).
    Friends,
    None,
}

#[derive(Clone, Debug)]
pub struct Notif {
    pub id: String,
    pub kind: u32,
    pub time: u64,
    pub read: bool,
    pub title: String,
    /// The game the notification is about (the name is filled in on display).
    pub appid: Option<u32>,
    pub count: u32,
    pub text: String,
    pub link: Link,
    /// Item from a "new item" notification: (inventory, context, item id).
    pub asset: Option<(u32, u32, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Notifs {
    pub unread: u32,
    pub list: Vec<Notif>,
}

fn s(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(x)) => x.clone(),
        Some(x) if !x.is_null() => x.to_string(),
        _ => String::new(),
    }
}

fn n(v: &serde_json::Value, k: &str) -> Option<u32> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_str()?.parse().ok())).map(|x| x as u32)
}

/// Parse one notification (body_data - JSON as a string, different for each kind).
pub fn parse_one(item: &serde_json::Value) -> Option<Notif> {
    let kind = n(item, "notification_type")?;
    let body: serde_json::Value = serde_json::from_str(item.get("body_data")?.as_str().unwrap_or("{}")).unwrap_or_default();
    let appid = n(&body, "appid")
        .or_else(|| n(&body, "app_id"))
        .filter(|a| *a != 0)
        .or_else(|| body.get("appids").and_then(|a| a.get(0)).and_then(|a| a.as_u64()).map(|a| a as u32).filter(|a| *a != 0));
    let count = n(&body, "count").unwrap_or(1);
    let (title, text, link): (String, String, Link) = match kind {
        2 => (tr!("Вам прислали подарок", "You received a gift").into(), String::new(), Link::Url("https://steamcommunity.com/my/inventory/#pending_gifts".into())),
        3 => (tr!("Новый комментарий", "New comment").into(), String::new(), Link::Url(comment_url(&body))),
        4 => {
            // Item: app_id - the inventory (753 - Steam), source_appid - the game it comes from.
            let src = n(&body, "source_appid").filter(|a| *a != 0);
            return Some(Notif {
                id: s(item, "notification_id"),
                kind,
                time: item.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0),
                read: item.get("read").and_then(|r| r.as_bool()).unwrap_or(false),
                title: tr!("Новый предмет в инвентаре", "New item in the inventory").into(),
                appid: src.or(appid),
                count,
                text: String::new(),
                link: Link::Inventory,
                asset: Some((appid.unwrap_or(753), n(&body, "context_id").unwrap_or(6), s(&body, "asset_id"))).filter(|a| !a.2.is_empty()),
            });
        }
        5 => (tr!("Заявка в друзья", "Friend request").into(), String::new(), Link::Friends),
        6 => (tr!("Началась распродажа Steam", "A Steam sale started").into(), String::new(), Link::Url("https://store.steampowered.com/".into())),
        7 => (tr!("Доступна предзагрузка", "Preload available").into(), String::new(), appid.map(Link::Store).unwrap_or(Link::None)),
        8 => {
            let title = if count > 1 {
                trf!("Скидки на игры из списка желаемого: {count}", "Discounts on wishlist games: {count}")
            } else {
                tr!("Скидка на игру из списка желаемого", "A discount on a wishlist game").to_string()
            };
            (title, String::new(), if count > 1 { Link::Wishlist } else { appid.map(Link::Store).unwrap_or(Link::Wishlist) })
        }
        9 => (tr!("Предложение обмена", "Trade offer").into(), String::new(), Link::Url("https://steamcommunity.com/my/tradeoffers/".into())),
        10 => {
            let t = s(&body, "title");
            let url = s(&body, "url");
            (
                if t.is_empty() { tr!("Сообщение Steam", "Steam message").into() } else { t },
                crate::store::plain_text(&s(&body, "body")),
                if url.is_empty() { Link::None } else { Link::Url(url) },
            )
        }
        12 => (tr!("Ваш ход в игре", "Your turn in a game").into(), String::new(), appid.map(Link::Store).unwrap_or(Link::None)),
        13 => return None, // messages - in the Vaporust chat
        14 => (tr!("Сообщение модератора", "Moderator message").into(), crate::store::plain_text(&s(&body, "body")), Link::None),
        16 => (tr!("Приглашение в семью Steam", "Steam Family invitation").into(), String::new(), Link::Url("https://store.steampowered.com/account/familymanagement".into())),
        17 | 19 => (tr!("Запрос покупки в семье", "Family purchase request").into(), String::new(), Link::Url("https://store.steampowered.com/account/familymanagement".into())),
        22 => (tr!("Запрошенная игра добавлена", "The requested game was added").into(), String::new(), appid.map(Link::Store).unwrap_or(Link::None)),
        26 => (tr!("Нужно подтверждение в мобильном приложении", "Confirmation needed in the mobile app").into(), String::new(), Link::None),
        27 => (tr!("Событие в игре", "In-game event").into(), String::new(), appid.map(Link::Store).unwrap_or(Link::None)),
        28 => (tr!("Приглашение в плейтест", "Playtest invitation").into(), String::new(), appid.map(Link::Store).unwrap_or(Link::None)),
        _ => (tr!("Уведомление Steam", "Steam notification").into(), String::new(), Link::None),
    };
    Some(Notif {
        id: s(item, "notification_id"),
        kind,
        time: item.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0),
        read: item.get("read").and_then(|r| r.as_bool()).unwrap_or(false),
        title,
        appid,
        count,
        text,
        link,
        asset: None,
    })
}

fn comment_url(body: &serde_json::Value) -> String {
    let owner = s(body, "owner_steam_id");
    if owner.is_empty() {
        "https://steamcommunity.com/my/commentnotifications/".into()
    } else {
        format!("https://steamcommunity.com/profiles/{owner}/#comments")
    }
}

pub fn parse(v: &serde_json::Value) -> Notifs {
    let mut list: Vec<Notif> = v
        .get("notifications")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter(|x| !x.get("hidden").and_then(|h| h.as_bool()).unwrap_or(false)).filter_map(parse_one).collect())
        .unwrap_or_default();
    list.sort_by(|a, b| b.time.cmp(&a.time));
    Notifs { unread: list.iter().filter(|n| !n.read).count() as u32, list }
}

pub async fn load(client: &SteamClient) -> Result<Notifs, String> {
    let v = webapi::get(
        client,
        "ISteamNotificationService/GetSteamNotifications/v1",
        &[("language", "russian".into()), ("include_read", "true".into())],
    )
    .await?;
    Ok(parse(&v))
}

/// Mark all as read (like the button in the Steam bell).
pub async fn mark_all_read(client: &SteamClient) -> Result<(), String> {
    webapi::post(client, "ISteamNotificationService/MarkNotificationsRead/v1", &[("mark_all_read", "true".into())])
        .await
        .map(|_| ())
}

impl LauncherApp {
    /// Every 5 minutes (and right after login) - Steam notifications; a popup about new unread ones.
    pub(crate) fn steam_notifs_tick(&mut self, net: &NetState) {
        if net.account.is_none() {
            return;
        }
        let due = self.notifs_at.is_none_or(|t| t.elapsed() >= crate::lite::secs(300, 1800));
        if due {
            self.notifs_at = Some(std::time::Instant::now());
            let _ = self.tx.send(Cmd::SteamNotifs);
        }
        // New unread ones (not present at the previous check) - one popup.
        if let Some(Ok(ns)) = &net.steam_notifs {
            let fresh: Vec<&Notif> = ns.list.iter().filter(|n| !n.read && !self.notifs_seen.contains(&n.id)).collect();
            let first_load = self.notifs_seen.is_empty();
            for n in &ns.list {
                self.notifs_seen.insert(n.id.clone());
            }
            if !first_load && !fresh.is_empty() && self.settings.notify_on && self.settings.notify_steam {
                let title = if fresh.len() == 1 {
                    self.notif_title(fresh[0], net)
                } else {
                    trf!("Новых уведомлений Steam: {}", "New Steam notifications: {}", fresh.len())
                };
                crate::popup::push(crate::popup::Kind::Info, "Steam", title);
            }
        }
    }

    /// Title with the game name.
    fn notif_title(&self, n: &Notif, net: &NetState) -> String {
        match n.appid {
            Some(a) if n.count <= 1 || n.kind != 8 => {
                let name = self
                    .games
                    .iter()
                    .find(|g| g.appid == a)
                    .map(|g| g.name.clone())
                    .or_else(|| net.owned.iter().find(|g| g.app_id == a).map(|g| g.name.clone()))
                    .or_else(|| net.app_names.get(&a).cloned());
                match name {
                    Some(name) => format!("{}: {name}", n.title),
                    None => n.title.clone(),
                }
            }
            _ => n.title.clone(),
        }
    }

    /// The "Notifications (N)" button in the top bar with the list.
    pub(crate) fn steam_notifs_button(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(Ok(ns)) = &net.steam_notifs else { return };
        let label = if ns.unread > 0 { trf!("Уведомления ({})", "Notifications ({})", ns.unread) } else { tr!("Уведомления", "Notifications").into() };
        let mut go: Option<Link> = None;
        let mut mark = false;
        // Names of games not in the library - ask Steam.
        let unknown: Vec<u32> = ns
            .list
            .iter()
            .filter_map(|n| n.appid)
            .filter(|a| !net.app_names.contains_key(a) && !net.owned.iter().any(|g| g.app_id == *a))
            .filter(|a| self.requested_apps.insert(*a))
            .collect();
        if !unknown.is_empty() {
            let _ = self.tx.send(Cmd::ResolveApps(unknown));
        }
        // Images of new items - from the inventory, once per session.
        static ICONS_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let invs: std::collections::BTreeSet<(u32, u32)> = ns
            .list
            .iter()
            .filter_map(|n| n.asset.as_ref())
            .filter(|a| !net.notif_icons.contains_key(&a.2))
            .map(|a| (a.0, a.1))
            .collect();
        if !invs.is_empty() && !ICONS_ASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let _ = self.tx.send(Cmd::NotifIcons(invs.into_iter().collect()));
        }
        // The bell, as in Steam: the unread count in a red circle.
        // A cell like the window buttons next to it (46x34), a thin line, highlight on hover.
        let bell = egui::Button::new("").frame(false).min_size(egui::vec2(36.0, 28.0));
        let bell_resp = egui::menu::menu_custom_button(ui, bell, |ui| {
            ui.set_min_width(340.0);
            ui.horizontal(|ui| {
                ui.strong(tr!("Уведомления Steam", "Steam notifications"));
                if ns.unread > 0 && ui.small_button(tr!("Прочитать все", "Mark all read")).clicked() {
                    mark = true;
                }
            });
            ui.separator();
            if ns.list.is_empty() {
                ui.weak(tr!("Уведомлений нет.", "No notifications."));
            }
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for n in ns.list.iter().take(40) {
                    let title = self.notif_title(n, net);
                    let text = if n.read { egui::RichText::new(&title) } else { egui::RichText::new(&title).strong() };
                    // As in Steam: an image on the left (of the game the notification is about), text on the right.
                    let row = ui.horizontal(|ui| {
                        let size = egui::vec2(86.0, 40.0);
                        // A new item - its image from the inventory (on a dark backing).
                        let mut item_drawn = false;
                        if let Some(icon) = n.asset.as_ref().and_then(|a| net.notif_icons.get(&a.2)) {
                            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            ui.painter().rect_filled(rect, 3.0, crate::theme::CARD);
                            let sq = egui::Rect::from_center_size(rect.center(), egui::vec2(38.0, 38.0));
                            egui::Image::new(icon.as_str()).paint_at(ui, sq);
                            item_drawn = true;
                        }
                        // No image (for Steam service apps, e.g. sales) - a caption instead of
                        // an error icon.
                        let img = n.appid.filter(|_| !item_drawn).map(|a| {
                            egui::Image::new(format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg"))
                                .fit_to_exact_size(size)
                                .rounding(3.0)
                        });
                        let failed = img.as_ref().is_some_and(|i| i.load_for_size(ui.ctx(), size).is_err());
                        match img.filter(|_| !failed) {
                            Some(img) => {
                                ui.add(img);
                            }
                            None if item_drawn => {}
                            None => {
                                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                ui.painter().rect_filled(rect, 3.0, crate::theme::CARD);
                                let mark = match n.kind {
                                    2 => tr!("подарок", "gift"),
                                    3 => tr!("комментарий", "comment"),
                                    5 => tr!("друг", "friend"),
                                    9 => tr!("обмен", "trade"),
                                    _ => "Steam",
                                };
                                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, mark, egui::FontId::proportional(11.0), crate::theme::WEAK);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.set_max_width(250.0);
                            ui.add(egui::Label::new(text).wrap());
                            if !n.text.is_empty() {
                                ui.add(egui::Label::new(egui::RichText::new(n.text.chars().take(200).collect::<String>()).weak().small()).wrap());
                            }
                            ui.weak(crate::socialwin::fmt_date(n.time));
                        });
                    });
                    let resp = ui.interact(row.response.rect, ui.id().with(("notif", &n.id)), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_stroke(row.response.rect.expand(2.0), 3.0, egui::Stroke::new(1.0, crate::theme::ACCENT));
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        go = Some(n.link.clone());
                        ui.close_menu();
                    }
                    ui.add_space(6.0);
                }
            });
        });
        let r = bell_resp.response.rect;
        if bell_resp.response.hovered() {
            ui.painter().rect_filled(r, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        draw_bell(ui.painter(), r, ui.visuals().text_color(), ns.unread);
        bell_resp.response.on_hover_text(label);
        if mark {
            let _ = self.tx.send(Cmd::SteamNotifsRead);
        }
        match go {
            Some(Link::Url(u)) => ui.ctx().open_url(egui::OpenUrl::new_tab(u)),
            Some(Link::Store(a)) => self.open_store(Some(a), false),
            Some(Link::Wishlist) => self.open_wishlist(),
            Some(Link::Inventory) => self.open_inventory(false),
            Some(Link::Friends) => self.show_friends = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kinds() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"unread_count":2,"notifications":[
            {"notification_id":"1","notification_type":8,"timestamp":100,"read":false,"body_data":"{\"appid\":70,\"appids\":[70],\"count\":1}"},
            {"notification_id":"2","notification_type":8,"timestamp":200,"read":false,"body_data":"{\"appid\":70,\"appids\":[70,10],\"count\":2}"},
            {"notification_id":"3","notification_type":4,"timestamp":50,"read":true,"body_data":"{\"app_id\":753,\"source_appid\":730,\"context_id\":\"6\",\"asset_id\":\"9\"}"},
            {"notification_id":"4","notification_type":13,"timestamp":60,"read":false,"body_data":"{}"},
            {"notification_id":"5","notification_type":9,"timestamp":70,"read":true,"hidden":true,"body_data":"{}"}]}"#,
        )
        .unwrap();
        let ns = parse(&v);
        assert_eq!(ns.list.len(), 3, "chat messages and hidden ones are not shown");
        assert_eq!(ns.unread, 2);
        assert_eq!(ns.list[0].id, "2");
        assert_eq!(ns.list[0].link, Link::Wishlist);
        assert_eq!(ns.list[1].link, Link::Store(70));
        assert_eq!(ns.list[2].appid, Some(730));
        assert_eq!(ns.list[2].link, Link::Inventory);
    }

    #[test]
    #[ignore]
    fn notifs_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let ns = load(&c).await.unwrap();
            println!("total {}, unread {}", ns.list.len(), ns.unread);
            for n in ns.list.iter().take(5) {
                println!("{} | {:?} | {:?}", n.title, n.appid, n.link);
            }
        });
    }
}

/// Bell icon (drawn with lines, no font) and a circle with the unread count.
fn draw_bell(p: &egui::Painter, r: egui::Rect, color: egui::Color32, unread: u32) {
    let c = r.center();
    // A thin 1-point line and the same size as the "minimize / close" icons next to it.
    let s = egui::Stroke::new(1.0, color);
    // The bell's dome and skirt.
    let mut pts = Vec::new();
    for i in 0..=12 {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
        pts.push(egui::pos2(c.x + 4.0 * a.cos(), c.y - 1.5 + 4.0 * a.sin()));
    }
    pts.push(egui::pos2(c.x + 4.0, c.y + 2.5));
    pts.push(egui::pos2(c.x + 5.5, c.y + 4.0));
    pts.push(egui::pos2(c.x - 5.5, c.y + 4.0));
    pts.push(egui::pos2(c.x - 4.0, c.y + 2.5));
    pts.push(pts[0]);
    p.add(egui::Shape::line(pts, s));
    // Clapper and loop.
    p.line_segment([egui::pos2(c.x - 1.5, c.y + 5.5), egui::pos2(c.x + 1.5, c.y + 5.5)], s);
    p.line_segment([egui::pos2(c.x, c.y - 5.5), egui::pos2(c.x, c.y - 6.5)], s);
    if unread > 0 {
        let b = egui::pos2(c.x + 7.5, c.y - 6.0);
        let text = if unread > 9 { "9+".to_string() } else { unread.to_string() };
        p.circle_filled(b, 6.5, egui::Color32::from_rgb(200, 60, 50));
        p.text(b, egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(9.5), egui::Color32::WHITE);
    }
}
