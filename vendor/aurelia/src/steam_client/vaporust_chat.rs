//! vaporust: chat additions missing in Aurelia 0.2.3.
//! - own emoticons and stickers (Player.GetEmoticonList);
//! - uploading an image to the chat (the same order as node-steamcommunity sendImageToUser:
//!   beginfileupload -> PUT to the given URL -> commitfileupload).

use super::*;
use steam_vent_proto::steammessages_player_steamclient::{
    CPlayer_GetEmoticonList_Request, CPlayer_GetEmoticonList_Response,
};

/// commit/begin response: success may be a number or a string.
fn success_of(v: &serde_json::Value) -> i64 {
    v.get("success")
        .and_then(|s| s.as_i64().or_else(|| s.as_str()?.parse().ok()))
        .unwrap_or(0)
}

impl SteamClient {
    /// Own emoticons and stickers (names without colons), most used first.
    ///
    /// Emoticons - Player.GetEmoticonList; this response has no stickers (checked), they are in the inventory.
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

        // Stickers - community items in the inventory (app 753, context 6) with a type "... Sticker".
        // Their name is the sticker type for [sticker type="..."] (checked: Buoyant Bobber).
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
        // Emoticons are useful even without stickers: an inventory error is not fatal.
        let mut stickers = stickers_res.unwrap_or_default();
        stickers.sort();
        stickers.dedup();
        Ok((emoticons.into_iter().map(|(_, n)| n).collect(), stickers))
    }

    /// Upload an image to the chat with a friend. Returns the image URL on Steam servers.
    /// `ext` - "png" / "jpeg" / "gif" / "webp"; `width`/`height` - image size.
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

        // 1. Begin the upload.
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

        // 2. The file itself.
        let put = web.put_bytes(&format!("{scheme}://{host}{path}"), &headers, bytes).await?;
        if !(200..300).contains(&put) {
            bail!("image upload failed: HTTP {put}");
        }

        // 3. Commit.
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

/// vaporust: a chat history message with its number (ordinal) - for loading older ones.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub from_self: bool,
    pub message: String,
    pub timestamp: u32,
    pub ordinal: u32,
    /// vaporust: message reactions: (type 1 emoticon / 2 sticker, name, who reacted - account numbers).
    pub reactions: Vec<(i32, String, Vec<u32>)>,
}

impl SteamClient {
    /// vaporust: conversation history. `before` = Some((time, ordinal)) of the oldest already
    /// loaded message - then messages before it are returned. Returns (messages newest
    /// to oldest, whether older ones exist).
    pub async fn chat_history(&self, target: u64, count: u32, before: Option<(u32, u32)>) -> Result<(Vec<HistoryMessage>, bool)> {
        use steam_vent_proto::steammessages_friendmessages_steamclient::{
            CFriendMessages_GetRecentMessages_Request, CFriendMessages_GetRecentMessages_Response,
        };
        let connection = self.require_connection()?;
        let me = u64::from(connection.steam_id());
        let me_account = (me & 0xFFFF_FFFF) as u32;
        let mut req = CFriendMessages_GetRecentMessages_Request::new();
        req.set_steamid1(me);
        req.set_steamid2(target);
        req.set_count(count);
        req.set_most_recent_conversation(false);
        req.set_bbcode_format(true);
        if let Some((t, o)) = before {
            req.set_time_last(t);
            req.set_ordinal_last(o);
        }
        let resp: CFriendMessages_GetRecentMessages_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("failed to fetch chat history: {e}"))?;
        let more = resp.more_available();
        let v = resp
            .messages
            .iter()
            .map(|m| HistoryMessage {
                from_self: m.accountid() == me_account,
                message: m.message().to_string(),
                timestamp: m.timestamp(),
                ordinal: m.ordinal(),
                reactions: m
                    .reactions
                    .iter()
                    .map(|r| (r.reaction_type.map(|t| t.value()).unwrap_or(0), r.reaction().to_string(), r.reactors.clone()))
                    .collect(),
            })
            // The boundary message comes again - drop it.
            .filter(|m| before.is_none_or(|(t, o)| (m.timestamp, m.ordinal) < (t, o)))
            .collect();
        Ok((v, more))
    }
}

/// vaporust: what a raw chat notification is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawChatKind {
    /// A message from the friend.
    Message,
    /// Our own message sent from another device.
    Echo,
    /// The friend is typing.
    Typing,
    /// The friend sent a game invite.
    GameInvite,
}

/// vaporust: a chat notification that keeps the server ordinal of the message.
/// steam-vent-chat drops the ordinal, and without it two equal messages sent in the same
/// second cannot be told apart from one message seen twice.
#[derive(Debug, Clone)]
pub struct RawChatEvent {
    pub friend: u64,
    pub kind: RawChatKind,
    pub message: String,
    /// Server time (unix seconds); 0 if Steam did not send one.
    pub timestamp: u64,
    /// Message number within that second; together with the time it identifies the message.
    pub ordinal: u32,
}

/// vaporust: result of sending a message, with its server ordinal.
#[derive(Debug, Clone)]
pub struct SentChatMessage {
    pub server_timestamp: u64,
    pub ordinal: u32,
    pub modified_message: Option<String>,
}

impl SteamClient {
    /// vaporust: incoming chat notifications with ordinals. The stream does not borrow the client.
    pub fn chat_events(&self) -> Result<impl tokio_stream::Stream<Item = RawChatEvent> + 'static> {
        use steam_vent_proto::steammessages_friendmessages_steamclient::CFriendMessages_IncomingMessage_Notification;
        use tokio_stream::StreamExt;
        let connection = self.require_connection_owned()?;
        // The notification stream borrows the connection, so a task owns both and forwards the
        // events; it ends with the first event after the receiver is dropped.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut events = std::pin::pin!(connection.on_notification::<CFriendMessages_IncomingMessage_Notification>());
            while let Some(n) = events.next().await {
                let Ok(n) = n else { continue }; // missed notifications - keep listening
                // chat_entry_type: 1 chat message, 2 typing, 3 game invite (EChatEntryType).
                let kind = match (n.chat_entry_type(), n.local_echo()) {
                    (1, false) => RawChatKind::Message,
                    (1, true) => RawChatKind::Echo,
                    (2, false) => RawChatKind::Typing,
                    (3, false) => RawChatKind::GameInvite,
                    _ => continue,
                };
                let ev = RawChatEvent {
                    friend: n.steamid_friend(),
                    kind,
                    timestamp: n.rtime32_server_timestamp() as u64,
                    ordinal: n.ordinal(),
                    message: n.message.unwrap_or_default(),
                };
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
        Ok(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }

    /// vaporust: send a chat message (BBCode, like Steam) and return its server ordinal.
    pub async fn send_chat_message_ordered(&self, target: u64, message: String) -> Result<SentChatMessage> {
        use steam_vent_proto::steammessages_friendmessages_steamclient::{
            CFriendMessages_SendMessage_Request, CFriendMessages_SendMessage_Response,
        };
        let connection = self.require_connection()?;
        let mut req = CFriendMessages_SendMessage_Request::new();
        req.set_steamid(target);
        req.set_message(message);
        req.set_chat_entry_type(1);
        req.set_contains_bbcode(true);
        let resp: CFriendMessages_SendMessage_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("failed to send chat message: {e}"))?;
        Ok(SentChatMessage {
            server_timestamp: resp.server_timestamp() as u64,
            ordinal: resp.ordinal(),
            modified_message: resp.modified_message,
        })
    }
}
