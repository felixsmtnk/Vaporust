//! Навигация как в клиенте Steam: сверху четыре раздела (МАГАЗИН, БИБЛИОТЕКА, СООБЩЕСТВО,
//! профиль), под ними - подразделы выбранного раздела. Все разделы видны всегда, вкладки не
//! появляются и не пропадают.

use crate::net::NetState;
use crate::storewin::Tab;
use crate::{theme, LauncherApp, View};
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Store,
    Library,
    Community,
    Profile,
}

pub fn section_of(v: View) -> Option<Section> {
    Some(match v {
        View::Store | View::News | View::Points => Section::Store,
        View::Library | View::Screenshots | View::Stats | View::Storage | View::Downloads => Section::Library,
        View::Activity | View::Groups | View::Community | View::Workshop => Section::Community,
        View::Inventory | View::Badges | View::Family | View::Profile | View::Friends => Section::Profile,
        View::Settings => return None,
    })
}

/// Крупный пункт верхней полосы (при наведении белеет, выбранный - голубой с чертой).
fn top_item(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let mut t = egui::RichText::new(text).size(15.0).strong();
    if on {
        t = t.color(theme::ACCENT);
    }
    let r = ui.add(egui::Button::new(t).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand);
    if on {
        let y = r.rect.bottom() + 2.0;
        ui.painter().line_segment([egui::pos2(r.rect.left() + 4.0, y), egui::pos2(r.rect.right() - 4.0, y)], egui::Stroke::new(2.0, theme::ACCENT));
    }
    r
}

/// Пункт подменю.
fn sub_item(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let t = egui::RichText::new(text).size(13.5).color(if on { egui::Color32::WHITE } else { theme::WEAK });
    let r = ui.add(egui::Button::new(t).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand);
    if on {
        let y = r.rect.bottom() + 1.0;
        ui.painter().line_segment([egui::pos2(r.rect.left() + 4.0, y), egui::pos2(r.rect.right() - 4.0, y)], egui::Stroke::new(1.5, theme::ACCENT));
    }
    r
}

impl LauncherApp {
    /// Имя профиля для верхней полосы: имя в Steam (если уже известно), иначе «ПРОФИЛЬ».
    fn profile_label(&mut self, ui: &egui::Ui, net: &NetState) -> String {
        let Some(me) = net.my_id else { return "ПРОФИЛЬ".into() };
        match net.profiles.get(&me) {
            Some(Ok(p)) => p.persona_name.clone().filter(|n| !n.trim().is_empty()).map(|n| crate::richtext::fix(ui, &n).to_uppercase()),
            Some(Err(_)) => None,
            None => {
                if !self.own_profile_asked {
                    self.own_profile_asked = true;
                    let _ = self.tx.send(crate::net::Cmd::Social(crate::social::SocialCmd::Profile(me)));
                }
                None
            }
        }
        .unwrap_or_else(|| "ПРОФИЛЬ".into())
    }

    /// Верхняя полоса: меню Vaporust, четыре раздела, справа уведомления Steam.
    pub(crate) fn top_nav(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let section = section_of(self.view);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            // Меню программы - простым текстом, как «Steam» в клиенте.
            egui::menu::menu_custom_button(ui, egui::Button::new(egui::RichText::new("Vaporust").size(14.0)).frame(false), |ui| {
                ui.set_min_width(190.0);
                if ui.button("Настройки").clicked() {
                    self.view = View::Settings;
                    ui.close_menu();
                }
                ui.separator();
                if ui.add_enabled(!net.busy, egui::Button::new("Выйти из аккаунта")).clicked() {
                    let _ = self.tx.send(crate::net::Cmd::Logout);
                    ui.close_menu();
                }
                if ui.button("Закрыть Vaporust").clicked() {
                    crate::tray::request_exit();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });
            if top_item(ui, section == Some(Section::Store), "МАГАЗИН")
                .on_hover_text("Ctrl+щелчок - открыть магазин в отдельном окне")
                .clicked()
            {
                self.open_store(None, ctrl);
            }
            if top_item(ui, section == Some(Section::Library), "БИБЛИОТЕКА").clicked() {
                self.close_extras(View::Library);
                self.view = View::Library;
                self.lib_page = None;
            }
            if top_item(ui, section == Some(Section::Community), "СООБЩЕСТВО").clicked() {
                if self.community_ui.appid.is_some() {
                    self.view = View::Community;
                } else {
                    self.open_activity();
                }
            }
            let name = self.profile_label(ui, net);
            if top_item(ui, section == Some(Section::Profile), &name).on_hover_text("Ваш профиль, инвентарь, значки, семья Steam").clicked() {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                self.steam_notifs_button(ui, net);
            });
        });
    }

    /// Подразделы выбранного раздела (строка под верхней полосой).
    pub(crate) fn sub_nav(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(section) = section_of(self.view) else {
            ui.label(egui::RichText::new("Настройки").size(13.5).color(egui::Color32::WHITE));
            return;
        };
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            match section {
                Section::Store => {
                    let (tab, page) = self.store_win.as_ref().map_or((Tab::Search, false), |w| (w.tab(), w.on_page()));
                    let in_store = self.view == View::Store && !self.store_detached;
                    for (t, label) in [
                        (Tab::Search, "Поиск"),
                        (Tab::Deals, "Скидки"),
                        (Tab::Charts, "Чарты"),
                        (Tab::Discovery, "Рекомендации"),
                        (Tab::Wishlist, "Желаемое"),
                    ] {
                        let on = in_store && !page && (tab == t || (t == Tab::Deals && tab == Tab::Events));
                        if sub_item(ui, on, label).clicked() {
                            self.open_store(None, ctrl);
                            if let Some(w) = self.store_win.as_mut() {
                                if !(t == Tab::Deals && w.tab() == Tab::Events) {
                                    w.set_tab(t);
                                } else {
                                    w.set_tab(Tab::Events);
                                }
                            }
                        }
                    }
                    ui.separator();
                    if sub_item(ui, self.view == View::News, "Новости").on_hover_text("Новости ваших игр одной лентой").clicked() {
                        self.close_extras(View::News);
                        self.view = View::News;
                    }
                    if sub_item(ui, self.view == View::Points, "Магазин очков")
                        .on_hover_text("Очки Steam: эффекты чата, стикеры, рамки")
                        .clicked()
                    {
                        self.open_points();
                    }
                }
                Section::Library => {
                    if sub_item(ui, self.view == View::Screenshots, "Скриншоты").clicked() {
                        self.open_shots(None);
                    }
                    if sub_item(ui, self.view == View::Stats, "Статистика")
                        .on_hover_text("Время в играх: по дням, за две недели, больше всего часов")
                        .clicked()
                    {
                        self.open_stats();
                    }
                    if sub_item(ui, self.view == View::Storage, "Хранилище")
                        .on_hover_text("Сколько места занимают игры, остатки удаленных игр")
                        .clicked()
                    {
                        self.open_storage();
                    }
                }
                Section::Community => {
                    if sub_item(ui, self.view == View::Activity, "Активность друзей").clicked() {
                        self.open_activity();
                    }
                    if sub_item(ui, self.view == View::Groups, "Групповые чаты").clicked() {
                        self.open_groups();
                    }
                    if let Some((_, name)) = self.community_ui.appid.clone() {
                        ui.separator();
                        let label = format!("Обсуждения: {}", crate::richtext::fix(ui, &name));
                        if sub_item(ui, self.view == View::Community, &label).clicked() {
                            self.view = View::Community;
                        }
                    }
                    if let Some((_, name)) = self.ws_open.clone().filter(|_| !self.ws_detached) {
                        let label = format!("Мастерская: {}", crate::richtext::fix(ui, &name));
                        if sub_item(ui, self.view == View::Workshop, &label).clicked() {
                            self.view = View::Workshop;
                        }
                    }
                }
                Section::Profile => {
                    let mine = self.view == View::Profile && self.social_ui.profile == net.my_id;
                    if sub_item(ui, mine, "Мой профиль").clicked() {
                        if let Some(me) = net.my_id {
                            self.open_profile(me, net);
                        }
                    }
                    if sub_item(ui, self.view == View::Friends, "Друзья").on_hover_text("Друзья, заявки в друзья, отправленные заявки").clicked() {
                        self.open_friends_page(0);
                    }
                    if sub_item(ui, self.view == View::Inventory && !self.inv_detached, "Инвентарь")
                        .on_hover_text("Инвентарь, кошелек и торговая площадка. Ctrl+щелчок - отдельным окном")
                        .clicked()
                    {
                        self.open_inventory(ctrl);
                    }
                    if sub_item(ui, self.view == View::Badges, "Значки и карточки").clicked() {
                        self.open_badges();
                    }
                    if sub_item(ui, self.view == View::Family, "Семья Steam").clicked() {
                        self.open_family();
                    }
                    // Открыт профиль друга - временный пункт с его именем.
                    if let Some(id) = self.social_ui.profile.filter(|id| Some(*id) != net.my_id && self.view == View::Profile) {
                        let name = net
                            .profiles
                            .get(&id)
                            .and_then(|p| p.as_ref().ok())
                            .and_then(|p| p.persona_name.clone())
                            .unwrap_or_else(|| "друг".into());
                        ui.separator();
                        let _ = sub_item(ui, true, &format!("Профиль: {}", crate::richtext::fix(ui, &name)));
                    }
                }
            }
        });
    }
}
