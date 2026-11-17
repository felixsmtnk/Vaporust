//! "Steam Family": members of the family group, how many games each shares in the common library,
//! what was played in the family. View only: invites and removals are on the Steam site
//! (these are account settings). Web API IFamilyGroupsService.

use crate::net::{Cmd, NetState};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Member {
    pub steamid: u64,
    /// 1 - adult, 2 - child.
    pub role: u32,
    pub joined: u64,
    /// How many of the member's games are in the shared library.
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
    /// Names of shared library games.
    pub names: HashMap<u32, String>,
    /// Games played in the family, most recent first.
    pub plays: Vec<Play>,
    /// Not in a family.
    pub none: bool,
    /// Family group id and sent invites: (SteamID, role 1 adult / 2 child).
    pub id: String,
    pub pending: Vec<(u64, u32)>,
}

use crate::util::num;

pub async fn load(client: &SteamClient, me: u64) -> Result<Family, String> {
    let g = webapi::get(client, "IFamilyGroupsService/GetFamilyGroupForUser/v1", &[("steamid", me.to_string())]).await?;
    if g.get("is_not_member_of_any_group").and_then(|x| x.as_bool()) == Some(true) {
        return Ok(Family { none: true, ..Default::default() });
    }
    let id = num(&g["family_groupid"]).to_string();
    let id_copy = id.clone();
    let f = webapi::get(client, "IFamilyGroupsService/GetFamilyGroup/v1", &[("family_groupid", id.clone())]).await?;
    let apps = webapi::get(
        client,
        "IFamilyGroupsService/GetSharedLibraryApps/v1",
        &[("family_groupid", id.clone()), ("include_own", "true".into()), ("language", crate::lang::steam().into())],
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
        pending: f["pending_invites"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| (num(&p["steamid"]), num(&p["role"]) as u32))
            .collect(),
        id: id_copy,
    })
}

#[derive(Default)]
pub struct FamilyUi {
    /// Invite: (whom - SteamID, role 1 adult / 2 child, waiting for confirmation).
    pub invite: (u64, u32, bool),
    pub open: bool,
    pub asked: bool,
    /// Whose profiles were already requested for the name.
    pub asked_names: std::collections::HashSet<u64>,
    /// Whose game names were already requested.
    pub asked_apps: std::collections::HashSet<u32>,
}

impl LauncherApp {
    pub(crate) fn open_family(&mut self) {
        self.close_extras(crate::View::Family);
        self.family_ui.open = true;
        self.family_ui.asked = false;
        self.view = crate::View::Family;
    }

    /// Member name: friends, profiles, otherwise the number (and request the profile).
    fn member_name(&mut self, id: u64, net: &NetState) -> String {
        if Some(id) == net.my_id {
            return tr!("Вы", "You").into();
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
        crate::social::unknown_name()
    }

    pub(crate) fn family_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.family_ui.asked && net.account.is_some() {
            self.family_ui.asked = true;
            let _ = self.tx.send(Cmd::Family);
        }
        ui.horizontal(|ui| {
            ui.heading(tr!("Семья Steam", "Steam Family"));
            if ui.button(tr!("Обновить", "Refresh")).clicked() {
                let _ = self.tx.send(Cmd::Family);
            }
            ui.hyperlink_to(tr!("Управление семьей на сайте", "Manage the family on the website"), "https://store.steampowered.com/account/familymanagement");
        });
        let fam = match &net.family_group {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                return;
            }
            Some(Ok(f)) => f.clone(),
        };
        if fam.none {
            ui.label(tr!("Вы не состоите в семье Steam. Создать семью или вступить в нее можно на сайте Steam.", "You are not in a Steam Family. You can create or join a family on the Steam website."));
            return;
        }
        let mut open_profile = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(&fam.name).font(crate::theme::semibold(18.0)).color(egui::Color32::WHITE));
            ui.weak(trf!("Свободных мест: {}", "Free slots: {}", fam.free_spots));
            invite_block(ui, &fam, net, &self.tx, &mut self.family_ui.invite);
            ui.add_space(6.0);
            ui.strong(tr!("Участники", "Members"));
            egui::Grid::new("family_members").num_columns(4).striped(true).show(ui, |ui| {
                for m in &fam.members {
                    let name = self.member_name(m.steamid, net);
                    if ui.link(&name).on_hover_text(tr!("Профиль", "Profile")).clicked() {
                        open_profile = Some(m.steamid);
                    }
                    ui.label(if m.role == 2 { tr!("ребенок", "child") } else { tr!("взрослый", "adult") });
                    ui.label(trf!("игр в общей библиотеке: {}", "games in the shared library: {}", m.shared));
                    ui.weak(trf!("в семье с {}", "in the family since {}", crate::socialwin::fmt_date(m.joined)));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.strong(tr!("Во что играли в семье", "What the family played"));
            ui.weak(tr!("Время в чужих играх из общей библиотеки (Steam считает его для каждого участника).", "Time in other members' games from the shared library (Steam counts it for each member)."));
            // Game names that are neither in the shared library nor in the cache - ask Steam once.
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
                        .unwrap_or_else(|| trf!("Игра {}", "Game {}", p.appid));
                    ui.label(game);
                    ui.label(trf!("{:.1} ч", "{:.1} h", p.seconds as f64 / 3600.0));
                    ui.weak(trf!("последний раз {}", "last time {}", crate::socialwin::fmt_date(p.latest)));
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
                "in family: {}, members {}, free slots {}, shared games {}, playtime records {}, members with shared games {:?}",
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

/// Family invites: sent ones (with cancel) and a new invite to a friend.
fn invite_block(
    ui: &mut egui::Ui,
    fam: &Family,
    net: &NetState,
    tx: &tokio::sync::mpsc::UnboundedSender<Cmd>,
    inv: &mut (u64, u32, bool),
) {
    let name_of = |id: u64| {
        net.roster.read().unwrap().get(&id).and_then(|f| f.persona_name.clone()).unwrap_or_else(crate::social::unknown_name)
    };
    if !fam.pending.is_empty() {
        ui.add_space(4.0);
        ui.strong(tr!("Отправленные приглашения", "Sent invitations"));
        for (id, role) in &fam.pending {
            ui.horizontal(|ui| {
                ui.label(format!("{} ({})", name_of(*id), if *role == 2 { tr!("ребенок", "child") } else { tr!("взрослый", "adult") }));
                if crate::exp::small_button(ui, tr!("Отменить", "Cancel")).clicked() {
                    let _ = tx.send(Cmd::FamilyCancel { family: fam.id.clone(), steamid: *id });
                }
            });
        }
    }
    if fam.free_spots == 0 || fam.id.is_empty() {
        return;
    }
    ui.add_space(4.0);
    let mut friends: Vec<(u64, String)> = net
        .roster
        .read()
        .unwrap()
        .values()
        .filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1) && !fam.members.iter().any(|m| m.steamid == f.steam_id))
        .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(crate::social::unknown_name)))
        .collect();
    friends.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    ui.horizontal(|ui| {
        ui.label(tr!("Пригласить:", "Invite:"));
        let current = friends.iter().find(|(id, _)| *id == inv.0).map(|(_, n)| n.clone()).unwrap_or_else(|| tr!("выберите друга", "choose a friend").into());
        egui::ComboBox::from_id_salt("family_invite").selected_text(current).show_ui(ui, |ui| {
            for (id, n) in &friends {
                ui.selectable_value(&mut inv.0, *id, n);
            }
        });
        ui.radio_value(&mut inv.1, 1, tr!("взрослый", "adult"));
        ui.radio_value(&mut inv.1, 2, tr!("ребенок", "child"));
        if inv.1 == 0 {
            inv.1 = 1;
        }
        if !inv.2 {
            if ui.add_enabled(inv.0 != 0 && crate::exp::on(), egui::Button::new(tr!("Пригласить...", "Invite..."))).on_disabled_hover_text(crate::exp::HINT()).clicked() {
                inv.2 = true;
            }
        } else {
            if ui.button(tr!("Да, отправить приглашение", "Yes, send the invitation")).clicked() {
                let _ = tx.send(Cmd::FamilyInvite { family: fam.id.clone(), steamid: inv.0, role: inv.1 });
                *inv = (0, 1, false);
            }
            if ui.button(tr!("Отмена", "Cancel")).clicked() {
                inv.2 = false;
            }
        }
    });
    if inv.2 {
        ui.weak(tr!("Steam попросит подтвердить приглашение письмом на почту. Вступивший получит доступ к общей библиотеке.", "Steam asks to confirm the invitation by email. The new member gets access to the shared library."));
    }
    if let Some(m) = &net.action_msg {
        ui.label(m);
    }
}
