//! vaporust: Steam screenshots uploaded to the Steam cloud (community files of the "screenshot" type).

use super::*;
use steam_vent_proto::steammessages_publishedfile_steamclient::{
    CPublishedFile_GetUserFiles_Request, CPublishedFile_GetUserFiles_Response,
};

/// The "screenshots" filter (EPublishedFileInfoMatchingFileType = 4; 5 - collections, checked).
const FILE_TYPE_SCREENSHOT: u32 = 4;

/// An uploaded screenshot.
#[derive(Debug, Clone)]
pub struct CloudScreenshot {
    pub id: u64,
    pub appid: u32,
    pub app_name: String,
    /// Screenshot caption.
    pub caption: String,
    /// Full size.
    pub file_url: String,
    /// Reduced copy.
    pub preview_url: String,
    pub width: u32,
    pub height: u32,
    pub time: u32,
    /// 0 - public, 1 - friends, 2 - only me, 3 - hidden.
    pub visibility: u32,
    /// vaporust: "like" votes and comments.
    pub votes_up: u32,
    pub comments: u32,
}

impl SteamClient {
    /// Own screenshots in the Steam cloud: game `appid` (0 - all), page `page` (from 1), `per_page` each.
    /// Returns (total, screenshots of the page).
    pub async fn my_cloud_screenshots(&self, appid: u32, page: u32, per_page: u32) -> Result<(u32, Vec<CloudScreenshot>)> {
        let me = u64::from(self.require_connection()?.steam_id());
        self.user_screenshots(me, appid, page, per_page).await
    }

    /// Screenshots of any user (for others - only those they show to you).
    pub async fn user_screenshots(&self, steam_id: u64, appid: u32, page: u32, per_page: u32) -> Result<(u32, Vec<CloudScreenshot>)> {
        let connection = self.require_connection()?;
        let mut req = CPublishedFile_GetUserFiles_Request::new();
        req.set_steamid(steam_id);
        req.set_appid(appid);
        req.set_page(page.max(1));
        req.set_numperpage(per_page);
        req.set_type("myfiles".to_string());
        req.set_filetype(FILE_TYPE_SCREENSHOT);
        req.set_sortmethod("newestfirst".to_string());
        req.set_return_short_description(true);
        req.set_return_vote_data(true);
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
                votes_up: d.vote_data.as_ref().map_or(0, |v| v.votes_up()),
                comments: d.num_comments_public().max(0) as u32,
            })
            .collect();
        Ok((resp.total(), shots))
    }
}
