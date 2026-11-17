//! Friends and community via Aurelia: profiles, friend requests, game invite,
//! Steam Workshop.

use aurelia::core::models::WorkshopItem;
use aurelia::steam_client::SteamClient;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// SteamID64 of a normal account = this base + the account number (the "friend code" in Steam).
pub const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// Short profile name from a steamcommunity.com/id/<name> link (or "id/name").
pub fn vanity_name(input: &str) -> Option<String> {
    let q = input.trim().trim_end_matches('/');
    let pos = q.find("/id/").map(|p| p + 4).or_else(|| q.starts_with("id/").then_some(3))?;
    let name: String =
        q[pos..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    (!name.is_empty()).then_some(name)
}

/// SteamID64 by short name: the profile page as XML (a normal public request).
pub async fn resolve_vanity(name: &str) -> Result<u64, String> {
    let http = aurelia::core::net::steam_web_client().map_err(err)?;
    let url = format!("https://steamcommunity.com/id/{name}/?xml=1");
    let body = http.get(&url).send().await.map_err(err)?.text().await.map_err(err)?;
    let id = body.split("<steamID64>").nth(1).and_then(|s| s.split("</steamID64>").next());
    match id.and_then(|s| s.trim().parse::<u64>().ok()) {
        Some(id) => Ok(id),
        None if body.contains("could not be found") => Err(trf!("Профиль «{name}» не найден.", "Profile \"{name}\" not found.")),
        None => Err(tr!("Steam не ответил, попробуйте еще раз.", "Steam did not answer, try again.").into()),
    }
}

/// Parse what the user entered: a SteamID64, a /profiles/<id> link or a friend code
/// (/id/<name> links go through vanity_name and resolve_vanity).
pub fn parse_friend_input(input: &str) -> Result<u64, String> {
    let q = input.trim().trim_end_matches('/');
    if q.is_empty() {
        return Err(tr!("Введите SteamID, ссылку на профиль или код друга.", "Enter a SteamID, a profile link or a friend code.").into());
    }
    if let Some(pos) = q.find("/profiles/") {
        let id = &q[pos + "/profiles/".len()..];
        let id: String = id.chars().take_while(|c| c.is_ascii_digit()).collect();
        return id.parse().map_err(|_| tr!("Не удалось прочитать номер из ссылки.", "Could not read the number from the link.").to_string());
    }
    let n: u64 = q.parse().map_err(|_| tr!("Это не похоже на SteamID, ссылку или код друга.", "This does not look like a SteamID, a link or a friend code.").to_string())?;
    if n >= STEAMID64_BASE {
        Ok(n)
    } else if n > 0 && n < u32::MAX as u64 {
        Ok(STEAMID64_BASE + n) // friend code
    } else {
        Err(tr!("Неверный номер.", "Wrong number.").into())
    }
}

#[derive(Default)]
pub struct WorkshopState {
    /// Search results: (appid, query, sort) -> items.
    pub results: HashMap<u32, Result<Vec<WorkshopItem>, String>>,
    pub total: HashMap<u32, u32>,
    pub searching: HashSet<u32>,
    /// Own subscriptions per game (item ids) and their data.
    pub subscribed: HashMap<u32, Result<Vec<WorkshopItem>, String>>,
    pub sub_ids: HashMap<u32, HashSet<u64>>,
    /// Mod pages opened inside the program (by mod id).
    pub details: HashMap<u64, Result<crate::workshopui::Detail, String>>,
    /// Short details of many mods at once (collections): one request per 100 mods, without the mod page.
    pub brief: HashMap<u64, Result<crate::workshopui::Detail, String>>,
    pub error: Option<String>,
}

pub type SharedWorkshop = Arc<Mutex<WorkshopState>>;

pub enum SocialCmd {
    Profile(u64),
    /// Look up the SteamID64 by a /id/<name> link.
    ResolveVanity(String),
    AddFriend(u64),
    RemoveFriend(u64),
    /// Block (true) or unblock a user (community site).
    Block(u64, bool),
    /// Search in the game's Workshop: text (empty - no search) and Steam sort type
    /// (0 popular, 3 trending, 1 newest, 21 updated, 9 by subscriptions, 12 by text).
    WorkshopQuery { appid: u32, text: String, sort: u32 },
    WorkshopSubscribed(u32),
    WorkshopSubscribe { appid: u32, id: u64, on: bool },
    /// Mod details for its page inside the program.
    WorkshopDetail(u64),
    /// Short details of many mods (names in a collection) - in batches of 100.
    WorkshopBrief(Vec<u64>),
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

pub fn handle(client: &SteamClient, cmd: SocialCmd, st: &crate::net::Shared, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    crate::net::spawn_session(async move {
        match cmd {
            SocialCmd::ResolveVanity(name) => {
                st.lock().unwrap().vanity.remove(&name);
                let res = resolve_vanity(&name).await;
                if let Ok(id) = res {
                    let p = c.user_profile(id).await.map_err(err);
                    let Some(mut cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                    cur.profiles.insert(id, p);
                }
                let Some(mut cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                cur.vanity.insert(name, res);
                ctx.request_repaint();
            }
            SocialCmd::Profile(id) => {
                let res = c.user_profile(id).await.map_err(err);
                let Some(mut cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                cur.profiles.insert(id, res);
            }
            SocialCmd::AddFriend(id) => {
                let res = c.add_friend(id).await;
                let Some(mut cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                cur.friend_msg = Some(match res {
                    Ok(a) => trf!(
                        "Заявка отправлена{}.", "Request sent{}.",
                        a.persona_name.map(|n| format!(": {n}")).unwrap_or_default()
                    ),
                    Err(e) => trf!("Заявка не отправлена: {}", "Request not sent: {}", err(e)),
                });
            }
            SocialCmd::Block(id, block) => {
                let res = crate::social::block_user(&c, id, block).await;
                let Some(cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                let mut s = cur;
                s.friend_msg = Some(match &res {
                    Ok(()) if block => tr!("Пользователь заблокирован.", "User blocked.").into(),
                    Ok(()) => tr!("Пользователь разблокирован.", "User unblocked.").into(),
                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                });
                if res.is_ok() {
                    if block {
                        s.blocked.insert(id);
                    } else {
                        s.blocked.remove(&id);
                    }
                }
            }
            SocialCmd::RemoveFriend(id) => {
                let res = c.remove_friend(id).await;
                let Some(cur) = crate::net::lock_current(&st) else { return }; // a response of the previous login - not written
                let mut s = cur;
                s.friend_msg = Some(match res {
                    Ok(()) => tr!("Готово.", "Done.").into(),
                    Err(e) => trf!("Не получилось: {}", "Failed: {}", err(e)),
                });
                // Remove from the list right away without waiting for an update from Steam.
                s.roster.write().unwrap().remove(&id);
            }
            SocialCmd::WorkshopBrief(ids) => {
                let ws = st.lock().unwrap().workshop.clone();
                for chunk in ids.chunks(100) {
                    let res = crate::workshopui::load_brief(chunk).await;
                    let mut w = ws.lock().unwrap();
                    for id in chunk {
                        let r = match &res {
                            Ok(m) => m.get(id).cloned().ok_or_else(|| tr!("мод не найден или скрыт автором", "the mod was not found or is hidden by the author").to_string()),
                            Err(e) => Err(e.clone()),
                        };
                        w.brief.insert(*id, r);
                    }
                    drop(w);
                    ctx.request_repaint();
                }
            }
            SocialCmd::WorkshopDetail(id) => {
                let ws = st.lock().unwrap().workshop.clone();
                let res = crate::workshopui::load_detail(&c, id).await;
                ws.lock().unwrap().details.insert(id, res);
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
                            // Not removed from the subscription list: in properties the mod stays with
                            // an unchecked box so it can be turned back on.
                            set.remove(&id);
                        }
                    }
                    Err(e) => w.error = Some(trf!("Подписка не изменена: {}", "Subscription not changed: {}", err(e))),
                }
            }
        }
        ctx.request_repaint();
        if crate::notify::chat_window_open() {
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
        assert_eq!(parse_friend_input("39734272"), Ok(76561198000000000)); // friend code
        assert_eq!(vanity_name("https://steamcommunity.com/id/some_one-2/"), Some("some_one-2".into()));
        assert_eq!(vanity_name("id/abc"), Some("abc".into()));
        assert_eq!(vanity_name("39734272"), None);
        assert!(parse_friend_input("abc").is_err());
    }
}

#[cfg(test)]
mod live {
    /// Live test: a /id/<name> link of a well-known public Valve profile.
    #[test]
    #[ignore]
    fn vanity_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert_eq!(super::resolve_vanity("gabelogannewell").await, Ok(76561197960287930));
            assert!(super::resolve_vanity("zzzz_no_such_vanity_qq123").await.unwrap_err().contains("не найден"));
        });
    }
}

#[cfg(test)]
mod nick_live {
    /// How many friends have a nickname and a detailed in-game status (no names). Read only.
    #[test]
    #[ignore]
    fn nicknames_rich_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let list = c.collect_friends(std::time::Duration::from_secs(8)).await.unwrap();
            let nick = list.iter().filter(|f| f.nickname.is_some()).count();
            let game = list.iter().filter(|f| f.game_app_id.is_some()).count();
            let rich: Vec<&str> = list.iter().filter_map(|f| f.rich_status.as_deref()).collect();
            println!("friends {}, with a nickname {nick}, in game {game}, with rich status {}: {:?}", list.len(), rich.len(), rich);
        });
    }
}

/// Blocking via the community site (like "More -> Block" on a Steam profile): the answer is {"success":1}.
pub async fn block_user(c: &SteamClient, id: u64, block: bool) -> Result<(), String> {
    let web = crate::webapi::web_session(c).await?;
    let fields = [
        ("sessionID", web.sessionid().to_string()),
        ("steamid", id.to_string()),
        ("block", if block { "1" } else { "0" }.to_string()),
    ];
    let t = web
        .post_form("https://steamcommunity.com/actions/BlockUserAjax", &fields, &format!("https://steamcommunity.com/profiles/{id}/"))
        .await
        .map_err(|e| format!("{e:#}"))?
        .1;
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|_| tr!("Steam ответил не так, как ожидалось", "Steam answered unexpectedly").to_string())?;
    if v["success"].as_i64() == Some(1) || v["success"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(trf!("Steam отказал (код {})", "Steam refused (code {})", v["success"]))
    }
}

/// Shown while a friend's name is not known yet. Never the account number: a SteamID is private
/// and looks like a broken name.
pub fn unknown_name() -> String {
    tr!("загружается...", "loading...").to_string()
}
