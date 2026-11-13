//! Страница «Друзья» (раздел профиля), как в Steam: вкладки «Мои друзья», «Заявки в друзья»,
//! «Отправленные» и кнопка «Добавить друга».

use crate::net::{Cmd, NetState};
use crate::social::SocialCmd;
use crate::{theme, LauncherApp, View};
use eframe::egui;

#[derive(Default)]
pub struct FriendsPage {
    /// 0 - мои друзья, 1 - заявки, 2 - отправленные.
    pub tab: u8,
    pub filter: String,
    /// Чьи профили уже запрошены (имена и аватары тех, кто в заявках).
    pub asked: std::collections::HashSet<u64>,
}

/// Человек в списке: id, имя, аватар, строка статуса и ее цвет.
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
        // Снимок списка: друзья (3), входящие (2), исходящие (4).
        let mut lists: [Vec<Person>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut ask = Vec::new();
        {
            let roster = net.roster.read().unwrap();
            for f in roster.values() {
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
                let avatar = f
                    .avatar_hash
                    .as_ref()
                    .map(|h| format!("https://avatars.fastly.steamstatic.com/{h}_medium.jpg"))
                    .or_else(|| prof.and_then(|p| p.avatar_url.clone()));
                let state = f.persona_state.unwrap_or(0);
                let (status, color) = match (state, &f.game_name) {
                    (_, Some(g)) if !g.is_empty() => (format!("В игре: {g}"), egui::Color32::from_rgb(140, 200, 80)),
                    (0, _) => ("не в сети".into(), egui::Color32::GRAY),
                    (3 | 4, _) => ("отошел".into(), egui::Color32::from_rgb(110, 140, 170)),
                    _ => ("в сети".into(), egui::Color32::from_rgb(90, 170, 230)),
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
        // Друзья: в игре, в сети, не в сети; внутри - по имени.
        lists[0].sort_by(|a, b| {
            let rank = |p: &Person| if p.status.starts_with("В игре") { 0 } else if p.status == "не в сети" { 2 } else { 1 };
            rank(a).cmp(&rank(b)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Друзья").size(20.0).strong());
            ui.add_space(12.0);
            for (i, label) in [
                format!("Мои друзья ({})", lists[0].len()),
                format!("Заявки в друзья ({})", lists[1].len()),
                format!("Отправленные ({})", lists[2].len()),
            ]
            .into_iter()
            .enumerate()
            {
                let mut text = egui::RichText::new(label);
                if i == 1 && !lists[1].is_empty() {
                    text = text.color(egui::Color32::from_rgb(240, 200, 80));
                }
                if ui.selectable_label(self.friends_page.tab == i as u8, text).clicked() {
                    self.friends_page.tab = i as u8;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Добавить друга...").clicked() {
                    self.social_ui.add_open = true;
                }
                if self.friends_page.tab == 0 {
                    ui.add(egui::TextEdit::singleline(&mut self.friends_page.filter).hint_text("Поиск друзей").desired_width(180.0));
                }
            });
        });
        ui.separator();

        let tab = self.friends_page.tab.min(2) as usize;
        let filter = self.friends_page.filter.to_lowercase();
        let people: Vec<&Person> = lists[tab].iter().filter(|p| tab != 0 || filter.is_empty() || p.name.to_lowercase().contains(&filter)).collect();
        if people.is_empty() {
            ui.weak(match tab {
                0 => "Никого не найдено.",
                1 => "Новых заявок нет.",
                _ => "Отправленных заявок нет.",
            });
            return;
        }
        let mut chat = None;
        let mut profile = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Карточки в несколько столбцов по ширине окна.
            let card_w = 300.0;
            let cols = ((ui.available_width() + 8.0) / (card_w + 8.0)).floor().max(1.0) as usize;
            for chunk in people.chunks(cols) {
                ui.horizontal_top(|ui| {
                    for p in chunk {
                        egui::Frame::none().fill(theme::CARD).rounding(4.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                            ui.set_width(card_w - 16.0);
                            ui.horizontal(|ui| {
                                let (ar, resp) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::click());
                                match &p.avatar {
                                    Some(u) => {
                                        egui::Image::new(u.as_str()).rounding(3.0).paint_at(ui, ar);
                                    }
                                    None => {
                                        ui.painter().rect_filled(ar, 3.0, theme::SIDEBAR);
                                    }
                                }
                                if tab == 0 {
                                    ui.painter().rect_stroke(ar.expand(1.0), 3.0, egui::Stroke::new(1.5, p.color));
                                }
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    profile = Some(p.id);
                                }
                                ui.vertical(|ui| {
                                    let name = crate::richtext::fix(ui, &p.name);
                                    if ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate().sense(egui::Sense::click())).on_hover_text("Профиль").clicked() {
                                        profile = Some(p.id);
                                    }
                                    match tab {
                                        0 => {
                                            ui.add(egui::Label::new(egui::RichText::new(&p.status).small().color(p.color)).truncate());
                                            if ui.small_button("Написать").clicked() {
                                                chat = Some((p.id, p.name.clone()));
                                            }
                                        }
                                        1 => {
                                            ui.horizontal(|ui| {
                                                if ui.small_button("Принять").clicked() {
                                                    let _ = self.tx.send(Cmd::Social(SocialCmd::AddFriend(p.id)));
                                                }
                                                if ui.small_button("Отклонить").clicked() {
                                                    let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(p.id)));
                                                }
                                            });
                                        }
                                        _ => {
                                            if ui.small_button("Отменить заявку").clicked() {
                                                let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(p.id)));
                                            }
                                        }
                                    }
                                });
                            });
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
        if let Some(m) = &net.friend_msg {
            ui.weak(m);
        }
        let ctx = ui.ctx().clone();
        if let Some((id, name)) = chat {
            self.open_chat(&ctx, id, name, net);
        }
        if let Some(id) = profile {
            self.open_profile(id, net);
        }
    }
}
