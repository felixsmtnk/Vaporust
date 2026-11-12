//! vaporust: скриншоты Steam, загруженные в облако Steam (файлы сообщества с типом «скриншот»).

use super::*;
use steam_vent_proto::steammessages_publishedfile_steamclient::{
    CPublishedFile_GetUserFiles_Request, CPublishedFile_GetUserFiles_Response,
};

/// Фильтр «скриншоты» (EPublishedFileInfoMatchingFileType = 4; 5 — коллекции, проверено).
const FILE_TYPE_SCREENSHOT: u32 = 4;

/// Загруженный скриншот.
#[derive(Debug, Clone)]
pub struct CloudScreenshot {
    pub id: u64,
    pub appid: u32,
    pub app_name: String,
    /// Подпись к скриншоту.
    pub caption: String,
    /// Полный размер.
    pub file_url: String,
    /// Уменьшенная копия.
    pub preview_url: String,
    pub width: u32,
    pub height: u32,
    pub time: u32,
    /// 0 — для всех, 1 — для друзей, 2 — только я, 3 — скрыт.
    pub visibility: u32,
}

impl SteamClient {
    /// Свои скриншоты в облаке Steam: игра `appid` (0 — все), страница `page` (с 1), по `per_page`.
    /// Возвращает (всего, скриншоты страницы).
    pub async fn my_cloud_screenshots(&self, appid: u32, page: u32, per_page: u32) -> Result<(u32, Vec<CloudScreenshot>)> {
        let connection = self.require_connection()?;
        let mut req = CPublishedFile_GetUserFiles_Request::new();
        req.set_steamid(u64::from(connection.steam_id()));
        req.set_appid(appid);
        req.set_page(page.max(1));
        req.set_numperpage(per_page);
        req.set_type("myfiles".to_string());
        req.set_filetype(FILE_TYPE_SCREENSHOT);
        req.set_sortmethod("newestfirst".to_string());
        req.set_return_short_description(true);
        let resp: CPublishedFile_GetUserFiles_Response = connection
            .service_method(req)
            .await
            .map_err(|e| anyhow!("PublishedFile.GetUserFiles failed: {e}"))?;
        let shots = resp
            .publishedfiledetails
            .iter()
            .map(|d| CloudScreenshot {
                id: d.publishedfileid(),
                appid: d.consumer_appid(),
                app_name: d.app_name().to_string(),
                caption: if d.title().is_empty() { d.short_description().to_string() } else { d.title().to_string() },
                file_url: d.file_url().to_string(),
                preview_url: d.preview_url().to_string(),
                width: d.image_width(),
                height: d.image_height(),
                time: d.time_created(),
                visibility: d.visibility(),
            })
            .collect();
        Ok((resp.total(), shots))
    }
}
