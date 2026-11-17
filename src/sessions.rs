//! Where the Steam account is logged in: devices and programs with an active login
//! (Web API IAuthenticationService/EnumerateTokens). View only: signing out on a device
//! is done on the Steam site ("Authorized devices").

use crate::webapi;
use aurelia::steam_client::SteamClient;

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub name: String,
    /// "Клиент Steam", "Браузер", "Мобильное приложение"...
    pub kind: String,
    pub logged_in: bool,
    pub last_time: u64,
    pub last_place: String,
    pub first_time: u64,
}

use crate::util::num;

fn kind(platform: u64) -> &'static str {
    match platform {
        1 => tr!("клиент Steam", "Steam client"),
        2 => tr!("браузер", "browser"),
        3 => tr!("мобильное приложение Steam", "Steam mobile app"),
        _ => tr!("другое", "other"),
    }
}

fn place(v: &serde_json::Value) -> String {
    ["city", "state", "country"]
        .iter()
        .filter_map(|k| v.get(*k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn parse(v: &serde_json::Value) -> Vec<Session> {
    let mut out: Vec<Session> = v["refresh_tokens"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| Session {
            name: t["token_description"].as_str().filter(|s| !s.is_empty()).unwrap_or(tr!("без названия", "no name")).to_string(),
            kind: kind(num(&t["platform_type"])).to_string(),
            logged_in: t["logged_in"].as_bool().unwrap_or(false),
            last_time: num(&t["last_seen"]["time"]).max(num(&t["time_updated"])),
            last_place: place(&t["last_seen"]),
            first_time: num(&t["first_seen"]["time"]),
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.last_time));
    out
}

pub async fn load(client: &SteamClient) -> Result<Vec<Session>, String> {
    // This Steam method accepts only POST.
    let v = webapi::post(client, "IAuthenticationService/EnumerateTokens/v1", &[]).await?;
    Ok(parse(&v))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_tokens() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"refresh_tokens":[
            {"token_description":"PC","platform_type":1,"logged_in":true,"time_updated":10,"last_seen":{"time":200,"city":"Tbilisi","country":"GE"},"first_seen":{"time":5}},
            {"token_description":"","platform_type":3,"logged_in":false,"time_updated":300,"last_seen":{}}]}"#,
        )
        .unwrap();
        let s = super::parse(&v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].kind, "мобильное приложение Steam");
        assert_eq!(s[1].last_place, "Tbilisi, GE");
        assert_eq!(s[0].name, "без названия");
    }
}
