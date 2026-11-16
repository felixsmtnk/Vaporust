//! "Steam Store" window (a separate Windows window): search, discounts, wishlist
//! and the game page (price, description, screenshots, news, reviews, similar games).
//! Purchase happens on the Steam page in a browser: the launcher does not process payments.

use crate::net::{Cmd, NetState};
use crate::store::{self, StoreCmd};
use crate::LauncherApp;
use aurelia::steam_client::StoreAppInfo;
use eframe::egui;
use std::collections::HashSet;

thread_local! {
    /// Reviews whose page was already requested (full text, hours at review time).
    static REVIEW_ASKED: std::cell::RefCell<HashSet<String>> = Default::default();
}

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    /// Home: shelves of discounts, top sellers, new releases and recommendations.
    Home,
    Search,
    Deals,
    Events,
    Wishlist,
    /// A friend's wishlist (temporary tab).
    FriendWishlist,
    /// Steam charts (which one - StoreWin.chart).
    Charts,
    /// Discovery queue.
    Discovery,
}

/// Sorting of store lists.
#[derive(PartialEq, Clone, Copy)]
enum Sort {
    Default,
    Name,
    PriceAsc,
    PriceDesc,
    Discount,
    Reviews,
    Newest,
}

impl Sort {
    const ALL: [Sort; 7] = [Sort::Default, Sort::Name, Sort::PriceAsc, Sort::PriceDesc, Sort::Discount, Sort::Reviews, Sort::Newest];
    fn label(self) -> &'static str {
        match self {
            Sort::Default => tr!("как в Steam", "as in Steam"),
            Sort::Name => tr!("по названию", "by name"),
            Sort::PriceAsc => tr!("сначала дешевые", "cheapest first"),
            Sort::PriceDesc => tr!("сначала дорогие", "most expensive first"),
            Sort::Discount => tr!("по размеру скидки", "by discount"),
            Sort::Reviews => tr!("по отзывам", "by reviews"),
            Sort::Newest => tr!("сначала новые", "newest first"),
        }
    }
}

/// Percent of positive reviews from "Very Positive (83% positive, 4238 reviews)".
fn review_pct(a: &StoreAppInfo) -> i32 {
    a.review_summary
        .as_deref()
        .and_then(|s| s.split('(').nth(1))
        .and_then(|s| s.split('%').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(-1)
}

/// Price for sorting: free - 0, without a price - at the end.
fn price_key(a: &StoreAppInfo) -> i64 {
    if a.is_free {
        0
    } else {
        a.price_cents.unwrap_or(i64::MAX)
    }
}

pub struct StoreWin {
    sort: Sort,
    only_discount: bool,
    only_free: bool,
    hide_owned: bool,
    tab: Tab,
    query: String,
    /// Open game pages (back - pop the last one).
    pages: Vec<u32>,
    /// Details were already requested (so we do not send a command every frame).
    asked: HashSet<u32>,
    deals_asked: bool,
    events_asked: bool,
    /// Which chart is open (index in store::CHARTS()) and which were already requested.
    chart: u8,
    charts_asked: HashSet<u8>,
    discovery_asked: bool,
    /// Search within the wishlist.
    wish_query: String,
}

const GREEN: egui::Color32 = egui::Color32::from_rgb(140, 200, 80);
const GRAY: egui::Color32 = egui::Color32::from_rgb(150, 150, 150);
const ROW_H: f32 = 88.0;

fn store_url(appid: u32) -> String {
    format!("https://store.steampowered.com/app/{appid}/")
}

/// "Overwhelmingly Positive (96% positive, 118575 reviews)" -> in Russian.
fn review_ru(s: &str) -> String {
    s.replace(" positive", tr!(" положительных", " positive")).replace(" reviews", tr!(" отзывов", " reviews"))
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

/// Price considering the discount, free and "coming soon".
fn price_ui(ui: &mut egui::Ui, a: &StoreAppInfo) {
    if a.coming_soon {
        ui.weak(trf!("Скоро{}", "Coming soon{}", a.release_date.as_ref().map(|d| format!(": {}", ru_date(d))).unwrap_or_default()));
    } else if a.is_free {
        ui.label(tr!("Бесплатно", "Free"));
    } else if let Some(p) = &a.price {
        if a.discount_pct > 0 {
            ui.label(
                egui::RichText::new(format!("-{}%", a.discount_pct))
                    .color(egui::Color32::BLACK)
                    .background_color(GREEN),
            );
            if let Some(o) = &a.original_price {
                ui.label(egui::RichText::new(o).strikethrough().color(GRAY));
            }
            ui.strong(p);
        } else {
            ui.label(p);
        }
    } else if a.region_locked {
        ui.weak(tr!("недоступна в регионе", "not available in your region"));
    }
}

impl StoreWin {
    pub fn new() -> Self {
        Self {
            sort: Sort::Default,
            only_discount: false,
            only_free: false,
            hide_owned: false,
            tab: Tab::Home,
            query: String::new(),
            pages: Vec::new(),
            asked: HashSet::new(),
            deals_asked: false,
            events_asked: false,
            chart: 0,
            charts_asked: HashSet::new(),
            discovery_asked: false,
            wish_query: String::new(),
        }
    }
}

impl LauncherApp {
    /// Open the store: in the main window or (detached) in a separate one.
    /// The store on the "Wishlist: <friend>" tab.
    pub(crate) fn open_friend_wishlist(&mut self, id: u64, name: String) {
        self.open_store(None, false);
        self.net.lock().unwrap().store.lock().unwrap().friend_wishlist = Some((id, name, None));
        let _ = self.tx.send(Cmd::Store(StoreCmd::FriendWishlist(id)));
        if let Some(w) = self.store_win.as_mut() {
            w.tab = Tab::FriendWishlist;
            w.pages.clear();
        }
    }

    /// The store on the "Wishlist" tab (click on a discount notification).
    pub(crate) fn open_wishlist(&mut self) {
        self.open_store(None, false);
        if let Some(w) = self.store_win.as_mut() {
            w.tab = Tab::Wishlist;
            w.pages.clear();
        }
    }

    pub(crate) fn open_store(&mut self, page: Option<u32>, detached: bool) {
        if detached {
            self.store_detached = true;
        } else {
            self.store_detached = false;
            self.view = crate::View::Store;
        }
        let win = self.store_win.get_or_insert_with(StoreWin::new);
        if let Some(appid) = page {
            if win.pages.last() != Some(&appid) {
                win.pages.push(appid);
                let _ = self.tx.send(Cmd::Store(StoreCmd::Page(appid)));
            }
        }
        // Own wishlist - to mark games as "wishlisted".
        let _ = self.tx.send(Cmd::Store(StoreCmd::Wishlist));
    }

    /// The store in a separate window (opened with Ctrl).
    pub(crate) fn store_window(&mut self, ctx: &egui::Context, net: &NetState) {
        if self.store_win.is_none() || !self.store_detached {
            return;
        }
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_store"),
            egui::ViewportBuilder::default()
                .with_title(tr!("Магазин Steam - Vaporust", "Steam Store - Vaporust"))
                .with_inner_size([900.0, 640.0])
                .with_min_inner_size([600.0, 400.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
                .with_decorations(crate::winframe::system()),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.store_contents(ui, net));
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.store_win = None;
            self.store_detached = false;
        }
    }

    /// Store contents: both in the main window and in the separate one.
    pub(crate) fn store_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let shared = net.store.clone();
        let mut st = shared.lock().unwrap();
        // Price region: saved in settings, otherwise the one Steam determined by IP.
        if st.country.is_empty() {
            if let Some(cc) = self.settings.store_country.clone() {
                st.country = cc;
            }
        }
        let owned: HashSet<u32> = net.owned.iter().map(|g| g.app_id).collect();
        let installed: HashSet<u32> = self.games.iter().filter(|g| g.ready).map(|g| g.appid).collect();
        let wish: HashSet<u32> = match &st.wishlist {
            Some(Ok(l)) => l.iter().copied().collect(),
            _ => HashSet::new(),
        };
        let mut cmds: Vec<Cmd> = Vec::new();
        let mut open_page: Option<u32> = None;
        let mut play: Option<u32> = None;
        let mut close_friend_wish = false;
        let mut install: Option<(u32, String)> = None;
        let own_tabs = self.store_detached;
        let Some(w) = self.store_win.as_mut() else { return };

        egui::TopBottomPanel::top("store_top").show_inside(ui, |ui| {
            ui.add_space(6.0);
            // Row 1: store sections and (on the right) the price region. In the main window the sections are in the
            // submenu on top, so the row is needed only on a game page and for a friend's wishlist.
            let row1 = own_tabs || !w.pages.is_empty() || st.friend_wishlist.is_some();
            if row1 {
            ui.horizontal(|ui| {
                if !w.pages.is_empty() && ui.button(tr!("< Назад", "< Back")).clicked() {
                    w.pages.pop();
                }
                // "Discounts" - one section with two lists (Deals and Events).
                let in_deals = matches!(w.tab, Tab::Deals | Tab::Events);
                // In the main window these sections are in the "STORE" submenu on top (nav.rs); here only
                // in the separate store window.
                let tabs: &[(Tab, &str)] = if own_tabs {
                    &[
                        (Tab::Home, tr!("Главная", "Home")),
                        (Tab::Deals, tr!("Скидки", "Specials")),
                        (Tab::Charts, tr!("Чарты", "Charts")),
                        (Tab::Discovery, tr!("Рекомендации", "Recommendations")),
                        (Tab::Wishlist, tr!("Желаемое", "Wishlist")),
                    ]
                } else {
                    &[]
                };
                for &(tab, label) in tabs {
                    let on = if tab == Tab::Deals { in_deals } else { w.tab == tab };
                    let text = egui::RichText::new(label).size(15.0);
                    if ui.selectable_label(on && w.pages.is_empty(), text).clicked() {
                        if !(tab == Tab::Deals && in_deals) {
                            w.tab = tab;
                        }
                        w.pages.clear();
                    }
                }
                // A friend's wishlist - a temporary section while it is open.
                if let Some((_, name, _)) = st.friend_wishlist.clone() {
                    let text = egui::RichText::new(trf!("Желаемое: {name}", "Wishlist: {name}")).size(15.0);
                    if ui.selectable_label(w.tab == Tab::FriendWishlist && w.pages.is_empty(), text).clicked() {
                        w.tab = Tab::FriendWishlist;
                        w.pages.clear();
                    }
                    if ui.small_button("✖").on_hover_text(tr!("Закрыть", "Close")).clicked() {
                        close_friend_wish = true;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    region_combo(ui, &st.country.clone(), &mut self.settings.store_country, &mut cmds, w);
                });
            });
            }
            // Row 2 (lists only): search or subsection on the left, order and filters on the right.
            if w.pages.is_empty() {
                ui.add_space(4.0);
                // In a narrow window order, filters and region go to a separate row below, otherwise they overlap the search.
                let narrow = ui.available_width() < 900.0;
                macro_rules! right_group {
                    ($ui:expr) => {
                        if !row1 {
                            region_combo($ui, &st.country.clone(), &mut self.settings.store_country, &mut cmds, w);
                        }
                        if w.tab != Tab::Home {
                            let active = [w.only_discount, w.only_free, w.hide_owned].iter().filter(|b| **b).count();
                            let label = if active > 0 { trf!("Фильтры ({active})", "Filters ({active})") } else { tr!("Фильтры", "Filters").to_string() };
                            $ui.menu_button(label, |ui| {
                                ui.checkbox(&mut w.only_discount, tr!("Только со скидкой", "Discounted only"));
                                ui.checkbox(&mut w.only_free, tr!("Только бесплатные", "Free only"));
                                ui.checkbox(&mut w.hide_owned, tr!("Скрыть купленные", "Hide owned"));
                            });
                            egui::ComboBox::from_id_salt("store_sort")
                                .selected_text(w.sort.label())
                                .show_ui($ui, |ui| {
                                    for s in Sort::ALL {
                                        ui.selectable_value(&mut w.sort, s, s.label());
                                    }
                                });
                            $ui.weak(tr!("Порядок:", "Order:"));
                        }
                    };
                }
                ui.horizontal(|ui| {
                    match w.tab {
                        // Store search is on "Home" (as in Steam); results replace the shelves.
                        Tab::Home | Tab::Search => {
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut w.query)
                                    .hint_text(tr!("Название игры, Enter - искать", "Game name, Enter - search"))
                                    .desired_width((ui.available_width() * 0.5).clamp(140.0, 320.0)),
                            );
                            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (enter || ui.button(tr!("Найти", "Find")).clicked()) && !w.query.trim().is_empty() {
                                cmds.push(Cmd::Store(StoreCmd::Search(w.query.trim().to_string())));
                                w.tab = Tab::Search;
                            }
                            if st.searching {
                                crate::spin::spinner(ui);
                            }
                            if w.tab == Tab::Search && ui.button(tr!("К витрине", "Back to the storefront")).clicked() {
                                w.tab = Tab::Home;
                            }
                        }
                        Tab::Wishlist | Tab::FriendWishlist => {
                            ui.add(egui::TextEdit::singleline(&mut w.wish_query).hint_text(tr!("Поиск в списке", "Search the list")).desired_width(260.0));
                        }
                        Tab::Deals | Tab::Events => {
                            ui.selectable_value(&mut w.tab, Tab::Deals, tr!("Популярное со скидкой", "Popular discounted"))
                                .on_hover_text(tr!("Самые продаваемые игры в вашем регионе, на которые сейчас есть скидка", "The best-selling games in your region that are on sale now"));
                            ui.selectable_value(&mut w.tab, Tab::Events, tr!("Акции Steam", "Steam events"))
                                .on_hover_text(tr!("Скидка дня, скидки недели и выходных, бесплатные выходные, новинки", "Daily deal, weekly and weekend deals, free weekends, new releases"));
                        }
                        Tab::Charts => {
                            for (i, (name, hint)) in store::CHARTS().iter().enumerate() {
                                ui.selectable_value(&mut w.chart, i as u8, *name).on_hover_text(*hint);
                            }
                        }
                        Tab::Discovery => {
                            let current: Vec<u32> = st.discovery.as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
                            if ui
                                .button(tr!("Другие игры", "Other games"))
                                .on_hover_text(tr!("Отметить эти игры просмотренными (как «Далее» в Steam) и подобрать новые", "Mark these games as viewed (like \"Next\" in Steam) and pick new ones"))
                                .clicked()
                            {
                                cmds.push(Cmd::Store(StoreCmd::Discovery { rebuild: false, skip: current }));
                            }
                        }
                    }
                    if !narrow {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            right_group!(ui);
                        });
                    }
                });
                if narrow && (!row1 || w.tab != Tab::Home) {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            right_group!(ui);
                        });
                    });
                }
            }
            if let Some(e) = &st.error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(&appid) = w.pages.last() {
                game_page(ui, appid, &st, &owned, &installed, &wish, &mut cmds, &mut open_page, &mut play, &mut install);
                return;
            }
            if w.tab == Tab::Home {
                let mut goto = None;
                let mut missing = Vec::new();
                home_view(ui, w, &st, &owned, &wish, &mut cmds, &mut open_page, &mut goto, &mut missing);
                if !missing.is_empty() {
                    w.asked.extend(missing.iter().copied());
                    cmds.push(Cmd::Store(StoreCmd::Details(missing)));
                }
                if let Some((tab, chart)) = goto {
                    w.tab = tab;
                    if let Some(c) = chart {
                        w.chart = c;
                    }
                }
                return;
            }
            let hint = match w.tab {
                Tab::Deals => Some(tr!("Самые продаваемые игры в вашем регионе, на которые сейчас есть скидка.", "The best-selling games in your region that are on sale now.")),
                Tab::Charts => Some(store::CHARTS()[w.chart as usize].1),
                Tab::Discovery => Some(tr!("Игры, которые Steam подобрал для вас по вашим играм и списку желаемого.", "Games Steam picked for you based on your games and wishlist.")),
                Tab::Events => Some(
                    tr!("Что Steam сейчас продвигает на главной странице: скидка дня, скидки середины недели и \
                     выходных, бесплатные выходные, новинки и обновления. Желтым - вид акции и до какого числа.", "What Steam promotes on its home page now: daily deal, midweek and weekend deals, free weekends, new releases and updates. In yellow - the kind of event and its end date."),
                ),
                _ => None,
            };
            if let Some(h) = hint {
                ui.weak(h);
                ui.add_space(4.0);
            }
            // Labels in the "Steam events" list: what event and until what date.
            let chart_badges: Option<std::collections::HashMap<u32, String>> = match w.tab {
                Tab::Charts => st.charts.get(&w.chart).and_then(|r| r.as_ref().ok()).map(|v| v.iter().cloned().collect()),
                _ => None,
            };
            let badges: std::collections::HashMap<u32, String> = match (&w.tab, &st.events) {
                _ if chart_badges.is_some() => chart_badges.unwrap(),
                (Tab::Events, Some(Ok(v))) => v
                    .iter()
                    .map(|(id, kind, end)| {
                        use chrono::TimeZone;
                        let until = chrono::Local
                            .timestamp_opt(*end as i64, 0)
                            .single()
                            .map(|t| trf!(" · до {}", " · until {}", t.format("%d.%m")))
                            .unwrap_or_default();
                        (*id, format!("{kind}{until}"))
                    })
                    .collect(),
                _ => Default::default(),
            };
            let ids: Option<Result<Vec<u32>, String>> = match w.tab {
                Tab::Home => None,
                Tab::Search => st.search.as_ref().map(|(_, r)| r.clone()),
                Tab::Deals => {
                    if !w.deals_asked && st.deals.is_none() {
                        w.deals_asked = true;
                        cmds.push(Cmd::Store(StoreCmd::Deals));
                    }
                    st.deals.clone()
                }
                Tab::Events => {
                    if !w.events_asked && st.events.is_none() {
                        w.events_asked = true;
                        cmds.push(Cmd::Store(StoreCmd::Events));
                    }
                    st.events.as_ref().map(|r| r.clone().map(|v| v.iter().map(|e| e.0).collect()))
                }
                Tab::Wishlist => st.wishlist.clone(),
                Tab::FriendWishlist => st.friend_wishlist.as_ref().and_then(|f| f.2.clone()),
                Tab::Charts => {
                    if w.charts_asked.insert(w.chart) && !st.charts.contains_key(&w.chart) {
                        cmds.push(Cmd::Store(StoreCmd::Charts(w.chart)));
                    }
                    st.charts.get(&w.chart).map(|r| r.clone().map(|v| v.iter().map(|e| e.0).collect()))
                }
                Tab::Discovery => {
                    if !w.discovery_asked && st.discovery.is_none() {
                        w.discovery_asked = true;
                        cmds.push(Cmd::Store(StoreCmd::Discovery { rebuild: false, skip: Vec::new() }));
                    }
                    st.discovery.clone()
                }
            };
            match ids {
                None if w.tab == Tab::Search => {
                    ui.weak(tr!("Введите название игры и нажмите Enter.", "Enter a game name and press Enter."));
                }
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                }
                Some(Ok(ids)) if ids.is_empty() => {
                    ui.weak(if st.searching { tr!("Ищу...", "Searching...") } else { tr!("Ничего не найдено.", "Nothing found.") });
                }
                Some(Ok(all)) => {
                    let mut missing = Vec::new();
                    // Filters and sorting need details of all games in the list:
                    // missing ones are loaded, and until they arrive the games are not shown.
                    let q = w.wish_query.trim().to_lowercase();
                    let wish_filter = matches!(w.tab, Tab::Wishlist | Tab::FriendWishlist) && !q.is_empty();
                    let filtering = w.sort != Sort::Default || w.only_discount || w.only_free || w.hide_owned || wish_filter;
                    let ids: Vec<u32> = if filtering {
                        let unknown: Vec<u32> =
                            all.iter().copied().filter(|id| !st.apps.contains_key(id) && !w.asked.contains(id)).collect();
                        missing.extend(unknown);
                        let mut v: Vec<&StoreAppInfo> = all
                            .iter()
                            .filter_map(|id| st.apps.get(id))
                            .filter(|a| !w.only_discount || a.discount_pct > 0)
                            .filter(|a| !w.only_free || a.is_free)
                            .filter(|a| !w.hide_owned || !owned.contains(&a.app_id))
                            .filter(|a| !wish_filter || a.name.to_lowercase().contains(&q))
                            .collect();
                        match w.sort {
                            Sort::Default => {}
                            Sort::Name => v.sort_by_key(|a| a.name.to_lowercase()),
                            Sort::PriceAsc => v.sort_by_key(|a| price_key(a)),
                            Sort::PriceDesc => v.sort_by_key(|a| std::cmp::Reverse(if price_key(a) == i64::MAX { -1 } else { price_key(a) })),
                            Sort::Discount => v.sort_by_key(|a| std::cmp::Reverse(a.discount_pct)),
                            Sort::Reviews => v.sort_by_key(|a| std::cmp::Reverse(review_pct(a))),
                            Sort::Newest => v.sort_by(|a, b| b.release_date.cmp(&a.release_date)),
                        }
                        v.into_iter().map(|a| a.app_id).collect()
                    } else {
                        all.clone()
                    };
                    let loaded = all.iter().filter(|id| st.apps.contains_key(id)).count();
                    ui.horizontal(|ui| {
                        if w.tab == Tab::Wishlist {
                            ui.weak(trf!("Игр в списке желаемого: {}", "Games in the wishlist: {}", all.len()));
                        }
                        if filtering {
                            ui.weak(trf!("Показано: {}", "Shown: {}", ids.len()));
                            if loaded < all.len() {
                                crate::spin::spinner(ui);
                                ui.weak(trf!("загружаю данные об играх: {loaded} из {}", "loading game details: {loaded} of {}", all.len()));
                            }
                        }
                    });
                    // Own wishlist without filters: the order changes by right click (like dragging in Steam).
                    let reorder = w.tab == Tab::Wishlist && !filtering;
                    let mut moved: Option<(usize, usize)> = None;
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, ids.len(), |ui, range| {
                        for &id in &ids[range] {
                            match st.apps.get(&id) {
                                Some(a) => {
                                    let row = list_row(ui, a, owned.contains(&id), wish.contains(&id), badges.get(&id).map(String::as_str));
                                    if row.clicked() {
                                        open_page = Some(id);
                                    }
                                    if reorder && crate::exp::on() {
                                        {
                                            let pos = all.iter().position(|x| *x == id).unwrap_or(0);
                                            row.context_menu(|ui| {
                                                ui.weak(trf!("Место в списке: {}", "Place in the list: {}", pos + 1));
                                                for (label, to) in [
                                                    (tr!("В начало", "To the top"), 0usize),
                                                    (tr!("Выше", "Up"), pos.saturating_sub(1)),
                                                    (tr!("Ниже", "Down"), (pos + 1).min(all.len() - 1)),
                                                    (tr!("В конец", "To the bottom"), all.len() - 1),
                                                ] {
                                                    if ui.add_enabled(to != pos, egui::Button::new(label)).clicked() {
                                                        moved = Some((pos, to));
                                                        ui.close_menu();
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                                None => {
                                    if !w.asked.contains(&id) {
                                        missing.push(id);
                                    }
                                    ui.allocate_ui(egui::vec2(ui.available_width(), ROW_H), |ui| {
                                        ui.horizontal(|ui| {
                                            crate::spin::spinner(ui);
                                            ui.weak(trf!("игра {id}", "game {id}"));
                                        });
                                    });
                                }
                            }
                        }
                    });
                    if !missing.is_empty() {
                        w.asked.extend(missing.iter().copied());
                        cmds.push(Cmd::Store(StoreCmd::Details(missing)));
                    }
                    if let Some((from, to)) = moved {
                        let mut order = all.clone();
                        let id = order.remove(from);
                        order.insert(to, id);
                        let upto = from.max(to);
                        let updates: Vec<(u32, u32)> = order.iter().take(upto + 1).enumerate().map(|(i, a)| (*a, i as u32 + 1)).collect();
                        cmds.push(Cmd::WishlistReorder(updates));
                        st.wishlist = Some(Ok(order));
                    }
                }
            }
        });
        drop(st);
        if close_friend_wish {
            shared.lock().unwrap().friend_wishlist = None;
            if w.tab == Tab::FriendWishlist {
                w.tab = Tab::Wishlist;
            }
        }

        if let Some(id) = open_page {
            w.pages.push(id);
            cmds.push(Cmd::Store(StoreCmd::Page(id)));
        }
        for c in cmds {
            let _ = self.tx.send(c);
        }
        if let Some(id) = play {
            let ctx = ui.ctx().clone();
            self.start(id, &ctx);
        }
        if let Some((id, name)) = install {
            self.enqueue(id, name, net);
        }
        self.drain_ui_req();
    }
}

/// Game page actions that need the whole LauncherApp (award, review, cart windows).
pub enum UiReq {
    Award(u32, String, String),
    Review(u32, String),
    Cart(u32),
}

pub static UI_REQ: std::sync::Mutex<Vec<UiReq>> = std::sync::Mutex::new(Vec::new());

/// Curators who reviewed the game (like the "Curators" block on the Steam game page).
fn curators_block(ui: &mut egui::Ui, appid: u32, st: &store::StoreState, cmds: &mut Vec<Cmd>) {
    thread_local! {
        static ASKED: std::cell::RefCell<HashSet<u32>> = Default::default();
    }
    if !st.curators.contains_key(&appid) && ASKED.with(|a| a.borrow_mut().insert(appid)) {
        cmds.push(Cmd::Curators(appid));
    }
    let Some(Ok(list)) = st.curators.get(&appid) else { return };
    if list.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new(tr!("КУРАТОРЫ", "CURATORS")).size(13.0).color(crate::theme::TEXT));
    // Three first, the rest by a button (like "All curators" in Steam).
    let key = egui::Id::new(("curators_all", appid));
    let all: bool = ui.data(|d| d.get_temp(key)).unwrap_or(false);
    for c in list.iter().take(if all { 10 } else { 3 }) {
        egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                if !c.avatar.is_empty() {
                    ui.add(egui::Image::new(c.avatar.as_str()).fit_to_exact_size(egui::vec2(32.0, 32.0)).rounding(2.0));
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui.link(egui::RichText::new(crate::richtext::fix(ui, &c.name)).strong()).clicked() && !c.link.is_empty() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(c.link.clone()));
                        }
                        let (t, col) = match c.state {
                            0 => (tr!("рекомендует", "recommends"), egui::Color32::from_rgb(102, 192, 244)),
                            1 => (tr!("не рекомендует", "does not recommend"), egui::Color32::from_rgb(200, 90, 70)),
                            _ => (tr!("информационный обзор", "informational review"), egui::Color32::from_rgb(190, 160, 90)),
                        };
                        ui.label(egui::RichText::new(t).color(col).small());
                        ui.weak(egui::RichText::new(trf!("{} подписчиков", "{} followers", crate::store::group_digits(c.followers))).small());
                    });
                    if !c.blurb.is_empty() {
                        ui.label(crate::richtext::fix(ui, &c.blurb));
                    }
                    if !c.review_url.is_empty() && ui.small_button(tr!("Полный обзор", "Full review")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(c.review_url.clone()));
                    }
                });
            });
        });
        ui.add_space(3.0);
    }
    if list.len() > 3 && ui.small_button(if all { tr!("Свернуть", "Collapse").to_string() } else { trf!("Все кураторы ({})", "All curators ({})", list.len()) }).clicked() {
        ui.data_mut(|d| d.insert_temp(key, !all));
    }
}

/// Store home: several shelves of cards, each with "All" - open the full list.
#[allow(clippy::too_many_arguments)]
fn home_view(
    ui: &mut egui::Ui,
    w: &mut StoreWin,
    st: &store::StoreState,
    owned: &HashSet<u32>,
    wish: &HashSet<u32>,
    cmds: &mut Vec<Cmd>,
    open_page: &mut Option<u32>,
    goto: &mut Option<(Tab, Option<u8>)>,
    missing: &mut Vec<u32>,
) {
    // Request the lists once.
    if !w.deals_asked && st.deals.is_none() {
        w.deals_asked = true;
        cmds.push(Cmd::Store(StoreCmd::Deals));
    }
    for c in [0u8, 2] {
        if w.charts_asked.insert(c) && !st.charts.contains_key(&c) {
            cmds.push(Cmd::Store(StoreCmd::Charts(c)));
        }
    }
    if !w.discovery_asked && st.discovery.is_none() {
        w.discovery_asked = true;
        cmds.push(Cmd::Store(StoreCmd::Discovery { rebuild: false, skip: Vec::new() }));
    }
    let chart = |c: u8| st.charts.get(&c).map(|r| r.clone().map(|v| v.iter().map(|e| e.0).collect::<Vec<u32>>()));
    // Discounts in own wishlist - only from already loaded details.
    let wish_sales: Option<Result<Vec<u32>, String>> = st.wishlist.as_ref().map(|r| {
        r.clone().map(|v| v.into_iter().filter(|id| st.apps.get(id).is_some_and(|a| a.discount_pct > 0)).collect())
    });
    let shelves: Vec<(&str, Option<Result<Vec<u32>, String>>, Tab, Option<u8>)> = vec![
        (tr!("ОСОБЫЕ ПРЕДЛОЖЕНИЯ", "SPECIAL OFFERS"), st.deals.clone(), Tab::Deals, None),
        (tr!("ЛИДЕРЫ ПРОДАЖ", "TOP SELLERS"), chart(0), Tab::Charts, Some(0)),
        (tr!("ПОПУЛЯРНЫЕ НОВИНКИ", "POPULAR NEW RELEASES"), chart(2), Tab::Charts, Some(2)),
        (tr!("РЕКОМЕНДУЕМ ВАМ", "RECOMMENDED FOR YOU"), st.discovery.clone(), Tab::Discovery, None),
        (tr!("СКИДКИ В ЖЕЛАЕМОМ", "WISHLIST DISCOUNTS"), wish_sales, Tab::Wishlist, None),
    ];
    const CARD_W: f32 = 230.0;
    const GAP: f32 = 10.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (title, ids, tab, chart_no) in shelves {
            // An empty wishlist discount shelf is not shown.
            if tab == Tab::Wishlist && ids.as_ref().map_or(true, |r| r.as_ref().map_or(true, |v| v.is_empty())) {
                continue;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(14.0).color(egui::Color32::WHITE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link(tr!("Все", "All")).clicked() {
                        *goto = Some((tab, chart_no));
                    }
                });
            });
            ui.add_space(4.0);
            let width = ui.available_width();
            let n = (((width + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
            let card_w = ((width + GAP) / n as f32 - GAP).max(120.0);
            match ids {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.weak(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                }
                Some(Ok(v)) if v.is_empty() => {
                    ui.weak(tr!("Пусто.", "Empty."));
                }
                Some(Ok(v)) => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for &id in v.iter().take(n) {
                            if !st.apps.contains_key(&id) && !w.asked.contains(&id) {
                                missing.push(id);
                            }
                            if home_card(ui, id, st.apps.get(&id), card_w, owned.contains(&id), wish.contains(&id)) {
                                *open_page = Some(id);
                            }
                        }
                    });
                }
            }
        }
        ui.add_space(8.0);
    });
}

/// Shelf card: game header, name, price. true - clicked.
fn home_card(ui: &mut egui::Ui, appid: u32, a: Option<&StoreAppInfo>, w: f32, owned: bool, wished: bool) -> bool {
    let img_h = w * 215.0 / 460.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, img_h + 46.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return false;
    }
    let bg = if resp.hovered() { egui::Color32::from_rgb(52, 66, 86) } else { crate::theme::CARD };
    ui.painter().rect_filled(rect, 3.0, bg);
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(w, img_h));
    egui::Image::new(capsule(ui, appid, a)).rounding(egui::Rounding { nw: 3.0, ne: 3.0, sw: 0.0, se: 0.0 }).paint_at(ui, img);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(egui::pos2(rect.left() + 6.0, img.bottom() + 3.0), rect.max - egui::vec2(6.0, 2.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    match a {
        Some(a) => {
            child.add(egui::Label::new(egui::RichText::new(&a.name).size(13.0).color(egui::Color32::WHITE)).truncate());
            child.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                if owned {
                    ui.label(egui::RichText::new(tr!("в библиотеке", "in library")).small().color(GREEN));
                } else if a.coming_soon {
                    ui.weak(tr!("Скоро", "Coming soon"));
                } else if a.is_free {
                    ui.label(tr!("Бесплатно", "Free"));
                } else if let Some(p) = &a.price {
                    if a.discount_pct > 0 {
                        ui.label(
                            egui::RichText::new(format!(" -{}% ", a.discount_pct))
                                .strong()
                                .color(egui::Color32::from_rgb(190, 238, 17))
                                .background_color(egui::Color32::from_rgb(76, 107, 34)),
                        );
                        if let Some(o) = &a.original_price {
                            ui.label(egui::RichText::new(o).strikethrough().color(GRAY).small());
                        }
                    }
                    ui.label(egui::RichText::new(p).strong());
                } else if a.region_locked {
                    ui.weak(tr!("недоступна", "unavailable"));
                }
                if wished && !owned {
                    ui.label(egui::RichText::new(tr!("в желаемом", "wishlisted")).small().color(egui::Color32::from_rgb(110, 160, 230)));
                }
            });
        }
        None => {
            crate::spin::spinner(&mut child);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Small game image (120x45); if the game does not have one (new games use other URLs) -
/// the header from store data.
fn capsule(_ui: &egui::Ui, appid: u32, a: Option<&StoreAppInfo>) -> String {
    // Every game has a "header"; the small capsule is often missing for new games.
    a.and_then(|a| a.assets.header.clone())
        .unwrap_or_else(|| format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg"))
}

/// Short review summary ("Очень положительные") and its color, as in Steam; the full text in a tooltip.
fn review_short(s: &str) -> (String, egui::Color32) {
    let short = s.split(" (").next().unwrap_or(s).trim().to_string();
    let l = short.to_lowercase();
    let color = if l.contains("отрицат") || l.contains("negative") {
        egui::Color32::from_rgb(200, 90, 70)
    } else if l.contains("смешан") || l.contains("mixed") {
        egui::Color32::from_rgb(190, 160, 90)
    } else {
        egui::Color32::from_rgb(102, 192, 244)
    };
    (short, color)
}

/// Price block on the right: the discount as a big badge, the old price struck through, the new one large.
fn price_block(ui: &mut egui::Ui, a: &StoreAppInfo) {
    if a.coming_soon {
        ui.weak(tr!("Скоро", "Coming soon"));
    } else if a.is_free {
        ui.strong(tr!("Бесплатно", "Free"));
    } else if let Some(p) = &a.price {
        ui.label(egui::RichText::new(p).size(15.0).strong());
        if a.discount_pct > 0 {
            if let Some(o) = &a.original_price {
                ui.label(egui::RichText::new(o).strikethrough().color(GRAY).small());
            }
            ui.label(
                egui::RichText::new(format!(" -{}% ", a.discount_pct))
                    .size(15.0)
                    .strong()
                    .color(egui::Color32::from_rgb(190, 238, 17))
                    .background_color(egui::Color32::from_rgb(76, 107, 34)),
            );
        }
    } else if a.region_locked {
        ui.weak(tr!("недоступна", "unavailable"));
    }
}

/// List row: image, name, developer and date, reviews, price. true - clicked.
fn list_row(ui: &mut egui::Ui, a: &StoreAppInfo, owned: bool, wished: bool, badge: Option<&str>) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROW_H - 6.0), egui::Sense::click());
    let bg = if resp.hovered() { egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18) } else { ui.visuals().faint_bg_color };
    ui.painter().rect_filled(rect, 6.0, bg);
    let img = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(150.0, 70.0));
    egui::Image::new(capsule(ui, a.app_id, Some(a))).rounding(4.0).paint_at(ui, img);
    let text_rect = egui::Rect::from_min_max(egui::pos2(img.right() + 12.0, rect.top() + 6.0), egui::pos2(rect.right() - 150.0, rect.bottom() - 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    child.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new(&a.name).size(15.0).strong()).truncate());
    });
    let mut sub = Vec::new();
    if let Some(d) = a.developers.first() {
        sub.push(d.clone());
    }
    if let Some(r) = &a.release_date {
        sub.push(ru_date(r));
    }
    if !sub.is_empty() {
        child.add(egui::Label::new(egui::RichText::new(sub.join(" · ")).weak().small()).truncate());
    }
    child.horizontal(|ui| {
        if let Some(r) = &a.review_summary {
            let (short, color) = review_short(r);
            ui.label(egui::RichText::new(short).color(color).small()).on_hover_text(review_ru(r));
        }
        if let Some(b) = badge {
            ui.label(egui::RichText::new(b).small().color(egui::Color32::from_rgb(240, 200, 80)));
        }
        if owned {
            ui.label(egui::RichText::new(tr!("в библиотеке", "in library")).small().color(GREEN));
        } else if wished {
            ui.label(egui::RichText::new(tr!("в желаемом", "wishlisted")).small().color(egui::Color32::from_rgb(110, 160, 230)));
        }
    });
    let price_rect = egui::Rect::from_min_max(egui::pos2(rect.right() - 140.0, rect.top() + 8.0), rect.max - egui::vec2(10.0, 4.0));
    let mut pr = ui.new_child(egui::UiBuilder::new().max_rect(price_rect).layout(egui::Layout::top_down(egui::Align::Max)));
    price_block(&mut pr, a);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[allow(clippy::too_many_arguments)]
fn game_page(
    ui: &mut egui::Ui,
    appid: u32,
    st: &store::StoreState,
    owned: &HashSet<u32>,
    installed: &HashSet<u32>,
    wish: &HashSet<u32>,
    cmds: &mut Vec<Cmd>,
    open_page: &mut Option<u32>,
    play: &mut Option<u32>,
    install: &mut Option<(u32, String)>,
) {
    let Some(a) = st.apps.get(&appid) else {
        crate::spin::spinner(ui);
        return;
    };
    let ex = st.extras.get(&appid);
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    let off = TOUR_SCROLL.swap(0, std::sync::atomic::Ordering::Relaxed);
    if off > 0 {
        area = area.vertical_scroll_offset(off as f32);
    }
    area.show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(h) = &a.assets.header {
                ui.add(egui::Image::new(h).fit_to_exact_size(egui::vec2(368.0, 172.0)));
            }
            ui.vertical(|ui| {
                ui.heading(&a.name);
                ui.horizontal(|ui| price_ui(ui, a));
                if a.discount_pct > 0 {
                    if let Some(end) = a.discount_end {
                        ui.weak(trf!("Скидка до {}", "Discount until {}", fmt_date(end)));
                    }
                }
                if let Some(r) = &a.review_summary {
                    ui.label(trf!("Отзывы: {}", "Reviews: {}", review_ru(r)));
                }
                if let Some(d) = &a.release_date {
                    ui.weak(trf!("Дата выхода: {}", "Release date: {}", ru_date(d)));
                }
                if let Some(p) = ex.and_then(|e| e.players) {
                    ui.weak(trf!("Сейчас играют: {p}", "Playing now: {p}"));
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if owned.contains(&appid) {
                        if installed.contains(&appid) {
                            if ui.button(tr!("Играть", "Play")).clicked() {
                                *play = Some(appid);
                            }
                        } else if ui.button(tr!("Установить", "Install")).clicked() {
                            *install = Some((appid, a.name.clone()));
                        }
                        ui.label(egui::RichText::new(tr!("в библиотеке", "in library")).color(GREEN));
                        if crate::exp::button(ui, tr!("Написать обзор", "Write a review")).on_hover_text(tr!("Свой обзор игры (или изменить уже написанный)", "Your own review of the game (or edit the one you wrote)")).clicked() {
                            UI_REQ.lock().unwrap().push(UiReq::Review(appid, a.name.clone()));
                        }
                        if ui.button(tr!("Страница в браузере", "Page in browser")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(store_url(appid)));
                        }
                    } else {
                        if ui
                            .button(tr!("Купить в Steam", "Buy on Steam"))
                            .on_hover_text(tr!("Откроется страница игры в браузере: покупка проходит на сайте Steam.", "The game page opens in the browser: the purchase happens on the Steam website."))
                            .clicked()
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(store_url(appid)));
                        }
                        if !a.is_free && !a.coming_soon && a.price.is_some() && ui.button(tr!("В корзину", "Add to cart")).on_hover_text(tr!("Положить в корзину Steam (оплата - на сайте Steam)", "Add to the Steam cart (payment on the Steam website)")).clicked() {
                            UI_REQ.lock().unwrap().push(UiReq::Cart(appid));
                        }
                        if wish.contains(&appid) {
                            if ui.button(tr!("Убрать из желаемого", "Remove from wishlist")).clicked() {
                                cmds.push(Cmd::Store(StoreCmd::WishlistSet(appid, false)));
                            }
                        } else if ui.button(tr!("В список желаемого", "Add to wishlist")).clicked() {
                            cmds.push(Cmd::Store(StoreCmd::WishlistSet(appid, true)));
                        }
                        // "Not interested", as in Steam: the game disappears from recommendations.
                        static IGNORED_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                        if st.ignored.is_none() && !IGNORED_ASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            cmds.push(Cmd::Store(StoreCmd::IgnoredList));
                        }
                        let ignored = st.ignored.as_ref().is_some_and(|s| s.contains(&appid));
                        let (text, hint) = if ignored {
                            (tr!("Интересует снова", "Interested again"), tr!("Вернуть игру в рекомендации магазина", "Bring the game back into store recommendations"))
                        } else {
                            (tr!("Не интересует", "Not interested"), tr!("Скрыть игру из рекомендаций магазина (как в Steam)", "Hide the game from store recommendations (as in Steam)"))
                        };
                        if ui.button(text).on_hover_text(hint).clicked() {
                            cmds.push(Cmd::Store(StoreCmd::Ignore(appid, !ignored)));
                        }
                        if let Some(t) = ex.and_then(|e| e.trailer.as_deref()) {
                            if ui.button(tr!("Трейлер", "Trailer")).on_hover_text(tr!("Открыть трейлер в браузере", "Open the trailer in the browser")).clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(t));
                            }
                        }
                    }
                });
            });
        });

        ui.add_space(6.0);
        if !a.developers.is_empty() {
            ui.label(trf!("Разработчик: {}", "Developer: {}", a.developers.join(", ")));
        }
        if !a.publishers.is_empty() {
            ui.label(trf!("Издатель: {}", "Publisher: {}", a.publishers.join(", ")));
        }
        // Tags: first the 12 most prominent, "all tags" expands the rest; a click - the tag in the Steam store.
        let all_tags: Vec<String> = a.tags.iter().filter_map(|t| t.name.clone()).collect();
        if !all_tags.is_empty() {
            let key = egui::Id::new(("all_tags", appid));
            let open: bool = ui.data(|d| d.get_temp(key)).unwrap_or(false);
            ui.horizontal_wrapped(|ui| {
                for t in all_tags.iter().take(if open { usize::MAX } else { 12 }) {
                    let r = ui
                        .add(egui::Label::new(egui::RichText::new(t).small().background_color(egui::Color32::from_gray(50))).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tr!("Игры с этой меткой в магазине Steam", "Games with this tag in the Steam store"));
                    if r.clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://store.steampowered.com/tags/ru/{}/", t.replace(' ', "%20"))));
                    }
                }
                if all_tags.len() > 12 && ui.small_button(if open { tr!("свернуть", "collapse") } else { tr!("все метки", "all tags") }).clicked() {
                    ui.data_mut(|d| d.insert_temp(key, !open));
                }
            });
        }

        if !a.screenshots.is_empty() {
            ui.add_space(8.0);
            egui::ScrollArea::horizontal().id_salt("shots").max_height(130.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for s in a.screenshots.iter().take(10) {
                        let r = ui
                            .add(
                                egui::Image::new(store::screenshot_thumb(s))
                                    .fit_to_exact_size(egui::vec2(213.0, 120.0))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(tr!("Открыть в полном размере в браузере", "Open full size in the browser"));
                        if r.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(s.split('?').next().unwrap_or(s)));
                        }
                    }
                });
            });
        }

        ui.add_space(8.0);
        let short = store::plain_text(&a.short_description);
        if !short.is_empty() {
            ui.label(&short);
        }
        egui::CollapsingHeader::new(tr!("Описание", "Description")).id_salt(("desc", appid)).show(ui, |ui| {
            for b in store::description_blocks(&a.full_description) {
                match b {
                    store::Block::Heading(t) => {
                        ui.add_space(6.0);
                        ui.strong(t);
                    }
                    store::Block::Para(t) => {
                        ui.label(t);
                    }
                    store::Block::Bullet(t) => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("  •");
                            ui.label(t);
                        });
                    }
                }
            }
        });

        egui::CollapsingHeader::new(tr!("Новости", "News")).id_salt(("news", appid)).show(ui, |ui| match ex.and_then(|e| e.news.as_ref()) {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.weak(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
            }
            Some(Ok(n)) if n.is_empty() => {
                ui.weak(tr!("Новостей нет.", "No news."));
            }
            Some(Ok(items)) => {
                for n in items {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(fmt_date(n.date));
                        ui.hyperlink_to(&n.title, &n.url);
                    });
                    let text = store::unglue(&store::plain_text(&n.contents));
                    if !text.is_empty() {
                        ui.weak(text.chars().take(300).collect::<String>());
                    }
                    ui.add_space(4.0);
                }
            }
        });

        if let Some((min, rec)) = ex.and_then(|e| e.requirements.as_ref()) {
            egui::CollapsingHeader::new(tr!("Системные требования", "System requirements")).id_salt(("req", appid)).show(ui, |ui| {
                if !min.is_empty() {
                    ui.label(min);
                }
                if !rec.is_empty() {
                    ui.add_space(4.0);
                    ui.label(rec);
                }
            });
        }

        egui::CollapsingHeader::new(tr!("Отзывы", "Reviews")).id_salt(("reviews", appid)).show(ui, |ui| {
            if let Some(r) = &a.review_summary {
                let (short, color) = review_short(r);
                ui.horizontal(|ui| {
                    ui.label(tr!("Итог:", "Summary:"));
                    ui.label(egui::RichText::new(short).color(color).strong());
                    ui.weak(review_ru(r).split_once('(').map(|x| format!("({}", x.1)).unwrap_or_default());
                });
            } else {
                ui.weak(tr!("Отзывов пока мало.", "Not many reviews yet."));
            }
            let rv = ex.and_then(|e| e.reviews.as_ref());
            let (sort, lang) = rv.map(|r| (r.sort, r.lang)).unwrap_or((0, 0));
            if rv.is_none() {
                // The section was opened - the first page (most helpful of all time, in Russian), as on the site.
                cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort: 0, lang: 0, page: 1 }));
            }
            // Type (all / positive / negative) - a filter over the already loaded ones.
            let type_key = egui::Id::new(("rev_type", appid));
            let mut kind: u8 = ui.data(|d| d.get_temp(type_key)).unwrap_or(0);
            ui.horizontal_wrapped(|ui| {
                let (mut r2, mut l2) = (sort, lang);
                egui::ComboBox::from_id_salt(("rev_sort", appid))
                    .selected_text(store::REVIEW_SORTS()[r2 as usize].1)
                    .show_ui(ui, |ui| {
                        for (i, (_, label)) in store::REVIEW_SORTS().iter().enumerate() {
                            ui.selectable_value(&mut r2, i as u8, *label);
                        }
                    });
                egui::ComboBox::from_id_salt(("rev_lang", appid))
                    .selected_text(store::REVIEW_LANGS()[l2 as usize].1)
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for (i, (_, label)) in store::REVIEW_LANGS().iter().enumerate() {
                            ui.selectable_value(&mut l2, i as u8, *label);
                        }
                    });
                if (r2, l2) != (sort, lang) {
                    cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort: r2, lang: l2, page: 1 }));
                }
                ui.separator();
                ui.selectable_value(&mut kind, 0, tr!("Все", "All"));
                ui.selectable_value(&mut kind, 1, tr!("Положительные", "Positive"));
                ui.selectable_value(&mut kind, 2, tr!("Отрицательные", "Negative"));
                ui.separator();
                ui.hyperlink_to(tr!("В браузере", "In browser"), format!("https://steamcommunity.com/app/{appid}/reviews/"));
            });
            ui.data_mut(|d| d.insert_temp(type_key, kind));
            let Some(rv) = rv else {
                crate::spin::spinner(ui);
                return;
            };
            if let Some(e) = &rv.error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), trf!("Не удалось загрузить отзывы: {e}", "Could not load reviews: {e}"));
            }
            let shown: Vec<&store::Review> =
                rv.list.iter().filter(|r| kind == 0 || (kind == 1) == r.positive).collect();
            // There are few negative ones among the first pages - load more ourselves (up to 10 pages).
            if kind != 0 && shown.len() < 6 && rv.more && !rv.loading && rv.pages < 10 {
                cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort, lang, page: rv.pages + 1 }));
            }
            if shown.is_empty() && !rv.loading && rv.error.is_none() {
                ui.weak(tr!("Отзывов с такими условиями нет.", "No reviews with these conditions."));
            }
            for r in shown {
                review_card(ui, r, st, cmds);
            }
            if rv.loading {
                crate::spin::spinner(ui);
            } else if rv.more && ui.button(tr!("Показать еще", "Show more")).clicked() {
                cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort, lang, page: rv.pages + 1 }));
            } else if sort == 0 && rv.list.len() >= 10 {
                ui.weak(tr!("За все время Steam показывает только первые 10 отзывов. Больше - выберите период (например, «за год») или «Новые».", "For all time Steam shows only the first 10 reviews. For more, choose a period (for example \"this year\") or \"Newest\"."));
            }
        });

        curators_block(ui, appid, st, cmds);
        if let Some(sim) = ex.and_then(|e| e.similar.as_ref()) {
            if !sim.is_empty() {
                ui.add_space(8.0);
                ui.strong(tr!("Похожие игры", "Similar games"));
                egui::ScrollArea::horizontal().id_salt("similar").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for &id in sim {
                            let name = st.apps.get(&id).map(|a| a.name.clone()).unwrap_or_default();
                            let r = ui
                                .add(
                                    egui::Image::new(capsule(ui, id, st.apps.get(&id)))
                                        .fit_to_exact_size(egui::vec2(120.0, 45.0))
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(name);
                            if r.clicked() {
                                *open_page = Some(id);
                            }
                        }
                    });
                });
            }
        }
    });
}

/// Review card: who, whether they recommend, text (full - by a button), helpful/funny, awards
/// and own votes.
fn review_card(ui: &mut egui::Ui, r: &store::Review, st: &store::StoreState, cmds: &mut Vec<Cmd>) {
    let (color, verdict) = if r.positive {
        (egui::Color32::from_rgb(102, 192, 244), tr!("Рекомендует", "Recommended"))
    } else {
        (egui::Color32::from_rgb(220, 100, 80), tr!("Не рекомендует", "Not recommended"))
    };
    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .rounding(6.0)
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if !r.avatar.is_empty() {
                    ui.add(egui::Image::new(r.avatar.as_str()).fit_to_exact_size(egui::vec2(32.0, 32.0)).rounding(4.0));
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(verdict).color(color).strong());
                        ui.label(egui::RichText::new(crate::richtext::fix(ui, &r.author)).strong());
                    });
                    // Hours at review time exist only on the review page - load it in the background, once.
                    let at = match st.review_full.get(&r.url) {
                        Some(Ok((_, _, at))) => at.clone(),
                        Some(Err(_)) => String::new(),
                        None => {
                            if !r.url.is_empty() && REVIEW_ASKED.with(|a| a.borrow_mut().insert(r.url.clone())) {
                                cmds.push(Cmd::Store(StoreCmd::ReviewFull(r.url.clone())));
                            }
                            String::new()
                        }
                    };
                    let mut sub = vec![r.hours.clone(), at, r.date.clone()];
                    if !r.products.is_empty() {
                        sub.push(r.products.clone());
                    }
                    ui.weak(egui::RichText::new(sub.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")).small());
                });
            });
            ui.add_space(4.0);
            // The full text if already loaded; otherwise what came in the list.
            let full = st.review_full.get(&r.url);
            let text = match full {
                Some(Ok((t, _, _))) if !t.is_empty() => t.as_str(),
                _ => r.text.as_str(),
            };
            let cut = full.is_none() && (r.text.ends_with('…') || r.text.ends_with("...") || r.text.chars().count() > 900);
            let text = crate::richtext::fix(ui, text);
            ui.add(egui::Label::new(text).wrap());
            ui.add_space(4.0);
            // Own vote in this session is added to Steam's numbers (we do not reread Steam's list itself).
            let mine = st.review_mine.get(&r.url).copied();
            let helpful_n = r.helpful_n + u32::from(mine == Some(0));
            let funny_n = r.funny_n + u32::from(mine == Some(2));
            ui.horizontal_wrapped(|ui| {
                if helpful_n > 0 {
                    ui.label(egui::RichText::new(trf!("Полезно: {helpful_n}", "Helpful: {helpful_n}")).small().color(egui::Color32::from_rgb(150, 200, 120)));
                }
                if funny_n > 0 {
                    ui.label(egui::RichText::new(trf!("Забавно: {funny_n}", "Funny: {funny_n}")).small().color(egui::Color32::from_rgb(230, 190, 90)));
                }
                // Steam points awards (reactions) - icons with a count, as on the site.
                for (id, n) in r.awards.iter().take(8) {
                    let url = format!("https://store.fastly.steamstatic.com/public/images/loyalty/reactions/still/{id}.png");
                    ui.add(egui::Image::new(url).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                    ui.label(egui::RichText::new(n.to_string()).small());
                }
            });
            if !r.url.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.weak(egui::RichText::new(tr!("Этот обзор:", "This review is:")).small());
                    for (kind, label) in [(0u8, tr!("Полезный", "Helpful")), (1, tr!("Бесполезный", "Not helpful")), (2, tr!("Забавный", "Funny"))] {
                        // The chosen vote is highlighted; "helpful" and "not helpful" are mutually exclusive.
                        let on = mine == Some(kind);
                        let text = if on { egui::RichText::new(label).small().strong() } else { egui::RichText::new(label).small() };
                        if ui.add_enabled(crate::exp::on(), egui::SelectableLabel::new(on, text)).on_disabled_hover_text(crate::exp::HINT()).clicked() && !on {
                            cmds.push(Cmd::Store(StoreCmd::ReviewVote { url: r.url.clone(), kind }));
                        }
                    }
                    if let Some(Ok((_, rid, _))) = full {
                        if *rid > 0 && crate::exp::small_button(ui, tr!("Наградить...", "Award...")).on_hover_text(tr!("Наградить обзор очками Steam", "Award the review with Steam Points")).clicked() {
                            UI_REQ.lock().unwrap().push(UiReq::Award(1, rid.to_string(), trf!("обзор {}", "review {}", r.author)));
                        }
                    }
                    if cut && ui.small_button(tr!("Читать полностью", "Read more")).clicked() {
                        cmds.push(Cmd::Store(StoreCmd::ReviewFull(r.url.clone())));
                    }
                    if let Some(m) = st.review_vote.get(&r.url).filter(|m| !m.is_empty()) {
                        ui.weak(egui::RichText::new(m).small());
                    }
                });
            }
        });
    ui.add_space(6.0);
}

/// Section tour (tour.rs): scroll the game page by this many pixels.
pub static TOUR_SCROLL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl StoreWin {
    /// Section tour (tour.rs): switch the tab.
    pub(crate) fn tab(&self) -> Tab {
        self.tab
    }

    /// A game page is open (not a list).
    pub(crate) fn on_page(&self) -> bool {
        !self.pages.is_empty()
    }

    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.pages.clear();
    }
    /// Section tour (tour.rs): search text.
    pub(crate) fn tour_search(&mut self, q: &str) {
        self.query = q.to_string();
    }
}

/// "2011-04-19" -> "19.04.2011"; another date format - as is.
pub(crate) fn ru_date(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit())) {
        format!("{}.{}.{}", p[2], p[1], p[0])
    } else {
        d.to_string()
    }
}

/// Price region choice (country). By default - the Steam wallet region.
fn region_combo(ui: &mut egui::Ui, current: &str, saved: &mut Option<String>, cmds: &mut Vec<Cmd>, w: &mut StoreWin) {
    let label = store::COUNTRIES()
        .iter()
        .find(|(c, _)| *c == current)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| if current.is_empty() { "...".into() } else { current.to_string() });
    egui::ComboBox::from_id_salt("country")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for &(cc, name) in store::COUNTRIES().iter() {
                if ui.selectable_label(current == cc, name).clicked() && current != cc {
                    *saved = Some(cc.to_string());
                    cmds.push(Cmd::Store(StoreCmd::Country(cc.to_string())));
                    w.asked.clear();
                    w.deals_asked = false;
                    w.events_asked = false;
                    if let Some(&p) = w.pages.last() {
                        cmds.push(Cmd::Store(StoreCmd::Details(vec![p])));
                    }
                }
            }
        })
        .response
        .on_hover_text(tr!("Регион цен. По умолчанию - регион вашего кошелька Steam.", "Price region. By default - the region of your Steam wallet."));
}
