//! Parsing Steam chat messages (BBCode) into pieces for display: text, links,
//! images, emoticons and stickers.
//!
//! Format from history (request with bbcode_format = true), checked on real messages:
//!   [emoticon]steamhappy[/emoticon]
//!   [sticker type="Buoyant Bobber" limit="0"][/sticker]
//!   [img src=URL thumbnail_src=URL srcset="" width=275 height=212][url=URL]URL[/url][/img]
//! Without BBCode an emoticon looks like ːsteamhappyː, an image like a bare link.

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Text(String),
    Link { url: String, label: String },
    /// Image from a message; dimensions, if Steam sent them.
    Image { url: String, size: Option<(f32, f32)> },
    Emoticon(String),
    Sticker(String),
    /// Room effect (balloons, confetti...): type from [roomeffect type="..."].
    RoomEffect(String),
    /// Hidden text ([spoiler]): shown on click.
    Spoiler(String),
    /// Code ([code]): monospace font, as a separate block.
    Code(String),
    /// Quote ([quote author="..."]).
    Quote { author: Option<String>, text: String },
    /// Third-person action: "/me waves".
    Me(String),
}

const CDN: &str = "https://community.fastly.steamstatic.com/economy";

/// Emoticon image URL.
pub fn emoticon_url(name: &str) -> String {
    format!("{CDN}/emoticonlarge/{}", urlencode(name))
}

/// Reduced copy of a chat image (the same URL Steam gives in thumbnail_src).
/// For non-Steam links - the original URL.
pub fn thumbnail_url(url: &str) -> String {
    if url.contains("steamusercontent.com/ugc/") && !url.contains('?') {
        format!("{url}?imw=512&ima=fit&impolicy=Letterbox&imcolor=%23000000&letterbox=false")
    } else {
        url.to_string()
    }
}

/// Host of an http(s) URL in lower case ("" for anything else).
/// Parsed by the standard URL parser - the same rules the browser and the downloader use
/// (a hand-made split read "https://other.site\@steamcommunity.com" as a Steam address).
/// An address with a user name or password inside is not accepted at all.
pub fn host_of(url: &str) -> String {
    match parsed(url) {
        Some(u) => u.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase(),
        None => String::new(),
    }
}

/// http(s) address without "user:password@"; None for anything else.
pub fn parsed(url: &str) -> Option<url::Url> {
    let u = url::Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    Some(u)
}

/// Sites that belong to Valve (Steam store, community, CDN).
const STEAM_HOSTS: &[&str] = &[
    "steampowered.com",
    "steamcommunity.com",
    "steamstatic.com",
    "steamusercontent.com",
    "steamcontent.com",
    "steamchina.com",
    "steam-chat.com",
    "valvesoftware.com",
    "steamgames.com",
    "steamserver.net",
];

/// A Valve site (the domain itself or a subdomain).
pub fn is_steam_host(host: &str) -> bool {
    STEAM_HOSTS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
        || host == "steamuserimages-a.akamaihd.net"
        || host == "steamcdn-a.akamaihd.net"
}

/// An image from a chat message may be loaded by itself only from a Steam site over https:
/// a request to any other server would show the sender our IP address and the time we read it.
pub fn trusted_image(url: &str) -> bool {
    parsed(url).is_some_and(|u| u.scheme() == "https" && is_steam_host(&host_of(url)))
}

/// The label of a link looks like an address of another site than the real one
/// ("[url=https://evil.example]https://steamcommunity.com/...[/url]").
pub fn misleading_label(url: &str, label: &str) -> bool {
    let l = label.trim();
    let looks = l.contains("://") || l.to_ascii_lowercase().starts_with("www.") || {
        let first = l.split(['/', ' ']).next().unwrap_or("");
        first.contains('.') && first.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
    };
    if !looks {
        return false;
    }
    let lh = if l.contains("://") { host_of(l) } else { host_of(&format!("https://{l}")) };
    lh != host_of(url)
}

/// Sticker image URL.
pub fn sticker_url(name: &str) -> String {
    format!("{CDN}/sticker/{}", urlencode(name))
}

use crate::util::urlencode;

/// Attribute value like key="value" or key=value from the tag contents.
fn attr(tag: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(key) {
        let start = from + pos;
        from = start + key.len();
        // The key must start at a word boundary (otherwise src is found inside thumbnail_src).
        if start > 0 && !tag[..start].ends_with(' ') {
            continue;
        }
        let Some(rest) = tag[start + key.len()..].strip_prefix('=') else {
            continue;
        };
        return Some(if let Some(r) = rest.strip_prefix('"') {
            r.split('"').next().unwrap_or("").to_string()
        } else {
            rest.split([' ', ']']).next().unwrap_or("").to_string()
        });
    }
    None
}

fn is_image_url(url: &str) -> bool {
    let u = url.to_lowercase();
    let path = u.split('?').next().unwrap_or("");
    u.contains("steamusercontent.com/ugc")
        || [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| path.ends_with(e))
}

fn url_seg(url: &str, label: Option<&str>) -> Seg {
    match label.map(str::trim).filter(|l| !l.is_empty() && *l != url) {
        Some(l) => Seg::Link { url: url.into(), label: l.into() },
        None if is_image_url(url) => Seg::Image { url: url.into(), size: None },
        None => Seg::Link { url: url.into(), label: url.into() },
    }
}

/// Formatting tags: drop the tags themselves, keep the contents.
const FORMAT_TAGS: &[&str] = &[
    "b", "i", "u", "s", "strike", "spoiler", "quote", "code", "noparse", "h1", "h2", "h3", "list",
    "olist", "*", "hr", "table", "tr", "td", "th",
];

/// Longest message that is parsed (Steam itself limits chat messages to a few thousand characters).
const MAX_LEN: usize = 20_000;
/// Nesting limit of spoilers, quotes and "/me" inside each other: deeper levels stay as plain text
/// (a crafted message of thousands of nested "/me" crashed the program with a stack overflow).
const MAX_DEPTH: u32 = 4;

pub fn parse(msg: &str) -> Vec<Seg> {
    let msg = match msg.char_indices().nth(MAX_LEN) {
        Some((cut, _)) => &msg[..cut],
        None => msg,
    };
    parse_at(msg, 0)
}

fn parse_at(msg: &str, depth: u32) -> Vec<Seg> {
    // "/me text" - third-person action (as in Steam chat).
    if let Some(rest) = msg.strip_prefix("/me ") {
        return vec![Seg::Me(plain_at(rest, depth + 1))];
    }
    let mut segs: Vec<Seg> = Vec::new();
    let mut text = String::new();
    let mut rest = msg;

    fn flush(text: &mut String, segs: &mut Vec<Seg>) {
        if !text.is_empty() {
            push_text(&std::mem::take(text), segs);
        }
    }

    while let Some(open) = rest.find('[') {
        text.push_str(&rest[..open]);
        let after = &rest[open..];
        let Some(close) = after.find(']') else {
            text.push_str(after);
            rest = "";
            break;
        };
        let tag = &after[1..close];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split([' ', '='])
            .next()
            .unwrap_or("")
            .to_lowercase();
        let body = &after[close + 1..];
        // Contents up to the matching closing tag, and the rest after it.
        let inner = |name: &str| -> Option<(&str, &str)> {
            let end = format!("[/{name}]");
            let pos = body.find(&end).or_else(|| body.find(&end.to_uppercase()))?;
            Some((&body[..pos], &body[pos + end.len()..]))
        };

        if !closing {
            match name.as_str() {
                "spoiler" | "code" | "quote" => {
                    if let Some((b, tail)) = inner(&name) {
                        flush(&mut text, &mut segs);
                        segs.push(match name.as_str() {
                            "spoiler" => Seg::Spoiler(plain_at(b, depth + 1)),
                            "code" => Seg::Code(b.trim_matches(['\n', '\r']).to_string()),
                            _ => Seg::Quote { author: attr(tag, "author").filter(|a| !a.is_empty()), text: plain_at(b, depth + 1) },
                        });
                        rest = tail;
                        continue;
                    }
                }
                "emoticon" => {
                    if let Some((b, tail)) = inner("emoticon") {
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Emoticon(b.trim().to_string()));
                        rest = tail;
                        continue;
                    }
                }
                "gameinvite" => {
                    // Game invite from Steam chat.
                    flush(&mut text, &mut segs);
                    segs.push(Seg::Text(tr!("[приглашение в игру]", "[game invite]").into()));
                    rest = inner("gameinvite").map(|(_, t)| t).unwrap_or(body);
                    continue;
                }
                "roomeffect" => {
                    flush(&mut text, &mut segs);
                    segs.push(Seg::RoomEffect(attr(tag, "type").unwrap_or_default()));
                    rest = inner("roomeffect").map(|(_, t)| t).unwrap_or(body);
                    continue;
                }
                "sticker" => {
                    flush(&mut text, &mut segs);
                    segs.push(Seg::Sticker(attr(tag, "type").unwrap_or_default()));
                    rest = inner("sticker").map(|(_, t)| t).unwrap_or(body);
                    continue;
                }
                "img" => {
                    let (b, tail) = inner("img").unwrap_or(("", body));
                    let url = attr(tag, "src").unwrap_or_else(|| b.trim().to_string());
                    let num = |k| attr(tag, k).and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0);
                    flush(&mut text, &mut segs);
                    if !url.is_empty() {
                        segs.push(Seg::Image {
                            url,
                            size: num("width").zip(num("height")),
                        });
                    }
                    rest = tail;
                    continue;
                }
                "url" => {
                    let (b, tail) = inner("url").unwrap_or(("", body));
                    let url = tag
                        .strip_prefix("url=")
                        .map(|u| u.trim_matches('"').to_string())
                        .unwrap_or_else(|| b.trim().to_string());
                    flush(&mut text, &mut segs);
                    if !url.is_empty() {
                        segs.push(url_seg(&url, Some(b)));
                    }
                    rest = tail;
                    continue;
                }
                _ => {}
            }
        }
        if FORMAT_TAGS.contains(&name.as_str()) || (closing && (name == "emoticon" || name == "sticker" || name == "roomeffect")) {
            rest = body;
            continue;
        }
        // Unknown bracket - keep as is.
        text.push('[');
        rest = &after[1..];
    }
    text.push_str(rest);
    flush(&mut text, &mut segs);
    segs
}

/// BBCode text without formatting (for spoilers, quotes, "/me"); `depth` stops deep nesting.
fn plain_at(s: &str, depth: u32) -> String {
    if depth > MAX_DEPTH {
        return s.to_string();
    }
    parse_at(s, depth)
        .into_iter()
        .map(|seg| match seg {
            Seg::Text(t) | Seg::Spoiler(t) | Seg::Code(t) | Seg::Me(t) => t,
            Seg::Quote { text, .. } => text,
            Seg::Link { label, .. } => label,
            Seg::Emoticon(n) => format!(":{n}:"),
            Seg::Sticker(n) => trf!("[стикер {n}]", "[sticker {n}]"),
            Seg::Image { .. } => tr!("[картинка]", "[image]").into(),
            Seg::RoomEffect(_) => String::new(),
        })
        .collect()
}

fn push_text(text: &str, segs: &mut Vec<Seg>) {
    let mut buf = String::new();
    let mut rest = text;
    loop {
        let url_pos = [rest.find("http://"), rest.find("https://")].into_iter().flatten().min();
        let emo_pos = rest.find('ː');
        match (url_pos, emo_pos) {
            (Some(u), e) if e.map_or(true, |e| u < e) => {
                buf.push_str(&rest[..u]);
                let tail = &rest[u..];
                let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
                if !buf.is_empty() {
                    segs.push(Seg::Text(std::mem::take(&mut buf)));
                }
                segs.push(url_seg(&tail[..end], None));
                rest = &tail[end..];
            }
            (_, Some(e)) => {
                let after = &rest[e + 'ː'.len_utf8()..];
                match after.find('ː') {
                    Some(end)
                        if end > 0
                            && after[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                    {
                        buf.push_str(&rest[..e]);
                        if !buf.is_empty() {
                            segs.push(Seg::Text(std::mem::take(&mut buf)));
                        }
                        segs.push(Seg::Emoticon(after[..end].to_string()));
                        rest = &after[end + 'ː'.len_utf8()..];
                    }
                    _ => {
                        buf.push_str(&rest[..e + 'ː'.len_utf8()]);
                        rest = after;
                    }
                }
            }
            _ => break,
        }
    }
    buf.push_str(rest);
    if !buf.is_empty() {
        segs.push(Seg::Text(buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Seg {
        Seg::Text(s.into())
    }

    #[test]
    fn plain() {
        assert_eq!(parse("привет"), vec![t("привет")]);
        assert_eq!(parse("[b]жирный[/b] [x]"), vec![t("жирный [x]")]);
        assert_eq!(parse("a [ b"), vec![t("a [ b")]);
    }

    #[test]
    fn real_messages() {
        assert_eq!(
            parse("[gameinvite appid=\"0\" connect=\"\"][/gameinvite]"),
            vec![t("[приглашение в игру]")]
        );
        let u = "https://images.steamusercontent.com/ugc/10327085474773862713/497ABEAE997110E3CCB65049B2A23780E3104118/";
        let img = format!(
            "[img src={u} thumbnail_src={u} srcset=\"\" width=275 height=212][url={u}]{u}[/url][/img]"
        );
        assert_eq!(parse(&img), vec![Seg::Image { url: u.into(), size: Some((275.0, 212.0)) }]);
        assert_eq!(
            parse("[sticker type=\"Buoyant Bobber\" limit=\"0\"][/sticker]"),
            vec![Seg::Sticker("Buoyant Bobber".into())]
        );
        assert_eq!(parse("[emoticon]steamhappy[/emoticon]"), vec![Seg::Emoticon("steamhappy".into())]);
    }

    #[test]
    fn no_bbcode_forms() {
        assert_eq!(
            parse("hi ːsteamhappyː!"),
            vec![t("hi "), Seg::Emoticon("steamhappy".into()), t("!")]
        );
        assert_eq!(parse("aːb c"), vec![t("aːb c")]);
        let u = "https://images.steamusercontent.com/ugc/1/ABC/";
        assert_eq!(parse(u), vec![Seg::Image { url: u.into(), size: None }]);
        assert_eq!(
            parse("see https://x.org/a ok"),
            vec![
                t("see "),
                Seg::Link { url: "https://x.org/a".into(), label: "https://x.org/a".into() },
                t(" ok")
            ]
        );
        assert_eq!(
            parse("[url=https://x.org]сайт[/url]"),
            vec![Seg::Link { url: "https://x.org".into(), label: "сайт".into() }]
        );
    }

    #[test]
    fn urls() {
        assert_eq!(
            sticker_url("Buoyant Bobber"),
            "https://community.fastly.steamstatic.com/economy/sticker/Buoyant%20Bobber"
        );
    }
}

/// For "Copy text": the real text of the message - spoilers open, links and images as addresses,
/// emoticons as :name:.
pub fn copy_text(msg: &str) -> String {
    let mut out = String::new();
    for seg in parse(msg) {
        let piece = match seg {
            Seg::Text(t) | Seg::Spoiler(t) | Seg::Code(t) => t,
            Seg::Link { url, label } if label == url => url,
            Seg::Link { url, label } => format!("{label} ({url})"),
            Seg::Image { url, .. } => url,
            Seg::Emoticon(n) => format!(":{n}:"),
            Seg::Sticker(n) => format!("[{n}]"),
            Seg::RoomEffect(n) => format!("[{n}]"),
            Seg::Quote { author, text } => match author {
                Some(a) => format!("{a}: \"{text}\"\n"),
                None => format!("\"{text}\"\n"),
            },
            Seg::Me(t) => format!("* {t}"),
        };
        out.push_str(&piece);
    }
    out.trim().to_string()
}

/// For a popup notification: message text and image (first emoticon or sticker).
pub fn preview(msg: &str) -> (String, Option<String>) {
    let mut text = String::new();
    let mut image = None;
    let mut what = "";
    for s in parse(msg) {
        match s {
            Seg::Text(t) | Seg::Code(t) => text.push_str(&t),
            Seg::Me(t) => text.push_str(&format!("* {t}")),
            Seg::Quote { text: q, .. } => text.push_str(&format!("«{q}» ")),
            // Do not reveal a spoiler in a notification.
            Seg::Spoiler(_) => text.push_str(tr!("[спойлер]", "[spoiler]")),
            Seg::Link { label, .. } => text.push_str(&label),
            Seg::Image { .. } => what = tr!("картинка", "image"),
            Seg::Emoticon(n) => {
                image.get_or_insert_with(|| emoticon_url(&n));
                if what.is_empty() {
                    what = tr!("смайлик", "emoticon");
                }
            }
            Seg::Sticker(n) => {
                image.get_or_insert_with(|| sticker_url(&n));
                what = tr!("стикер", "sticker");
            }
            Seg::RoomEffect(t) => {
                image.get_or_insert_with(|| crate::effects::icon_url(&t));
                text.push_str(&trf!("эффект комнаты: {}", "room effect: {}", crate::effects::short_name(&t)));
            }
        }
    }
    let mut text = text.trim().to_string();
    if text.is_empty() {
        text = what.to_string();
    }
    (text, image)
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn room_effect_and_preview() {
        let s = parse(r#"[roomeffect type="balloons" limit="0"][/roomeffect]"#);
        assert!(matches!(s.as_slice(), [Seg::RoomEffect(t)] if t == "balloons"));
        let (t, img) = preview(r#"[roomeffect type="balloons" limit="0"][/roomeffect]"#);
        assert!(t.starts_with("эффект комнаты: "));
        assert!(img.unwrap().ends_with("96px/balloons.png"));
        let (t, img) = preview("[emoticon]steamhappy[/emoticon]");
        assert_eq!(t, "смайлик");
        assert!(img.unwrap().contains("steamhappy"));
        let (t, img) = preview(r#"[sticker type="SteamLaugh" limit="0"][/sticker]"#);
        assert_eq!(t, "стикер");
        assert!(img.unwrap().contains("SteamLaugh"));
        assert_eq!(preview("привет ːsteamhappyː").0, "привет");
    }
}

#[cfg(test)]
mod format_tags_tests {
    use super::{parse, Seg};

    #[test]
    fn spoiler_code_quote_me() {
        let s = parse(r#"а [spoiler]тайна[/spoiler] [code]let x = 1;[/code] [quote author="Аня"]привет[/quote]"#);
        assert!(s.iter().any(|x| matches!(x, Seg::Spoiler(t) if t == "тайна")));
        assert!(s.iter().any(|x| matches!(x, Seg::Code(t) if t == "let x = 1;")));
        assert!(s.iter().any(|x| matches!(x, Seg::Quote { author: Some(a), text } if a == "Аня" && text == "привет")));
        assert!(matches!(parse("/me машет рукой").as_slice(), [Seg::Me(t)] if t == "машет рукой"));
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    #[test]
    fn images_only_from_steam() {
        assert!(trusted_image("https://images.steamusercontent.com/ugc/123/ABC/"));
        assert!(trusted_image("https://community.fastly.steamstatic.com/economy/emoticonlarge/x"));
        assert!(trusted_image("https://steamuserimages-a.akamaihd.net/ugc/1/2/"));
        assert!(!trusted_image("http://images.steamusercontent.com/ugc/1/")); // not https
        assert!(!trusted_image("https://evil.example/a.png"));
        assert!(!trusted_image("https://steamcommunity.com.evil.example/a.png"));
        assert!(!trusted_image("https://notsteamcommunity.com/a.png"));
        assert!(!trusted_image("https://user@evil.example/steamcommunity.com/a.png"));
        assert!(!trusted_image(r"file://\\server\share\a.png"));
        assert!(!trusted_image("file:///C:/a.png"));
        assert_eq!(host_of("https://Store.SteamPowered.com:443/app/1"), "store.steampowered.com");
        // A user name or password inside the address - not accepted at all.
        assert_eq!(host_of("https://User:pw@Store.SteamPowered.com/app/1"), "");
        // A backslash ends the host for the browser: these lead to outside.invalid, not to Steam.
        assert!(!trusted_image(r"https://outside.invalid\@steamcommunity.com/a.png"));
        assert!(!trusted_image(r"https://outside.invalid\.steamcommunity.com/a.png"));
        assert_ne!(host_of(r"https://outside.invalid\@steamcommunity.com/"), "steamcommunity.com");
    }

    #[test]
    fn deep_nesting_does_not_crash() {
        // Thousands of nested "/me" and spoilers: parsed without running out of stack.
        let me = "/me ".repeat(5000) + "x";
        let _ = parse(&me);
        let _ = preview(&me);
        let sp = "[spoiler]".repeat(3000) + "x" + &"[/spoiler]".repeat(3000);
        let _ = parse(&sp);
        let q = "[quote]".repeat(3000);
        let _ = copy_text(&q);
    }

    #[test]
    fn fake_link_labels() {
        // A label that pretends to be a Steam address while the link leads elsewhere.
        assert!(misleading_label("https://steamcornmunity.example/tradeoffer/1", "https://steamcommunity.com/tradeoffer/new/"));
        assert!(misleading_label("https://evil.example/", "steamcommunity.com/id/me"));
        // An honest label, or a plain word.
        assert!(!misleading_label("https://steamcommunity.com/id/me", "https://steamcommunity.com/id/me"));
        assert!(!misleading_label("https://evil.example/", "мой сайт"));
        assert!(!misleading_label("https://evil.example/", "click here"));
    }

    #[test]
    fn copy_keeps_hidden_parts() {
        let t = copy_text("a [spoiler]тайна[/spoiler] [url=https://x.example/]сайт[/url] [img src=https://images.steamusercontent.com/ugc/1/][/img]");
        assert!(t.contains("тайна") && t.contains("сайт (https://x.example/)") && t.contains("https://images.steamusercontent.com/ugc/1/"), "{t}");
    }
}
