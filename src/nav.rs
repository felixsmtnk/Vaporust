//! Navigation as in the Steam client: four sections on top (STORE, LIBRARY, COMMUNITY,
//! profile), the selected section's subsections below. All sections are always visible, tabs do not
//! appear and disappear.

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
        View::Store | View::News | View::Points | View::Cart => Section::Store,
        View::Library | View::Screenshots | View::Stats | View::Storage | View::Downloads | View::Achievements | View::Props | View::Music => Section::Library,
        View::Activity | View::Groups | View::Group | View::Community | View::Workshop => Section::Community,
        View::Inventory | View::Badges | View::Family | View::Profile | View::Friends | View::ProfileEdit => Section::Profile,
        View::Settings => return None,
    })
}

/// A text menu item drawn by hand: the text lights up smoothly on hover, the underline grows from
/// the middle (on hover - dim, for the selected item - full and blue).
fn nav_text(ui: &mut egui::Ui, on: bool, text: &str, size: f32, rest: egui::Color32, line: f32) -> egui::Response {
    let font = egui::FontId::proportional(size);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, egui::Color32::WHITE);
    let pad = egui::vec2(6.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let color = if on { theme::ACCENT } else { theme::lerp_color(rest, egui::Color32::WHITE, t) };
    ui.painter().galley_with_override_text_color(rect.min + pad, galley, color);
    let u = if on { 1.0 } else { t * 0.6 };
    if u > 0.01 {
        let half = (rect.width() / 2.0 - pad.x) * u;
        let y = rect.bottom() + 1.0;
        let c = if on { theme::ACCENT } else { theme::lerp_color(theme::TOP, theme::WEAK, t) };
        ui.painter().line_segment([egui::pos2(rect.center().x - half, y), egui::pos2(rect.center().x + half, y)], egui::Stroke::new(line, c));
    }
    resp
}

/// Large top bar item (STORE, LIBRARY...).
pub(crate) fn top_item(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    nav_text(ui, on, text, 16.5, theme::TEXT, 2.0)
}

/// Submenu item.
fn sub_item(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    nav_text(ui, on, text, 14.0, theme::WEAK, 1.5)
}

impl LauncherApp {
    /// Profile name for the top bar: the Steam name (if already known), otherwise "ПРОФИЛЬ".
    fn profile_label(&mut self, ui: &egui::Ui, net: &NetState) -> String {
        let Some(me) = net.my_id else { return tr!("ПРОФИЛЬ", "PROFILE").into() };
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
        .unwrap_or_else(|| tr!("ПРОФИЛЬ", "PROFILE").into())
    }

    /// Top bar: Vaporust menu, four sections, Steam notifications on the right.
    pub(crate) fn top_nav(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let section = section_of(self.view);
        // Game and Steam: actions that used to be in the bottom bar.
        let launching = self.launching.load(std::sync::atomic::Ordering::Relaxed);
        let closing = self.closing.load(std::sync::atomic::Ordering::Relaxed);
        let steam_running = !closing && self.steam_running();
        let running = crate::local::RUNNING.lock().unwrap().clone().filter(|_| launching && !closing);
        let force_ok = running.as_ref().is_some_and(|(a, _, _)| {
            self.stop_asked.is_some_and(|(id, t)| id == *a && t.elapsed() >= std::time::Duration::from_secs(10))
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            // Program menu - plain text, like "Steam" in the client.
            egui::menu::menu_custom_button(ui, egui::Button::new(egui::RichText::new("Vaporust").size(14.0)).frame(false), |ui| {
                ui.set_min_width(190.0);
                if ui.button(tr!("Настройки", "Settings")).clicked() {
                    self.view = View::Settings;
                    ui.close_menu();
                }
                if let Some((appid, name, _)) = &running {
                    ui.separator();
                    if ui
                        .button(trf!("Закрыть «{name}»", "Close \"{name}\""))
                        .on_hover_text(tr!("Просит игру закрыться, как крестик окна или Alt+F4", "Asks the game to close, like the window's close button or Alt+F4"))
                        .clicked()
                    {
                        self.stop_running(*appid, false);
                        ui.close_menu();
                    }
                    if force_ok
                        && ui
                            .button(tr!("Закрыть игру принудительно", "Force-close the game"))
                            .on_hover_text(tr!("Сразу завершает процессы игры, если она зависла. Несохраненное пропадет.", "Ends the game processes at once if it hangs. Unsaved progress is lost."))
                            .clicked()
                    {
                        self.stop_running(*appid, true);
                        ui.close_menu();
                    }
                }
                if launching && !running.as_ref().is_some_and(|r| crate::custom::is_custom(r.0)) {
                    if running.is_none() {
                        ui.separator();
                    }
                    if ui
                        .button(tr!("Перестать следить за игрой", "Stop following the game"))
                        .on_hover_text(tr!("Запасной выход, если Vaporust не заметил закрытия игры.
Steam остается открытым.", "A fallback if Vaporust did not notice the game closing.\nSteam stays open."))
                        .clicked()
                    {
                        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        ui.close_menu();
                    }
                }
                if steam_running {
                    if !launching {
                        ui.separator();
                    }
                    if ui.button(tr!("Закрыть Steam", "Close Steam")).on_hover_text(tr!("Закрывает Steam (если он завис - принудительно)", "Closes Steam (forcibly if it hangs)")).clicked() {
                        self.close_steam(ui.ctx());
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button(tr!("Аккаунты...", "Accounts...")).on_hover_text(tr!("Несколько аккаунтов Steam и быстрое переключение", "Several Steam accounts and quick switching")).clicked() {
                    self.accounts_open = true;
                    ui.close_menu();
                }
                if crate::exp::button(ui, tr!("Редактировать профиль", "Edit profile")).clicked() {
                    self.open_profile_edit();
                    ui.close_menu();
                }
                ui.separator();
                if ui.add_enabled(!net.busy, egui::Button::new(tr!("Выйти из аккаунта", "Log out"))).clicked() {
                    let _ = self.tx.send(crate::net::Cmd::Logout);
                    ui.close_menu();
                }
                if ui.button(tr!("Закрыть Vaporust", "Quit Vaporust")).clicked() {
                    crate::tray::request_exit();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });
            if top_item(ui, section == Some(Section::Store), tr!("МАГАЗИН", "STORE"))
                .on_hover_text(tr!("Ctrl+щелчок - открыть магазин в отдельном окне", "Ctrl+click - open the store in a separate window"))
                .clicked()
            {
                self.open_store(None, ctrl);
            }
            if top_item(ui, section == Some(Section::Library), tr!("БИБЛИОТЕКА", "LIBRARY")).clicked() {
                self.close_extras(View::Library);
                self.view = View::Library;
                self.lib_page = None;
            }
            if top_item(ui, section == Some(Section::Community), tr!("СООБЩЕСТВО", "COMMUNITY")).clicked() {
                if self.community_ui.appid.is_some() {
                    self.view = View::Community;
                } else {
                    self.open_activity();
                }
            }
            let name = self.profile_label(ui, net);
            if top_item(ui, section == Some(Section::Profile), &name).on_hover_text(tr!("Ваш профиль, инвентарь, значки, семья Steam", "Your profile, inventory, badges, Steam Family")).clicked() {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
        });
    }

    /// Subsections of the selected section (the row under the top bar).
    pub(crate) fn sub_nav(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(section) = section_of(self.view) else {
            ui.label(egui::RichText::new(tr!("Настройки", "Settings")).size(13.5).color(egui::Color32::WHITE));
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
                        (Tab::Home, tr!("Главная", "Home")),
                        (Tab::Deals, tr!("Скидки", "Specials")),
                        (Tab::Charts, tr!("Чарты", "Charts")),
                        (Tab::Discovery, tr!("Рекомендации", "Recommendations")),
                        (Tab::Wishlist, tr!("Желаемое", "Wishlist")),
                    ] {
                        let on = in_store && !page && (tab == t || (t == Tab::Deals && tab == Tab::Events) || (t == Tab::Home && tab == Tab::Search));
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
                    if sub_item(ui, self.view == View::News, tr!("Новости", "News")).on_hover_text(tr!("Новости ваших игр одной лентой", "News of your games in one feed")).clicked() {
                        self.close_extras(View::News);
                        self.view = View::News;
                    }
                    if sub_item(ui, self.view == View::Points, tr!("Магазин очков", "Points Shop"))
                        .on_hover_text(tr!("Очки Steam: эффекты чата, стикеры, рамки", "Steam Points: chat effects, stickers, frames"))
                        .clicked()
                    {
                        self.open_points();
                    }
                    if sub_item(ui, self.view == View::Cart, tr!("Корзина", "Cart")).on_hover_text(tr!("Корзина магазина Steam", "Steam store cart")).clicked() {
                        self.open_cart();
                    }
                }
                Section::Library => {
                    if sub_item(ui, self.view == View::Screenshots, tr!("Скриншоты", "Screenshots")).clicked() {
                        self.open_shots(None);
                    }
                    if sub_item(ui, self.view == View::Stats, tr!("Статистика", "Statistics"))
                        .on_hover_text(tr!("Время в играх: по дням, за две недели, больше всего часов", "Playtime: by day, in two weeks, most hours"))
                        .clicked()
                    {
                        self.open_stats();
                    }
                    if sub_item(ui, self.view == View::Storage, tr!("Хранилище", "Storage"))
                        .on_hover_text(tr!("Сколько места занимают игры, остатки удаленных игр", "How much space the games take, leftovers of removed games"))
                        .clicked()
                    {
                        self.open_storage();
                    }
                    if sub_item(ui, self.view == View::Music, tr!("Саундтреки", "Soundtracks")).on_hover_text(tr!("Музыка из ваших игр и купленные саундтреки", "Music from your games and purchased soundtracks")).clicked() {
                        self.open_music();
                    }
                }
                Section::Community => {
                    if sub_item(ui, self.view == View::Activity, tr!("Активность друзей", "Friends activity")).clicked() {
                        self.open_activity();
                    }
                    if sub_item(ui, self.view == View::Groups, tr!("Групповые чаты", "Group chats")).clicked() {
                        self.open_groups();
                    }
                    if let Some((_, name)) = self.community_ui.appid.clone() {
                        ui.separator();
                        let label = trf!("Обсуждения: {}", "Discussions: {}", crate::richtext::fix(ui, &name));
                        if sub_item(ui, self.view == View::Community, &label).clicked() {
                            self.view = View::Community;
                        }
                    }
                    if let Some((_, name)) = self.ws_open.clone().filter(|_| !self.ws_detached) {
                        let label = trf!("Мастерская: {}", "Workshop: {}", crate::richtext::fix(ui, &name));
                        if sub_item(ui, self.view == View::Workshop, &label).clicked() {
                            self.view = View::Workshop;
                        }
                    }
                }
                Section::Profile => {
                    let mine = self.view == View::Profile && self.social_ui.profile == net.my_id;
                    if sub_item(ui, mine, tr!("Мой профиль", "My profile")).clicked() {
                        if let Some(me) = net.my_id {
                            self.open_profile(me, net);
                        }
                    }
                    if sub_item(ui, self.view == View::Friends, tr!("Друзья", "Friends")).on_hover_text(tr!("Друзья, заявки в друзья, отправленные заявки", "Friends, friend requests, sent requests")).clicked() {
                        self.open_friends_page(0);
                    }
                    if sub_item(ui, self.view == View::Inventory && !self.inv_detached, tr!("Инвентарь", "Inventory"))
                        .on_hover_text(tr!("Инвентарь, кошелек и торговая площадка. Ctrl+щелчок - отдельным окном", "Inventory, wallet and Community Market. Ctrl+click - separate window"))
                        .clicked()
                    {
                        self.open_inventory(ctrl);
                    }
                    if sub_item(ui, self.view == View::Badges, tr!("Значки и карточки", "Badges and cards")).clicked() {
                        self.open_badges();
                    }
                    if sub_item(ui, self.view == View::Family, tr!("Семья Steam", "Steam Family")).clicked() {
                        self.open_family();
                    }
                    // A friend's profile is open - a temporary item with their name.
                    if let Some(id) = self.social_ui.profile.filter(|id| Some(*id) != net.my_id && self.view == View::Profile) {
                        let name = net
                            .profiles
                            .get(&id)
                            .and_then(|p| p.as_ref().ok())
                            .and_then(|p| p.persona_name.clone())
                            .unwrap_or_else(|| tr!("друг", "friend").into());
                        ui.separator();
                        let _ = sub_item(ui, true, &trf!("Профиль: {}", "Profile: {}", crate::richtext::fix(ui, &name)));
                    }
                }
            }
        });
    }
}
