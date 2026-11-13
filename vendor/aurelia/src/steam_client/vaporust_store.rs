//! vaporust: очередь открытий магазина Steam (Store.GetDiscoveryQueue).

use super::*;
use steam_vent_proto::steammessages_store_steamclient::{
    CStore_GetDiscoveryQueue_Request, CStore_GetDiscoveryQueue_Response, CStore_SkipDiscoveryQueueItem_Request,
    CStore_SkipDiscoveryQueueItem_Response, EStoreDiscoveryQueueType,
};

impl SteamClient {
    /// Очередь открытий (обычно 12 игр, подобранных Steam). `rebuild` - собрать новую.
    pub async fn discovery_queue(&self, country: &str, rebuild: bool) -> Result<Vec<u32>> {
        let connection = self.require_connection()?;
        let mut req = CStore_GetDiscoveryQueue_Request::new();
        req.set_queue_type(EStoreDiscoveryQueueType::k_EStoreDiscoveryQueueTypeNew);
        if !country.is_empty() {
            req.set_country_code(country.to_string());
        }
        req.set_rebuild_queue(rebuild);
        req.set_rebuild_queue_if_stale(true);
        let resp: CStore_GetDiscoveryQueue_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("Store.GetDiscoveryQueue failed: {e}"))?;
        Ok(resp.appids.clone())
    }

    /// Отметить игру очереди просмотренной (как «Далее» в очереди Steam).
    pub async fn discovery_skip(&self, appid: u32) -> Result<()> {
        let connection = self.require_connection()?;
        let mut req = CStore_SkipDiscoveryQueueItem_Request::new();
        req.set_queue_type(EStoreDiscoveryQueueType::k_EStoreDiscoveryQueueTypeNew);
        req.set_appid(appid);
        let _: CStore_SkipDiscoveryQueueItem_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("Store.SkipDiscoveryQueueItem failed: {e}"))?;
        Ok(())
    }
}

impl SteamClient {
    /// vaporust: веб-токен доступа (тот же, что у WebSession) - для методов Web API Steam
    /// с параметром access_token (уведомления, значки, очки Steam, семья).
    pub async fn web_access_token(&self) -> Result<String> {
        use steam_vent_proto::steammessages_auth_steamclient::{
            CAuthentication_AccessToken_GenerateForApp_Request, CAuthentication_AccessToken_GenerateForApp_Response,
        };
        let connection = self.require_connection()?;
        let mut req = CAuthentication_AccessToken_GenerateForApp_Request::new();
        req.set_refresh_token(connection.refresh_token().token().to_string());
        req.set_steamid(u64::from(connection.steam_id()));
        let resp: CAuthentication_AccessToken_GenerateForApp_Response =
            connection.service_method(req).await.map_err(|e| anyhow!("failed to mint a web access token: {e}"))?;
        let token = resp.access_token();
        if token.is_empty() {
            bail!("Steam returned an empty web access token");
        }
        Ok(token.to_string())
    }
}
