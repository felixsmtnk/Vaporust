//! Окно «Инвентарь» (отдельное окно Windows): предметы по играм, кошелек, свои лоты
//! и поиск по торговой площадке. Продажа и покупка - на сайте Steam в браузере.

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
}

pub struct InvWin {
    tab: Tab,
    /// Выбранная игра (все ее разделы инвентаря показываются вместе).
    app: Option<u32>,
    filter: String,
    /// Выбранный предмет (class_id).
    selected: Option<String>,
    query: String,
    /// Цены, которые уже запрошены (чтобы не спрашивать каждый кадр).
    asked_prices: HashSet<(u32, String)>,
}

/// Одинаковые предметы (один class_id) - одной плиткой с общим количеством.
struct Stack<'a> {
    item: &'a InventoryItem,
    amount: u64,
    /// Раздел инвентаря (для ссылки на предмет в Steam).
    ctx: u32,
}

fn stacks<'a>(parts: &[(u32, &'a [InventoryItem])], filter: &str) -> Vec<Stack<'a>> {
    let f = filter.trim().to_lowercase();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<Stack> = Vec::new();
    for (ctx, it) in parts.iter().flat_map(|(c, items)| items.iter().map(move |it| (*c, it))) {
        if !f.is_empty() && !it.name.to_lowercase().contains(&f) && !it.item_type.to_lowercase().contains(&f) {
            continue;
        }
        match index.get(it.class_id.as_str()) {
            Some(&i) => out[i].amount += it.amount,
            None => {
                index.insert(&it.class_id, out.len());
                out.push(Stack { item: it, amount: it.amount, ctx });
            }
        }
    }
    out
}

/// Разделы инвентаря игры (из списка игр; пока его нет - пусто).
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

/// Иконка предмета нужного размера (сервер Steam уменьшает сам).
fn icon(url: &str, px: u32) -> String {
    format!("{url}/{px}fx{px}f")
}

impl LauncherApp {
    /// Открыть инвентарь: в главном окне или (detached) в отдельном.
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
            });
            // Кошелек - первым: по нему выбирается валюта цен.
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
                .with_title("Инвентарь - Vaporust")
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

    /// Содержимое инвентаря: и в главном окне, и в отдельном.
    pub(crate) fn inventory_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let shared = net.inv.clone();
        let st = shared.lock().unwrap();
        let mut cmds: Vec<InvCmd> = Vec::new();
        let my_id = net.my_id.unwrap_or(0);
        let Some(w) = self.inv_win.as_mut() else { return };

        // Первая игра выбирается сама, как только пришел список.
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
                for (tab, label) in [(Tab::Items, "Инвентарь"), (Tab::Listings, "Мои лоты"), (Tab::Market, "Торговая площадка")] {
                    ui.selectable_value(&mut w.tab, tab, label);
                }
                ui.separator();
                match &st.wallet {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Ok(wl)) => {
                        ui.label(format!("Кошелек: {}", crate::invent::money(wl.balance_cents, wl.currency)));
                    }
                    Some(Err(e)) => {
                        ui.weak("Кошелек: нет данных").on_hover_text(e);
                    }
                }
                if ui.small_button("Обновить").on_hover_text("Перечитать кошелек, лоты и предметы").clicked() {
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
            Tab::Listings => listings_tab(ui, &st),
            Tab::Market => market_tab(ui, w, &st, &mut cmds),
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
                    // Новый поиск - с чистого листа; «Показать еще» оставляет загруженное.
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
    // Слева - игры с предметами.
    egui::SidePanel::left("inv_apps").resizable(true).default_width(200.0).show_inside(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.apps {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
            }
            Some(Ok(apps)) => {
                for a in apps {
                    // Разделы игры (у CS2 - обычные и «защищенные от обмена», у Steam -
                    // подарки и предметы сообщества) показываем одним списком.
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
    // Предметы всех разделов игры; пока какой-то раздел грузится - крутится значок.
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
                ui.label(format!("Не удалось загрузить предметы: {e}"));
                ui.weak("Steam ограничивает частые запросы инвентаря - попробуйте «Обновить» через минуту.");
            }
            _ => {
                crate::spin::spinner(ui);
            }
        });
        return;
    }
    let total: usize = parts.iter().map(|p| p.1.len()).sum();
    let list = stacks(&parts, &w.filter);

    // Справа - выбранный предмет.
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
                    ui.label(format!("Количество: {}", s.amount));
                }
                ui.label(if it.tradable { "Можно обменять" } else { "Нельзя обменять" });
                ui.label(if it.marketable { "Можно продать на площадке" } else { "Нельзя продать на площадке" });
                if it.marketable && !it.market_hash_name.is_empty() {
                    let key = (appid, it.market_hash_name.clone());
                    ui.separator();
                    match st.prices.get(&key) {
                        None => {
                            // Цену спрашиваем, когда известен кошелек: в его валюте.
                            if st.wallet.is_some() && w.asked_prices.insert(key.clone()) {
                                cmds.push(InvCmd::Price { appid, name: it.market_hash_name.clone() });
                            }
                            ui.horizontal(|ui| {
                                crate::spin::spinner(ui);
                                ui.weak("Узнаю цену...");
                            });
                        }
                        Some(Err(_)) => {
                            ui.weak("Цену узнать не удалось (Steam ограничивает частые запросы).");
                        }
                        Some(Ok(p)) => {
                            egui::Grid::new("price").num_columns(2).show(ui, |ui| {
                                ui.weak("Самая низкая");
                                ui.label(p.lowest_price.as_deref().unwrap_or("-"));
                                ui.end_row();
                                ui.weak("Средняя");
                                ui.label(p.median_price.as_deref().unwrap_or("-"));
                                ui.end_row();
                                ui.weak("Продано за сутки");
                                ui.label(p.volume.as_deref().unwrap_or("0"));
                                ui.end_row();
                            });
                        }
                    }
                    if ui.button("Страница на площадке").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(invent::listing_url(appid, &it.market_hash_name)));
                    }
                }
                ui.separator();
                if ui
                    .button("Открыть в инвентаре Steam")
                    .on_hover_text("Продать, обменять или использовать предмет - на сайте Steam")
                    .clicked()
                {
                    let url = format!("https://steamcommunity.com/profiles/{my_id}/inventory/#{appid}_{}_{}", s.ctx, it.asset_id);
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        });
    }

    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut w.filter).hint_text("Поиск по предметам").desired_width(220.0));
            ui.weak(format!("Предметов: {}", list.len()));
            if loading {
                crate::spin::spinner(ui);
            }
        });
        ui.add_space(4.0);
        if list.is_empty() {
            ui.weak(if total == 0 { "Здесь пусто." } else { "Ничего не найдено." });
            return;
        }
        // Плитка; грузятся и рисуются только видимые ряды.
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

/// Строка списка: картинка 40x40, название-ссылка на площадку, подпись и справа цена.
fn item_row(ui: &mut egui::Ui, icon_url: &Option<String>, name: &str, url: String, sub: &str, price: &str, price_hint: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, ui.visuals().faint_bg_color);
        if let Some(u) = icon_url {
            egui::Image::new(icon(u, 64)).paint_at(ui, rect);
        }
        ui.vertical(|ui| {
            ui.hyperlink_to(name, url).on_hover_text("Открыть страницу предмета на площадке в браузере");
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

fn listings_tab(ui: &mut egui::Ui, st: &InvState) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        if ui.button("Торговая площадка в браузере").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://steamcommunity.com/market/"));
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.listings {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.label(format!("Не удалось загрузить: {e}"));
            }
            Some(Ok(m)) => {
                ui.strong(format!("Выставлено на продажу: {}", m.lots.len()));
                if m.to_confirm > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(240, 200, 80),
                        format!("Ждут подтверждения в мобильном приложении Steam: {}", m.to_confirm),
                    );
                }
                if m.lots.is_empty() {
                    ui.weak("Нет активных лотов.");
                }
                ui.add_space(4.0);
                for l in &m.lots {
                    item_row(
                        ui,
                        &l.icon,
                        &l.name,
                        invent::listing_url(l.appid, &l.hash_name),
                        &format!("вы получите {}", invent::money(l.receive, l.currency)),
                        &invent::money(l.pay, l.currency),
                        "Цена для покупателя (с комиссией Steam)",
                    );
                }
                ui.add_space(10.0);
                ui.strong(format!("Заявки на покупку: {}", m.orders.len()));
                if m.orders.is_empty() {
                    ui.weak("Нет открытых заявок.");
                }
                let cur = invent::wallet_currency(st);
                for o in &m.orders {
                    item_row(
                        ui,
                        &o.icon,
                        &o.name,
                        invent::listing_url(o.appid, &o.hash_name),
                        &format!("{} шт.", o.quantity),
                        &format!("по {}", invent::money(o.price, cur)),
                        "",
                    );
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
                    .hint_text("Название предмета, Enter - искать")
                    .desired_width(300.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || ui.button("Искать").clicked()) && !w.query.trim().is_empty() {
                cmds.push(InvCmd::Search { query: w.query.trim().to_string(), start: 0 });
            }
            if st.searching {
                crate::spin::spinner(ui);
            }
        });
        ui.weak("Цена - самая низкая на площадке сейчас. Купить - на странице предмета в браузере.")
            .on_hover_text(
                "Steam находит только то, что сейчас кто-то продает. Очень дорогие предметы \
                 (дороже предела площадки, около 2000 $, например AWP | Dragon Lore) на ней не \
                 выставляют, поэтому их не найти.",
            );
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &st.search {
            None => {}
            Some((_, Err(e))) => {
                ui.label(format!("Не удалось: {e}"));
            }
            Some((query, Ok((total, list)))) => {
                if list.is_empty() {
                    if !st.searching {
                        ui.weak("Ничего не найдено.");
                        ui.weak(
                            "Steam ищет только предметы, которые сейчас кто-то продает. Очень дорогие \
                             предметы (дороже предела площадки, около 2000 $) на ней не выставляют.",
                        );
                    }
                    return;
                }
                ui.weak(format!("Найдено: {total}, показано {}", list.len()));
                ui.add_space(4.0);
                for r in list {
                    item_row(
                        ui,
                        &r.icon,
                        &r.name,
                        invent::listing_url(r.appid, &r.hash_name),
                        &format!("{} · лотов: {}", r.app_name, r.listings),
                        &r.price_text,
                        "",
                    );
                }
                if (list.len() as u32) < *total {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!st.searching, egui::Button::new("Показать еще")).clicked() {
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
