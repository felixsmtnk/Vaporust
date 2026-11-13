//! Когда игра куплена и можно ли ее вернуть. Даты - со страницы лицензий аккаунта в магазине
//! Steam (store.steampowered.com/account/licenses). Условия возврата Steam: не больше 14 дней
//! с покупки и не больше 2 часов в игре (решает сам Steam, у нас только подсказка).

use crate::webapi;
use aurelia::steam_client::SteamClient;

#[derive(Clone, Debug, PartialEq)]
pub struct License {
    /// Время получения (unix, полночь UTC дня).
    pub date: i64,
    pub name: String,
    /// «Steam Store», «Complimentary», «Gift/Guest Pass», «Retail»...
    pub method: String,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&#39;", "'").replace("&quot;", "\"").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Строки таблицы лицензий (l=english).
pub fn parse(html: &str) -> Vec<License> {
    // В разметке Steam «<td  class=...>» с двумя пробелами - режем по самому классу.
    html.split("class=\"license_date_col\">")
        .skip(1)
        .filter(|row| !row.starts_with("Date<"))
        .filter_map(|row| {
            let date_text = row.split('<').next()?.trim();
            let date = crate::store::parse_en_date(date_text).or_else(|| {
                // Без года - текущий год.
                crate::store::parse_en_date(&format!("{date_text}, {}", chrono::Local::now().format("%Y")))
            })?;
            let item = between(row, "<td>", "</td>")?;
            // Ссылка «Remove» у бесплатных лицензий - в своем div, его пропускаем.
            let item = match item.find("</div>") {
                Some(i) => &item[i + 6..],
                None => item,
            };
            let method = between(row, "license_acquisition_col\">", "</td>").map(clean).unwrap_or_default();
            Some(License { date, name: clean(item), method })
        })
        .collect()
}

pub async fn load(client: &SteamClient) -> Result<Vec<License>, String> {
    let web = webapi::web_session(client).await?;
    let html = web
        .get_text("https://store.steampowered.com/account/licenses/?l=english")
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(parse(&html))
}

/// Имя для сравнения: строчные буквы и цифры, без ™®©.
fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Лицензия этой игры: то же название или название с припиской издания.
pub fn find<'a>(game: &str, list: &'a [License]) -> Option<&'a License> {
    let g = norm(game);
    if g.is_empty() {
        return None;
    }
    let editions = ["edition", "deluxe", "standard", "game of the year", "goty", "complete", "remastered"];
    list.iter()
        .filter(|l| {
            let n = norm(&l.name);
            n == g || (n.starts_with(&format!("{g} ")) && editions.iter().any(|e| n[g.len()..].contains(e)))
        })
        .min_by_key(|l| l.date)
}

/// Подсказка о возврате: Some(текст), если игра куплена в магазине меньше 14 дней назад и
/// наиграно меньше 2 часов.
pub fn hint(l: &License, playtime_min: u32, now: i64) -> Option<String> {
    if l.method != "Steam Store" {
        return None;
    }
    let days = (now - l.date) / 86_400;
    if days > 14 || playtime_min >= 120 {
        return None;
    }
    let until = chrono::DateTime::from_timestamp(l.date + 14 * 86_400, 0)?.with_timezone(&chrono::Local);
    Some(format!(
        "Можно вернуть: куплена {} дн. назад, наиграно {} мин из 120. Срок до {}.",
        days,
        playtime_min,
        until.format("%d.%m.%Y")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match() {
        let html = r#"<td class="license_date_col">11 Nov, 2026</td>
        <td><div class="free_license_remove_link"><a href="x">Remove</a></div> CHAOS;HEAD NOAH - COSTUME DLC </td>
        <td class="license_acquisition_col"> Complimentary </td></tr>
        <td class="license_date_col">3 Nov, 2026</td><td> Hollow Knight&trade; Game of the Year Edition </td>
        <td class="license_acquisition_col"> Steam Store </td>"#;
        let v = parse(html);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "CHAOS;HEAD NOAH - COSTUME DLC");
        assert_eq!(v[1].method, "Steam Store");
        assert!(find("CHAOS;HEAD NOAH", &v).is_none(), "DLC - не сама игра");
        let lic = License { date: v[1].date, name: "Hollow Knight Game of the Year Edition".into(), method: "Steam Store".into() };
        assert!(find("Hollow Knight", &[lic.clone()]).is_some());
        assert!(hint(&lic, 30, lic.date + 3 * 86_400).unwrap().contains("3 дн."));
        assert!(hint(&lic, 130, lic.date + 3 * 86_400).is_none());
        assert!(hint(&lic, 30, lic.date + 20 * 86_400).is_none());
    }

    #[test]
    #[ignore]
    fn licenses_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let v = load(&c).await.unwrap();
            let store = v.iter().filter(|l| l.method == "Steam Store").count();
            println!("лицензий {}, куплено в магазине {}", v.len(), store);
            let owned = c.fetch_owned_games().await.unwrap();
            let matched = owned.iter().filter(|g| find(&g.name, &v).is_some()).count();
            println!("игр с найденной датой покупки: {matched} из {}", owned.len());
            assert!(!v.is_empty());
        });
    }
}

