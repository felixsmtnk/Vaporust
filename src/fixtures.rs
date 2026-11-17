//! Parsing tests on saved Steam responses (folder testdata/steam).
//!
//! Pages were captured WITHOUT login (public pages), so they contain no session keys. If Valve
//! changes the markup, capture them again (`capture_live`) - the failing test shows which parser
//! broke. Responses available only with login (wallet, licenses, own listings and so on)
//! are checked on short samples right in the tests of their modules.

#[cfg(test)]
pub(crate) mod capture {
    use std::path::PathBuf;

    pub fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata").join("steam")
    }

    /// Strip the page of extras: scripts (except those the parsers need), styles, stylesheet links,
    /// meta tags and comments. This makes the files smaller and keeps random keys out.
    pub fn trim(html: &str) -> String {
        const KEEP: [&str; 4] = ["g_rgAppContextData", "InitializeCommentThread", "InitializeForum", "g_rgWalletInfo"];
        let mut out = String::with_capacity(html.len() / 2);
        let mut rest = html;
        loop {
            let next = ["<script", "<style", "<!--", "<link", "<meta"]
                .iter()
                .filter_map(|t| rest.find(t).map(|i| (i, *t)))
                .min_by_key(|(i, _)| *i);
            let Some((i, tag)) = next else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..i]);
            let after = &rest[i..];
            let end = match tag {
                "<script" => after.find("</script>").map(|e| e + 9),
                "<style" => after.find("</style>").map(|e| e + 8),
                "<!--" => after.find("-->").map(|e| e + 3),
                _ => after.find('>').map(|e| e + 1),
            }
            .unwrap_or(after.len());
            let block = &after[..end];
            if tag == "<script" && KEEP.iter().any(|k| block.contains(k)) {
                out.push_str(block);
            }
            rest = &after[end..];
        }
        // Consecutive empty lines - as one.
        let mut lines: Vec<&str> = Vec::new();
        for l in out.lines() {
            let l = l.trim_end();
            if l.trim().is_empty() && lines.last().is_some_and(|p| p.trim().is_empty()) {
                continue;
            }
            lines.push(l);
        }
        let mut s = lines.join("\n");
        // Replace the guest session key just in case.
        for key in ["g_sessionID = \"", "sessionid=\"", "\"sessionid\":\"", "name=\"sessionid\" value=\""] {
            let mut from = 0;
            while let Some(p) = s[from..].find(key) {
                let start = from + p + key.len();
                let end = s[start..].find('"').map(|e| start + e).unwrap_or(start);
                s.replace_range(start..end, "0");
                from = start + 1;
            }
        }
        s
    }

    fn first_between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
        let i = s.find(a)? + a.len();
        let j = s[i..].find(b)? + i;
        Some(&s[i..j])
    }

    /// Capture the pages again: `cargo test capture_live -- --ignored`. Read only, no login.
    #[test]
    #[ignore]
    fn capture_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let http = aurelia::core::net::steam_web_client().unwrap();
            let get = |url: String| {
                let http = http.clone();
                async move {
                    let r = http.get(&url).header("Accept-Language", "ru").send().await.unwrap();
                    assert!(r.status().is_success(), "{url}: {}", r.status());
                    r.text().await.unwrap()
                }
            };
            let save = |name: &str, body: &str| {
                std::fs::create_dir_all(dir()).unwrap();
                let body = if name.ends_with(".html") { trim(body) } else { body.to_string() };
                std::fs::write(dir().join(name), &body).unwrap();
                println!("{name}: {} KB", body.len() / 1024);
            };
            // Public profile with showcases, badges and groups.
            const GABE: u64 = 76561197960435530; // Robin Walker (Valve), a public profile
            let profile = get(format!("https://steamcommunity.com/profiles/{GABE}/?l=russian")).await;
            save("profile.html", &profile);
            save("profile.xml", &get(format!("https://steamcommunity.com/profiles/{GABE}/?xml=1")).await);
            save(
                "profile_comments.json",
                &get(format!("https://steamcommunity.com/comment/Profile/render/{GABE}/-1/?start=0&count=10")).await,
            );
            save("badges.html", &get(format!("https://steamcommunity.com/profiles/{GABE}/badges/?l=english")).await);
            save("group.xml", &get("https://steamcommunity.com/groups/steamuniverse/memberslistxml/?xml=1&l=russian".into()).await);
            // Half-Life discussions, topic and guide.
            let topics = get("https://steamcommunity.com/app/70/discussions/?l=english&fp=1".into()).await;
            save("topics.html", &topics);
            let topic_url = crate::community::parse_topics(&trim(&topics))
                .into_iter()
                .find(|t| !t.sticky && t.replies > 3)
                .map(|t| t.url)
                .expect("no topics");
            save("topic.html", &get(format!("{topic_url}?l=english")).await);
            let guides = get("https://steamcommunity.com/app/70/guides/?browsefilter=toprated&l=russian".into()).await;
            let guide = first_between(&guides, "sharedfiles/filedetails/?id=", "\"").unwrap().split('&').next().unwrap().to_string();
            save("guide.html", &get(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={guide}&l=russian")).await);
            // L4D2 Workshop mod (screenshots).
            let ws = get("https://steamcommunity.com/workshop/browse/?appid=550&browsesort=trend&section=readytouseitems".into()).await;
            let item = first_between(&ws, "sharedfiles/filedetails/?id=", "\"").unwrap().split('&').next().unwrap().to_string();
            save("workshop_item.html", &get(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={item}")).await);
            // Reviews (community page) and a single review page.
            let reviews = get(
                "https://steamcommunity.com/app/70/homecontent/?userreviewsoffset=0&p=1&numperpage=10&browsefilter=trendweek\
                 &appHubSubSection=10&filterLanguage=russian&l=russian"
                    .into(),
            )
            .await;
            save("reviews.html", &reviews);
            let review_url = crate::store::parse_reviews(&reviews).into_iter().map(|r| r.url).find(|u| !u.is_empty()).expect("no reviews");
            save("review_page.html", &get(review_url).await);
            // Curators, store, market.
            save(
                "curators.json",
                &get("https://store.steampowered.com/curators/ajaxgetcurators/render/?query=&start=0&count=10&filter=top_curators_reviewing&appid=70&l=russian".into()).await,
            );
            save("appdetails.json", &get("https://store.steampowered.com/api/appdetails?appids=70&l=russian".into()).await);
            save(
                "market_search.json",
                &get("https://steamcommunity.com/market/search/render/?norender=1&l=russian&start=0&count=10&query=Mann%20Co".into()).await,
            );
            // GitHub, not Steam: without a login it allows 60 requests an hour per address, and the build
            // machine shares its address with others. A refusal there must not look like "Steam changed
            // its pages" - the saved file simply stays as it was.
            match http.get("https://api.github.com/repos/felixsmtnk/Vaporust/releases?per_page=10").header("User-Agent", "Vaporust-tests").send().await {
                Ok(r) if r.status().is_success() => save("github_releases.json", &r.text().await.unwrap_or_default()),
                Ok(r) => println!("github_releases.json: kept the old one (GitHub answered {})", r.status()),
                Err(e) => println!("github_releases.json: kept the old one ({e})"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    fn page(name: &str) -> String {
        let p = super::capture::dir().join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }
    fn json(name: &str) -> serde_json::Value {
        serde_json::from_str(&page(name)).unwrap()
    }

    #[test]
    fn profile_page() {
        let html = page("profile.html");
        let info = crate::profile_extra::parse_page(&html);
        assert!(info.friends.len() >= 5, "friends: {}", info.friends.len());
        let counts = crate::profile_extra::parse_counts(&html);
        assert!(counts.len() >= 3, "counters: {counts:?}");
        let shows = crate::profile_extra::parse_showcases(&html);
        assert!(!shows.is_empty(), "showcases");
        assert!(shows.iter().any(|s| !s.stats.is_empty()), "showcase numbers");
    }

    #[test]
    fn profile_xml_and_comments() {
        // Steam gives details (name, URL) only for the first groups, the rest - as bare ids.
        let xml = page("profile.xml");
        let groups = crate::profile_extra::parse_groups(&xml);
        assert!(groups.len() >= 3 && groups.len() == xml.matches("<groupName>").count(), "groups: {}", groups.len());
        assert!(groups.iter().all(|g| !g.name.is_empty() && g.url.starts_with("https://")));
        let j = json("profile_comments.json");
        let comments = crate::profile_extra::parse_comments(j["comments_html"].as_str().unwrap());
        assert!(comments.len() >= 5, "comments: {}", comments.len());
        assert!(comments.iter().all(|c| !c.author.is_empty() && !c.id.is_empty() && c.time > 1_000_000_000));
    }

    #[test]
    fn badges_page() {
        let games = crate::badges::parse_badges_page(&page("badges.html"));
        assert!(!games.is_empty());
        assert!(games.iter().all(|g| g.appid != 0 && !g.name.is_empty()));
    }

    #[test]
    fn group_xml() {
        let g = crate::grouppage::parse(&page("group.xml")).expect("group");
        assert!(g.id64 > 103582791429521408);
        assert!(!g.name.is_empty());
    }

    #[test]
    fn discussions() {
        let topics = crate::community::parse_topics(&page("topics.html"));
        assert!(topics.len() >= 10, "topics: {}", topics.len());
        assert!(topics.iter().all(|t| t.url.contains("/discussions/") && !t.title.is_empty()));
        let t = crate::community::parse_topic(&page("topic.html"), "https://steamcommunity.com/app/70/discussions/0/1/");
        assert!(!t.title.is_empty());
        assert!(!t.items.is_empty(), "first post");
        assert!(!t.owner.is_empty() && !t.forum.is_empty() && !t.topic.is_empty(), "thread data for replies");
        assert!(!t.replies.is_empty(), "replies");
    }

    #[test]
    fn guide() {
        let g = crate::community::parse_guide(&page("guide.html"), 1);
        assert!(!g.title.is_empty());
        assert!(!g.sections.is_empty() || !g.intro.is_empty());
    }

    #[test]
    fn workshop_item() {
        let shots = crate::workshopui::page_screenshots(&page("workshop_item.html"));
        assert!(!shots.is_empty(), "mod screenshots not found");
        assert!(shots.iter().all(|u| u.starts_with("https://")), "{shots:?}");
    }

    #[test]
    fn reviews() {
        let r = crate::store::parse_reviews(&page("reviews.html"));
        assert!(r.len() >= 5, "reviews: {}", r.len());
        assert!(r.iter().all(|x| !x.author.is_empty() && !x.text.is_empty() && x.url.contains("/recommended/")));
        // Every card that says "1 пользователь посчитал..." has one helpful vote (it used to read 0).
        let html = page("reviews.html");
        let single = html.matches("1 пользователь посчитал этот обзор полезным").count();
        assert!(r.iter().filter(|x| x.helpful_n == 1).count() >= single, "{:?}", r.iter().map(|x| x.helpful_n).collect::<Vec<_>>());
        let (text, id, _hours) = crate::store::parse_review_page(&page("review_page.html")).unwrap();
        assert!(!text.is_empty());
        assert!(id > 0);
    }

    #[test]
    fn curators() {
        let j = json("curators.json");
        let c = crate::actions::parse_curators(j["results_html"].as_str().unwrap(), 70);
        assert!(!c.is_empty());
        assert!(c.iter().all(|x| !x.name.is_empty() && x.state <= 2));
    }

    #[test]
    fn appdetails() {
        let d = crate::store::parse_appdetails(&json("appdetails.json"), 70);
        assert!(d.requirements.is_some(), "requirements");
        assert!(!d.genres.is_empty(), "genres");
    }

    #[test]
    fn market_search() {
        let (total, hits) = crate::invent::parse_search(&page("market_search.json")).unwrap();
        assert!(total > 0 && !hits.is_empty());
        assert!(hits.iter().all(|h| h.appid != 0 && !h.hash_name.is_empty() && !h.price_text.is_empty()));
    }

    #[test]
    fn github_releases() {
        let r = crate::updater::parse_latest(page("github_releases.json").as_bytes(), "0.0.1").unwrap().expect("release");
        assert!(r.asset_url.is_some());
        assert!(r.sha_url.is_some(), "CI releases have .sha256");
    }

    /// Corrupted and empty responses: parsing does not crash and does not invent data.
    #[test]
    fn garbage_in() {
        for junk in ["", "<html></html>", "{\"success\":false}", "<<<>>>\u{0}\u{feff}", "{\"clanID\":", "<div class=\"badge_row"] {
            assert!(crate::profile_extra::parse_page(junk).friends.is_empty());
            assert!(crate::profile_extra::parse_showcases(junk).is_empty());
            assert!(crate::profile_extra::parse_comments(junk).is_empty());
            assert!(crate::profile_extra::parse_groups(junk).is_empty());
            assert!(crate::badges::parse_badges_page(junk).is_empty());
            assert!(crate::community::parse_topics(junk).is_empty());
            assert!(crate::store::parse_reviews(junk).is_empty());
            assert!(crate::store::parse_review_page(junk).is_err());
            assert!(crate::actions::parse_curators(junk, 70).is_empty());
            assert!(crate::invent::parse_search(junk).map(|r| r.1.is_empty()).unwrap_or(true));
            assert!(crate::trade::parse_history(junk).map(|r| r.is_empty()).unwrap_or(true));
            assert!(crate::updater::parse_latest(junk.as_bytes(), "0.0.1").map(|r| r.is_none()).unwrap_or(true));
            let _ = crate::community::parse_topic(junk, "u");
            let _ = crate::community::parse_guide(junk, 1);
            let _ = crate::grouppage::parse(junk);
            let _ = crate::feed::parse(junk);
            let _ = crate::coplay::parse(junk);
            let _ = crate::wallet::parse(junk);
            let _ = crate::refund::parse(junk);
            let _ = crate::profileedit::parse(junk);
            let _ = crate::keys::parse(junk);
            let _ = crate::invent::parse_my_market(junk);
            let _ = crate::invent::parse_app_contexts(junk);
            let _ = crate::trade::parse_spread(junk);
            let _ = crate::trade::parse_spread_page(junk);
            let _ = crate::richtext::parse(junk);
            let _ = crate::chatfmt::parse(junk);
        }
        // Truncated real pages (connection lost mid-response) do not crash the program either.
        for name in ["profile.html", "topic.html", "reviews.html", "guide.html", "badges.html", "workshop_item.html"] {
            let full = page(name);
            for n in 1..12 {
                let mut cut = full.len() * n / 12;
                while !full.is_char_boundary(cut) {
                    cut -= 1;
                }
                let part = &full[..cut];
                let _ = crate::profile_extra::parse_page(part);
                let _ = crate::profile_extra::parse_showcases(part);
                let _ = crate::profile_extra::parse_counts(part);
                let _ = crate::community::parse_topics(part);
                let _ = crate::community::parse_topic(part, "u");
                let _ = crate::community::parse_guide(part, 1);
                let _ = crate::store::parse_reviews(part);
                let _ = crate::store::parse_review_page(part);
                let _ = crate::badges::parse_badges_page(part);
                let _ = crate::workshopui::page_screenshots(part);
            }
        }
    }
}
