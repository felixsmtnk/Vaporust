//! Community and store actions (second-tier roadmap items): your own game review, a new
//! discussion topic, points awards, wishlist order, cart, curators, family invites,
//! reactions and group chats, Workshop collections and votes.
//! Almost all of these actions are visible to other people - each one asks for confirmation in the UI.

use crate::webapi;
use aurelia::steam_client::SteamClient;

fn ok_json(status: u16, body: &str) -> Result<serde_json::Value, String> {
    if status >= 400 {
        return Err(trf!("Steam ответил {status}", "Steam answered {status}"));
    }
    serde_json::from_str(body).map_err(|_| tr!("Steam ответил не так, как ожидалось", "Steam answered unexpectedly").to_string())
}

fn success(v: &serde_json::Value) -> bool {
    v["success"].as_i64() == Some(1) || v["success"].as_bool() == Some(true)
}

use crate::util::num;

// ---------- Own review ----------

#[derive(Clone, Debug, Default)]
pub struct MyReview {
    pub text: String,
    pub up: bool,
    pub public: bool,
}

/// Own review of a game (None - not written yet): page /profiles/<me>/recommended/<appid>/.
pub async fn review_load(c: &SteamClient, appid: u32) -> Result<Option<MyReview>, String> {
    let me = c.my_steam_id().ok_or(tr!("нет входа", "not logged in"))?;
    let web = webapi::web_session(c).await?;
    let t = web
        .get_text(&format!("https://steamcommunity.com/profiles/{me}/recommended/{appid}/?l=english"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    let Some(i) = t.find("id=\"ReviewText\"") else { return Ok(None) };
    let rest = &t[i..];
    let body = rest.split_once('>').map(|x| x.1).unwrap_or("");
    let end = body.find("</div>").unwrap_or(body.len());
    let text = crate::store::plain_text(&body[..end].replace("<br>", "\n")).trim().to_string();
    let up = !t.contains("icon_thumbsDown");
    Ok(Some(MyReview { text, up, public: !t.contains("ReviewVisibility_Private") }))
}

/// Write or update your review (like "Write a review" on the store page).
pub async fn review_save(c: &SteamClient, appid: u32, r: &MyReview) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let fields = [
        ("appid", appid.to_string()),
        ("steamworksappid", appid.to_string()),
        ("comment", r.text.clone()),
        ("rated_up", r.up.to_string()),
        ("is_public", r.public.to_string()),
        ("language", crate::lang::steam().into()),
        ("received_compensation", "0".into()),
        ("disable_comments", "0".into()),
        ("sessionid", web.sessionid().to_string()),
    ];
    let page = format!("https://store.steampowered.com/app/{appid}/");
    let (st, body) = web
        .post_form("https://store.steampowered.com/friends/recommendgame", &fields, &page)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v = ok_json(st, &body)?;
    if success(&v) {
        Ok(())
    } else {
        Err(v["strError"].as_str().map(String::from).unwrap_or_else(|| trf!("Steam отказал (код {})", "Steam refused (code {})", v["success"])))
    }
}

// ---------- New discussion topic ----------

/// Create a topic in the game's "General Discussions". Returns the new topic's URL.
pub async fn create_topic(c: &SteamClient, appid: u32, title: &str, text: &str) -> Result<String, String> {
    let web = webapi::web_session(c).await?;
    let page = format!("https://steamcommunity.com/app/{appid}/discussions/0/?l=english");
    let html = web.get_text(&page).await.map_err(|e| format!("{e:#}"))?;
    // InitializeForum( "General_<owner>", {... "feature":"0" ...}, 'https://steamcommunity.com/forum/<owner>/General/' );
    let i = html.find("InitializeForum(").ok_or(tr!("на странице обсуждений нет формы новой темы", "the discussions page has no new topic form"))?;
    let tail = &html[i..];
    let url = tail
        .split("'https://steamcommunity.com/forum/")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .map(|s| format!("https://steamcommunity.com/forum/{s}"))
        .ok_or(tr!("не найден адрес форума", "forum address not found"))?;
    let feature = tail.split("\"feature\":\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("");
    let can_post = tail.contains("\"can_post\":1");
    if !can_post {
        return Err(tr!("Steam не разрешает создавать темы в этом обсуждении (нужна игра на аккаунте или форум закрыт).", "Steam does not allow creating topics in this discussion (you need the game on your account, or the forum is closed).").into());
    }
    let mut action = format!("{url}createtopic/");
    if !feature.is_empty() {
        action.push_str(feature);
        action.push('/');
    }
    let fields = [
        ("sessionid", web.sessionid().to_string()),
        ("appid", appid.to_string()),
        ("topic", title.to_string()),
        ("text", text.to_string()),
        ("subforum", format!("General_{}", if feature.is_empty() { "0" } else { feature })),
    ];
    let (st, body) = web.post_form(&action, &fields, &page).await.map_err(|e| format!("{e:#}"))?;
    let v = ok_json(st, &body)?;
    v["topic_url"].as_str().map(String::from).ok_or_else(|| {
        v["error"].as_str().map(String::from).unwrap_or_else(|| trf!("Steam отказал (код {})", "Steam refused (code {})", v["success"]))
    })
}

// ---------- Points awards ----------

/// Which awards can be given: (award id, price in points).
pub async fn award_config(c: &SteamClient) -> Result<Vec<(u32, u32)>, String> {
    let v = webapi::get(c, "ILoyaltyRewardsService/GetReactionConfig/v1", &[]).await?;
    let mut out: Vec<(u32, u32)> = v["reactions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| (num(&r["reactionid"]) as u32, num(&r["points_cost"]) as u32))
        .filter(|(id, _)| *id > 0)
        .collect();
    out.sort();
    Ok(out)
}

/// Give an award: target_type 1 - review, 2 - item (screenshot, mod), 3 - profile.
pub async fn award(c: &SteamClient, target_type: u32, target: &str, reaction: u32) -> Result<(), String> {
    webapi::post(
        c,
        "ILoyaltyRewardsService/AddReaction/v1",
        &[("target_type", target_type.to_string()), ("targetid", target.to_string()), ("reactionid", reaction.to_string())],
    )
    .await
    .map(|_| ())
}

// ---------- Wishlist order ----------

/// New order: (appid, position from 1). Same as dragging on Steam's wishlist page.
pub async fn wishlist_reorder(c: &SteamClient, items: &[(u32, u32)]) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let updates: Vec<serde_json::Value> = items.iter().map(|(a, p)| serde_json::json!({ "appid": a, "priority": p })).collect();
    let body = serde_json::json!({ "m": "Reorder", "mp": [updates] }).to_string();
    let (st, text) = web
        .post_raw(
            "https://store.steampowered.com/wishlist/action",
            "application/json; charset=utf-8",
            body.into_bytes(),
            "https://store.steampowered.com/wishlist/",
            &[("x-valve-request-type", "mutationAction")],
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v = ok_json(st, &text)?;
    if v["data"]["result"].as_i64() == Some(1) || v["result"].as_i64() == Some(1) {
        Ok(())
    } else {
        Err(trf!("Steam не принял новый порядок ({})", "Steam did not accept the new order ({})", text.chars().take(120).collect::<String>()))
    }
}

// ---------- Cart ----------

#[derive(Clone, Debug, Default)]
pub struct CartLine {
    pub line_id: u64,
    pub packageid: u32,
    pub bundleid: u32,
    pub name: String,
    pub price: String,
}

#[derive(Clone, Debug, Default)]
pub struct Cart {
    pub lines: Vec<CartLine>,
    pub total: String,
}

fn money(v: &serde_json::Value) -> String {
    v["formatted_amount"].as_str().unwrap_or("").to_string()
}

pub async fn cart_get(c: &SteamClient, country: &str) -> Result<Cart, String> {
    let v = webapi::get(c, "IAccountCartService/GetCart/v1", &[("user_country", country.to_string())]).await?;
    let cart = &v["cart"];
    let lines = cart["line_items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| CartLine {
            line_id: num(&l["line_item_id"]),
            packageid: num(&l["packageid"]) as u32,
            bundleid: num(&l["bundleid"]) as u32,
            name: String::new(),
            price: money(&l["price_when_added"]),
        })
        .collect();
    Ok(Cart { lines, total: money(&cart["subtotal"]) })
}

/// Put a game package into the cart (first purchase option from the store page).
pub async fn cart_add(c: &SteamClient, packageid: u32, country: &str) -> Result<(), String> {
    // The Web API accepts a nested item list only via input_json.
    let input = serde_json::json!({ "user_country": country, "items": [{ "packageid": packageid }] }).to_string();
    let v = webapi::post(c, "IAccountCartService/AddItemsToCart/v1", &[("input_json", input)]).await?;
    let added = v["line_item_ids"].as_array().is_some_and(|a| !a.is_empty())
        || v["cart"]["line_items"].as_array().is_some_and(|a| !a.is_empty());
    if added {
        Ok(())
    } else {
        Err(trf!("Steam не добавил товар ({})", "Steam did not add the item ({})", v.to_string().chars().take(120).collect::<String>()))
    }
}

pub async fn cart_remove(c: &SteamClient, line_id: u64, country: &str) -> Result<(), String> {
    let input = serde_json::json!({ "line_item_id": line_id.to_string(), "user_country": country }).to_string();
    webapi::post(c, "IAccountCartService/RemoveItemFromCart/v1", &[("input_json", input)])
    .await
    .map(|_| ())
}

/// Game packages for the cart: (packageid, option name, price) from appdetails.
pub async fn app_packages(appid: u32, cc: &str) -> Result<Vec<(u32, String, String)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&cc={cc}&l={lang}", lang = crate::lang::steam());
    let v: serde_json::Value = http.get(url).send().await.map_err(|e| format!("{e:#}"))?.json().await.map_err(|e| format!("{e:#}"))?;
    let groups = &v[appid.to_string()]["data"]["package_groups"];
    Ok(groups
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["subs"].as_array().cloned().unwrap_or_default())
        .map(|s| {
            let name = crate::store::plain_text(s["option_text"].as_str().unwrap_or(""));
            (num(&s["packageid"]) as u32, name, String::new())
        })
        .filter(|(p, _, _)| *p > 0)
        .collect())
}

// ---------- Curators ----------

#[derive(Clone, Debug, Default)]
pub struct Curator {
    pub name: String,
    pub link: String,
    pub avatar: String,
    pub followers: u64,
    /// 0 - recommended, 1 - not recommended, 2 - informational review.
    pub state: u8,
    pub blurb: String,
    pub review_url: String,
}

/// Curators who reviewed the game (like "Curators" on the store page).
pub async fn curators_for_app(appid: u32) -> Result<Vec<Curator>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let url = format!(
        "https://store.steampowered.com/curators/ajaxgetcurators/render/?query=&start=0&count=10&filter=top_curators_reviewing&appid={appid}&l={lang}",
        lang = crate::lang::steam()
    );
    let v: serde_json::Value = http.get(url).send().await.map_err(|e| format!("{e:#}"))?.json().await.map_err(|e| format!("{e:#}"))?;
    Ok(parse_curators(v["results_html"].as_str().unwrap_or(""), appid))
}

pub fn parse_curators(html: &str, appid: u32) -> Vec<Curator> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find("{\"clanID\":") {
        let start = from + i;
        let mut it = serde_json::Deserializer::from_str(&html[start..]).into_iter::<serde_json::Value>();
        let Some(Ok(c)) = it.next() else {
            from = start + 10;
            continue;
        };
        from = start + it.byte_offset().max(10);
        let rec = c["m_rgAppRecommendations"].as_array().and_then(|a| a.iter().find(|r| num(&r["appid"]) as u32 == appid)).cloned();
        let Some(rec) = rec else { continue };
        let hash = c["strAvatarHash"].as_str().unwrap_or("");
        out.push(Curator {
            name: c["name"].as_str().unwrap_or("").to_string(),
            link: c["link"].as_str().unwrap_or("").to_string(),
            avatar: if hash.is_empty() { String::new() } else { format!("https://avatars.fastly.steamstatic.com/{hash}_medium.jpg") },
            followers: num(&c["total_followers"]),
            state: num(&rec["recommendation_state"]) as u8,
            blurb: crate::store::plain_text(rec["blurb"].as_str().unwrap_or("")),
            review_url: rec["link_url"].as_str().unwrap_or("").to_string(),
        });
    }
    out
}

// ---------- Steam Family ----------

/// Invite to the family (role 1 - adult, 2 - child). Steam sends a confirmation email.
pub async fn family_invite(c: &SteamClient, family: &str, steamid: u64, role: u32) -> Result<(), String> {
    webapi::post(
        c,
        "IFamilyGroupsService/InviteToFamilyGroup/v1",
        &[("family_groupid", family.to_string()), ("receiver_steamid", steamid.to_string()), ("receiver_role", role.to_string())],
    )
    .await
    .map(|_| ())
}

pub async fn family_cancel(c: &SteamClient, family: &str, steamid: u64) -> Result<(), String> {
    webapi::post(
        c,
        "IFamilyGroupsService/CancelFamilyGroupInvite/v1",
        &[("family_groupid", family.to_string()), ("steamid_to_cancel", steamid.to_string())],
    )
    .await
    .map(|_| ())
}

// ---------- Chat: reactions, group chats ----------

pub async fn chat_reaction(c: &SteamClient, friend: u64, time: u32, ordinal: u32, kind: i32, name: &str, add: bool) -> Result<(), String> {
    webapi::post(
        c,
        "IFriendMessagesService/UpdateMessageReaction/v1",
        &[
            ("steamid", friend.to_string()),
            ("server_timestamp", time.to_string()),
            ("ordinal", ordinal.to_string()),
            ("reaction_type", kind.to_string()),
            ("reaction", name.to_string()),
            ("is_add", add.to_string()),
        ],
    )
    .await
    .map(|_| ())
}

/// Create a group chat and invite friends. Returns the group id.
pub async fn create_chat_group(c: &SteamClient, name: &str, invitees: &[u64]) -> Result<u64, String> {
    let mut params: Vec<(String, String)> = vec![("name".into(), name.to_string())];
    for (i, id) in invitees.iter().enumerate() {
        params.push((format!("steamid_invitees[{i}]"), id.to_string()));
    }
    let refs: Vec<(&str, String)> = params.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let v = webapi::post(c, "IChatRoomService/CreateChatRoomGroup/v1", &refs).await?;
    let id = num(&v["chat_group_id"]);
    if id == 0 {
        return Err(tr!("Steam не создал группу", "Steam did not create the group").into());
    }
    Ok(id)
}

pub async fn invite_to_chat_group(c: &SteamClient, group: u64, friend: u64) -> Result<(), String> {
    webapi::post(c, "IChatRoomService/InviteFriendToChatRoomGroup/v1", &[("chat_group_id", group.to_string()), ("steamid", friend.to_string())])
        .await
        .map(|_| ())
}

// ---------- Workshop: collections, votes, favorites ----------

#[derive(Clone, Debug, Default)]
pub struct WsCollection {
    pub id: u64,
    pub title: String,
    pub preview: String,
    pub items: u32,
    pub subscriptions: u64,
}

/// Workshop collections of a game (most popular or by search text).
pub async fn ws_collections(c: &SteamClient, appid: u32, text: &str) -> Result<Vec<WsCollection>, String> {
    let mut params = vec![
        ("appid", appid.to_string()),
        ("creator_appid", appid.to_string()),
        ("filetype", "1".to_string()),
        ("numperpage", "30".to_string()),
        ("return_previews", "true".to_string()),
        ("return_details", "true".to_string()),
        ("return_vote_data", "true".to_string()),
        ("return_children", "true".to_string()),
        ("return_short_description", "true".to_string()),
        ("query_type", if text.trim().is_empty() { "0" } else { "12" }.to_string()),
    ];
    if !text.trim().is_empty() {
        params.push(("search_text", text.trim().to_string()));
    }
    let v = webapi::get(c, "IPublishedFileService/QueryFiles/v1", &params).await?;
    Ok(v["publishedfiledetails"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| WsCollection {
            id: num(&f["publishedfileid"]),
            title: f["title"].as_str().unwrap_or("").to_string(),
            preview: f["preview_url"].as_str().unwrap_or("").to_string(),
            items: f["children"].as_array().map_or(num(&f["num_children"]) as u32, |a| a.len() as u32),
            subscriptions: num(&f["followers"]).max(num(&f["lifetime_followers"])),
        })
        .collect())
}

/// Collection contents: mod ids (in order).
pub async fn ws_collection_items(id: u64) -> Result<Vec<u64>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = http
        .post("https://api.steampowered.com/ISteamRemoteStorage/GetCollectionDetails/v1/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("collectioncount=1&publishedfileids[0]={id}"))
        .send()
        .await
        .map_err(|e| format!("{e:#}"))?
        .json()
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(v["response"]["collectiondetails"][0]["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|ch| num(&ch["filetype"]) == 0)
        .map(|ch| num(&ch["publishedfileid"]))
        .collect())
}

/// Vote on a mod (like the thumbs up/down buttons on the mod page).
pub async fn ws_vote(c: &SteamClient, id: u64, up: bool) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let action = if up { "voteup" } else { "votedown" };
    let fields = [("id", id.to_string()), ("sessionid", web.sessionid().to_string())];
    let page = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}");
    let (st, body) = web
        .post_form(&format!("https://steamcommunity.com/sharedfiles/{action}"), &fields, &page)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v = ok_json(st, &body)?;
    if success(&v) { Ok(()) } else { Err(trf!("Steam отказал (код {})", "Steam refused (code {})", v["success"])) }
}

/// Add a mod to favorites (or remove it).
pub async fn ws_favorite(c: &SteamClient, id: u64, appid: u32, add: bool) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let action = if add { "favorite" } else { "unfavorite" };
    let fields = [("id", id.to_string()), ("appid", appid.to_string()), ("sessionid", web.sessionid().to_string())];
    let page = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}");
    let (st, _) = web
        .post_form(&format!("https://steamcommunity.com/sharedfiles/{action}"), &fields, &page)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if st >= 400 { Err(trf!("Steam ответил {st}", "Steam answered {st}")) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn curators_json() {
        let html = r#"<script>x = [{"clanID":"1","name":"Кошки","link":"https://store.steampowered.com/curator/1-C/","strAvatarHash":"ab","total_followers":20,"m_rgAppRecommendations":[{"appid":70,"blurb":"Классика","recommendation_state":0,"link_url":"https://x"}]},{"clanID":"2","name":"Другой","m_rgAppRecommendations":[{"appid":10}]}];</script>"#;
        let v = super::parse_curators(html, 70);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "Кошки");
        assert_eq!(v[0].blurb, "Классика");
        assert_eq!(v[0].followers, 20);
    }

    /// Read only: curators, Workshop collections, cart, awards.
    #[test]
    #[ignore]
    fn actions_read_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cur = super::curators_for_app(70).await.unwrap();
            println!("curators reviewing HL: {}", cur.len());
            for c in cur.iter().take(3) {
                println!("  {} [{}] {}", c.name, c.state, c.blurb.chars().take(60).collect::<String>());
            }
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let colls = super::ws_collections(&c, 550, "").await.unwrap();
            println!("L4D2 collections: {}", colls.len());
            if let Some(f) = colls.first() {
                let items = super::ws_collection_items(f.id).await.unwrap();
                println!("  \"{}\": mods {} (listed {}), subscribers {}, image {}", f.title, items.len(), f.items, f.subscriptions, !f.preview.is_empty());
            }
            match super::cart_get(&c, "UA").await {
                Ok(cart) => println!("cart: lines {}, total {}", cart.lines.len(), cart.total),
                Err(e) => println!("cart: error {e}"),
            }
            match super::award_config(&c).await {
                Ok(a) => println!("awards: {} (first {:?})", a.len(), a.iter().take(4).collect::<Vec<_>>()),
                Err(e) => println!("awards: error {e}"),
            }
            match super::app_packages(70, "UA").await {
                Ok(p) => println!("HL packages: {:?}", p.iter().take(3).collect::<Vec<_>>()),
                Err(e) => println!("packages: error {e}"),
            }
            match super::review_load(&c, 70).await {
                Ok(r) => println!("own HL review: {}", r.map_or("none".into(), |r| format!("{} chars, up {}", r.text.len(), r.up))),
                Err(e) => println!("review: error {e}"),
            }
        });
    }
}

#[cfg(test)]
mod dump {
    /// Read only: which fields Steam returns for a Workshop collection.
    #[test]
    #[ignore]
    fn collection_fields_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let v = crate::webapi::get(
                &c,
                "IPublishedFileService/QueryFiles/v1",
                &[("appid", "550".into()), ("filetype", "1".into()), ("numperpage", "2".into()), ("return_previews", "true".into()), ("return_vote_data", "true".into()), ("query_type", "0".into())],
            )
            .await
            .unwrap();
            let f = &v["publishedfiledetails"][0];
            let keys: Vec<String> = f.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            println!("keys: {keys:?}");
            println!("subscriptions={} lifetime={} preview_url={} file_url={}", f["subscriptions"], f["lifetime_subscriptions"], f["preview_url"], f["file_url"]);
        });
    }
}

#[cfg(test)]
mod cart_dump {
    /// Read only: raw cart response.
    #[test]
    #[ignore]
    fn cart_raw_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for cc in ["UA", "US"] {
                let v = crate::webapi::get(&c, "IAccountCartService/GetCart/v1", &[("user_country", cc.into())]).await;
                println!("{cc}: {}", v.map(|v| v.to_string().chars().take(700).collect::<String>()).unwrap_or_else(|e| e));
            }
            let w = crate::webapi::get(&c, "IWishlistService/GetWishlist/v1", &[("steamid", c.my_steam_id().unwrap().to_string())]).await;
            let w = w.unwrap(); let items = w["items"].as_array().unwrap(); let mut ranked: Vec<(u64,u64)> = items.iter().map(|i| (i["priority"].as_u64().unwrap_or(0), i["appid"].as_u64().unwrap_or(0))).filter(|p| p.0 > 0).collect(); ranked.sort(); println!("wishlist: total {}, with priority {}, first {:?}", items.len(), ranked.len(), ranked.iter().take(5).collect::<Vec<_>>());
        });
    }
}

#[cfg(test)]
mod cart_live {
    /// Cart: add a wishlisted game package, check, remove (the cart stays as it was).
    #[test]
    #[ignore]
    fn cart_add_remove_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let wl = crate::store::wishlist_ranked(&c, me).await.unwrap();
            println!("wishlist in Steam order, first: {:?}", wl.iter().take(3).collect::<Vec<_>>());
            let mut pkg = None;
            for a in wl.iter().take(15) {
                if let Ok(p) = super::app_packages(*a, "UA").await {
                    if let Some(first) = p.first() {
                        pkg = Some(first.0);
                        break;
                    }
                }
            }
            let pkg = pkg.expect("package");
            let r = super::cart_add(&c, pkg, "UA").await;
            println!("add {pkg}: {r:?}");
            let cart = super::cart_get(&c, "UA").await.unwrap();
            println!("cart lines: {} total {}", cart.lines.len(), cart.total);
            for l in &cart.lines {
                if l.packageid == pkg {
                    println!("remove: {:?}", super::cart_remove(&c, l.line_id, "UA").await);
                }
            }
            let cart = super::cart_get(&c, "UA").await.unwrap();
            println!("after: lines {}", cart.lines.len());
        });
    }
}

// ---------- Download regions ----------

/// City by Steam server code (part of the cacheN-<code>N.steamcontent.com name).
fn region_name(code: &str) -> String {
    let c: String = code.chars().take_while(|ch| ch.is_ascii_alphabetic()).collect();
    let name = match c.as_str() {
        "vie" => tr!("Австрия - Вена", "Austria - Vienna"),
        "waw" => tr!("Польша - Варшава", "Poland - Warsaw"),
        "fra" => tr!("Германия - Франкфурт", "Germany - Frankfurt"),
        "sto" => tr!("Швеция - Стокгольм", "Sweden - Stockholm"),
        "ams" => tr!("Нидерланды - Амстердам", "Netherlands - Amsterdam"),
        "par" => tr!("Франция - Париж", "France - Paris"),
        "lhr" | "lon" => tr!("Великобритания - Лондон", "United Kingdom - London"),
        "mad" => tr!("Испания - Мадрид", "Spain - Madrid"),
        "mil" | "mxp" => tr!("Италия - Милан", "Italy - Milan"),
        "hel" => tr!("Финляндия - Хельсинки", "Finland - Helsinki"),
        "prg" => tr!("Чехия - Прага", "Czechia - Prague"),
        "bud" => tr!("Венгрия - Будапешт", "Hungary - Budapest"),
        "buh" | "otp" => tr!("Румыния - Бухарест", "Romania - Bucharest"),
        "ist" => tr!("Турция - Стамбул", "Turkey - Istanbul"),
        "mow" | "svo" | "dme" => tr!("Россия - Москва", "Russia - Moscow"),
        "kbp" | "iev" => tr!("Украина - Киев", "Ukraine - Kyiv"),
        "iad" | "was" => tr!("США - Вашингтон", "USA - Washington"),
        "atl" => tr!("США - Атланта", "USA - Atlanta"),
        "ord" | "chi" => tr!("США - Чикаго", "USA - Chicago"),
        "dfw" => tr!("США - Даллас", "USA - Dallas"),
        "lax" => tr!("США - Лос-Анджелес", "USA - Los Angeles"),
        "sea" => tr!("США - Сиэтл", "USA - Seattle"),
        "mia" => tr!("США - Майами", "USA - Miami"),
        "nyc" | "jfk" => tr!("США - Нью-Йорк", "USA - New York"),
        "yyz" | "tor" => tr!("Канада - Торонто", "Canada - Toronto"),
        "gru" | "sao" => tr!("Бразилия - Сан-Паулу", "Brazil - Sao Paulo"),
        "eze" | "bue" => tr!("Аргентина - Буэнос-Айрес", "Argentina - Buenos Aires"),
        "scl" => tr!("Чили - Сантьяго", "Chile - Santiago"),
        "lim" => tr!("Перу - Лима", "Peru - Lima"),
        "bog" => tr!("Колумбия - Богота", "Colombia - Bogota"),
        "jnb" => tr!("ЮАР - Йоханнесбург", "South Africa - Johannesburg"),
        "dxb" => tr!("ОАЭ - Дубай", "UAE - Dubai"),
        "bom" => tr!("Индия - Мумбаи", "India - Mumbai"),
        "maa" => tr!("Индия - Ченнаи", "India - Chennai"),
        "sgp" => tr!("Сингапур", "Singapore"),
        "hkg" => tr!("Гонконг", "Hong Kong"),
        "tyo" | "nrt" | "hnd" => tr!("Япония - Токио", "Japan - Tokyo"),
        "sel" | "icn" => tr!("Корея - Сеул", "Korea - Seoul"),
        "syd" => tr!("Австралия - Сидней", "Australia - Sydney"),
        "mel" => tr!("Австралия - Мельбурн", "Australia - Melbourne"),
        "akl" => tr!("Новая Зеландия - Окленд", "New Zealand - Auckland"),
        _ => "",
    };
    if name.is_empty() { c.to_uppercase() } else { name.to_string() }
}

/// Download regions with Steam servers: (region id, name). Queries Steam's server directory
/// for ids 0..=255 (like the region list in Steam settings), 24 requests in parallel.
pub async fn download_regions() -> Result<Vec<(u32, String)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let mut found: std::collections::BTreeMap<u32, String> = Default::default();
    let mut set = tokio::task::JoinSet::new();
    let mut next = 0u32;
    loop {
        while set.len() < 24 && next <= 255 {
            let (h, cell) = (http.clone(), next);
            next += 1;
            set.spawn(async move {
                let url = format!("https://api.steampowered.com/IContentServerDirectoryService/GetServersForSteamPipe/v1/?cell_id={cell}&max_servers=20");
                let v: serde_json::Value = h.get(url).send().await.ok()?.json().await.ok()?;
                Some(v)
            });
        }
        let Some(done) = set.join_next().await else { break };
        let Ok(Some(v)) = done else { continue };
        for s in v["response"]["servers"].as_array().into_iter().flatten() {
            let cell = num(&s["cell_id"]) as u32;
            let host = s["host"].as_str().unwrap_or("");
            // cache1-vie1.steamcontent.com -> vie1
            let Some(code) = host.split('.').next().and_then(|p| p.split('-').nth(1)) else { continue };
            if cell > 0 && host.ends_with(".steamcontent.com") {
                found.entry(cell).or_insert_with(|| region_name(code));
            }
        }
    }
    if found.is_empty() {
        return Err(tr!("Steam не ответил", "Steam did not answer").into());
    }
    let mut v: Vec<(u32, String)> = found.into_iter().collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(v)
}
