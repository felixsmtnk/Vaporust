//! Интерфейс сообщества: окно профиля, окно «Добавить друга», заявки в друзья,
//! меню друга по правому щелчку.

use crate::net::{Cmd, NetState};
use crate::social::{self, SocialCmd};
use crate::LauncherApp;
use eframe::egui;

/// Состояние окон сообщества.
#[derive(Default)]
pub struct SocialUi {
    /// Открытый профиль (SteamID64).
    pub profile: Option<u64>,
    /// Окно «Добавить друга» открыто.
    pub add_open: bool,
    pub add_input: String,
    pub add_error: Option<String>,
    /// Найденный по вводу аккаунт (для предпросмотра перед заявкой).
    pub add_found: Option<u64>,
    /// Друг, которого подтверждаем удалить.
    pub confirm_remove: Option<u64>,
    /// Профили, уже запрошенные ради имени в заявках (чтобы не слать запрос каждый кадр).
    pub requested: std::collections::HashSet<u64>,
}

/// Что выбрали в меню друга.
pub enum FriendAction {
    Profile(u64),
    Chat(u64, String),
    Remove(u64),
    /// Список желаемого друга (в магазине).
    Wishlist(u64, String),
}

/// Меню по правому щелчку на друге.
pub fn friend_menu(resp: &egui::Response, id: u64, name: &str, out: &mut Option<FriendAction>) {
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new(name).strong());
        ui.separator();
        if ui.button("Профиль").clicked() {
            *out = Some(FriendAction::Profile(id));
            ui.close_menu();
        }
        if ui.button("Написать").clicked() {
            *out = Some(FriendAction::Chat(id, name.to_string()));
            ui.close_menu();
        }
        if ui.button("Список желаемого").clicked() {
            *out = Some(FriendAction::Wishlist(id, name.to_string()));
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Удалить из друзей…").clicked() {
            *out = Some(FriendAction::Remove(id));
            ui.close_menu();
        }
    });
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

fn state_label(state: Option<u32>, game: &Option<String>) -> String {
    if let Some(g) = game {
        return format!("играет в {g}");
    }
    match state.unwrap_or(0) {
        0 => "не в сети",
        2 => "не беспокоить",
        3 | 4 => "отошёл",
        7 => "невидимка",
        _ => "в сети",
    }
    .to_string()
}

impl LauncherApp {
    pub(crate) fn open_profile(&mut self, id: u64, net: &NetState) {
        self.social_ui.profile = Some(id);
        if !net.profiles.contains_key(&id) {
            let _ = self.tx.send(Cmd::Social(SocialCmd::Profile(id)));
        }
    }

    /// Кнопки над списком друзей, заявки в друзья и сообщения о результатах.
    pub(crate) fn friends_header(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // «Мой профиль» — в меню «Аккаунт» в левом верхнем углу.
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Добавить друга").clicked() {
                self.social_ui.add_open = true;
            }
            if ui
                .checkbox(&mut self.settings.notify_sound, "Звук")
                .on_hover_text("Звук при новом сообщении (кроме случая, когда открыт чат именно с этим другом).")
                .changed()
            {
                crate::popup::set_sound(self.settings.notify_sound);
            }
        });
        // Заявки: входящие (2) — принять/отклонить; исходящие (4) — отменить.
        let (incoming, outgoing): (Vec<_>, Vec<_>) = {
            let roster = net.roster.read().unwrap();
            let pick = |rel: u32| {
                roster
                    .values()
                    .filter(|f| f.relationship == rel)
                    .map(|f| (f.steam_id, f.persona_name.clone()))
                    .collect::<Vec<_>>()
            };
            (pick(2), pick(4))
        };
        // Steam не присылает имена тех, кто в заявках: берём их из профиля (запрашиваем один раз).
        let mut name_of = |id: u64, name: Option<String>| -> String {
            if let Some(n) = name {
                return n;
            }
            if let Some(Ok(p)) = net.profiles.get(&id) {
                if let Some(n) = &p.persona_name {
                    return n.clone();
                }
            }
            if !net.profiles.contains_key(&id) && self.social_ui.requested.insert(id) {
                let _ = self.tx.send(Cmd::Social(SocialCmd::Profile(id)));
            }
            id.to_string()
        };
        let incoming: Vec<(u64, String)> = incoming.into_iter().map(|(id, n)| (id, name_of(id, n))).collect();
        let outgoing: Vec<(u64, String)> = outgoing.into_iter().map(|(id, n)| (id, name_of(id, n))).collect();
        if !incoming.is_empty() {
            // Свёрнуто по умолчанию, чтобы заявки не вытесняли список друзей.
            let title = egui::RichText::new(format!("Заявки в друзья: {}", incoming.len()))
                .color(egui::Color32::from_rgb(240, 200, 80));
            egui::CollapsingHeader::new(title)
                .id_salt("incoming")
                .default_open(false)
                .show(ui, |ui| {
                    for (id, name) in incoming {
                        // Имя отдельной строкой, кнопки под ним — чтобы не налезали друг на друга.
                        if ui.link(&name).on_hover_text("Профиль").clicked() {
                            self.open_profile(id, net);
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Принять").clicked() {
                                let _ = self.tx.send(Cmd::Social(SocialCmd::AddFriend(id)));
                            }
                            if ui.small_button("Отклонить").clicked() {
                                let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(id)));
                            }
                        });
                        ui.add_space(2.0);
                    }
                });
        }
        if !outgoing.is_empty() {
            egui::CollapsingHeader::new(format!("Отправленные заявки: {}", outgoing.len()))
                .id_salt("outgoing")
                .show(ui, |ui| {
                    for (id, name) in outgoing {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(&name);
                            if ui.small_button("Отменить").clicked() {
                                let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(id)));
                            }
                        });
                    }
                });
        }
        if let Some(m) = &net.friend_msg {
            ui.weak(m);
        }
    }

    /// Обработать выбор в меню друга.
    pub(crate) fn friend_action(&mut self, a: FriendAction, net: &NetState, ctx: &egui::Context) {
        match a {
            FriendAction::Profile(id) => self.open_profile(id, net),
            FriendAction::Chat(id, name) => self.open_chat(ctx, id, name, net),
            FriendAction::Remove(id) => self.social_ui.confirm_remove = Some(id),
            FriendAction::Wishlist(id, name) => self.open_friend_wishlist(id, name),
        }
    }

    /// Окна профиля, «Добавить друга» и подтверждения удаления.
    pub(crate) fn social_windows(&mut self, ctx: &egui::Context, net: &NetState) {
        // Профиль.
        if let Some(id) = self.social_ui.profile {
            let mut open = true;
            let is_friend = net.roster.read().unwrap().get(&id).is_some_and(|f| f.relationship == 3);
            let is_me = net.my_id == Some(id);
            let mut chat = None;
            let mut wish: Option<(u64, String)> = None;
            let tx = self.tx.clone();
            let closed = ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("vaporust_profile"),
                viewport("Профиль — Vaporust", [440.0, 380.0], [360.0, 280.0]),
                |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| match net.profiles.get(&id) {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.label(format!("Не удалось загрузить профиль: {e}"));
                    }
                    Some(Ok(p)) => {
                        let name = p.persona_name.clone().unwrap_or_else(|| id.to_string());
                        ui.horizontal(|ui| {
                            if let Some(a) = &p.avatar_url {
                                ui.add(egui::Image::new(a).fit_to_exact_size(egui::vec2(64.0, 64.0)));
                            }
                            ui.vertical(|ui| {
                                ui.heading(&name);
                                ui.weak(state_label(p.persona_state, &p.game_name));
                                if let Some(r) = &p.real_name {
                                    ui.label(r);
                                }
                                if let Some(l) = &p.location {
                                    ui.weak(l);
                                }
                            });
                        });
                        if let Some(h) = &p.headline {
                            ui.label(h);
                        }
                        if let Some(s) = &p.summary {
                            let text = crate::store::plain_text(s);
                            if !text.is_empty() {
                                ui.label(text.chars().take(400).collect::<String>());
                            }
                        }
                        if let Some(c) = p.created {
                            ui.weak(format!("В Steam с {}", fmt_date(c)));
                        }
                        if !p.is_public {
                            ui.weak("Профиль скрыт настройками приватности.");
                        }
                        ui.weak(format!("Код друга: {}", id.saturating_sub(social::STEAMID64_BASE)));
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if is_friend && ui.button("Написать").clicked() {
                                chat = Some((id, name.clone()));
                            }
                            if !is_friend && !is_me && ui.button("Добавить в друзья").clicked() {
                                let _ = tx.send(Cmd::Social(SocialCmd::AddFriend(id)));
                            }
                            if !is_me && ui.button("Список желаемого").clicked() {
                                wish = Some((id, name.clone()));
                            }
                            if ui.button("Открыть в браузере").clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&p.profile_url));
                            }
                        });
                    }
                });
                ctx.input(|i| i.viewport().close_requested())
                },
            );
            if closed {
                open = false;
            }
            if !open {
                self.social_ui.profile = None;
            }
            if let Some((id, name)) = chat {
                self.open_chat(ctx, id, name, net);
            }
            if let Some((id, name)) = wish {
                self.open_friend_wishlist(id, name);
            }
        }

        // Добавить друга.
        if self.social_ui.add_open {
            let closed = ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("vaporust_add_friend"),
                viewport("Добавить друга — Vaporust", [460.0, 300.0], [400.0, 240.0]),
                |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Код друга, SteamID64 или ссылка вида steamcommunity.com/profiles/…");
                    ui.weak("Код друга есть в Steam: Друзья → Добавить друга.");
                    ui.horizontal(|ui| {
                        let edit = ui.add(egui::TextEdit::singleline(&mut self.social_ui.add_input).desired_width(260.0));
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter || ui.button("Найти").clicked() {
                            match social::parse_friend_input(&self.social_ui.add_input) {
                                Ok(id) => {
                                    self.social_ui.add_error = None;
                                    self.social_ui.add_found = Some(id);
                                    if !net.profiles.contains_key(&id) {
                                        let _ = self.tx.send(Cmd::Social(SocialCmd::Profile(id)));
                                    }
                                }
                                Err(e) => {
                                    self.social_ui.add_error = Some(e);
                                    self.social_ui.add_found = None;
                                }
                            }
                        }
                    });
                    if let Some(e) = &self.social_ui.add_error {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                    }
                    if let Some(id) = self.social_ui.add_found {
                        ui.separator();
                        match net.profiles.get(&id) {
                            None => {
                                ui.spinner();
                            }
                            Some(Err(e)) => {
                                ui.label(format!("Аккаунт не найден: {e}"));
                            }
                            Some(Ok(p)) => {
                                ui.horizontal(|ui| {
                                    if let Some(a) = &p.avatar_url {
                                        ui.add(egui::Image::new(a).fit_to_exact_size(egui::vec2(40.0, 40.0)));
                                    }
                                    ui.strong(p.persona_name.clone().unwrap_or_else(|| id.to_string()));
                                    if ui.button("Отправить заявку").clicked() {
                                        let _ = self.tx.send(Cmd::Social(SocialCmd::AddFriend(id)));
                                    }
                                });
                            }
                        }
                    }
                    if let Some(m) = &net.friend_msg {
                        ui.weak(m);
                    }
                });
                ctx.input(|i| i.viewport().close_requested())
                },
            );
            if closed {
                self.social_ui.add_open = false;
            }
        }

        // Подтверждение удаления из друзей.
        if let Some(id) = self.social_ui.confirm_remove {
            let name = net
                .roster
                .read()
                .unwrap()
                .get(&id)
                .and_then(|f| f.persona_name.clone())
                .unwrap_or_else(|| id.to_string());
            let mut close = false;
            egui::Window::new("Удалить из друзей?")
                .id(egui::Id::new("confirm_remove"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Удалить «{name}» из списка друзей?"));
                    ui.horizontal(|ui| {
                        if ui.button("Да, удалить").clicked() {
                            let _ = self.tx.send(Cmd::Social(SocialCmd::RemoveFriend(id)));
                            close = true;
                        }
                        if ui.button("Отмена").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.social_ui.confirm_remove = None;
            }
        }
    }
}

/// Отдельное окно Windows с минимальным размером (как чат), со значком Vaporust.
fn viewport(title: &str, size: [f32; 2], min: [f32; 2]) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(min)
        .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
}
