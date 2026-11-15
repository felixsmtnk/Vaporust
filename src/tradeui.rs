//! Интерфейс площадки в инвентаре: заявки и график цены у предмета, продажа (своя цена,
//! быстрая, мгновенная), «Снять с продажи», «Отменить заявку», вкладка «Обмены».

use crate::trade::{MarketCmd, OfferItem};
use eframe::egui;

/// Просьбы интерфейса инвентаря к сети (отправляет LauncherApp::drain_ui_req).
pub static MARKET_REQ: std::sync::Mutex<Vec<MarketCmd>> = std::sync::Mutex::new(Vec::new());

fn req(c: MarketCmd) {
    MARKET_REQ.lock().unwrap().push(c);
}

/// Окно продажи: что, сколько, цена для покупателя, откуда пришла цена.
#[derive(Clone)]
pub struct SellDlg {
    pub appid: u32,
    pub ctx: u32,
    pub assetid: String,
    pub name: String,
    /// market_hash_name предмета (к нему привязывается итог продажи).
    pub hash: String,
    pub max_amount: u64,
    pub amount: u64,
    /// Цена для покупателя, минимальные единицы.
    pub pay: i64,
    pub pay_text: String,
    /// 0 - своя цена, 1 - быстрая, 2 - мгновенная.
    pub mode: u8,
}

#[derive(Default)]
pub struct MarketUi {
    pub asked: std::collections::HashSet<(u32, String)>,
    pub sell: Option<SellDlg>,
    /// Ждут второго нажатия: снять лот / отменить заявку (номер).
    pub confirm: Option<String>,
    pub offers_asked: bool,
    /// Обмен: (номер, действие 0 принять / 1 отклонить / 2 отменить) - ждет подтверждения.
    pub offer_confirm: Option<(String, u8)>,
    /// На сколько быстрая продажа дешевле самого дешевого лота (минимальные единицы; 0 - на один
    /// шаг цены валюты). Берется из настроек каждый кадр.
    pub undercut: i64,
}

/// Цена из поля ввода, в копейках/центах. Строго: только цифры и не больше одного
/// разделителя («,» или «.») с 1-2 знаками после него, например «12», «12,5», «0.99».
/// Любой другой текст («1.2.3», «12,345», «10р», «-5») - None, а не догадка.
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
        return None; // «12,» - недописанная цена
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

/// Блок площадки у выбранного предмета: заявки, график цены, продажа.
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
                ui.weak("Узнаю заявки...");
            });
        }
        Some(Err(e)) => {
            ui.weak(format!("Заявки: {e}"));
            if ui.small_button("Еще раз").clicked() {
                mk.asked.remove(&key);
            }
        }
        Some(Ok(sp)) => {
            egui::Grid::new("spread").num_columns(2).show(ui, |ui| {
                ui.weak("Купят сразу за");
                ui.label(if sp.highest_buy > 0 { format!("{} ({} заявок)", m(sp.highest_buy), sp.buy_count) } else { "нет заявок".into() });
                ui.end_row();
                ui.weak("Дешевле всех лот");
                ui.label(if sp.lowest_sell > 0 { format!("{} ({} лотов)", m(sp.lowest_sell), sp.sell_count) } else { "нет лотов".into() });
                ui.end_row();
            });
        }
    }
    if let Some(Ok(h)) = st.trade.history.get(&key) {
        price_chart(ui, h, cur);
    }
    if it.marketable {
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
        // Правила цен своей валюты (минимум, шаг, комиссии) приходят с кошельком. Без них цены не
        // посчитать честно - продажу не предлагаем.
        let Some(r) = rules(st, appid) else {
            ui.weak("Продажа недоступна: Steam не прислал правила цен для вашей валюты. Обновите кошелек (вкладка «Мои лоты») или продайте на сайте Steam.");
            return_with_msg(ui, mk, st, &key);
            return;
        };
        if let Some(Ok(sp)) = spread {
            // Быстрая: дешевле самого дешевого лота на заданную сумму (по умолчанию - на один шаг цены
            // валюты), но не ниже лучшей заявки и минимальной цены. Если самый дешевый лот уже по
            // минимальной цене - быстрая продажа по ней же (дешевле Steam не разрешит).
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            if sp.lowest_sell > 0 {
                let cut = undercut_amount(mk.undercut, &r);
                let want = (sp.lowest_sell - cut).max(sp.highest_buy).max(min_total);
                let pay = effective(want, &r);
                let hint = if pay >= sp.lowest_sell {
                    "Самый дешевый лот уже по минимальной цене (или вровень с лучшей заявкой): дешевле выставить нельзя, лот будет по той же цене".to_string()
                } else {
                    format!("Лот на {} дешевле самого дешевого на площадке: обычно продается быстро. Шаг меняется в «Настройки -> Аккаунт».", m(cut))
                };
                if ui
                    .add(egui::Button::new(format!("Быстрая продажа за {}", m(pay))).fill(egui::Color32::from_rgb(60, 90, 40)))
                    .on_hover_text(hint)
                    .clicked()
                {
                    mk.sell = Some(base(1, pay));
                }
            }
            // Мгновенная: по самой высокой заявке - продается сразу.
            if sp.highest_buy >= min_total {
                let pay = effective(sp.highest_buy, &r);
                if ui
                    .add(egui::Button::new(format!("Мгновенная продажа за {}", m(pay))).fill(egui::Color32::from_rgb(110, 80, 30)))
                    .on_hover_text("По самой высокой заявке на покупку: предмет продается сразу")
                    .clicked()
                {
                    mk.sell = Some(base(2, pay));
                }
            }
        }
        if ui.button("Продать по своей цене...").clicked() {
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            let pay = spread.and_then(|x| x.as_ref().ok()).map(|sp| sp.lowest_sell.max(min_total)).unwrap_or(min_total.max(100));
            mk.sell = Some(base(0, effective(pay, &r)));
        }
    }
    return_with_msg(ui, mk, st, &key);
}

/// Правила цен для предмета игры: минимум и шаг - из кошелька, комиссия игры - своя, если у игры
/// она есть (market_pubfee_rate), иначе общая из кошелька. None - кошелек еще не загружен или
/// Steam не прислал правила.
fn rules(st: &crate::invent::InvState, appid: u32) -> Option<crate::trade::FeeRules> {
    let mut r = st.wallet.as_ref()?.as_ref().ok()?.fee_rules?;
    let app_fee = st.apps.as_ref().and_then(|a| a.as_ref().ok()).and_then(|apps| apps.iter().find(|a| a.appid == appid)).and_then(|a| a.pubfee);
    if let Some(f) = app_fee.filter(|f| (0.0..1.0).contains(f)) {
        r.publisher_fee_percent = f;
    }
    Some(r)
}

/// На сколько дешевле быстрая продажа: заданная сумма, округленная вверх до шага цены валюты
/// (меньше шага Steam не разрешит), 0 - один шаг.
pub fn undercut_amount(setting: i64, r: &crate::trade::FeeRules) -> i64 {
    let step = r.currency_increment.max(1);
    if setting <= 0 {
        return step;
    }
    ((setting + step - 1) / step) * step
}

/// Цена для покупателя, которую Steam выставит на самом деле, если покупатель готов заплатить
/// `total`: Steam сначала переводит ее в «вы получите», а потом обратно с комиссиями и шагом цены.
fn effective(total: i64, r: &crate::trade::FeeRules) -> i64 {
    crate::trade::pay_for(crate::trade::receive_for(total, r), r)
}

/// Хвост блока: сообщение о последнем действии и окно продажи.
fn return_with_msg(ui: &mut egui::Ui, mk: &mut MarketUi, st: &crate::invent::InvState, key: &(u32, String)) {
    if let Some(msg) = &st.trade.msg {
        // Итог продажи - только у того предмета, который выставляли, и 90 секунд (потом, например,
        // «подтвердите в телефоне» уже неактуально - продажу могли отменить).
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

/// Окно подтверждения продажи: цена для покупателя, что получите вы, количество.
fn sell_window(ctx: &egui::Context, mk: &mut MarketUi, st: &crate::invent::InvState) {
    let Some(mut d) = mk.sell.take() else { return };
    let Some(r) = rules(st, d.appid) else { return };
    let cur = crate::invent::wallet_currency(st);
    let m = |c: i64| crate::invent::money(c, cur);
    let mut keep = true;
    // Escape - отмена (запрос не отправляется).
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return;
    }
    let title = match d.mode {
        1 => "Быстрая продажа",
        2 => "Мгновенная продажа",
        _ => "Продажа на площадке",
    };
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&d.name).strong());
            ui.horizontal(|ui| {
                ui.label("Покупатель заплатит:");
                if ui.add(egui::TextEdit::singleline(&mut d.pay_text).desired_width(80.0)).changed() {
                    d.mode = 0; // своя цена: уже не «быстрая» и не «мгновенная»
                }
            });
            // Цена каждый кадр берется из того, что написано в поле сейчас: при ошибке не
            // остается прежнее число, а продажа выключена. Дальше - ровно то, что сделает Steam:
            // «вы получите» по правилам валюты и цена для покупателя после округления.
            let parsed = parse_money(&d.pay_text);
            if let Some(p) = parsed {
                d.pay = p;
            }
            let min_total = crate::trade::pay_for(r.market_minimum, &r);
            let receive = parsed.filter(|&p| p >= min_total).map(|p| crate::trade::receive_for(p, &r)).unwrap_or(0);
            let real_pay = if receive > 0 { crate::trade::pay_for(receive, &r) } else { 0 };
            match parsed {
                Some(p) if p < min_total => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), format!("Слишком мало: в вашей валюте цена для покупателя не меньше {}.", m(min_total)));
                }
                Some(p) => {
                    ui.label(format!("Вы получите: {} (комиссия Steam и игры: {})", m(receive), m(real_pay - receive)));
                    if real_pay != p {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 170, 60),
                            format!("Steam округлит цену по правилам вашей валюты: покупатель заплатит {}.", m(real_pay)),
                        );
                    }
                }
                None => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), "Неверная цена: только цифры и до двух знаков после запятой, например 12,50.");
                }
            }
            if d.max_amount > 1 {
                ui.horizontal(|ui| {
                    ui.label("Сколько штук:");
                    ui.add(egui::DragValue::new(&mut d.amount).range(1..=d.max_amount));
                });
            }
            if d.mode == 2 {
                ui.weak("Цена равна самой высокой заявке: предмет могут купить сразу.");
            }
            ui.weak(
                "Нажатие «Выставить» отправляет лот в Steam. Если Steam попросит подтверждение, подтвердите \
                 продажу в мобильном приложении; иначе лот появится сразу. Деньги придут в кошелек Steam.",
            );
            ui.horizontal(|ui| {
                let ok = receive >= r.market_minimum && !st.trade.busy;
                let text = if receive > 0 { format!("Выставить за {}", m(real_pay)) } else { "Выставить".to_string() };
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
                if ui.button("Отмена").clicked() {
                    keep = false;
                }
            });
            ui.weak("Esc - отмена.");
        });
    if keep {
        mk.sell = Some(d);
    }
}

/// Линия цены за последние 30 дней, с подсказкой при наведении.
fn price_chart(ui: &mut egui::Ui, h: &[(i64, f64, u64)], cur: u32) {
    let now = chrono::Utc::now().timestamp();
    let pts: Vec<&(i64, f64, u64)> = h.iter().filter(|p| p.0 > now - 30 * 86400).collect();
    if pts.len() < 2 {
        return;
    }
    let (min, max) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let span = (max - min).max(0.01);
    let (t0, t1) = (pts[0].0, pts[pts.len() - 1].0);
    ui.weak("Цена за 30 дней");
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
    p.add(egui::Shape::line(line, egui::Stroke::new(1.5, egui::Color32::from_rgb(102, 192, 244))));
    let fmt = |v: f64| crate::invent::money((v * 100.0).round() as i64, cur);
    p.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, fmt(max), egui::FontId::proportional(10.0), crate::theme::WEAK);
    p.text(rect.left_bottom() + egui::vec2(4.0, -2.0), egui::Align2::LEFT_BOTTOM, fmt(min), egui::FontId::proportional(10.0), crate::theme::WEAK);
    if let Some(pos) = resp.hover_pos() {
        let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let t = t0 + ((t1 - t0) as f32 * frac) as i64;
        if let Some(q) = pts.iter().min_by_key(|q| (q.0 - t).abs()) {
            use chrono::TimeZone;
            let d = chrono::Local.timestamp_opt(q.0, 0).single().map(|d| d.format("%d.%m %H:00").to_string()).unwrap_or_default();
            resp.on_hover_text(format!("{d}: {} (продано {})", fmt(q.1), q.2));
        }
    }
}

/// «Снять с продажи» у своего лота (второе нажатие подтверждает).
pub fn remove_lot_button(ui: &mut egui::Ui, mk: &mut MarketUi, id: &str, name: &str) {
    if id.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        if mk.confirm.as_deref() == Some(id) {
            ui.label("Снять с продажи? Предмет вернется в инвентарь.");
            if ui.button("Да, снять").clicked() {
                req(MarketCmd::RemoveListing { id: id.to_string(), name: name.to_string() });
                mk.confirm = None;
            }
            if ui.button("Нет").clicked() {
                mk.confirm = None;
            }
        } else if ui.small_button("Снять с продажи").clicked() {
            mk.confirm = Some(id.to_string());
        }
    });
}

/// «Отменить заявку» у своей заявки на покупку.
pub fn cancel_order_button(ui: &mut egui::Ui, mk: &mut MarketUi, id: &str, name: &str) {
    if id.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        if mk.confirm.as_deref() == Some(id) {
            ui.label("Отменить заявку? Деньги вернутся в кошелек.");
            if ui.button("Да, отменить").clicked() {
                req(MarketCmd::CancelOrder { id: id.to_string(), name: name.to_string() });
                mk.confirm = None;
            }
            if ui.button("Нет").clicked() {
                mk.confirm = None;
            }
        } else if ui.small_button("Отменить заявку").clicked() {
            mk.confirm = Some(id.to_string());
        }
    });
}

fn offer_items(ui: &mut egui::Ui, title: &str, items: &[OfferItem]) {
    ui.weak(format!("{title} ({})", items.len()));
    if items.is_empty() {
        ui.weak("ничего");
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

/// Вкладка «Обмены»: активные входящие и свои предложения.
pub fn offers_tab(ui: &mut egui::Ui, st: &crate::invent::InvState, mk: &mut MarketUi) {
    if !mk.offers_asked {
        mk.offers_asked = true;
        req(MarketCmd::Offers);
    }
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Обновить").clicked() {
                req(MarketCmd::Offers);
            }
            if ui.button("Обмены в браузере").clicked() {
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
                ui.label(format!("Не удалось загрузить обмены: {e}"));
                return;
            }
            Some(Ok(o)) => o,
        };
        if offers.is_empty() {
            ui.weak("Активных обменов нет. Предложить обмен другу пока можно на сайте Steam.");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for o in offers {
                egui::Frame::none().fill(crate::theme::CARD).rounding(4.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.strong(if o.incoming { "Вам предлагают обмен" } else { "Ваше предложение" });
                        ui.hyperlink_to("профиль участника", format!("https://steamcommunity.com/profiles/{}", o.partner));
                        match o.state {
                            9 => {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), "ждет подтверждения в мобильном приложении");
                            }
                            11 => {
                                ui.weak("удержание Steam");
                            }
                            _ => {}
                        }
                    });
                    if !o.message.is_empty() {
                        ui.label(format!("«{}»", o.message));
                    }
                    offer_items(ui, "Вы отдаете", &o.give);
                    offer_items(ui, "Вы получаете", &o.get);
                    ui.horizontal(|ui| match mk.offer_confirm.clone() {
                        Some((id, act)) if id == o.id => {
                            ui.label(match act {
                                0 => "Принять обмен? Предметы перейдут после подтверждения в мобильном приложении.",
                                1 => "Отклонить обмен?",
                                _ => "Отменить свое предложение?",
                            });
                            if ui.button("Да").clicked() {
                                req(MarketCmd::Offer { id: o.id.clone(), partner: o.partner, act });
                                mk.offer_confirm = None;
                            }
                            if ui.button("Нет").clicked() {
                                mk.offer_confirm = None;
                            }
                        }
                        _ => {
                            if o.incoming {
                                if o.state == 2 && ui.button("Принять...").clicked() {
                                    mk.offer_confirm = Some((o.id.clone(), 0));
                                }
                                if ui.button("Отклонить...").clicked() {
                                    mk.offer_confirm = Some((o.id.clone(), 1));
                                }
                            } else if ui.button("Отменить...").clicked() {
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
