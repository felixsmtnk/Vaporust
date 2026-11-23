//! "Friends" page (a profile section), as in Steam: tabs "My friends", "Friend requests",
//! "Sent" and an "Add friend" button.

use crate::net::{Cmd, NetState};
use crate::social::SocialCmd;
use crate::{theme, LauncherApp, View};
use eframe::egui;

#[derive(Default)]
pub struct FriendsPage {
    /// 0 - my friends, 1 - requests, 2 - sent.
    pub tab: u8,
    pub filter: String,
    /// Whose profiles were already requested (names and avatars of those in requests).
    pub asked: std::collections::HashSet<u64>,
    /// "Recent players" already requested.
    pub coplay_asked: bool,
}

/// Person in the list: id, name, avatar, status line and its color.
struct Person {
    id: u64,
    name: String,
    avatar: Option<String>,
    status: String,
    color: egui::Color32,
}

impl LauncherApp {
    pub(crate) fn open_friends_page(&mut self, tab: u8) {
        self.close_extras(View::Friends);
        self.view = View::Friends;
        self.friends_page.tab = tab;
    }

    pub(crate) fn friends_page_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // List snapshot: friends (3), incoming (2), outgoing (4).
        let mut lists: [Vec<Person>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut ask = Vec::new();
        {
            let roster = net.roster.read().unwrap();
            for f in roster.values() {
                // Only people: Steam groups (account type 7) also come in the list.
                if (f.steam_id >> 52) & 0xF != 1 {
                    continue;
                }
                let slot = match f.relationship {
                    3 => 0,
                    2 => 1,
                    4 => 2,
                    _ => continue,
                };
                let prof = net.profiles.get(&f.steam_id).and_then(|p| p.as_ref().ok());
                let name = f
                    .persona_name
                    .clone()
                    .or_else(|| prof.and_then(|p| p.persona_name.clone()))
                    .unwrap_or_else(|| {
                        ask.push(f.steam_id);
                        "...".into()
                    });
                // Own nickname for the friend first, as in the friends list.
                let name = match f.nickname.as_deref().filter(|n| !n.is_empty()) {
                    Some(n) => format!("{n} ({name})"),
                    None => name,
                };
                let avatar = f
                    .avatar_hash
                    .as_ref()
                    .map(|h| format!("https://avatars.fastly.steamstatic.com/{h}_medium.jpg"))
                    .or_else(|| prof.and_then(|p| p.avatar_url.clone()));
                let state = f.persona_state.unwrap_or(0);
                let (status, color) = match (state, &f.game_name) {
                    (_, Some(g)) if !g.is_empty() => (trf!("В игре: {g}", "In game: {g}"), crate::theme::IN_GAME),
                    (0, _) => (tr!("не в сети", "offline").into(), crate::theme::OFFLINE),
                    (2, _) => (tr!("не беспокоить", "busy").into(), crate::theme::ONLINE.gamma_multiply(0.7)),
                    (3 | 4, _) => (tr!("отошел", "away").into(), crate::theme::ONLINE.gamma_multiply(0.7)),
                    _ => (tr!("в сети", "online").into(), crate::theme::ONLINE),
                };
                let status = if slot == 0 { status } else { String::new() };
                lists[slot].push(Person { id: f.steam_id, name, avatar, status, color });
            }
        }
        for id in ask {
            if !net.profiles.contains_key(&id) && self.friends_page.asked.insert(id) {
                let _ = self.tx.send(Cmd::Social(SocialCmd::Profile(id)));
            }
        }
        // Friends: in game, online, offline; within each - by name.
        lists[0].sort_by(|a, b| {
            let rank = |p: &Person| if p.status.starts_with(tr!("В игре", "In game")) { 0 } else if p.status == tr!("не в сети", "offline") { 2 } else { 1 };
            rank(a).cmp(&rank(b)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            crate::theme::page_title(ui, tr!("Друзья", "Friends"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::theme::accent_button(ui, tr!("Добавить друга", "Add a friend")).clicked() {
                    self.social_ui.add_open = true;
                }
                if self.friends_page.tab == 0 {
                    crate::theme::search_field(ui, &mut self.friends_page.filter, tr!("Поиск друзей", "Search friends"), 200.0);
                }
            });
        });
        ui.add_space(10.0);
        let mut chat = None;
        let mut profile = None;
        ui.horizontal_top(|ui| {
            // The menu on the left, as on the Steam friends page.
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, label) in [
                    trf!("Мои друзья ({})", "Your friends ({})", lists[0].len()),
                    trf!("Заявки в друзья ({})", "Pending invites ({})", lists[1].len()),
                    trf!("Отправленные ({})", "Sent invites ({})", lists[2].len()),
                    tr!("Недавние игроки", "Recently played with").to_string(),
                ]
                .into_iter()
                .enumerate()
                {
                    if theme::side_item(ui, self.friends_page.tab == i as u8, &label, None).clicked() {
                        self.friends_page.tab = i as u8;
                    }
                }
            });
            ui.add_space(14.0);
            ui.vertical(|ui| {
                if self.friends_page.tab == 3 {
                    self.coplay_contents(ui, net);
                    return;
                }
                let tab = self.friends_page.tab.min(2) as usize;
                let filter = self.friends_page.filter.to_lowercase();
                let people: Vec<&Person> = lists[tab].iter().filter(|p| tab != 0 || filter.is_empty() || p.name.to_lowercase().contains(&filter)).collect();
                if people.is_empty() {
                    ui.label(
                        egui::RichText::new(match tab {
                            0 => tr!("Никого не найдено.", "Nobody found."),
                            1 => tr!("Новых заявок нет.", "No new requests."),
                            _ => tr!("Отправленных заявок нет.", "No sent requests."),
                        })
                        .color(theme::WEAK),
                    );
                    return;
                }
                // Friends in groups (in game, online, offline), as on the Steam page; requests in one group.
                let groups: Vec<(String, Vec<&Person>)> = if tab == 0 {
                    let in_game = tr!("В игре", "In game");
                    let off = tr!("не в сети", "offline");
                    let a: Vec<&Person> = people.iter().copied().filter(|p| p.status.starts_with(in_game)).collect();
                    let b: Vec<&Person> = people.iter().copied().filter(|p| !p.status.starts_with(in_game) && p.status != off).collect();
                    let c: Vec<&Person> = people.iter().copied().filter(|p| p.status == off).collect();
                    vec![(tr!("В игре", "In game").to_string(), a), (tr!("В сети", "Online").to_string(), b), (tr!("Не в сети", "Offline").to_string(), c)]
                } else {
                    vec![(String::new(), people)]
                };
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let card_w = 250.0;
                    let cols = ((ui.available_width() + 6.0) / (card_w + 6.0)).floor().max(1.0) as usize;
                    for (title, list) in groups.iter().filter(|g| !g.1.is_empty()) {
                        if !title.is_empty() {
                            theme::caps_title(ui, title, Some(list.len()));
                            ui.add_space(4.0);
                        }
                        for chunk in list.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for p in chunk {
                                    egui::Frame::none().fill(egui::Color32::from_rgb(32, 38, 46)).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                                        ui.set_width(card_w - 12.0);
                                        ui.horizontal(|ui| {
                                            let (ar, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
                                            match &p.avatar {
                                                Some(u) => {
                                                    egui::Image::new(u.as_str()).paint_at(ui, ar);
                                                }
                                                None => {
                                                    ui.painter().rect_filled(ar, 0.0, theme::SIDEBAR);
                                                }
                                            }
                                            if tab == 0 {
                                                ui.painter().rect_stroke(ar.expand(1.0), 0.0, egui::Stroke::new(1.5_f32, p.color));
                                            }
                                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                profile = Some(p.id);
                                            }
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 2.0;
                                                let name = crate::richtext::fix(ui, &p.name);
                                                let color = if tab == 0 { p.color } else { egui::Color32::WHITE };
                                                if ui
                                                    .add(egui::Label::new(egui::RichText::new(name).size(13.5).color(color)).truncate().sense(egui::Sense::click()))
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .on_hover_text(tr!("Профиль", "Profile"))
                                                    .clicked()
                                                {
                                                    profile = Some(p.id);
                                                }
                                                match tab {
                                                    0 => {
                                                        ui.horizontal(|ui| {
                                                            ui.add(egui::Label::new(egui::RichText::new(&p.status).size(11.5).color(p.color.gamma_multiply(0.85))).truncate());
                                                        });
                                                        if theme::text_link(ui, tr!("Написать", "Message")).clicked() {
                                                            chat = Some((p.id, p.name.clone()));
                                                        }
                                                    }
                                                    1 => {
                                                        ui.horizontal(|ui| {
                                                            if theme::accent_button(ui, tr!("Принять", "Accept")).clicked() {
                                                                let _ = self.tx.send(Cmd::Social(SocialCmd::AddFriend(p.id)));
                                                            }
                                                            if ui.small_button(tr!("Отклонить", "Decline")).clicked() {
                                                                let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(p.id)));
                                                            }
                                                        });
                                                    }
                                                    _ => {
                                                        if theme::text_link(ui, tr!("Отменить заявку", "Cancel invite")).clicked() {
                                                            let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(p.id)));
                                                        }
                                                    }
                                                }
                                            });
                                        });
                                    });
                                }
                            });
                            ui.add_space(6.0);
                        }
                        ui.add_space(8.0);
                    }
                    if let Some(m) = &net.friend_msg {
                        ui.label(egui::RichText::new(m).color(theme::WEAK));
                    }
                });
            });
        });
        let ctx = ui.ctx().clone();
        if let Some((id, name)) = chat {
            self.open_chat(&ctx, id, name, net);
        }
        if let Some(id) = profile {
            self.open_profile(id, net);
        }
    }
}
