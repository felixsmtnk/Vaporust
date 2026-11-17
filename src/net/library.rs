//! Library and connection: the owned games cache, the encrypted login, the connection probe.

use super::*;

/// Own game list cache - a separate file per account (Aurelia has one for everybody).
/// The library is taken from it when there is no connection to Steam.
pub(crate) fn library_cache_file(steam_id: u64) -> Option<std::path::PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join(format!("library_{steam_id}.json")))
}

/// Write the cache only while the response belongs to the current login (under the state lock):
/// a late response for the previous account must not get into the new one's cache.
pub(crate) fn save_library_cache(st: &Shared, steam_id: Option<u64>, games: &[aurelia::core::models::OwnedGame]) {
    let Some(p) = steam_id.and_then(library_cache_file) else { return };
    let Some(_guard) = lock_current(st) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    if serde_json::to_vec(games).ok().is_some_and(|b| std::fs::write(&tmp, b).is_ok()) {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The account's game list cache (for working offline).
pub(crate) fn load_library_cache(steam_id: Option<u64>) -> Vec<aurelia::core::models::OwnedGame> {
    steam_id
        .and_then(library_cache_file)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) async fn load_owned(client: &mut SteamClient, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| s.message = tr!("Загружаю библиотеку...", "Loading the library...").into());
    match client.fetch_owned_games().await {
        Ok(games) => {
            save_library_cache(st, client.my_steam_id(), &games);
            upd(st, ctx, |s| {
                s.message.clear();
                s.owned = Arc::new(games);
                s.busy = false;
            })
        }
        Err(e) => upd(st, ctx, |s| {
            s.message = trf!("Не удалось получить список игр: {e:#}", "Could not get the game list: {e:#}");
            s.busy = false;
        }),
    }
    // Family library: an error does not affect the rest (the account may not be in a family).
    if let Ok(family) = client.fetch_family_shared_apps().await {
        upd(st, ctx, |s| s.family = Arc::new(family));
    }
}

/// Keep the saved login (Aurelia's session.json) encrypted: it holds the key with which
/// Steam lets you in without a password. Aurelia writes it in plain text by default (encryption
/// is enabled only by its console command secure-session), so after every login
/// we turn on encrypt_session and save the file again. The key lives in the Windows Credential
/// Manager (for this Windows user only). Ok(false) - the key store is unavailable
/// (or disabled by AURELIA_DISABLE_KEYRING), the file stayed open.
pub(crate) async fn secure_session() -> anyhow::Result<bool> {
    use aurelia::core::config::{load_launcher_config, load_session, save_launcher_config, save_session, session_path};
    use aurelia::core::session_crypto::{keyring_secret_or_create, EncryptedSession};
    let key = tokio::task::spawn_blocking(keyring_secret_or_create).await.ok().flatten();
    if key.is_none() {
        return Ok(false);
    }
    let mut cfg = load_launcher_config().await?;
    if !cfg.encrypt_session {
        cfg.encrypt_session = true;
        save_launcher_config(&cfg).await?;
    }
    let path = session_path()?;
    let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    if raw.is_empty() || EncryptedSession::from_json(&raw).is_some() {
        return Ok(true);
    }
    let session = load_session().await?;
    save_session(&session).await?;
    let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    Ok(EncryptedSession::from_json(&raw).is_some())
}

/// Every 20 s check whether Steam is reachable (a connection to steamcommunity.com:443). Two
/// failures in a row - offline mode, as when starting without network: the "No connection to Steam" bar,
/// and offline_tick in the UI tries to log in again once a minute. Previously Vaporust noticed
/// a lost internet connection only after a restart.
pub(crate) fn start_link_probe(st: &Shared, ctx: &egui::Context) {
    let gen = PROBE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        let mut fails = 0;
        loop {
            tokio::time::sleep(crate::lite::secs(20, 60)).await;
            if PROBE_GEN.load(Ordering::Relaxed) != gen {
                return;
            }
            let ok = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect("steamcommunity.com:443"),
            )
            .await
            .is_ok_and(|r| r.is_ok());
            if ok {
                fails = 0;
                continue;
            }
            fails += 1;
            if fails < 2 || PROBE_GEN.load(Ordering::Relaxed) != gen {
                continue;
            }
            let mut went = false;
            upd(&st, &ctx, |s| {
                if s.account.is_some() {
                    s.offline = true;
                    s.offline_reason = tr!("Нет подключения к интернету", "No internet connection").into();
                    s.account = None;
                    went = true;
                }
            });
            if went {
                crate::errlog::write(tr!("сеть", "network"), tr!("связь со Steam пропала (проверка соединения)", "connection to Steam lost (connection check)"));
            }
            return;
        }
    });
}

/// Did the login fail because of the network, not the login itself? Aurelia does not always tell them apart
/// (without internet it has a fallback server list, and the error may be a "timeout" of kind Unknown).
/// With a saved login, any error counts as a network one unless it is a login refusal.
pub(crate) async fn network_failure(e: &anyhow::Error) -> bool {
    use aurelia::core::error::ErrorKind as K;
    let kind = aurelia::core::error::classify(e).kind;
    if matches!(kind, K::Offline | K::NetworkTimeout | K::Timeout) {
        return true;
    }
    if matches!(kind, K::AuthRequired | K::LoginDenied | K::LoginExpired | K::AccessDenied) {
        return false;
    }
    let has_session = aurelia::core::config::load_session()
        .await
        .is_ok_and(|s| s.account_name.is_some() && s.refresh_token.is_some());
    let text = format!("{e:#}").to_lowercase();
    let auth_text = ["invalid", "expired", "revoked", "denied", "access", "no persisted"].iter().any(|w| text.contains(w));
    has_session && !auth_text
}
