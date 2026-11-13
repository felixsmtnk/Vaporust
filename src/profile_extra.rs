//! Профиль целиком: уровень, рамка аватара и анимированный аватар, фон профиля, недавние игры,
//! группы и комментарии (можно написать свой). Web API IPlayerService (GetSteamLevel,
//! GetProfileItemsEquipped, GetRecentlyPlayedGames), профиль в XML (группы), комментарии -
//! как их подгружает сайт (comment/Profile/render).

use crate::activity::{parse_recent, Recent};
use crate::webapi;
use aurelia::steam_client::SteamClient;
use eframe::egui;

#[derive(Clone, Debug, Default)]
pub struct Comment {
    pub author: String,
    pub author_url: String,
    pub avatar: String,
    pub time: u64,
    pub text: String,
    /// Номер комментария (для удаления) и можно ли его удалить (свой профиль или свой комментарий).
    pub id: String,
    pub can_delete: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub name: String,
    pub url: String,
    pub members: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ProfileExtra {
    pub level: Option<u32>,
    pub frame: Option<String>,
    pub animated_avatar: Option<String>,
    pub background: Option<String>,
    pub recent: Vec<Recent>,
    pub groups: Vec<Group>,
    pub comments: Vec<Comment>,
    pub comments_total: u32,
    pub showcases: Vec<Showcase>,
    /// Счетчики правой колонки профиля: (подпись, число, ссылка) - значки, игры, скриншоты...
    pub counts: Vec<(String, String, String)>,
    /// То, что видно только на странице профиля: 3 недавние игры, значки, друзья...
    pub page: PageInfo,
}

const ITEMS_CDN: &str = "https://shared.fastly.steamstatic.com/community_assets/images/";

fn item_url(v: &serde_json::Value, key: &str) -> Option<String> {
    let p = v.get(key)?.as_str()?;
    if p.is_empty() {
        return None;
    }
    Some(if p.starts_with("http") { p.to_string() } else { format!("{ITEMS_CDN}{p}") })
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn cdata(s: &str) -> String {
    s.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim().to_string()
}

/// Группы из профиля в XML.
pub fn parse_groups(xml: &str) -> Vec<Group> {
    xml.split("<group ")
        .skip(1)
        .filter_map(|g| {
            let name = cdata(between(g, "<groupName>", "</groupName>")?);
            let url = cdata(between(g, "<groupURL>", "</groupURL>").unwrap_or(""));
            let members = between(g, "<memberCount>", "</memberCount>").and_then(|m| m.trim().parse().ok()).unwrap_or(0);
            Some(Group { url: if url.is_empty() { String::new() } else { format!("https://steamcommunity.com/groups/{url}") }, name, members })
        })
        .collect()
}

/// Комментарии из comments_html.
pub fn parse_comments(html: &str) -> Vec<Comment> {
    html.split("class=\"commentthread_comment ")
        .skip(1)
        .map(|c| {
            let avatar = between(c, "<img src=\"", "\"").unwrap_or("").to_string();
            let author_url = between(c, "commentthread_author_link\" href=\"", "\"").unwrap_or("").to_string();
            let author = between(c, "<bdi>", "</bdi>").map(crate::store::plain_text).unwrap_or_default();
            let time = between(c, "data-timestamp=\"", "\"").and_then(|t| t.parse().ok()).unwrap_or(0);
            let text = between(c, "commentthread_comment_text\"", "</div>")
                .and_then(|t| t.split_once('>').map(|x| x.1))
                .map(|t| crate::store::plain_text(&t.replace("<br>", "\n")))
                .unwrap_or_default();
            let id = between(c, "id=\"comment_", "\"").unwrap_or("").to_string();
            let can_delete = c.contains("DeleteComment");
            Comment { author, author_url, avatar, time, text: text.trim().to_string(), id, can_delete }
        })
        .collect()
}

/// Витрина профиля: заголовок, картинки (со ссылками) и текст.
#[derive(Clone, Debug, Default)]
pub struct Showcase {
    pub title: String,
    pub images: Vec<(String, Option<String>)>,
    pub text: String,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let v = between(tag, &format!("{name}=\""), "\"")?;
    Some(v.replace("&amp;", "&"))
}

/// Витрины со страницы профиля (блоки profile_customization).
pub fn parse_showcases(html: &str) -> Vec<Showcase> {
    let Some(area) = html.split("class=\"profile_customization_area\"").nth(1) else { return Vec::new() };
    // Область витрин кончается перед блоком комментариев / правой колонкой.
    let area = area.split("profile_comment_area").next().unwrap_or(area);
    let area = area.split("profile_rightcol").next().unwrap_or(area);
    // Начала витрин: «<div class="profile_customization"» и «... profile_customization myart"»
    // (но не «profile_customization_header» и т.п. внутри витрины).
    let starts: Vec<usize> = area
        .match_indices("class=\"profile_customization")
        .map(|(i, _)| i)
        .filter(|&i| {
            let next = area.as_bytes().get(i + "class=\"profile_customization".len()).copied();
            next == Some(b'"') || next == Some(b' ')
        })
        .collect();
    let blocks: Vec<&str> =
        starts.iter().enumerate().map(|(n, &s)| &area[s..starts.get(n + 1).copied().unwrap_or(area.len())]).collect();
    blocks
        .into_iter()
        .filter(|b| !b.contains("customization_edit") && !b.contains("recent_game"))
        .filter_map(|b| {
            let title = between(b, "profile_customization_header\">", "<").map(|t| t.trim().to_string()).unwrap_or_default();
            let mut images = Vec::new();
            let mut pos = 0;
            while let Some(i) = b[pos..].find("<img") {
                let at = pos + i;
                let tag_end = b[at..].find('>').map(|e| at + e).unwrap_or(b.len());
                let tag = &b[at..tag_end];
                pos = tag_end;
                let Some(src) = attr(tag, "src").filter(|s| s.starts_with("http")) else { continue };
                // Значки интерфейса Steam и пустышки не нужны.
                if src.contains("trans.gif") || src.contains("/public/images/") || src.contains("/public/shared/images/") {
                    continue;
                }
                let before = &b[..at];
                let open = before.rfind("<a ");
                let close = before.rfind("</a>");
                let href = match (open, close) {
                    (Some(o), c) if c.is_none_or(|c| c < o) => {
                        let a_end = before[o..].find('>').map(|e| o + e).unwrap_or(before.len());
                        attr(&before[o..a_end], "href")
                    }
                    _ => None,
                };
                if images.len() < 16 && !images.iter().any(|(s, _)| *s == src) {
                    images.push((src, href));
                }
            }
            let body = b.split("<script").next().unwrap_or(b);
            let body = body.split_once("profile_customization_block").map(|x| x.1).unwrap_or(body);
            let text: String = text_of(body);
            let text = if text.chars().count() > 400 { format!("{}...", text.chars().take(400).collect::<String>()) } else { text };
            (!images.is_empty() || !text.is_empty()).then(|| Showcase { title: if title.is_empty() { "Витрина".into() } else { title }, images, text })
        })
        .collect()
}

fn text_of(html: &str) -> String {
    strip_tags(&format!("<div {html}"))
}

pub async fn load(client: &SteamClient, id: u64) -> Result<ProfileExtra, String> {
    let sid = id.to_string();
    let mut x = ProfileExtra::default();
    if let Ok(v) = webapi::get(client, "IPlayerService/GetSteamLevel/v1", &[("steamid", sid.clone())]).await {
        x.level = v.get("player_level").and_then(|l| l.as_u64()).map(|l| l as u32);
    }
    if let Ok(v) = webapi::get(client, "IPlayerService/GetProfileItemsEquipped/v1", &[("steamid", sid.clone())]).await {
        x.frame = item_url(&v["avatar_frame"], "image_small");
        x.animated_avatar = item_url(&v["animated_avatar"], "image_small");
        x.background = item_url(&v["profile_background"], "image_large");
    }
    if let Ok(v) = webapi::get(client, "IPlayerService/GetRecentlyPlayedGames/v1", &[("steamid", sid.clone())]).await {
        x.recent = parse_recent(&v);
    }
    let web = webapi::web_session(client).await?;
    if let Ok(xml) = web.get_text(&format!("https://steamcommunity.com/profiles/{id}/?xml=1")).await {
        x.groups = parse_groups(&xml);
    }
    if let Ok(html) = web.get_text(&format!("https://steamcommunity.com/profiles/{id}/?l=russian")).await {
        x.showcases = parse_showcases(&html);
        x.counts = parse_counts(&html);
        x.page = parse_page(&html);
    }
    if let Ok(t) = web.get_text(&format!("https://steamcommunity.com/comment/Profile/render/{id}/-1/?start=0&count=10")).await {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
            x.comments_total = j["total_count"].as_u64().unwrap_or(0) as u32;
            x.comments = parse_comments(j["comments_html"].as_str().unwrap_or(""));
        }
    }
    Ok(x)
}

/// Написать комментарий в профиль.
pub async fn post_comment(client: &SteamClient, id: u64, text: &str) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let fields = [
        ("comment", text.to_string()),
        ("count", "6".to_string()),
        ("sessionid", web.sessionid().to_string()),
        ("feature2", "-1".to_string()),
    ];
    let (status, body) = web
        .post_form(
            &format!("https://steamcommunity.com/comment/Profile/post/{id}/-1/"),
            &fields,
            &format!("https://steamcommunity.com/profiles/{id}/"),
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if status == 200 && v.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(())
    } else {
        let why = v.get("error").and_then(|e| e.as_str()).map(crate::store::plain_text).unwrap_or_else(|| format!("код {status}"));
        Err(format!("Комментарий не отправлен: {why}"))
    }
}

/// Уровень - кружок с числом, как в Steam.
pub fn level_badge(ui: &mut egui::Ui, level: u32) {
    level_badge_sized(ui, level, 34.0);
}

/// Уровень заданного размера (в списке друзей - мелкий). Цвет по десяткам, как в Steam.
pub fn level_badge_sized(ui: &mut egui::Ui, level: u32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let color = match level {
        0..=9 => egui::Color32::from_rgb(154, 154, 154),
        10..=19 => egui::Color32::from_rgb(196, 40, 40),
        20..=29 => egui::Color32::from_rgb(214, 115, 0),
        30..=39 => egui::Color32::from_rgb(201, 172, 0),
        40..=49 => egui::Color32::from_rgb(76, 165, 0),
        50..=59 => egui::Color32::from_rgb(30, 130, 200),
        60..=69 => egui::Color32::from_rgb(130, 70, 200),
        70..=79 => egui::Color32::from_rgb(200, 70, 170),
        80..=89 => egui::Color32::from_rgb(140, 50, 70),
        90..=99 => egui::Color32::from_rgb(160, 125, 80),
        _ => egui::Color32::from_rgb(220, 90, 140),
    };
    let r = size / 2.0 - 2.0;
    ui.painter().circle_stroke(rect.center(), r, egui::Stroke::new(if size > 30.0 { 2.5 } else { 1.8 }, color));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, level.to_string(), egui::FontId::proportional(if size > 30.0 { 14.0 } else { 10.5 }), ui.visuals().strong_text_color());
}

/// Аватар с рамкой (рамка Steam шире аватара примерно на 22%).
pub fn avatar_with_frame(ui: &mut egui::Ui, avatar: &str, frame: Option<&str>, size: f32) {
    let outer = size * 1.22;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(outer, outer), egui::Sense::hover());
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    egui::Image::new(avatar).paint_at(ui, inner);
    if let Some(f) = frame {
        egui::Image::new(f).paint_at(ui, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xml_and_comments() {
        let xml = r#"<groups><group isPrimary="1"><groupID64>1</groupID64><groupName><![CDATA[Тест группа]]></groupName><groupURL><![CDATA[testgroup]]></groupURL><memberCount>1234</memberCount></group></groups>"#;
        let g = parse_groups(xml);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Тест группа");
        assert_eq!(g[0].url, "https://steamcommunity.com/groups/testgroup");
        assert_eq!(g[0].members, 1234);
        let html = r#"<div class="commentthread_comment responsive_body_text" id="comment_1"><div class="commentthread_comment_avatar"><a href="u"><img src="https://a/x.jpg"></a></div>
        <a class="hoverunderline commentthread_author_link" href="https://steamcommunity.com/id/a" data-miniprofile="1"><bdi>Аня</bdi></a>
        <span class="commentthread_comment_timestamp" data-timestamp="1700000000">x</span>
        <div class="commentthread_comment_text" id="comment_content_1">Привет<br>мир</div></div>"#;
        let c = parse_comments(html);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].author.as_str(), c[0].time, c[0].avatar.as_str()), ("Аня", 1700000000, "https://a/x.jpg"));
        assert!(c[0].text.contains("Привет") && c[0].text.contains("мир"));
    }

    #[test]
    #[ignore]
    fn profile_extra_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let x = load(&c, me).await.unwrap();
            for u in [&x.animated_avatar, &x.frame, &x.background].into_iter().flatten() {
                let r = ehttp::fetch_blocking(&ehttp::Request::get(u));
                println!("{u} -> {:?}", r.map(|r| (r.status, r.content_type().map(str::to_owned), r.bytes.len())));
            }
            println!(
                "уровень {:?}, рамка {}, аним. аватар {}, фон {}, недавних {}, групп {}, комментариев {} из {}, с текстом {}, с автором {}",
                x.level,
                x.frame.is_some(),
                x.animated_avatar.is_some(),
                x.background.is_some(),
                x.recent.len(),
                x.groups.len(),
                x.comments.len(),
                x.comments_total,
                x.comments.iter().filter(|c| !c.text.is_empty()).count(),
                x.comments.iter().filter(|c| !c.author.is_empty() && c.time > 0).count()
            );
        });
    }
}

#[cfg(test)]
mod showcase_tests {
    /// Разбор сохраненной страницы профиля друга (VAPORUST_DUMP/profile_friend.html).
    #[test]
    #[ignore]
    fn showcases_dump() {
        let dir = std::env::var("VAPORUST_DUMP").unwrap();
        let html = std::fs::read_to_string(format!("{dir}/profile_friend.html")).unwrap();
        let v = super::parse_showcases(&html);
        for s in &v {
            println!("{} | картинок {} (со ссылкой {}) | текст {} знаков", s.title, s.images.len(), s.images.iter().filter(|i| i.1.is_some()).count(), s.text.chars().count());
            if s.title == "Витрина" { println!("   {}", s.text.chars().take(160).collect::<String>()); }
        }
        assert!(v.len() >= 5);
    }
}

/// Счетчики правой колонки профиля (значки, игры, инвентарь, скриншоты, обзоры, друзья,
/// группы): (подпись, число, ссылка).
pub fn parse_counts(html: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    // Только сами счетчики («profile_count_link ellipsis»), а не _total, _preview и т.п.
    let key = "class=\"profile_count_link";
    let starts: Vec<usize> = html
        .match_indices(key)
        .map(|(i, _)| i)
        .filter(|&i| matches!(html.as_bytes().get(i + key.len()), Some(b' ' | b'"')))
        .collect();
    for i in starts {
        let b = &html[i..];
        let b = b.split("</a>").next().unwrap_or(b);
        let Some(label) = between(b, "count_link_label\">", "<") else { continue };
        let href = between(b, "href=\"", "\"").unwrap_or("").to_string();
        let total = between(b, "profile_count_link_total\">", "<").map(|t| t.replace("&nbsp;", "").trim().to_string()).unwrap_or_default();
        let label = crate::store::plain_text(label);
        if !label.is_empty() && !out.iter().any(|(l, _, _): &(String, String, String)| *l == label) {
            out.push((label, total, href));
        }
    }
    out
}

/// Комментарии профиля с `start` (по 10, для «Показать еще»): (всего, комментарии).
pub async fn load_comments(client: &SteamClient, id: u64, start: u32) -> Result<(u32, Vec<Comment>), String> {
    let web = webapi::web_session(client).await?;
    let t = web
        .get_text(&format!("https://steamcommunity.com/comment/Profile/render/{id}/-1/?start={start}&count=10"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    let j: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    Ok((j["total_count"].as_u64().unwrap_or(0) as u32, parse_comments(j["comments_html"].as_str().unwrap_or(""))))
}

/// Удалить комментарий (в своем профиле - любой, в чужом - свой).
pub async fn delete_comment(client: &SteamClient, owner: u64, gid: &str) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let fields = [
        ("gidcomment", gid.to_string()),
        ("start", "0".to_string()),
        ("count", "10".to_string()),
        ("sessionid", web.sessionid().to_string()),
        ("feature2", "-1".to_string()),
    ];
    let (status, body) = web
        .post_form(
            &format!("https://steamcommunity.com/comment/Profile/delete/{owner}/-1/"),
            &fields,
            &format!("https://steamcommunity.com/profiles/{owner}/"),
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if status == 200 && v.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(())
    } else {
        let why = v.get("error").and_then(|e| e.as_str()).map(crate::store::plain_text).unwrap_or_else(|| format!("код {status}"));
        Err(format!("Комментарий не удален: {why}"))
    }
}

#[cfg(test)]
mod counts_live {
    /// Живой тест (только чтение): счетчики своего профиля и первые комментарии.
    #[test]
    #[ignore]
    fn counts_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = super::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let x = super::load(&c, me).await.unwrap();
            println!("счетчики: {:?}", x.counts.iter().map(|(l, n, _)| format!("{l}={n}")).collect::<Vec<_>>());
            println!("комментариев {}, с номером {}, можно удалить {}", x.comments.len(), x.comments.iter().filter(|c| !c.id.is_empty()).count(), x.comments.iter().filter(|c| c.can_delete).count());
            let (total, more) = super::load_comments(&c, me, 10).await.unwrap();
            println!("всего {total}, со 10-го: {}", more.len());
        });
    }
}

#[cfg(test)]
mod counts_dump {
    #[test]
    #[ignore]
    fn counts_dump() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = super::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let web = super::webapi::web_session(&c).await.unwrap();
            let html = web.get_text(&format!("https://steamcommunity.com/profiles/{me}/?l=russian")).await.unwrap();
            let dir = std::env::var("VAPORUST_DUMP").unwrap();
            std::fs::write(format!("{dir}/profile_me.html"), html.replace(&me.to_string(), "ID")).unwrap();
            println!("сохранено {} байт", html.len());
        });
    }
}

/// Недавняя игра со страницы профиля (как показывает Steam: 3 штуки).
#[derive(Clone, Debug, Default)]
pub struct PageGame {
    pub appid: u32,
    pub name: String,
    pub capsule: String,
    /// «519 ч. всего», «последний запуск 12 ноя».
    pub details: Vec<String>,
}

/// Друг в правой колонке профиля.
#[derive(Clone, Debug, Default)]
pub struct PageFriend {
    pub steamid: u64,
    pub name: String,
    pub status: String,
    pub avatar: String,
    pub level: Option<u32>,
    pub online: bool,
    pub in_game: bool,
}

/// То, что есть только на странице профиля (как ее видит Steam).
#[derive(Clone, Debug, Default)]
pub struct PageInfo {
    pub recent: Vec<PageGame>,
    /// «2,9 ч. за последние 2 недели».
    pub recent_total: String,
    pub awards: Vec<String>,
    pub badges: Vec<String>,
    pub friends: Vec<PageFriend>,
    /// Избранный значок: картинка, название, опыт.
    pub fav_badge: Option<(String, String, String)>,
    /// Статус в шапке правой колонки («В сети», «В игре») и игра.
    pub status: String,
    pub status_game: String,
}

/// Убрать теги и комментарии HTML, сжать пробелы.
pub fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        out.push(' ');
        let after = &rest[i..];
        let end = if after.starts_with("<!--") { after.find("-->").map(|e| e + 3) } else { after.find('>').map(|e| e + 1) };
        match end {
            Some(e) => rest = &after[e..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    let t = out
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_page(html: &str) -> PageInfo {
    let mut p = PageInfo::default();
    for b in html.split("class=\"recent_game\"").skip(1).take(3) {
        let b = b.split("class=\"recent_game\"").next().unwrap_or(b);
        let appid = between(b, "steamcommunity.com/app/", "\"").and_then(|a| a.trim_end_matches('/').parse().ok()).unwrap_or(0);
        let capsule = between(b, "class=\"game_capsule\" src=\"", "\"").unwrap_or("").to_string();
        let details = between(b, "game_info_details\">", "</div>")
            .map(|d| d.split("<br>").map(strip_tags).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        let name = between(b, "class=\"game_name\">", "</div>").map(strip_tags).unwrap_or_default();
        if appid != 0 || !name.is_empty() {
            p.recent.push(PageGame { appid, name, capsule, details });
        }
    }
    p.recent_total = between(html, "recentgame_recentplaytime\">", "</div>\n").map(strip_tags).or_else(|| between(html, "recentgame_recentplaytime\">", "</div> </div>").map(strip_tags)).unwrap_or_default();
    if p.recent_total.len() > 80 {
        p.recent_total.clear();
    }
    // Награды и значки - картинки под счетчиками.
    let mut pos = 0;
    while let Some(i) = html[pos..].find("class=\"award_icon") {
        let at = pos + i;
        let tag_start = html[..at].rfind("<img").unwrap_or(at);
        if let Some(src) = attr(&html[tag_start..at + 40], "src") {
            if !p.awards.contains(&src) && p.awards.len() < 6 {
                p.awards.push(src);
            }
        }
        pos = at + 10;
    }
    let mut pos = 0;
    while let Some(i) = html[pos..].find("class=\"badge_icon") {
        let at = pos + i;
        let tag_start = html[..at].rfind("<img").unwrap_or(at);
        if let Some(src) = attr(&html[tag_start..at], "src") {
            if !p.badges.contains(&src) && p.badges.len() < 6 {
                p.badges.push(src);
            }
        }
        pos = at + 10;
    }
    // Друзья в правой колонке.
    for b in html.split("class=\"friendBlock persona").skip(1).take(8) {
        let state = b.split('"').next().unwrap_or("").trim().to_string();
        let account: u64 = between(b, "data-miniprofile=\"", "\"").and_then(|a| a.parse().ok()).unwrap_or(0);
        let name = between(b, "friendBlockContent\">", "<br>").map(strip_tags).unwrap_or_default();
        let status = between(b, "friendSmallText\">", "</span>").map(strip_tags).unwrap_or_default();
        let avatar = between(b, "<img src=\"", "\"").unwrap_or("").to_string();
        let level = between(b, "friendPlayerLevelNum\">", "<").and_then(|l| l.trim().parse().ok());
        if !name.is_empty() {
            p.friends.push(PageFriend {
                steamid: if account > 0 { account + crate::social::STEAMID64_BASE } else { 0 },
                name,
                status,
                avatar,
                level,
                online: state.starts_with("online") || state.starts_with("in-game"),
                in_game: state.starts_with("in-game"),
            });
        }
    }
    // Избранный значок в шапке.
    if let Some(b) = between(html, "class=\"favorite_badge\"", "profile_header_actions") {
        let icon = between(b, "<img src=\"", "\"").unwrap_or("").to_string();
        let name = between(b, "class=\"name ellipsis\">", "</div>").map(strip_tags).unwrap_or_default();
        let xp = between(b, "class=\"xp\">", "</div>").map(strip_tags).unwrap_or_default();
        if !icon.is_empty() {
            p.fav_badge = Some((icon, name, xp));
        }
    }
    p.status = between(html, "profile_in_game_header\">", "</div>").map(strip_tags).unwrap_or_default();
    p.status_game = between(html, "profile_in_game_name\">", "</div>").map(strip_tags).unwrap_or_default();
    p
}

#[cfg(test)]
mod page_tests {
    /// Разбор сохраненной страницы своего профиля (VAPORUST_DUMP/profile_me.html).
    #[test]
    #[ignore]
    fn page_dump() {
        let dir = std::env::var("VAPORUST_DUMP").unwrap();
        let html = std::fs::read_to_string(format!("{dir}/profile_me.html")).unwrap();
        let p = super::parse_page(&html);
        for g in &p.recent {
            println!("игра {} {} {:?} картинка {}", g.appid, g.name, g.details, !g.capsule.is_empty());
        }
        println!("за 2 недели: {:?}", p.recent_total);
        println!("наград {}, значков {}, друзей {}", p.awards.len(), p.badges.len(), p.friends.len());
        for f in &p.friends {
            println!("друг: {} / {} / ур. {:?} / в сети {} / id есть {}", f.name, f.status, f.level, f.online, f.steamid != 0);
        }
        println!("статус {:?} {:?}, избранный значок {:?}", p.status, p.status_game, p.fav_badge.as_ref().map(|b| &b.1));
        let s = super::parse_showcases(&html);
        println!("витрин {}", s.len());
    }

    #[test]
    fn strips_tags() {
        assert_eq!(super::strip_tags("<!-- x --> 1 313 Предметов <div data-panel=\"{}\">"), "1 313 Предметов");
        assert_eq!(super::strip_tags("a<br>b &amp; c <div"), "a b & c");
    }
}
