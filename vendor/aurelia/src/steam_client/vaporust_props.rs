//! vaporust: data for the "Game properties" window that is missing in Aurelia 0.2.3.

use super::*;

impl SteamClient {
    /// Languages the game supports (keys `common/supported_languages`, otherwise
    /// `common/languages` from PICS appinfo), for example "english", "russian".
    pub async fn fetch_game_languages(&self, appid: u32) -> Result<Vec<String>> {
        let buffer = self
            .pics_buffer(appid, "failed requesting appinfo product info for languages")
            .await?;
        let vdf = find_vdf_in_pics(&buffer).context("failed to parse product info VDF")?;
        let section = pics_app_section(vdf.value());
        let mut langs: Vec<String> = section
            .get_obj(&["common", "supported_languages"])
            .or_else(|| section.get_obj(&["common", "languages"]))
            .map(|o| o.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default();
        langs.sort();
        langs.dedup();
        Ok(langs)
    }

    /// Which installed games (appid, game folder) need an update: Steam's
    /// UpdateRequired flag in appmanifest or a newer build of the active branch on the servers.
    pub async fn games_needing_update(&self, games: &[(u32, std::path::PathBuf)]) -> Result<Vec<u32>> {
        let mut list: Vec<LibraryGame> = games
            .iter()
            .map(|(app_id, dir)| LibraryGame {
                app_id: *app_id,
                name: String::new(),
                playtime_forever_minutes: None,
                is_installed: true,
                install_path: Some(dir.to_string_lossy().into_owned()),
                local_manifest_ids: HashMap::new(),
                update_available: false,
                update_queued: false,
                active_branch: "public".into(),
                is_owned: true,
                is_family_shared: false,
                online_required: None,
                platform: None,
                from_windows_steam: false,
                steam_config_mismatch: false,
                size_on_disk: None,
            })
            .collect();
        self.check_for_updates(&mut list).await?;
        Ok(list.into_iter().filter(|g| g.update_available).map(|g| g.app_id).collect())
    }

    /// vaporust: latest depot versions of a branch (depot -> manifest ID) and the build number.
    /// Needed for version pinning: Vaporust writes them into appmanifest, and
    /// the real Steam considers the game up to date.
    pub async fn latest_depot_versions(
        &self,
        appid: u32,
        branch: &str,
    ) -> Result<(HashMap<u32, u64>, Option<String>)> {
        let connection = self.require_connection()?;
        let manifests = Self::remote_manifest_ids_static(connection, appid, branch).await?;
        let buildid = Self::remote_buildid_static(connection, appid, branch).await;
        Ok((manifests.into_iter().map(|(d, m)| (d as u32, m)).collect(), buildid))
    }

    /// vaporust: game depot descriptions from PICS: depot -> (name, oslist, language, DLC appid).
    /// Many depots have no name - then the UI describes them by OS and language.
    pub async fn depot_descriptions(
        &self,
        appid: u32,
    ) -> Result<HashMap<u32, (Option<String>, Option<String>, Option<String>, Option<u32>)>> {
        let buffer = self.pics_buffer(appid, "failed requesting appinfo for depots").await?;
        let vdf = find_vdf_in_pics(&buffer).context("failed to parse product info VDF")?;
        let mut out = HashMap::new();
        if let Some(depots) = pics_depots_value(&vdf).and_then(|v| v.as_obj()) {
            for (key, value) in depots.iter() {
                let Ok(id) = key.parse::<u32>() else { continue };
                // A string value by path inside a depot description.
                let s = |path: &[&str]| -> Option<String> {
                    let mut cur = value;
                    for p in path {
                        cur = cur.as_obj()?.get(*p)?;
                    }
                    cur.as_str().map(str::to_string).filter(|v| !v.is_empty())
                };
                let dlc = s(&["dlcappid"]).and_then(|v| v.parse().ok());
                out.insert(id, (s(&["name"]), s(&["config", "oslist"]), s(&["config", "language"]), dlc));
            }
        }
        Ok(out)
    }

    /// vaporust: language depots of a game for changing the language without Steam: (depot, language, oslist,
    /// depotfromapp - the depot belongs to another game (shared), dlcappid, current version on branch `branch`).
    pub async fn language_depots(
        &self,
        appid: u32,
        branch: &str,
    ) -> Result<Vec<(u32, String, Option<String>, Option<u32>, Option<u32>, Option<u64>)>> {
        let buffer = self.pics_buffer(appid, "failed requesting appinfo for language depots").await?;
        let vdf = find_vdf_in_pics(&buffer).context("failed to parse product info VDF")?;
        let mut out = Vec::new();
        if let Some(depots) = pics_depots_value(&vdf).and_then(|v| v.as_obj()) {
            for (key, value) in depots.iter() {
                let Ok(id) = key.parse::<u32>() else { continue };
                let s = |path: &[&str]| -> Option<String> {
                    let mut cur = value;
                    for p in path {
                        cur = cur.as_obj()?.get(*p)?;
                    }
                    cur.as_str().map(str::to_string).filter(|v| !v.is_empty())
                };
                let Some(lang) = s(&["config", "language"]) else { continue };
                // The robust reader: binary PICS can store the manifest id as a number, not a string.
                let manifest = extract_manifest_id_robust(value, branch).or_else(|| extract_manifest_id_robust(value, "public"));
                out.push((
                    id,
                    lang,
                    s(&["config", "oslist"]),
                    s(&["depotfromapp"]).and_then(|v| v.parse().ok()),
                    s(&["dlcappid"]).and_then(|v| v.parse().ok()),
                    manifest,
                ));
            }
        }
        Ok(out)
    }

    /// vaporust: all game branches with a description and the "password required" flag (pwdrequired).
    /// `fetch_branches` hides private branches, but the UI needs to know they exist.
    pub async fn branches_detailed(&self, appid: u32) -> Result<Vec<(String, Option<String>, bool)>> {
        let buffer = self.pics_buffer(appid, "failed requesting appinfo for branches").await?;
        let vdf = find_vdf_in_pics(&buffer).context("failed to parse product info VDF")?;
        let mut out = Vec::new();
        if let Some(branches) = pics_depots_value(&vdf)
            .and_then(|v| v.as_obj())
            .and_then(|d| d.get("branches"))
            .and_then(|b| b.as_obj())
        {
            for (name, node) in branches.iter() {
                let o = node.as_obj();
                let get = |k: &str| o.and_then(|o| o.get(k)).and_then(|v| v.as_str()).map(str::to_string);
                let private = get("pwdrequired").is_some_and(|v| v != "0");
                out.push((name.to_string(), get("description").filter(|d| !d.is_empty()), private));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Country for store prices: by the IP of the Steam connection (otherwise US).
    pub fn store_country(&self) -> String {
        self.connection
            .as_ref()
            .map(|c| c.ip_country_code().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "US".to_string())
    }

    /// SteamID64 of the logged-in account.
    pub fn my_steam_id(&self) -> Option<u64> {
        self.connection.as_ref().map(|c| u64::from(c.steam_id()))
    }
}

impl SteamClient {
    /// vaporust: file names of a depot in version `manifest` (lowercase, "/" separator).
    /// Unlike `fetch_manifest_files` (depot_browser creates a CDN client without servers and fails
    /// with "no available cdn servers"), it takes the servers and key the same way the install does.
    pub async fn depot_file_names(&self, appid: u32, depot: u32, manifest: u64) -> Result<Vec<String>> {
        let connection = self.require_connection_owned()?;
        let hosts = self.get_content_servers(connection.cell_id()).await?;
        let key = self.get_depot_key(appid, depot).await?;
        let key: [u8; 32] = key.as_slice().try_into().map_err(|_| anyhow!("bad depot key length"))?;
        let code = self.get_manifest_request_code(appid, depot, manifest).await.ok();
        let mut last = anyhow!("no content servers");
        for host in hosts.iter().take(4) {
            let token = self.get_cdn_auth_token(appid, depot, &host.host).await.ok();
            let server = super::install::cdn_server_for(host, connection.cell_id(), token);
            let cdn = steam_cdn::CDNClient::with_servers(Arc::new(connection.clone()), vec![server]);
            match cdn.get_manifest(appid, depot, manifest, code, Some(key)).await {
                Ok(m) => {
                    return Ok(m
                        .files()
                        .into_iter()
                        .map(|f| f.full_path().to_lowercase())
                        .filter(|n| !n.is_empty())
                        .collect())
                }
                Err(e) => last = anyhow!("{e}"),
            }
        }
        Err(last)
    }
}
