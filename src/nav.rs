//! Navigation as in the Steam client: a row of text menus, then back / forward and the four sections
//! (STORE, LIBRARY, COMMUNITY, profile) with their pages in a drop-down on hover.

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

/// A section of the top bar (STORE, LIBRARY...): Steam's capital semibold text, white on hover,
/// blue with a blue underline when selected. Compact: 16 px instead of Steam's 18 px.
pub(crate) fn top_item(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), theme::semibold(16.0), egui::Color32::WHITE);
    let pad = egui::vec2(7.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let color = if on { theme::ACCENT } else { theme::lerp_color(theme::TEXT, egui::Color32::WHITE, t) };
    ui.painter().galley_with_override_text_color(rect.min + pad, galley, color);
    if on {
        let y = rect.bottom() - 3.0;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left() + pad.x, y), egui::pos2(rect.right() - pad.x, y + 3.0)),
            1.5,
            theme::ACCENT,
        );
    }
    resp
}

/// A text menu of the first row ("Vaporust", "View", "Friends"...), as in the Steam title bar.
fn menu_title(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), egui::Color32::WHITE);
    let b = egui::Button::new("").frame(false).min_size(egui::vec2(galley.size().x + 12.0, 22.0));
    let r = egui::menu::menu_custom_button(ui, b, |ui| {
        ui.set_min_width(210.0);
        add(ui);
    });
    let resp = r.response;
    let color = if resp.hovered() { egui::Color32::WHITE } else { theme::TEXT2 };
    ui.painter().galley_with_override_text_color(resp.rect.center() - galley.size() / 2.0, galley, color);
}

/// An item of a hover drop-down under a top section.
fn drop_item(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let w = ui.available_width().clamp(170.0, 230.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::BUTTON_HOVER);
    }
    let color = if on { theme::ACCENT } else if resp.hovered() { egui::Color32::WHITE } else { theme::TEXT };
    ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), color);
    resp
}

/// What a drop-down item does.
#[derive(Clone, Copy)]
enum Go {
    StoreTab(Tab),
    News,
    Points,
    Cart,
    LibHome,
    Collections,
    Downloads,
    Screenshots,
    Stats,
    Storage,
    Music,
    Activity,
    Groups,
    Discussions,
    Workshop,
    Profile,
    Friends,
    Inventory,
    Badges,
    Family,
}

impl LauncherApp {
    /// Profile name for the top bar: the Steam name (if already known), otherwise "PROFILE".
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

    /// Back / forward history, as in Steam: every change of the page is remembered.
    pub(crate) fn nav_history_tick(&mut self) {
        let cur = (self.view, self.lib_page, self.collections_page);
        if cur != self.nav_last {
            self.nav_back.push(self.nav_last);
            if self.nav_back.len() > 50 {
                self.nav_back.remove(0);
            }
            self.nav_fwd.clear();
            self.nav_last = cur;
        }
    }

    fn nav_go(&mut self, to: (View, Option<u32>, bool)) {
        self.view = to.0;
        self.lib_page = to.1;
        self.collections_page = to.2;
        self.nav_last = to;
    }

    /// The whole top of the window, as in Steam: a row of text menus (window buttons, notifications
    /// and the account on the right), then back / forward and the sections.
    pub(crate) fn title_rows(&mut self, ui: &mut egui::Ui, net: &NetState) {
        self.nav_history_tick();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // ----- Row 1: menus -----
            ui.horizontal(|ui| {
                ui.set_height(28.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    crate::winframe::window_buttons(ui, 28.0);
                    if net.account.is_some() {
                        ui.add_space(6.0);
                        self.account_chip(ui, net);
                        ui.add_space(4.0);
                        self.steam_notifs_button(ui, net);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        self.menu_bar(ui, net);
                    });
                });
            });
            // ----- Row 2: back / forward and the sections -----
            ui.horizontal(|ui| {
                ui.set_height(36.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let back = theme::bare_icon(ui, theme::icon_back, 22.0, !self.nav_back.is_empty(), false).on_hover_text(tr!("Назад", "Back"));
                if back.clicked() {
                    if let Some(to) = self.nav_back.pop() {
                        self.nav_fwd.push(self.nav_last);
                        self.nav_go(to);
                    }
                }
                let fwd = theme::bare_icon(ui, theme::icon_forward, 22.0, !self.nav_fwd.is_empty(), false).on_hover_text(tr!("Вперед", "Forward"));
                if fwd.clicked() {
                    if let Some(to) = self.nav_fwd.pop() {
                        self.nav_back.push(self.nav_last);
                        self.nav_go(to);
                    }
                }
                ui.add_space(8.0);
                if net.account.is_some() {
                    self.sections(ui, net);
                } else {
                    self.account_panel(ui, net);
                }
            });
        });
    }

    /// STORE, LIBRARY, COMMUNITY, profile name - each with a drop-down of its pages on hover (as in Steam).
    fn sections(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let section = section_of(self.view);
        let full = !self.settings.games_only;
        if full {
            let r = top_item(ui, section == Some(Section::Store), tr!("Магазин", "Store"))
                .on_hover_text(tr!("Ctrl+щелчок - открыть магазин в отдельном окне", "Ctrl+click - open the store in a separate window"));
            if r.clicked() {
                self.open_store(None, ctrl);
            }
            self.drop_down(ui, &r, Section::Store, net);
        }
        let r = top_item(ui, section == Some(Section::Library), tr!("Библиотека", "Library"));
        if r.clicked() {
            self.close_extras(View::Library);
            self.view = View::Library;
            self.lib_page = None;
        }
        self.drop_down(ui, &r, Section::Library, net);
        if full {
            let r = top_item(ui, section == Some(Section::Community), tr!("Сообщество", "Community"));
            if r.clicked() {
                if self.community_ui.appid.is_some() {
                    self.view = View::Community;
                } else {
                    self.open_activity();
                }
            }
            self.drop_down(ui, &r, Section::Community, net);
            let name = self.profile_label(ui, net);
            let r = top_item(ui, section == Some(Section::Profile), &name);
            if r.clicked() {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
            self.drop_down(ui, &r, Section::Profile, net);
        }
    }

    /// The pages of a section in a drop-down that opens on hover and stays while the mouse is over it.
    pub(crate) fn drop_down(&mut self, ui: &mut egui::Ui, item: &egui::Response, s: Section, net: &NetState) {
        let id = egui::Id::new(("nav_drop", s as u8));
        let prev: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(id));
        let pointer = ui.ctx().pointer_hover_pos();
        let over = |r: egui::Rect| pointer.is_some_and(|p| r.contains(p));
        // A bridge between the item and the list, so moving the mouse down does not close it.
        let bridge = egui::Rect::from_min_max(item.rect.left_bottom() - egui::vec2(0.0, 2.0), item.rect.right_bottom() + egui::vec2(0.0, 4.0));
        let open = item.hovered() || over(bridge) || prev.is_some_and(|r| over(r.expand(6.0)));
        if !open {
            if prev.is_some() {
                ui.ctx().data_mut(|d| d.remove::<egui::Rect>(id));
            }
            return;
        }
        let mut go: Option<Go> = None;
        let area = egui::Area::new(id.with("area"))
            .order(egui::Order::Foreground)
            .fixed_pos(item.rect.left_bottom() + egui::vec2(0.0, 2.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).fill(theme::MENU).inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
                    ui.set_min_width(180.0);
                    ui.set_max_width(230.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let items = self.drop_items(s, net);
                    for (label, g, on) in items {
                        if drop_item(ui, &label, on).clicked() {
                            go = Some(g);
                        }
                    }
                });
            });
        ui.ctx().data_mut(|d| d.insert_temp(id, area.response.rect));
        if let Some(g) = go {
            ui.ctx().data_mut(|d| d.remove::<egui::Rect>(id));
            self.go(g, net, ui.input(|i| i.modifiers.ctrl));
        }
    }

    fn drop_items(&self, s: Section, net: &NetState) -> Vec<(String, Go, bool)> {
        let v = self.view;
        let tab = self.store_win.as_ref().map(|w| w.tab());
        let in_store = v == View::Store && !self.store_detached;
        let t = |ru: &str, en: &str| -> String { if crate::lang::en() { en.to_string() } else { ru.to_string() } };
        match s {
            Section::Store => vec![
                (t("Главная", "Home"), Go::StoreTab(Tab::Home), in_store && matches!(tab, Some(Tab::Home) | Some(Tab::Search))),
                (t("Скидки", "Specials"), Go::StoreTab(Tab::Deals), in_store && matches!(tab, Some(Tab::Deals) | Some(Tab::Events))),
                (t("Чарты", "Charts"), Go::StoreTab(Tab::Charts), in_store && tab == Some(Tab::Charts)),
                (t("Рекомендации", "Recommendations"), Go::StoreTab(Tab::Discovery), in_store && tab == Some(Tab::Discovery)),
                (t("Список желаемого", "Wishlist"), Go::StoreTab(Tab::Wishlist), in_store && tab == Some(Tab::Wishlist)),
                (t("Новости", "News"), Go::News, v == View::News),
                (t("Магазин очков", "Points Shop"), Go::Points, v == View::Points),
                (t("Корзина", "Cart"), Go::Cart, v == View::Cart),
            ],
            Section::Library => vec![
                (t("Главная", "Home"), Go::LibHome, v == View::Library && self.lib_page.is_none() && !self.collections_page),
                (t("Коллекции", "Collections"), Go::Collections, v == View::Library && self.collections_page),
                (t("Загрузки", "Downloads"), Go::Downloads, v == View::Downloads),
                (t("Скриншоты", "Screenshots"), Go::Screenshots, v == View::Screenshots),
                (t("Статистика", "Statistics"), Go::Stats, v == View::Stats),
                (t("Хранилище", "Storage"), Go::Storage, v == View::Storage),
                (t("Саундтреки", "Soundtracks"), Go::Music, v == View::Music),
            ],
            Section::Community => {
                let mut l = vec![
                    (t("Активность друзей", "Friends activity"), Go::Activity, v == View::Activity),
                    (t("Групповые чаты", "Group chats"), Go::Groups, v == View::Groups),
                ];
                if let Some((_, name)) = &self.community_ui.appid {
                    l.push((format!("{}: {name}", t("Обсуждения", "Discussions")), Go::Discussions, v == View::Community));
                }
                if let Some((_, name)) = self.ws_open.as_ref().filter(|_| !self.ws_detached) {
                    l.push((format!("{}: {name}", t("Мастерская", "Workshop")), Go::Workshop, v == View::Workshop));
                }
                l
            }
            Section::Profile => vec![
                (t("Профиль", "Profile"), Go::Profile, v == View::Profile && self.social_ui.profile == net.my_id),
                (t("Друзья", "Friends"), Go::Friends, v == View::Friends),
                (t("Инвентарь", "Inventory"), Go::Inventory, v == View::Inventory),
                (t("Значки и карточки", "Badges"), Go::Badges, v == View::Badges),
                (t("Семья Steam", "Steam Family"), Go::Family, v == View::Family),
            ],
        }
    }

    fn go(&mut self, g: Go, net: &NetState, ctrl: bool) {
        match g {
            Go::StoreTab(t) => {
                self.open_store(None, ctrl);
                if let Some(w) = self.store_win.as_mut() {
                    w.set_tab(t);
                }
            }
            Go::News => {
                self.close_extras(View::News);
                self.view = View::News;
            }
            Go::Points => self.open_points(),
            Go::Cart => self.open_cart(),
            Go::LibHome => {
                self.close_extras(View::Library);
                self.view = View::Library;
                self.lib_page = None;
                self.collections_page = false;
            }
            Go::Collections => {
                self.close_extras(View::Library);
                self.view = View::Library;
                self.lib_page = None;
                self.collections_page = true;
            }
            Go::Downloads => {
                self.close_extras(View::Downloads);
                self.view = View::Downloads;
            }
            Go::Screenshots => self.open_shots(None),
            Go::Stats => self.open_stats(),
            Go::Storage => self.open_storage(),
            Go::Music => self.open_music(),
            Go::Activity => self.open_activity(),
            Go::Groups => self.open_groups(),
            Go::Discussions => self.view = View::Community,
            Go::Workshop => self.view = View::Workshop,
            Go::Profile => {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
            Go::Friends => self.open_friends_page(0),
            Go::Inventory => self.open_inventory(ctrl),
            Go::Badges => self.open_badges(),
            Go::Family => self.open_family(),
        }
    }

    /// The account on the right of the first row: avatar, name and wallet, with a menu (as in Steam).
    fn account_chip(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let me = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok());
        let name = me.and_then(|p| p.persona_name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| net.account.clone().unwrap_or_default());
        let name = crate::richtext::fix(ui, &name);
        let wallet = net.inv.lock().unwrap().wallet.as_ref().and_then(|w| w.as_ref().ok()).map(|w| crate::invent::money(w.balance_cents, w.currency));
        // The wallet is loaded once per account right away, as Steam shows it next to the name.
        static WALLET_ASKED: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
        if wallet.is_none() && net.my_id.is_some() {
            let mut asked = WALLET_ASKED.lock().unwrap();
            if *asked != net.my_id {
                *asked = net.my_id;
                let _ = self.tx.send(crate::net::Cmd::Inv(crate::invent::InvCmd::Wallet));
            }
        }
        let galley = ui.painter().layout_no_wrap(name.clone(), egui::FontId::proportional(12.5), egui::Color32::WHITE);
        let wallet_g = wallet.map(|w| ui.painter().layout_no_wrap(w, egui::FontId::proportional(12.0), egui::Color32::WHITE));
        let w = 22.0 + 6.0 + galley.size().x + 14.0 + wallet_g.as_ref().map_or(0.0, |g| g.size().x + 8.0) + 8.0;
        let b = egui::Button::new("").frame(false).min_size(egui::vec2(w, 24.0));
        let mut action: Option<u8> = None;
        let r = egui::menu::menu_custom_button(ui, b, |ui| {
            ui.set_min_width(200.0);
            if ui.button(tr!("Открыть профиль", "View my profile")).clicked() {
                action = Some(0);
                ui.close_menu();
            }
            if ui.button(tr!("Об аккаунте", "Account details")).clicked() {
                action = Some(1);
                ui.close_menu();
            }
            if crate::exp::button(ui, tr!("Редактировать профиль", "Edit profile")).clicked() {
                action = Some(2);
                ui.close_menu();
            }
            if ui.button(tr!("Сменить аккаунт...", "Change account...")).clicked() {
                action = Some(3);
                ui.close_menu();
            }
            ui.separator();
            if ui.add_enabled(!net.busy, egui::Button::new(tr!("Выйти из аккаунта", "Sign out"))).clicked() {
                action = Some(4);
                ui.close_menu();
            }
        });
        let rect = r.response.rect;
        let hovered = r.response.hovered();
        if hovered {
            ui.painter().rect_filled(rect, 2.0, theme::BUTTON);
        }
        let p = ui.painter();
        let av = egui::Rect::from_min_size(egui::pos2(rect.left() + 3.0, rect.center().y - 10.0), egui::vec2(20.0, 20.0));
        p.rect_filled(av, 2.0, theme::BUTTON);
        if let Some(url) = me.and_then(|p| p.avatar_url.clone()) {
            egui::Image::new(url).paint_at(ui, av);
        }
        let mut x = av.right() + 6.0;
        let color = if hovered { egui::Color32::WHITE } else { theme::ONLINE };
        let gw = galley.size().x;
        ui.painter().galley_with_override_text_color(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
        x += gw + 2.0;
        theme::icon_caret(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 5.0, rect.center().y + 1.0), egui::vec2(10.0, 10.0)), theme::WEAK);
        x += 14.0;
        if let Some(g) = wallet_g {
            ui.painter().galley_with_override_text_color(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, theme::WEAK);
        }
        match action {
            Some(0) => {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
            Some(1) => {
                self.settings_section = 0;
                self.view = View::Settings;
            }
            Some(2) => self.open_profile_edit(),
            Some(3) => self.accounts_open = true,
            Some(4) => {
                let _ = self.tx.send(crate::net::Cmd::Logout);
            }
            _ => {}
        }
    }

    /// The text menus of the first row: Vaporust, View, Friends, Games, Help (like Steam, View, Friends, Games, Help).
    fn menu_bar(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let launching = self.launching.load(std::sync::atomic::Ordering::Relaxed);
        let closing = self.closing.load(std::sync::atomic::Ordering::Relaxed);
        let steam_running = !closing && self.steam_running();
        let running = crate::local::RUNNING.lock().unwrap().clone().filter(|_| launching && !closing);
        let force_ok = running.as_ref().is_some_and(|(a, _, _)| {
            self.stop_asked.is_some_and(|(id, t)| id == *a && t.elapsed() >= std::time::Duration::from_secs(10))
        });
        let logged_in = net.account.is_some();
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        // The program icon, as the Steam logo at the start of the row.
        let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
        egui::Image::new(egui::include_image!("../assets/logo.png")).paint_at(ui, r.shrink(3.0));
        menu_title(ui, "Vaporust", |ui| {
            if ui.button(tr!("Настройки", "Settings")).clicked() {
                self.view = View::Settings;
                ui.close_menu();
            }
            if ui.button(tr!("Аккаунты...", "Accounts...")).on_hover_text(tr!("Несколько аккаунтов Steam и быстрое переключение", "Several Steam accounts and quick switching")).clicked() {
                self.accounts_open = true;
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
                    .on_hover_text(tr!("Запасной выход, если Vaporust не заметил закрытия игры.\nSteam остается открытым.", "A fallback if Vaporust did not notice the game closing.\nSteam stays open."))
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
            if logged_in && ui.add_enabled(!net.busy, egui::Button::new(tr!("Выйти из аккаунта", "Sign out"))).clicked() {
                let _ = self.tx.send(crate::net::Cmd::Logout);
                ui.close_menu();
            }
            if ui.button(tr!("Выход", "Exit")).clicked() {
                crate::tray::request_exit();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close_menu();
            }
        });
        menu_title(ui, tr!("Вид", "View"), |ui| {
            if logged_in && !self.settings.games_only && ui.button(tr!("Магазин", "Store")).clicked() {
                self.open_store(None, ctrl);
                ui.close_menu();
            }
            if ui.button(tr!("Библиотека", "Library")).clicked() {
                self.go(Go::LibHome, net, false);
                ui.close_menu();
            }
            if logged_in && !self.settings.games_only && ui.button(tr!("Сообщество", "Community")).clicked() {
                self.open_activity();
                ui.close_menu();
            }
            if ui.button(tr!("Загрузки", "Downloads")).clicked() {
                self.go(Go::Downloads, net, false);
                ui.close_menu();
            }
            if logged_in && ui.button(tr!("Скриншоты", "Screenshots")).clicked() {
                self.open_shots(None);
                ui.close_menu();
            }
            ui.separator();
            let mut table = self.settings.view == crate::settings::View::List;
            if ui.checkbox(&mut table, tr!("Компактный режим (таблица)", "Small mode (table)")).changed() {
                self.settings.view = if table { crate::settings::View::List } else { crate::settings::View::Grid };
                ui.close_menu();
            }
            if logged_in && !self.settings.games_only && ui.checkbox(&mut self.show_friends, tr!("Друзья и чат", "Friends & Chat")).changed() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button(tr!("Настройки", "Settings")).clicked() {
                self.view = View::Settings;
                ui.close_menu();
            }
        });
        if logged_in && !self.settings.games_only {
            menu_title(ui, tr!("Друзья", "Friends"), |ui| {
                if ui.button(tr!("Список друзей", "View friends list")).clicked() {
                    self.show_friends = true;
                    ui.close_menu();
                }
                if ui.button(tr!("Добавить друга...", "Add a friend...")).clicked() {
                    self.social_ui.add_open = true;
                    ui.close_menu();
                }
                if ui.button(tr!("Друзья и заявки", "Friends and requests")).clicked() {
                    self.open_friends_page(0);
                    ui.close_menu();
                }
                if ui.button(tr!("Активность друзей", "Friends activity")).clicked() {
                    self.open_activity();
                    ui.close_menu();
                }
                if ui.button(tr!("Групповые чаты", "Group chats")).clicked() {
                    self.open_groups();
                    ui.close_menu();
                }
                ui.separator();
                let before = self.settings.persona;
                for (state, label) in crate::settings::PERSONAS() {
                    ui.radio_value(&mut self.settings.persona, state, label);
                }
                if self.settings.persona != before {
                    let _ = self.tx.send(crate::net::Cmd::SetPersona(self.settings.persona));
                    ui.close_menu();
                }
            });
        }
        menu_title(ui, tr!("Игры", "Games"), |ui| {
            if ui.button(tr!("Библиотека игр", "View games library")).clicked() {
                self.go(Go::LibHome, net, false);
                ui.close_menu();
            }
            if logged_in && ui.button(tr!("Активировать в Steam...", "Activate a product on Steam...")).clicked() {
                self.key_input = Some(String::new());
                self.key_refreshed = false;
                self.net.lock().unwrap().key_result = None;
                ui.close_menu();
            }
            if ui.button(tr!("Добавить стороннюю игру...", "Add a non-Steam game...")).clicked() {
                self.custom_edit = Some(crate::custom::Edit { to_steam: self.settings.custom_to_steam, ..Default::default() });
                ui.close_menu();
            }
            if ui.button(tr!("Взять сторонние игры из Steam", "Import non-Steam games from Steam")).clicked() {
                self.import_steam_shortcuts();
                ui.close_menu();
            }
            ui.separator();
            if ui.button(tr!("Загрузки", "Manage downloads")).clicked() {
                self.go(Go::Downloads, net, false);
                ui.close_menu();
            }
        });
        menu_title(ui, tr!("Справка", "Help"), |ui| {
            if ui.button(tr!("Vaporust на GitHub", "Vaporust on GitHub")).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/felixsmtnk/Vaporust"));
                ui.close_menu();
            }
            if ui.button(tr!("Проверить обновления", "Check for updates")).clicked() {
                self.check_updates_now(ui.ctx());
                ui.close_menu();
            }
            if ui.button(tr!("Сообщить об ошибке...", "Report a problem...")).clicked() {
                self.settings_section = crate::settingsui::SECTIONS().len() as u8 - 1;
                self.view = View::Settings;
                ui.close_menu();
            }
            if ui.button(tr!("О программе", "About Vaporust")).clicked() {
                self.settings_section = crate::settingsui::SECTIONS().len() as u8 - 1;
                self.view = View::Settings;
                ui.close_menu();
            }
        });
    }
}
