//! Друзья и сообщество через Aurelia: профили, заявки в друзья, приглашение в игру,
//! Мастерская Steam.

use aurelia::core::models::WorkshopItem;
use aurelia::steam_client::SteamClient;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// SteamID64 обычного аккаунта = эта база + номер аккаунта («код друга» в Steam).
pub const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// Разобрать то, что ввел пользователь: SteamID64, ссылку /profiles/<id> или код друга.
pub fn parse_friend_input(input: &str) -> Result<u64, String> {
    let q = input.trim().trim_end_matches('/');
    if q.is_empty() {
        return Err("Введите SteamID, ссылку на профиль или код друга.".into());
    }
    if let Some(pos) = q.find("/profiles/") {
        let id = &q[pos + "/profiles/".len()..];
        let id: String = id.chars().take_while(|c| c.is_ascii_digit()).collect();
        return id.parse().map_err(|_| "Не удалось прочитать номер из ссылки.".to_string());
    }
    if q.contains("/id/") {
        return Err("Короткие ссылки вида steamcommunity.com/id/имя не поддерживаются. Попросите \
                    у друга код друга (Steam → Друзья → Добавить друга) или ссылку /profiles/…"
            .into());
    }
    let n: u64 = q.parse().map_err(|_| "Это не похоже на SteamID, ссылку или код друга.".to_string())?;
    if n >= STEAMID64_BASE {
        Ok(n)
    } else if n > 0 && n < u32::MAX as u64 {
        Ok(STEAMID64_BASE + n) // код друга
    } else {
        Err("Неверный номер.".into())
    }
}

#[derive(Default)]
pub struct WorkshopState {
    /// Результаты поиска: (appid, запрос, сортировка) -> предметы.
    pub results: HashMap<u32, Result<Vec<WorkshopItem>, String>>,
    pub total: HashMap<u32, u32>,
    pub searching: HashSet<u32>,
    /// Свои подписки по игре (id предметов) и их данные.
    pub subscribed: HashMap<u32, Result<Vec<WorkshopItem>, String>>,
    pub sub_ids: HashMap<u32, HashSet<u64>>,
    pub error: Option<String>,
}

pub type SharedWorkshop = Arc<Mutex<WorkshopState>>;

pub enum SocialCmd {
    Profile(u64),
    AddFriend(u64),
    RemoveFriend(u64),
    /// Поиск в Мастерской игры: текст (пусто - без поиска) и тип сортировки Steam
    /// (0 популярные, 3 в тренде, 1 новые, 21 обновленные, 9 по подпискам, 12 по тексту).
    WorkshopQuery { appid: u32, text: String, sort: u32 },
    WorkshopSubscribed(u32),
    WorkshopSubscribe { appid: u32, id: u64, on: bool },
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

pub fn handle(client: &SteamClient, cmd: SocialCmd, st: &crate::net::Shared, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        match cmd {
            SocialCmd::Profile(id) => {
                let res = c.user_profile(id).await.map_err(err);
                st.lock().unwrap().profiles.insert(id, res);
            }
            SocialCmd::AddFriend(id) => {
                let res = c.add_friend(id).await;
                st.lock().unwrap().friend_msg = Some(match res {
                    Ok(a) => format!(
                        "Заявка отправлена{}.",
                        a.persona_name.map(|n| format!(": {n}")).unwrap_or_default()
                    ),
                    Err(e) => format!("Заявка не отправлена: {}", err(e)),
                });
            }
            SocialCmd::RemoveFriend(id) => {
                let res = c.remove_friend(id).await;
                let mut s = st.lock().unwrap();
                s.friend_msg = Some(match res {
                    Ok(()) => "Готово.".into(),
                    Err(e) => format!("Не получилось: {}", err(e)),
                });
                // Убрать из списка сразу, не дожидаясь обновления от Steam.
                s.roster.write().unwrap().remove(&id);
            }
            SocialCmd::WorkshopQuery { appid, text, sort } => {
                let ws = st.lock().unwrap().workshop.clone();
                ws.lock().unwrap().searching.insert(appid);
                ctx.request_repaint();
                let text = text.trim().to_string();
                let sort = if text.is_empty() { sort } else { 12 };
                let res = c
                    .query_workshop_files(appid, (!text.is_empty()).then_some(text.as_str()), sort, "*", 40, &[])
                    .await
                    .map_err(err);
                let mut w = ws.lock().unwrap();
                w.searching.remove(&appid);
                if let Ok(p) = &res {
                    w.total.insert(appid, p.total);
                }
                w.results.insert(appid, res.map(|p| p.items));
            }
            SocialCmd::WorkshopSubscribed(appid) => {
                let ws = st.lock().unwrap().workshop.clone();
                let ids = c.fetch_subscribed_items(appid).await;
                let res = match ids {
                    Ok(ids) => {
                        ws.lock().unwrap().sub_ids.insert(appid, ids.iter().copied().collect());
                        c.fetch_published_file_details(&ids).await.map_err(err)
                    }
                    Err(e) => Err(err(e)),
                };
                ws.lock().unwrap().subscribed.insert(appid, res);
            }
            SocialCmd::WorkshopSubscribe { appid, id, on } => {
                let ws = st.lock().unwrap().workshop.clone();
                let res = if on {
                    c.subscribe_published_file(id, appid).await
                } else {
                    c.unsubscribe_published_file(id, appid).await
                };
                let mut w = ws.lock().unwrap();
                match res {
                    Ok(()) => {
                        w.error = None;
                        let set = w.sub_ids.entry(appid).or_default();
                        if on {
                            set.insert(id);
                        } else {
                            // Из списка подписок не убираем: в свойствах мод остается с
                            // выключенной галочкой, чтобы его можно было включить обратно.
                            set.remove(&id);
                        }
                    }
                    Err(e) => w.error = Some(format!("Подписка не изменена: {}", err(e))),
                }
            }
        }
        ctx.request_repaint();
        if crate::notify::chat_open() {
            ctx.request_repaint_of(crate::chatwin::viewport_id());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_inputs() {
        assert_eq!(parse_friend_input("76561198000000000"), Ok(76561198000000000));
        assert_eq!(parse_friend_input("https://steamcommunity.com/profiles/76561198000000000/"), Ok(76561198000000000));
        assert_eq!(parse_friend_input("39734272"), Ok(76561198000000000)); // код друга
        assert!(parse_friend_input("https://steamcommunity.com/id/someone").is_err());
        assert!(parse_friend_input("abc").is_err());
    }
}
