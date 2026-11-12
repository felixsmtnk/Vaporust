//! Разбор сообщений чата Steam (BBCode) в куски для отображения: текст, ссылки,
//! картинки, смайлики и стикеры.
//!
//! Формат из истории (запрос с bbcode_format = true), проверено на настоящих сообщениях:
//!   [emoticon]steamhappy[/emoticon]
//!   [sticker type="Buoyant Bobber" limit="0"][/sticker]
//!   [img src=URL thumbnail_src=URL srcset="" width=275 height=212][url=URL]URL[/url][/img]
//! Без BBCode смайлик выглядит как ːsteamhappyː, картинка — как голая ссылка.

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Text(String),
    Link { url: String, label: String },
    /// Картинка из сообщения; размеры, если Steam их прислал.
    Image { url: String, size: Option<(f32, f32)> },
    Emoticon(String),
    Sticker(String),
    /// Эффект комнаты (шарики, конфетти…): тип из [roomeffect type="…"].
    RoomEffect(String),
}

const CDN: &str = "https://community.fastly.steamstatic.com/economy";

/// Адрес картинки смайлика.
pub fn emoticon_url(name: &str) -> String {
    format!("{CDN}/emoticonlarge/{}", urlencode(name))
}

/// Уменьшенная копия картинки из чата (тот же адрес, что Steam даёт в thumbnail_src).
/// Для не-Steam ссылок — исходный адрес.
pub fn thumbnail_url(url: &str) -> String {
    if url.contains("steamusercontent.com/ugc/") && !url.contains('?') {
        format!("{url}?imw=512&ima=fit&impolicy=Letterbox&imcolor=%23000000&letterbox=false")
    } else {
        url.to_string()
    }
}

/// Адрес картинки стикера.
pub fn sticker_url(name: &str) -> String {
    format!("{CDN}/sticker/{}", urlencode(name))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Значение атрибута вида key="value" или key=value из содержимого тега.
fn attr(tag: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(key) {
        let start = from + pos;
        from = start + key.len();
        // Ключ должен начинаться с начала слова (иначе src найдётся внутри thumbnail_src).
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

/// Теги оформления: сами теги убираем, содержимое оставляем.
const FORMAT_TAGS: &[&str] = &[
    "b", "i", "u", "s", "strike", "spoiler", "quote", "code", "noparse", "h1", "h2", "h3", "list",
    "olist", "*", "hr", "table", "tr", "td", "th",
];

pub fn parse(msg: &str) -> Vec<Seg> {
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
        // Содержимое до парного закрывающего тега и остаток после него.
        let inner = |name: &str| -> Option<(&str, &str)> {
            let end = format!("[/{name}]");
            let pos = body.find(&end).or_else(|| body.find(&end.to_uppercase()))?;
            Some((&body[..pos], &body[pos + end.len()..]))
        };

        if !closing {
            match name.as_str() {
                "emoticon" => {
                    if let Some((b, tail)) = inner("emoticon") {
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Emoticon(b.trim().to_string()));
                        rest = tail;
                        continue;
                    }
                }
                "gameinvite" => {
                    // Приглашение в игру из чата Steam.
                    flush(&mut text, &mut segs);
                    segs.push(Seg::Text("[приглашение в игру]".into()));
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
        // Неизвестная скобка — оставляем как есть.
        text.push('[');
        rest = &after[1..];
    }
    text.push_str(rest);
    flush(&mut text, &mut segs);
    segs
}

/// Обычный текст: выделяем смайлики ːимяː и голые ссылки http(s)://.
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


/// Для всплывающего уведомления: текст сообщения и картинка (первый смайлик или стикер).
pub fn preview(msg: &str) -> (String, Option<String>) {
    let mut text = String::new();
    let mut image = None;
    let mut what = "";
    for s in parse(msg) {
        match s {
            Seg::Text(t) => text.push_str(&t),
            Seg::Link { label, .. } => text.push_str(&label),
            Seg::Image { .. } => what = "картинка",
            Seg::Emoticon(n) => {
                image.get_or_insert_with(|| emoticon_url(&n));
                if what.is_empty() {
                    what = "смайлик";
                }
            }
            Seg::Sticker(n) => {
                image.get_or_insert_with(|| sticker_url(&n));
                what = "стикер";
            }
            Seg::RoomEffect(t) => {
                image.get_or_insert_with(|| crate::effects::icon_url(&t));
                text.push_str(&format!("эффект комнаты: {}", crate::effects::short_name(&t)));
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
