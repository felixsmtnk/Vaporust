//! Market UI in the inventory: item orders and price chart, selling (own price,
//! quick, instant), "Remove listing", "Cancel order", the "Trades" tab.

use crate::trade::{MarketCmd, OfferItem};
use eframe::egui;

/// Requests from the inventory UI to the network (sent by LauncherApp::drain_ui_req).
pub static MARKET_REQ: std::sync::Mutex<Vec<MarketCmd>> = std::sync::Mutex::new(Vec::new());

fn req(c: MarketCmd) {
    MARKET_REQ.lock().unwrap().push(c);
}

/// Sell window: what, how many, buyer price, where the price came from.
#[derive(Clone)]
pub struct SellDlg {
    pub appid: u32,
    pub ctx: u32,
    pub assetid: String,
    pub name: String,
    /// The item's market_hash_name (the sale result is tied to it).
    pub hash: String,
    pub max_amount: u64,
    pub amount: u64,
    /// Buyer price, minimal units.
    pub pay: i64,
    pub pay_text: String,
    /// 0 - own price, 1 - quick, 2 - instant.
    pub mode: u8,
}

#[derive(Default)]
pub struct MarketUi {
    pub asked: std::collections::HashSet<(u32, String)>,
    pub sell: Option<SellDlg>,
    /// Waiting for a second click: remove listing / cancel order (id).
    pub confirm: Option<String>,
    pub offers_asked: bool,
    /// Trade: (id, action 0 accept / 1 decline / 2 cancel) - awaiting confirmation.
    pub offer_confirm: Option<(String, u8)>,
    /// How much cheaper quick sell is than the lowest listing (minimal units; 0 - by one
    /// currency price step). Taken from settings every frame.
    pub undercut: i64,
}

/// Price from the input field, in kopecks/cents. Strict: only digits and at most one
/// separator ("," or ".") with 1-2 digits after it, for example "12", "12,5", "0.99".
/// Any other text ("1.2.3", "12,345", "10р", "-5") - None, not a guess.
fn parse_money(s: &str) -> Option<i64> {
    let t = s.trim();
    let (int, frac) = match t.find([',', '.']) {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, ""),
    };
    let digits = |x: &str| x.chars().all(|c| c.is_ascii_digit());
    if !digits(int) || !digits(frac) || int.len() > 9 || frac.len() > 2 {
        return None;
    }
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if t.contains([',', '.']) && frac.is_empty() {
        return None; // "12," - an unfinished price
    }
    let whole: i64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let cents: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse().ok()?,
    };
    let v = whole * 100 + cents;
    (v > 0).then_some(v)
}

fn fmt_plain(cents: i64) -> String {
    format!("{},{:02}", cents / 100, cents % 100)
}

/// Market block of the selected item: orders, price chart, selling.
pub fn market_block(ui: &mut egui::Ui, mk: &mut MarketUi, st: &crate::invent::InvState, appid: u32, ctx: u32, it: &aurelia::steam_client::InventoryItem) {
    let key = (appid, it.market_hash_name.clone());
    if st.wallet.is_some() && mk.asked.insert(key.clone()) {
        req(MarketCmd::Spread { appid, hash: key.1.clone() });
        req(MarketCmd::History { appid, hash: key.1.clone() });
    }
    let cur = crate::invent::wallet_currency(st);
    let m = |c: i64| crate::invent::money(c, cur);
    ui.separator();
    let spread = st.trade.spread.get(&key);
    match spread {
        None => {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak(tr!("Узнаю заявки...", "Checking buy orders..."));
            });
        }
        Some(Err(e)) => {
            ui.weak(trf!("Заявки: {e}", "Buy orders: {e}"));
            if ui.small_button(tr!("Еще раз", "Again")).clicked() {
                mk.asked.remove(&key);
            }
        }
        Some(Ok(sp)) => {
            egui::Grid::new("spread").num_columns(2).show(ui, |ui| {
                ui.weak(tr!("Купят сразу за", "Instant buyers pay"));
                ui.label(if sp.highest_buy > 0 { trf!("{} ({} заявок)", "{} ({} orders)", m(sp.highest_buy), sp.buy_count) } else { tr!("нет заявок", "no orders").into() });
                ui.end_row();
                ui.weak(tr!("Дешевле всех лот", "Cheapest listing"));
                ui.label(if sp.lowest_sell > 0 { trf!("{} ({} лотов)", "{} ({} listings)", m(sp.lowest_sell), sp.sell_count) } else { tr!("нет лотов", "no listings").into() });
                ui.end_row();
            });
        }
    }
    if let Some(Ok(h)) = st.trade.history.get(&key) {
        price_chart(ui, h, cur);
    }
    if it.marketable && !crate::exp::on() {
        ui.add_space(4.0);
        crate::exp::note(ui, tr!("Продажа на площадке", "Selling on the market"));
    } else if it.marketable {
        let max_amount = it.amount.max(1);
        let base = |mode: u8, pay: i64| SellDlg {
            appid,
            ctx,
            assetid: it.asset_id.clone(),
            name: it.name.clone(),
            hash: it.market_hash_name.clone(),
            max_amount,
            amount: 1,
            pay,
            pay_text: fmt_plain(pay),
            mode,
        };
        ui.add_space(4.0);
        // Price rules of own currency (minimum, step, fees) come with the wallet. Without them prices
        // cannot be computed honestly - selling is not offered.
        let Some(r) = rules(st, appid) else {
            ui.weak(tr!("Продажа недоступна: Steam не прислал правила цен для вашей валюты. Обновите кошелек (вкладка «Мои лоты») или продайте на сайте Steam.", "Selling is unavailable: Steam did not send the price rules for your currency. Refresh the wallet (\"My listings\" tab) or sell on the Steam website."));
            return_with_msg(ui, mk, st, &key);
            return;
        };
        if let Some(Ok(sp)) = spread {
            // Quick: cheaper than the lowest listing by the given amount (one currency price step by
            // default), but not below the best order and the minimum price. If the lowest listing is already
            // at the minimum price - quick sell at that same price (Steam does not allow cheaper).
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            if sp.lowest_sell > 0 {
                let cut = undercut_amount(mk.undercut, &r);
                let want = (sp.lowest_sell - cut).max(sp.highest_buy).max(min_total);
                let pay = effective(want, &r);
                let hint = if pay >= sp.lowest_sell {
                    tr!("Самый дешевый лот уже по минимальной цене (или вровень с лучшей заявкой): дешевле выставить нельзя, лот будет по той же цене", "The cheapest listing is already at the minimum price (or level with the best buy order): you cannot list cheaper, the listing will be at the same price").to_string()
                } else {
                    trf!("Лот на {} дешевле самого дешевого на площадке: обычно продается быстро. Шаг меняется в «Настройки -> Аккаунт».", "A listing {} cheaper than the cheapest one on the market: usually sells fast. The step is changed in \"Settings -> Account\".", m(cut))
                };
                if ui
                    .add(egui::Button::new(trf!("Быстрая продажа за {}", "Quick sale for {}", m(pay))).fill(egui::Color32::from_rgb(60, 90, 40)))
                    .on_hover_text(hint)
                    .clicked()
                {
                    mk.sell = Some(base(1, pay));
                }
            }
            // Instant: at the highest buy order - sells right away.
            if sp.highest_buy >= min_total {
                let pay = effective(sp.highest_buy, &r);
                if ui
                    .add(egui::Button::new(trf!("Мгновенная продажа за {}", "Instant sale for {}", m(pay))).fill(egui::Color32::from_rgb(110, 80, 30)))
                    .on_hover_text(tr!("По самой высокой заявке на покупку: предмет продается сразу", "At the highest buy order: the item sells at once"))
                    .clicked()
                {
                    mk.sell = Some(base(2, pay));
                }
            }
        }
        if ui.button(tr!("Продать по своей цене...", "Sell at your own price...")).clicked() {
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            let pay = spread.and_then(|x| x.as_ref().ok()).map(|sp| sp.lowest_sell.max(min_total)).unwrap_or(min_total.max(100));
            mk.sell = Some(base(0, effective(pay, &r)));
        }
    }
    return_with_msg(ui, mk, st, &key);
}

/// Price rules for a game's item: minimum and step - from the wallet, the game fee - its own if the game
/// has one (market_pubfee_rate), otherwise the common one from the wallet. None - the wallet is not loaded yet or
/// Steam did not send the rules.
fn rules(st: &crate::invent::InvState, appid: u32) -> Option<crate::trade::FeeRules> {
    let mut r = st.wallet.as_ref()?.as_ref().ok()?.fee_rules?;
    let app_fee = st.apps.as_ref().and_then(|a| a.as_ref().ok()).and_then(|apps| apps.iter().find(|a| a.appid == appid)).and_then(|a| a.pubfee);
    if let Some(f) = app_fee.filter(|f| (0.0..1.0).contains(f)) {
        r.publisher_fee_percent = f;
    }
    Some(r)
}

/// How much cheaper quick sell is: the given amount rounded up to the currency price step
/// (Steam does not allow less than a step), 0 - one step.
pub fn undercut_amount(setting: i64, r: &crate::trade::FeeRules) -> i64 {
    let step = r.currency_increment.max(1);
    if setting <= 0 {
        return step;
    }
    ((setting + step - 1) / step) * step
}

/// The buyer price Steam will actually set if the buyer is ready to pay
/// `total`: Steam first converts it to "you receive", then back with fees and the price step.
fn effective(total: i64, r: &crate::trade::FeeRules) -> i64 {
    crate::trade::pay_for(crate::trade::receive_for(total, r), r)
}

/// Block tail: a message about the last action and the sell window.
fn return_with_msg(ui: &mut egui::Ui, mk: &mut MarketUi, st: &crate::invent::InvState, key: &(u32, String)) {
    if let Some(msg) = &st.trade.msg {
        // The sale result - only on the item that was listed, and for 90 seconds (after that, for example,
        // "confirm on your phone" is no longer relevant - the sale may have been cancelled).
        let fresh = st.trade.msg_item.as_ref() == Some(key)
            && st.trade.msg_at.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(90));
        if fresh {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(240, 200, 80)));
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(5));
        }
    }
    sell_window(ui.ctx(), mk, st);
}

/// Sale confirmation window: buyer price, what you receive, quantity.
fn sell_window(ctx: &egui::Context, mk: &mut MarketUi, st: &crate::invent::InvState) {
    let Some(mut d) = mk.sell.take() else { return };
    let Some(r) = rules(st, d.appid) else { return };
    let cur = crate::invent::wallet_currency(st);
    let m = |c: i64| crate::invent::money(c, cur);
    let mut keep = true;
    // Escape - cancel (no request is sent).
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return;
    }
    let title = match d.mode {
        1 => tr!("Быстрая продажа", "Quick sale"),
        2 => tr!("Мгновенная продажа", "Instant sale"),
        _ => tr!("Продажа на площадке", "Selling on the market"),
    };
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&d.name).strong());
            ui.horizontal(|ui| {
                ui.label(tr!("Покупатель заплатит:", "The buyer pays:"));
                if ui.add(egui::TextEdit::singleline(&mut d.pay_text).desired_width(80.0)).changed() {
                    d.mode = 0; // own price: no longer "quick" or "instant"
                }
            });
            // The price is taken every frame from what is in the field now: on an error the previous
            // number does not stay, and selling is disabled. Then exactly what Steam will do:
            // "you receive" by the currency rules and the buyer price after rounding.
            let parsed = parse_money(&d.pay_text);
            if let Some(p) = parsed {
                d.pay = p;
            }
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            let receive = parsed.filter(|&p| p >= min_total).map(|p| crate::trade::receive_for(p, &r)).unwrap_or(0);
            let real_pay = if receive > 0 { crate::trade::pay_for(receive, &r) } else { 0 };
            match parsed {
                Some(p) if p < min_total => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), trf!("Слишком мало: в вашей валюте цена для покупателя не меньше {}.", "Too low: in your currency the buyer price is at least {}.", m(min_total)));
                }
                Some(p) => {
                    ui.label(trf!("Вы получите: {} (комиссия Steam и игры: {})", "You receive: {} (Steam and game fees: {})", m(receive), m(real_pay - receive)));
                    if real_pay != p {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 170, 60),
                            trf!("Steam округлит цену по правилам вашей валюты: покупатель заплатит {}.", "Steam rounds the price by the rules of your currency: the buyer pays {}.", m(real_pay)),
                        );
                    }
                }
                None => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), tr!("Неверная цена: только цифры и до двух знаков после запятой, например 12,50.", "Wrong price: digits only and up to two decimals, for example 12.50."));
                }
            }
            if d.max_amount > 1 {
                ui.horizontal(|ui| {
                    ui.label(tr!("Сколько штук:", "Quantity:"));
                    ui.add(egui::DragValue::new(&mut d.amount).range(1..=d.max_amount));
                });
            }
            if d.mode == 2 {
                ui.weak(tr!("Цена равна самой высокой заявке: предмет могут купить сразу.", "The price equals the highest buy order: the item may be bought at once."));
            }
            ui.weak(
                tr!("Нажатие «Выставить» отправляет лот в Steam. Если Steam попросит подтверждение, подтвердите \
                 продажу в мобильном приложении; иначе лот появится сразу. Деньги придут в кошелек Steam.", "Pressing \"List\" sends the listing to Steam. If Steam asks for confirmation, confirm the sale in the mobile app; otherwise the listing appears right away. The money goes to the Steam wallet."),
            );
            ui.horizontal(|ui| {
                let ok = receive >= r.market_minimum && !st.trade.busy;
                let text = if receive > 0 { trf!("Выставить за {}", "List for {}", m(real_pay)) } else { tr!("Выставить", "List").to_string() };
                if ui.add_enabled(ok, egui::Button::new(text)).clicked() {
                    req(MarketCmd::Sell {
                        appid: d.appid,
                        ctx: d.ctx,
                        assetid: d.assetid.clone(),
                        amount: d.amount,
                        receive,
                        name: d.name.clone(),
                        hash: d.hash.clone(),
                    });
                    keep = false;
                }
                if ui.button(tr!("Отмена", "Cancel")).clicked() {
                    keep = false;
                }
            });
            ui.weak(tr!("Esc - отмена.", "Esc - cancel."));
        });
    if keep {
        mk.sell = Some(d);
    }
}

/// Price line for the last 30 days, with a tooltip on hover.
fn price_chart(ui: &mut egui::Ui, h: &[(i64, f64, u64)], cur: u32) {
    let now = chrono::Utc::now().timestamp();
    let pts: Vec<&(i64, f64, u64)> = h.iter().filter(|p| p.0 > now - 30 * 86400).collect();
    if pts.len() < 2 {
        return;
    }
    let (min, max) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let span = (max - min).max(0.01);
    let (t0, t1) = (pts[0].0, pts[pts.len() - 1].0);
    ui.weak(tr!("Цена за 30 дней", "Price over 30 days"));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(240.0), 80.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, egui::Color32::from_rgb(20, 26, 34));
    let to = |t: i64, v: f64| {
        egui::pos2(
            rect.left() + 4.0 + (t - t0) as f32 / (t1 - t0).max(1) as f32 * (rect.width() - 8.0),
            rect.bottom() - 4.0 - ((v - min) / span) as f32 * (rect.height() - 8.0),
        )
    };
    let line: Vec<egui::Pos2> = pts.iter().map(|q| to(q.0, q.1)).collect();
    p.add(egui::Shape::line(line, egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(102, 192, 244))));
    let fmt = |v: f64| crate::invent::money((v * 100.0).round() as i64, cur);
    p.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, fmt(max), egui::FontId::proportional(10.0), crate::theme::WEAK);
    p.text(rect.left_bottom() + egui::vec2(4.0, -2.0), egui::Align2::LEFT_BOTTOM, fmt(min), egui::FontId::proportional(10.0), crate::theme::WEAK);
    if let Some(pos) = resp.hover_pos() {
        let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let t = t0 + ((t1 - t0) as f32 * frac) as i64;
        if let Some(q) = pts.iter().min_by_key(|q| (q.0 - t).abs()) {
            use chrono::TimeZone;
            let d = chrono::Local.timestamp_opt(q.0, 0).single().map(|d| d.format("%d.%m %H:00").to_string()).unwrap_or_default();
            resp.on_hover_text(trf!("{d}: {} (продано {})", "{d}: {} ({} sold)", fmt(q.1), q.2));
        }
    }
}

/// "Remove listing" on own listing (a second click confirms).
pub fn remove_lot_button(ui: &mut egui::Ui, mk: &mut MarketUi, id: &str, name: &str) {
    if id.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        if mk.confirm.as_deref() == Some(id) {
            ui.label(tr!("Снять с продажи? Предмет вернется в инвентарь.", "Remove from sale? The item goes back to the inventory."));
            if ui.button(tr!("Да, снять", "Yes, remove")).clicked() {
                req(MarketCmd::RemoveListing { id: id.to_string(), name: name.to_string() });
                mk.confirm = None;
            }
            if ui.button(tr!("Нет", "No")).clicked() {
                mk.confirm = None;
            }
        } else if crate::exp::small_button(ui, tr!("Снять с продажи", "Remove from sale")).clicked() {
            mk.confirm = Some(id.to_string());
        }
    });
}

/// "Cancel order" on own buy order.
pub fn cancel_order_button(ui: &mut egui::Ui, mk: &mut MarketUi, id: &str, name: &str) {
    if id.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        if mk.confirm.as_deref() == Some(id) {
            ui.label(tr!("Отменить заявку? Деньги вернутся в кошелек.", "Cancel the buy order? The money goes back to the wallet."));
            if ui.button(tr!("Да, отменить", "Yes, cancel")).clicked() {
                req(MarketCmd::CancelOrder { id: id.to_string(), name: name.to_string() });
                mk.confirm = None;
            }
            if ui.button(tr!("Нет", "No")).clicked() {
                mk.confirm = None;
            }
        } else if crate::exp::small_button(ui, tr!("Отменить заявку", "Cancel request")).clicked() {
            mk.confirm = Some(id.to_string());
        }
    });
}

fn offer_items(ui: &mut egui::Ui, title: &str, items: &[OfferItem]) {
    ui.weak(format!("{title} ({})", items.len()));
    if items.is_empty() {
        ui.weak(tr!("ничего", "nothing"));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for it in items.iter().take(40) {
            let r = match &it.icon {
                Some(u) => ui.add(egui::Image::new(u.as_str()).fit_to_exact_size(egui::vec2(48.0, 48.0))),
                None => ui.label("?"),
            };
            r.on_hover_text(if it.amount > 1 { format!("{} x{}", it.name, it.amount) } else { it.name.clone() });
        }
    });
}

/// "Trades" tab: active incoming and own offers.
pub fn offers_tab(ui: &mut egui::Ui, st: &crate::invent::InvState, mk: &mut MarketUi) {
    if !mk.offers_asked {
        mk.offers_asked = true;
        req(MarketCmd::Offers);
    }
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button(tr!("Обновить", "Refresh")).clicked() {
                req(MarketCmd::Offers);
            }
            if ui.button(tr!("Обмены в браузере", "Trades in the browser")).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://steamcommunity.com/my/tradeoffers/"));
            }
        });
        if let Some(msg) = &st.trade.msg {
            ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(240, 200, 80)));
        }
        ui.add_space(4.0);
        let offers = match &st.trade.offers {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить обмены: {e}", "Could not load trades: {e}"));
                return;
            }
            Some(Ok(o)) => o,
        };
        if offers.is_empty() {
            ui.weak(tr!("Активных обменов нет. Предложить обмен другу пока можно на сайте Steam.", "No active trades. For now you can offer a friend a trade on the Steam website."));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for o in offers {
                egui::Frame::none().fill(crate::theme::CARD).rounding(4.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.strong(if o.incoming { tr!("Вам предлагают обмен", "You are offered a trade") } else { tr!("Ваше предложение", "Your offer") });
                        ui.hyperlink_to(tr!("профиль участника", "partner's profile"), format!("https://steamcommunity.com/profiles/{}", o.partner));
                        match o.state {
                            9 => {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), tr!("ждет подтверждения в мобильном приложении", "waiting for confirmation in the mobile app"));
                            }
                            11 => {
                                ui.weak(tr!("удержание Steam", "Steam hold"));
                            }
                            _ => {}
                        }
                    });
                    if !o.message.is_empty() {
                        ui.label(format!("«{}»", o.message));
                    }
                    offer_items(ui, tr!("Вы отдаете", "You give"), &o.give);
                    offer_items(ui, tr!("Вы получаете", "You get"), &o.get);
                    ui.horizontal(|ui| match mk.offer_confirm.clone() {
                        Some((id, act)) if id == o.id => {
                            ui.label(match act {
                                0 => tr!("Принять обмен? Предметы перейдут после подтверждения в мобильном приложении.", "Accept the trade? The items move after confirmation in the mobile app."),
                                1 => tr!("Отклонить обмен?", "Decline the trade?"),
                                _ => tr!("Отменить свое предложение?", "Cancel your offer?"),
                            });
                            if ui.button(tr!("Да", "Yes")).clicked() {
                                req(MarketCmd::Offer { id: o.id.clone(), partner: o.partner, act });
                                mk.offer_confirm = None;
                            }
                            if ui.button(tr!("Нет", "No")).clicked() {
                                mk.offer_confirm = None;
                            }
                        }
                        _ => {
                            if !crate::exp::on() {
                                ui.weak(tr!("Принять или отклонить обмен: экспериментальная функция (Настройки -> Аккаунт).", "Accepting or declining a trade is an experimental feature (Settings -> Account)."));
                            } else if o.incoming {
                                if o.state == 2 && ui.button(tr!("Принять...", "Accept...")).clicked() {
                                    mk.offer_confirm = Some((o.id.clone(), 0));
                                }
                                if ui.button(tr!("Отклонить...", "Decline...")).clicked() {
                                    mk.offer_confirm = Some((o.id.clone(), 1));
                                }
                            } else if ui.button(tr!("Отменить...", "Cancel...")).clicked() {
                                mk.offer_confirm = Some((o.id.clone(), 2));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::parse_money;

    #[test]
    fn strict_price() {
        assert_eq!(parse_money("12"), Some(1200));
        assert_eq!(parse_money(" 12,5 "), Some(1250));
        assert_eq!(parse_money("0.99"), Some(99));
        assert_eq!(parse_money(",5"), Some(50));
        for bad in ["", "0", "0,00", "1.2.3", "12,345", "12,", "10р", "-5", "1 000", "abc", "1e3"] {
            assert_eq!(parse_money(bad), None, "{bad}");
        }
    }
}
