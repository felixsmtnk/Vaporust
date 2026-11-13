//! Методы Web API Steam с токеном входа (access_token) - так их вызывает сам клиент Steam
//! и сайт: уведомления, значки, очки Steam, семья. Токен живет около суток, кэшируем его на час.

use aurelia::steam_client::SteamClient;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

async fn token(client: &SteamClient) -> Result<String, String> {
    if let Some((t, at)) = TOKEN.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(t);
        }
    }
    // Сразу после входа Steam иногда не успевает ответить - вторая попытка через 3 с.
    let t = match client.web_access_token().await {
        Ok(t) => t,
        Err(_) => {
            tokio::time::sleep(Duration::from_secs(3)).await;
            client.web_access_token().await.map_err(err)?
        }
    };
    *TOKEN.lock().unwrap() = Some((t.clone(), Instant::now()));
    Ok(t)
}

/// GET-метод: `iface/method/v1` и параметры. Ответ - поле "response".
pub async fn get(client: &SteamClient, method: &str, params: &[(&str, String)]) -> Result<serde_json::Value, String> {
    let t = token(client).await?;
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let mut url = format!("https://api.steampowered.com/{method}/?access_token={t}");
    for (k, v) in params {
        url.push('&');
        url.push_str(k);
        url.push('=');
        url.push_str(&urlenc(v));
    }
    let resp = http.get(url).send().await.map_err(err)?;
    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        *TOKEN.lock().unwrap() = None;
        crate::errlog::write("steam api", &format!("{method}: вход не принят ({status})"));
        return Err("Steam не принял вход (войдите заново).".into());
    }
    if status != 200 {
        crate::errlog::write("steam api", &format!("{method}: код {status}"));
        return Err(format!("Steam ответил кодом {status}."));
    }
    let v: serde_json::Value = resp.json().await.map_err(err)?;
    Ok(v.get("response").cloned().unwrap_or(v))
}

/// POST-метод (действия: отметить прочитанным, купить за очки и т.п.).
pub async fn post(client: &SteamClient, method: &str, params: &[(&str, String)]) -> Result<serde_json::Value, String> {
    let t = token(client).await?;
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let mut body = format!("access_token={t}");
    for (k, v) in params {
        body.push('&');
        body.push_str(k);
        body.push('=');
        body.push_str(&urlenc(v));
    }
    let resp = http
        .post(format!("https://api.steampowered.com/{method}/"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(err)?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("Steam ответил кодом {status}."));
    }
    let v: serde_json::Value = resp.json().await.map_err(err)?;
    Ok(v.get("response").cloned().unwrap_or(v))
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod live {
    /// Живой тест (только чтение): какие методы Web API отвечают с нашим токеном. Печатает только
    /// размеры и ключи ответа, без личных данных.
    #[test]
    #[ignore]
    fn webapi_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap_or(0).to_string();
            for (m, p) in [
                ("ISteamNotificationService/GetSteamNotifications/v1", vec![("language", "russian".to_string()), ("include_read", "true".into())]),
                ("IPlayerService/GetBadges/v1", vec![("steamid", me.clone())]),
                ("ILoyaltyRewardsService/GetSummary/v1", vec![("steamid", me.clone())]),
                ("IFamilyGroupsService/GetFamilyGroupForUser/v1", vec![("steamid", me.clone())]),
                ("IPlayerService/GetRecentlyPlayedGames/v1", vec![("steamid", me.clone())]),
            ] {
                match super::get(&c, m, &p).await {
                    Ok(v) => {
                        let keys: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                        println!("{m}: ок, ключи {keys:?}, {} байт", v.to_string().len());
                    }
                    Err(e) => println!("{m}: {e}"),
                }
            }
        });
    }
}




/// Веб-сеанс сайта Steam на кэшированном токене (не выпрашивать новый токен на каждый запрос:
/// при частых запросах Steam перестает их выдавать - «Response timed out»).
pub async fn web_session(client: &SteamClient) -> Result<aurelia::web::web_session::WebSession, String> {
    let t = token(client).await?;
    let me = client.my_steam_id().ok_or("нет входа")?;
    let cc = client.store_country();
    aurelia::web::web_session::WebSession::new(me, &t, Some(cc.as_str()).filter(|c| !c.is_empty())).map_err(err)
}





