//! Инвентарь, кошелек и торговая площадка Steam (через веб-сессию Aurelia).
//! Только просмотр: продажа и покупка - на сайте Steam в браузере.

use aurelia::steam_client::{self as sc, InventoryItem, MarketPrice, SteamClient, WalletBalance};
use eframe::egui;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Игра, в которой у аккаунта есть предметы.
#[derive(Clone, Debug)]
pub struct InvApp {
    pub appid: u32,
    pub name: String,
    pub icon: Option<String>,
    pub count: u64,
    /// Разделы инвентаря игры: (id, название, предметов).
    pub contexts: Vec<(u32, String, u64)>,
}

#[derive(Default)]
pub struct InvState {
    pub apps: Option<Result<Vec<InvApp>, String>>,
    pub items: HashMap<(u32, u32), Result<Vec<InventoryItem>, String>>,
    pub loading: HashSet<(u32, u32)>,
    pub wallet: Option<Result<WalletBalance, String>>,
    pub listings: Option<Result<MyMarket, String>>,
    /// Цены предметов по (appid, market_hash_name).
    pub prices: HashMap<(u32, String), Result<MarketPrice, String>>,
    /// Поиск по площадке: запрос и (всего найдено, загруженные результаты).
    pub search: Option<(String, Result<(u32, Vec<MarketHit>), String>)>,
    pub searching: bool,
}

pub type SharedInv = Arc<Mutex<InvState>>;

pub enum InvCmd {
    /// Список игр с предметами + кошелек.
    Apps,
    Items { appid: u32, ctx: u32 },
    Wallet,
    Listings,
    Price { appid: u32, name: String },
    /// Поиск: запрос и с какого результата (Steam отдает по 10).
    Search { query: String, start: u32 },
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Steam: инвентарь «Steam» (карточки, фоны, смайлики) - это 753, раздел 6.
const STEAM_APP: u32 = 753;

pub fn handle(client: &SteamClient, cmd: InvCmd, st: &SharedInv, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        let web = match c.web_session().await {
            Ok(w) => w,
            Err(e) => {
                let e = format!("Нет доступа к сайту Steam: {}", err(e));
                let mut s = st.lock().unwrap();
                match cmd {
                    InvCmd::Apps => s.apps = Some(Err(e)),
                    InvCmd::Items { appid, ctx: cx } => {
                        s.loading.remove(&(appid, cx));
                        s.items.insert((appid, cx), Err(e));
                    }
                    InvCmd::Wallet => s.wallet = Some(Err(e)),
                    InvCmd::Listings => s.listings = Some(Err(e)),
                    InvCmd::Price { appid, name } => {
                        s.prices.insert((appid, name), Err(e));
                    }
                    InvCmd::Search { query, .. } => {
                        s.searching = false;
                        s.search = Some((query, Err(e)));
                    }
                }
                drop(s);
                ctx.request_repaint();
                return;
            }
        };
        match cmd {
            InvCmd::Apps => {
                let url = format!("https://steamcommunity.com/profiles/{}/inventory/?l=russian", web.steam_id());
                let res = match web.get_text(&url).await {
                    Ok(html) => parse_app_contexts(&html),
                    Err(e) => Err(err(e)),
                };
                // Если страницу разобрать не удалось - хотя бы инвентарь Steam.
                let res = match res {
                    Ok(list) if !list.is_empty() => Ok(list),
                    Ok(_) => Ok(vec![steam_app()]),
                    Err(e) => {
                        eprintln!("inventory apps: {e}");
                        Ok(vec![steam_app()])
                    }
                };
                st.lock().unwrap().apps = Some(res);
            }
            InvCmd::Items { appid, ctx: cx } => {
                let res = sc::inventory_via_lang(&web, appid, cx, "russian").await.map_err(err);
                let mut s = st.lock().unwrap();
                s.loading.remove(&(appid, cx));
                s.items.insert((appid, cx), res);
            }
            InvCmd::Wallet => {
                let res = sc::wallet_via(&web).await.map_err(err);
                st.lock().unwrap().wallet = Some(res);
            }
            InvCmd::Listings => {
                let url = "https://steamcommunity.com/market/mylistings/?norender=1&count=100&l=russian";
                let res = match web.get_text(url).await {
                    Ok(body) => parse_my_market(&body),
                    Err(e) => Err(err(e)),
                };
                st.lock().unwrap().listings = Some(res);
            }
            InvCmd::Price { appid, name } => {
                let cur = currency(&st);
                let res = sc::market_price(appid, &name, cur).await.map_err(err);
                st.lock().unwrap().prices.insert((appid, name), res);
            }
            InvCmd::Search { query, start } => {
                // С нашей веб-сессией Steam отдает русские названия и цены в валюте кошелька.
                let url = format!(
                    "https://steamcommunity.com/market/search/render/?norender=1&l=russian&start={start}&count=10&query={}",
                    url_encode(&query)
                );
                let res = match web.get_text(&url).await {
                    Ok(body) => parse_search(&body),
                    Err(e) => Err(err(e)),
                };
                let mut s = st.lock().unwrap();
                // Пока искали, могли ввести новый запрос - оставляем только свежий.
                if s.search.as_ref().is_some_and(|(t, _)| *t == query) {
                    let res = match (res, s.search.take()) {
                        // «Показать еще»: дописываем к уже загруженным.
                        (Ok((total, more)), Some((_, Ok((_, mut have))))) if start > 0 => {
                            have.extend(more);
                            Ok((total, have))
                        }
                        (r, _) => r,
                    };
                    s.search = Some((query, res));
                    s.searching = false;
                }
            }
        }
        ctx.request_repaint();
    });
}

fn steam_app() -> InvApp {
    InvApp { appid: STEAM_APP, name: "Steam".into(), icon: None, count: 0, contexts: vec![(6, "Сообщество".into(), 0)] }
}

/// Валюта кошелька (для цен площадки); пока неизвестна - доллары.
fn currency(st: &SharedInv) -> u32 {
    match &st.lock().unwrap().wallet {
        Some(Ok(w)) if w.currency > 0 => w.currency,
        _ => 1,
    }
}

/// Сумма из минимальных единиц (копеек, центов) в валюте Steam: «3,00 ₴», «$1.00».
pub fn money(cents: i64, currency: u32) -> String {
    let sym = match currency {
        2 => "£",
        3 => "€",
        5 => "₽",
        18 => "₴",
        37 => "₸",
        1 => return format!("${:.2}", cents as f64 / 100.0),
        _ => return sc::format_currency(cents, currency),
    };
    format!("{:.2} {sym}", cents as f64 / 100.0).replace('.', ",")
}

/// Валюта кошелька из состояния (для заявок на покупку и подписи цен).
pub fn wallet_currency(st: &InvState) -> u32 {
    match &st.wallet {
        Some(Ok(w)) if w.currency > 0 => w.currency,
        _ => 1,
    }
}

/// Свой лот на площадке.
#[derive(Clone, Debug)]
pub struct Lot {
    pub appid: u32,
    pub name: String,
    pub hash_name: String,
    pub icon: Option<String>,
    /// Сколько получите вы.
    pub receive: i64,
    /// Сколько заплатит покупатель (с комиссией).
    pub pay: i64,
    pub currency: u32,
}

/// Своя заявка на покупку.
#[derive(Clone, Debug)]
pub struct Order {
    pub appid: u32,
    pub name: String,
    pub hash_name: String,
    pub icon: Option<String>,
    pub price: i64,
    pub quantity: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MyMarket {
    pub lots: Vec<Lot>,
    /// Лоты, ждущие подтверждения в мобильном приложении.
    pub to_confirm: usize,
    pub orders: Vec<Order>,
}

/// Результат поиска по площадке.
#[derive(Clone, Debug)]
pub struct MarketHit {
    pub appid: u32,
    pub name: String,
    pub hash_name: String,
    pub app_name: String,
    pub icon: Option<String>,
    pub price_text: String,
    pub listings: u64,
}

fn num(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

fn s(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

fn econ_icon(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| format!("{}{s}", aurelia::web::web_session::ECON_IMAGE_BASE))
}

/// Ответ /market/mylistings/?norender=1.
pub fn parse_my_market(body: &str) -> Result<MyMarket, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Steam прислал непонятный ответ".to_string())?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err("Steam отклонил запрос (попробуйте позже)".into());
    }
    let lot = |x: &Value| -> Option<Lot> {
        let a = x.get("asset")?;
        let price = num(x.get("price"));
        let cur = num(x.get("currencyid")) as u32;
        Some(Lot {
            appid: num(a.get("appid")) as u32,
            // market_name - с износом/качеством, как на площадке.
            name: Some(s(a.get("market_name"))).filter(|n| !n.is_empty()).unwrap_or_else(|| s(a.get("name"))),
            hash_name: s(a.get("market_hash_name")),
            icon: econ_icon(a.get("icon_url")),
            receive: price,
            pay: price + num(x.get("fee")),
            // Код валюты лота: 2000 + валюта Steam.
            currency: if cur > 2000 { cur - 2000 } else { cur },
        })
    };
    let arr = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let orders = arr("buy_orders")
        .iter()
        .map(|o| {
            let d = o.get("description");
            Order {
                appid: num(o.get("appid")) as u32,
                name: d.map(|d| s(d.get("name"))).filter(|n| !n.is_empty()).unwrap_or_else(|| s(o.get("hash_name"))),
                hash_name: s(o.get("hash_name")),
                icon: econ_icon(d.and_then(|d| d.get("icon_url"))),
                price: num(o.get("price")),
                quantity: num(o.get("quantity_remaining")).max(num(o.get("quantity"))) as u64,
            }
        })
        .collect();
    Ok(MyMarket {
        lots: arr("listings").iter().filter_map(lot).collect(),
        to_confirm: arr("listings_to_confirm").len(),
        orders,
    })
}

/// Ответ /market/search/render/?norender=1: (всего найдено, страница результатов).
pub fn parse_search(body: &str) -> Result<(u32, Vec<MarketHit>), String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Steam прислал непонятный ответ".to_string())?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err("Steam отклонил запрос (попробуйте позже)".into());
    }
    let hits = v
        .get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| {
                    let d = r.get("asset_description");
                    MarketHit {
                        appid: num(d.and_then(|d| d.get("appid"))) as u32,
                        name: s(r.get("name")),
                        hash_name: s(r.get("hash_name")),
                        app_name: s(r.get("app_name")),
                        icon: econ_icon(d.and_then(|d| d.get("icon_url"))),
                        price_text: s(r.get("sell_price_text")),
                        listings: num(r.get("sell_listings")) as u64,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((num(v.get("total_count")) as u32, hits))
}

/// Список игр с инвентарем со страницы профиля: `g_rgAppContextData = {...};`.
pub fn parse_app_contexts(html: &str) -> Result<Vec<InvApp>, String> {
    let key = "g_rgAppContextData";
    let start = html.find(key).ok_or("на странице инвентаря нет списка игр")?;
    let rest = &html[start + key.len()..];
    let rest = rest.trim_start().strip_prefix('=').ok_or("неожиданный формат страницы")?;
    let v: Value = serde_json::Deserializer::from_str(rest.trim_start())
        .into_iter::<Value>()
        .next()
        .ok_or("пустой список игр")?
        .map_err(err)?;
    let obj = v.as_object().ok_or("неожиданный формат списка игр")?;
    let num = |v: Option<&Value>| v.and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0);
    let mut apps: Vec<InvApp> = obj
        .values()
        .filter_map(|a| {
            let appid = num(a.get("appid")) as u32;
            if appid == 0 {
                return None;
            }
            let mut contexts: Vec<(u32, String, u64)> = a
                .get("rgContexts")
                .and_then(Value::as_object)
                .map(|m| {
                    m.values()
                        .filter_map(|cx| {
                            let id = num(cx.get("id")) as u32;
                            let name = cx.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                            Some((id, name, num(cx.get("asset_count"))))
                        })
                        .collect()
                })
                .unwrap_or_default();
            contexts.retain(|c| c.2 > 0);
            contexts.sort_by_key(|c| c.0);
            Some(InvApp {
                appid,
                name: a.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                icon: a.get("icon").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from),
                count: num(a.get("asset_count")),
                contexts,
            })
        })
        .filter(|a| !a.contexts.is_empty())
        .collect();
    // Steam первым, остальные - по числу предметов.
    apps.sort_by(|a, b| (b.appid == STEAM_APP).cmp(&(a.appid == STEAM_APP)).then(b.count.cmp(&a.count)));
    Ok(apps)
}

/// Кодирование названия предмета для ссылки на торговую площадку.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Страница предмета на торговой площадке.
pub fn listing_url(appid: u32, hash_name: &str) -> String {
    format!("https://steamcommunity.com/market/listings/{appid}/{}", url_encode(hash_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Живой тест: сохраняет ответы сайта Steam в папку из VAPORUST_DUMP.
    #[test]
    #[ignore]
    fn inv_live_dump() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = c.web_session().await.unwrap();
            let id = web.steam_id();
            for (name, url) in [
                ("inv_page.html", format!("https://steamcommunity.com/profiles/{id}/inventory/?l=russian")),
                ("market.html", "https://steamcommunity.com/market/".to_string()),
                ("mylistings.json", "https://steamcommunity.com/market/mylistings/?norender=1&count=100".to_string()),
                ("search.json", "https://steamcommunity.com/market/search/render/?norender=1&query=Mann%20Co&start=0&count=50&l=russian".to_string()),
            ] {
                match web.get_text(&url).await {
                    Ok(t) => std::fs::write(dir.join(name), t).unwrap(),
                    Err(e) => println!("{name}: {e:#}"),
                }
            }
            match aurelia::steam_client::wallet_via(&web).await {
                Ok(w) => println!("wallet ok: {}", w.formatted),
                Err(e) => println!("wallet err: {e:#}"),
            }
        });
    }

    /// Живой тест поиска: разные варианты запроса «Dragon Lore».
    #[test]
    #[ignore]
    fn market_search_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = c.web_session().await.unwrap();
            for q in [
                "query=Dragon%20Lore",
                "query=Dragon%20Lore&appid=730",
                "query=AWP%20Dragon%20Lore",
                "query=AWP%20%7C%20Dragon%20Lore",
                "query=Dragon%20Lore&search_descriptions=0&sort_column=popular&sort_dir=desc",
                "query=AWP&appid=730",
            ] {
                for lang in ["russian", "english"] {
                    let url = format!("https://steamcommunity.com/market/search/render/?norender=1&l={lang}&start=0&count=10&{q}");
                    match web.get_text(&url).await.map(|b| parse_search(&b)) {
                        Ok(Ok((t, hits))) => {
                            println!("{q} [{lang}] total {t}: {:?}", hits.iter().map(|h| h.name.clone()).collect::<Vec<_>>())
                        }
                        Ok(Err(e)) => println!("{q} [{lang}] parse: {e}"),
                        Err(e) => println!("{q} [{lang}] err: {e:#}"),
                    }
                }
            }
        });
    }

    /// Разбор сохраненных живым тестом ответов (папка VAPORUST_DUMP).
    #[test]
    #[ignore]
    fn inv_parse_dump() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let apps = parse_app_contexts(&std::fs::read_to_string(dir.join("inv_page.html")).unwrap()).unwrap();
        for a in &apps {
            println!("app {} {} {:?}", a.appid, a.name, a.contexts);
        }
        let m = parse_my_market(&std::fs::read_to_string(dir.join("mylistings.json")).unwrap()).unwrap();
        for l in &m.lots {
            println!("lot {} | {} | pay {} receive {}", l.appid, l.name, money(l.pay, l.currency), money(l.receive, l.currency));
        }
        let (total, hits) = parse_search(&std::fs::read_to_string(dir.join("search.json")).unwrap()).unwrap();
        println!("search total {total}");
        for h in &hits {
            println!("hit {} | {} | {} | {}", h.name, h.app_name, h.price_text, h.icon.is_some());
        }
    }

    #[test]
    fn contexts() {
        let html = r#"<script> var g_rgAppContextData = {"753":{"appid":753,"name":"Steam","icon":"x.jpg","asset_count":12,"rgContexts":{"6":{"asset_count":12,"id":"6","name":"Community"},"7":{"asset_count":0,"id":"7","name":"Rewards"}}},"440":{"appid":440,"name":"Team Fortress 2","icon":"","asset_count":30,"rgContexts":{"2":{"asset_count":30,"id":"2","name":"Backpack"}}}};
        var g_x = 1;</script>"#;
        let apps = parse_app_contexts(html).unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].appid, 753);
        assert_eq!(apps[0].contexts, vec![(6, "Community".to_string(), 12)]);
        assert_eq!(apps[1].contexts[0].0, 2);
        assert_eq!(url_encode("Mann Co. Key"), "Mann%20Co.%20Key");
    }
}
