//! Магазин Steam: данные и сетевые запросы (через Aurelia).
//!
//! Списки (поиск, скидки, список желаемого) хранят только appid; подробности об играх
//! (`StoreAppInfo`: цена, описание, картинки…) подгружаются лениво - для тех игр, что
//! видны в окне, порциями по 50. Все лежит в отдельном Arc<Mutex<StoreState>>, а не в
//! NetState, который копируется на каждом кадре.

use aurelia::steam_client::{DealsScope, SteamClient, StoreAppInfo};
use aurelia::web::discovery::NewsItem;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Язык текстов магазина.
pub const LANG: &str = "russian";

/// Регионы цен, которые можно выбрать (код страны, название).
pub const COUNTRIES: [(&str, &str); 10] = [
    ("UA", "Украина (₴)"),
    ("RU", "Россия (₽)"),
    ("KZ", "Казахстан (₸)"),
    ("BY", "Беларусь"),
    ("GE", "Грузия ($)"),
    ("PL", "Польша (zł)"),
    ("DE", "Германия (€)"),
    ("TR", "Турция ($)"),
    ("US", "США ($)"),
    ("GB", "Великобритания (£)"),
];

#[derive(Default)]
pub struct Extras {
    pub players: Option<i32>,
    pub news: Option<Result<Vec<NewsItem>, String>>,
    pub similar: Option<Vec<u32>>,
    /// Системные требования (Windows): минимальные и рекомендуемые, текстом.
    pub requirements: Option<(String, String)>,
    /// Отзывы со страницы сообщества (подгружаются по кнопке).
    pub reviews: Option<Reviews>,
}

/// Отзывы игры: выбранный порядок и язык, загруженные страницы.
#[derive(Default, Clone)]
pub struct Reviews {
    /// Порядок (номер в REVIEW_SORTS).
    pub sort: u8,
    /// false - на русском, true - на всех языках.
    pub lang: u8,
    pub pages: u32,
    pub list: Vec<Review>,
    pub more: bool,
    /// Курсор следующей страницы (Steam листает отзывы по нему).
    pub cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
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
    /// Сколько посчитали полезным / забавным.
    pub helpful_n: u32,
    pub funny_n: u32,
    /// Награды (реакции за очки Steam): (номер реакции, сколько).
    pub awards: Vec<(u32, u32)>,
    /// «Продуктов на аккаунте: N».
    pub products: String,
}

#[derive(Default)]
pub struct StoreState {
    /// Страна цен; пусто - еще не определена.
    pub country: String,
    /// Подробности об играх по appid (для текущей страны).
    pub apps: HashMap<u32, StoreAppInfo>,
    /// Уже запрошенные подробности (чтобы не просить повторно).
    pub requested: HashSet<u32>,
    /// Поиск: (запрос, результат).
    pub search: Option<(String, Result<Vec<u32>, String>)>,
    pub searching: bool,
    pub deals: Option<Result<Vec<u32>, String>>,
    /// Свой список желаемого, по приоритету.
    pub wishlist: Option<Result<Vec<u32>, String>>,
    /// События и распродажи Steam: (appid, что за событие, до какого времени).
    pub events: Option<Result<Vec<(u32, String, u64)>, String>>,
    /// Доп. данные страницы игры: онлайн, новости, отзывы, похожие.
    pub extras: HashMap<u32, Extras>,
    /// Ошибка последнего действия (например, добавления в желаемое).
    pub error: Option<String>,
    /// Список желаемого друга: (SteamID, имя, список по приоритету).
    pub friend_wishlist: Option<(u64, String, Option<Result<Vec<u32>, String>>)>,
    /// Скидки на игры из списка желаемого (для уведомлений); растет sales_gen при каждой проверке.
    pub sales: Vec<Sale>,
    pub sales_gen: u64,
    /// Отзывы по ссылке: полный текст и номер; итог оценки.
    pub review_full: HashMap<String, Result<(String, u64, String), String>>,
    /// Свои оценки отзывов в этом сеансе: ссылка -> 0 полезен, 1 бесполезен, 2 забавный.
    pub review_mine: HashMap<String, u8>,
    pub review_vote: HashMap<String, String>,
    /// Чарты: номер -> (appid, пометка).
    pub charts: HashMap<u8, Result<Vec<(u32, String)>, String>>,
    /// Очередь открытий.
    pub discovery: Option<Result<Vec<u32>, String>>,
}

/// Игра из списка желаемого со скидкой.
#[derive(Clone)]
pub struct Sale {
    pub appid: u32,
    pub name: String,
    pub pct: i32,
    /// Цена со скидкой (текстом, в валюте региона).
    pub price: String,
    /// Конец скидки (unix), 0 - неизвестен.
    pub end: u64,
}

pub type SharedStore = Arc<Mutex<StoreState>>;

/// Команды магазина (обрабатываются в сетевом потоке).
pub enum StoreCmd {
    Search(String),
    Deals,
    /// События и распродажи (скидки дня/выходных, выход игр, обновления).
    Events,
    Wishlist,
    /// Проверить скидки на игры из списка желаемого (для уведомлений).
    WishlistSales,
    /// Список желаемого друга (если он открыт настройками приватности).
    FriendWishlist(u64),
    /// Отзывы игры: порядок (REVIEW_SORTS), все языки?, страница (с 1).
    Reviews { appid: u32, sort: u8, lang: u8, page: u32 },
    /// Полный текст отзыва (по его ссылке) и оценка отзыва (0 полезен, 1 бесполезен, 2 забавный).
    ReviewFull(String),
    ReviewVote { url: String, kind: u8 },
    /// Подробности для этих игр (лениво, для видимых в списке).
    Details(Vec<u32>),
    /// Страница игры: подробности + онлайн, новости, отзывы, похожие.
    Page(u32),
    /// Добавить в список желаемого (true) или убрать.
    WishlistSet(u32, bool),
    /// Сменить регион цен (сбрасывает подробности).
    Country(String),
    /// Чарт Steam (номер в CHARTS).
    Charts(u8),
    /// Очередь открытий: отметить просмотренными `skip`, `rebuild` - собрать новую.
    Discovery { rebuild: bool, skip: Vec<u32> },
}

/// Русское название вида события Steam (MarketingMessage).
pub fn event_kind(kind: &str) -> String {
    match kind {
        "MidweekDeal" => "скидка середины недели",
        "WeekendDeal" => "скидка на выходные",
        "DailyDeal" => "скидка дня",
        "NowAvailable" => "уже вышла",
        "Update" => "обновление",
        "FreeWeekend" => "бесплатные выходные",
        "FreeToPlay" => "бесплатно",
        "Sale" | "PublisherSale" | "FranchiseSale" => "распродажа",
        "DLCNowAvailable" => "новое дополнение",
        "PreOrder" | "Preorder" => "предзаказ",
        other => return other.to_string(),
    }
    .to_string()
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Определить страну цен, если еще не задана (по IP подключения к Steam).
/// Регион цен: выбранный вручную, иначе страна кошелька Steam (регион аккаунта), иначе страна
/// подключения (по IP - в поездке она другая).
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

/// Загрузить подробности для игр, которых еще нет в кэше (порциями по 50).
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
        if let Ok(apps) = client.fetch_store_apps(chunk, LANG, &cc).await {
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
                let res = c.search_store(&term, 30, LANG, &cc).await.map(|p| p.app_ids).map_err(err);
                if let Ok(ids) = &res {
                    load_details(&c, ids.clone(), &st, &ctx).await;
                }
                let mut s = st.lock().unwrap();
                // Пока искали, могли ввести новый запрос - оставляем только свежий.
                if s.search.as_ref().is_some_and(|(t, _)| *t == term) {
                    s.search = Some((term, res));
                    s.searching = false;
                }
            }
            StoreCmd::Deals => {
                let cc = country(&c, &st).await;
                let res = c
                    .query_deals(DealsScope::DiscountedTopSellers, 1, 0, 50, LANG, &cc)
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
                let res = fetch_chart(kind, &cc).await;
                if let Ok(v) = &res {
                    load_details(&c, v.iter().take(20).map(|e| e.0).collect(), &st, &ctx).await;
                }
                st.lock().unwrap().charts.insert(kind, res);
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
                let res = match c.my_steam_id() {
                    Some(me) => c.wishlist(me).await.map(|w| w.into_iter().map(|e| e.app_id).collect()).map_err(err),
                    None => Err("нет входа в Steam".into()),
                };
                st.lock().unwrap().wishlist = Some(res);
            }
            StoreCmd::WishlistSales => {
                let Some(me) = c.my_steam_id() else { return };
                let Ok(list) = c.wishlist(me).await else { return };
                let ids: Vec<u32> = list.into_iter().map(|e| e.app_id).collect();
                let cc = country(&c, &st).await;
                let mut sales = Vec::new();
                // Свежие цены (без кэша страницы магазина): скидки меняются.
                for chunk in ids.chunks(50) {
                    if let Ok(apps) = c.fetch_store_apps(chunk, LANG, &cc).await {
                        for a in apps {
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
                s.sales_gen += 1;
                drop(s);
                ctx.request_repaint();
            }
            StoreCmd::FriendWishlist(id) => {
                let res = c
                    .wishlist(id)
                    .await
                    .map(|w| w.into_iter().map(|e| e.app_id).collect::<Vec<u32>>())
                    .map_err(|e| format!("список закрыт настройками приватности или недоступен ({})", err(e)));
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
            StoreCmd::Page(appid) => {
                st.lock().unwrap().extras.entry(appid).or_default();
                ctx.request_repaint();
                load_details(&c, vec![appid], &st, &ctx).await;
                let cc = country(&c, &st).await;
                let players = c.current_players(appid).await.ok();
                let similar = c.similar_apps(appid, 8, LANG, &cc).await.map(|p| p.app_ids).ok();
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
                // Отзывы отдельно не грузим: store.steampowered.com/appreviews сейчас отвечает
                // перенаправлением на главную (проверено 2026-11-12). Итог по отзывам приходит
                // в StoreAppInfo.review_summary, полные отзывы - по ссылке в браузере.
                let news = match aurelia::core::net::steam_web_client() {
                    Ok(http) => aurelia::web::discovery::fetch_news(&http, appid, 5, 600).await.map_err(err),
                    Err(e) => Err(err(e)),
                };
                st.lock().unwrap().extras.entry(appid).or_default().news = Some(news);
                // Системные требования - из открытого API магазина (api/appdetails), по-русски.
                if let Some(req) = fetch_requirements(appid).await {
                    st.lock().unwrap().extras.entry(appid).or_default().requirements = Some(req);
                }
                ctx.request_repaint();
            }
            StoreCmd::Reviews { appid, sort, lang, page } => {
                {
                    let mut s = st.lock().unwrap();
                    let r = s.extras.entry(appid).or_default().reviews.get_or_insert_with(Default::default);
                    // Тот же запрос уже идет (раздел открыт, ответа еще нет) - не повторяем.
                    if r.loading && r.sort == sort && r.lang == lang {
                        return;
                    }
                    // Сменили порядок или язык - начинаем заново.
                    if page == 1 || r.sort != sort || r.lang != lang {
                        *r = Reviews { sort, lang, ..Default::default() };
                    }
                    r.loading = true;
                }
                ctx.request_repaint();
                let cursor = st.lock().unwrap().extras.get(&appid).and_then(|e| e.reviews.as_ref()).and_then(|r| r.cursor.clone());
                let res = fetch_reviews(appid, sort, lang, page, cursor).await;
                let mut s = st.lock().unwrap();
                let r = s.extras.entry(appid).or_default().reviews.get_or_insert_with(Default::default);
                if r.sort != sort || r.lang != lang {
                    return; // пока грузили, выбрали другой порядок или язык
                }
                r.loading = false;
                match res {
                    Ok((v, next)) => {
                        r.more = v.len() >= 10 && next.is_some() && REVIEW_SORTS[sort as usize].0 != "toprated";
                        r.cursor = next;
                        r.pages = page;
                        r.list.extend(v);
                        r.error = None;
                    }
                    Err(e) => r.error = Some(e),
                }
            }
            StoreCmd::ReviewFull(url) => {
                let r = review_page(&c, &url).await;
                st.lock().unwrap().review_full.insert(url, r);
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
                    Err(e) => s.error = Some(format!("Список желаемого не изменен: {}", err(e))),
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

/// Кусок описания игры для отображения.
#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    Para(String),
    Bullet(String),
}

/// Описание из магазина в блоки. Понимает и HTML (`<h2>`, `<p>`, `<li>`), и BBCode Steam
/// (`[h2]`, `[p]`, `[*]`, `[list]`); картинки и видео (`[img]`, `[video]`, `<img>`) пропускает.
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
            // Не тег (например, «[18+]» в тексте) - оставляем как есть.
            buf.push(open);
            rest = after;
            continue;
        }
        rest = &after[end + 1..];
        match (closing, name.as_str()) {
            // Картинки и видео: в BBCode пропускаем вместе с содержимым до закрывающего тега.
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
            _ => {} // оформление (b, i, url...) - сам тег убираем, текст оставляем
        }
    }
    flush(&mut buf, mode, &mut blocks);
    blocks
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace('\u{a0}', " ")
}

/// Описание или новость одной строкой (для кратких превью).
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

/// Маленькая картинка игры для списков (120x45, ~5 КБ).
/// Уменьшенный скриншот (600x338) вместо полного: в ~7 раз легче.
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
    /// Живой тест (только чтение): события и распродажи Steam для региона UA.
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

/// Системные требования для Windows (минимальные, рекомендуемые) из store.steampowered.com/api/appdetails.
async fn fetch_requirements(appid: u32) -> Option<(String, String)> {
    let http = aurelia::core::net::steam_web_client().ok()?;
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=russian");
    let text = http.get(url).send().await.ok()?.text().await.ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let req = v.get(appid.to_string())?.get("data")?.get("pc_requirements")?;
    // Нет требований - Steam присылает пустой массив вместо объекта.
    let get = |k: &str| req.get(k).and_then(|x| x.as_str()).map(plain_text).unwrap_or_default();
    let (min, rec) = (get("minimum"), get("recommended"));
    (!min.is_empty() || !rec.is_empty()).then_some((min, rec))
}

/// Порядок отзывов, как на сайте Steam: (browsefilter, подпись).
pub const REVIEW_SORTS: [(&str, &str); 8] = [
    ("toprated", "Полезные за все время"),
    ("trendyear", "Полезные за год"),
    ("trendsixmonths", "Полезные за полгода"),
    ("trendthreemonths", "Полезные за 3 месяца"),
    ("trendmonth", "Полезные за месяц"),
    ("trendweek", "Полезные за неделю"),
    ("mostrecent", "Новые"),
    ("funny", "Забавные"),
];

/// Языки отзывов: (код Steam, название).
pub const REVIEW_LANGS: [(&str, &str); 13] = [
    ("russian", "На русском"),
    ("all", "На всех языках"),
    ("english", "Английский"),
    ("ukrainian", "Украинский"),
    ("german", "Немецкий"),
    ("french", "Французский"),
    ("spanish", "Испанский"),
    ("polish", "Польский"),
    ("brazilian", "Португальский (Бразилия)"),
    ("turkish", "Турецкий"),
    ("schinese", "Китайский"),
    ("japanese", "Японский"),
    ("koreana", "Корейский"),
];

/// Страница отзывов из сообщества Steam (как ее подгружает сам сайт при прокрутке): по 10 отзывов.
/// Следующие страницы - по курсору из формы MoreContentForm предыдущего ответа.
/// «Полезные за все время» без входа Steam не листает (всегда первые 10) - проверено.
async fn fetch_reviews(
    appid: u32,
    sort: u8,
    lang: u8,
    page: u32,
    cursor: Option<String>,
) -> Result<(Vec<Review>, Option<String>), String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let filter = REVIEW_SORTS.get(sort as usize).map_or("toprated", |s| s.0);
    let lang = REVIEW_LANGS.get(lang as usize).map_or("russian", |l| l.0);
    let p = page.max(1);
    let cursor = match (p, cursor) {
        (1, _) => String::new(),
        (_, Some(c)) => format!("&userreviewscursor={}", urlenc(&c)),
        (_, None) => return Ok((Vec::new(), None)),
    };
    let url = format!(
        "https://steamcommunity.com/app/{appid}/homecontent/?userreviewsoffset={off}&p={p}{cursor}&numperpage=10\
         &browsefilter={filter}&appid={appid}&appHubSubSection=10&l=russian&filterLanguage={lang}&searchText=\
         &maxInappropriateScore=50",
        off = (p - 1) * 10
    );
    let resp = http.get(url).send().await.map_err(err)?;
    if !resp.status().is_success() {
        return Err(format!("Steam ответил {}", resp.status()));
    }
    let html = resp.text().await.map_err(err)?;
    let next = between(&html, "name=\"userreviewscursor\" value=\"", "\"").map(str::to_string);
    Ok((parse_reviews(&html), next))
}

/// Страница отзыва: полный текст и номер отзыва (для оценок).
pub async fn review_page(client: &SteamClient, url: &str) -> Result<(String, u64, String), String> {
    let web = crate::webapi::web_session(client).await?;
    let html = web.get_text(&format!("{}?l=russian", url.trim_end_matches('/'))).await.map_err(err)?;
    let text = between(&html, "id=\"ReviewText\">", "</div>").map(strip_html).unwrap_or_default();
    let id = between(&html, "RecommendationVoteUpBtn", "\"")
        .and_then(|n| n.parse().ok())
        .ok_or("Steam не дал номер отзыва")?;
    Ok((text, id, hours_at_review(&html)))
}

/// «(42.0 ч. в момент написания)» со страницы отзыва -> «42.0 ч. на момент отзыва».
pub fn hours_at_review(html: &str) -> String {
    let Some(pos) = html.find("в момент написания") else { return String::new() };
    let before = &html[..pos];
    let start = before.rfind('(').map(|i| i + 1).unwrap_or(pos);
    let h = before[start..].trim();
    if h.is_empty() || h.len() > 40 { String::new() } else { format!("{h} на момент отзыва") }
}

/// Оценить отзыв: 0 - полезен, 1 - бесполезен, 2 - забавный.
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
        Err(format!("Steam не принял оценку (код {status})"))
    }
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// &quot; &amp; и т.п. -> символы.
fn entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// HTML-кусок -> текст: <br> -> перевод строки, теги убираются.
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

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

/// Разбор карточек отзывов (apphub_Card) со страницы сообщества.
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
            // Хвост карточки: закрывающие </div> текста и карточки.
            let text = strip_html(text.trim_end().trim_end_matches("</div>"));
            let author = between(card, "apphub_CardContentAuthorName", "</a>")
                .and_then(|a| a.rsplit_once('>').map(|(_, n)| entities(n.trim())))
                .unwrap_or_default();
            let num_after = |key: &str| -> u32 {
                card.split(key).nth(1).map(|s| s.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == ',').filter(|c| c.is_ascii_digit()).collect::<String>()).and_then(|n| n.parse().ok()).unwrap_or(0)
            };
            let helpful_n = num_after("полезным:");
            let funny_n = num_after("забавным:");
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
    /// VAPORUST_DUMP=страница отзывов (сохраненная) - разбор карточек.
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
    /// Три страницы «Полезные за год» (Half-Life) - отзывы не должны повторяться.
    #[test]
    #[ignore]
    fn reviews_paging_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cursor = None;
            let mut seen = std::collections::HashSet::new();
            for page in 1..=3 {
                let (v, next) = super::fetch_reviews(70, 1, 0, page, cursor.clone()).await.unwrap();
                println!("стр. {page}: {} отзывов, первый - {}", v.len(), v.first().map(|r| r.author.clone()).unwrap_or_default());
                for r in &v {
                    assert!(seen.insert(r.url.clone()), "повтор: {}", r.url);
                }
                cursor = next;
            }
        });
    }
}

/// Чарты Steam: (название, пояснение).
pub const CHARTS: [(&str, &str); 3] = [
    ("Лидеры продаж", "Самые продаваемые сейчас в вашем регионе (по выручке, как на странице Steam)."),
    ("Больше всего игроков", "Игры, в которые прямо сейчас играет больше всего людей."),
    ("Популярные новинки", "Лидеры продаж, вышедшие за последние 3 месяца."),
];

/// appid из адреса картинки магазина (…/steam/apps/<appid>/…); у наборов и комплектов - None.
fn appid_from_logo(url: &str) -> Option<u32> {
    let rest = url.split("/apps/").nth(1)?;
    rest.split('/').next()?.parse().ok()
}

/// Список чарта: (appid, пометка). Открытые запросы магазина Steam, без входа.
pub async fn fetch_chart(kind: u8, cc: &str) -> Result<Vec<(u32, String)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    if kind == 1 {
        let url = "https://api.steampowered.com/ISteamChartsService/GetGamesByConcurrentPlayers/v1/";
        let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
        let ranks = v.pointer("/response/ranks").and_then(|r| r.as_array()).ok_or("Steam не прислал чарт")?;
        return Ok(ranks
            .iter()
            .filter_map(|r| {
                let id = r.get("appid")?.as_u64()? as u32;
                let now = r.get("concurrent_in_game").and_then(|x| x.as_u64()).unwrap_or(0);
                Some((id, format!("сейчас играют {}", group_digits(now))))
            })
            .collect());
    }
    if kind == 2 {
        // Фильтр «новинки» поиск Steam не применяет - берем лидеров продаж, вышедших за 3 месяца.
        let since = chrono::Utc::now().timestamp() - 92 * 86_400;
        let mut out = Vec::new();
        for start in [0, 100, 200] {
            for (id, date) in topsellers_with_dates(cc, start).await? {
                if date.is_some_and(|d| d >= since) && !out.iter().any(|(a, _)| *a == id) {
                    use chrono::TimeZone;
                    let when = chrono::Utc.timestamp_opt(date.unwrap(), 0).single().map(|t| t.format("%d.%m.%Y").to_string());
                    out.push((id, format!("вышла {}", when.unwrap_or_default())));
                }
            }
        }
        return Ok(out);
    }
    let filter = "topsellers";
    let url = format!("https://store.steampowered.com/search/results/?filter={filter}&json=1&cc={cc}&l=russian&count=100");
    let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
    let items = v.get("items").and_then(|i| i.as_array()).ok_or("Steam не прислал чарт")?;
    let mut seen = HashSet::new();
    Ok(items
        .iter()
        .filter_map(|i| appid_from_logo(i.get("logo")?.as_str()?))
        .filter(|id| seen.insert(*id))
        .enumerate()
        .map(|(n, id)| (id, format!("№ {}", n + 1)))
        .collect())
}

/// «12 Nov, 2026» -> unix-время (полночь UTC); None, если дата не точная («Coming soon», «Q1 2027»).
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

/// Лидеры продаж с датами выхода (HTML-выдача поиска магазина): (appid, дата выхода).
async fn topsellers_with_dates(cc: &str, start: u32) -> Result<Vec<(u32, Option<i64>)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let url = format!(
        "https://store.steampowered.com/search/results/?filter=topsellers&infinite=1&cc={cc}&l=english&count=100&start={start}"
    );
    let v: serde_json::Value = http.get(url).send().await.map_err(err)?.json().await.map_err(err)?;
    let html = v.get("results_html").and_then(|h| h.as_str()).ok_or("Steam не прислал список")?;
    Ok(html
        .split("data-ds-appid=\"")
        .skip(1)
        .filter_map(|chunk| {
            let id: u32 = chunk.split('"').next()?.parse().ok()?; // у наборов здесь список через запятую
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
                println!("{} : {} игр, первая {:?}", CHARTS[k as usize].0, v.len(), v.first());
                assert!(v.len() >= 5, "{}", v.len());
            }
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let q = c.discovery_queue("GE", false).await.unwrap();
            println!("очередь открытий: {} игр", q.len());
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
    /// Живой тест (только чтение): отзывы с наградами, счетчиками и аватарами; полный текст и номер отзыва.
    #[test]
    #[ignore]
    fn reviews_rich_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (v, _) = super::fetch_reviews(550, 0, 0, 1, None).await.unwrap();
            println!(
                "отзывов {}, с аватаром {}, с наградами {}, полезно у первого {}, забавно {}",
                v.len(),
                v.iter().filter(|r| !r.avatar.is_empty()).count(),
                v.iter().filter(|r| !r.awards.is_empty()).count(),
                v[0].helpful_n,
                v[0].funny_n
            );
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (text, id, at) = super::review_page(&c, &v[0].url).await.unwrap();
            println!("часы на момент отзыва: {at}");
            println!("полный текст {} знаков (в списке {}), номер есть: {}", text.chars().count(), v[0].text.chars().count(), id > 0);
            let (en, _) = super::fetch_reviews(550, 7, 2, 1, None).await.unwrap();
            println!("забавные на английском: {}", en.len());
        });
    }
}

/// Steam в коротком тексте новости вырезает разметку без пробелов, и абзацы слипаются:
/// «is out!There are». Вернуть пробел после конца предложения перед заглавной буквой.
pub fn unglue(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut prev: Option<char> = None;
    let mut prev2: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            // «U.S.A» и «т.е.» не трогаем: перед точкой одна заглавная буква без пробела.
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
