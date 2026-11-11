//! vaporust: данные для окна «Свойства игры», которых нет в Aurelia 0.2.3.

use super::*;

impl SteamClient {
    /// Языки, которые поддерживает игра (ключи `common/supported_languages`, иначе
    /// `common/languages` из PICS appinfo), например "english", "russian".
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

    /// Какие из установленных игр (appid, папка игры) требуют обновления: флаг Steam
    /// UpdateRequired в appmanifest или более новая сборка активной ветки на серверах.
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
            })
            .collect();
        self.check_for_updates(&mut list).await?;
        Ok(list.into_iter().filter(|g| g.update_available).map(|g| g.app_id).collect())
    }

    /// Страна для цен магазина: по IP подключения к Steam (иначе US).
    pub fn store_country(&self) -> String {
        self.connection
            .as_ref()
            .map(|c| c.ip_country_code().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "US".to_string())
    }

    /// SteamID64 вошедшего аккаунта.
    pub fn my_steam_id(&self) -> Option<u64> {
        self.connection.as_ref().map(|c| u64::from(c.steam_id()))
    }
}
