//! The achievements page of a game.

use crate::*;

impl LauncherApp {
    /// Game achievements - a page of the main window ("Library" section).
    pub(crate) fn achievements_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some((appid, name)) = self.ach_open.clone() else {
            return;
        };
        let mut open = true;
        let mut ask_friend: Option<u64> = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(egui::RichText::new(tr!("< К игре", "< Back to game")).size(12.5).color(theme::TEXT2)).frame(false)).clicked() {
                open = false;
            }
            ui.label(egui::RichText::new(trf!("Достижения: {name}", "Achievements: {name}")).font(theme::semibold(20.0)).color(egui::Color32::WHITE));
        });
        ui.separator();
        let st = &mut self.ach_state;
        egui::Frame::none()
            .show(ui, |ui| match net.achievements.get(&appid) {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.label(trf!("Ошибка: {e}", "Error: {e}"));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label(tr!("У этой игры нет достижений.", "This game has no achievements."));
                }
                Some(Ok(list)) => {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    let pct = done as f32 / list.len().max(1) as f32;
                    ui.label(egui::RichText::new(trf!("Получено {done} из {} ({:.0}%)", "You've unlocked {done} of {} ({:.0}%)", list.len(), pct * 100.0)).size(13.0).color(theme::TEXT));
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(520.0), 6.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 3.0, egui::Color32::from_rgb(14, 20, 27));
                    let mut fill = bar;
                    fill.set_width(bar.width() * pct);
                    ui.painter().rect_filled(fill, 3.0, theme::ACCENT);
                    ui.add_space(4.0);
                    // Order, filter, comparison with a friend.
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("ach_sort")
                            .selected_text(ACH_SORTS()[st.sort as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_SORTS().iter().enumerate() {
                                    ui.selectable_value(&mut st.sort, i as u8, *s);
                                }
                            });
                        egui::ComboBox::from_id_salt("ach_filter")
                            .selected_text(ACH_FILTERS()[st.filter as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_FILTERS().iter().enumerate() {
                                    ui.selectable_value(&mut st.filter, i as u8, *s);
                                }
                            });
                        let roster = net.roster.read().unwrap();
                        let mut friends: Vec<(u64, String)> = roster
                            .values()
                            .filter(|f| f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)
                            .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(crate::social::unknown_name)))
                            .collect();
                        drop(roster);
                        friends.sort_by_key(|f| f.1.to_lowercase());
                        let current = st
                            .friend
                            .and_then(|id| friends.iter().find(|f| f.0 == id))
                            .map(|f| trf!("Сравнить с: {}", "Compare with: {}", f.1))
                            .unwrap_or_else(|| tr!("Сравнить с другом", "Compare with a friend").into());
                        egui::ComboBox::from_id_salt("ach_friend").selected_text(current).height(320.0).show_ui(ui, |ui| {
                            if ui.selectable_label(st.friend.is_none(), tr!("Без сравнения", "No comparison")).clicked() {
                                st.friend = None;
                            }
                            for (id, n) in &friends {
                                if ui.selectable_label(st.friend == Some(*id), n).clicked() {
                                    st.friend = Some(*id);
                                    ask_friend = Some(*id);
                                }
                            }
                        });
                    });
                    // Friend achievements: ✔/✖ by API name.
                    let friend_map: Option<HashMap<&str, bool>> = match st.friend.map(|f| net.friend_achievements.get(&(appid, f))) {
                        Some(Some(Ok(v))) => {
                            let got = v.iter().filter(|a| a.unlocked).count();
                            if got == 0 {
                                ui.weak(tr!("У друга нет полученных достижений (не играл или скрыл данные об играх).", "The friend has no unlocked achievements (did not play or hides game details)."));
                            } else {
                                ui.label(trf!("У друга {got} из {}", "The friend has {got} of {}", v.len()));
                            }
                            Some(v.iter().map(|a| (a.api_name.as_str(), a.unlocked)).collect())
                        }
                        Some(Some(Err(e))) => {
                            ui.weak(trf!("Достижения друга не получены: {e}", "Could not get the friend's achievements: {e}"));
                            None
                        }
                        Some(None) => {
                            crate::spin::spinner(ui);
                            None
                        }
                        None => None,
                    };
                    ui.separator();
                    let mut rows: Vec<&aurelia::steam_client::GameAchievement> = list
                        .iter()
                        .filter(|a| match st.filter {
                            1 => a.unlocked,
                            2 => !a.unlocked,
                            _ => true,
                        })
                        .collect();
                    match st.sort {
                        1 => rows.sort_by(|a, b| a.global_percent.total_cmp(&b.global_percent)),
                        2 => rows.sort_by(|a, b| b.global_percent.total_cmp(&a.global_percent)),
                        3 => rows.sort_by_key(|a| std::cmp::Reverse(a.unlock_time.unwrap_or(0))),
                        _ => {}
                    }
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for a in rows {
                            // A row as on the Steam achievements page: icon, name and description, on the
                            // right the unlock date and how many players have it (with a small bar).
                            egui::Frame::none().fill(if a.unlocked { theme::CARD } else { egui::Color32::from_rgb(33, 38, 46) }).rounding(2.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let icon = if a.unlocked { &a.icon_unlocked } else { &a.icon_locked };
                                    if !icon.is_empty() {
                                        ui.add(egui::Image::new(icon.as_str()).fit_to_exact_size(egui::vec2(48.0, 48.0)).rounding(2.0));
                                    } else {
                                        ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                                    }
                                    let right_w = 150.0;
                                    ui.vertical(|ui| {
                                        ui.set_width((ui.available_width() - right_w - 8.0).max(120.0));
                                        ui.horizontal(|ui| {
                                            let c = if a.unlocked { theme::TEXT } else { theme::TEXT2 };
                                            ui.add(egui::Label::new(egui::RichText::new(&a.name).font(theme::semibold(14.0)).color(c)).truncate());
                                            if a.global_percent > 0.0 && a.global_percent < 10.0 {
                                                ui.label(egui::RichText::new(tr!("редкое", "rare")).size(11.0).color(egui::Color32::from_rgb(230, 190, 80)));
                                            }
                                        });
                                        let desc = if a.hidden && !a.unlocked && a.description.is_empty() { tr!("Скрытое достижение", "Hidden achievement").to_string() } else { a.description.clone() };
                                        if !desc.is_empty() {
                                            ui.add(egui::Label::new(egui::RichText::new(desc).size(12.0).color(theme::WEAK)).wrap());
                                        }
                                        if let Some(m) = &friend_map {
                                            match m.get(a.api_name.as_str()) {
                                                Some(true) => {
                                                    ui.label(egui::RichText::new(tr!("у друга есть", "the friend has it")).size(11.5).color(theme::IN_GAME));
                                                }
                                                Some(false) => {
                                                    ui.label(egui::RichText::new(tr!("у друга нет", "the friend does not have it")).size(11.5).color(theme::DIM));
                                                }
                                                None => {}
                                            }
                                        }
                                    });
                                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                        ui.spacing_mut().item_spacing.y = 3.0;
                                        if let Some(t) = a.unlock_time.filter(|_| a.unlocked) {
                                            ui.label(egui::RichText::new(trf!("Получено {}", "Unlocked {}", crate::socialwin::fmt_date(t as u64))).size(11.5).color(theme::TEXT2));
                                        }
                                        ui.label(egui::RichText::new(trf!("{:.1}% игроков", "{:.1}% of players", a.global_percent)).size(11.5).color(theme::DIM));
                                        let (bar, _) = ui.allocate_exact_size(egui::vec2(right_w - 20.0, 4.0), egui::Sense::hover());
                                        ui.painter().rect_filled(bar, 2.0, egui::Color32::from_rgb(14, 20, 27));
                                        let mut f = bar;
                                        f.set_width(bar.width() * (a.global_percent / 100.0).clamp(0.0, 1.0));
                                        ui.painter().rect_filled(f, 2.0, theme::BUTTON_HOVER);
                                    });
                                });
                            });
                        }
                    });
                }
            });
        if let Some(f) = ask_friend {
            if !net.friend_achievements.contains_key(&(appid, f)) {
                let _ = self.tx.send(Cmd::FriendAchievements(appid, f));
            }
        }
        if !open {
            self.ach_open = None;
            self.ach_state = AchState::default();
            self.view = View::Library;
            self.lib_page = Some(appid);
        }
    }
}
