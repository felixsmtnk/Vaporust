//! Действия в сообществе и магазине (второй план дорожной карты): свой обзор игры, новая тема
//! в обсуждениях, награды за очки, порядок списка желаемого, корзина, кураторы, приглашения в
//! семью, реакции и групповые чаты, коллекции и оценки в Мастерской.
//! Почти все эти действия видны другим людям - в интерфейсе каждое с подтверждением.

use crate::webapi;
use aurelia::steam_client::SteamClient;

fn ok_json(status: u16, body: &str) -> Result<serde_json::Value, String> {
    if status >= 400 {
        return Err(format!("Steam ответил {status}"));
    }
    serde_json::from_str(body).map_err(|_| "Steam ответил не так, как ожидалось".to_string())
}

fn success(v: &serde_json::Value) -> bool {
    v["success"].as_i64() == Some(1) || v["success"].as_bool() == Some(true)
}

fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}

// ---------- Свой обзор ----------

#[derive(Clone, Debug, Default)]
pub struct MyReview {
    pub text: String,
    pub up: bool,
    pub public: bool,
}

/// Свой обзор игры (None - еще не писали): страница /profiles/<я>/recommended/<appid>/.
pub async fn review_load(c: &SteamClient, appid: u32) -> Result<Option<MyReview>, String> {
    let me = c.my_steam_id().ok_or("нет входа")?;
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

/// Написать или обновить свой обзор (как «Написать обзор» на странице игры в магазине).
pub async fn review_save(c: &SteamClient, appid: u32, r: &MyReview) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let fields = [
        ("appid", appid.to_string()),
        ("steamworksappid", appid.to_string()),
        ("comment", r.text.clone()),
        ("rated_up", r.up.to_string()),
        ("is_public", r.public.to_string()),
        ("language", crate::lang::STEAM.into()),
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
        Err(v["strError"].as_str().map(String::from).unwrap_or_else(|| format!("Steam отказал (код {})", v["success"])))
    }
}

// ---------- Новая тема в обсуждениях ----------

/// Создать тему в «Общем обсуждении» игры. Возвращает адрес новой темы.
pub async fn create_topic(c: &SteamClient, appid: u32, title: &str, text: &str) -> Result<String, String> {
    let web = webapi::web_session(c).await?;
    let page = format!("https://steamcommunity.com/app/{appid}/discussions/0/");
    let html = web.get_text(&page).await.map_err(|e| format!("{e:#}"))?;
    // InitializeForum( "General_<owner>", {... "feature":"0" ...}, 'https://steamcommunity.com/forum/<owner>/General/' );
    let i = html.find("InitializeForum(").ok_or("на странице обсуждений нет формы новой темы")?;
    let tail = &html[i..];
    let url = tail
        .split("'https://steamcommunity.com/forum/")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .map(|s| format!("https://steamcommunity.com/forum/{s}"))
        .ok_or("не найден адрес форума")?;
    let feature = tail.split("\"feature\":\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("");
    let can_post = tail.contains("\"can_post\":1");
    if !can_post {
        return Err("Steam не разрешает создавать темы в этом обсуждении (нужна игра на аккаунте или форум закрыт).".into());
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
        v["error"].as_str().map(String::from).unwrap_or_else(|| format!("Steam отказал (код {})", v["success"]))
    })
}

// ---------- Награды за очки ----------

/// Какие награды можно дать: (номер награды, цена в очках).
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

/// Наградить: target_type 1 - обзор, 2 - работа (скриншот, мод), 3 - профиль.
pub async fn award(c: &SteamClient, target_type: u32, target: &str, reaction: u32) -> Result<(), String> {
    webapi::post(
        c,
        "ILoyaltyRewardsService/AddReaction/v1",
        &[("target_type", target_type.to_string()), ("targetid", target.to_string()), ("reactionid", reaction.to_string())],
    )
    .await
    .map(|_| ())
}

// ---------- Список желаемого: порядок ----------

/// Новый порядок: (appid, место с 1). Так же, как перетаскивание на странице желаемого Steam.
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
        Err(format!("Steam не принял новый порядок ({})", text.chars().take(120).collect::<String>()))
    }
}

// ---------- Корзина ----------

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

/// Положить в корзину пакет игры (первый вариант покупки со страницы магазина).
pub async fn cart_add(c: &SteamClient, packageid: u32, country: &str) -> Result<(), String> {
    // Вложенный список товаров Web API понимает только в input_json.
    let input = serde_json::json!({ "user_country": country, "items": [{ "packageid": packageid }] }).to_string();
    let v = webapi::post(c, "IAccountCartService/AddItemsToCart/v1", &[("input_json", input)]).await?;
    let added = v["line_item_ids"].as_array().is_some_and(|a| !a.is_empty())
        || v["cart"]["line_items"].as_array().is_some_and(|a| !a.is_empty());
    if added {
        Ok(())
    } else {
        Err(format!("Steam не добавил товар ({})", v.to_string().chars().take(120).collect::<String>()))
    }
}

pub async fn cart_remove(c: &SteamClient, line_id: u64, country: &str) -> Result<(), String> {
    let input = serde_json::json!({ "line_item_id": line_id.to_string(), "user_country": country }).to_string();
    webapi::post(c, "IAccountCartService/RemoveItemFromCart/v1", &[("input_json", input)])
    .await
    .map(|_| ())
}

/// Пакеты игры для корзины: (packageid, название варианта, цена) из appdetails.
pub async fn app_packages(appid: u32, cc: &str) -> Result<Vec<(u32, String, String)>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&cc={cc}&l={lang}", lang = crate::lang::STEAM);
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

// ---------- Кураторы ----------

#[derive(Clone, Debug, Default)]
pub struct Curator {
    pub name: String,
    pub link: String,
    pub avatar: String,
    pub followers: u64,
    /// 0 - рекомендует, 1 - не рекомендует, 2 - информационный обзор.
    pub state: u8,
    pub blurb: String,
    pub review_url: String,
}

/// Кураторы, написавшие обзор игры (как «Кураторы» на странице игры).
pub async fn curators_for_app(appid: u32) -> Result<Vec<Curator>, String> {
    let http = aurelia::core::net::steam_web_client().map_err(|e| format!("{e:#}"))?;
    let url = format!(
        "https://store.steampowered.com/curators/ajaxgetcurators/render/?query=&start=0&count=10&filter=top_curators_reviewing&appid={appid}&l={lang}",
        lang = crate::lang::STEAM
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

// ---------- Семья Steam ----------

/// Пригласить в семью (role 1 - взрослый, 2 - ребенок). Steam пришлет подтверждение на почту.
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

// ---------- Чат: реакции, групповые чаты ----------

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

/// Создать групповой чат и пригласить друзей. Возвращает номер группы.
pub async fn create_chat_group(c: &SteamClient, name: &str, invitees: &[u64]) -> Result<u64, String> {
    let mut params: Vec<(String, String)> = vec![("name".into(), name.to_string())];
    for (i, id) in invitees.iter().enumerate() {
        params.push((format!("steamid_invitees[{i}]"), id.to_string()));
    }
    let refs: Vec<(&str, String)> = params.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let v = webapi::post(c, "IChatRoomService/CreateChatRoomGroup/v1", &refs).await?;
    let id = num(&v["chat_group_id"]);
    if id == 0 {
        return Err("Steam не создал группу".into());
    }
    Ok(id)
}

pub async fn invite_to_chat_group(c: &SteamClient, group: u64, friend: u64) -> Result<(), String> {
    webapi::post(c, "IChatRoomService/InviteFriendToChatRoomGroup/v1", &[("chat_group_id", group.to_string()), ("steamid", friend.to_string())])
        .await
        .map(|_| ())
}

// ---------- Мастерская: коллекции, оценка, избранное ----------

#[derive(Clone, Debug, Default)]
pub struct WsCollection {
    pub id: u64,
    pub title: String,
    pub preview: String,
    pub items: u32,
    pub subscriptions: u64,
}

/// Коллекции Мастерской игры (самые популярные или по запросу).
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

/// Состав коллекции: номера модов (по порядку).
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

/// Оценить мод (как кнопки «палец вверх/вниз» на странице мода).
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
    if success(&v) { Ok(()) } else { Err(format!("Steam отказал (код {})", v["success"])) }
}

/// Добавить мод в избранное (или убрать).
pub async fn ws_favorite(c: &SteamClient, id: u64, appid: u32, add: bool) -> Result<(), String> {
    let web = webapi::web_session(c).await?;
    let action = if add { "favorite" } else { "unfavorite" };
    let fields = [("id", id.to_string()), ("appid", appid.to_string()), ("sessionid", web.sessionid().to_string())];
    let page = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}");
    let (st, _) = web
        .post_form(&format!("https://steamcommunity.com/sharedfiles/{action}"), &fields, &page)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if st >= 400 { Err(format!("Steam ответил {st}")) } else { Ok(()) }
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

    /// Только чтение: кураторы, коллекции Мастерской, корзина, награды.
    #[test]
    #[ignore]
    fn actions_read_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cur = super::curators_for_app(70).await.unwrap();
            println!("кураторов с обзором HL: {}", cur.len());
            for c in cur.iter().take(3) {
                println!("  {} [{}] {}", c.name, c.state, c.blurb.chars().take(60).collect::<String>());
            }
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let colls = super::ws_collections(&c, 550, "").await.unwrap();
            println!("коллекций L4D2: {}", colls.len());
            if let Some(f) = colls.first() {
                let items = super::ws_collection_items(f.id).await.unwrap();
                println!("  «{}»: модов {} (в списке {}), подписчиков {}, картинка {}", f.title, items.len(), f.items, f.subscriptions, !f.preview.is_empty());
            }
            match super::cart_get(&c, "UA").await {
                Ok(cart) => println!("корзина: строк {}, итого {}", cart.lines.len(), cart.total),
                Err(e) => println!("корзина: ошибка {e}"),
            }
            match super::award_config(&c).await {
                Ok(a) => println!("наград: {} (первые {:?})", a.len(), a.iter().take(4).collect::<Vec<_>>()),
                Err(e) => println!("награды: ошибка {e}"),
            }
            match super::app_packages(70, "UA").await {
                Ok(p) => println!("пакеты HL: {:?}", p.iter().take(3).collect::<Vec<_>>()),
                Err(e) => println!("пакеты: ошибка {e}"),
            }
            match super::review_load(&c, 70).await {
                Ok(r) => println!("свой обзор HL: {}", r.map_or("нет".into(), |r| format!("{} знаков, up {}", r.text.len(), r.up))),
                Err(e) => println!("обзор: ошибка {e}"),
            }
        });
    }
}

#[cfg(test)]
mod dump {
    /// Только чтение: какие поля Steam отдает для коллекции Мастерской.
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
            println!("ключи: {keys:?}");
            println!("subscriptions={} lifetime={} preview_url={} file_url={}", f["subscriptions"], f["lifetime_subscriptions"], f["preview_url"], f["file_url"]);
        });
    }
}

#[cfg(test)]
mod cart_dump {
    /// Только чтение: сырой ответ корзины.
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
            let w = w.unwrap(); let items = w["items"].as_array().unwrap(); let mut ranked: Vec<(u64,u64)> = items.iter().map(|i| (i["priority"].as_u64().unwrap_or(0), i["appid"].as_u64().unwrap_or(0))).filter(|p| p.0 > 0).collect(); ranked.sort(); println!("wishlist: всего {}, с приоритетом {}, первые {:?}", items.len(), ranked.len(), ranked.iter().take(5).collect::<Vec<_>>());
        });
    }
}

#[cfg(test)]
mod cart_live {
    /// Корзина: положить пакет игры из желаемого, проверить, убрать (корзина остается как была).
    #[test]
    #[ignore]
    fn cart_add_remove_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let wl = crate::store::wishlist_ranked(&c, me).await.unwrap();
            println!("желаемое по порядку Steam, первые: {:?}", wl.iter().take(3).collect::<Vec<_>>());
            let mut pkg = None;
            for a in wl.iter().take(15) {
                if let Ok(p) = super::app_packages(*a, "UA").await {
                    if let Some(first) = p.first() {
                        pkg = Some(first.0);
                        break;
                    }
                }
            }
            let pkg = pkg.expect("пакет");
            let r = super::cart_add(&c, pkg, "UA").await;
            println!("добавить {pkg}: {r:?}");
            let cart = super::cart_get(&c, "UA").await.unwrap();
            println!("в корзине строк: {} итого {}", cart.lines.len(), cart.total);
            for l in &cart.lines {
                if l.packageid == pkg {
                    println!("убрать: {:?}", super::cart_remove(&c, l.line_id, "UA").await);
                }
            }
            let cart = super::cart_get(&c, "UA").await.unwrap();
            println!("после: строк {}", cart.lines.len());
        });
    }
}

// ---------- Регионы загрузки ----------

/// Город по коду сервера Steam (часть имени cacheN-<код>N.steamcontent.com).
fn region_name(code: &str) -> String {
    let c: String = code.chars().take_while(|ch| ch.is_ascii_alphabetic()).collect();
    let name = match c.as_str() {
        "vie" => "Австрия - Вена",
        "waw" => "Польша - Варшава",
        "fra" => "Германия - Франкфурт",
        "sto" => "Швеция - Стокгольм",
        "ams" => "Нидерланды - Амстердам",
        "par" => "Франция - Париж",
        "lhr" | "lon" => "Великобритания - Лондон",
        "mad" => "Испания - Мадрид",
        "mil" | "mxp" => "Италия - Милан",
        "hel" => "Финляндия - Хельсинки",
        "prg" => "Чехия - Прага",
        "bud" => "Венгрия - Будапешт",
        "buh" | "otp" => "Румыния - Бухарест",
        "ist" => "Турция - Стамбул",
        "mow" | "svo" | "dme" => "Россия - Москва",
        "kbp" | "iev" => "Украина - Киев",
        "iad" | "was" => "США - Вашингтон",
        "atl" => "США - Атланта",
        "ord" | "chi" => "США - Чикаго",
        "dfw" => "США - Даллас",
        "lax" => "США - Лос-Анджелес",
        "sea" => "США - Сиэтл",
        "mia" => "США - Майами",
        "nyc" | "jfk" => "США - Нью-Йорк",
        "yyz" | "tor" => "Канада - Торонто",
        "gru" | "sao" => "Бразилия - Сан-Паулу",
        "eze" | "bue" => "Аргентина - Буэнос-Айрес",
        "scl" => "Чили - Сантьяго",
        "lim" => "Перу - Лима",
        "bog" => "Колумбия - Богота",
        "jnb" => "ЮАР - Йоханнесбург",
        "dxb" => "ОАЭ - Дубай",
        "bom" => "Индия - Мумбаи",
        "maa" => "Индия - Ченнаи",
        "sgp" => "Сингапур",
        "hkg" => "Гонконг",
        "tyo" | "nrt" | "hnd" => "Япония - Токио",
        "sel" | "icn" => "Корея - Сеул",
        "syd" => "Австралия - Сидней",
        "mel" => "Австралия - Мельбурн",
        "akl" => "Новая Зеландия - Окленд",
        _ => "",
    };
    if name.is_empty() { c.to_uppercase() } else { name.to_string() }
}

/// Регионы загрузки с серверами Steam: (номер региона, название). Опрашивает каталог серверов
/// Steam для номеров 0..=255 (как список регионов в настройках Steam), 24 запроса параллельно.
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
        return Err("Steam не ответил".into());
    }
    let mut v: Vec<(u32, String)> = found.into_iter().collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(v)
}
