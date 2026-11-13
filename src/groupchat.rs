//! «Групповые чаты» Steam: слева группы и их каналы, справа сообщения канала и поле ввода.
//! Пока вкладка открыта, канал обновляется раз в 4 секунды.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use aurelia::steam_client::GroupMessage;
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct GroupsUi {
    pub open: bool,
    pub asked: bool,
    /// Выбранный канал: (группа, канал).
    pub current: Option<(u64, u64)>,
    pub draft: String,
    pub polled: Option<Instant>,
    /// Чьи имена уже запрошены.
    pub asked_names: std::collections::HashSet<u64>,
}

impl LauncherApp {
    pub(crate) fn open_groups(&mut self) {
        self.close_extras(crate::View::Groups);
        self.groups_ui.open = true;
        self.groups_ui.asked = false;
        self.view = crate::View::Groups;
    }

    /// Имя по номеру аккаунта: друзья, профили, иначе номер (и запросить профиль).
    fn group_sender(&mut self, account: u32, net: &NetState) -> String {
        let id = crate::social::STEAMID64_BASE + account as u64;
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
        if self.groups_ui.asked_names.insert(id) {
            let _ = self.tx.send(Cmd::Social(crate::social::SocialCmd::Profile(id)));
        }
        format!("{account}")
    }

    pub(crate) fn groups_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.groups_ui.asked && net.account.is_some() {
            self.groups_ui.asked = true;
            let _ = self.tx.send(Cmd::ChatGroups);
        }
        let groups = match &net.chat_groups {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить групповые чаты: {e}"));
                return;
            }
            Some(Ok(g)) => g.clone(),
        };
        if groups.is_empty() {
            ui.label("Вы не состоите ни в одном групповом чате Steam.");
            return;
        }
        // По умолчанию - первый канал первой группы.
        if self.groups_ui.current.is_none() {
            self.groups_ui.current = groups.first().map(|g| (g.group_id, g.default_chat.max(g.channels.first().map_or(0, |c| c.chat_id))));
        }
        let (gid, cid) = self.groups_ui.current.unwrap();
        // Обновлять открытый канал раз в 4 с (и сразу при выборе).
        if self.groups_ui.polled.is_none_or(|t| t.elapsed() >= Duration::from_secs(4)) {
            self.groups_ui.polled = Some(Instant::now());
            let _ = self.tx.send(Cmd::GroupMessages { group: gid, chat: cid });
        }
        ui.ctx().request_repaint_after(Duration::from_secs(4));

        egui::SidePanel::left("groups_list").resizable(true).default_width(220.0).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Групповые чаты");
                if ui.small_button("Обновить").clicked() {
                    let _ = self.tx.send(Cmd::ChatGroups);
                }
            });
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for g in &groups {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if !g.avatar_url.is_empty() {
                            ui.add(egui::Image::new(g.avatar_url.as_str()).fit_to_exact_size(egui::vec2(24.0, 24.0)));
                        }
                        // Группа без названия: как в Steam - имена участников.
                        let title = if g.name.is_empty() {
                            let me = net.my_id.map(|m| (m - crate::social::STEAMID64_BASE) as u32);
                            let names: Vec<String> = g
                                .top_members
                                .iter()
                                .filter(|a| Some(**a) != me)
                                .take(3)
                                .map(|a| self.group_sender(*a, net))
                                .collect();
                            if names.is_empty() { "Групповой чат".to_string() } else { names.join(", ") }
                        } else {
                            g.name.clone()
                        };
                        ui.strong(crate::richtext::fix(ui, &title));
                    });
                    if g.members_online > 0 {
                        ui.weak(format!("в сети: {}", g.members_online));
                    }
                    for c in &g.channels {
                        let sel = (gid, cid) == (g.group_id, c.chat_id);
                        let name = if c.name.is_empty() { "Общий".to_string() } else { c.name.clone() };
                        if ui.selectable_label(sel, format!("  # {name}")).clicked() && !sel {
                            self.groups_ui.current = Some((g.group_id, c.chat_id));
                            self.groups_ui.polled = None;
                        }
                    }
                }
            });
        });

        let msgs: Vec<GroupMessage> = match net.group_messages.get(&(gid, cid)) {
            Some(Ok(v)) => v.clone(),
            Some(Err(e)) => {
                ui.label(format!("Сообщения не загрузились: {e}"));
                Vec::new()
            }
            None => Vec::new(),
        };
        let mut send = None;
        egui::TopBottomPanel::bottom("group_input").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.groups_ui.draft)
                        .hint_text("Сообщение, Enter - отправить")
                        .desired_width(ui.available_width() - 100.0),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button("Отправить").clicked()) && !self.groups_ui.draft.trim().is_empty() {
                    send = Some(self.groups_ui.draft.trim().to_string());
                    edit.request_focus();
                }
            });
            if let Some(e) = &net.group_error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
            }
            ui.add_space(4.0);
        });
        let names: Vec<String> = msgs.iter().map(|m| self.group_sender(m.sender, net)).collect();
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if net.group_messages.get(&(gid, cid)).is_none() {
                crate::spin::spinner(ui);
            } else if msgs.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.weak("В этом канале пока нет сообщений. Напишите первым."));
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for (m, name) in msgs.iter().zip(&names) {
                    if m.deleted {
                        continue;
                    }
                    let (text, _img) = crate::chatfmt::preview(&m.text);
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(crate::fmt_day(m.time as u64));
                        ui.colored_label(egui::Color32::from_rgb(120, 170, 230), format!("{name}:"));
                        ui.add(egui::Label::new(text).wrap());
                    });
                }
            });
        });
        if let Some(t) = send {
            self.groups_ui.draft.clear();
            self.groups_ui.polled = Some(Instant::now());
            let _ = self.tx.send(Cmd::GroupSend { group: gid, chat: cid, text: t });
        }
    }
}

#[cfg(test)]
mod tests {
    /// Живой тест (только чтение): свои групповые чаты и последние сообщения первого канала.
    #[test]
    #[ignore]
    fn groups_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let g = c.my_chat_groups().await.unwrap();
            c.activate_chat_groups(&g.iter().map(|x| x.group_id).collect::<Vec<_>>()).await.unwrap();
            println!("групп: {}, каналов: {:?}", g.len(), g.iter().map(|x| x.channels.len()).collect::<Vec<_>>());
            for x in &g {
                println!("группа: имя есть {}, значок {:?}, участников в списке {}", !x.name.is_empty(), x.avatar_url, x.top_members.len());
            }
            for x in &g {
                for ch in &x.channels {
                    println!("канал: последнее сообщение есть {}, время {}", !ch.last_message.is_empty(), ch.last_time);
                }
            }
            for x in &g {
                for ch in &x.channels {
                    let m = c.group_messages(x.group_id, ch.chat_id, 20).await;
                    println!("канал {}: {:?}", ch.chat_id % 1000, m.map(|v| v.len()).map_err(|e| format!("{e:#}")));
                }
            }
            if let Some(first) = g.first() {
                let ch = first.channels.first().map_or(first.default_chat, |c| c.chat_id);
                let m = c.group_messages(first.group_id, ch, 20).await.unwrap();
                println!("сообщений: {}, по порядку: {}", m.len(), m.windows(2).all(|w| w[0].time <= w[1].time));
            }
        });
    }
}
