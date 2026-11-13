//! Лента событий друзей (как «Активность» на главной сообщества Steam): кто во что впервые
//! сыграл, что купил, достижения, скриншоты, обзоры. Страницы ленты - по дням
//! (/profiles/<я>/ajaxgetusernews/?start=<время>), каждая содержит готовый HTML.

use crate::webapi;
use aurelia::steam_client::SteamClient;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// Заголовок дня («12 ноя»).
    pub day: String,
    pub avatar: String,
    pub text: String,
    /// Первая ссылка на игру в строке (страница магазина по щелчку).
    pub appid: Option<u32>,
    /// Картинки события (скриншоты, значки достижений).
    pub images: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FeedPage {
    pub entries: Vec<Entry>,
    /// С какого времени просить следующую (более старую) страницу.
    pub next: Option<u64>,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

/// Текст без тегов и с раскрытыми &amp; и т.п.
fn text(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ");
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // Теги заменялись пробелом: убрать пробел перед знаками препинания («Игра .» -> «Игра.»).
    let mut fixed = String::with_capacity(out.len());
    for ch in out.chars() {
        if matches!(ch, '.' | ',' | ':' | ';' | '!' | '?' | ')') && fixed.ends_with(' ') {
            fixed.pop();
        }
        fixed.push(ch);
    }
    fixed
}

fn appid_of(s: &str) -> Option<u32> {
    let rest = s.split("steamcommunity.com/app/").nth(1).or_else(|| s.split("store.steampowered.com/app/").nth(1))?;
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

fn images_of(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    for part in s.split("<img").skip(1) {
        let Some(src) = between(part, "src=\"", "\"") else { continue };
        let ok = src.contains("steamusercontent.com") || src.contains("/apps/") || src.contains("/ugc/") || src.contains("achievement");
        if ok && !src.contains("avatars") && !v.iter().any(|x| x == src) && v.len() < 4 {
            v.push(src.replace("&amp;", "&"));
        }
    }
    v
}

/// Разобрать HTML ленты одного запроса.
pub fn parse(html: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for day in html.split("class=\"blotter_day\"").skip(1) {
        let day_name = between(day, "blotter_day_header_date\">", "<").map(|d| d.trim().to_string()).unwrap_or_default();
        // Короткие строки дня («сыграл впервые», «добавил в библиотеку»).
        for line in day.split("class=\"blotter_daily_rollup_line\"").skip(1) {
            let line = line.split("class=\"blotter_daily_rollup_line\"").next().unwrap_or(line);
            let avatar = between(line, "<img src=\"", "\"").unwrap_or("").to_string();
            let span = between(line, "<span>", "</span>").unwrap_or(line);
            let mut t = text(span);
            // Достижения: названия - в подсказках значков после текста («... в Игре:» + значки).
            let after = line.split("</span>").nth(1).unwrap_or("");
            let names: Vec<String> = after
                .split("<img").skip(1)
                .filter_map(|p| between(p, "title=\"", "\""))
                .map(|t| text(t.lines().next().unwrap_or("")))
                .filter(|n| !n.is_empty())
                .collect();
            if t.ends_with(':') {
                if names.is_empty() {
                    t.pop();
                } else {
                    t = format!("{t} {}", names.join(", "));
                }
            }
            if !t.is_empty() {
                out.push(Entry { day: day_name.clone(), avatar, text: t, appid: appid_of(span), images: Vec::new() });
            }
        }
        // Крупные события (скриншоты, достижения, обзоры) - отдельные блоки.
        for block in day.split("class=\"blotter_block\"").skip(1) {
            if block.trim_start().starts_with('>') && block.contains("blotter_daily_rollup") {
                continue;
            }
            let block = block.split("class=\"blotter_block\"").next().unwrap_or(block);
            if block.contains("blotter_daily_rollup") {
                continue;
            }
            let avatar = block
                .split("<img src=\"")
                .skip(1)
                .filter_map(|p| p.split('"').next())
                .find(|s| s.contains("avatars"))
                .unwrap_or("")
                .to_string();
            let head = between(block, "class=\"blotter_author_block\"", "</div>\n\t\t</div>")
                .or_else(|| between(block, "class=\"blotter_author_block\"", "<div class=\"blotter_"))
                .map(|h| text(&format!("<div {h}")))
                .unwrap_or_default();
            let body = text(block.split("<script").next().unwrap_or(block));
            let t = if head.is_empty() { body } else { head };
            let t: String = t.trim_start_matches('>').trim().chars().take(300).collect();
            if !t.is_empty() {
                out.push(Entry { day: day_name.clone(), avatar, text: t, appid: appid_of(block), images: images_of(block) });
            }
        }
    }
    out
}

/// Страница ленты: `start` - время (0 - с сегодняшнего дня).
pub async fn load(client: &SteamClient, me: u64, start: u64) -> Result<FeedPage, String> {
    let web = webapi::web_session(client).await?;
    let mut url = format!("https://steamcommunity.com/profiles/{me}/ajaxgetusernews/?l=russian");
    if start > 0 {
        url.push_str(&format!("&start={start}"));
    }
    let t = web.get_text(&url).await.map_err(|e| format!("{e:#}"))?;
    let j: serde_json::Value = serde_json::from_str(&t).map_err(|_| "Steam ответил непонятно.".to_string())?;
    let next = j["next_request"].as_str().and_then(|u| u.split("start=").nth(1)).and_then(|n| n.parse().ok());
    Ok(FeedPage { entries: parse(j["blotter_html"].as_str().unwrap_or("")), next })
}

/// Несколько дней подряд (лента бывает пустой в отдельные дни).
pub async fn load_days(client: &SteamClient, me: u64, start: u64, min_entries: usize) -> Result<FeedPage, String> {
    let mut page = load(client, me, start).await?;
    let mut tries = 0;
    while page.entries.len() < min_entries && tries < 6 {
        let Some(n) = page.next else { break };
        let more = load(client, me, n).await?;
        page.entries.extend(more.entries);
        page.next = more.next;
        tries += 1;
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rollup() {
        let html = r#"<div class="blotter_day" id="d"><div class="blotter_day_header_date">11 ноя</div>
        <div class="blotter_block"><div class="blotter_daily_rollup">
        <div class="blotter_daily_rollup_line"><img src="https://avatars/a.jpg"><span><a href="p">Друг</a> впервые сыграл в <a href="https://steamcommunity.com/app/945360">Among Us</a>.</span></div>
        </div></div></div>"#;
        let v = parse(html);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].day, "11 ноя");
        assert_eq!(v[0].appid, Some(945360));
        assert!(v[0].text.contains("Among Us"));
    }

    #[test]
    #[ignore]
    fn feed_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let p = load_days(&c, me, 0, 20).await.unwrap();
            let days: std::collections::BTreeSet<&str> = p.entries.iter().map(|e| e.day.as_str()).collect();
            println!(
                "событий {}, дней {}, с игрой {}, с картинками {}, дальше {}",
                p.entries.len(),
                days.len(),
                p.entries.iter().filter(|e| e.appid.is_some()).count(),
                p.entries.iter().filter(|e| !e.images.is_empty()).count(),
                p.next.is_some()
            );
            for e in p.entries.iter().filter(|e| !e.images.is_empty()).take(3) {
                println!("КРУПНОЕ: {}", e.text.chars().take(140).collect::<String>());
            }
            assert!(!p.entries.is_empty());
        });
    }
}

#[cfg(test)]
mod dump {
    /// VAPORUST_DUMP=<файл> cargo test feed_dump -- --ignored: сырой HTML ленты (для разбора).
    #[test]
    #[ignore]
    fn feed_parse_file() {
        let html = std::fs::read_to_string(std::env::var("VAPORUST_DUMP").unwrap()).unwrap();
        for e in super::parse(&html).iter().take(12) {
            println!("{}", e.text);
        }
    }

    #[test]
    #[ignore]
    fn feed_dump() {
        let path = std::env::var("VAPORUST_DUMP").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let t = web.get_text(&format!("https://steamcommunity.com/profiles/{me}/ajaxgetusernews/?l=russian")).await.unwrap();
            let j: serde_json::Value = serde_json::from_str(&t).unwrap();
            std::fs::write(path, j["blotter_html"].as_str().unwrap_or("")).unwrap();
        });
    }
}
