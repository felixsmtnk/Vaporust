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
    /// Ждем SteamID по ссылке /id/<имя>.
    pub add_vanity: Option<String>,
    /// Черновик комментария в окне профиля.
    pub comment_draft: String,
    /// Комментарий, удаление которого ждет подтверждения (номер).
    pub confirm_delete: Option<String>,
    /// Открытая страница комментариев профиля (с нуля).
    pub comment_page: u32,
    /// Редактируемая заметка о друге: (SteamID64, имя, текст).
    pub note_edit: Option<(u64, String, String)>,
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
    /// Скриншоты друга.
    Shots(u64, String),
    /// В избранное (наверх списка) или убрать.
    Favorite(u64),
    /// Своя заметка о друге (видна только вам).
    Note(u64, String),
}

/// Меню по правому щелчку на друге.
pub fn friend_menu(resp: &egui::Response, id: u64, name: &str, fav: bool, out: &mut Option<FriendAction>) {
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new(name).strong());
        ui.separator();
        if ui.button(if fav { "Убрать из избранных" } else { "В избранные" }).clicked() {
            *out = Some(FriendAction::Favorite(id));
            ui.close_menu();
        }
        if ui.button("Заметка...").clicked() {
            *out = Some(FriendAction::Note(id, name.to_string()));
            ui.close_menu();
        }
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
        if ui.button("Скриншоты").clicked() {
            *out = Some(FriendAction::Shots(id, name.to_string()));
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Удалить из друзей…").clicked() {
            *out = Some(FriendAction::Remove(id));
            ui.close_menu();
        }
    });
}

pub(crate) fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

impl LauncherApp {
    pub(crate) fn open_profile(&mut self, id: u64, net: &NetState) {
        self.close_extras(crate::View::Profile);
        self.view = crate::View::Profile;
        self.net.lock().unwrap().comment_msg = None;
        self.social_ui.profile = Some(id);
        // Уровень, рамка, недавние игры, комментарии - всегда свежие.
        let _ = self.tx.send(Cmd::ProfileExtra(id));
        self.social_ui.comment_draft.clear();
        self.social_ui.comment_page = 0;
        self.social_ui.confirm_delete = None;
        if !net.profiles.contains_key(&id) {
            let _ = self.tx.send(Cmd::Social(SocialCmd::Profile(id)));
        }
    }

    /// Кнопки над списком друзей, заявки в друзья и сообщения о результатах.
    pub(crate) fn friends_header(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Заявки: входящие (2) - принять/отклонить; исходящие (4) - отменить.
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
        // Steam не присылает имена тех, кто в заявках: берем их из профиля (запрашиваем один раз).
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

        // Шапка, как в Steam: свой аватар с рамкой цвета статуса, имя, статус (меню); справа «...».
        let me = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok());
        let persona = self.settings.persona;
        let color = match persona {
            0 | 7 => egui::Color32::from_gray(140),
            2 => egui::Color32::from_rgb(220, 120, 110),
            3 | 4 => egui::Color32::from_rgb(110, 140, 170),
            _ => egui::Color32::from_rgb(90, 170, 230),
        };
        let mut open_profile = false;
        let mut open_page: Option<u8> = None;
        ui.horizontal(|ui| {
            let (ar, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
            match me.and_then(|p| p.avatar_url.clone()) {
                Some(u) => {
                    egui::Image::new(u).rounding(3.0).paint_at(ui, ar);
                }
                None => {
                    ui.painter().rect_filled(ar, 3.0, crate::theme::CARD);
                }
            }
            ui.painter().rect_stroke(ar.expand(1.0), 3.0, egui::Stroke::new(2.0, color));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Мой профиль").clicked() {
                open_profile = true;
            }
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 66.0).max(60.0));
                ui.spacing_mut().item_spacing.y = 0.0;
                let name = me.and_then(|p| p.persona_name.clone()).unwrap_or_else(|| "Вы".into());
                ui.add(egui::Label::new(egui::RichText::new(crate::richtext::fix(ui, &name)).size(15.0).strong()).truncate());
                // Статус - щелчок открывает выбор, как в Steam.
                // Стрелка вниз после статуса и рука под курсором - видно, что статус можно сменить.
                // Текст в одну строку (без переноса), место под стрелку - справа от текста.
                let text = crate::settings::persona_label(persona);
                // Текст и стрелку рисуем сами поверх пустой кнопки - так их положение точно известно.
                let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), color);
                let text_w = galley.size().x;
                let btn = egui::Button::new("").frame(false).min_size(egui::vec2(text_w + 13.0, galley.size().y + 2.0));
                let st = egui::menu::menu_custom_button(ui, btn, |ui| {
                    let before = self.settings.persona;
                    for (state, label) in crate::settings::PERSONAS {
                        ui.radio_value(&mut self.settings.persona, state, label);
                    }
                    if self.settings.persona != before {
                        let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
                        ui.close_menu();
                    }
                });
                let r = st.response.rect;
                let fg = if st.response.hovered() { egui::Color32::WHITE } else { color };
                let ty = r.center().y - galley.size().y / 2.0;
                ui.painter().galley_with_override_text_color(egui::pos2(r.left(), ty), galley, fg);
                // Стрелка - в 4 точках после текста, как в Steam.
                let c = egui::pos2(r.left() + text_w + 7.5, r.center().y + 0.5);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c + egui::vec2(-3.5, -1.8), c + egui::vec2(3.5, -1.8), c + egui::vec2(0.0, 2.5)],
                    fg,
                    egui::Stroke::NONE,
                ));
                st.response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Сменить статус. «Невидимка»: вы видите друзей, они вас - нет.");
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Справа значок меню друзей (с числом новых заявок): добавить друга, заявки, активность, группы.
                let menu = egui::menu::menu_custom_button(ui, egui::Button::new("").frame(false).min_size(egui::vec2(28.0, 28.0)), |ui| {
                    ui.set_min_width(220.0);
                    if ui.button("Добавить друга...").clicked() {
                        self.social_ui.add_open = true;
                        ui.close_menu();
                    }
                    // Заявки - на странице «Друзья» (подменю закрывали собой остальные пункты).
                    let req = if incoming.is_empty() {
                        egui::RichText::new("Заявки в друзья")
                    } else {
                        egui::RichText::new(format!("Заявки в друзья ({})", incoming.len())).color(egui::Color32::from_rgb(240, 200, 80))
                    };
                    if ui.button(req).clicked() {
                        open_page = Some(1);
                        ui.close_menu();
                    }
                    if !outgoing.is_empty() && ui.button(format!("Отправленные заявки ({})", outgoing.len())).clicked() {
                        open_page = Some(2);
                        ui.close_menu();
                    }
                    if ui.button("Все друзья").clicked() {
                        open_page = Some(0);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Активность друзей").on_hover_text("Лента событий и во что играли за две недели").clicked() {
                        self.open_activity();
                        ui.close_menu();
                    }
                    if ui.button("Групповые чаты").clicked() {
                        self.open_groups();
                        ui.close_menu();
                    }
                });
                let mr = menu.response.rect;
                let col = if menu.response.hovered() { egui::Color32::WHITE } else { crate::theme::TEXT };
                for k in 0..3 {
                    let y = mr.center().y - 5.0 + k as f32 * 5.0;
                    ui.painter().line_segment([egui::pos2(mr.center().x - 7.0, y), egui::pos2(mr.center().x + 7.0, y)], egui::Stroke::new(1.8, col));
                }
                if !incoming.is_empty() {
                    let b = egui::pos2(mr.right() - 3.0, mr.top() + 4.0);
                    ui.painter().circle_filled(b, 7.0, egui::Color32::from_rgb(220, 160, 40));
                    ui.painter().text(b, egui::Align2::CENTER_CENTER, incoming.len().to_string(), egui::FontId::proportional(10.0), egui::Color32::BLACK);
                }
                menu.response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Заявки в друзья, активность, групповые чаты");
            });
        });
        if let Some(t) = open_page {
            self.open_friends_page(t);
        }
        if open_profile {
            if let Some(m) = net.my_id {
                self.open_profile(m, net);
            }
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
            FriendAction::Shots(id, name) => self.open_friend_shots(id, name),
            FriendAction::Favorite(id) => {
                if !self.settings.fav_friends.remove(&id) {
                    self.settings.fav_friends.insert(id);
                }
            }
            FriendAction::Note(id, name) => {
                let text = self.settings.friend_notes.get(&id).cloned().unwrap_or_default();
                self.social_ui.note_edit = Some((id, name, text));
            }
        }
    }

    /// Окна профиля, «Добавить друга» и подтверждения удаления.
    /// Профиль (свой или друга) - раздел главного окна, по образцу страницы профиля Steam:
    /// фон во всю ширину; шапка (аватар с рамкой, имя, о себе; справа уровень, значок, кнопки);
    /// ниже две колонки - слева витрины, 3 недавние игры и комментарии по страницам, справа
    /// статус, счетчики со значками и наградами, друзья с уровнями.
    pub(crate) fn profile_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(id) = self.social_ui.profile else {
            if let Some(me) = net.my_id {
                self.open_profile(me, net);
            }
            return;
        };
        let ctx = ui.ctx().clone();
        let is_friend = net.roster.read().unwrap().get(&id).is_some_and(|f| f.relationship == 3);
        let is_me = net.my_id == Some(id);
        let mut act: Option<ProfileAct> = None;
        let extra = net.profile_extra.get(&id).and_then(|r| r.as_ref().ok()).cloned();
        let my_avatar = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok()).and_then(|p| p.avatar_url.clone());
        let p = match net.profiles.get(&id) {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить профиль: {e}"));
                return;
            }
            Some(Ok(p)) => p.clone(),
        };
        let name = p.persona_name.clone().unwrap_or_else(|| id.to_string());
        let page = extra.as_ref().map(|x| x.page.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Фон профиля во всю ширину, к низу плавно уходит в цвет окна.
            let full = ui.available_rect_before_wrap();
            if let Some(bg) = extra.as_ref().and_then(|x| x.background.clone()) {
                let h = (full.width() * 9.0 / 16.0).min(760.0);
                let r = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), h));
                egui::Image::new(bg).tint(egui::Color32::from_gray(175)).paint_at(ui, r);
                let fade = egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - h * 0.45), r.max);
                let mut mesh = egui::Mesh::default();
                let (a, b) = (crate::theme::BG.gamma_multiply(0.0), crate::theme::BG);
                mesh.colored_vertex(fade.left_top(), a);
                mesh.colored_vertex(fade.right_top(), a);
                mesh.colored_vertex(fade.right_bottom(), b);
                mesh.colored_vertex(fade.left_bottom(), b);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().add(mesh);
            }
            ui.add_space(28.0);
            crate::theme::centered(ui, 1000.0, |ui| {
                // ---- Шапка ----
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90))
                    .inner_margin(egui::Margin::same(18.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            let avatar = extra.as_ref().and_then(|x| x.animated_avatar.clone()).or_else(|| p.avatar_url.clone());
                            if let Some(a) = &avatar {
                                let frame = extra.as_ref().and_then(|x| x.frame.clone());
                                crate::profile_extra::avatar_with_frame(ui, a, frame.as_deref(), 150.0);
                            }
                            ui.add_space(14.0);
                            let right_w = 270.0;
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - right_w - 16.0).max(200.0));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(crate::richtext::fix(ui, &name)).size(26.0).color(egui::Color32::WHITE));
                                let place = [p.real_name.clone(), p.location.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                if !place.is_empty() {
                                    ui.label(egui::RichText::new(place).color(crate::theme::WEAK));
                                }
                                if let Some(s) = p.summary.as_ref().map(|s| crate::store::plain_text(s)).filter(|t| !t.is_empty()) {
                                    ui.add_space(10.0);
                                    ui.add(egui::Label::new(egui::RichText::new(crate::richtext::fix(ui, &s.chars().take(800).collect::<String>())).size(13.5)).wrap());
                                }
                                if !p.is_public {
                                    ui.add_space(6.0);
                                    ui.weak("Профиль скрыт настройками приватности.");
                                }
                            });
                            ui.vertical(|ui| {
                                ui.set_width(right_w);
                                ui.add_space(8.0);
                                if let Some(l) = extra.as_ref().and_then(|x| x.level) {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Уровень").size(22.0).color(egui::Color32::WHITE));
                                        crate::profile_extra::level_badge(ui, l);
                                    });
                                }
                                if let Some((icon, bname, xp)) = &page.fav_badge {
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Image::new(icon.as_str()).fit_to_exact_size(egui::vec2(54.0, 54.0)));
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(crate::richtext::fix(ui, bname)).size(13.0));
                                            ui.weak(xp);
                                        });
                                    });
                                }
                                ui.add_space(10.0);
                                if is_me {
                                    if ui.button("Редактировать профиль").on_hover_text("Откроется в браузере").clicked() {
                                        act = Some(ProfileAct::Url(format!("https://steamcommunity.com/profiles/{id}/edit/info")));
                                    }
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        if is_friend && ui.button("Написать").clicked() {
                                            act = Some(ProfileAct::Chat);
                                        }
                                        if !is_friend && ui.button("Добавить в друзья").clicked() {
                                            let _ = self.tx.send(Cmd::Social(SocialCmd::AddFriend(id)));
                                        }
                                        ui.menu_button("Еще", |ui| {
                                            if ui.button("Список желаемого").clicked() {
                                                act = Some(ProfileAct::Wishlist);
                                                ui.close_menu();
                                            }
                                            if ui.button("Открыть в браузере").clicked() {
                                                act = Some(ProfileAct::Url(p.profile_url.clone()));
                                                ui.close_menu();
                                            }
                                        });
                                    });
                                }
                            });
                        });
                    });

                // ---- Две колонки ----
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 15, 20, 205))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let right_w = 270.0;
                        ui.horizontal_top(|ui| {
                            let left_w = ui.available_width() - right_w - 18.0;
                            ui.vertical(|ui| {
                                ui.set_width(left_w);
                                match &extra {
                                    Some(x) => profile_left(ui, x, &page, is_me, is_friend || p.is_public, my_avatar.as_deref(), &mut self.social_ui, net.comment_msg.as_ref(), id, &mut act),
                                    None if net.profile_extra.get(&id).is_none() => {
                                        crate::spin::spinner(ui);
                                    }
                                    None => {}
                                }
                            });
                            ui.add_space(18.0);
                            ui.vertical(|ui| {
                                ui.set_width(right_w);
                                profile_right(ui, &p, extra.as_ref(), &page, id, &mut act);
                            });
                        });
                    });
                ui.add_space(20.0);
            });
        });
        match act {
            Some(ProfileAct::Chat) => self.open_chat(&ctx, id, name.clone(), net),
            Some(ProfileAct::Wishlist) => self.open_friend_wishlist(id, name.clone()),
            Some(ProfileAct::MyWishlist) => self.open_wishlist(),
            Some(ProfileAct::Recent) => {
                // Своя библиотека, недавно запущенные сверху.
                self.close_extras(crate::View::Library);
                self.view = crate::View::Library;
                self.lib_page = None;
                self.settings.sort = crate::settings::Sort::LastPlayed;
            }
            Some(ProfileAct::Url(u)) => ctx.open_url(egui::OpenUrl::new_tab(u)),
            Some(ProfileAct::Store(a)) => self.open_store(Some(a), false),
            Some(ProfileAct::Profile(sid)) => self.open_profile(sid, net),
            Some(ProfileAct::Post(text)) => {
                let _ = self.tx.send(Cmd::PostComment { id, text });
                self.social_ui.comment_draft.clear();
                self.social_ui.comment_page = 0;
            }
            Some(ProfileAct::Delete(gid)) => {
                self.social_ui.confirm_delete = None;
                let _ = self.tx.send(Cmd::DeleteComment { id, gid });
                self.social_ui.comment_page = 0;
            }
            Some(ProfileAct::Page(n)) => {
                self.social_ui.comment_page = n;
                let _ = self.tx.send(Cmd::MoreComments { id, start: n * 10 });
            }
            Some(ProfileAct::Count(label, href)) => {
                // Свои разделы открываем в программе, остальное - в браузере.
                let l = label.to_lowercase();
                if is_me && l.starts_with("значки") {
                    self.open_badges();
                } else if is_me && l.starts_with("инвентарь") {
                    self.open_inventory(false);
                } else if is_me && l.starts_with("друзья") {
                    self.open_friends_page(0);
                } else if is_me && l.starts_with("игры") {
                    self.close_extras(crate::View::Library);
                    self.view = crate::View::Library;
                    self.lib_page = None;
                } else if l.starts_with("скриншоты") {
                    if is_me {
                        self.open_shots(None);
                    } else {
                        self.open_friend_shots(id, name.clone());
                    }
                } else if !href.is_empty() {
                    ctx.open_url(egui::OpenUrl::new_tab(href));
                }
            }
            None => {}
        }
    }

    pub(crate) fn social_windows(&mut self, ctx: &egui::Context, net: &NetState) {

        // Своя заметка о друге (видна только вам, хранится в настройках Vaporust).
        if let Some((id, name, mut text)) = self.social_ui.note_edit.take() {
            let mut keep = true;
            let mut save = false;
            egui::Window::new(format!("Заметка: {name}"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.weak("Видна только вам: например, как друга зовут на самом деле или откуда вы знакомы.");
                    ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(320.0));
                    ui.horizontal(|ui| {
                        if ui.button("Сохранить").clicked() {
                            save = true;
                            keep = false;
                        }
                        if ui.button("Отмена").clicked() {
                            keep = false;
                        }
                    });
                });
            if save {
                let t = text.trim().to_string();
                if t.is_empty() {
                    self.settings.friend_notes.remove(&id);
                } else {
                    self.settings.friend_notes.insert(id, t);
                }
            }
            if keep {
                self.social_ui.note_edit = Some((id, name, text));
            }
        }

        // Добавить друга.
        if self.social_ui.add_open {
            let closed = ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("vaporust_add_friend"),
                viewport("Добавить друга - Vaporust", [460.0, 300.0], [400.0, 240.0]),
                |ctx, _| {
                    crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Код друга, SteamID64 или ссылка на профиль (…/profiles/… или …/id/имя)");
                    ui.weak("Код друга есть в Steam: Друзья, затем «Добавить друга».");
                    ui.horizontal(|ui| {
                        let edit = ui.add(egui::TextEdit::singleline(&mut self.social_ui.add_input).desired_width(260.0));
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let find = enter || ui.button("Найти").clicked();
                        let vanity = social::vanity_name(&self.social_ui.add_input);
                        if find && vanity.is_some() {
                            let name = vanity.unwrap();
                            self.social_ui.add_error = None;
                            self.social_ui.add_found = None;
                            match net.vanity.get(&name) {
                                Some(Ok(id)) => self.social_ui.add_found = Some(*id),
                                _ => {
                                    let _ = self.tx.send(Cmd::Social(SocialCmd::ResolveVanity(name.clone())));
                                    self.social_ui.add_vanity = Some(name);
                                }
                            }
                        } else if find {
                            self.social_ui.add_vanity = None;
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
                    // Ответ на ссылку /id/<имя>.
                    if let Some(name) = self.social_ui.add_vanity.clone() {
                        match net.vanity.get(&name) {
                            None => {
                                ui.horizontal(|ui| {
                                    crate::spin::spinner(ui);
                                    ui.weak(format!("Ищу профиль «{name}»..."));
                                });
                            }
                            Some(Ok(id)) => {
                                self.social_ui.add_found = Some(*id);
                                self.social_ui.add_vanity = None;
                            }
                            Some(Err(e)) => {
                                self.social_ui.add_error = Some(e.clone());
                                self.social_ui.add_vanity = None;
                            }
                        }
                    }
                    if let Some(e) = &self.social_ui.add_error {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                    }
                    if let Some(id) = self.social_ui.add_found {
                        ui.separator();
                        match net.profiles.get(&id) {
                            None => {
                                crate::spin::spinner(ui);
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
                                    // Уже в друзьях / заявка уже отправлена / это вы сами - кнопки нет.
                                    let rel = net.roster.read().unwrap().get(&id).map(|f| f.relationship);
                                    if Some(id) == net.my_id {
                                        ui.weak("это вы");
                                    } else if rel == Some(3) {
                                        ui.weak("уже в друзьях");
                                    } else if rel == Some(4) {
                                        ui.weak("заявка уже отправлена");
                                    } else if rel == Some(2) {
                                        ui.weak("ждет вашего ответа (заявки - в панели друзей)");
                                    } else if ui.button("Отправить заявку").clicked() {
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
pub(crate) fn viewport(title: &str, size: [f32; 2], min: [f32; 2]) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(min)
        .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
        .with_decorations(crate::winframe::system())
}


/// Что нажали на странице профиля.
enum ProfileAct {
    Chat,
    Wishlist,
    /// Свой список желаемого (в магазине программы).
    MyWishlist,
    /// Свои недавно запущенные игры (библиотека).
    Recent,
    Url(String),
    Store(u32),
    Profile(u64),
    Post(String),
    Delete(String),
    Page(u32),
    Count(String, String),
}

/// Заголовок раздела полосой, как в профиле Steam: слева название, справа подпись.
fn section_bar(ui: &mut egui::Ui, title: &str, right: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    let mut mesh = egui::Mesh::default();
    let (a, b) = (egui::Color32::from_rgb(52, 92, 140), egui::Color32::from_rgb(30, 52, 82));
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), a);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
    ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(15.0), egui::Color32::WHITE);
    if !right.is_empty() {
        ui.painter().text(rect.right_center() - egui::vec2(12.0, 0.0), egui::Align2::RIGHT_CENTER, right, egui::FontId::proportional(13.5), egui::Color32::from_gray(225));
    }
    ui.add_space(8.0);
}

/// Дата комментария как в Steam: «18 мая в 18:08», за прошлые годы - «2 дек 2025 в 17:12».
fn comment_date(unix: u64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    const M: [&str; 12] = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
    let Some(t) = Local.timestamp_opt(unix as i64, 0).single() else { return String::new() };
    let month = M[t.month0() as usize];
    if t.year() == Local::now().year() {
        format!("{} {month} в {}", t.day(), t.format("%H:%M"))
    } else {
        format!("{} {month} {} в {}", t.day(), t.year(), t.format("%H:%M"))
    }
}

/// Номера страниц комментариев: «< 1 2 3 4 5 6 ... 9 >».
fn pager(ui: &mut egui::Ui, current: u32, pages: u32, act: &mut Option<ProfileAct>) {
    if pages <= 1 {
        return;
    }
    // Порядок «< 1 2 3 ... >»; в раскладке справа налево элементы добавляются задом наперед.
    enum Item {
        Prev,
        Num(u32),
        Gap,
        Next,
    }
    let mut items = vec![Item::Prev];
    let mut shown_gap = false;
    for n in 0..pages {
        if n < 6 || n + 1 == pages || n.abs_diff(current) <= 1 {
            items.push(Item::Num(n));
            shown_gap = false;
        } else if !shown_gap {
            items.push(Item::Gap);
            shown_gap = true;
        }
    }
    items.push(Item::Next);
    if ui.layout().prefer_right_to_left() {
        items.reverse();
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for it in items {
            match it {
                Item::Prev => {
                    if ui.add_enabled(current > 0, egui::Button::new("<").small()).clicked() {
                        *act = Some(ProfileAct::Page(current - 1));
                    }
                }
                Item::Next => {
                    if ui.add_enabled(current + 1 < pages, egui::Button::new(">").small()).clicked() {
                        *act = Some(ProfileAct::Page(current + 1));
                    }
                }
                Item::Gap => {
                    ui.weak("...");
                }
                Item::Num(n) => {
                    let text = egui::RichText::new((n + 1).to_string())
                        .color(if n == current { egui::Color32::WHITE } else { crate::theme::WEAK });
                    if ui.add(egui::Button::new(text).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && n != current {
                        *act = Some(ProfileAct::Page(n));
                    }
                }
            }
        }
    });
}

/// Левая колонка профиля: витрины, недавняя активность (3 игры, как в Steam), комментарии.
#[allow(clippy::too_many_arguments)]
fn profile_left(
    ui: &mut egui::Ui,
    x: &crate::profile_extra::ProfileExtra,
    page: &crate::profile_extra::PageInfo,
    is_me: bool,
    can_comment: bool,
    my_avatar: Option<&str>,
    sui: &mut SocialUi,
    msg: Option<&String>,
    owner: u64,
    act: &mut Option<ProfileAct>,
) {
    // Витрины.
    for s in &x.showcases {
        section_bar(ui, &s.title, "");
        if !s.images.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for (img, href) in &s.images {
                    let r = ui.add(
                        egui::Image::new(crate::richtext::ugc_small(img, 400))
                            .fit_to_exact_size(egui::vec2(120.0, 72.0))
                            .sense(egui::Sense::click()),
                    );
                    if let Some(h) = href {
                        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            *act = Some(ProfileAct::Url(h.clone()));
                        }
                    }
                }
            });
        }
        // Показатели, как в Steam: крупное число, под ним подпись.
        if !s.stats.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 26.0;
                for (value, label) in &s.stats {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(egui::RichText::new(value).size(22.0).color(egui::Color32::WHITE));
                        ui.label(egui::RichText::new(label).size(12.0).color(crate::theme::WEAK));
                    });
                }
            });
        }
        if !s.text.is_empty() {
            ui.add_space(2.0);
            ui.add(egui::Label::new(egui::RichText::new(crate::richtext::fix(ui, &s.text)).size(13.0).color(crate::theme::WEAK)).wrap());
        }
        ui.add_space(14.0);
    }

    // Недавняя активность - как на странице Steam (3 игры, часы и последний запуск справа).
    if !page.recent.is_empty() {
        section_bar(ui, "Недавняя активность", &page.recent_total);
        for g in &page.recent {
            let row = egui::Frame::none().fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 70)).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(egui::vec2(184.0, 69.0), egui::Sense::hover());
                    ui.painter().rect_filled(ir, 2.0, crate::theme::CARD);
                    if !g.capsule.is_empty() {
                        egui::Image::new(g.capsule.as_str()).paint_at(ui, ir);
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(crate::richtext::fix(ui, &g.name)).size(15.0).color(egui::Color32::WHITE));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                        ui.vertical(|ui| {
                            for d in &g.details {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    ui.label(egui::RichText::new(d).size(12.5).color(crate::theme::WEAK));
                                });
                            }
                        });
                    });
                });
            });
            let resp = ui.interact(row.response.rect, ui.id().with(("recent", g.appid)), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_stroke(row.response.rect, 2.0, egui::Stroke::new(1.0, crate::theme::ACCENT));
            }
            if g.appid != 0 && resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Страница в магазине").clicked() {
                *act = Some(ProfileAct::Store(g.appid));
            }
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let base = format!("https://steamcommunity.com/profiles/{owner}");
                if ui.link("Обзоры").clicked() {
                    *act = Some(ProfileAct::Url(format!("{base}/recommended/")));
                }
                ui.weak("|");
                if ui.link("Список желаемого").clicked() {
                    *act = Some(if is_me { ProfileAct::MyWishlist } else { ProfileAct::Wishlist });
                }
                ui.weak("|");
                if ui.link("Все недавно запущенные").clicked() {
                    *act = Some(if is_me { ProfileAct::Recent } else { ProfileAct::Url(format!("{base}/games/?tab=recent")) });
                }
            });
        });
        ui.add_space(14.0);
    }

    // Комментарии - по страницам, как в Steam.
    let pages = x.comments_total.div_ceil(10);
    let all = format!("Все комментарии ({})", x.comments_total);
    section_bar(ui, "Комментарии", &all);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| pager(ui, sui.comment_page, pages, act));
        });
    });
    if can_comment {
        ui.horizontal(|ui| {
            let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            match my_avatar {
                Some(a) => {
                    egui::Image::new(a).rounding(2.0).paint_at(ui, ar);
                }
                None => {
                    ui.painter().rect_filled(ar, 2.0, crate::theme::CARD);
                }
            }
            ui.add(egui::TextEdit::singleline(&mut sui.comment_draft).hint_text("Оставить комментарий").desired_width(ui.available_width() - 100.0));
            if ui.add_enabled(!sui.comment_draft.trim().is_empty(), egui::Button::new("Отправить")).clicked() {
                *act = Some(ProfileAct::Post(sui.comment_draft.trim().to_string()));
            }
        });
        if let Some(m) = msg {
            ui.weak(m);
        }
        ui.add_space(8.0);
    }
    for c in &x.comments {
        // Строка комментария: наведена ли мышь - по месту с прошлого кадра (без учета слоев,
        // иначе значок мусорки «уводил» наведение со строки и пропадал до щелчка).
        let row_id = ui.id().with(("comment_row", &c.id));
        let hovered = ui
            .ctx()
            .data(|d| d.get_temp::<egui::Rect>(row_id))
            .zip(ui.input(|i| i.pointer.hover_pos()))
            .is_some_and(|(r, p)| r.contains(p));
        let confirm = sui.confirm_delete.as_deref() == Some(c.id.as_str());
        let row = ui.horizontal_top(|ui| {
            // Аватар; рамка аватара (предмет Steam) - поверх, чуть больше, как в Steam.
            let (ar, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
            if !c.avatar.is_empty() {
                egui::Image::new(c.avatar.as_str()).rounding(2.0).paint_at(ui, ar);
            } else {
                ui.painter().rect_filled(ar, 2.0, crate::theme::CARD);
            }
            if !c.frame.is_empty() {
                egui::Image::new(c.frame.as_str()).paint_at(ui, ar.expand(ar.width() * 0.11));
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    if ui.link(egui::RichText::new(crate::richtext::fix(ui, &c.author)).color(egui::Color32::WHITE).strong()).clicked()
                        && !c.author_url.is_empty()
                    {
                        *act = Some(ProfileAct::Url(c.author_url.clone()));
                    }
                    // Удалить - значок мусорки справа при наведении, как в Steam.
                    if c.can_delete && !c.id.is_empty() && (hovered || confirm) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if confirm {
                                if ui.small_button("Нет").clicked() {
                                    sui.confirm_delete = None;
                                }
                                if ui
                                    .small_button(egui::RichText::new("Удалить навсегда").color(egui::Color32::from_rgb(230, 110, 110)))
                                    .clicked()
                                {
                                    *act = Some(ProfileAct::Delete(c.id.clone()));
                                }
                            } else if trash_button(ui)
                                .on_hover_text(if is_me { "Удалить комментарий из своего профиля" } else { "Удалить свой комментарий" })
                                .clicked()
                            {
                                sui.confirm_delete = Some(c.id.clone());
                            }
                        });
                    }
                });
                if c.time > 0 {
                    ui.label(egui::RichText::new(comment_date(c.time)).size(12.0).color(crate::theme::WEAK));
                }
                ui.add(egui::Label::new(crate::richtext::fix(ui, &c.text)).wrap());
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(row_id, row.response.rect));
        ui.add_space(10.0);
    }
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| pager(ui, sui.comment_page, pages, act));
        });
    });
}

/// Правая колонка профиля: статус, счетчики (с картинками наград и значков), друзья.
fn profile_right(
    ui: &mut egui::Ui,
    p: &aurelia::steam_client::UserProfile,
    extra: Option<&crate::profile_extra::ProfileExtra>,
    page: &crate::profile_extra::PageInfo,
    id: u64,
    act: &mut Option<ProfileAct>,
) {
    let (status, color) = match (p.persona_state, &p.game_name) {
        (_, Some(g)) if !g.is_empty() => (format!("В игре\n{g}"), egui::Color32::from_rgb(140, 200, 80)),
        (Some(0) | None, _) => ("Не в сети".to_string(), egui::Color32::GRAY),
        _ => ("В сети".to_string(), egui::Color32::from_rgb(90, 170, 230)),
    };
    ui.label(egui::RichText::new(status).size(22.0).color(color));
    ui.add_space(12.0);
    let counts = extra.map(|x| x.counts.clone()).unwrap_or_default();
    for (label, total, href) in counts.into_iter().filter(|(l, t, _)| !t.is_empty() || l.to_lowercase().starts_with("инвентарь")) {
        let lower = label.to_lowercase();
        let resp = ui
            // Строка фиксированной высоты: иначе раскладка с выравниванием по низу растягивалась на всю
            // оставшуюся высоту колонки и уводила остальное под комментарии.
            .allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::left_to_right(egui::Align::Max), |ui| {
                // Как в Steam: подпись, за ней через небольшой отступ число - крупнее и серее, по нижнему краю.
                ui.spacing_mut().item_spacing.x = 7.0;
                let l = ui.add(egui::Label::new(egui::RichText::new(&label).size(15.0).color(egui::Color32::from_gray(215))).sense(egui::Sense::click()));
                if !total.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&total).size(20.0).color(crate::theme::WEAK)).selectable(false));
                }
                l
            })
            .inner
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            *act = Some(ProfileAct::Count(label.clone(), href.clone()));
        }
        // Картинки под счетчиком: награды профиля и значки.
        let icons: &[String] = if lower.starts_with("награды") {
            &page.awards
        } else if lower.starts_with("значки") {
            &page.badges
        } else {
            &[]
        };
        if !icons.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for i in icons.iter().take(4) {
                    ui.add(egui::Image::new(i.as_str()).fit_to_exact_size(egui::vec2(48.0, 48.0)));
                }
            });
        }
        ui.add_space(6.0);
    }
    // Друзья с уровнями, как в правой колонке Steam.
    if !page.friends.is_empty() {
        ui.add_space(6.0);
        for f in &page.friends {
            let row = ui.horizontal(|ui| {
                let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                if !f.avatar.is_empty() {
                    egui::Image::new(f.avatar.as_str()).rounding(2.0).paint_at(ui, ar);
                }
                let c = if f.in_game {
                    egui::Color32::from_rgb(140, 200, 80)
                } else if f.online {
                    egui::Color32::from_rgb(90, 170, 230)
                } else {
                    egui::Color32::from_gray(110)
                };
                ui.painter().rect_stroke(ar.expand(1.0), 2.0, egui::Stroke::new(1.5, c));
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 34.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add(egui::Label::new(egui::RichText::new(crate::richtext::fix(ui, &f.name)).size(13.0).color(c)).truncate());
                    ui.add(egui::Label::new(egui::RichText::new(&f.status).size(11.0).color(crate::theme::WEAK)).truncate());
                });
                if let Some(l) = f.level {
                    crate::profile_extra::level_badge_sized(ui, l, 28.0);
                }
            });
            let resp = ui.interact(row.response.rect, ui.id().with(("pfriend", f.steamid)), egui::Sense::click());
            if f.steamid != 0 && resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Профиль").clicked() {
                *act = Some(ProfileAct::Profile(f.steamid));
            }
            ui.add_space(4.0);
        }
    }
    if let Some(x) = extra {
        if !x.groups.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("Группы  {}", x.groups.len())).size(15.0).color(egui::Color32::from_gray(225)));
            for g in x.groups.iter().take(6) {
                let l = ui.link(egui::RichText::new(crate::richtext::fix(ui, &g.name)).size(12.5));
                let l = if g.members > 0 { l.on_hover_text(format!("участников: {}", crate::store::group_digits(g.members))) } else { l };
                if l.clicked() && !g.url.is_empty() {
                    *act = Some(ProfileAct::Url(g.url.clone()));
                }
            }
        }
    }
    ui.add_space(10.0);
    if let Some(c) = p.created {
        ui.weak(format!("В Steam с {}", fmt_date(c)));
    }
    ui.weak(format!("Код друга: {}", id.saturating_sub(social::STEAMID64_BASE)));
}

/// Значок мусорки (рисуется линиями, как в Steam): кнопка удаления комментария.
fn trash_button(ui: &mut egui::Ui) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let c = if resp.hovered() { egui::Color32::from_rgb(230, 110, 110) } else { crate::theme::WEAK };
    let s = egui::Stroke::new(1.4, c);
    let p = ui.painter();
    let m = r.center();
    // Крышка с ручкой.
    p.line_segment([m + egui::vec2(-6.0, -4.5), m + egui::vec2(6.0, -4.5)], s);
    p.line_segment([m + egui::vec2(-2.0, -6.5), m + egui::vec2(2.0, -6.5)], s);
    // Корзина.
    let body = [m + egui::vec2(-4.5, -3.0), m + egui::vec2(-3.5, 6.5), m + egui::vec2(3.5, 6.5), m + egui::vec2(4.5, -3.0)];
    p.add(egui::Shape::line(body.to_vec(), s));
    p.line_segment([m + egui::vec2(-1.3, -1.0), m + egui::vec2(-1.1, 4.5)], s);
    p.line_segment([m + egui::vec2(1.3, -1.0), m + egui::vec2(1.1, 4.5)], s);
    resp
}
