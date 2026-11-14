//! Разведка запросов Steam (только для разработки, только чтение): VAPORUST_DUMP=<папка>
//! cargo test probe_live -- --ignored --nocapture

#[cfg(test)]
mod live {
    fn around(t: &str, pat: &str, n: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(i) = t[from..].find(pat) {
            let i = from + i;
            let mut a = i.saturating_sub(n);
            while !t.is_char_boundary(a) {
                a -= 1;
            }
            let mut b = (i + pat.len() + n).min(t.len());
            while !t.is_char_boundary(b) {
                b += 1;
            }
            out.push(t[a..b].replace('\n', " "));
            from = i + pat.len();
            if out.len() >= 6 {
                break;
            }
        }
        out
    }

    #[test]
    #[ignore]
    fn probe_live() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            let web = crate::webapi::web_session(&c).await.unwrap();
            let http = aurelia::core::net::steam_web_client().unwrap();
            // Страницы (с входом): сохраняем целиком для разбора.
            for (name, url) in [
                ("edit_info", format!("https://steamcommunity.com/profiles/{me}/edit/info")),
                ("edit_settings", format!("https://steamcommunity.com/profiles/{me}/edit/settings")),
                ("discussions", "https://steamcommunity.com/app/70/discussions/0/".to_string()),
                ("wishlist", "https://store.steampowered.com/wishlist/".to_string()),
                ("app", "https://store.steampowered.com/app/70/?l=russian".to_string()),
                ("cart", "https://store.steampowered.com/cart/".to_string()),
            ] {
                match web.get_text(&url).await {
                    Ok(t) => {
                        std::fs::write(dir.join(format!("{name}.html")), &t).unwrap();
                        println!("== {name}: {} байт", t.len());
                        // Скрипты магазина: ищем в них методы Web API.
                        if name == "wishlist" || name == "cart" || name == "app" {
                            for src in t.split("<script").filter_map(|s| s.split("src=\"").nth(1)).filter_map(|s| s.split('"').next()) {
                                if !src.contains(".js") || !src.contains("steamstatic") {
                                    continue;
                                }
                                let Ok(r) = http.get(src.replace("&amp;", "&")).send().await else { continue };
                                let Ok(js) = r.text().await else { continue };
                                for pat in ["IWishlistService/", "IAccountCartService/", "ICuratorService/", "IStoreService/GetCurator", "curator"] {
                                    let hits: std::collections::BTreeSet<String> = js
                                        .match_indices(pat)
                                        .filter_map(|(i, _)| js.get(i..(i + 60).min(js.len())))
                                        .map(|s| s.split(|c: char| c == '"' || c == '\'' || c == '`' || c == ',').next().unwrap_or("").to_string())
                                        .collect();
                                    if !hits.is_empty() && pat != "curator" {
                                        println!("   {src}: {:?}", hits.iter().take(30).collect::<Vec<_>>());
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => println!("== {name}: ошибка {e:#}"),
                }
            }
            let t = std::fs::read_to_string(dir.join("discussions.html")).unwrap_or_default();
            for s in around(&t, "createtopic", 250) {
                println!("createtopic: {s}");
            }
            let t = std::fs::read_to_string(dir.join("edit_info.html")).unwrap_or_default();
            for s in around(&t, "data-profile-edit", 600).into_iter().take(1) {
                println!("profile-edit: {s}");
            }
            let t = std::fs::read_to_string(dir.join("app.html")).unwrap_or_default();
            for s in around(&t, "curator", 150).into_iter().take(4) {
                println!("app curator: {s}");
            }
        });
    }
}
