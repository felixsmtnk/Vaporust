//! Магазин Steam: данные и сетевые запросы (через Aurelia).
//!
//! Списки (поиск, скидки, список желаемого) хранят только appid; подробности об играх
//! (`StoreAppInfo`: цена, описание, картинки…) подгружаются лениво — для тех игр, что
//! видны в окне, порциями по 50. Всё лежит в отдельном Arc<Mutex<StoreState>>, а не в
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
}

#[derive(Default)]
pub struct StoreState {
    /// Страна цен; пусто — ещё не определена.
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
    /// Доп. данные страницы игры: онлайн, новости, отзывы, похожие.
    pub extras: HashMap<u32, Extras>,
    /// Ошибка последнего действия (например, добавления в желаемое).
    pub error: Option<String>,
}

pub type SharedStore = Arc<Mutex<StoreState>>;

/// Команды магазина (обрабатываются в сетевом потоке).
pub enum StoreCmd {
    Search(String),
    Deals,
    Wishlist,
    /// Подробности для этих игр (лениво, для видимых в списке).
    Details(Vec<u32>),
    /// Страница игры: подробности + онлайн, новости, отзывы, похожие.
    Page(u32),
    /// Добавить в список желаемого (true) или убрать.
    WishlistSet(u32, bool),
    /// Сменить регион цен (сбрасывает подробности).
    Country(String),
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Определить страну цен, если ещё не задана (по IP подключения к Steam).
fn country(client: &SteamClient, st: &SharedStore) -> String {
    let mut s = st.lock().unwrap();
    if s.country.is_empty() {
        s.country = client.store_country();
    }
    s.country.clone()
}

/// Загрузить подробности для игр, которых ещё нет в кэше (порциями по 50).
async fn load_details(client: &SteamClient, ids: Vec<u32>, st: &SharedStore, ctx: &egui::Context) {
    let cc = country(client, st);
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
                let cc = country(&c, &st);
                let res = c.search_store(&term, 30, LANG, &cc).await.map(|p| p.app_ids).map_err(err);
                if let Ok(ids) = &res {
                    load_details(&c, ids.clone(), &st, &ctx).await;
                }
                let mut s = st.lock().unwrap();
                // Пока искали, могли ввести новый запрос — оставляем только свежий.
                if s.search.as_ref().is_some_and(|(t, _)| *t == term) {
                    s.search = Some((term, res));
                    s.searching = false;
                }
            }
            StoreCmd::Deals => {
                let cc = country(&c, &st);
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
            StoreCmd::Wishlist => {
                let res = match c.my_steam_id() {
                    Some(me) => c.wishlist(me).await.map(|w| w.into_iter().map(|e| e.app_id).collect()).map_err(err),
                    None => Err("нет входа в Steam".into()),
                };
                st.lock().unwrap().wishlist = Some(res);
            }
            StoreCmd::Details(ids) => load_details(&c, ids, &st, &ctx).await,
            StoreCmd::Page(appid) => {
                st.lock().unwrap().extras.entry(appid).or_default();
                ctx.request_repaint();
                load_details(&c, vec![appid], &st, &ctx).await;
                let cc = country(&c, &st);
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
                // в StoreAppInfo.review_summary, полные отзывы — по ссылке в браузере.
                let news = match aurelia::core::net::steam_web_client() {
                    Ok(http) => aurelia::web::discovery::fetch_news(&http, appid, 5, 600).await.map_err(err),
                    Err(e) => Err(err(e)),
                };
                st.lock().unwrap().extras.entry(appid).or_default().news = Some(news);
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
                    Err(e) => s.error = Some(format!("Список желаемого не изменён: {}", err(e))),
                }
            }
            StoreCmd::Country(cc) => {
                let mut s = st.lock().unwrap();
                s.country = cc;
                s.apps.clear();
                s.requested.clear();
                s.deals = None;
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
        "td", "th", "spoiler", "noparse", "span", "div", "sup", "sub", "font",
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
            // Не тег (например, «[18+]» в тексте) — оставляем как есть.
            buf.push(open);
            rest = after;
            continue;
        }
        rest = &after[end + 1..];
        match (closing, name.as_str()) {
            // Картинки и видео: в BBCode пропускаем вместе с содержимым до закрывающего тега.
            (false, "img") | (false, "video") if open == '[' => {
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
            _ => {} // оформление (b, i, url...) — сам тег убираем, текст оставляем
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
pub fn small_capsule(appid: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/capsule_sm_120.jpg")
}

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
