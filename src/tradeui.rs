//! Интерфейс площадки в инвентаре: заявки и график цены у предмета, продажа (своя цена,
//! быстрая, мгновенная), «Снять с продажи», «Отменить заявку», вкладка «Обмены».

use crate::trade::{receive_for, MarketCmd, OfferItem};
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
}

/// «12,34» / «12.34» / «12» -> минимальные единицы.
fn parse_money(s: &str) -> Option<i64> {
    let t = s.trim().replace(',', ".");
    let t: String = t.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let v: f64 = t.parse().ok()?;
    (v > 0.0).then(|| (v * 100.0).round() as i64)
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
            max_amount,
            amount: 1,
            pay,
            pay_text: fmt_plain(pay),
            mode,
        };
        ui.add_space(4.0);
        if let Some(Ok(sp)) = spread {
            // Быстрая: на 0,01 дешевле самого дешевого лота (но не ниже лучшей заявки).
            if sp.lowest_sell > 3 {
                let pay = (sp.lowest_sell - 1).max(sp.highest_buy).max(3);
                if ui
                    .add(egui::Button::new(format!("Быстрая продажа за {}", m(pay))).fill(egui::Color32::from_rgb(60, 90, 40)))
                    .on_hover_text("Лот на 0,01 дешевле самого дешевого на площадке: обычно продается быстро")
                    .clicked()
                {
                    mk.sell = Some(base(1, pay));
                }
            }
            // Мгновенная: по самой высокой заявке - продается сразу.
            if sp.highest_buy > 3
                && ui
                    .add(egui::Button::new(format!("Мгновенная продажа за {}", m(sp.highest_buy))).fill(egui::Color32::from_rgb(110, 80, 30)))
                    .on_hover_text("По самой высокой заявке на покупку: предмет продается сразу")
                    .clicked()
            {
                mk.sell = Some(base(2, sp.highest_buy));
            }
        }
        if ui.button("Продать по своей цене...").clicked() {
            let pay = spread.and_then(|r| r.as_ref().ok()).map(|sp| sp.lowest_sell.max(3)).unwrap_or(100);
            mk.sell = Some(base(0, pay));
        }
    }
    if let Some(msg) = &st.trade.msg {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(240, 200, 80)));
    }
    sell_window(ui.ctx(), mk, st);
}

/// Окно подтверждения продажи: цена для покупателя, что получите вы, количество.
fn sell_window(ctx: &egui::Context, mk: &mut MarketUi, st: &crate::invent::InvState) {
    let Some(mut d) = mk.sell.take() else { return };
    let cur = crate::invent::wallet_currency(st);
    let m = |c: i64| crate::invent::money(c, cur);
    let mut keep = true;
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
                    if let Some(p) = parse_money(&d.pay_text) {
                        d.pay = p;
                        d.mode = 0;
                    }
                }
            });
            let receive = receive_for(d.pay);
            ui.label(format!("Вы получите: {} (комиссия Steam и игры: {})", m(receive), m(d.pay - receive)));
            if d.max_amount > 1 {
                ui.horizontal(|ui| {
                    ui.label("Сколько штук:");
                    ui.add(egui::DragValue::new(&mut d.amount).range(1..=d.max_amount));
                });
            }
            if d.mode == 2 {
                ui.weak("Цена равна самой высокой заявке: предмет купят сразу после подтверждения.");
            }
            ui.weak("После выставления Steam попросит подтвердить продажу в мобильном приложении. Деньги придут в кошелек Steam.");
            ui.horizontal(|ui| {
                let ok = receive >= 1 && !st.trade.busy;
                if ui.add_enabled(ok, egui::Button::new(format!("Выставить за {}", m(d.pay)))).clicked() {
                    req(MarketCmd::Sell {
                        appid: d.appid,
                        ctx: d.ctx,
                        assetid: d.assetid.clone(),
                        amount: d.amount,
                        receive,
                        name: d.name.clone(),
                    });
                    keep = false;
                }
                if ui.button("Отмена").clicked() {
                    keep = false;
                }
            });
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
