//! Окно «Магазин Steam» (отдельное окно Windows): поиск, скидки, список желаемого
//! и страница игры (цена, описание, скриншоты, новости, отзывы, похожие игры).
//! Покупка - на странице Steam в браузере: лаунчер платежи не проводит.

use crate::net::{Cmd, NetState};
use crate::store::{self, StoreCmd};
use crate::LauncherApp;
use aurelia::steam_client::StoreAppInfo;
use eframe::egui;
use std::collections::HashSet;

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Search,
    Deals,
    Events,
    Wishlist,
    /// Список желаемого друга (временная вкладка).
    FriendWishlist,
}

/// Сортировка списков магазина.
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
            Sort::Default => "как в Steam",
            Sort::Name => "по названию",
            Sort::PriceAsc => "сначала дешевые",
            Sort::PriceDesc => "сначала дорогие",
            Sort::Discount => "по размеру скидки",
            Sort::Reviews => "по отзывам",
            Sort::Newest => "сначала новые",
        }
    }
}

/// Процент положительных отзывов из «Очень положительные (83% positive, 4238 reviews)».
fn review_pct(a: &StoreAppInfo) -> i32 {
    a.review_summary
        .as_deref()
        .and_then(|s| s.split('(').nth(1))
        .and_then(|s| s.split('%').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(-1)
}

/// Цена для сортировки: бесплатные - 0, без цены - в конец.
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
    /// Открытые страницы игр (назад - снять последнюю).
    pages: Vec<u32>,
    /// Уже попросили подробности (чтобы не слать команду каждый кадр).
    asked: HashSet<u32>,
    deals_asked: bool,
    events_asked: bool,
}

const GREEN: egui::Color32 = egui::Color32::from_rgb(140, 200, 80);
const GRAY: egui::Color32 = egui::Color32::from_rgb(150, 150, 150);
const ROW_H: f32 = 52.0;

fn store_url(appid: u32) -> String {
    format!("https://store.steampowered.com/app/{appid}/")
}

/// «Крайне положительные (96% positive, 118575 reviews)» -> по-русски.
fn review_ru(s: &str) -> String {
    s.replace(" positive", " положительных").replace(" reviews", " отзывов")
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

/// Цена с учетом скидки, бесплатности и «скоро выйдет».
fn price_ui(ui: &mut egui::Ui, a: &StoreAppInfo) {
    if a.coming_soon {
        ui.weak(format!("Скоро{}", a.release_date.as_ref().map(|d| format!(": {d}")).unwrap_or_default()));
    } else if a.is_free {
        ui.label("Бесплатно");
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
        ui.weak("недоступна в регионе");
    }
}

impl StoreWin {
    pub fn new() -> Self {
        Self {
            sort: Sort::Default,
            only_discount: false,
            only_free: false,
            hide_owned: false,
            tab: Tab::Deals,
            query: String::new(),
            pages: Vec::new(),
            asked: HashSet::new(),
            deals_asked: false,
            events_asked: false,
        }
    }
}

impl LauncherApp {
    /// Открыть магазин: в главном окне или (detached) в отдельном.
    /// Магазин на вкладке «Желаемое: <друг>».
    pub(crate) fn open_friend_wishlist(&mut self, id: u64, name: String) {
        self.open_store(None, false);
        self.net.lock().unwrap().store.lock().unwrap().friend_wishlist = Some((id, name, None));
        let _ = self.tx.send(Cmd::Store(StoreCmd::FriendWishlist(id)));
        if let Some(w) = self.store_win.as_mut() {
            w.tab = Tab::FriendWishlist;
            w.pages.clear();
        }
    }

    /// Магазин на вкладке «Список желаемого» (щелчок по уведомлению о скидках).
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
        // Свой список желаемого - чтобы отмечать игры «в желаемом».
        let _ = self.tx.send(Cmd::Store(StoreCmd::Wishlist));
    }

    /// Магазин в отдельном окне (открыт с Ctrl).
    pub(crate) fn store_window(&mut self, ctx: &egui::Context, net: &NetState) {
        if self.store_win.is_none() || !self.store_detached {
            return;
        }
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_store"),
            egui::ViewportBuilder::default()
                .with_title("Магазин Steam - Vaporust")
                .with_inner_size([900.0, 640.0])
                .with_min_inner_size([600.0, 400.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher)),
            |ctx, _| {
                egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.store_contents(ui, net));
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.store_win = None;
            self.store_detached = false;
        }
    }

    /// Содержимое магазина: и в главном окне, и в отдельном.
    pub(crate) fn store_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let shared = net.store.clone();
        let mut st = shared.lock().unwrap();
        // Регион цен: сохраненный в настройках, иначе определенный Steam по IP.
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
        let Some(w) = self.store_win.as_mut() else { return };

        egui::TopBottomPanel::top("store_top").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if !w.pages.is_empty() && ui.button("< Назад").clicked() {
                    w.pages.pop();
                }
                // «Скидки» - одна вкладка с двумя списками (Deals и Events), переключатель ниже.
                let in_deals = matches!(w.tab, Tab::Deals | Tab::Events);
                for (tab, label) in [(Tab::Search, "Поиск"), (Tab::Deals, "Скидки"), (Tab::Wishlist, "Список желаемого")] {
                    let on = if tab == Tab::Deals { in_deals } else { w.tab == tab };
                    if ui.selectable_label(on && w.pages.is_empty(), label).clicked() {
                        if !(tab == Tab::Deals && in_deals) {
                            w.tab = tab;
                        }
                        w.pages.clear();
                    }
                }
                // Желаемое друга - временная вкладка, пока открыта.
                if let Some((_, name, _)) = st.friend_wishlist.clone() {
                    if ui.selectable_label(w.tab == Tab::FriendWishlist && w.pages.is_empty(), format!("Желаемое: {name}")).clicked() {
                        w.tab = Tab::FriendWishlist;
                        w.pages.clear();
                    }
                    if ui.small_button("✖").on_hover_text("Закрыть").clicked() {
                        close_friend_wish = true;
                    }
                }
                ui.separator();
                let current = st.country.clone();
                let label = store::COUNTRIES
                    .iter()
                    .find(|(c, _)| *c == current)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| if current.is_empty() { "…".into() } else { current.clone() });
                egui::ComboBox::from_id_salt("country")
                    .selected_text(format!("Цены: {label}"))
                    .show_ui(ui, |ui| {
                        for (cc, name) in store::COUNTRIES {
                            if ui.selectable_label(current == cc, name).clicked() && current != cc {
                                self.settings.store_country = Some(cc.to_string());
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
                    .on_hover_text("В какой валюте показывать цены. По умолчанию - по стране, которую Steam определил по IP.");
            });
            if w.pages.is_empty() && w.tab == Tab::Search {
                ui.horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut w.query)
                            .hint_text("Название игры, Enter - искать")
                            .desired_width(360.0),
                    );
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter || ui.button("Найти").clicked()) && !w.query.trim().is_empty() {
                        cmds.push(Cmd::Store(StoreCmd::Search(w.query.trim().to_string())));
                    }
                    if st.searching {
                        ui.spinner();
                    }
                });
            }
            if w.pages.is_empty() && matches!(w.tab, Tab::Deals | Tab::Events) {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut w.tab, Tab::Deals, "Популярное со скидкой")
                        .on_hover_text("50 самых продаваемых игр в вашем регионе, на которые сейчас есть скидка");
                    ui.selectable_value(&mut w.tab, Tab::Events, "Акции Steam")
                        .on_hover_text("Скидка дня, скидки недели и выходных, бесплатные выходные, новинки");
                });
            }
            if w.pages.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("store_sort")
                        .selected_text(format!("Сортировка: {}", w.sort.label()))
                        .show_ui(ui, |ui| {
                            for s in Sort::ALL {
                                ui.selectable_value(&mut w.sort, s, s.label());
                            }
                        });
                    ui.checkbox(&mut w.only_discount, "только со скидкой");
                    ui.checkbox(&mut w.only_free, "только бесплатные");
                    ui.checkbox(&mut w.hide_owned, "скрыть купленные");
                });
            }
            if let Some(e) = &st.error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
            }
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(&appid) = w.pages.last() {
                game_page(ui, appid, &st, &owned, &installed, &wish, &mut cmds, &mut open_page, &mut play, &mut install);
                return;
            }
            let hint = match w.tab {
                Tab::Deals => Some("Самые продаваемые игры в вашем регионе, на которые сейчас есть скидка."),
                Tab::Events => Some(
                    "Что Steam сейчас продвигает на главной странице: скидка дня, скидки середины недели и \
                     выходных, бесплатные выходные, новинки и обновления. Желтым - вид акции и до какого числа.",
                ),
                _ => None,
            };
            if let Some(h) = hint {
                ui.weak(h);
                ui.add_space(4.0);
            }
            // Пометки в списке «Акции Steam»: что за акция и до какого числа.
            let badges: std::collections::HashMap<u32, String> = match (&w.tab, &st.events) {
                (Tab::Events, Some(Ok(v))) => v
                    .iter()
                    .map(|(id, kind, end)| {
                        use chrono::TimeZone;
                        let until = chrono::Local
                            .timestamp_opt(*end as i64, 0)
                            .single()
                            .map(|t| format!(" · до {}", t.format("%d.%m")))
                            .unwrap_or_default();
                        (*id, format!("{kind}{until}"))
                    })
                    .collect(),
                _ => Default::default(),
            };
            let ids: Option<Result<Vec<u32>, String>> = match w.tab {
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
            };
            match ids {
                None if w.tab == Tab::Search => {
                    ui.weak("Введите название игры и нажмите Enter.");
                }
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.label(format!("Не удалось загрузить: {e}"));
                }
                Some(Ok(ids)) if ids.is_empty() => {
                    ui.weak(if st.searching { "Ищу..." } else { "Ничего не найдено." });
                }
                Some(Ok(all)) => {
                    let mut missing = Vec::new();
                    // Фильтры и сортировка нужны по подробностям всех игр списка:
                    // недостающие догружаем, а пока не пришли - игры не показываем.
                    let filtering = w.sort != Sort::Default || w.only_discount || w.only_free || w.hide_owned;
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
                            ui.weak(format!("Игр в списке желаемого: {}", all.len()));
                        }
                        if filtering {
                            ui.weak(format!("Показано: {}", ids.len()));
                            if loaded < all.len() {
                                ui.spinner();
                                ui.weak(format!("загружаю данные об играх: {loaded} из {}", all.len()));
                            }
                        }
                    });
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, ids.len(), |ui, range| {
                        for &id in &ids[range] {
                            match st.apps.get(&id) {
                                Some(a) => {
                                    if list_row(ui, a, owned.contains(&id), wish.contains(&id), badges.get(&id).map(String::as_str)) {
                                        open_page = Some(id);
                                    }
                                }
                                None => {
                                    if !w.asked.contains(&id) {
                                        missing.push(id);
                                    }
                                    ui.allocate_ui(egui::vec2(ui.available_width(), ROW_H), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.weak(format!("игра {id}"));
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
    }
}

/// Маленькая картинка игры (120x45); если у игры ее нет (у новых игр другие адреса) -
/// шапка из данных магазина.
fn capsule(ui: &egui::Ui, appid: u32, a: Option<&StoreAppInfo>) -> String {
    let small = store::small_capsule(appid);
    if ui.ctx().try_load_image(&small, egui::SizeHint::default()).is_err() {
        if let Some(h) = a.and_then(|a| a.assets.header.clone()) {
            return h;
        }
    }
    small
}

/// Строка списка: картинка, название, цена, отзывы. Возвращает true, если по ней щелкнули.
fn list_row(ui: &mut egui::Ui, a: &StoreAppInfo, owned: bool, wished: bool, badge: Option<&str>) -> bool {
    let resp = ui
        .allocate_ui(egui::vec2(ui.available_width(), ROW_H), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(capsule(ui, a.app_id, Some(a))).fit_to_exact_size(egui::vec2(120.0, 45.0)));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&a.name);
                        if let Some(b) = badge {
                            ui.label(egui::RichText::new(b).small().color(egui::Color32::from_rgb(240, 200, 80)));
                        }
                        if owned {
                            ui.label(egui::RichText::new("в библиотеке").small().color(GREEN));
                        } else if wished {
                            ui.label(egui::RichText::new("в желаемом").small().color(egui::Color32::from_rgb(110, 160, 230)));
                        }
                    });
                    ui.horizontal(|ui| {
                        price_ui(ui, a);
                        if let Some(r) = &a.review_summary {
                            ui.weak(review_ru(r));
                        }
                    });
                });
            });
        })
        .response;
    let resp = ui.interact(resp.rect, ui.id().with(("store_row", a.app_id)), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(resp.rect, 3.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
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
        ui.spinner();
        return;
    };
    let ex = st.extras.get(&appid);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(h) = &a.assets.header {
                ui.add(egui::Image::new(h).fit_to_exact_size(egui::vec2(368.0, 172.0)));
            }
            ui.vertical(|ui| {
                ui.heading(&a.name);
                ui.horizontal(|ui| price_ui(ui, a));
                if a.discount_pct > 0 {
                    if let Some(end) = a.discount_end {
                        ui.weak(format!("Скидка до {}", fmt_date(end)));
                    }
                }
                if let Some(r) = &a.review_summary {
                    ui.label(format!("Отзывы: {}", review_ru(r)));
                }
                if let Some(d) = &a.release_date {
                    ui.weak(format!("Дата выхода: {d}"));
                }
                if let Some(p) = ex.and_then(|e| e.players) {
                    ui.weak(format!("Сейчас играют: {p}"));
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if owned.contains(&appid) {
                        if installed.contains(&appid) {
                            if ui.button("Играть").clicked() {
                                *play = Some(appid);
                            }
                        } else if ui.button("Установить").clicked() {
                            *install = Some((appid, a.name.clone()));
                        }
                        ui.label(egui::RichText::new("в библиотеке").color(GREEN));
                        if ui.button("Страница в браузере").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(store_url(appid)));
                        }
                    } else {
                        if ui
                            .button("Купить в Steam")
                            .on_hover_text("Откроется страница игры в браузере: покупка проходит на сайте Steam.")
                            .clicked()
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(store_url(appid)));
                        }
                        if wish.contains(&appid) {
                            if ui.button("Убрать из желаемого").clicked() {
                                cmds.push(Cmd::Store(StoreCmd::WishlistSet(appid, false)));
                            }
                        } else if ui.button("В список желаемого").clicked() {
                            cmds.push(Cmd::Store(StoreCmd::WishlistSet(appid, true)));
                        }
                    }
                });
            });
        });

        ui.add_space(6.0);
        if !a.developers.is_empty() {
            ui.label(format!("Разработчик: {}", a.developers.join(", ")));
        }
        if !a.publishers.is_empty() {
            ui.label(format!("Издатель: {}", a.publishers.join(", ")));
        }
        let tags: Vec<String> = a.tags.iter().filter_map(|t| t.name.clone()).take(12).collect();
        if !tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for t in tags {
                    ui.label(egui::RichText::new(t).small().background_color(egui::Color32::from_gray(50)));
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
                            .on_hover_text("Открыть в полном размере в браузере");
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
        egui::CollapsingHeader::new("Описание").id_salt(("desc", appid)).show(ui, |ui| {
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

        egui::CollapsingHeader::new("Новости").id_salt(("news", appid)).show(ui, |ui| match ex.and_then(|e| e.news.as_ref()) {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.weak(format!("Не удалось загрузить: {e}"));
            }
            Some(Ok(n)) if n.is_empty() => {
                ui.weak("Новостей нет.");
            }
            Some(Ok(items)) => {
                for n in items {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(fmt_date(n.date));
                        ui.hyperlink_to(&n.title, &n.url);
                    });
                    let text = store::plain_text(&n.contents);
                    if !text.is_empty() {
                        ui.weak(text.chars().take(300).collect::<String>());
                    }
                    ui.add_space(4.0);
                }
            }
        });

        if let Some((min, rec)) = ex.and_then(|e| e.requirements.as_ref()) {
            egui::CollapsingHeader::new("Системные требования").id_salt(("req", appid)).show(ui, |ui| {
                if !min.is_empty() {
                    ui.label(min);
                }
                if !rec.is_empty() {
                    ui.add_space(4.0);
                    ui.label(rec);
                }
            });
        }

        egui::CollapsingHeader::new("Отзывы").id_salt(("reviews", appid)).show(ui, |ui| {
            match &a.review_summary {
                Some(r) => ui.label(format!("Итог: {}", review_ru(r))),
                None => ui.weak("Отзывов пока мало."),
            };
            let rv = ex.and_then(|e| e.reviews.as_ref());
            let (sort, all_langs) = rv.map(|r| (r.sort, r.all_langs)).unwrap_or((0, false));
            if rv.is_none() {
                // Раздел открыли - первая страница (полезные за все время, на русском), как на сайте.
                cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort: 0, all_langs: false, page: 1 }));
            }
            ui.horizontal(|ui| {
                let (mut r2, mut l2) = (sort, all_langs);
                egui::ComboBox::from_id_salt(("rev_sort", appid))
                    .selected_text(store::REVIEW_SORTS[r2 as usize].1)
                    .show_ui(ui, |ui| {
                        for (i, (_, label)) in store::REVIEW_SORTS.iter().enumerate() {
                            ui.selectable_value(&mut r2, i as u8, *label);
                        }
                    });
                ui.separator();
                ui.selectable_value(&mut l2, false, "На русском");
                ui.selectable_value(&mut l2, true, "На всех языках");
                if (r2, l2) != (sort, all_langs) {
                    cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort: r2, all_langs: l2, page: 1 }));
                }
                ui.separator();
                ui.hyperlink_to("В браузере", format!("https://steamcommunity.com/app/{appid}/reviews/"));
            });
            let Some(rv) = rv else {
                ui.spinner();
                return;
            };
            if let Some(e) = &rv.error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), format!("Не удалось загрузить отзывы: {e}"));
            }
            if rv.list.is_empty() && !rv.loading && rv.error.is_none() {
                ui.weak("Отзывов с такими условиями нет.");
            }
            for r in &rv.list {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let (c, t) = if r.positive {
                            (egui::Color32::from_rgb(110, 190, 240), "Рекомендует")
                        } else {
                            (egui::Color32::from_rgb(230, 110, 110), "Не рекомендует")
                        };
                        ui.colored_label(c, egui::RichText::new(t).strong());
                        ui.label(&r.author);
                        ui.weak(format!("· {} · {}", r.hours, r.date));
                    });
                    // Очень длинные отзывы - начало и ссылка на полный текст.
                    let long = r.text.chars().count() > 1200;
                    let text: String = if long { r.text.chars().take(1200).collect::<String>() + "…" } else { r.text.clone() };
                    ui.label(text);
                    ui.horizontal_wrapped(|ui| {
                        if !r.helpful.is_empty() {
                            ui.weak(egui::RichText::new(r.helpful.replace('\n', " · ")).small());
                        }
                        if long && !r.url.is_empty() {
                            ui.hyperlink_to("Читать полностью", &r.url);
                        }
                    });
                });
            }
            if rv.loading {
                ui.spinner();
            } else if rv.more && ui.button("Показать еще").clicked() {
                cmds.push(Cmd::Store(StoreCmd::Reviews { appid, sort, all_langs, page: rv.pages + 1 }));
            } else if sort == 0 && rv.list.len() >= 10 {
                ui.weak("За все время Steam показывает только первые 10 отзывов. Больше - выберите период (например, «за год») или «Новые».");
            }
        });


        if let Some(sim) = ex.and_then(|e| e.similar.as_ref()) {
            if !sim.is_empty() {
                ui.add_space(8.0);
                ui.strong("Похожие игры");
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
