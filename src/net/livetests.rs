//! Tests of the network part: live ones go to Steam with the saved login (#[ignore]), the rest work offline.

#[cfg(test)]
mod live {
    /// Manual check on a real account (read only):
    /// cargo test live_emoticons -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_emoticons() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (e, s) = c.fetch_emoticons_and_stickers().await.unwrap();
            println!("emoticons {}: {:?}", e.len(), &e[..e.len().min(15)]);
            println!("stickers {}: {:?}", s.len(), &s[..s.len().min(15)]);
        });
    }
}

#[cfg(test)]
mod live_props {
    /// cargo test live_props_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_props_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for app in [70u32, 2280] {
                println!("== {app}");
                println!("langs {:?}", c.fetch_game_languages(app).await.map_err(|e| e.to_string()));
                println!("branches {:?}", c.fetch_branches(app).await.map_err(|e| e.to_string()));
                let ext = c.get_extended_app_info(app).await.unwrap();
                println!("dlcs {:?} branch {}", ext.dlcs, ext.active_branch);
                let st = c.dlc_states(app, &ext.dlcs).await.map_err(|e| e.to_string());
                println!("dlc states {:?}", st.map(|v| v.iter().map(|s| (s.app_id, s.owned, s.installed, s.disabled)).collect::<Vec<_>>()));
                let cloud = c.cloud_client().unwrap().get_file_list(app).await.map(|f| f.len()).map_err(|e| e.to_string());
                println!("cloud files {:?}", cloud);
            }
        });
    }
}

#[cfg(test)]
mod live_updates {
    /// cargo test live_updates_check -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_updates_check() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let list: Vec<_> = games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let t = std::time::Instant::now();
            let ids = c.games_needing_update(&list).await.unwrap();
            let names: Vec<_> = ids.iter().filter_map(|i| games.iter().find(|g| g.appid == *i)).map(|g| g.name.clone()).collect();
            println!("checked {} in {:.1}s, need update: {:?}", list.len(), t.elapsed().as_secs_f32(), names);
        });
    }
}

#[cfg(test)]
mod live_store {
    /// cargo test live_store_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_store_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let cc = c.store_country();
            println!("country {cc} me {:?}", c.my_steam_id());
            let s = c.search_store("doom", 5, "russian", &cc).await.unwrap();
            println!("search {:?} total {}", s.app_ids, s.total);
            let apps = c.fetch_store_apps(&[70, 2280], "russian", &cc).await.unwrap();
            for a in &apps {
                println!("== {} {} price {:?} orig {:?} disc {} review {:?} rel {:?}", a.app_id, a.name, a.price, a.original_price, a.discount_pct, a.review_summary, a.release_date);
                println!("dev {:?} pub {:?} tags {:?}", a.developers, a.publishers, a.tags.iter().take(5).map(|t| t.name.clone()).collect::<Vec<_>>());
                println!("assets {:?}", a.assets);
                println!("shots {} first {:?}", a.screenshots.len(), a.screenshots.first());
                println!("short {:?}", a.short_description.chars().take(150).collect::<String>());
                println!("full {:?}", a.full_description.chars().take(300).collect::<String>());
            }
            let w = c.wishlist(c.my_steam_id().unwrap()).await.map(|w| w.len());
            println!("wishlist {:?}", w.map_err(|e| e.to_string()));
            let d = c.query_deals(aurelia::steam_client::DealsScope::DiscountedTopSellers, 1, 0, 10, "russian", &cc).await.map(|p| p.app_ids.len());
            println!("deals {:?}", d.map_err(|e| e.to_string()));
            let sim = c.similar_apps(70, 5, "russian", &cc).await.map(|p| p.app_ids);
            println!("similar {:?}", sim.map_err(|e| e.to_string()));
            println!("players {:?}", c.current_players(70).await.map_err(|e| e.to_string()));
            let http = aurelia::core::net::steam_web_client().unwrap();
            let news = aurelia::web::discovery::fetch_news(&http, 70, 3, 300).await.map(|n| n.iter().map(|x| x.title.clone()).collect::<Vec<_>>());
            println!("news {:?}", news.map_err(|e| e.to_string()));
        });
    }
}

#[cfg(test)]
mod live_collections {
    /// cargo test live_collections_pull -- --ignored --nocapture (read only from Steam)
    #[test]
    #[ignore]
    fn live_collections_pull() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let store = aurelia::library::collections::pull(&c).await.unwrap();
            for col in &store.collections {
                println!("{} | {} | added {} | dynamic {} | builtin {}", col.id, col.name, col.added.len(), col.is_dynamic(), col.is_builtin());
            }
            println!("total {} version {}", store.collections.len(), store.namespace_version);
        });
    }
}

#[cfg(test)]
mod live_social {
    /// cargo test live_social_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_social_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            match c.user_profile(me).await {
                Ok(p) => println!("profile name {:?} url {} avatar {:?} vis {} created {:?} loc {:?} headline {:?} summary {:?} state {:?} game {:?} sources {:?}",
                    p.persona_name, p.profile_url, p.avatar_url.is_some(), p.visibility, p.created, p.location, p.headline, p.summary.map(|s| s.chars().take(60).collect::<String>()), p.persona_state, p.game_name, p.sources),
                Err(e) => println!("profile err {e:#}"),
            }
            let page = c.query_workshop_files(550, None, 0, "*", 5, &[]).await;
            match page {
                Ok(p) => println!("workshop total {} items {:?}", p.total, p.items.iter().map(|i| (i.id, i.title.clone(), i.file_size)).collect::<Vec<_>>()),
                Err(e) => println!("workshop err {e:#}"),
            }
            println!("subscribed {:?}", c.fetch_subscribed_items(550).await.map(|v| v.len()).map_err(|e| e.to_string()));
        });
    }
}

#[cfg(test)]
mod limit_live {
    /// Live speed limit test: downloads a game into a temporary folder (VAPORUST_DUMP)
    /// with the VAPORUST_LIMIT kbit/s limit for 20 seconds and prints what the progress counted.
    #[test]
    #[ignore]
    fn download_limit_live() {
        use std::sync::{atomic::Ordering, Arc, RwLock};
        let dir = std::env::var("VAPORUST_DUMP").unwrap();
        let limit: u64 = std::env::var("VAPORUST_LIMIT").unwrap().parse().unwrap();
        let appid: u32 = std::env::var("VAPORUST_APP").unwrap_or("3620".into()).parse().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.download_limit_kbit = limit;
            cfg.save().await.unwrap();
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let state = Arc::new(RwLock::new(aurelia::core::models::DownloadState::default()));
            let abort = state.read().unwrap().abort_signal.clone();
            let opts = aurelia::steam_client::InstallOptions {
                library_override: Some(dir),
                ..Default::default()
            };
            let mut rx = c
                .install_game(appid, aurelia::core::models::DepotPlatform::Windows, opts, state)
                .await
                .unwrap();
            let t0 = std::time::Instant::now();
            let mut last = 0u64;
            let mut mark: Option<(f64, u64)> = None;
            let switch: Option<u64> = std::env::var("VAPORUST_LIMIT2").ok().and_then(|v| v.parse().ok());
            while t0.elapsed().as_secs() < 25 {
                if let (Some(l2), None) = (switch, mark) {
                    if t0.elapsed().as_secs() >= 8 {
                        let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
                        cfg.download_limit_kbit = l2;
                        cfg.save().await.unwrap();
                        aurelia::steam_client::set_download_limit_now(l2);
                        println!("limit changed: {last} bytes in 8 s");
                        mark = Some((t0.elapsed().as_secs_f64(), last));
                    }
                }
                match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
                    Ok(Some(p)) => last = p.bytes_downloaded.saturating_sub(p.reused_bytes),
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
            println!("progress bytes: {last} in {:.1} s", t0.elapsed().as_secs_f64());
            if let Some((t, b)) = mark {
                println!("after the change: {:.2} MB/s", (last - b) as f64 / 1e6 / (t0.elapsed().as_secs_f64() - t));
            }
            abort.store(true, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });
    }
}

#[cfg(test)]
mod family_live {
    /// Live test: games of the Steam family group (without own ones).
    #[test]
    #[ignore]
    fn family_apps_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            match c.fetch_family_shared_apps().await {
                Ok(v) => {
                    println!("family games: {}", v.len());
                    for a in v.iter().take(40) {
                        println!("{} {} owner={}", a.app_id, a.name, a.owner_steamid.is_some());
                    }
                }
                Err(e) => println!("error: {e:#}"),
            }
        });
    }
}

#[cfg(test)]
mod effects_live {
    /// Live test: which item types are in the Steam inventory (753/6) - looking for chat effects.
    #[test]
    #[ignore]
    fn inventory_types_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = c.web_session().await.unwrap();
            let items = aurelia::steam_client::inventory_via_lang(&web, 753, 6, "english").await.unwrap();
            let mut seen = std::collections::BTreeMap::<String, Vec<String>>::new();
            for i in items {
                let e = seen.entry(i.item_type.clone()).or_default();
                if e.len() < 4 {
                    e.push(format!("{} | {}", i.name, i.icon_url.unwrap_or_default()));
                }
            }
            for (t, v) in seen {
                if t.contains("Card") || t.contains("Background") {
                    continue;
                }
                println!("== {t}");
                for x in v {
                    println!("   {x}");
                }
            }
        });
    }
}

#[cfg(test)]
mod logon_burst_live {
    /// Live test: which messages the server sends right after login (buffer of unhandled ones).
    #[test]
    #[ignore]
    fn logon_burst_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let _ = c.connection().unwrap().take_unprocessed();
            c.announce_persona(7).await.unwrap(); // invisible, like the user's
            for wait in [0u64, 3, 6] {
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                let kinds: Vec<String> =
                    c.connection().unwrap().take_unprocessed().iter().map(|r| format!("{:?}", r.kind)).collect();
                println!("+{wait}s: {kinds:?}");
            }
        });
    }
}

#[cfg(test)]
mod shots_live {
    /// Live test: own screenshots in the Steam cloud.
    #[test]
    #[ignore]
    fn shots_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            use steam_vent_proto::steammessages_publishedfile_steamclient::*;
            use steam_vent::ConnectionTrait;
            let conn = c.connection().unwrap();
            for (ty, ft) in [("myfiles", Some(4u32))] {
                let mut req = CPublishedFile_GetUserFiles_Request::new();
                req.set_steamid(u64::from(conn.steam_id()));
                req.set_appid(0);
                req.set_numperpage(6);
                req.set_type(ty.to_string());
                if let Some(ft) = ft {
                    req.set_filetype(ft);
                }
                let r: Result<CPublishedFile_GetUserFiles_Response, _> = conn.service_method(req).await;
                match r {
                    Ok(r) => {
                        println!("== {ty} {ft:?}: total {}", r.total());
                        for d in &r.publishedfiledetails {
                            println!("  {} type={} {} [{}] file={} url={} img={} prev={} fn={} {}x{}", d.publishedfileid(), d.file_type(), d.app_name(), d.title(), d.file_url(), d.url(), d.image_url(), d.preview_url(), d.filename(), d.image_width(), d.image_height());
                        }
                    }
                    Err(e) => println!("== {ty}: err {e}"),
                }
            }
        });
    }
}

#[cfg(test)]
mod update_direct_live {
    /// Live test of updating without Steam in a SEPARATE library (VAPORUST_DUMP), your games are not touched:
    /// own Aurelia settings folder,
    /// "old" version = a fake number in appmanifest + language and SharedDepots, then update and check.
    /// WARNING: Aurelia also looks for appmanifest in C:\Program Files (x86)\Steam (alphabetical sort) -
    /// so only a game that is not in the real libraries (Ricochet by default, 5 MB).
    #[test]
    #[ignore]
    fn update_direct_live() {
        use aurelia::core::models::{DepotPlatform, DownloadProgressState, DownloadState};
        let root = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let cfg_dir = root.join("cfg");
        let lib = root.join("lib");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(lib.join("steamapps")).unwrap();
        let real = dirs_config();
        for f in ["session.json"] {
            std::fs::copy(real.join(f), cfg_dir.join(f)).unwrap();
        }
        std::env::set_var("AURELIA_CONFIG_DIR", &cfg_dir);
        // Only a game that is NOT in the Steam libraries: Aurelia always also looks in C:\Program Files (x86)\Steam.
        let appid: u32 = std::env::var("VAPORUST_APP").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
        let steam = crate::local::find_steam().unwrap();
        assert!(
            !crate::local::scan_games(&steam.dir).iter().any(|g| g.appid == appid),
            "game {appid} is installed in Steam - the test does not touch it"
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.steam_library_path = lib.to_string_lossy().into_owned();
            cfg.windows_steam_discovery_enabled = false;
            cfg.download_limit_kbit = 0;
            cfg.save().await.unwrap();
            aurelia::steam_client::set_download_limit_now(0);
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let acf = lib.join("steamapps").join(format!("appmanifest_{appid}.acf"));
            async fn drain(mut rx: tokio::sync::mpsc::Receiver<aurelia::core::models::DownloadProgress>) -> bool {
                while let Some(p) = rx.recv().await {
                    match p.state {
                        DownloadProgressState::Completed => return true,
                        DownloadProgressState::Failed => {
                            println!("error: {}", p.current_file);
                            return false;
                        }
                        _ => {}
                    }
                }
                false
            }
            if !acf.exists() {
                let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
                let opts = aurelia::steam_client::InstallOptions {
                    library_override: Some(lib.to_string_lossy().into_owned()),
                    ..Default::default()
                };
                let rx = c.install_game(appid, DepotPlatform::Windows, opts, st).await.unwrap();
                assert!(drain(rx).await, "the install failed");
            }
            // "Old" version + what Aurelia loses when rewriting.
            let real_versions = crate::local::installed_depots(&acf);
            let fake: std::collections::BTreeMap<u32, u64> = real_versions.keys().map(|d| (*d, 1111)).collect();
            crate::local::write_depot_versions(&acf, &fake, Some("1"), 6).unwrap();
            let mut text = std::fs::read_to_string(&acf).unwrap();
            let end = text.rfind('}').unwrap();
            text.insert_str(end, "\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"russian\"\n\t}\n\t\"SharedDepots\"\n\t{\n\t\t\"228988\"\t\t\"228980\"\n\t}\n");
            std::fs::write(&acf, &text).unwrap();
            let original = std::fs::read_to_string(&acf).unwrap();
            // Update - like Cmd::UpdateDirect.
            let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
            let t = std::time::Instant::now();
            let rx = c.update_game(appid, st).await.unwrap();
            let ok = drain(rx).await;
            crate::net::merge_after_update(&acf, &original, ok).unwrap();
            let after = std::fs::read_to_string(&acf).unwrap();
            println!("update: {ok}, {:.1} s\n{after}", t.elapsed().as_secs_f32());
            assert!(ok);
            assert_eq!(crate::local::installed_depots(&acf), real_versions, "versions not updated");
            assert!(after.contains("\"language\"\t\t\"russian\""), "language lost");
            assert!(after.contains("SharedDepots"), "SharedDepots lost");
            assert!(after.contains("\"StateFlags\"\t\t\"4\""));
        });
    }

    fn dirs_config() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia")
    }
}

#[cfg(test)]
mod small_games_live {
    /// Small owned/family games that are not on disk (for safe download tests).
    #[test]
    #[ignore]
    fn small_games_live() {
        let steam = crate::local::find_steam().unwrap();
        let installed: std::collections::HashSet<u32> = crate::local::scan_games(&steam.dir).iter().map(|g| g.appid).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let mut ids: Vec<(u32, String)> = c.fetch_owned_games().await.unwrap().into_iter().map(|g| (g.app_id, g.name)).collect();
            ids.extend(c.fetch_family_shared_apps().await.unwrap_or_default().into_iter().map(|a| (a.app_id, a.name)));
            let mut out = Vec::new();
            for (id, name) in ids.into_iter().filter(|(id, _)| !installed.contains(id)).take(120) {
                if let Ok(e) = c.estimate_install_size(id, aurelia::core::models::DepotPlatform::Windows).await {
                    if e.download_size > 0 && e.download_size < 60_000_000 {
                        out.push((e.download_size, id, name));
                    }
                }
            }
            out.sort();
            for (s, id, n) in out.iter().take(10) {
                println!("{id} {n}: {} MB", s / 1_000_000);
            }
        });
    }
}

#[cfg(test)]
mod lang_depots_live {
    /// Language depots of games (VAPORUST_APPS, comma-separated).
    #[test]
    #[ignore]
    fn lang_depots_live() {
        let ids: Vec<u32> = std::env::var("VAPORUST_APPS").unwrap().split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for id in ids {
                let mut v = c.language_depots(id, "public").await.unwrap_or_default();
                v.sort_by_key(|x| x.0);
                println!("== {id}");
                for (dep, lang, os, from, dlc, man) in v {
                    println!("  {dep} lang={lang} os={os:?} from={from:?} dlc={dlc:?} manifest={man:?}");
                }
            }
        });
    }
}

#[cfg(test)]
mod language_live {
    /// Language change without Steam in a SEPARATE library (VAPORUST_DUMP) on Ricochet (60), which is not in Steam:
    /// english -> french (own depot 62 is downloaded, shared 72 from Half-Life - as a link to a fake
    /// appmanifest_70 in the temp folder) -> english (62 is removed, files verified).
    #[test]
    #[ignore]
    fn language_live() {
        use aurelia::core::models::{DepotPlatform, DownloadProgressState, DownloadState};
        let appid = 60u32;
        let steam = crate::local::find_steam().unwrap();
        assert!(!crate::local::scan_games(&steam.dir).iter().any(|g| g.appid == appid), "Ricochet is installed in Steam");
        let root = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let (cfg_dir, lib) = (root.join("cfg"), root.join("lib"));
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(lib.join("steamapps")).unwrap();
        let real = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia");
        std::fs::copy(real.join("session.json"), cfg_dir.join("session.json")).unwrap();
        std::env::set_var("AURELIA_CONFIG_DIR", &cfg_dir);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.steam_library_path = lib.to_string_lossy().into_owned();
            cfg.windows_steam_discovery_enabled = false;
            cfg.save().await.unwrap();
            aurelia::steam_client::set_download_limit_now(0);
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let acf = lib.join("steamapps").join(format!("appmanifest_{appid}.acf"));
            if !acf.exists() {
                let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
                let opts = aurelia::steam_client::InstallOptions {
                    library_override: Some(lib.to_string_lossy().into_owned()),
                    ..Default::default()
                };
                let mut rx = c.install_game(appid, DepotPlatform::Windows, opts, st).await.unwrap();
                let mut ok = false;
                while let Some(p) = rx.recv().await {
                    if p.state == DownloadProgressState::Completed {
                        ok = true;
                        break;
                    }
                    if p.state == DownloadProgressState::Failed {
                        break;
                    }
                }
                assert!(ok, "the install failed");
                crate::local::set_user_config(&acf, "language", "english").unwrap();
                crate::local::set_mounted_config(&acf, "language", "english").unwrap();
            }
            // A fake "Half-Life" next to it: it has the French depot 72.
            std::fs::write(
                lib.join("steamapps").join("appmanifest_70.acf"),
                "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"72\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"491735997354925434\"\n\t\t\t\"size\"\t\t\"1\"\n\t\t}\n\t}\n}\n",
            )
            .unwrap();
            let shared: crate::net::Shared = Default::default();
            let ctx = eframe::egui::Context::default();
            let abort = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let dir = lib.join("steamapps").join("common").join("Half-Life");
            let count = |d: &std::path::Path| walk(d);
            let before = count(&dir);
            crate::net::switch_language(&c, appid, &acf, "french", &abort, &shared, &ctx).await.unwrap();
            let fr = std::fs::read_to_string(&acf).unwrap();
            println!("--- french, files {} -> {}\n{fr}", before, count(&dir));
            assert!(crate::local::installed_depots(&acf).contains_key(&62), "62 not installed");
            assert_eq!(crate::local::shared_depots(&acf).get(&72), Some(&70));
            let i = crate::local::read_acf(&acf);
            assert_eq!((i.user_language.as_str(), i.mounted_language.as_str(), i.state_flags), ("french", "french", 4));
            crate::net::switch_language(&c, appid, &acf, "english", &abort, &shared, &ctx).await.unwrap();
            let en = std::fs::read_to_string(&acf).unwrap();
            let after = count(&dir);
            println!("--- english, files {after}\n{en}");
            assert_eq!(after, before, "French files not removed");
            assert!(!crate::local::installed_depots(&acf).contains_key(&62));
            assert!(!crate::local::shared_depots(&acf).contains_key(&72));
            let i = crate::local::read_acf(&acf);
            assert_eq!((i.user_language.as_str(), i.mounted_language.as_str(), i.state_flags), ("english", "english", 4));
        });
    }

    fn walk(d: &std::path::Path) -> usize {
        std::fs::read_dir(d)
            .map(|r| r.flatten().map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 }).sum())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod installed_langs_live {
    /// Installed games with their own language depots (for testing language change without Steam).
    #[test]
    #[ignore]
    fn installed_langs_live() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for g in games.iter().filter(|g| g.ready) {
                let Ok(d) = c.language_depots(g.appid, "public").await else { continue };
                let own: Vec<String> = d
                    .iter()
                    .filter(|x| x.3.is_none() && x.4.is_none())
                    .map(|x| x.1.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let shared = d.iter().filter(|x| x.3.is_some()).count();
                if !own.is_empty() || shared > 0 {
                    let acf = crate::local::read_acf(&crate::local::appmanifest_path(g));
                    println!(
                        "{} ({}, {} MB): own languages {:?}, shared {}; current language \"{}\"",
                        g.name, g.appid, g.size / 1_000_000, own, shared, acf.mounted_language
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod offline_live {
    /// "No network" without disconnecting the internet: a copy of Aurelia settings in a temp folder and all requests
    /// through a nonexistent proxy. Expected: login failed exactly as "offline", the library cache exists.
    #[test]
    #[ignore]
    fn offline_live() {
        let dir = std::env::temp_dir().join(format!("vaporust_offline_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia");
        for f in ["session.json", "library_cache.json"] {
            let _ = std::fs::copy(real.join(f), dir.join(f));
        }
        std::env::set_var("AURELIA_CONFIG_DIR", &dir);
        for v in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            std::env::set_var(v, "http://127.0.0.1:9");
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            let t = std::time::Instant::now();
            let r = c.restore_session().await;
            let cached = aurelia::core::config::load_library_cache().await.unwrap_or_default();
            if let Err(e) = &r {
                println!("Vaporust treats this as a network problem: {}", crate::net::network_failure(e).await);
            }
            println!(
                "login: {}; error kind: {:?}; offline: {}; cached games: {}; {:.1} s",
                r.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|e| format!("error ({e:#})")),
                r.as_ref().err().map(|e| aurelia::core::error::classify(e).kind),
                c.is_offline(),
                cached.len(),
                t.elapsed().as_secs_f32()
            );
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod ach_live {
    /// Live test (read only): own L4D2 achievements and those of a few friends. Prints numbers only.
    #[test]
    #[ignore]
    fn friend_achievements_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let mine = c.fetch_achievements(550, "russian").await.unwrap();
            let icons = mine.iter().filter(|a| !a.icon_unlocked.is_empty()).count();
            println!("mine: {} of {}, icons {icons}", mine.iter().filter(|a| a.unlocked).count(), mine.len());
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            for f in friends.iter().filter(|f| f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1).take(6) {
                match c.fetch_achievements_for(550, "russian", f.steam_id).await {
                    Ok(v) => println!("friend: {} of {}", v.iter().filter(|a| a.unlocked).count(), v.len()),
                    Err(e) => println!("friend: error {e:#}"),
                }
            }
        });
    }
}

#[cfg(test)]
mod shared_uninstall_live {
    /// What a new uninstall would delete for a game in a shared folder (Counter-Strike, 10 by default).
    /// DELETES NOTHING. VAPORUST_APP - another game.
    #[test]
    #[ignore]
    fn shared_plan_live() {
        let appid: u32 = std::env::var("VAPORUST_APP").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let me = games.iter().find(|g| g.appid == appid).expect("the game is not installed");
        let dir = me.dir.to_string_lossy().to_lowercase();
        let others: Vec<u32> =
            games.iter().filter(|g| g.appid != appid && g.dir.to_string_lossy().to_lowercase() == dir).map(|g| g.appid).collect();
        println!("{} ({appid}), neighbours: {others:?}", me.name);
        let steamapps = me.dir.parent().unwrap().parent().unwrap().to_path_buf();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let list = crate::net::shared_remove_list(&c, appid, &steamapps, &others).await.unwrap();
            let mut tops = std::collections::BTreeMap::<String, usize>::new();
            for f in &list {
                *tops.entry(f.split(['/', '\\']).next().unwrap_or("").to_lowercase()).or_default() += 1;
            }
            println!("files that would be removed: {}; by top folders: {tops:?}", list.len());
        });
    }
}

#[cfg(test)]
mod session_tests {
    use crate::net::*;

    /// Both tests change the shared login number - one at a time.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A response of a previous-login task arriving after a logout or a new login is not written;
    /// tasks of the current login and normal code write as before.
    #[test]
    fn stale_task_writes_are_dropped() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let st: Shared = Arc::new(SharedNet::default());
        let ctx = egui::Context::default();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let old = spawn_session(async move {
                let _ = rx.await; // a "long request" of the old account
                upd(&st2, &ctx2, |s| s.message = "old account".into());
            });
            // While the request was running, another account logged in.
            SESSION_GEN.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(());
            old.await.unwrap();
            assert!(st.lock().unwrap().message.is_empty(), "an answer from the previous login is dropped");
            let (st3, ctx3) = (st.clone(), ctx.clone());
            spawn_session(async move { upd(&st3, &ctx3, |s| s.message = "new".into()) }).await.unwrap();
            assert_eq!(st.lock().unwrap().message, "new");
        });
        upd(&st, &ctx, |s| s.message = "outside tasks".into());
        assert_eq!(st.lock().unwrap().message, "outside tasks");
    }

    /// QA-03: a subtask started from a login task also counts as that login's task - even if
    /// it started after the account switch.
    #[test]
    fn child_tasks_inherit_session() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let st: Shared = Arc::new(SharedNet::default());
        let ctx = egui::Context::default();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let parent = spawn_session(async move {
                let _ = rx.await;
                // The subtask is created after the login switch.
                spawn_session(async move { upd(&st2, &ctx2, |s| s.message = "subtask answer".into()) }).await.unwrap();
            });
            SESSION_GEN.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(());
            parent.await.unwrap();
            assert!(st.lock().unwrap().message.is_empty(), "a subtask of the previous login wrote nothing");
        });
    }
}

#[cfg(test)]
mod merge_tests {
    use crate::net::{merge_history, ChatLine};

    fn l(from_self: bool, text: &str, time: u64) -> ChatLine {
        ChatLine { from_self, text: text.into(), time, ..Default::default() }
    }

    /// A late history response does not remove a message sent while it was loading.
    #[test]
    fn late_history_keeps_new_lines() {
        let history = vec![l(false, "hi", 1000), l(true, "how are you", 1010)];
        let current = vec![l(true, "how are you", 1012), l(true, "new", 1100)];
        let m = merge_history(history, current);
        let texts: Vec<&str> = m.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["hi", "how are you", "new"], "no duplicates, the new line is kept");
        // Old lines that are not in the history (long ago) are not added.
        let m = merge_history(vec![l(false, "a", 5000)], vec![l(false, "old", 100)]);
        assert_eq!(m.len(), 1);
    }

    /// Two identical messages in a row are two messages: a history line covers only one.
    #[test]
    fn same_text_twice_is_not_dup() {
        let current = vec![l(true, "yes", 1000), l(true, "yes", 1060)];
        assert_eq!(merge_history(vec![l(false, "hi", 990)], current.clone()).len(), 3);
        assert_eq!(merge_history(vec![l(true, "yes", 1000)], current).len(), 2);
    }

    /// A new "yes" sent while the history (with an older "yes" a minute ago) was loading stays on screen.
    #[test]
    fn new_message_not_swallowed_by_older_same_text() {
        let merged = merge_history(vec![l(true, "yes", 1000)], vec![l(true, "yes", 1060)]);
        assert_eq!(merged.iter().map(|m| m.time).collect::<Vec<_>>(), vec![1000, 1060]);
    }

    fn x(from_self: bool, text: &str, time: u64, ordinal: u32) -> ChatLine {
        ChatLine { from_self, text: text.into(), time, ordinal, exact: true, ..Default::default() }
    }

    /// Lines with a server identity: two separate "yes" one second (or zero seconds) apart are
    /// two messages, even when the history arrives late and holds only the first one.
    #[test]
    fn quick_repeats_with_server_ids() {
        for from_self in [true, false] {
            // History has "yes" at 1000; on screen only the new "yes" at 1001 (arrived after the request).
            let m = merge_history(vec![x(from_self, "yes", 1000, 0)], vec![x(from_self, "yes", 1001, 0)]);
            assert_eq!(m.len(), 2, "one second later");
            // Same second, next ordinal.
            let m = merge_history(vec![x(from_self, "yes", 1000, 0)], vec![x(from_self, "yes", 1000, 1)]);
            assert_eq!(m.len(), 2, "in the same second");
            assert_eq!(m.iter().map(|l| l.ordinal).collect::<Vec<_>>(), vec![0, 1]);
            // The same message seen twice is one line.
            let m = merge_history(vec![x(from_self, "yes", 1000, 1)], vec![x(from_self, "yes", 1000, 1)]);
            assert_eq!(m.len(), 1, "the same message");
            // Server rewrote the text (sticker command): still one message by its identity.
            let m = merge_history(vec![x(from_self, "[sticker]", 1000, 0)], vec![x(from_self, "/sticker A", 1000, 0)]);
            assert_eq!(m.len(), 1, "text changed by the server");
        }
    }
}
