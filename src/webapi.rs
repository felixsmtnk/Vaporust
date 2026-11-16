//! Steam Web API methods with the login token (access_token) - the way the Steam client itself
//! and the site call them: notifications, badges, Steam Points, family. The token lives about a day, we cache it for an hour.

use aurelia::steam_client::SteamClient;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The token together with the SteamID of the account it was issued for: after a logout or a login into another
/// account someone else's token is never used.
static TOKEN: Mutex<Option<(u64, String, Instant)>> = Mutex::new(None);

/// Forget the token (logout, new login).
pub fn clear_token() {
    *TOKEN.lock().unwrap() = None;
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

async fn token(client: &SteamClient) -> Result<String, String> {
    let me = client.my_steam_id().ok_or_else(|| tr!("Нет входа в Steam.", "Not logged in to Steam.").to_string())?;
    if let Some((id, t, at)) = TOKEN.lock().unwrap().clone() {
        if id == me && at.elapsed() < Duration::from_secs(3600) {
            return Ok(t);
        }
    }
    // Right after login Steam sometimes does not answer in time - a second attempt after 3 s.
    let t = match client.web_access_token().await {
        Ok(t) => t,
        Err(_) => {
            tokio::time::sleep(Duration::from_secs(3)).await;
            client.web_access_token().await.map_err(err)?
        }
    };
    *TOKEN.lock().unwrap() = Some((me, t.clone(), Instant::now()));
    Ok(t)
}

/// GET method: `iface/method/v1` and parameters. The answer is the "response" field.
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
    // The URL contains the login key: the URL is removed from the error text (without_url) so the key never shows on screen.
    let resp = http.get(url).send().await.map_err(|e| err(e.without_url()))?;
    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        *TOKEN.lock().unwrap() = None;
        crate::errlog::write("steam api", &trf!("{method}: вход не принят ({status})", "{method}: login not accepted ({status})"));
        return Err(tr!("Steam не принял вход (войдите заново).", "Steam did not accept the login (log in again).").into());
    }
    if status != 200 {
        crate::errlog::write("steam api", &trf!("{method}: код {status}", "{method}: code {status}"));
        return Err(trf!("Steam ответил кодом {status}.", "Steam answered with code {status}."));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| err(e.without_url()))?;
    Ok(v.get("response").cloned().unwrap_or(v))
}

/// POST method (actions: mark as read, buy for points and the like).
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
        return Err(trf!("Steam ответил кодом {status}.", "Steam answered with code {status}."));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| err(e.without_url()))?;
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
    /// Live test (read only): which Web API methods answer with our token. Prints only
    /// response sizes and keys, no personal data.
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




/// Steam site web session on the cached token (do not beg for a new token on every request:
/// with frequent requests Steam stops issuing them - "Response timed out").
pub async fn web_session(client: &SteamClient) -> Result<aurelia::web::web_session::WebSession, String> {
    let t = token(client).await?;
    let me = client.my_steam_id().ok_or(tr!("нет входа", "not logged in"))?;
    let cc = client.store_country();
    aurelia::web::web_session::WebSession::new(me, &t, Some(cc.as_str()).filter(|c| !c.is_empty())).map_err(err)
}





