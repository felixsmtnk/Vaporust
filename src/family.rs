//! «Семья Steam»: участники семейной группы, сколько игр каждый дает в общую библиотеку,
//! во что играли в семье. Только просмотр: приглашения и исключения - на сайте Steam
//! (это настройки аккаунта). Web API IFamilyGroupsService.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Member {
    pub steamid: u64,
    /// 1 - взрослый, 2 - ребенок.
    pub role: u32,
    pub joined: u64,
    /// Сколько игр участника в общей библиотеке.
    pub shared: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Play {
    pub steamid: u64,
    pub appid: u32,
    pub seconds: u64,
    pub latest: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Family {
    pub name: String,
    pub free_spots: u32,
    pub members: Vec<Member>,
    /// Названия игр общей библиотеки.
    pub names: HashMap<u32, String>,
    /// Игры в семье, последние первыми.
    pub plays: Vec<Play>,
    /// Не в семье.
    pub none: bool,
}

fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}

pub async fn load(client: &SteamClient, me: u64) -> Result<Family, String> {
    let g = webapi::get(client, "IFamilyGroupsService/GetFamilyGroupForUser/v1", &[("steamid", me.to_string())]).await?;
    if g.get("is_not_member_of_any_group").and_then(|x| x.as_bool()) == Some(true) {
        return Ok(Family { none: true, ..Default::default() });
    }
    let id = num(&g["family_groupid"]).to_string();
    let f = webapi::get(client, "IFamilyGroupsService/GetFamilyGroup/v1", &[("family_groupid", id.clone())]).await?;
    let apps = webapi::get(
        client,
        "IFamilyGroupsService/GetSharedLibraryApps/v1",
        &[("family_groupid", id.clone()), ("include_own", "true".into()), ("language", "russian".into())],
    )
    .await
    .unwrap_or_default();
    let play = webapi::post(client, "IFamilyGroupsService/GetPlaytimeSummary/v1", &[("family_groupid", id)]).await.unwrap_or_default();

    let mut shared: HashMap<u64, usize> = HashMap::new();
    let mut names = HashMap::new();
    for a in apps["apps"].as_array().into_iter().flatten() {
        names.insert(num(&a["appid"]) as u32, a["name"].as_str().unwrap_or("").to_string());
        for o in a["owner_steamids"].as_array().into_iter().flatten() {
            *shared.entry(num(o)).or_default() += 1;
        }
    }
    let members = f["members"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let steamid = num(&m["steamid"]);
            Member { steamid, role: num(&m["role"]) as u32, joined: num(&m["time_joined"]), shared: shared.get(&steamid).copied().unwrap_or(0) }
        })
        .collect();
    let mut plays: Vec<Play> = play["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Play {
            steamid: num(&e["steamid"]),
            appid: num(&e["appid"]) as u32,
            seconds: num(&e["seconds_played"]),
            latest: num(&e["latest_played"]),
        })
        .collect();
    plays.sort_by_key(|p| std::cmp::Reverse(p.latest));
    Ok(Family {
        name: f["name"].as_str().unwrap_or("").to_string(),
        free_spots: num(&f["free_spots"]) as u32,
        members,
        names,
        plays,
        none: false,
    })
}

#[derive(Default)]
pub struct FamilyUi {
    pub open: bool,
    pub asked: bool,
    /// Чьи профили уже запрошены ради имени.
    pub asked_names: std::collections::HashSet<u64>,
    /// Чьи названия игр уже запрошены.
    pub asked_apps: std::collections::HashSet<u32>,
}

impl LauncherApp {
    pub(crate) fn open_family(&mut self) {
        self.close_extras(crate::View::Family);
        self.family_ui.open = true;
        self.family_ui.asked = false;
        self.view = crate::View::Family;
    }

    /// Имя участника: друзья, профили, иначе номер (и запросить профиль).
    fn member_name(&mut self, id: u64, net: &NetState) -> String {
        if Some(id) == net.my_id {
            return "Вы".into();
        }
        if let Some(n) = net.roster.read().unwrap().get(&id).and_then(|f| f.persona_name.clone()) {
            return n;
        }
        if let Some(Ok(p)) = net.profiles.get(&id) {
            if let Some(n) = &p.persona_name {
                return n.clone();
            }
        }
        if self.family_ui.asked_names.insert(id) {
            let _ = self.tx.send(Cmd::Social(crate::social::SocialCmd::Profile(id)));
        }
        id.to_string()
    }

    pub(crate) fn family_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.family_ui.asked && net.account.is_some() {
            self.family_ui.asked = true;
            let _ = self.tx.send(Cmd::Family);
        }
        ui.horizontal(|ui| {
            ui.heading("Семья Steam");
            if ui.button("Обновить").clicked() {
                let _ = self.tx.send(Cmd::Family);
            }
            ui.hyperlink_to("Управление семьей на сайте", "https://store.steampowered.com/account/familymanagement");
        });
        let fam = match &net.family_group {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
                return;
            }
            Some(Ok(f)) => f.clone(),
        };
        if fam.none {
            ui.label("Вы не состоите в семье Steam. Создать семью или вступить в нее можно на сайте Steam.");
            return;
        }
        let mut open_profile = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(&fam.name).size(18.0).strong());
            ui.weak(format!("Свободных мест: {}", fam.free_spots));
            ui.add_space(6.0);
            ui.strong("Участники");
            egui::Grid::new("family_members").num_columns(4).striped(true).show(ui, |ui| {
                for m in &fam.members {
                    let name = self.member_name(m.steamid, net);
                    if ui.link(&name).on_hover_text("Профиль").clicked() {
                        open_profile = Some(m.steamid);
                    }
                    ui.label(if m.role == 2 { "ребенок" } else { "взрослый" });
                    ui.label(format!("игр в общей библиотеке: {}", m.shared));
                    ui.weak(format!("в семье с {}", crate::socialwin::fmt_date(m.joined)));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.strong("Во что играли в семье");
            ui.weak("Время в чужих играх из общей библиотеки (Steam считает его для каждого участника).");
            // Названия игр, которых нет ни в общей библиотеке, ни в кэше, - спросить у Steam один раз.
            let unknown: Vec<u32> = fam
                .plays
                .iter()
                .take(80)
                .map(|p| p.appid)
                .filter(|a| !fam.names.get(a).is_some_and(|n| !n.is_empty()) && !net.app_names.contains_key(a))
                .filter(|a| self.family_ui.asked_apps.insert(*a))
                .collect();
            if !unknown.is_empty() {
                let _ = self.tx.send(crate::net::Cmd::ResolveApps(unknown));
            }
            egui::Grid::new("family_plays").num_columns(4).striped(true).show(ui, |ui| {
                for p in fam.plays.iter().take(80) {
                    ui.label(self.member_name(p.steamid, net));
                    let game = fam
                        .names
                        .get(&p.appid)
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .or_else(|| net.app_names.get(&p.appid).cloned())
                        .unwrap_or_else(|| format!("Игра {}", p.appid));
                    ui.label(game);
                    ui.label(format!("{:.1} ч", p.seconds as f64 / 3600.0));
                    ui.weak(format!("последний раз {}", crate::socialwin::fmt_date(p.latest)));
                    ui.end_row();
                }
            });
        });
        if let Some(id) = open_profile {
            self.open_profile(id, net);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn family_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let f = super::load(&c, me).await.unwrap();
            println!(
                "в семье: {}, участников {}, свободно {}, игр в общей {}, записей игры {}, с общими играми у участников {:?}",
                !f.none,
                f.members.len(),
                f.free_spots,
                f.names.len(),
                f.plays.len(),
                f.members.iter().map(|m| m.shared).collect::<Vec<_>>()
            );
            assert!(f.none || !f.members.is_empty());
        });
    }
}
