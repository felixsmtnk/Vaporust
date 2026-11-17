//! Steam community text (guides, discussions, reviews) for display in a window: HTML with
//! lines, images and headings -> "text / heading / image" pieces. Also replaces
//! characters missing from the program's font (arrows, lines) so there are no squares.

use eframe::egui;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Text(String),
    Heading(String),
    Image(String),
}

fn decode(s: &str) -> String {
    crate::util::html_unescape(s)
}

/// Remove tags while keeping line breaks (<br>, </div>, </p>, list items).
fn strip(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.trim().to_lowercase();
                if t.starts_with("br") || t == "/p" || t == "/div" || t.starts_with("/h") || t == "/li" {
                    out.push('\n');
                } else if t.starts_with("li") {
                    out.push_str("\n• ");
                }
            }
            c if in_tag => tag.push(c),
            c => out.push(c),
        }
    }
    let text = decode(&out);
    // Spaces inside lines - one at a time; no more than two empty lines in a row.
    let mut lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_string()
}

/// HTML -> pieces. Headings are bb_h1/bb_h2/bb_h3 blocks, images are <img src>.
pub fn parse(html: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut rest = html;
    loop {
        let img = rest.find("<img");
        let head = rest.find("class=\"bb_h");
        let next = match (img, head) {
            (Some(i), Some(h)) => Some(i.min(h)),
            (a, b) => a.or(b),
        };
        let Some(pos) = next else { break };
        // Text before the tag (for a heading - before its <div starts).
        let cut = if Some(pos) == head { rest[..pos].rfind('<').unwrap_or(pos) } else { pos };
        let t = strip(&rest[..cut]);
        if !t.is_empty() {
            out.push(Item::Text(t));
        }
        if Some(pos) == img {
            let end = rest[pos..].find('>').map(|e| pos + e + 1).unwrap_or(rest.len());
            let tag = &rest[pos..end];
            if let Some(src) = tag.split("src=\"").nth(1).and_then(|s| s.split('"').next()) {
                if src.starts_with("http") && !src.contains("trans.gif") {
                    out.push(Item::Image(decode(src)));
                }
            }
            rest = &rest[end..];
        } else {
            // Heading: from '>' after class to the closing </div>; images inside are separate.
            let start = rest[pos..].find('>').map(|e| pos + e + 1).unwrap_or(rest.len());
            let end = rest[start..].find("</div>").map(|e| start + e).unwrap_or(rest.len());
            let inner = &rest[start..end];
            for im in inner.split("<img").skip(1) {
                if let Some(src) = im.split("src=\"").nth(1).and_then(|s| s.split('"').next()) {
                    if src.starts_with("http") {
                        out.push(Item::Image(decode(src)));
                    }
                }
            }
            let h = strip(inner);
            if !h.is_empty() {
                out.push(Item::Heading(h));
            }
            rest = &rest[(end + 6).min(rest.len())..];
        }
    }
    let t = strip(rest);
    if !t.is_empty() {
        out.push(Item::Text(t));
    }
    out
}

/// Replace characters missing from the font: arrows, lines; remove the other missing ones.
pub fn fix(ui: &egui::Ui, s: &str) -> String {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts(|f| {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if c.is_ascii() || c == '\n' || f.has_glyph(&font, c) {
                out.push(c);
                continue;
            }
            match c {
                '\u{2192}' | '\u{21D2}' | '\u{27A1}' | '\u{279C}' | '\u{2794}' => out.push_str("->"),
                '\u{2190}' | '\u{21D0}' | '\u{2B05}' => out.push_str("<-"),
                '\u{2191}' | '\u{2B06}' => out.push('^'),
                '\u{2193}' | '\u{2B07}' => out.push('v'),
                '\u{2500}'..='\u{257F}' | '\u{25AC}' | '\u{2014}' | '\u{2013}' => out.push('-'),
                '\u{25CF}' | '\u{25AA}' | '\u{25A0}' | '\u{25B6}' | '\u{25BA}' => out.push('•'),
                '\u{2714}' | '\u{2705}' | '\u{2611}' => out.push('+'),
                '\u{FE0F}' | '\u{200D}' => {}
                _ => {}
            }
        }
        out
    })
}

/// Show pieces: wrapped text, larger headings, images fit to width (up to 640).
pub fn show(ui: &mut egui::Ui, items: &[Item]) {
    for it in items {
        match it {
            Item::Text(t) => {
                let t = fix(ui, t);
                ui.add(egui::Label::new(t).wrap());
                ui.add_space(4.0);
            }
            Item::Heading(t) => {
                ui.add_space(6.0);
                let t = fix(ui, t);
                ui.label(egui::RichText::new(t).size(16.0).strong());
            }
            Item::Image(u) => {
                let w = ui.available_width().min(640.0);
                // Community images can be huge - ask Steam for a reduced copy.
                let url = ugc_small(u, 640);
                ui.add(egui::Image::new(url).max_width(w).max_height(420.0));
                ui.add_space(4.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_guide_html() {
        let html = r#"Intro line<br><br><div class="bb_h1">1. <a href="x"><img src="https://img/a.jpg" class="s"></a><b>Price</b></div> Survive it.<br>Next &quot;line&quot;"#;
        let v = parse(html);
        assert_eq!(
            v,
            vec![
                Item::Text("Intro line".into()),
                Item::Image("https://img/a.jpg".into()),
                Item::Heading("1. Price".into()),
                Item::Text("Survive it.\nNext \"line\"".into()),
            ]
        );
    }
}

/// Steam community image (steamusercontent) - as a reduced copy at most `px` wide:
/// originals can be several megabytes and animated GIFs.
pub fn ugc_small(url: &str, px: u32) -> String {
    if url.contains("steamusercontent.com") && !url.contains('?') {
        format!("{url}?imw={px}&imh={px}&ima=fit&impolicy=Letterbox")
    } else {
        url.to_string()
    }
}
