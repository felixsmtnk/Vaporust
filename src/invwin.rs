//! "Inventory" window (a separate Windows window): items by game, wallet, own listings
//! and Community Market search. Selling and buying happen on the Steam site in a browser.

use crate::invent::{self, InvCmd, InvState};
use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use aurelia::steam_client::InventoryItem;
use eframe::egui;
use std::collections::{HashMap, HashSet};

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Items,
    Listings,
    Market,
    Offers,
}

pub struct InvWin {
    tab: Tab,
    /// Selected game (all its inventory sections are shown together).
    app: Option<u32>,
    filter: String,
    /// Selected item (class_id).
    selected: Option<String>,
    query: String,
    /// Prices already requested (so we do not ask every frame).
    asked_prices: HashSet<(u32, String)>,
    /// Market: what is already requested (orders and history), sell window, confirmations.
    pub(crate) mk: crate::trade::MarketUi,
}

/// Identical items (one class_id) - one tile with the total amount.
struct Stack<'a> {
    item: &'a InventoryItem,
    amount: u64,
    /// Inventory section (for the item link in Steam).
    ctx: u32,
}

fn stacks<'a>(parts: &[(u32, &'a [InventoryItem])], filter: &str, stack: bool) -> Vec<Stack<'a>> {
    let f = filter.trim().to_lowercase();
    let mut index: HashMap<(&str, bool, bool, u32), usize> = HashMap::new();
    let mut out: Vec<Stack> = Vec::new();
    for (ctx, it) in parts.iter().flat_map(|(c, items)| items.iter().map(move |it| (*c, it))) {
        if !f.is_empty() && !it.name.to_lowercase().contains(&f) && !it.item_type.to_lowercase().contains(&f) {
            continue;
        }
        // Only equal items: the same class, section and the same "can be traded / sold".
        let key = (it.class_id.as_str(), it.tradable, it.marketable, ctx);
        match index.get(&key).filter(|_| stack) {
            Some(&i) => out[i].amount += it.amount,
            None => {
                index.insert(key, out.len());
                out.push(Stack { item: it, amount: it.amount, ctx });
            }
        }
    }
    out
}

/// Inventory sections of the game (from the game list; empty while it is missing).
fn app_ctxs(st: &InvState, appid: u32) -> Vec<u32> {
    match &st.apps {
        Some(Ok(apps)) => apps
            .iter()
            .find(|a| a.appid == appid)
            .map(|a| a.contexts.iter().map(|c| c.0).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Item icon of the needed size (the Steam server scales it).
fn icon(url: &str, px: u32) -> String {
    format!("{url}/{px}fx{px}f")
}

impl LauncherApp {
    /// Open the inventory: in the main window or (detached) in a separate one.
    pub(crate) fn open_inventory(&mut self, detached: bool) {
        if detached {
            self.inv_detached = true;
        } else {
            self.inv_detached = false;
            self.view = crate::View::Inventory;
        }
        if self.inv_win.is_none() {
            self.inv_win = Some(InvWin {
                tab: Tab::Items,
                app: None,
                filter: String::new(),
                selected: None,
                query: String::new(),
                asked_prices: HashSet::new(),
                mk: Default::default(),
            });
            // Wallet first: it decides the price currency.
            for c in [InvCmd::Wallet, InvCmd::Apps, InvCmd::Listings] {
                let _ = self.tx.send(Cmd::Inv(c));
            }
        }
    }

    pub(crate) fn inventory_window(&mut self, ctx: &egui::Context, net: &NetState) {
        if self.inv_win.is_none() || !self.inv_detached {
            return;
        }
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_inventory"),
            egui::ViewportBuilder::default()
                .with_title(tr!("Инвентарь - Vaporust", "Inventory - Vaporust"))
                .with_inner_size([900.0, 620.0])
                .with_min_inner_size([620.0, 400.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
                .with_decorations(crate::winframe::system()),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.inventory_contents(ui, net));
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.inv_win = None;
            self.inv_detached = false;
        }
    }

    /// Inventory contents: both in the main window and in the separate one.
    pub(crate) fn inventory_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if let Some(w) = self.inv_win.as_mut() {
            w.mk.undercut = self.settings.quick_undercut;
        }
        let shared = net.inv.clone();
        let st = shared.lock().unwrap();
        let mut cmds: Vec<InvCmd> = Vec::new();
        let my_id = net.my_id.unwrap_or(0);
        let Some(w) = self.inv_win.as_mut() else { return };

        // The first game is selected automatically once the list arrives.
        if w.app.is_none() {
            if let Some(Ok(apps)) = &st.apps {
                if let Some(a) = apps.first() {
                    w.app = Some(a.appid);
                }
            }
        }
        if let Some(appid) = w.app {
            for cx in app_ctxs(&st, appid) {
                if !st.items.contains_key(&(appid, cx)) && !st.loading.contains(&(appid, cx)) {
                    cmds.push(InvCmd::Items { appid, ctx: cx });
                }
            }
        }

        egui::TopBottomPanel::top("inv_top").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [(Tab::Items, tr!("Инвентарь", "Inventory")), (Tab::Listings, tr!("Мои лоты", "My listings")), (Tab::Market, tr!("Торговая площадка", "Community Market")), (Tab::Offers, tr!("Обмены", "Trades"))] {
                    ui.selectable_value(&mut w.tab, tab, label);
                }
                ui.separator();
                match &st.wallet {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Ok(wl)) => {
                        ui.label(trf!("Кошелек: {}", "Wallet: {}", crate::invent::money(wl.balance_cents, wl.currency)));
                    }
                    Some(Err(e)) => {
                        ui.weak(tr!("Кошелек: нет данных", "Wallet: no data")).on_hover_text(e);
                    }
                }
                if ui.small_button(tr!("Обновить", "Refresh")).on_hover_text(tr!("Перечитать кошелек, лоты и предметы", "Reload the wallet, listings and items")).clicked() {
                    cmds.push(InvCmd::Wallet);
                    cmds.push(InvCmd::Listings);
                    if let Some(appid) = w.app {
                        for cx in app_ctxs(&st, appid) {
                            cmds.push(InvCmd::Items { appid, ctx: cx });
                        }
                    }
                }
            });
            ui.add_space(4.0);
        });

        match w.tab {
            Tab::Items => items_tab(ui, w, &st, my_id, &mut cmds),
            Tab::Listings => listings_tab(ui, &st, &mut w.mk),
            Tab::Market => market_tab(ui, w, &st, &mut cmds),
            Tab::Offers => crate::trade::offers_tab(ui, &st, &mut w.mk),
        }

        drop(st);
        let mut s = shared.lock().unwrap();
        for c in cmds {
            match &c {
                InvCmd::Items { appid, ctx } => {
                    s.loading.insert((*appid, *ctx));
                }
                InvCmd::Search { query, start } => {
                    s.searching = true;
                    // A new search starts from scratch; "Show more" keeps what is loaded.
                    if *start == 0 {
                        s.search = Some((query.clone(), Ok((0, Vec::new()))));
                    }
                }
                _ => {}
            }
            let _ = self.tx.send(Cmd::Inv(c));
        }
    }
}

fn items_tab(ui: &mut egui::Ui, w: &mut InvWin, st: &InvState, my_id: u64, cmds: &mut Vec<InvCmd>) {
    // On the left - games with items.
    egui::SidePanel::left("inv_apps").resizable(true).default_width(200.0).show_inside(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.apps {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
            }
            Some(Ok(apps)) => {
                for a in apps {
                    // Game sections (CS2 has normal and "trade protected", Steam has
                    // gifts and community items) are shown as one list.
                    let count: u64 = a.contexts.iter().map(|c| c.2).sum();
                    let sel = w.app == Some(a.appid);
                    ui.horizontal(|ui| {
                        match a.icon.as_deref().filter(|u| u.starts_with("http")) {
                            Some(u) => {
                                ui.add(egui::Image::new(u).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                            }
                            None => {
                                ui.add_space(22.0);
                            }
                        }
                        if ui.selectable_label(sel, format!("{} ({count})", a.name)).clicked() && !sel {
                            w.app = Some(a.appid);
                            w.selected = None;
                        }
                    });
                }
            }
        });
    });

    let Some(appid) = w.app else {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            crate::spin::spinner(ui);
        });
        return;
    };
    // Items of all game sections; while some section loads, a spinner turns.
    let mut parts: Vec<(u32, &[InventoryItem])> = Vec::new();
    let mut error: Option<&String> = None;
    let mut loading = false;
    for cx in app_ctxs(st, appid) {
        match st.items.get(&(appid, cx)) {
            Some(Ok(v)) => parts.push((cx, v.as_slice())),
            Some(Err(e)) => error = Some(e),
            None => loading = true,
        }
    }
    if parts.is_empty() {
        egui::CentralPanel::default().show_inside(ui, |ui| match error {
            Some(e) if !loading => {
                ui.label(trf!("Не удалось загрузить предметы: {e}", "Could not load items: {e}"));
                ui.weak(tr!("Steam ограничивает частые запросы инвентаря - попробуйте «Обновить» через минуту.", "Steam limits frequent inventory requests - try \"Refresh\" in a minute."));
            }
            _ => {
                crate::spin::spinner(ui);
            }
        });
        return;
    }
    let total: usize = parts.iter().map(|p| p.1.len()).sum();
    // Steam items (cards, backgrounds) are stacked; game items are shown one by one, as in Steam:
    // two copies of a skin differ (wear, stickers, trade lock), the first one must not stand for all.
    let list = stacks(&parts, &w.filter, w.app == Some(753));

    // On the right - the selected item.
    let chosen = w.selected.as_ref().and_then(|id| list.iter().find(|s| &s.item.class_id == id));
    if let Some(s) = chosen {
        let it = s.item;
        egui::SidePanel::right("inv_item").resizable(false).exact_width(260.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if let Some(u) = &it.icon_url {
                    ui.add(egui::Image::new(icon(u, 192)).fit_to_exact_size(egui::vec2(160.0, 160.0)));
                }
                ui.strong(&it.name);
                if !it.item_type.is_empty() {
                    ui.weak(&it.item_type);
                }
                if s.amount > 1 {
                    ui.label(trf!("Количество: {}", "Quantity: {}", s.amount));
                }
                ui.label(if it.tradable { tr!("Можно обменять", "Tradable") } else { tr!("Нельзя обменять", "Not tradable") });
                ui.label(if it.marketable { tr!("Можно продать на площадке", "Marketable") } else { tr!("Нельзя продать на площадке", "Not marketable") });
                if it.marketable && !it.market_hash_name.is_empty() {
                    let key = (appid, it.market_hash_name.clone());
                    ui.separator();
                    match st.prices.get(&key) {
                        None => {
                            // The price is requested once the wallet is known: in its currency.
                            if st.wallet.is_some() && w.asked_prices.insert(key.clone()) {
                                cmds.push(InvCmd::Price { appid, name: it.market_hash_name.clone() });
                            }
                            ui.horizontal(|ui| {
                                crate::spin::spinner(ui);
                                ui.weak(tr!("Узнаю цену...", "Checking the price..."));
                            });
                        }
                        Some(Err(_)) => {
                            ui.weak(tr!("Цену узнать не удалось (Steam ограничивает частые запросы).", "Could not get the price (Steam limits frequent requests)."));
                        }
                        Some(Ok(p)) => {
                            egui::Grid::new("price").num_columns(2).show(ui, |ui| {
                                ui.weak(tr!("Самая низкая", "Lowest"));
                                ui.label(p.lowest_price.as_deref().unwrap_or("-"));
                                ui.end_row();
                                ui.weak(tr!("Средняя", "Median"));
                                ui.label(p.median_price.as_deref().unwrap_or("-"));
                                ui.end_row();
                                ui.weak(tr!("Продано за сутки", "Sold in 24 hours"));
                                ui.label(p.volume.as_deref().unwrap_or("0"));
                                ui.end_row();
                            });
                        }
                    }
                    crate::trade::market_block(ui, &mut w.mk, st, appid, s.ctx, it);
                    if ui.button(tr!("Страница на площадке", "Market page")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(invent::listing_url(appid, &it.market_hash_name)));
                    }
                }
                ui.separator();
                if ui
                    .button(tr!("Открыть в инвентаре Steam", "Open in the Steam inventory"))
                    .on_hover_text(tr!("Продать, обменять или использовать предмет - на сайте Steam", "Sell, trade or use an item on the Steam website"))
                    .clicked()
                {
                    let url = format!("https://steamcommunity.com/profiles/{my_id}/inventory/#{appid}_{}_{}", s.ctx, it.asset_id);
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        });
    }

    egui::CentralPanel::default().show_inside(ui, |ui| {
        // Search width fits the space (little when an item is open on the right), the count wraps.
        ui.horizontal_wrapped(|ui| {
            let fw = (ui.available_width() - 110.0).clamp(90.0, 220.0);
            ui.add(egui::TextEdit::singleline(&mut w.filter).hint_text(tr!("Поиск по предметам", "Search items")).desired_width(fw));
            ui.weak(trf!("Предметов: {}", "Items: {}", list.len()));
            if loading {
                crate::spin::spinner(ui);
            }
        });
        ui.add_space(4.0);
        if list.is_empty() {
            ui.weak(if total == 0 { tr!("Здесь пусто.", "Nothing here.") } else { tr!("Ничего не найдено.", "Nothing found.") });
            return;
        }
        // Tiles; only visible rows are loaded and drawn.
        let cell = egui::vec2(96.0, 118.0);
        let cols = ((ui.available_width() - 16.0) / (cell.x + 6.0)).floor().max(1.0) as usize;
        let rows = list.len().div_ceil(cols);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell.y + 6.0, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    for s in list.iter().skip(row * cols).take(cols) {
                        let it = s.item;
                        let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                        let sel = w.selected.as_deref() == Some(it.class_id.as_str());
                        let bg = if sel {
                            ui.visuals().selection.bg_fill
                        } else if resp.hovered() {
                            ui.visuals().widgets.hovered.bg_fill
                        } else {
                            ui.visuals().faint_bg_color
                        };
                        ui.painter().rect_filled(rect, 4.0, bg);
                        let img = egui::Rect::from_center_size(rect.center_top() + egui::vec2(0.0, 40.0), egui::vec2(72.0, 72.0));
                        if let Some(u) = &it.icon_url {
                            egui::Image::new(icon(u, 96)).paint_at(ui, img);
                        }
                        if s.amount > 1 {
                            ui.painter().text(
                                rect.right_top() + egui::vec2(-4.0, 3.0),
                                egui::Align2::RIGHT_TOP,
                                format!("x{}", s.amount),
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(240, 200, 80),
                            );
                        }
                        let text = egui::Rect::from_min_max(egui::pos2(rect.left() + 3.0, rect.top() + 80.0), rect.max - egui::vec2(3.0, 2.0));
                        ui.put(text, egui::Label::new(egui::RichText::new(&it.name).small()).truncate());
                        let resp = resp.on_hover_text(format!("{}\n{}", it.name, it.item_type)).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            w.selected = if sel { None } else { Some(it.class_id.clone()) };
                        }
                    }
                });
            }
        });
    });
}

/// List row: 40x40 image, name linking to the market, caption and price on the right.
fn item_row(ui: &mut egui::Ui, icon_url: &Option<String>, name: &str, url: String, sub: &str, price: &str, price_hint: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, ui.visuals().faint_bg_color);
        if let Some(u) = icon_url {
            egui::Image::new(icon(u, 64)).paint_at(ui, rect);
        }
        ui.vertical(|ui| {
            ui.hyperlink_to(name, url).on_hover_text(tr!("Открыть страницу предмета на площадке в браузере", "Open the item's market page in the browser"));
            if !sub.is_empty() {
                ui.weak(sub);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui.strong(price);
            if !price_hint.is_empty() {
                r.on_hover_text(price_hint);
            }
        });
    });
    ui.separator();
}

fn listings_tab(ui: &mut egui::Ui, st: &InvState, mk: &mut crate::trade::MarketUi) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        if ui.button(tr!("Торговая площадка в браузере", "Community Market in the browser")).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://steamcommunity.com/market/"));
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.listings {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
            }
            Some(Ok(m)) => {
                ui.strong(trf!("Выставлено на продажу: {}", "Listed for sale: {}", m.lots.len()));
                if m.to_confirm > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(240, 200, 80),
                        trf!("Ждут подтверждения в мобильном приложении Steam: {}", "Waiting for confirmation in the Steam mobile app: {}", m.to_confirm),
                    );
                }
                if m.lots.is_empty() {
                    ui.weak(tr!("Нет активных лотов.", "No active listings."));
                }
                ui.add_space(4.0);
                for l in &m.lots {
                    item_row(
                        ui,
                        &l.icon,
                        &l.name,
                        invent::listing_url(l.appid, &l.hash_name),
                        &trf!("вы получите {}", "you receive {}", invent::money(l.receive, l.currency)),
                        &invent::money(l.pay, l.currency),
                        tr!("Цена для покупателя (с комиссией Steam)", "Buyer pays (with the Steam fee)"),
                    );
                    crate::trade::remove_lot_button(ui, mk, &l.id, &l.name);
                }
                ui.add_space(10.0);
                ui.strong(trf!("Заявки на покупку: {}", "Buy orders: {}", m.orders.len()));
                if m.orders.is_empty() {
                    ui.weak(tr!("Нет открытых заявок.", "No open buy orders."));
                }
                let cur = invent::wallet_currency(st);
                for o in &m.orders {
                    item_row(
                        ui,
                        &o.icon,
                        &o.name,
                        invent::listing_url(o.appid, &o.hash_name),
                        &trf!("{} шт.", "{} pcs.", o.quantity),
                        &trf!("по {}", "at {}", invent::money(o.price, cur)),
                        "",
                    );
                    crate::trade::cancel_order_button(ui, mk, &o.id, &o.name);
                }
            }
        });
    });
}

fn market_tab(ui: &mut egui::Ui, w: &mut InvWin, st: &InvState, cmds: &mut Vec<InvCmd>) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut w.query)
                    .hint_text(tr!("Название предмета, Enter - искать", "Item name, Enter - search"))
                    .desired_width(300.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || ui.button(tr!("Искать", "Search")).clicked()) && !w.query.trim().is_empty() {
                cmds.push(InvCmd::Search { query: w.query.trim().to_string(), start: 0 });
            }
            if st.searching {
                crate::spin::spinner(ui);
            }
        });
        ui.weak(tr!("Цена - самая низкая на площадке сейчас. Купить - на странице предмета в браузере.", "The price is the lowest on the market right now. Buy on the item page in the browser."))
            .on_hover_text(
                tr!("Steam находит только то, что сейчас кто-то продает. Очень дорогие предметы \
                 (дороже предела площадки, около 2000 $, например AWP | Dragon Lore) на ней не \
                 выставляют, поэтому их не найти.", "Steam only finds what someone is selling right now. Very expensive items (above the market limit, about $2000, for example AWP | Dragon Lore) are not listed there, so they cannot be found."),
            );
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.search {
            None => {}
            Some((_, Err(e))) => {
                ui.label(trf!("Не удалось: {e}", "Failed: {e}"));
            }
            Some((query, Ok((total, list)))) => {
                if list.is_empty() {
                    if !st.searching {
                        ui.weak(tr!("Ничего не найдено.", "Nothing found."));
                        ui.weak(
                            tr!("Steam ищет только предметы, которые сейчас кто-то продает. Очень дорогие \
                             предметы (дороже предела площадки, около 2000 $) на ней не выставляют.", "Steam only searches items that someone is selling right now. Very expensive items (above the market limit, about $2000) are not listed there."),
                        );
                    }
                    return;
                }
                ui.weak(trf!("Найдено: {total}, показано {}", "Found: {total}, shown {}", list.len()));
                ui.add_space(4.0);
                for r in list {
                    item_row(
                        ui,
                        &r.icon,
                        &r.name,
                        invent::listing_url(r.appid, &r.hash_name),
                        &trf!("{} · лотов: {}", "{} · listings: {}", r.app_name, r.listings),
                        &r.price_text,
                        "",
                    );
                }
                if (list.len() as u32) < *total {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!st.searching, egui::Button::new(tr!("Показать еще", "Show more"))).clicked() {
                            cmds.push(InvCmd::Search { query: query.clone(), start: list.len() as u32 });
                        }
                        if st.searching {
                            crate::spin::spinner(ui);
                        }
                    });
                }
            }
        });
    });
}

impl InvWin {
    /// Section tour (tour.rs): select a game and an item.
    pub(crate) fn tour_select(&mut self, app: u32, class: String) {
        self.tab = Tab::Items;
        self.app = Some(app);
        self.selected = Some(class);
    }
}
