//! Steam community group page inside the program: name, avatar, description, members
//! (total, online, in game, in chat); "Join" and "Leave group" (with confirmation).
//! Data - steamcommunity.com/groups/<url>/memberslistxml/?xml=1 (no login).

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use aurelia::steam_client::SteamClient;
use eframe::egui;

#[derive(Clone, Debug, Default)]
pub struct GroupInfo {
    pub id64: u64,
    pub name: String,
    /// Short URL (groups/<url>).
    pub url: String,
    pub headline: String,
    pub summary: String,
    pub avatar: String,
    pub members: u64,
    pub in_chat: u64,
    pub in_game: u64,
    pub online: u64,
}

#[derive(Default)]
pub struct GroupUi {
    /// Open group: page URL (https://steamcommunity.com/groups/<url>).
    pub url: Option<String>,
    /// Waiting for confirmation: true - join, false - leave.
    pub confirm: Option<bool>,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn cdata(s: &str) -> String {
    s.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim().to_string()
}

pub fn parse(xml: &str) -> Option<GroupInfo> {
    let d = between(xml, "<groupDetails>", "</groupDetails>")?;
    let num = |k: &str| between(d, &format!("<{k}>"), &format!("</{k}>")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    let text = |k: &str| between(d, &format!("<{k}>"), &format!("</{k}>")).map(cdata).unwrap_or_default();
    let summary = crate::store::plain_text(&text("summary").replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n"));
    Some(GroupInfo {
        id64: between(xml, "<groupID64>", "</groupID64>").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
        name: text("groupName"),
        url: text("groupURL"),
        headline: text("headline"),
        summary,
        avatar: text("avatarFull"),
        members: num("memberCount"),
        in_chat: num("membersInChat"),
        in_game: num("membersInGame"),
        online: num("membersOnline"),
    })
}

/// Group page URL -> XML URL.
fn xml_url(url: &str) -> String {
    format!("{}/memberslistxml/?xml=1&l={}", url.trim_end_matches('/'), crate::lang::STEAM)
}

pub async fn load(url: &str) -> Result<GroupInfo, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let t = http.get(xml_url(url)).send().await.map_err(|e| format!("{e:#}"))?.text().await.map_err(|e| format!("{e:#}"))?;
    parse(&t).ok_or_else(|| "Группа не найдена.".to_string())
}

/// Join (join = true) or leave the group. Visible to others (the profile's group list).
pub async fn membership(client: &SteamClient, g: &GroupInfo, join: bool) -> Result<(), String> {
    let web = crate::webapi::web_session(client).await?;
    let sid = web.sessionid().to_string();
    let page = format!("https://steamcommunity.com/groups/{}", g.url);
    let r = if join {
        web.post_form(&format!("{page}/"), &[("action", "join".into()), ("sessionID", sid)], &page).await
    } else {
        let me = client.my_steam_id().ok_or("нет входа")?;
        web.post_form(
            &format!("https://steamcommunity.com/profiles/{me}/home_process"),
            &[("sessionID", sid), ("action", "leaveGroup".into()), ("groupId", g.id64.to_string())],
            &page,
        )
        .await
    };
    r.map(|_| ()).map_err(|e| format!("{e:#}"))
}

impl LauncherApp {
    pub(crate) fn open_group(&mut self, url: String) {
        self.close_extras(crate::View::Group);
        self.group_ui = GroupUi { url: Some(url.clone()), confirm: None };
        self.view = crate::View::Group;
        let _ = self.tx.send(Cmd::GroupInfo(url));
        if let Some(me) = self.net.lock().unwrap().my_id {
            let _ = self.tx.send(Cmd::ProfileExtra(me));
        }
    }

    pub(crate) fn group_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(url) = self.group_ui.url.clone() else { return };
        let Some(r) = net.groups_info.get(&url) else {
            crate::spin::spinner(ui);
            return;
        };
        let g = match r {
            Ok(g) => g.clone(),
            Err(e) => {
                ui.label(format!("Не удалось загрузить группу: {e}"));
                return;
            }
        };
        // Am I a member: from the group list of my profile.
        let member = net
            .my_id
            .and_then(|me| net.profile_extra.get(&me))
            .and_then(|x| x.as_ref().ok())
            .map(|x| x.groups.iter().any(|m| m.url.trim_end_matches('/').eq_ignore_ascii_case(url.trim_end_matches('/'))));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if !g.avatar.is_empty() {
                    ui.add(egui::Image::new(g.avatar.as_str()).fit_to_exact_size(egui::vec2(110.0, 110.0)).rounding(3.0));
                }
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(crate::richtext::fix(ui, &g.name)).size(24.0).color(egui::Color32::WHITE));
                    if !g.headline.is_empty() {
                        ui.label(crate::richtext::fix(ui, &g.headline));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let d = crate::store::group_digits;
                        ui.label(egui::RichText::new(format!("{} участников", d(g.members))).strong());
                        ui.label(egui::RichText::new(format!("{} в игре", d(g.in_game))).color(crate::theme::PLAY_TEXT));
                        ui.label(egui::RichText::new(format!("{} в сети", d(g.online))).color(egui::Color32::from_rgb(90, 170, 230)));
                        ui.weak(format!("{} в чате", d(g.in_chat)));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        match self.group_ui.confirm {
                            None => match member {
                                Some(true) => {
                                    if ui.button("Выйти из группы...").clicked() {
                                        self.group_ui.confirm = Some(false);
                                    }
                                }
                                Some(false) => {
                                    if crate::exp::button(ui, "Вступить в группу...").clicked() {
                                        self.group_ui.confirm = Some(true);
                                    }
                                }
                                None => {}
                            },
                            Some(join) => {
                                ui.label(if join {
                                    "Вступить? Группа появится в вашем профиле."
                                } else {
                                    "Выйти из группы? Если она закрытая, вернуться можно только по приглашению."
                                });
                                if crate::exp::button(ui, if join { "Вступить" } else { "Выйти" }).clicked() {
                                    let _ = self.tx.send(Cmd::GroupMembership { url: url.clone(), info: g.clone(), join });
                                    self.group_ui.confirm = None;
                                }
                                if ui.button("Отмена").clicked() {
                                    self.group_ui.confirm = None;
                                }
                            }
                        }
                        if ui.button("Страница в браузере").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                        }
                    });
                    if let Some(m) = net.group_msg.as_ref() {
                        ui.weak(m);
                    }
                });
            });
            ui.add_space(10.0);
            if !g.summary.trim().is_empty() {
                egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(crate::richtext::fix(ui, g.summary.trim()));
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_xml() {
        let xml = "<memberList><groupID64>103582791429521408</groupID64><groupDetails><groupName><![CDATA[Valve]]></groupName>\
            <groupURL><![CDATA[Valve]]></groupURL><headline><![CDATA[Valve]]></headline><summary><![CDATA[Строка 1<br>Строка 2]]></summary>\
            <avatarFull><![CDATA[https://a/full.jpg]]></avatarFull><memberCount>12345</memberCount><membersInChat>1</membersInChat>\
            <membersInGame>20</membersInGame><membersOnline>300</membersOnline></groupDetails></memberList>";
        let g = super::parse(xml).unwrap();
        assert_eq!(g.id64, 103582791429521408);
        assert_eq!(g.name, "Valve");
        assert_eq!(g.members, 12345);
        assert_eq!(g.online, 300);
        assert!(g.summary.contains("Строка 1") && g.summary.contains("Строка 2"));
    }

    #[test]
    #[ignore]
    fn group_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let g = rt.block_on(super::load("https://steamcommunity.com/groups/steamuniverse")).unwrap();
        println!("{} | {} | участников {} в сети {} | {}", g.name, g.headline, g.members, g.online, g.summary.chars().take(80).collect::<String>());
    }
}
