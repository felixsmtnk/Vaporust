//! Steam store: data and network requests (via Aurelia).
//!
//! Lists (search, discounts, wishlist) store only appids; game details
//! (`StoreAppInfo`: price, description, images...) are loaded lazily - for the games
//! visible in the window, in batches of 50. Everything lives in a separate Arc<Mutex<StoreState>>, not in
//! NetState, which is copied on every frame.

use aurelia::steam_client::{DealsScope, SteamClient, StoreAppInfo};
use aurelia::web::discovery::NewsItem;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Store text language.
fn lang() -> &'static str {
    crate::lang::steam()
}

/// Selectable price regions (country code, name).
#[allow(non_snake_case)]
pub fn COUNTRIES() -> [(&'static str, &'static str); 10] {
    [
    ("UA", tr!("Украина (₴)", "Ukraine (₴)")),
    ("RU", tr!("Россия (₽)", "Russia (₽)")),
    ("KZ", tr!("Казахстан (₸)", "Kazakhstan (₸)")),
    ("BY", tr!("Беларусь", "Belarus")),
    ("GE", tr!("Грузия ($)", "Georgia ($)")),
    ("PL", tr!("Польша (zł)", "Poland (zł)")),
    ("DE", tr!("Германия (€)", "Germany (€)")),
    ("TR", tr!("Турция ($)", "Turkey ($)")),
    ("US", tr!("США ($)", "USA ($)")),
    ("GB", tr!("Великобритания (£)", "United Kingdom (£)")),
]
}

#[derive(Default)]
pub struct Extras {
    pub players: Option<i32>,
    pub news: Option<Result<Vec<NewsItem>, String>>,
    pub similar: Option<Vec<u32>>,
    /// System requirements (Windows): minimum and recommended, as text.
    pub requirements: Option<(String, String)>,
    /// Trailer link (video, opens in a browser), genres and the game support page.
    pub trailer: Option<String>,
    pub genres: Vec<String>,
    pub support: Option<String>,
    /// Reviews from the community page (loaded on a button click).
    pub reviews: Option<Reviews>,
}

/// Review request numbers, unique for the run.
static REVIEW_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Grows when the store cache is dropped (language change): the "already asked" marks of the
/// interface include it, so the pages are asked for again.
pub static CACHE_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The current cache generation. A background answer is stored only if the generation did not
/// change while it was loading: otherwise it is in the old language (or for the old region).
pub fn gen() -> u32 {
    CACHE_GEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Game reviews: the chosen order and language, loaded pages.
#[derive(Default, Clone)]
pub struct Reviews {
    /// Order (index in REVIEW_SORTS()).
    pub sort: u8,
    /// Review language: Steam code from REVIEW_LANGS() ("russian", "all"...). A code, not a list
    /// position: the list order follows the interface language.
    pub lang: &'static str,
    /// Number of the latest request: an answer to an older one is dropped.
    pub seq: u32,
    pub pages: u32,
    pub list: Vec<Review>,
    pub more: bool,
    /// Next page cursor (Steam pages reviews by it).
    pub cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Reviews {
    /// Start a request for (order, language, page). Returns the page to fetch and the request
    /// number, or None if the same order and language is already loading.
    /// A changed order or language starts over from page 1 with no cursor: a Steam cursor belongs
    /// to one order and language, and the loaded list must not mix languages.
    pub fn begin(&mut self, sort: u8, lang: &'static str, mut page: u32) -> Option<(u32, u32)> {
        let same = self.sort == sort && self.lang == lang;
        if self.loading && same {
            return None;
        }
        // "Next page" of other criteria than the current ones: a request left over from the frame in
        // which the user changed the order or language - it must not switch the choice back.
        if !same && page > 1 {
            return None;
        }
        if !same || page <= 1 {
            page = 1;
            let seq = self.seq;
            *self = Reviews { sort, lang, seq, ..Default::default() };
        }
        // The number is unique for the whole run (not per object): after the cache is dropped (language
        // change) a new object would start from 1 again and accept an old answer as its own.
        self.seq = REVIEW_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.loading = true;
        Some((page, self.seq))
    }
}

#[derive(Clone, Debug)]
pub struct Review {
    pub positive: bool,
    pub hours: String,
    pub date: String,
    pub author: String,
    pub text: String,
    pub url: String,
    pub avatar: String,
    /// How many found it helpful / funny.
    pub helpful_n: u32,
    pub funny_n: u32,
    /// Awards (Steam points reactions): (reaction id, count).
    pub awards: Vec<(u32, u32)>,
    /// "Products on account: N".
    pub products: String,
}

#[derive(Default)]
pub struct StoreState {
    /// Price country; empty - not determined yet.
    pub country: String,
    /// Game details by appid (for the current country).
    pub apps: HashMap<u32, StoreAppInfo>,
    /// Details already requested (so we do not ask again).
    pub requested: HashSet<u32>,
    /// Search: (query, result).
    pub search: Option<(String, Result<Vec<u32>, String>)>,
    pub searching: bool,
    pub deals: Option<Result<Vec<u32>, String>>,
    /// Own wishlist, by priority.
    pub wishlist: Option<Result<Vec<u32>, String>>,
    /// Steam events and sales: (appid, what event, until when).
    pub events: Option<Result<Vec<(u32, String, u64)>, String>>,
    /// Extra game page data: player count, news, reviews, similar games.
    pub extras: HashMap<u32, Extras>,
    /// Error of the last action (for example adding to the wishlist).
    pub error: Option<String>,
    /// A friend's wishlist: (SteamID, name, list by priority).
    pub friend_wishlist: Option<(u64, String, Option<Result<Vec<u32>, String>>)>,
    /// Discounts on wishlisted games (for notifications); sales_gen grows on every check.
    pub sales: Vec<Sale>,
    pub sales_gen: u64,
    /// Wishlist at the last check: (appid, name, not released yet).
    pub wish_status: Vec<(u32, String, bool)>,
    /// Reviews by URL: full text and id; the vote result.
    pub review_full: HashMap<String, Result<(String, u64, String), String>>,
    /// Own review votes in this session: URL -> 0 helpful, 1 not helpful, 2 funny.
    pub review_mine: HashMap<String, u8>,
    pub review_vote: HashMap<String, String>,
    /// Charts: number -> (appid, label).
    pub charts: HashMap<u8, Result<Vec<(u32, String)>, String>>,
    /// Games hidden from recommendations ("Not interested"); None - the list is not loaded yet.
    pub ignored: Option<HashSet<u32>>,
    /// Curators followed during this session (Steam does not report the list here).
    pub followed: HashSet<u64>,
    /// Discovery queue.
    pub discovery: Option<Result<Vec<u32>, String>>,
    /// Package names (cart).
    pub package_names: HashMap<u32, String>,
    /// The first game of a package (cart pictures).
    pub package_apps: HashMap<u32, u32>,
    /// Curators who reviewed the game.
    pub curators: HashMap<u32, Result<Vec<crate::actions::Curator>, String>>,
}

/// A wishlisted game on sale.
#[derive(Clone)]
pub struct Sale {
    pub appid: u32,
    pub name: String,
    pub pct: i32,
    /// Discounted price (as text, in the region currency).
    pub price: String,
    /// End of the discount (unix), 0 - unknown.
    pub end: u64,
}

pub type SharedStore = Arc<Mutex<StoreState>>;

/// Store commands (handled in the network thread).
pub enum StoreCmd {
    Search(String),
    Deals,
    /// Events and sales (daily/weekend deals, releases, updates).
    Events,
    Wishlist,
    /// Check discounts on wishlisted games (for notifications).
    WishlistSales,
    /// A friend's wishlist (if their privacy settings allow it).
    FriendWishlist(u64),
    /// Game reviews: order (REVIEW_SORTS()), language code (REVIEW_LANGS()), page (from 1).
    Reviews { appid: u32, sort: u8, lang: &'static str, page: u32 },
    /// Full review text (by its URL) and a review vote (0 helpful, 1 not helpful, 2 funny).
    ReviewFull(String),
    ReviewVote { url: String, kind: u8 },
    /// Details for these games (lazily, for those visible in the list).
    Details(Vec<u32>),
    /// Package name (for the cart).
    PackageName(u32),
    /// Game page: details + player count, news, reviews, similar games.
    Page(u32),
    /// Add to the wishlist (true) or remove.
    WishlistSet(u32, bool),
    /// Change the price region (resets details).
    Country(String),
    /// A Steam chart (index in CHARTS()).
    Charts(u8),
    /// Discovery queue: mark `skip` as viewed, `rebuild` - build a new one.
    Discovery { rebuild: bool, skip: Vec<u32> },
    /// "Not interested": hide a game from store recommendations (true) or bring it back.
    Ignore(u32, bool),
    /// Follow (true) or unfollow a curator (its clan number).
    FollowCurator(u64, bool),
    /// Which games are already hidden from recommendations (once per session).
    IgnoredList,
}

/// Russian name of a Steam event kind (MarketingMessage).
pub fn event_kind(kind: &str) -> String {
    match kind {
        "MidweekDeal" => tr!("скидка середины недели", "midweek deal"),
        "WeekendDeal" => tr!("скидка на выходные", "weekend deal"),
        "DailyDeal" => tr!("скидка дня", "daily deal"),
        "NowAvailable" => tr!("уже вышла", "already released"),
        "Update" => tr!("обновление", "update"),
        "FreeWeekend" => tr!("бесплатные выходные", "free weekend"),
        "FreeToPlay" => tr!("бесплатно", "free"),
        "Sale" | "PublisherSale" | "FranchiseSale" => tr!("распродажа", "sale"),
        "DLCNowAvailable" => tr!("новое дополнение", "new DLC"),
        "PreOrder" | "Preorder" => tr!("предзаказ", "pre-order"),
        other => return other.to_string(),
    }
    .to_string()
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Determine the price country if not set yet (by the IP of the Steam connection).
/// Price region: chosen manually, otherwise the Steam wallet country (account region), otherwise the
/// connection country (by IP - different when traveling).
async fn country(client: &SteamClient, st: &SharedStore) -> String {
    let known = st.lock().unwrap().country.clone();
    if !known.is_empty() {
        return known;
    }
    let mut cc = String::new();
    if let Ok(web) = crate::webapi::web_session(client).await {
        if let Ok(w) = aurelia::steam_client::wallet_via(&web).await {
            cc = w.country.to_uppercase();
        }
    }
    if cc.is_empty() {
        cc = client.store_country();
    }
    let mut s = st.lock().unwrap();
    if s.country.is_empty() {
        s.country = cc;
    }
    s.country.clone()
}

/// Load details for games not in the cache yet (in batches of 50).
async fn load_details(client: &SteamClient, ids: Vec<u32>, st: &SharedStore, ctx: &egui::Context) {
    let cc = country(client, st).await;
    let need: Vec<u32> = {
        let mut s = st.lock().unwrap();
        let need: Vec<u32> = ids
            .into_iter()
            .filter(|id| !s.apps.contains_key(id) && !s.requested.contains(id))
            .collect();
        s.requested.extend(need.iter().copied());
        need
    };
    for chunk in need.chunks(50) {
        let g = gen();
        if let Ok(apps) = client.fetch_store_apps(chunk, lang(), &cc).await {
            if gen() != g {
                return; // the language or region changed meanwhile: these are old
            }
            let mut s = st.lock().unwrap();
            for a in apps {
                s.apps.insert(a.app_id, a);
            }
        }
        ctx.request_repaint();
    }
}

pub fn handle(client: &SteamClient, cmd: StoreCmd, st: &SharedStore, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        match cmd {
            StoreCmd::Search(term) => {
                {
                    let mut s = st.lock().unwrap();
                    s.searching = true;
                    s.search = Some((term.clone(), Ok(Vec::new())));
                }
                ctx.request_repaint();
                let cc = country(&c, &st).await;
                let res = c.search_store(&term, 30, lang(), &cc).await.map(|p| p.app_ids).map_err(err);
                if let Ok(ids) = &res {
                    load_details(&c, ids.clone(), &st, &ctx).await;
                }
                let mut s = st.lock().unwrap();
                // A new query may have been entered during the search - keep only the fresh one.
                if s.search.as_ref().is_some_and(|(t, _)| *t == term) {
                    s.search = Some((term, res));
                    s.searching = false;
                }
            }
            StoreCmd::Deals => {
                let cc = country(&c, &st).await;
                let res = c
                    .query_deals(DealsScope::DiscountedTopSellers, 1, 0, 50, lang(), &cc)
                    .await
                    .map(|p| p.app_ids)
                    .map_err(err);
                if let Ok(ids) = &res {
                    load_details(&c, ids.iter().take(20).copied().collect(), &st, &ctx).await;
                }
                st.lock().unwrap().deals = Some(res);
            }
            StoreCmd::Charts(kind) => {
                let cc = country(&c, &st).await;
                let g = gen();
                let res = fetch_chart(kind, &cc).await;
                if let Ok(v) = &res {
                    load_details(&c, v.iter().take(20).map(|e| e.0).collect(), &st, &ctx).await;
                }
                if gen() == g {
                    st.lock().unwrap().charts.insert(kind, res);
                }
            }
            StoreCmd::Discovery { rebuild, skip } => {
                let cc = country(&c, &st).await;
                for a in skip {
                    let _ = c.discovery_skip(a).await;
                }
                let res = c.discovery_queue(&cc, rebuild).await.map_err(err);
                if let Ok(v) = &res {
                    load_details(&c, v.clone(), &st, &ctx).await;
                }
                st.lock().unwrap().discovery = Some(res);
            }
            StoreCmd::Events => {
                let cc = country(&c, &st).await;
                let res = c.active_store_events(&cc).await.map_err(err).map(|list| {
                    let mut seen = HashSet::new();
                    list.into_iter()
                        .filter(|e| e.associated_id != 0 && seen.insert(e.associated_id))
                        .map(|e| (e.associated_id, event_kind(&e.kind), e.end))
                        .collect::<Vec<_>>()
                });
                if let Ok(v) = &res {
                    load_details(&c, v.iter().take(20).map(|e| e.0).collect(), &st, &ctx).await;
                }
                st.lock().unwrap().events = Some(res);
            }
            StoreCmd::Wishlist => {
                // Order as in Steam ("your preference"): first games with a position (priority 1, 2...),
                // the rest by date added, oldest first. If that fails - Aurelia's list.
                let res = match c.my_steam_id() {
                    Some(me) => match wishlist_ranked(&c, me).await {
                        Ok(v) => Ok(v),
                        Err(_) => c.wishlist(me).await.map(|w| w.into_iter().map(|e| e.app_id).collect()).map_err(err),
                    },
                    None => Err(tr!("нет входа в Steam", "not logged in to Steam").into()),
                };
                st.lock().unwrap().wishlist = Some(res);
            }
            StoreCmd::WishlistSales => {
                let Some(me) = c.my_steam_id() else { return };
                let Ok(list) = c.wishlist(me).await else { return };
                let ids: Vec<u32> = list.into_iter().map(|e| e.app_id).collect();
                let cc = country(&c, &st).await;
                let mut sales = Vec::new();
                let mut status = Vec::new();
                // Fresh prices (without the store page cache): discounts change.
                for chunk in ids.chunks(50) {
                    if let Ok(apps) = c.fetch_store_apps(chunk, lang(), &cc).await {
                        for a in apps {
                            status.push((a.app_id, a.name.clone(), a.coming_soon));
                            if a.discount_pct > 0 {
                                sales.push(Sale {
                                    appid: a.app_id,
                                    name: a.name.clone(),
                                    pct: a.discount_pct,
                                    price: a.price.clone().unwrap_or_default(),
                                    end: a.discount_end.unwrap_or(0),
                                });
                            }
                        }
                    }
                }
                let mut s = st.lock().unwrap();
                s.sales = sales;
                s.wish_status = status;
                s.sales_gen += 1;
                drop(s);
                ctx.request_repaint();
            }
            StoreCmd::FriendWishlist(id) => {
                let res = c
                    .wishlist(id)
                    .await
                    .map(|w| w.into_iter().map(|e| e.app_id).collect::<Vec<u32>>())
                    .map_err(|e| trf!("список закрыт настройками приватности или недоступен ({})", "the list is closed by privacy settings or unavailable ({})", err(e)));
                if let Ok(ids) = &res {
                    load_details(&c, ids.iter().take(30).copied().collect(), &st, &ctx).await;
                }
                let mut s = st.lock().unwrap();
                if let Some(fw) = s.friend_wishlist.as_mut().filter(|f| f.0 == id) {
                    fw.2 = Some(res);
                }
                drop(s);
                ctx.request_repaint();
            }
            StoreCmd::Details(ids) => load_details(&c, ids, &st, &ctx).await,
            StoreCmd::PackageName(id) => {
                if let Ok((name, app)) = package_name(id).await {
                    let mut s = st.lock().unwrap();
                    s.package_names.insert(id, name);
                    if let Some(a) = app {
                        s.package_apps.insert(id, a);
                    }
                    drop(s);
                    ctx.request_repaint();
                }
            }
            StoreCmd::Page(appid) => {
                let g = gen();
                st.lock().unwrap().extras.entry(appid).or_default();
                ctx.request_repaint();
                load_details(&c, vec![appid], &st, &ctx).await;
                let cc = country(&c, &st).await;
                let players = c.current_players(appid).await.ok();
                let similar = c.similar_apps(appid, 8, lang(), &cc).await.map(|p| p.app_ids).ok();
                {
                    let mut s = st.lock().unwrap();
                    let e = s.extras.entry(appid).or_default();
                    e.players = players;
                    e.similar = similar.clone();
                }
                ctx.request_repaint();
                if let Some(sim) = similar {
                    load_details(&c, sim, &st, &ctx).await;
                }
                // Reviews are not loaded separately: store.steampowered.com/appreviews currently answers
                // with a redirect to the home page (checked 2026-11-12). The review summary comes
                // in StoreAppInfo.review_summary, full reviews - via a link in the browser.
                let news = match aurelia::core::net::steam_web_client() {
                    Ok(http) => aurelia::web::discovery::fetch_news(&http, appid, 5, 600).await.map_err(err),
                    Err(e) => Err(err(e)),
                };
                if gen() != g {
                    return; // the language changed: the page is asked again in the new one
                }
                st.lock().unwrap().extras.entry(appid).or_default().news = Some(news);
                // System requirements - from the public store API (api/appdetails), in Russian.
                if let Some(d) = fetch_appdetails(appid).await.filter(|_| gen() == g) {
                    let mut s = st.lock().unwrap();
                    let e = s.extras.entry(appid).or_default();
                    e.requirements = d.requirements;
                    e.trailer = d.trailer;
                    e.genres = d.genres;
                    e.support = d.support;
                }
                ctx.request_repaint();
            }
            StoreCmd::Reviews { appid, sort, lang, mut page } => {
                let (seq, cursor) = {
                    let mut s = st.lock().unwrap();
                    let r = s.extras.entry(appid).or_default().reviews.get_or_insert_with(Default::default);
                    let Some((p, seq)) = r.begin(sort, lang, page) else {
                        return;
                    };
                    page = p;
                    (seq, r.cursor.clone())
                };
                ctx.request_repaint();
                let res = fetch_reviews(appid, sort, lang, page, cursor).await;
                let mut s = st.lock().unwrap();
                let r = s.extras.entry(appid).or_default().reviews.get_or_insert_with(Default::default);
                if r.seq != seq {
                    return; // another order, language or page was asked for while loading
                }
                r.loading = false;
                match res {
                    Ok((v, next)) => {
                        r.more = v.len() >= 10 && next.is_some() && REVIEW_SORTS()[sort as usize].0 != "toprated";
                        r.cursor = next;
                        r.pages = page;
                        r.list.extend(v);
                        r.error = None;
                    }
                    Err(e) => r.error = Some(e),
                }
            }
            StoreCmd::ReviewFull(url) => {
                let g = gen();
                let r = review_page(&c, &url).await;
                if gen() == g {
                    st.lock().unwrap().review_full.insert(url, r);
                }
            }
            StoreCmd::ReviewVote { url, kind } => {
                let known = st.lock().unwrap().review_full.get(&url).and_then(|r| r.as_ref().ok().map(|x| x.1));
                let id = match known {
                    Some(id) => Ok(id),
                    None => review_page(&c, &url).await.map(|(t, id, at)| {
                        st.lock().unwrap().review_full.insert(url.clone(), Ok((t, id, at)));
                        id
                    }),
                };
                let msg = match id {
                    Ok(id) => match vote_review(&c, id, kind).await {
                        Ok(()) => {
                            st.lock().unwrap().review_mine.insert(url.clone(), kind);
                            String::new()
                        }
                        Err(e) => e,
                    },
                    Err(e) => e,
                };
                st.lock().unwrap().review_vote.insert(url, msg);
            }
            StoreCmd::WishlistSet(appid, add) => {
                let res = if add { c.wishlist_add(appid).await } else { c.wishlist_remove(appid).await };
                let mut s = st.lock().unwrap();
                match res {
                    Ok(_) => {
                        s.error = None;
                        if let Some(Ok(list)) = s.wishlist.as_mut() {
                            list.retain(|a| *a != appid);
                            if add {
                                list.push(appid);
                            }
                        }
                    }
                    Err(e) => s.error = Some(trf!("Список желаемого не изменен: {}", "Wishlist not changed: {}", err(e))),
                }
            }
            StoreCmd::Ignore(appid, on) => {
                let res = ignore(&c, appid, on).await;
                let mut s = st.lock().unwrap();
                match res {
                    Ok(()) => {
                        s.error = None;
                        let set = s.ignored.get_or_insert_with(Default::default);
                        if on {
                            set.insert(appid);
                        } else {
                            set.remove(&appid);
                        }
                    }
                    Err(e) => s.error = Some(trf!("Не удалось: {e}", "Failed: {e}")),
                }
            }
            StoreCmd::FollowCurator(clan, on) => {
                let res = crate::actions::follow_curator(&c, clan, on).await;
                let mut s = st.lock().unwrap();
                match res {
                    Ok(()) => {
                        s.error = None;
                        if on {
                            s.followed.insert(clan);
                        } else {
                            s.followed.remove(&clan);
                        }
                    }
                    Err(e) => s.error = Some(trf!("Подписка на куратора не изменена: {e}", "Curator follow not changed: {e}")),
                }
            }
            StoreCmd::IgnoredList => {
                if let Ok(set) = ignored_list(&c).await {
                    st.lock().unwrap().ignored = Some(set);
                }
            }
            StoreCmd::Country(cc) => {
                let mut s = st.lock().unwrap();
                s.country = cc;
                s.apps.clear();
                s.requested.clear();
                s.deals = None;
                s.events = None;
            }
        }
        ctx.request_repaint();
    });
}

/// A piece of the game description for display.
#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    Para(String),
    Bullet(String),
}

/// Store description into blocks. Understands both HTML (`<h2>`, `<p>`, `<li>`) and Steam BBCode
/// (`[h2]`, `[p]`, `[*]`, `[list]`); skips images and videos (`[img]`, `[video]`, `<img>`).
pub fn description_blocks(src: &str) -> Vec<Block> {
    const KNOWN: &[&str] = &[
        "p", "br", "h1", "h2", "h3", "h4", "h5", "b", "i", "u", "s", "strong", "em", "strike", "list",
        "olist", "ul", "ol", "li", "*", "img", "video", "url", "a", "hr", "quote", "code", "table", "tr",
        "td", "th", "spoiler", "noparse", "span", "div", "sup", "sub", "font", "dynamiclink", "previewyoutube",
        "carousel", "expand", "c", "center",
    ];
    #[derive(PartialEq, Clone, Copy)]
    enum Mode {
        Para,
        Heading,
        Bullet,
    }
    let mut blocks = Vec::new();
    let mut buf = String::new();
    let mut mode = Mode::Para;
    let flush = |buf: &mut String, mode: Mode, blocks: &mut Vec<Block>| {
        let t = decode_entities(buf.trim());
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            blocks.push(match mode {
                Mode::Heading => Block::Heading(t),
                Mode::Bullet => Block::Bullet(t),
                Mode::Para => Block::Para(t),
            });
        }
        buf.clear();
    };
    let mut rest = src;
    loop {
        let next = rest.find(['<', '[']);
        let Some(pos) = next else {
            buf.push_str(rest);
            break;
        };
        buf.push_str(&rest[..pos]);
        let open = rest.as_bytes()[pos] as char;
        let close = if open == '<' { '>' } else { ']' };
        let after = &rest[pos + 1..];
        let Some(end) = after.find(close) else {
            buf.push_str(&rest[pos..]);
            break;
        };
        let tag = after[..end].trim();
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split([' ', '=', '/']).next().unwrap_or("").to_lowercase();
        if !KNOWN.contains(&name.as_str()) {
            // Not a tag (for example "[18+]" in the text) - keep as is.
            buf.push(open);
            rest = after;
            continue;
        }
        rest = &after[end + 1..];
        match (closing, name.as_str()) {
            // Images and videos: in BBCode skipped together with the contents up to the closing tag.
            (false, "img" | "video" | "dynamiclink" | "previewyoutube") if open == '[' => {
                let end_tag = format!("[/{name}]");
                if let Some(p) = rest.find(&end_tag) {
                    rest = &rest[p + end_tag.len()..];
                }
            }
            (false, "h1" | "h2" | "h3" | "h4" | "h5") => {
                flush(&mut buf, mode, &mut blocks);
                mode = Mode::Heading;
            }
            (true, "h1" | "h2" | "h3" | "h4" | "h5") => {
                flush(&mut buf, mode, &mut blocks);
                mode = Mode::Para;
            }
            (false, "li" | "*") => {
                flush(&mut buf, mode, &mut blocks);
                mode = Mode::Bullet;
            }
            (true, "li") | (_, "list" | "olist" | "ul" | "ol") => {
                flush(&mut buf, mode, &mut blocks);
                mode = Mode::Para;
            }
            (_, "p" | "br" | "div" | "hr" | "tr") => {
                flush(&mut buf, mode, &mut blocks);
                if mode != Mode::Bullet {
                    mode = Mode::Para;
                }
            }
            _ => {} // formatting (b, i, url...) - drop the tag itself, keep the text
        }
    }
    flush(&mut buf, mode, &mut blocks);
    blocks
}

fn decode_entities(s: &str) -> String {
    crate::util::html_unescape(s).replace('\u{a0}', " ")
}

/// A description or a news item on one line (for short previews).
pub fn plain_text(src: &str) -> String {
    description_blocks(src)
        .into_iter()
        .map(|b| match b {
            Block::Heading(t) | Block::Para(t) => t,
            Block::Bullet(t) => format!("• {t}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Small game image for lists (120x45, ~5 KB).
/// A reduced screenshot (600x338) instead of the full one: ~7 times lighter.
pub fn screenshot_thumb(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    match base.strip_suffix(".jpg") {
        Some(b) if !b.ends_with(".600x338") => format!("{b}.600x338.jpg"),
        _ => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbcode_description() {
        let src = "[p][img src=\"{STEAM_APP_IMAGE}/extras/a.webp\" width=\"600\"][/img][/p][p]Зовите друзей[/p][h2]Особенности[/h2][list][*]Кооператив[*]Боссы[/list][p]Конец [18+][/p]";
        assert_eq!(
            description_blocks(src),
            vec![
                Block::Para("Зовите друзей".into()),
                Block::Heading("Особенности".into()),
                Block::Bullet("Кооператив".into()),
                Block::Bullet("Боссы".into()),
                Block::Para("Конец [18+]".into()),
            ]
        );
    }

    #[test]
    fn html_description() {
        let src = "<h2 class=\"bb_tag\">Об игре</h2>Строка&nbsp;1<br>Строка 2<ul><li>один</li><li>два</li></ul><img src=\"x.jpg\">";
        assert_eq!(
            description_blocks(src),
            vec![
                Block::Heading("Об игре".into()),
                Block::Para("Строка 1".into()),
                Block::Para("Строка 2".into()),
                Block::Bullet("один".into()),
                Block::Bullet("два".into()),
            ]
        );
    }

    #[test]
    fn thumbs() {
        assert_eq!(
            screenshot_thumb("https://x/apps/70/0000002343.jpg?t=1"),
            "https://x/apps/70/0000002343.600x338.jpg"
        );
    }
}

#[cfg(test)]
mod events_live {
    /// Live test (read only): Steam events and sales for the UA region.
    #[test]
    #[ignore]
    fn store_events_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for e in c.active_store_events("UA").await.unwrap() {
                println!("{} | {} | id {} {} | {}..{}", e.kind, e.title, e.associated_id, e.associated_name, e.start, e.end);
            }
        });
    }
}

/// Info from the public store API (store.steampowered.com/api/appdetails), in Russian.
#[derive(Default)]
pub(crate) struct AppDetails {
    pub(crate) requirements: Option<(String, String)>,
    pub(crate) trailer: Option<String>,
    pub(crate) genres: Vec<String>,
    pub(crate) support: Option<String>,
}

async fn fetch_appdetails(appid: u32) -> Option<AppDetails> {
    let http = aurelia::core::net::steam_web_client().ok()?;
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l={}", crate::lang::steam());
    let text = http.get(url).send().await.ok()?.text().await.ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(parse_appdetails(&v, appid))
}

pub(crate) fn parse_appdetails(v: &serde_json::Value, appid: u32) -> AppDetails {
    let Some(data) = v.get(appid.to_string()).and_then(|a| a.get("data")) else { return AppDetails::default() };
    // System requirements for Windows. No requirements - Steam sends an empty array instead of an object.
    let req = &data["pc_requirements"];
    let get = |k: &str| req.get(k).and_then(|x| x.as_str()).map(plain_text).unwrap_or_default();
    let (min, rec) = (get("minimum"), get("recommended"));
    // Trailer: the first video, a file link (mp4 or webm), otherwise a stream (hls).
    let trailer = data["movies"].as_array().and_then(|m| m.first()).and_then(|m| {
        [&m["mp4"]["max"], &m["webm"]["max"], &m["hls_h264"], &m["dash_h264"]]
            .into_iter()
            .find_map(|x| x.as_str().filter(|s| s.starts_with("http")).map(str::to_string))
    });
    let genres = data["genres"]
        .as_array()
        .map(|g| g.iter().filter_map(|x| x["description"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let support = data["support_info"]["url"].as_str().filter(|s| s.starts_with("http")).map(str::to_string);
    AppDetails {
        requirements: (!min.is_empty() || !rec.is_empty()).then_some((min, rec)),
        trailer,
        genres,
        support,
    }
}

/// Review order, as on the Steam site: (browsefilter, caption).
#[allow(non_snake_case)]
pub fn REVIEW_SORTS() -> [(&'static str, &'static str); 8] {
    [
    ("toprated", tr!("Полезные за все время", "Most helpful of all time")),
    ("trendyear", tr!("Полезные за год", "Most helpful this year")),
    ("trendsixmonths", tr!("Полезные за полгода", "Most helpful in 6 months")),
    ("trendthreemonths", tr!("Полезные за 3 месяца", "Most helpful in 3 months")),
    ("trendmonth", tr!("Полезные за месяц", "Most helpful this month")),
    ("trendweek", tr!("Полезные за неделю", "Most helpful this week")),
    ("mostrecent", tr!("Новые", "Newest")),
    ("funny", tr!("Забавные", "Funny")),
]
}

/// Review languages: (Steam code, name).
#[allow(non_snake_case)]
pub fn REVIEW_LANGS() -> [(&'static str, &'static str); 13] {
    // The first entry (the default) is the interface language.
    let (first, third) = if crate::lang::en() {
        (("english", "English"), ("russian", "Russian"))
    } else {
        (("russian", "На русском"), ("english", "Английский"))
    };
    [
    first,
    ("all", tr!("На всех языках", "All languages")),
    third,
    ("ukrainian", tr!("Украинский", "Ukrainian")),
    ("german", tr!("Немецкий", "German")),
    ("french", tr!("Французский", "French")),
    ("spanish", tr!("Испанский", "Spanish")),
    ("polish", tr!("Польский", "Polish")),
    ("brazilian", tr!("Португальский (Бразилия)", "Portuguese (Brazil)")),
    ("turkish", tr!("Турецкий", "Turkish")),
    ("schinese", tr!("Китайский", "Chinese")),
    ("japanese", tr!("Японский", "Japanese")),
    ("koreana", tr!("Корейский", "Korean")),
]
}

/// Reviews page from the Steam community (the way the site loads it on scroll): 10 reviews each.
/// Next pages - by the cursor from the MoreContentForm of the previous response.
/// "Most helpful of all time" is not paged by Steam without login (always the first 10) - checked.
async fn fetch_reviews(
    appid: u32,
    sort: u8,
    lang: &str,
    page: u32,
    cursor: Option<String>,
) -> Result<(Vec<Review>, Option<String>), String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let filter = REVIEW_SORTS().get(sort as usize).map_or("toprated", |s| s.0);
    // Only a code from the list goes into the address.
    let lang = REVIEW_LANGS().iter().find(|l| l.0 == lang).map_or(crate::lang::steam(), |l| l.0);
    let p = page.max(1);
    let cursor = match (p, cursor) {
        (1, _) => String::new(),
        (_, Some(c)) => format!("&userreviewscursor={}", urlenc(&c)),
        (_, None) => return Ok((Vec::new(), None)),
    };
    let url = format!(
        "https://steamcommunity.com/app/{appid}/homecontent/?userreviewsoffset={off}&p={p}{cursor}&numperpage=10\
         &browsefilter={filter}&appid={appid}&appHubSubSection=10&l={ui}&filterLanguage={lang}&searchText=\
         &maxInappropriateScore=50",
        off = (p - 1) * 10,
        // The page in the interface language (hours, dates); the parser knows both.
        ui = crate::lang::steam()
    );
    let resp = http.get(url).send().await.map_err(err)?;
    if !resp.status().is_success() {
        return Err(trf!("Steam ответил {}", "Steam answered {}", resp.status()));
    }
    let html = resp.text().await.map_err(err)?;
    let next = between(&html, "name=\"userreviewscursor\" value=\"", "\"").map(str::to_string);
    Ok((parse_reviews(&html), next))
}

/// Review page: full text and review id (for votes).
pub async fn review_page(client: &SteamClient, url: &str) -> Result<(String, u64, String), String> {
    let web = crate::webapi::web_session(client).await?;
    let html = web.get_text(&format!("{}?l={}", url.trim_end_matches('/'), crate::lang::steam())).await.map_err(err)?;
    parse_review_page(&html)
}

/// Parsing a review page: (full text, review id, hours at review time).
pub fn parse_review_page(html: &str) -> Result<(String, u64, String), String> {
    let text = between(html, "id=\"ReviewText\">", "</div>").map(strip_html).unwrap_or_default();
    let id = between(html, "RecommendationVoteUpBtn", "\"")
        .and_then(|n| n.parse().ok())
        .ok_or(tr!("Steam не дал номер отзыва", "Steam did not give the review number"))?;
    Ok((text, id, hours_at_review(html)))
}

/// "(42.0 ч. в момент написания)" / "(42.0 hrs at review time)" from the review page
/// -> "42.0 ч. на момент отзыва" / "42.0 hrs at review time".
pub fn hours_at_review(html: &str) -> String {
    let Some(pos) = html.find("в момент написания").or_else(|| html.find("at review time")) else { return String::new() };
    let before = &html[..pos];
    let start = before.rfind('(').map(|i| i + 1).unwrap_or(pos);
    let h = before[start..].trim();
    if h.is_empty() || h.len() > 40 { String::new() } else { trf!("{h} на момент отзыва", "{h} at review time") }
}

/// Vote on a review: 0 - helpful, 1 - not helpful, 2 - funny.
pub async fn vote_review(client: &SteamClient, id: u64, kind: u8) -> Result<(), String> {
    let web = crate::webapi::web_session(client).await?;
    let (url, mut fields) = if kind == 2 {
        (format!("https://steamcommunity.com/userreviews/votetag/{id}"), vec![("tagid", "1".to_string()), ("rateup", "true".to_string())])
    } else {
        (format!("https://steamcommunity.com/userreviews/rate/{id}"), vec![("rateup", if kind == 0 { "true" } else { "false" }.to_string())])
    };
    fields.push(("sessionid", web.sessionid().to_string()));
    let (status, body) = web.post_form(&url, &fields, "https://steamcommunity.com/").await.map_err(err)?;
    let ok = status == 200
        && serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v.get("success")?.as_i64()) == Some(1);
    if ok {
        Ok(())
    } else {
        Err(trf!("Steam не принял оценку (код {status})", "Steam did not accept the vote (code {status})"))
    }
}

use crate::util::urlencode as urlenc;

/// &quot; &amp; and the like -> characters.
fn entities(s: &str) -> String {
    crate::util::html_unescape(s)
}

/// HTML piece -> text: <br> -> line break, tags removed.
fn strip_html(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let lines: Vec<String> = entities(&out).lines().map(|l| l.trim().to_string()).collect();
    lines.join("\n").trim().to_string()
}

use crate::util::between;

/// Parsing review cards (apphub_Card) from the community page.
pub fn parse_reviews(html: &str) -> Vec<Review> {
    html.split("apphub_Card modalContentLink")
        .skip(1)
        .filter_map(|card| {
            let url = between(card, "data-modal-content-url=\"", "\"").unwrap_or("").to_string();
            let positive = between(card, "<div class=\"thumb\">", "</div>").is_some_and(|t| t.contains("thumbsUp"));
            let hours = between(card, "<div class=\"hours\">", "</div>").map(strip_html).unwrap_or_default();
            let body = between(card, "<div class=\"apphub_CardTextContent\">", "<div class=\"UserReviewCardContent_Footer\"")?;
            let date = between(body, "<div class=\"date_posted\">", "</div>").map(strip_html).unwrap_or_default();
            let text = body.split_once("</div>").map(|(_, t)| t).unwrap_or(body);
            // Card tail: the closing </div> of the text and the card.
            let text = strip_html(text.trim_end().trim_end_matches("</div>"));
            let author = between(card, "apphub_CardContentAuthorName", "</a>")
                .and_then(|a| a.rsplit_once('>').map(|(_, n)| entities(n.trim())))
                .unwrap_or_default();
            let num_after = |key: &str| -> u32 {
                card.split(key).nth(1).map(|s| s.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == ',').filter(|c| c.is_ascii_digit()).collect::<String>()).and_then(|n| n.parse().ok()).unwrap_or(0)
            };
            // English page: "12 people found this review helpful" - the number comes before the words.
            let num_before = |key: &str| -> u32 {
                let Some(pos) = card.find(key) else { return 0 };
                let head = card[..pos].trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace());
                let digits: String = head.chars().rev().take_while(|c| c.is_ascii_digit() || *c == ',').collect();
                digits.chars().rev().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
            };
            // Russian has two forms: "Пользователей, посчитавших обзор полезным: 2" and for one vote
            // "1 пользователь посчитал этот обзор полезным" (the number before the words, as in English).
            let helpful_n = [num_after("полезным:"), num_before("посчитал этот обзор полезным"), num_before("found this review helpful")]
                .into_iter()
                .find(|n| *n > 0)
                .unwrap_or(0);
            let funny_n = [num_after("забавным:"), num_before("посчитал этот обзор забавным"), num_before("found this review funny")]
                .into_iter()
                .find(|n| *n > 0)
                .unwrap_or(0);
            let mut awards = Vec::new();
            for part in card.split("data-reaction=&quot;").skip(1) {
                let id: u32 = part.split('&').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let cnt: u32 = part.split("data-reactioncount=&quot;").nth(1).and_then(|s| s.split('&').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
                if id > 0 && !awards.iter().any(|(a, _)| *a == id) {
                    awards.push((id, cnt));
                }
            }
            awards.sort_by_key(|a| std::cmp::Reverse(a.1));
            let avatar = between(card, "appHubIconHolder", "</div>").and_then(|h| between(h, "<img src=\"", "\"")).unwrap_or("").to_string();
            let products = between(card, "apphub_CardContentMoreLink ellipsis\">", "<").map(|s| s.trim().to_string()).unwrap_or_default();
            Some(Review { positive, hours, date, author, text, url, avatar, helpful_n, funny_n, awards, products })
        })
        .collect()
}

#[cfg(test)]
mod reviews_tests {
    /// VAPORUST_DUMP=a saved reviews page - card parsing.
    #[test]
    #[ignore]
    fn parse_saved_reviews() {
        let html = std::fs::read_to_string(std::env::var("VAPORUST_DUMP").unwrap()).unwrap();
        let v = super::parse_reviews(&html);
        for r in &v {
            println!("{} | {} | {} | {} | {}\n  {}", r.positive, r.author, r.hours, r.date, r.helpful_n, r.text.chars().take(120).collect::<String>());
        }
        assert_eq!(v.len(), 10);
    }
}

#[cfg(test)]
mod reviews_live {
    /// Three pages of "Most helpful this year" (Half-Life) - reviews must not repeat.
    #[test]
    #[ignore]
    fn reviews_paging_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cursor = None;
            let mut seen = std::collections::HashSet::new();
            for page in 1..=3 {
                let (v, next) = super::fetch_reviews(70, 1, "russian", page, cursor.clone()).await.unwrap();
                println!("page {page}: {} reviews, first - {}", v.len(), v.first().map(|r| r.author.clone()).unwrap_or_default());
                for r in &v {
                    assert!(seen.insert(r.url.clone()), "repeat: {}", r.url);
                }
                cursor = next;
            }
        });
    }
}

/// Steam charts: (name, explanation).
#[allow(non_snake_case)]
pub fn CHARTS() -> [(&'static str, &'static str); 3] {
    [
    (tr!("Лидеры продаж", "Top sellers"), tr!("Самые продаваемые сейчас в вашем регионе (по выручке, как на странице Steam).", "The best sellers right now in your region (by revenue, as on the Steam page).")),
    (tr!("Больше всего игроков", "Most played"), tr!("Игры, в которые прямо сейчас играет больше всего людей.", "Games that the most people are playing right now.")),
    (tr!("Популярные новинки", "Popular new releases"), tr!("Лидеры продаж, вышедшие за последние 3 месяца.", "Top sellers released in the last 3 months.")),
]
}

/// appid from a store image URL (.../steam/apps/<appid>/...); None for bundles and packages.
fn appid_from_logo(url: &str) -> Option<u32> {
    let rest = url.split("/apps/").nth(1)?;
    rest.split('/').next()?.parse().ok()
}

/// Chart list: (appid, label). Public Steam store requests, no login.
pub async fn fetch_chart(kind: u8, cc: &str) -> Result<Vec<(u32, String)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    if kind == 1 {
        let url = "https://api.steampowered.com/ISteamChartsService/GetGamesByConcurrentPlayers/v1/";
        let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
        let ranks = v.pointer("/response/ranks").and_then(|r| r.as_array()).ok_or(tr!("Steam не прислал чарт", "Steam did not send the chart"))?;
        return Ok(ranks
            .iter()
            .filter_map(|r| {
                let id = r.get("appid")?.as_u64()? as u32;
                let now = r.get("concurrent_in_game").and_then(|x| x.as_u64()).unwrap_or(0);
                Some((id, trf!("сейчас играют {}", "{} playing now", group_digits(now))))
            })
            .collect());
    }
    if kind == 2 {
        // Steam search does not apply the "new releases" filter - take top sellers released within 3 months.
        let since = chrono::Utc::now().timestamp() - 92 * 86_400;
        let mut out = Vec::new();
        for start in [0, 100, 200] {
            for (id, date) in topsellers_with_dates(cc, start).await? {
                if date.is_some_and(|d| d >= since) && !out.iter().any(|(a, _)| *a == id) {
                    use chrono::TimeZone;
                    let when = chrono::Utc.timestamp_opt(date.unwrap(), 0).single().map(|t| t.format("%d.%m.%Y").to_string());
                    out.push((id, trf!("вышла {}", "released {}", when.unwrap_or_default())));
                }
            }
        }
        return Ok(out);
    }
    let filter = "topsellers";
    let url = format!("https://store.steampowered.com/search/results/?filter={filter}&json=1&cc={cc}&l={}&count=100", crate::lang::steam());
    let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
    let items = v.get("items").and_then(|i| i.as_array()).ok_or(tr!("Steam не прислал чарт", "Steam did not send the chart"))?;
    let mut seen = HashSet::new();
    Ok(items
        .iter()
        .filter_map(|i| appid_from_logo(i.get("logo")?.as_str()?))
        .filter(|id| seen.insert(*id))
        .enumerate()
        .map(|(n, id)| (id, format!("№ {}", n + 1)))
        .collect())
}

/// "12 Nov, 2026" -> unix time (midnight UTC); None if the date is not exact ("Coming soon", "Q1 2027").
pub(crate) fn parse_en_date(s: &str) -> Option<i64> {
    let s = s.replace(',', "");
    let mut it = s.split_whitespace();
    let day: u32 = it.next()?.parse().ok()?;
    let m = it.next()?;
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|x| *x == m)? as u32
        + 1;
    let year: i32 = it.next()?.parse().ok()?;
    let d = chrono::NaiveDate::from_ymd_opt(year, mon, day)?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// Top sellers with release dates (store search HTML results): (appid, release date).
async fn topsellers_with_dates(cc: &str, start: u32) -> Result<Vec<(u32, Option<i64>)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let url = format!(
        "https://store.steampowered.com/search/results/?filter=topsellers&infinite=1&cc={cc}&l=english&count=100&start={start}"
    );
    let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
    let html = v.get("results_html").and_then(|h| h.as_str()).ok_or(tr!("Steam не прислал список", "Steam did not send the list"))?;
    Ok(html
        .split("data-ds-appid=\"")
        .skip(1)
        .filter_map(|chunk| {
            let id: u32 = chunk.split('"').next()?.parse().ok()?; // bundles have a comma-separated list here
            let date = chunk
                .split("search_released")
                .nth(1)
                .and_then(|r| r.split('>').nth(1))
                .and_then(|r| r.split('<').next())
                .and_then(|d| parse_en_date(d.trim()));
            Some((id, date))
        })
        .collect())
}

/// 1234567 -> «1 234 567».
pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod chart_tests {
    use super::*;

    #[test]
    fn logo_and_digits() {
        assert_eq!(appid_from_logo("https://x/store_item_assets/steam/apps/227300/da84/capsule_sm_120.jpg?t=1"), Some(227300));
        assert_eq!(appid_from_logo("https://x/steam/bundles/123/capsule.jpg"), None);
        assert_eq!(group_digits(1234567), "1 234 567");
        assert_eq!(group_digits(999), "999");
    }

    #[test]
    #[ignore]
    fn charts_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            for k in 0..3u8 {
                let v = fetch_chart(k, "GE").await.unwrap();
                println!("{} : {} games, first {:?}", CHARTS()[k as usize].0, v.len(), v.first());
                assert!(v.len() >= 5, "{}", v.len());
            }
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let q = c.discovery_queue("GE", false).await.unwrap();
            println!("discovery queue: {} games", q.len());
            assert!(!q.is_empty());
        });
    }
}

#[cfg(test)]
mod date_tests {
    #[test]
    fn en_dates() {
        assert!(super::parse_en_date("12 Nov, 2026").is_some());
        assert!(super::parse_en_date("Coming soon").is_none());
        assert!(super::parse_en_date("Q1 2027").is_none());
    }
}

#[cfg(test)]
mod reviews_rich_live {
    /// Live test (read only): reviews with awards, counters and avatars; full text and review id.
    #[test]
    #[ignore]
    fn reviews_rich_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (v, _) = super::fetch_reviews(550, 0, "russian", 1, None).await.unwrap();
            println!(
                "reviews {}, with avatar {}, with awards {}, first helpful {}, funny {}",
                v.len(),
                v.iter().filter(|r| !r.avatar.is_empty()).count(),
                v.iter().filter(|r| !r.awards.is_empty()).count(),
                v[0].helpful_n,
                v[0].funny_n
            );
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (text, id, at) = super::review_page(&c, &v[0].url).await.unwrap();
            println!("hours at review time: {at}");
            println!("full text {} chars (in the list {}), has id: {}", text.chars().count(), v[0].text.chars().count(), id > 0);
            let (en, _) = super::fetch_reviews(550, 7, "english", 1, None).await.unwrap();
            println!("funny in English: {}", en.len());
        });
    }
}

/// In a short news text Steam strips markup without spaces, and paragraphs stick together:
/// "is out!There are". Put a space back after a sentence end before a capital letter.
pub fn unglue(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut prev: Option<char> = None;
    let mut prev2: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            // "U.S.A" and "т.е." are left alone: a single capital letter before the dot with no space.
            let abbrev = p == '.' && prev2.is_some_and(|q| q.is_uppercase());
            if matches!(p, '.' | '!' | '?') && c.is_uppercase() && !abbrev {
                out.push(' ');
            }
        }
        out.push(c);
        prev2 = prev;
        prev = Some(c);
    }
    out
}

#[cfg(test)]
mod unglue_tests {
    #[test]
    fn splits_sentences() {
        assert_eq!(super::unglue("is out!There are. Merch!Plus"), "is out! There are. Merch! Plus");
        assert_eq!(super::unglue("U.S.A version 1.2"), "U.S.A version 1.2");
        assert_eq!(super::unglue("Готово.Дальше"), "Готово. Дальше");
    }
}

/// "Not interested" in the Steam store: hide a game from recommendations (on) or bring it back.
async fn ignore(c: &SteamClient, appid: u32, on: bool) -> Result<(), String> {
    let web = crate::webapi::web_session(c).await?;
    let fields = [
        ("sessionid", web.sessionid().to_string()),
        ("appid", appid.to_string()),
        ("remove", if on { "0" } else { "1" }.to_string()),
        ("snr", "1_5_9__403".to_string()),
    ];
    match web
        .post_form(
            "https://store.steampowered.com/recommended/ignorerecommendation/",
            &fields,
            &format!("https://store.steampowered.com/app/{appid}/"),
        )
        .await
    {
        Ok((200, _)) => Ok(()),
        Ok((status, _)) => Err(trf!("Steam ответил кодом {status}", "Steam answered with code {status}")),
        Err(e) => Err(trf!("нет связи со Steam: {e:#}", "no connection to Steam: {e:#}")),
    }
}

/// Games hidden from recommendations: dynamicstore/userdata (rgIgnoredApps).
async fn ignored_list(c: &SteamClient) -> Result<HashSet<u32>, String> {
    let web = crate::webapi::web_session(c).await?;
    let text = web
        .get_text("https://store.steampowered.com/dynamicstore/userdata/")
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(parse_ignored(&v))
}

fn parse_ignored(v: &serde_json::Value) -> HashSet<u32> {
    // rgIgnoredApps can be an object {"appid": 0} or an array.
    match &v["rgIgnoredApps"] {
        serde_json::Value::Object(m) => m.keys().filter_map(|k| k.parse().ok()).collect(),
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect(),
        _ => HashSet::new(),
    }
}

#[cfg(test)]
mod ignored_tests {
    #[test]
    fn parses_both_forms() {
        let v: serde_json::Value = serde_json::from_str(r#"{"rgIgnoredApps":{"10":0,"20":0}}"#).unwrap();
        assert_eq!(super::parse_ignored(&v).len(), 2);
        let v: serde_json::Value = serde_json::from_str(r#"{"rgIgnoredApps":[30]}"#).unwrap();
        assert!(super::parse_ignored(&v).contains(&30));
    }

    #[test]
    fn appdetails_extras() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"70":{"success":true,"data":{"pc_requirements":[],"genres":[{"id":"1","description":"Экшены"}],
            "support_info":{"url":"https://help.example.com","email":""},
            "movies":[{"id":1,"mp4":{"480":"http://x/480.mp4","max":"http://x/max.mp4"}}]}}}"#,
        )
        .unwrap();
        let d = super::parse_appdetails(&v, 70);
        assert_eq!(d.genres, vec!["Экшены"]);
        assert_eq!(d.trailer.as_deref(), Some("http://x/max.mp4"));
        assert_eq!(d.support.as_deref(), Some("https://help.example.com"));
        assert!(d.requirements.is_none());
    }
}

/// Store package name (packagedetails, no login).
async fn package_name(id: u32) -> Result<(String, Option<u32>), String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let url = format!("https://store.steampowered.com/api/packagedetails?packageids={id}&l={}", crate::lang::steam());
    let v: serde_json::Value = http.get(url).send().await.map_err(|e| format!("{e:#}"))?.json().await.map_err(|e| format!("{e:#}"))?;
    let d = &v[id.to_string()]["data"];
    let app = d["apps"].get(0).and_then(|a| a["id"].as_u64()).map(|a| a as u32);
    d["name"].as_str().map(|n| (n.to_string(), app)).ok_or_else(|| tr!("нет данных", "no data").into())
}

/// Wishlist in Steam's order: (priority > 0 ascending), then by date added.
pub async fn wishlist_ranked(c: &SteamClient, me: u64) -> Result<Vec<u32>, String> {
    let v = crate::webapi::get(c, "IWishlistService/GetWishlist/v1", &[("steamid", me.to_string())]).await?;
    let num = |x: &serde_json::Value| x.as_u64().or_else(|| x.as_str()?.parse().ok()).unwrap_or(0);
    let mut items: Vec<(u64, u64, u32)> = v["items"]
        .as_array()
        .ok_or(tr!("нет списка", "no list"))?
        .iter()
        .map(|i| (num(&i["priority"]), num(&i["date_added"]), num(&i["appid"]) as u32))
        .collect();
    items.sort_by_key(|(p, d, _)| (*p == 0, *p, *d));
    Ok(items.into_iter().map(|(_, _, a)| a).collect())
}

#[cfg(test)]
mod review_paging_tests {
    use super::{Review, Reviews};

    fn review(text: &str) -> Review {
        Review {
            positive: true,
            hours: String::new(),
            date: String::new(),
            author: String::new(),
            text: text.into(),
            url: String::new(),
            avatar: String::new(),
            helpful_n: 0,
            funny_n: 0,
            awards: Vec::new(),
            products: String::new(),
        }
    }

    /// A language change starts over: no old cursor, no old reviews, older answers dropped.
    #[test]
    fn language_change_resets_paging() {
        let mut r = Reviews::default();
        let (p, first) = r.begin(0, "russian", 1).unwrap();
        assert_eq!(p, 1);
        // The Russian page arrived.
        r.loading = false;
        r.pages = 1;
        r.cursor = Some("ru-cursor".into());
        r.list.push(review("in Russian"));
        // A "next page" of other criteria (left over from the frame where English was chosen) is
        // dropped: it must not switch the choice back.
        assert!(r.begin(0, "english", 2).is_none());
        assert_eq!(r.lang, "russian");
        // English chosen: page 1 of English, without the old cursor.
        let (p, second) = r.begin(0, "english", 1).unwrap();
        assert_eq!(p, 1);
        assert!(r.cursor.is_none() && r.list.is_empty());
        assert_eq!(r.lang, "english");
        assert_ne!(first, second, "an answer to the Russian request is dropped");
        // While English loads, the same request is not repeated, but a switch back is a new one.
        assert!(r.begin(0, "english", 1).is_none());
        let (_, third) = r.begin(0, "russian", 1).unwrap();
        assert_ne!(third, second);
    }

    /// Request numbers are unique for the run: after the cache is dropped (a language change), a new
    /// object does not start from 1 again and cannot take an old answer for its own.
    #[test]
    fn numbers_survive_cache_drop() {
        let mut old = Reviews::default();
        let (_, a) = old.begin(0, "russian", 1).unwrap();
        let mut fresh = Reviews::default();
        let (_, b) = fresh.begin(0, "english", 1).unwrap();
        assert_ne!(a, b);
    }

    /// The next page of the same order and language keeps the list and the cursor.
    #[test]
    fn next_page_keeps_cursor() {
        let mut r = Reviews::default();
        r.begin(1, "all", 1).unwrap();
        r.loading = false;
        r.pages = 1;
        r.cursor = Some("c".into());
        r.list.push(review("a"));
        let (p, _) = r.begin(1, "all", 2).unwrap();
        assert_eq!(p, 2);
        assert_eq!(r.cursor.as_deref(), Some("c"));
        assert_eq!(r.list.len(), 1);
    }
}
