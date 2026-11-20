//! Steam inventory, wallet and Community Market (via Aurelia's web session).
//! Selling (quick, instant, own price), listings and trade offers are in trade.rs (experimental
//! functions); buying happens on the Steam site.

use aurelia::steam_client::{self as sc, InventoryItem, MarketPrice, SteamClient, WalletBalance};
use eframe::egui;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// A game in which the account has items.
#[derive(Clone, Debug)]
pub struct InvApp {
    pub appid: u32,
    pub name: String,
    pub icon: Option<String>,
    pub count: u64,
    /// Inventory sections of the game: (id, name, items).
    pub contexts: Vec<(u32, String, u64)>,
    /// The game's market fee (market_pubfee_rate) if the game has its own; otherwise from the wallet.
    pub pubfee: Option<f64>,
}

#[derive(Default)]
pub struct InvState {
    pub apps: Option<Result<Vec<InvApp>, String>>,
    pub items: HashMap<(u32, u32), Result<Vec<InventoryItem>, String>>,
    pub loading: HashSet<(u32, u32)>,
    pub wallet: Option<Result<WalletBalance, String>>,
    pub listings: Option<Result<MyMarket, String>>,
    /// Item prices by (appid, market_hash_name).
    pub prices: HashMap<(u32, String), Result<MarketPrice, String>>,
    /// Market search: query and (total found, loaded results).
    pub search: Option<(String, Result<(u32, Vec<MarketHit>), String>)>,
    pub searching: bool,
    /// Market and trades: price history, orders, selling, trades.
    pub trade: crate::trade::TradeState,
}

pub type SharedInv = Arc<Mutex<InvState>>;

pub enum InvCmd {
    /// List of games with items + wallet.
    Apps,
    Items { appid: u32, ctx: u32 },
    Wallet,
    Listings,
    Price { appid: u32, name: String },
    /// Search: query and starting result (Steam returns 10 at a time).
    Search { query: String, start: u32 },
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Steam: the "Steam" inventory (cards, backgrounds, emoticons) is 753, context 6.
const STEAM_APP: u32 = 753;

pub fn handle(client: &SteamClient, cmd: InvCmd, st: &SharedInv, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        let web = match crate::webapi::web_session(&c).await {
            Ok(w) => w,
            Err(e) => {
                let e = trf!("Нет доступа к сайту Steam: {}", "No access to the Steam website: {}", err(e));
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
                let url = format!("https://steamcommunity.com/profiles/{}/inventory/?l={}", web.steam_id(), crate::lang::steam());
                let res = match web.get_text(&url).await {
                    Ok(html) => parse_app_contexts(&html),
                    Err(e) => Err(err(e)),
                };
                // If the page could not be parsed - at least the Steam inventory.
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
                let res = sc::inventory_via_lang(&web, appid, cx, crate::lang::steam()).await.map_err(err);
                let mut s = st.lock().unwrap();
                s.loading.remove(&(appid, cx));
                s.items.insert((appid, cx), res);
            }
            InvCmd::Wallet => {
                let res = sc::wallet_via(&web).await.map_err(err);
                st.lock().unwrap().wallet = Some(res);
            }
            InvCmd::Listings => {
                let url = format!("https://steamcommunity.com/market/mylistings/?norender=1&count=100&l={}", crate::lang::steam());
                let res = match web.get_text(&url).await {
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
                // With our web session Steam returns Russian names and prices in the wallet currency.
                let url = format!(
                    "https://steamcommunity.com/market/search/render/?norender=1&l={lang}&start={start}&count=10&query={}",
                    url_encode(&query),
                    lang = crate::lang::steam()
                );
                let res = match web.get_text(&url).await {
                    Ok(body) => parse_search(&body),
                    Err(e) => Err(err(e)),
                };
                let mut s = st.lock().unwrap();
                // A new query may have been entered during the search - keep only the fresh one.
                if s.search.as_ref().is_some_and(|(t, _)| *t == query) {
                    let res = match (res, s.search.take()) {
                        // "Show more": append to what is already loaded.
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
    InvApp { appid: STEAM_APP, name: "Steam".into(), icon: None, count: 0, contexts: vec![(6, tr!("Сообщество", "Community").into(), 0)], pubfee: None }
}

/// Wallet currency (for market prices); dollars while unknown.
fn currency(st: &SharedInv) -> u32 {
    match &st.lock().unwrap().wallet {
        Some(Ok(w)) if w.currency > 0 => w.currency,
        _ => 1,
    }
}

/// Amount from minimal units (kopecks, cents) in a Steam currency: "3,00 ₴", "$1.00".
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

/// Wallet currency from the state (for buy orders and price labels).
pub fn wallet_currency(st: &InvState) -> u32 {
    match &st.wallet {
        Some(Ok(w)) if w.currency > 0 => w.currency,
        _ => 1,
    }
}

/// Own market listing.
#[derive(Clone, Debug)]
pub struct Lot {
    /// Listing id (to remove it from sale).
    pub id: String,
    pub appid: u32,
    pub name: String,
    pub hash_name: String,
    pub icon: Option<String>,
    /// How much you receive.
    pub receive: i64,
    /// How much the buyer pays (with fees).
    pub pay: i64,
    pub currency: u32,
}

/// Own buy order.
#[derive(Clone, Debug)]
pub struct Order {
    /// Order id (to cancel).
    pub id: String,
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
    /// Listings waiting for confirmation in the mobile app.
    pub to_confirm: usize,
    pub orders: Vec<Order>,
}

/// Market search result.
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

/// Id (string or number in JSON) as a string.
fn id_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(x) => x.to_string(),
    }
}

fn s(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

fn econ_icon(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| format!("{}{s}", aurelia::web::web_session::ECON_IMAGE_BASE))
}

/// Response of /market/mylistings/?norender=1.
pub fn parse_my_market(body: &str) -> Result<MyMarket, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| tr!("Steam прислал непонятный ответ", "Steam sent an unclear answer").to_string())?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(tr!("Steam отклонил запрос (попробуйте позже)", "Steam rejected the request (try later)").into());
    }
    let lot = |x: &Value| -> Option<Lot> {
        let a = x.get("asset")?;
        let price = num(x.get("price"));
        let cur = num(x.get("currencyid")) as u32;
        Some(Lot {
            id: id_str(x.get("listingid")),
            appid: num(a.get("appid")) as u32,
            // market_name - with wear/quality, as on the market.
            name: Some(s(a.get("market_name"))).filter(|n| !n.is_empty()).unwrap_or_else(|| s(a.get("name"))),
            hash_name: s(a.get("market_hash_name")),
            icon: econ_icon(a.get("icon_url")),
            receive: price,
            pay: price + num(x.get("fee")),
            // Listing currency code: 2000 + Steam currency.
            currency: if cur > 2000 { cur - 2000 } else { cur },
        })
    };
    let arr = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let orders = arr("buy_orders")
        .iter()
        .map(|o| {
            let d = o.get("description");
            Order {
                id: id_str(o.get("buy_orderid")),
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

/// Response of /market/search/render/?norender=1: (total found, page of results).
pub fn parse_search(body: &str) -> Result<(u32, Vec<MarketHit>), String> {
    let v: Value = serde_json::from_str(body).map_err(|_| tr!("Steam прислал непонятный ответ", "Steam sent an unclear answer").to_string())?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(tr!("Steam отклонил запрос (попробуйте позже)", "Steam rejected the request (try later)").into());
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

/// List of games with an inventory from the profile page: `g_rgAppContextData = {...};`.
pub fn parse_app_contexts(html: &str) -> Result<Vec<InvApp>, String> {
    let key = "g_rgAppContextData";
    let start = html.find(key).ok_or(tr!("на странице инвентаря нет списка игр", "the inventory page has no game list"))?;
    let rest = &html[start + key.len()..];
    let rest = rest.trim_start().strip_prefix('=').ok_or(tr!("неожиданный формат страницы", "unexpected page format"))?;
    let v: Value = serde_json::Deserializer::from_str(rest.trim_start())
        .into_iter::<Value>()
        .next()
        .ok_or(tr!("пустой список игр", "empty game list"))?
        .map_err(err)?;
    let obj = v.as_object().ok_or(tr!("неожиданный формат списка игр", "unexpected game list format"))?;
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
                pubfee: a.get("market_pubfee_rate").and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))),
            })
        })
        .filter(|a| !a.contexts.is_empty())
        .collect();
    // Steam first, others by item count.
    apps.sort_by(|a, b| (b.appid == STEAM_APP).cmp(&(a.appid == STEAM_APP)).then(b.count.cmp(&a.count)));
    Ok(apps)
}

/// Encoding an item name for a Community Market link.
pub use crate::util::urlencode as url_encode;

/// Item page on the Community Market.
pub fn listing_url(appid: u32, hash_name: &str) -> String {
    format!("https://steamcommunity.com/market/listings/{appid}/{}", url_encode(hash_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live test: saves Steam site responses to the folder from VAPORUST_DUMP.
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

    /// Live search test: different variants of the "Dragon Lore" query.
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

    /// Parsing of responses saved by the live test (folder VAPORUST_DUMP).
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

