//! Steam "Group chats": groups and their channels on the left, channel messages and input on the right.
//! While the tab is open, the channel refreshes every 4 seconds.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use aurelia::steam_client::GroupMessage;
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct GroupsUi {
    pub open: bool,
    pub asked: bool,
    /// Selected channel: (group, channel).
    pub current: Option<(u64, u64)>,
    pub draft: String,
    pub polled: Option<Instant>,
    /// Whose names were already requested.
    pub asked_names: std::collections::HashSet<u64>,
    /// Animated stickers of the open channel.
    pub anims: crate::anim::Cache,
}

impl LauncherApp {
    pub(crate) fn open_groups(&mut self) {
        self.close_extras(crate::View::Groups);
        self.groups_ui.open = true;
        self.groups_ui.asked = false;
        self.view = crate::View::Groups;
    }

    /// Name by account number: friends, profiles, otherwise the number (and request the profile).
    fn group_sender(&mut self, account: u32, net: &NetState) -> String {
        let id = crate::social::STEAMID64_BASE + account as u64;
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
                ui.label(trf!("Не удалось загрузить групповые чаты: {e}", "Could not load group chats: {e}"));
                return;
            }
            Some(Ok(g)) => g.clone(),
        };
        if groups.is_empty() {
            ui.label(tr!("Вы не состоите ни в одном групповом чате Steam.", "You are not in any Steam group chat."));
            return;
        }
        // By default - the first channel of the first group.
        if self.groups_ui.current.is_none() {
            self.groups_ui.current = groups.first().map(|g| (g.group_id, // The group's default channel if it is in the list, otherwise the first one (comparing
                // numbers with max chose just the larger number).
                if g.channels.iter().any(|c| c.chat_id == g.default_chat) { g.default_chat } else { g.channels.first().map_or(g.default_chat, |c| c.chat_id) }));
        }
        let (gid, cid) = self.groups_ui.current.unwrap();
        // Refresh the open channel every 4 s (and right away on selection).
        if self.groups_ui.polled.is_none_or(|t| t.elapsed() >= Duration::from_secs(4)) {
            self.groups_ui.polled = Some(Instant::now());
            let _ = self.tx.send(Cmd::GroupMessages { group: gid, chat: cid });
        }
        ui.ctx().request_repaint_after(Duration::from_secs(4));

        egui::SidePanel::left("groups_list").resizable(true).default_width(220.0).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(tr!("Групповые чаты", "Group chats"));
                if ui.small_button(tr!("Обновить", "Refresh")).clicked() {
                    let _ = self.tx.send(Cmd::ChatGroups);
                }
                if ui.small_button(tr!("Создать...", "Create...")).clicked() {
                    self.net.lock().unwrap().action_msg = None;
                    self.social_ui.group_create = Some((String::new(), Default::default()));
                }
            });
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for g in &groups {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if !g.avatar_url.is_empty() {
                            ui.add(egui::Image::new(g.avatar_url.as_str()).fit_to_exact_size(egui::vec2(24.0, 24.0)));
                        }
                        // Group without a name: as in Steam - member names.
                        let title = if g.name.is_empty() {
                            let me = net.my_id.map(|m| (m - crate::social::STEAMID64_BASE) as u32);
                            let names: Vec<String> = g
                                .top_members
                                .iter()
                                .filter(|a| Some(**a) != me)
                                .take(3)
                                .map(|a| self.group_sender(*a, net))
                                .collect();
                            if names.is_empty() { tr!("Групповой чат", "Group chat").to_string() } else { names.join(", ") }
                        } else {
                            g.name.clone()
                        };
                        ui.strong(crate::richtext::fix(ui, &title));
                    });
                    if g.members_online > 0 {
                        ui.weak(trf!("в сети: {}", "online: {}", g.members_online));
                    }
                    for c in &g.channels {
                        let sel = (gid, cid) == (g.group_id, c.chat_id);
                        let name = if c.name.is_empty() { tr!("Общий", "General").to_string() } else { c.name.clone() };
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
                ui.label(trf!("Сообщения не загрузились: {e}", "Messages did not load: {e}"));
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
                        .hint_text(tr!("Сообщение, Enter - отправить", "Message, Enter - send"))
                        .desired_width(ui.available_width() - 100.0),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button(tr!("Отправить", "Send")).clicked()) && !self.groups_ui.draft.trim().is_empty() {
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
                ui.vertical_centered(|ui| ui.weak(tr!("В этом канале пока нет сообщений. Напишите первым.", "No messages in this channel yet. Write first.")));
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for (m, name) in msgs.iter().zip(&names) {
                    if m.deleted {
                        continue;
                    }
                    // The same pieces as in a friend chat: images, stickers, spoilers, links.
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(crate::fmt_day(m.time as u64));
                        ui.colored_label(egui::Color32::from_rgb(120, 170, 230), format!("{name}:"));
                        for seg in crate::chatfmt::parse(&m.text) {
                            crate::draw_seg(ui, seg, &mut self.groups_ui.anims, name);
                        }
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
    /// Live test (read only): own group chats and the latest messages of the first channel.
    #[test]
    #[ignore]
    fn groups_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let g = c.my_chat_groups().await.unwrap();
            c.activate_chat_groups(&g.iter().map(|x| x.group_id).collect::<Vec<_>>()).await.unwrap();
            println!("groups: {}, channels: {:?}", g.len(), g.iter().map(|x| x.channels.len()).collect::<Vec<_>>());
            for x in &g {
                println!("group: has name {}, icon {:?}, members listed {}", !x.name.is_empty(), x.avatar_url, x.top_members.len());
            }
            for x in &g {
                for ch in &x.channels {
                    println!("channel: has last message {}, time {}", !ch.last_message.is_empty(), ch.last_time);
                }
            }
            for x in &g {
                for ch in &x.channels {
                    let m = c.group_messages(x.group_id, ch.chat_id, 20).await;
                    println!("channel {}: {:?}", ch.chat_id % 1000, m.map(|v| v.len()).map_err(|e| format!("{e:#}")));
                }
            }
            if let Some(first) = g.first() {
                let ch = first.channels.first().map_or(first.default_chat, |c| c.chat_id);
                let m = c.group_messages(first.group_id, ch, 20).await.unwrap();
                println!("messages: {}, in order: {}", m.len(), m.windows(2).all(|w| w[0].time <= w[1].time));
            }
        });
    }
}
