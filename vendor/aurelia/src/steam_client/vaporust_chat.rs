//! vaporust: дополнения к чату, которых нет в Aurelia 0.2.3.
//! - список своих смайликов и стикеров (Player.GetEmoticonList);
//! - загрузка картинки в чат (тот же порядок, что у node-steamcommunity sendImageToUser:
//!   beginfileupload -> PUT на выданный адрес -> commitfileupload).

use super::*;
use steam_vent_proto::steammessages_player_steamclient::{
    CPlayer_GetEmoticonList_Request, CPlayer_GetEmoticonList_Response,
};

/// Ответ commit/begin: success бывает числом или строкой.
fn success_of(v: &serde_json::Value) -> i64 {
    v.get("success")
        .and_then(|s| s.as_i64().or_else(|| s.as_str()?.parse().ok()))
        .unwrap_or(0)
}

impl SteamClient {
    /// Свои смайлики и стикеры (имена без двоеточий), самые часто используемые первыми.
    ///
    /// Смайлики — Player.GetEmoticonList; стикеров в этом ответе нет (проверено), они в инвентаре.
    pub async fn fetch_emoticons_and_stickers(&self) -> Result<(Vec<String>, Vec<String>)> {
        let connection = self.require_connection()?;
        let resp: CPlayer_GetEmoticonList_Response = connection
            .service_method(CPlayer_GetEmoticonList_Request::new())
            .await
            .map_err(|e| anyhow!("failed to fetch emoticon list: {e}"))?;

        let mut emoticons: Vec<(u32, String)> = resp
            .emoticons
            .iter()
            .map(|e| (e.use_count(), e.name().trim_matches(':').to_string()))
            .filter(|(_, n)| !n.is_empty())
            .collect();
        emoticons.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        // Стикеры — предметы сообщества в инвентаре (app 753, context 6) с типом «... Sticker».
        // Их имя и есть тип стикера для [sticker type="..."] (проверено: Buoyant Bobber).
        let stickers_res: Result<Vec<String>> = async {
            let web = self.web_session().await?;
            let items = super::market::inventory_via(&web, 753, 6).await?;
            Ok(items
                .into_iter()
                .filter(|i| i.item_type.trim_end().ends_with("Sticker"))
                .map(|i| i.name)
                .collect())
        }
        .await;
        // Без стикеров смайлики всё равно полезны: ошибку инвентаря не считаем фатальной.
        let mut stickers = stickers_res.unwrap_or_default();
        stickers.sort();
        stickers.dedup();
        Ok((emoticons.into_iter().map(|(_, n)| n).collect(), stickers))
    }

    /// Загрузить картинку в чат с другом. Возвращает адрес картинки на серверах Steam.
    /// `ext` — "png" / "jpeg" / "gif" / "webp"; `width`/`height` — размеры картинки.
    pub async fn upload_chat_image(
        &self,
        friend: u64,
        bytes: Vec<u8>,
        ext: &str,
        width: u32,
        height: u32,
    ) -> Result<String> {
        use sha1::{Digest, Sha1};
        let web = self.web_session().await?;
        let sha = hex::encode(Sha1::digest(&bytes));
        let file_name = format!(
            "{}_image.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            if ext == "jpeg" { "jpg" } else { ext }
        );
        let file_type = format!("image/{ext}");
        let referer = "https://steamcommunity.com/chat/";
        let common = |extra: &[(&'static str, String)]| {
            let mut v: Vec<(&str, String)> = vec![
                ("sessionid", web.sessionid().to_string()),
                ("l", "english".into()),
                ("file_name", file_name.clone()),
                ("file_sha", sha.clone()),
                ("file_image_width", width.to_string()),
                ("file_image_height", height.to_string()),
                ("file_type", file_type.clone()),
            ];
            v.extend(extra.iter().cloned());
            v
        };

        // 1. Начать загрузку.
        let (status, text) = web
            .post_form(
                "https://steamcommunity.com/chat/beginfileupload/?l=english",
                &common(&[("file_size", bytes.len().to_string())]),
                referer,
            )
            .await?;
        let begin: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| anyhow!("beginfileupload: HTTP {status}, unexpected answer: {}", text.chars().take(200).collect::<String>()))?;
        if success_of(&begin) != 1 {
            bail!("beginfileupload refused (HTTP {status}): {}", text.chars().take(300).collect::<String>());
        }
        let r = &begin["result"];
        let host = r["url_host"].as_str().ok_or_else(|| anyhow!("beginfileupload: no url_host"))?;
        let path = r["url_path"].as_str().unwrap_or("");
        let scheme = if r["use_https"].as_bool().unwrap_or(true) { "https" } else { "http" };
        let ugcid = match &r["ugcid"] {
            serde_json::Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        let headers: Vec<(String, String)> = r["request_headers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| {
                        Some((h["name"].as_str()?.to_lowercase(), h["value"].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let json_str = |v: &serde_json::Value| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            v => v.to_string(),
        };
        let timestamp = json_str(&begin["timestamp"]);
        let hmac = json_str(&begin["hmac"]);

        // 2. Сам файл.
        let put = web.put_bytes(&format!("{scheme}://{host}{path}"), &headers, bytes).await?;
        if !(200..300).contains(&put) {
            bail!("image upload failed: HTTP {put}");
        }

        // 3. Подтвердить.
        let (status, text) = web
            .post_form(
                "https://steamcommunity.com/chat/commitfileupload/",
                &common(&[
                    ("success", "1".into()),
                    ("ugcid", ugcid),
                    ("timestamp", timestamp),
                    ("hmac", hmac),
                    ("friend_steamid", friend.to_string()),
                    ("spoiler", "0".into()),
                ]),
                referer,
            )
            .await?;
        let commit: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| anyhow!("commitfileupload: HTTP {status}, unexpected answer: {}", text.chars().take(200).collect::<String>()))?;
        if success_of(&commit) != 1 || success_of(&commit["result"]) != 1 {
            bail!("commitfileupload refused (HTTP {status}): {}", text.chars().take(300).collect::<String>());
        }
        commit["result"]["details"]["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("commitfileupload: no image url in answer"))
    }
}
