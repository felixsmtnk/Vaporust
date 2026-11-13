//! vaporust: групповые чаты Steam (ChatRoom): мои группы с каналами, история канала, отправка.

use super::*;
use steam_vent_proto::steammessages_chat_steamclient::{
    CChatRoom_GetMessageHistory_Request, CChatRoom_GetMessageHistory_Response, CChatRoom_GetMyChatRoomGroups_Request,
    CChatRoom_GetMyChatRoomGroups_Response, CChatRoom_SendChatMessage_Request, CChatRoom_SendChatMessage_Response,
};

/// Канал группового чата.
#[derive(Debug, Clone, Default)]
pub struct ChatChannel {
    pub chat_id: u64,
    pub name: String,
    pub last_time: u32,
    pub last_message: String,
}

/// Групповой чат (группа с каналами).
#[derive(Debug, Clone, Default)]
pub struct ChatGroup {
    pub group_id: u64,
    pub name: String,
    pub tagline: String,
    pub avatar_url: String,
    pub members_online: u32,
    pub default_chat: u64,
    pub channels: Vec<ChatChannel>,
    /// Несколько участников (номера аккаунтов) - для названия группы без имени.
    pub top_members: Vec<u32>,
}

/// Сообщение группового чата.
#[derive(Debug, Clone, Default)]
pub struct GroupMessage {
    /// Номер аккаунта отправителя (SteamID64 = 76561197960265728 + номер).
    pub sender: u32,
    pub time: u32,
    pub ordinal: u32,
    pub text: String,
    pub deleted: bool,
}

impl SteamClient {
    pub async fn my_chat_groups(&self) -> Result<Vec<ChatGroup>> {
        let connection = self.require_connection()?;
        let resp: CChatRoom_GetMyChatRoomGroups_Response = connection
            .service_method(CChatRoom_GetMyChatRoomGroups_Request::new())
            .await
            .map_err(|e| anyhow!("ChatRoom.GetMyChatRoomGroups failed: {e}"))?;
        Ok(resp
            .chat_room_groups
            .iter()
            .filter_map(|p| p.group_summary.as_ref())
            .map(|g| {
                let sha: String = g.chat_group_avatar_sha().iter().map(|b| format!("{b:02x}")).collect();
                let avatar_url = if !g.avatar_ugc_url().is_empty() {
                    g.avatar_ugc_url().to_string()
                } else if !sha.is_empty() && sha.chars().any(|c| c != '0') {
                    // Так строит адрес клиент Steam (sp.js): {sha}_32/_64/_256.jpg; «_medium» дает 404.
                    format!("https://avatars.fastly.steamstatic.com/{sha}_64.jpg")
                } else {
                    String::new()
                };
                let mut channels: Vec<ChatChannel> = g
                    .chat_rooms
                    .iter()
                    .map(|r| ChatChannel {
                        chat_id: r.chat_id(),
                        name: r.chat_name().to_string(),
                        last_time: r.time_last_message(),
                        last_message: r.last_message().to_string(),
                    })
                    .collect();
                channels.sort_by_key(|c| c.chat_id != g.default_chat_id());
                ChatGroup {
                    group_id: g.chat_group_id(),
                    name: g.chat_group_name().to_string(),
                    tagline: g.chat_group_tagline().to_string(),
                    avatar_url,
                    members_online: g.active_member_count(),
                    default_chat: g.default_chat_id(),
                    channels,
                    top_members: g.top_members.clone(),
                }
            })
            .collect())
    }

    /// Последние `max` сообщений канала (старые первыми).
    pub async fn group_messages(&self, group: u64, chat: u64, max: u32) -> Result<Vec<GroupMessage>> {
        let connection = self.require_connection()?;
        let mut req = CChatRoom_GetMessageHistory_Request::new();
        req.set_chat_group_id(group);
        req.set_chat_id(chat);
        req.set_max_count(max);
        // Сообщения «до этого времени»: без него Steam отдает пустой список.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(u32::MAX);
        req.set_last_time(now);
        let resp: CChatRoom_GetMessageHistory_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("ChatRoom.GetMessageHistory failed: {e}"))?;
        let mut v: Vec<GroupMessage> = resp
            .messages
            .iter()
            .map(|m| GroupMessage {
                sender: m.sender(),
                time: m.server_timestamp(),
                ordinal: m.ordinal(),
                text: m.message().to_string(),
                deleted: m.deleted(),
            })
            .collect();
        v.sort_by_key(|m| (m.time, m.ordinal));
        Ok(v)
    }

    pub async fn send_group_message(&self, group: u64, chat: u64, text: &str) -> Result<()> {
        let connection = self.require_connection()?;
        let mut req = CChatRoom_SendChatMessage_Request::new();
        req.set_chat_group_id(group);
        req.set_chat_id(chat);
        req.set_message(text.to_string());
        req.set_echo_to_sender(true);
        let _: CChatRoom_SendChatMessage_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("ChatRoom.SendChatMessage failed: {e}"))?;
        Ok(())
    }
}

impl SteamClient {
    /// Сделать группы «активными» в этом сеансе (как открытое окно чата в Steam): после этого
    /// Steam отдает историю каналов и присылает новые сообщения.
    pub async fn activate_chat_groups(&self, groups: &[u64]) -> Result<()> {
        use steam_vent_proto::steammessages_chat_steamclient::{
            CChatRoom_SetSessionActiveChatRoomGroups_Request, CChatRoom_SetSessionActiveChatRoomGroups_Response,
        };
        let connection = self.require_connection()?;
        let mut req = CChatRoom_SetSessionActiveChatRoomGroups_Request::new();
        req.chat_group_ids = groups.to_vec();
        req.chat_groups_data_requested = groups.to_vec();
        let _: CChatRoom_SetSessionActiveChatRoomGroups_Response = connection
            .service_method(req)
            .await
            .map_err(|e| anyhow!("ChatRoom.SetSessionActiveChatRoomGroups failed: {e}"))?;
        Ok(())
    }
}
