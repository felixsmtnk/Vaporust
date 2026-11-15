//! Торговая площадка и обмены (безопасная часть + продажа из инвентаря):
//! история цены и заявки на покупку предмета, продажа (обычная, быстрая и мгновенная - как в
//! Steam Inventory Helper), снятие своего лота, отмена своей заявки, обмены (принять, отклонить,
//! отменить). Продажу и обмены Steam просит подтвердить в мобильном приложении - Vaporust этого
//! не делает и не обходит.

use crate::invent::SharedInv;
use aurelia::steam_client::SteamClient;
use eframe::egui;
use serde_json::Value;

/// Заявки и лоты предмета (в минимальных единицах валюты кошелька).
#[derive(Clone, Debug, Default)]
pub struct Spread {
    /// Самая высокая заявка на покупку (за столько продастся сразу).
    pub highest_buy: i64,
    /// Самый дешевый лот.
    pub lowest_sell: i64,
    pub buy_count: u64,
    pub sell_count: u64,
}

/// Предмет обмена.
#[derive(Clone, Debug, Default)]
pub struct OfferItem {
    pub name: String,
    pub icon: Option<String>,
    pub amount: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Offer {
    pub id: String,
    /// SteamID64 второго участника.
    pub partner: u64,
    pub message: String,
    /// Мы отдаем / мы получаем.
    pub give: Vec<OfferItem>,
    pub get: Vec<OfferItem>,
    /// Входящее (нам прислали).
    pub incoming: bool,
    /// ETradeOfferState: 2 активно, 9 ждет мобильного подтверждения, 11 удержание.
    pub state: u32,
    pub expires: u64,
}

#[derive(Default)]
pub struct TradeState {
    /// История цены: (appid, hash) -> [(unix-время, цена, продано штук)].
    pub history: std::collections::HashMap<(u32, String), Result<Vec<(i64, f64, u64)>, String>>,
    pub spread: std::collections::HashMap<(u32, String), Result<Spread, String>>,
    /// Итог последней продажи / снятия / обмена.
    pub msg: Option<String>,
    pub busy: bool,
    pub offers: Option<Result<Vec<Offer>, String>>,
}

pub enum MarketCmd {
    History { appid: u32, hash: String },
    Spread { appid: u32, hash: String },
    /// Продать: сколько получите вы (минимальные единицы), штук.
    Sell { appid: u32, ctx: u32, assetid: String, amount: u64, receive: i64, name: String },
    RemoveListing { id: String, name: String },
    CancelOrder { id: String, name: String },
    Offers,
    /// 0 - принять, 1 - отклонить, 2 - отменить свое.
    Offer { id: String, partner: u64, act: u8 },
}

// ---------- Цены и комиссия, как на сайте Steam (economy_common.js) ----------
//
// У каждой валюты кошелька свои правила: минимальная цена (она же минимальная комиссия) и шаг
// цены. Steam берет их из g_rgWalletInfo (wallet_market_minimum, wallet_currency_increment,
// wallet_fee_percent, wallet_publisher_fee_percent_default). Функции ниже - перенос
// ToValidMarketPrice, CalculateFee, GetTotalWithFees и GetItemPriceFromTotal один в один.
// Раньше Vaporust считал как для доллара (минимум 0,01), и для гривны предпросмотр врал:
// «покупатель заплатит 1,02 ₴» вместо 3,00 ₴.

pub use aurelia::steam_client::MarketFeeRules as FeeRules;

/// Правила, когда кошелек их не прислал: как у доллара. Только для тестов и подсказок -
/// продажа без настоящих правил запрещена (см. tradeui).
pub fn default_rules() -> FeeRules {
    FeeRules { market_minimum: 1, currency_increment: 1, steam_fee_percent: 0.05, publisher_fee_percent: 0.10 }
}

/// ToValidMarketPrice: не меньше минимума и кратно шагу валюты.
pub fn to_valid(price: i64, r: &FeeRules) -> i64 {
    let floor = r.market_minimum;
    let inc = r.currency_increment;
    if price <= floor {
        return floor;
    }
    if price <= inc {
        return inc;
    }
    if inc > 1 {
        let d = price as f64 / inc as f64;
        let sign = if d < 0.0 { -1.0 } else { 1.0 };
        return ((sign * (d.abs() + 0.5).floor()) * inc as f64) as i64;
    }
    price
}

/// CalculateFee.
fn fee(base: i64, pct: f64, r: &FeeRules) -> i64 {
    if pct > 0.0 {
        to_valid((base as f64 * pct).floor() as i64, r)
    } else {
        0
    }
}

/// Сколько заплатит покупатель, если вы получите receive (GetTotalWithFees).
pub fn pay_for(receive: i64, r: &FeeRules) -> i64 {
    to_valid(receive, r) + fee(receive, r.publisher_fee_percent, r) + fee(receive, r.steam_fee_percent, r)
}

/// Сколько получите вы, если покупатель платит total (GetItemPriceFromTotal).
pub fn receive_for(total: i64, r: &FeeRules) -> i64 {
    let (ppct, spct) = (r.publisher_fee_percent, r.steam_fee_percent);
    let (inc, floor) = (r.currency_increment, r.market_minimum);
    let guess = (total as f64 / (1.0 + ppct + spct)).floor() as i64;
    let max_base = total - 2 * floor;
    let mut base = to_valid(guess.min(max_base), r);
    for _ in 0..3 {
        let calc = pay_for(base, r);
        if calc == total {
            return base;
        }
        if calc < total {
            base += inc;
        } else {
            base -= inc;
            break;
        }
    }
    base.max(floor)
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn num(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.replace(',', "").parse().ok())).unwrap_or(0)
}

/// «Nov 14 2026 01: +0» -> unix-время.
fn parse_hist_date(s: &str) -> Option<i64> {
    let s = s.split(':').next()?.trim(); // «Nov 14 2026 01»
    let dt = chrono::NaiveDateTime::parse_from_str(&format!("{s}:00:00"), "%b %d %Y %H:%M:%S").ok()?;
    Some(dt.and_utc().timestamp())
}

pub fn parse_history(body: &str) -> Result<Vec<(i64, f64, u64)>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Steam прислал непонятный ответ".to_string())?;
    if v["success"].as_bool() != Some(true) {
        return Err("истории цены нет".into());
    }
    Ok(v["prices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let t = parse_hist_date(p[0].as_str()?)?;
            let price = p[1].as_f64()?;
            let n = p[2].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            Some((t, price, n))
        })
        .collect())
}

/// Число после ключа в JSON страницы (кавычки бывают экранированы: \"key\":123).
fn page_num(t: &str, key: &str) -> Option<i64> {
    let i = t.find(key)? + key.len();
    let rest = &t[i..];
    let start = rest.find(|c: char| c.is_ascii_digit())?;
    if start > 4 {
        return None;
    }
    let digits: String = rest[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Заявки со страницы предмета на площадке (новая страница Steam: amtMaxBuyOrder, amtMinSellOrder...).
pub fn parse_spread_page(html: &str) -> Result<(Spread, u32), String> {
    // Заявок на покупку может не быть совсем (amtMaxBuyOrder: null) - это не «не продается».
    if !html.contains("amtMinSellOrder") && !html.contains("amtMaxBuyOrder") {
        return Err("предмет не продается на площадке".into());
    }
    let buy = page_num(html, "amtMaxBuyOrder").unwrap_or(0);
    Ok((
        Spread {
            highest_buy: buy,
            lowest_sell: page_num(html, "amtMinSellOrder").unwrap_or(0),
            buy_count: page_num(html, "cBuyOrders").unwrap_or(0) as u64,
            sell_count: page_num(html, "cSellOrders").unwrap_or(0) as u64,
        },
        page_num(html, "eCurrency").unwrap_or(1) as u32,
    ))
}

pub fn parse_spread(body: &str) -> Result<Spread, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Steam прислал непонятный ответ".to_string())?;
    if num(&v["success"]) != 1 {
        return Err("заявки не загрузились".into());
    }
    let count = |k: &str| {
        // buy_order_graph / sell_order_graph: [[цена, накоплено штук, подпись], ...] - последнее = всего.
        v[k].as_array().and_then(|a| a.last()).map(|x| num(&x[1]) as u64).unwrap_or(0)
    };
    Ok(Spread {
        highest_buy: num(&v["highest_buy_order"]),
        lowest_sell: num(&v["lowest_sell_order"]),
        buy_count: count("buy_order_graph"),
        sell_count: count("sell_order_graph"),
    })
}

fn wallet(st: &SharedInv) -> (u32, String) {
    match &st.lock().unwrap().wallet {
        Some(Ok(w)) if w.currency > 0 => (w.currency, w.country.clone()),
        _ => (1, "US".into()),
    }
}

pub fn handle(client: &SteamClient, cmd: MarketCmd, st: &SharedInv, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        let set_msg = |m: String| {
            let mut s = st.lock().unwrap();
            s.trade.msg = Some(m);
            s.trade.busy = false;
        };
        let web = match crate::webapi::web_session(&c).await {
            Ok(w) => w,
            Err(e) => {
                set_msg(format!("Нет доступа к сайту Steam: {e}"));
                ctx.request_repaint();
                return;
            }
        };
        match cmd {
            MarketCmd::History { appid, hash } => {
                let url = format!("https://steamcommunity.com/market/pricehistory/?appid={appid}&market_hash_name={}", url_encode(&hash));
                let r = match web.get_text(&url).await {
                    Ok(b) => parse_history(&b),
                    Err(e) => Err(format!("{e:#}")),
                };
                st.lock().unwrap().trade.history.insert((appid, hash), r);
            }
            MarketCmd::Spread { appid, hash } => {
                let key = (appid, hash.clone());
                let qp = serde_json::json!([appid, hash]).to_string();
                let url = format!("https://steamcommunity.com/market/orderbook?q=Load&qp={}", url_encode(&qp));
                let (cur, _) = wallet(&st);
                let r = match web.get_raw(&url, &[("x-valve-request-type", "queryAction")]).await {
                    Ok((429, _)) => Err("Steam просит подождать (частые запросы)".to_string()),
                    Ok((_, t)) => parse_spread_page(&t).and_then(|(sp, c)| {
                        if c == cur || cur == 1 {
                            Ok(sp)
                        } else {
                            Err("Steam показал цены в другой валюте, обновите позже".to_string())
                        }
                    }),
                    Err(e) => Err(format!("{e:#}")),
                };
                st.lock().unwrap().trade.spread.insert(key, r);
            }
            MarketCmd::Sell { appid, ctx: cx, assetid, amount, receive, name } => {
                let fields = [
                    ("sessionid", web.sessionid().to_string()),
                    ("appid", appid.to_string()),
                    ("contextid", cx.to_string()),
                    ("assetid", assetid.clone()),
                    ("amount", amount.max(1).to_string()),
                    ("price", receive.to_string()),
                ];
                let referer = format!("https://steamcommunity.com/profiles/{}/inventory/", web.steam_id());
                let m = match web.post_form("https://steamcommunity.com/market/sellitem/", &fields, &referer).await {
                    Ok((status, body)) => {
                        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        if v["success"].as_bool() == Some(true) {
                            let mobile = v["needs_mobile_confirmation"].as_bool() == Some(true) || num(&v["requires_confirmation"]) == 1;
                            if mobile {
                                format!("«{name}» выставлен. Подтвердите продажу в мобильном приложении Steam (Подтверждения).")
                            } else {
                                format!("«{name}» выставлен на продажу.")
                            }
                        } else {
                            let why = v["message"].as_str().map(String::from).unwrap_or_else(|| format!("ответ {status}"));
                            format!("Не выставлен: {why}")
                        }
                    }
                    Err(e) => format!("Не выставлен: {e:#}"),
                };
                set_msg(m);
                // Перечитать инвентарь этого раздела и свои лоты.
                st.lock().unwrap().items.remove(&(appid, cx));
                if let Ok(items) = aurelia::steam_client::inventory_via_lang(&web, appid, cx, "russian").await {
                    st.lock().unwrap().items.insert((appid, cx), Ok(items));
                }
                if let Ok(body) = web.get_text("https://steamcommunity.com/market/mylistings/?norender=1&count=100&l=russian").await {
                    st.lock().unwrap().listings = Some(crate::invent::parse_my_market(&body));
                }
            }
            MarketCmd::RemoveListing { id, name } => {
                let fields = [("sessionid", web.sessionid().to_string())];
                let r = web
                    .post_form(&format!("https://steamcommunity.com/market/removelisting/{id}"), &fields, "https://steamcommunity.com/market/")
                    .await;
                set_msg(match r {
                    Ok((s, _)) if s < 400 => format!("Лот «{name}» снят, предмет вернулся в инвентарь."),
                    Ok((s, _)) => format!("Не снят: ответ {s}"),
                    Err(e) => format!("Не снят: {e:#}"),
                });
                if let Ok(body) = web.get_text("https://steamcommunity.com/market/mylistings/?norender=1&count=100&l=russian").await {
                    st.lock().unwrap().listings = Some(crate::invent::parse_my_market(&body));
                }
            }
            MarketCmd::CancelOrder { id, name } => {
                let fields = [("sessionid", web.sessionid().to_string()), ("buy_orderid", id)];
                let r = web.post_form("https://steamcommunity.com/market/cancelbuyorder/", &fields, "https://steamcommunity.com/market/").await;
                set_msg(match r {
                    Ok((_, b)) if serde_json::from_str::<Value>(&b).map(|v| num(&v["success"]) == 1).unwrap_or(false) => {
                        format!("Заявка на «{name}» отменена, деньги вернулись в кошелек.")
                    }
                    Ok((s, _)) => format!("Не отменена: ответ {s}"),
                    Err(e) => format!("Не отменена: {e:#}"),
                });
                if let Ok(body) = web.get_text("https://steamcommunity.com/market/mylistings/?norender=1&count=100&l=russian").await {
                    st.lock().unwrap().listings = Some(crate::invent::parse_my_market(&body));
                }
            }
            MarketCmd::Offers => {
                let r = load_offers(&c).await;
                st.lock().unwrap().trade.offers = Some(r);
            }
            MarketCmd::Offer { id, partner, act } => {
                let r = match act {
                    0 => {
                        let fields = [
                            ("sessionid", web.sessionid().to_string()),
                            ("serverid", "1".to_string()),
                            ("tradeofferid", id.clone()),
                            ("partner", partner.to_string()),
                            ("captcha", String::new()),
                        ];
                        match web
                            .post_form(
                                &format!("https://steamcommunity.com/tradeoffer/{id}/accept"),
                                &fields,
                                &format!("https://steamcommunity.com/tradeoffer/{id}/"),
                            )
                            .await
                        {
                            Ok((s, b)) if s < 400 => {
                                let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
                                if v["needs_mobile_confirmation"].as_bool() == Some(true) {
                                    Ok("Обмен принят: подтвердите его в мобильном приложении Steam.".to_string())
                                } else {
                                    Ok("Обмен принят.".to_string())
                                }
                            }
                            Ok((s, b)) => Err(serde_json::from_str::<Value>(&b)
                                .ok()
                                .and_then(|v| v["strError"].as_str().map(String::from))
                                .unwrap_or_else(|| format!("ответ {s}"))),
                            Err(e) => Err(format!("{e:#}")),
                        }
                    }
                    1 => crate::webapi::post(&c, "IEconService/DeclineTradeOffer/v1", &[("tradeofferid", id.clone())])
                        .await
                        .map(|_| "Обмен отклонен.".to_string()),
                    _ => crate::webapi::post(&c, "IEconService/CancelTradeOffer/v1", &[("tradeofferid", id.clone())])
                        .await
                        .map(|_| "Предложение обмена отменено.".to_string()),
                };
                set_msg(r.unwrap_or_else(|e| format!("Не получилось: {e}")));
                let fresh = load_offers(&c).await;
                st.lock().unwrap().trade.offers = Some(fresh);
            }
        }
        st.lock().unwrap().trade.busy = false;
        ctx.request_repaint();
    });
}

/// Активные обмены (входящие и свои) с названиями и картинками предметов.
pub async fn load_offers(c: &SteamClient) -> Result<Vec<Offer>, String> {
    let v = crate::webapi::get(
        c,
        "IEconService/GetTradeOffers/v1",
        &[
            ("get_received_offers", "1".into()),
            ("get_sent_offers", "1".into()),
            ("active_only", "1".into()),
            ("get_descriptions", "1".into()),
            ("language", "russian".into()),
        ],
    )
    .await?;
    let mut desc: std::collections::HashMap<(String, String), (String, Option<String>)> = Default::default();
    for d in v["descriptions"].as_array().into_iter().flatten() {
        let key = (num(&d["classid"]).to_string(), num(&d["instanceid"]).to_string());
        let name = d["market_name"].as_str().or(d["name"].as_str()).unwrap_or("").to_string();
        let icon = d["icon_url"].as_str().filter(|s| !s.is_empty()).map(|s| format!("{}{s}/96fx96f", aurelia::web::web_session::ECON_IMAGE_BASE));
        desc.insert(key, (name, icon));
    }
    let items = |a: &Value| -> Vec<OfferItem> {
        a.as_array()
            .into_iter()
            .flatten()
            .map(|it| {
                let key = (num(&it["classid"]).to_string(), num(&it["instanceid"]).to_string());
                let (name, icon) = desc.get(&key).cloned().unwrap_or_else(|| ("предмет".into(), None));
                OfferItem { name, icon, amount: num(&it["amount"]).max(1) as u64 }
            })
            .collect()
    };
    let mut out = Vec::new();
    for (k, incoming) in [("trade_offers_received", true), ("trade_offers_sent", false)] {
        for o in v[k].as_array().into_iter().flatten() {
            let state = num(&o["trade_offer_state"]) as u32;
            if !matches!(state, 2 | 9 | 11) {
                continue;
            }
            out.push(Offer {
                id: num(&o["tradeofferid"]).to_string(),
                partner: num(&o["accountid_other"]) as u64 + crate::social::STEAMID64_BASE,
                message: o["message"].as_str().unwrap_or("").to_string(),
                give: items(&o["items_to_give"]),
                get: items(&o["items_to_receive"]),
                incoming,
                state,
                expires: num(&o["expiration_time"]) as u64,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_math() {
        let usd = default_rules();
        // Как на площадке: покупатель платит 1,15 - вы получаете 1,00.
        assert_eq!(pay_for(100, &usd), 115);
        assert_eq!(receive_for(115, &usd), 100);
        // Минимум комиссий - по одной минимальной единице.
        assert_eq!(pay_for(1, &usd), 3);
        assert_eq!(receive_for(3, &usd), 1);
        for base in 1..20000 {
            assert_eq!(receive_for(pay_for(base, &usd), &usd), base, "base {base}");
        }
        // Валюта с минимумом и шагом 100 (пример из сверки с кодом Steam): 100 + 100 + 100.
        let big = FeeRules { market_minimum: 100, currency_increment: 100, ..usd };
        assert_eq!(pay_for(100, &big), 300);
        assert_eq!(receive_for(300, &big), 100);
        for base in (100..20000).step_by(100) {
            assert_eq!(receive_for(pay_for(base, &big), &big), base, "base {base}");
        }
        // Минимум 100, шаг 1 (как у гривны на лотах пользователя: 3,00 / получите 1,00).
        let uah = FeeRules { market_minimum: 100, currency_increment: 1, ..usd };
        assert_eq!(pay_for(100, &uah), 300);
        assert_eq!(receive_for(300, &uah), 100);
        assert_eq!(to_valid(1234, &big), 1200);
        assert_eq!(to_valid(1250, &big), 1300);
        assert_eq!(to_valid(5, &uah), 100);
    }

    #[test]
    fn history_and_spread() {
        let h = parse_history(r#"{"success":true,"prices":[["Nov 13 2026 01: +0",1.234,"12"],["Nov 14 2026 01: +0",1.5,"3"]]}"#).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].2, 3);
        assert!(h[1].0 > h[0].0);
        let s = parse_spread(r#"{"success":1,"highest_buy_order":"120","lowest_sell_order":"150","buy_order_graph":[[1.2,5,""],[1.1,40,""]],"sell_order_graph":[[1.5,2,""]]}"#).unwrap();
        assert_eq!((s.highest_buy, s.lowest_sell, s.buy_count, s.sell_count), (120, 150, 40, 2));
    }

    /// Только чтение: история цены, заявки и обмены.
    #[test]
    #[ignore]
    fn market_read_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let hash = "Mann Co. Supply Crate Key";
            let h = web.get_text(&format!("https://steamcommunity.com/market/pricehistory/?appid=440&market_hash_name={}", url_encode(hash))).await.unwrap();
            let h = parse_history(&h).unwrap();
            println!("история: {} точек, последняя {:?}", h.len(), h.last());
            let qp = serde_json::json!([440, hash]).to_string();
            let (code, page) = web
                .get_raw(&format!("https://steamcommunity.com/market/orderbook?q=Load&qp={}", url_encode(&qp)), &[("x-valve-request-type", "queryAction")])
                .await
                .unwrap();
            println!("заявки ({code}): {:?}", parse_spread_page(&page));
            match load_offers(&c).await {
                Ok(o) => println!("обменов активных: {}", o.len()),
                Err(e) => println!("обмены: ошибка {e}"),
            }
        });
    }
}
pub use crate::tradeui::{cancel_order_button, market_block, offers_tab, remove_lot_button, MarketUi, MARKET_REQ};

#[cfg(test)]
mod rules_live {
    /// Живой тест (только чтение): правила цен кошелька пользователя. Печатает только правила.
    #[test]
    #[ignore]
    fn wallet_rules_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let w = aurelia::steam_client::wallet_via(&web).await.unwrap();
            println!("валюта {}: {:?}", w.currency, w.fee_rules);
            if let Some(r) = w.fee_rules {
                for receive in [1, 50, 100, 101, 1000] {
                    println!("получите {receive} -> покупатель {}", super::pay_for(receive, &r));
                }
            }
        });
    }
}

#[cfg(test)]
mod spread_live {
    /// Живой тест (только чтение): заявки по первым предметам CS2 из инвентаря - сырой ответ.
    #[test]
    #[ignore]
    fn cs2_spread_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let items = aurelia::steam_client::inventory_via_lang(&web, 730, 2, "english").await.unwrap();
            for it in items.iter().filter(|i| i.marketable).take(2) {
                println!("hash: {}", it.market_hash_name);
                let qp = serde_json::json!([730, it.market_hash_name]).to_string();
                let url = format!("https://steamcommunity.com/market/orderbook?q=Load&qp={}", super::url_encode(&qp));
                let (code, t) = web.get_raw(&url, &[("x-valve-request-type", "queryAction")]).await.unwrap();
                println!("{code}: {}", &t[..t.len().min(700)]);
            }
        });
    }
}

#[cfg(test)]
mod pubfee_live {
    /// Живой тест (только чтение): ставки комиссии игр из g_rgAppContextData инвентаря.
    #[test]
    #[ignore]
    fn pubfee_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let html = web.get_text(&format!("https://steamcommunity.com/profiles/{}/inventory/?l=english", web.steam_id())).await.unwrap();
            for part in html.split("\"appid\":").skip(1).take(14) {
                let id: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
                let fee = part.find("market_pubfee_rate").map(|i| &part[i..(i + 30).min(part.len())]).unwrap_or("-");
                println!("{id}: {fee}");
            }
        });
    }
}

#[cfg(test)]
mod spread_tests {
    /// Нет ни одной заявки на покупку (amtMaxBuyOrder: null) - это не «не продается».
    #[test]
    fn no_buy_orders() {
        let t = r#"{"data":{"success":true,"data":{"amtMaxBuyOrder":null,"amtMinSellOrder":300,"eCurrency":18,"cBuyOrders":0,"cSellOrders":30318}}}"#;
        let (sp, cur) = super::parse_spread_page(t).unwrap();
        assert_eq!((sp.highest_buy, sp.lowest_sell, sp.sell_count, cur), (0, 300, 30318, 18));
        assert!(super::parse_spread_page(r#"{"data":{"success":false}}"#).is_err());
    }

    #[test]
    fn undercut_steps() {
        let uah = super::FeeRules { market_minimum: 100, currency_increment: 100, ..super::default_rules() };
        let usd = super::default_rules();
        assert_eq!(crate::tradeui::undercut_amount(0, &uah), 100, "по умолчанию - один шаг");
        assert_eq!(crate::tradeui::undercut_amount(2, &uah), 100, "2 копейки у гривны - не меньше шага");
        assert_eq!(crate::tradeui::undercut_amount(150, &uah), 200, "вверх до шага");
        assert_eq!(crate::tradeui::undercut_amount(2, &usd), 2);
        assert_eq!(crate::tradeui::undercut_amount(0, &usd), 1);
    }
}
