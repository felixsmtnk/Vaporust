//! Game files: installing, downloading progress, changing the language, uninstalling a game that shares its folder.

use super::*;

#[allow(non_snake_case)]
pub(crate) fn CANCELLED() -> &'static str {
    tr!("Установка отменена. Скачанное сохранено, ее можно продолжить.", "Installation cancelled. What was downloaded is kept, it can be resumed.")
}

pub(crate) async fn run_install(
    client: SteamClient,
    appid: u32,
    library: PathBuf,
    language: Option<String>,
    abort: Arc<AtomicBool>,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    // If the download was hidden from Steam (pause/cancel) - put the appmanifest back.
    crate::local::unstash_manifest(&library, appid);
    set_install(st, ctx, |i| i.stage = tr!("Узнаю размер игры...", "Checking the game size...").into());
    let est = client
        .estimate_install_size(appid, DepotPlatform::Windows)
        .await
        .map_err(|e| trf!("Не удалось узнать размер игры: {e:#}", "Could not get the game size: {e:#}"))?;
    // What is already downloaded (resuming an install) needs no space.
    let game_files = crate::local::installed_size(&library, appid);
    if let Some(e) = space_error(est.disk_size, game_files, crate::local::free_space(&library)) {
        return Err(e);
    }
    set_install(st, ctx, |i| i.total = est.disk_size);
    if abort.load(Ordering::Relaxed) {
        return Err(CANCELLED().into());
    }

    let state = Arc::new(RwLock::new(DownloadState::default()));
    // We replace Aurelia's cancel flag with ours so the "Cancel" button works immediately.
    if let Ok(mut s) = state.write() {
        s.abort_signal = abort.clone();
    }
    let opts = InstallOptions {
        library_override: Some(library.to_string_lossy().into_owned()),
        language,
        ..Default::default()
    };
    let rx = client
        .install_game(appid, DepotPlatform::Windows, opts, state)
        .await
        .map_err(|e| trf!("Не удалось начать установку: {e:#}", "Could not start the installation: {e:#}"))?;
    drain_progress(rx, &abort, CANCELLED(), st, ctx).await
}

/// Common Aurelia progress loop (install, file verification, move): updates the progress
/// bar at the bottom of the window, computes speed. `cancelled` - text for a cancellation.
pub(crate) async fn drain_progress(
    mut rx: tokio::sync::mpsc::Receiver<aurelia::core::models::DownloadProgress>,
    abort: &Arc<AtomicBool>,
    cancelled: &str,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    // Speed: measured every ~2 s with smoothing.
    let mut last: Option<(Instant, u64)> = None;
    let mut speed = 0.0f64;
    while let Some(p) = rx.recv().await {
        match p.state {
            DownloadProgressState::Queued => {
                set_install(st, ctx, |i| i.stage = tr!("Подготовка загрузки...", "Preparing the download...").into());
            }
            DownloadProgressState::Downloading
            | DownloadProgressState::Verifying
            | DownloadProgressState::Moving => {
                let net_bytes = p.bytes_downloaded.saturating_sub(p.reused_bytes);
                let now = Instant::now();
                match last {
                    Some((t, b)) if now.duration_since(t) >= Duration::from_secs(2) => {
                        let secs = now.duration_since(t).as_secs_f64();
                        let inst = net_bytes.saturating_sub(b) as f64 / secs;
                        speed = if speed == 0.0 { inst } else { speed * 0.6 + inst * 0.4 };
                        last = Some((now, net_bytes));
                    }
                    None => last = Some((now, net_bytes)),
                    _ => {}
                }
                let stage = match p.state {
                    DownloadProgressState::Verifying => tr!("Проверка файлов", "Verifying files"),
                    DownloadProgressState::Moving => tr!("Перенос файлов", "Moving files"),
                    _ => tr!("Загрузка", "Download"),
                };
                set_install(st, ctx, |i| {
                    i.stage = stage.into();
                    i.done = p.bytes_downloaded;
                    if p.total_bytes > 0 {
                        i.total = p.total_bytes;
                    }
                    i.speed = speed;
                });
            }
            DownloadProgressState::Completed => return Ok(()),
            DownloadProgressState::Failed => {
                return Err(if abort.load(Ordering::Relaxed) {
                    cancelled.into()
                } else {
                    trf!("Ошибка: {}", "Error: {}", p.current_file)
                });
            }
        }
    }
    Err(if abort.load(Ordering::Relaxed) {
        cancelled.into()
    } else {
        tr!("Операция прервалась без результата.", "The operation stopped without a result.").into()
    })
}

/// A long operation on a game in the `install` slot (one at a time): progress bar at the bottom,
/// at the end - the result text and a disk rescan.
pub(crate) fn start_op<F, Fut>(appid: u32, name: String, done_text: &str, st: &Shared, ctx: &egui::Context, f: F)
where
    F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    if let Some(busy) = st.lock().unwrap().install.as_ref().filter(|i| i.active()).map(|i| i.name.clone()) {
        // One file operation at a time: said instead of silently dropping the request.
        crate::popup::push(
            crate::popup::Kind::Info,
            "Vaporust",
            trf!("«{name}»: сначала дождитесь окончания «{busy}» (загрузка или проверка файлов).", "\"{name}\": wait until \"{busy}\" finishes first (a download or a file check)."),
        );
        return;
    }
    let abort = Arc::new(AtomicBool::new(false));
    upd(st, ctx, |s| {
        s.install = Some(Install {
            appid,
            name,
            stage: tr!("Подготовка...", "Preparing...").into(),
            done: 0,
            total: 0,
            speed: 0.0,
            result: None,
            abort: abort.clone(),
            done_text: done_text.into(),
            download: false,
        });
    });
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        let res = f(abort).await;
        upd(&st, &ctx, |s| {
            if let Some(i) = s.install.as_mut() {
                i.result = Some(res);
                i.speed = 0.0;
            }
            s.disk_changed += 1;
        });
    });
}

/// Load everything for the "Properties" window (in parallel, as it becomes ready).
pub(crate) fn load_props(client: &SteamClient, appid: u32, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| {
        s.props.insert(appid, PropsData::default());
    });
    let set = move |st: &Shared, ctx: &egui::Context, f: Box<dyn FnOnce(&mut PropsData) + Send>| {
        upd(st, ctx, |s| f(s.props.entry(appid).or_default()));
    };

    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = c.fetch_game_languages(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.languages = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            // Steam does not show private (password) branches anyway until they are unlocked.
            let r = c
                .branches_detailed(appid)
                .await
                .map(|v| {
                    let mut v: Vec<_> = v.into_iter().filter(|b| !b.2).map(|b| (b.0, b.1)).collect();
                    if !v.iter().any(|b| b.0 == "public") {
                        v.push(("public".into(), None));
                    }
                    v
                })
                .map_err(es);
            set(&st, &ctx, Box::new(move |p| p.branches = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = load_dlc(&c, appid).await;
            set(&st, &ctx, Box::new(move |p| p.dlc = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = c.depot_descriptions(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.depots = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = async {
                let cloud = c.cloud_client().map_err(es)?;
                let ufs = c.fetch_ufs_config(appid).await.unwrap_or_default();
                let paths = (ufs.root_overrides, cloud.steam_id());
                let st2 = st.clone();
                set(&st2, &ctx, Box::new(move |p| p.cloud_paths = Some(paths)));
                let mut files = cloud.get_file_list(appid).await.map_err(es)?;
                files.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                Ok(files
                    .into_iter()
                    .map(|f| CloudFile { name: f.filename, size: f.size, time: f.timestamp as u64 })
                    .collect())
            }
            .await;
            set(&st, &ctx, Box::new(move |p| p.cloud = Some(r)));
        });
    }
}

pub(crate) async fn load_dlc(c: &SteamClient, appid: u32) -> Result<Vec<DlcRow>, String> {
    let ext = c.get_extended_app_info(appid).await.map_err(|e| format!("{e:#}"))?;
    let states = c.dlc_states(appid, &ext.dlcs).await.map_err(|e| format!("{e:#}"))?;
    Ok(states
        .into_iter()
        .map(|s| DlcRow { app_id: s.app_id, owned: s.owned, installed: s.installed, disabled: s.disabled })
        .collect())
}

/// After an Aurelia update: restore the original appmanifest (language, SharedDepots, InstallScripts...)
/// and, if the update succeeded, write the new depot versions and build from Aurelia's file into it.
pub fn merge_after_update(acf: &std::path::Path, original: &str, ok: bool) -> Result<(), String> {
    let updated = crate::local::installed_depots(acf);
    let build = crate::local::acf_buildid(acf);
    let tmp = acf.with_extension("acf.tmp");
    std::fs::write(&tmp, original)
        .and_then(|_| std::fs::rename(&tmp, acf))
        .map_err(|e| trf!("Не удалось вернуть файл игры: {e}", "Could not restore the game file: {e}"))?;
    if ok {
        crate::local::write_depot_versions(acf, &updated, build.as_deref(), 4)?;
    }
    Ok(())
}

/// Language change without Steam. `lang` - the new language (as in UserConfig: "russian", "english"...).
///
/// 1. Old-language depots are removed from appmanifest; if there were any - file verification
///    (the old language may have overwritten base files - verification restores the originals).
/// 2. Own new-language depots are downloaded (explicitly with a version: otherwise Aurelia takes only English).
///    Shared new-language depots (depotfromapp) - only as a link in SharedDepots, if the owner game
///    is already installed in the same library and has the depot; otherwise refused (Steam can do it).
/// 3. The original appmanifest is restored (Aurelia writes its own from scratch) and edited: depots, language
///    in UserConfig and MountedConfig, StateFlags 4.
pub async fn switch_language(
    c: &SteamClient,
    appid: u32,
    acf: &std::path::Path,
    lang: &str,
    abort: &Arc<AtomicBool>,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    let new_state = || {
        let s = Arc::new(RwLock::new(DownloadState::default()));
        if let Ok(mut w) = s.write() {
            w.abort_signal = abort.clone();
        }
        s
    };
    use crate::local;
    let info = local::read_acf(acf);
    let branch = if info.user_beta.is_empty() { "public".to_string() } else { info.user_beta.clone() };
    let depots = c
        .language_depots(appid, &branch)
        .await
        .map_err(|e| trf!("Не удалось узнать языковые файлы игры: {e:#}", "Could not get the game's language files: {e:#}"))?;
    let library = acf.parent().and_then(|p| p.parent()).ok_or(tr!("Не найдена папка библиотеки", "Library folder not found"))?.to_path_buf();
    let installed = local::installed_depots(acf);
    let shared = local::shared_depots(acf);
    let lang_of = |d: u32| depots.iter().find(|x| x.0 == d).map(|x| x.1.clone());
    let windows_ok = |os: &Option<String>| os.as_ref().is_none_or(|o| o.to_lowercase().contains("windows"));

    let mut edit = local::DepotEdit::default();
    // Old language: own depots - remove (with file verification), shared - remove the link.
    edit.remove_installed = installed.keys().copied().filter(|d| lang_of(*d).is_some_and(|l| l != lang)).collect();
    edit.remove_shared = shared.keys().copied().filter(|d| lang_of(*d).is_some_and(|l| l != lang)).collect();
    // New language.
    let mut new_own: Vec<(u32, u64)> = Vec::new();
    for (d, l, os, from, dlc, manifest) in &depots {
        if l != lang || dlc.is_some() || !windows_ok(os) || installed.contains_key(d) || shared.contains_key(d) {
            continue;
        }
        match from {
            None => {
                let m = manifest.ok_or(trf!("Нет версии языкового хранилища {d} на ветке {branch}", "No version of language depot {d} on branch {branch}"))?;
                new_own.push((*d, m));
            }
            Some(owner) => {
                // Shared depot: fits only if the owner game is next to it and has it installed.
                let owner_acf = library.join("steamapps").join(format!("appmanifest_{owner}.acf"));
                if local::installed_depots(&owner_acf).contains_key(d) {
                    edit.add_shared.push((*d, *owner));
                } else {
                    return Err(trf!(
                        "Этот язык ставится вместе с другой игрой (appid {owner}), которой нет в этой библиотеке \
                         или в ней нет этого языка. Смените язык через Steam: «Свойства» -> «Обновить через Steam».", "This language is installed with another game (appid {owner}) that is not in this library or lacks this language. Change the language through Steam: \"Properties\" -> \"Update via Steam\"."
                    ));
                }
            }
        }
    }

    let mut text = std::fs::read_to_string(acf).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
    let restore = |t: &str| -> Result<(), String> {
        let tmp = acf.with_extension("acf.tmp");
        std::fs::write(&tmp, t).and_then(|_| std::fs::rename(&tmp, acf)).map_err(|e| trf!("Не удалось вернуть файл игры: {e}", "Could not restore the game file: {e}"))
    };

    // 1. Remove the old language and verify files.
    if !edit.remove_installed.is_empty() {
        // As Steam does: delete old-language files that are not in the game's remaining depots
        // (shared ones included). If some list could not be obtained - delete nothing.
        if let Err(e) = remove_language_files(c, appid, acf, &library, &installed, &shared, &edit.remove_installed).await {
            eprintln!("old language files not removed: {e}");
        }
        local::edit_depot_lists(acf, &local::DepotEdit { remove_installed: edit.remove_installed.clone(), ..Default::default() })?;
        text = std::fs::read_to_string(acf).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
        set_install(st, ctx, |i| i.stage = tr!("Проверка файлов (после смены языка)", "Verifying files (after the language change)").into());
        let rx = c.verify_game(appid, new_state()).await.map_err(|e| trf!("Не удалось начать проверку: {e:#}", "Could not start the verification: {e:#}"))?;
        let res = drain_progress(rx, abort, tr!("Смена языка отменена.", "Language change cancelled."), st, ctx).await;
        restore(&text)?;
        res?;
    }

    // 2. Download own new-language depots.
    if !new_own.is_empty() {
        let opts = InstallOptions {
            library_override: Some(library.to_string_lossy().into_owned()),
            filter_depots: Some(new_own.iter().map(|(d, _)| *d as u64).collect()),
            manifest_overrides: Some(new_own.iter().copied().collect()),
            branch: Some(branch.clone()).filter(|b| b != "public"),
            ..Default::default()
        };
        let rx = c
            .install_game(appid, DepotPlatform::Windows, opts, new_state())
            .await
            .map_err(|e| trf!("Не удалось начать загрузку языка: {e:#}", "Could not start downloading the language: {e:#}"))?;
        let res = drain_progress(rx, abort, tr!("Смена языка отменена.", "Language change cancelled."), st, ctx).await;
        let sizes = local::installed_depot_sizes(acf);
        let got = local::installed_depots(acf);
        restore(&text)?;
        res?;
        for (d, _) in &new_own {
            let m = *got.get(d).ok_or(trf!("Языковое хранилище {d} не скачалось", "Language depot {d} was not downloaded"))?;
            edit.add_installed.push((*d, m, sizes.get(d).copied().unwrap_or(0)));
        }
    }

    // 3. Write the changes into the original appmanifest.
    local::edit_depot_lists(acf, &edit)?;
    local::set_user_config(acf, "language", lang)?;
    local::set_mounted_config(acf, "language", lang)?;
    // set_user_config marks "update needed" - the files are already in place.
    local::write_depot_versions(acf, &Default::default(), None, 4)?;
    Ok(())
}

/// Delete files of the `removed` depots that are in none of the game's remaining depots (own and shared).
pub(crate) async fn remove_language_files(
    c: &SteamClient,
    appid: u32,
    acf: &std::path::Path,
    library: &std::path::Path,
    installed: &std::collections::BTreeMap<u32, u64>,
    shared: &std::collections::BTreeMap<u32, u32>,
    removed: &[u32],
) -> Result<usize, String> {
    let text = std::fs::read_to_string(acf).map_err(es)?;
    let installdir = text
        .lines()
        .find_map(|l| {
            let t = l.trim();
            t.to_lowercase().starts_with("\"installdir\"").then(|| t.rsplit('"').nth(1).unwrap_or("").to_string())
        })
        .filter(|d| !d.is_empty())
        .ok_or(tr!("нет installdir", "no installdir"))?;
    let game_dir = library.join("steamapps").join("common").join(installdir);
    let files = |app: u32, depot: u32, manifest: u64| async move {
        c.depot_file_names(app, depot, manifest)
            .await
            .map_err(|e| trf!("список файлов хранилища {depot}: {e:#}", "file list of depot {depot}: {e:#}"))
    };
    let mut keep = std::collections::HashSet::new();
    for (d, m) in installed.iter().filter(|(d, _)| !removed.contains(d)) {
        keep.extend(files(appid, *d, *m).await?.iter().map(|f| file_key(f)));
    }
    for (d, owner) in shared {
        let owner_acf = library.join("steamapps").join(format!("appmanifest_{owner}.acf"));
        let m = *crate::local::installed_depots(&owner_acf)
            .get(d)
            .ok_or(trf!("неизвестна версия общего хранилища {d}", "unknown version of shared depot {d}"))?;
        keep.extend(files(*owner, *d, m).await?.iter().map(|f| file_key(f)));
    }
    let mut n = 0;
    for d in removed {
        let Some(m) = installed.get(d) else { continue };
        for f in files(appid, *d, *m).await? {
            // Windows file names do not depend on letter case: compare in one form.
            if keep.contains(&file_key(&f)) {
                continue;
            }
            let Some(rel) = safe_rel(&f) else { continue };
            let p = game_dir.join(rel);
            if p.is_file() && std::fs::remove_file(&p).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// The game's appmanifest in steamapps: normal or hidden from Steam (an unfinished Vaporust download).
pub(crate) fn manifest_of(steamapps: &std::path::Path, appid: u32) -> std::path::PathBuf {
    let acf = steamapps.join(format!("appmanifest_{appid}.acf"));
    if acf.exists() {
        return acf;
    }
    steamapps.join(format!("appmanifest_{appid}{}", crate::local::STASH_EXT))
}

/// Uninstall a game that shares its folder with other games (for example Counter-Strike, Half-Life,
/// Blue Shift and Opposing Force live in one common\Half-Life folder). Only files of this game's
/// depots that the neighbor games do not have are deleted; folders left empty are removed too.
/// File lists come from Steam; if even one cannot be obtained - nothing is deleted.
pub(crate) async fn uninstall_shared(
    c: &SteamClient,
    appid: u32,
    game_dir: &std::path::Path,
    others: &[u32],
) -> Result<usize, String> {
    let steamapps = game_dir
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or(tr!("неверная папка игры", "wrong game folder"))?
        .to_path_buf();
    let remove = shared_remove_list(c, appid, &steamapps, others).await?;
    remove_listed(appid, game_dir, &steamapps, &remove)
}

/// Which files (paths inside the game folder) to delete for a game in a shared folder: its depot files
/// that the neighbors do not have. Deletes nothing.
pub(crate) async fn shared_remove_list(
    c: &SteamClient,
    appid: u32,
    steamapps: &std::path::Path,
    others: &[u32],
) -> Result<Vec<String>, String> {
    let steamapps = steamapps.to_path_buf();
    let my_acf = manifest_of(&steamapps, appid);
    let installed = crate::local::installed_depots(&my_acf);
    let files = |app: u32, depot: u32, manifest: u64| async move {
        c.depot_file_names(app, depot, manifest)
            .await
            .map_err(|e| trf!("список файлов хранилища {depot}: {e:#}", "file list of depot {depot}: {e:#}"))
    };
    // Neighbors' files (their own and shared depots) - not touched.
    let mut keep = std::collections::HashSet::new();
    // This game's depots used by neighbors (their SharedDepots) - not touched either.
    let mut protected = std::collections::HashSet::new();
    for &other in others {
        let acf = manifest_of(&steamapps, other);
        for (d, m) in crate::local::installed_depots(&acf) {
            keep.extend(files(other, d, m).await?.into_iter().map(|f| f.to_lowercase()));
        }
        for (d, owner) in crate::local::shared_depots(&acf) {
            if owner == appid {
                protected.insert(d);
            }
            let owner_acf = manifest_of(&steamapps, owner);
            if let Some(m) = crate::local::installed_depots(&owner_acf).get(&d) {
                keep.extend(files(owner, d, *m).await?.into_iter().map(|f| f.to_lowercase()));
            }
        }
    }
    // Collect all lists first, then delete: on a network error the disk stays as it was.
    let mut remove = Vec::new();
    for (d, m) in installed.iter().filter(|(d, _)| !protected.contains(d)) {
        for f in files(appid, *d, *m).await? {
            if !keep.contains(&f.to_lowercase()) {
                remove.push(f);
            }
        }
    }
    Ok(remove)
}

/// Delete the listed files, emptied folders, the unfinished download and the appmanifest.
pub(crate) fn remove_listed(
    appid: u32,
    game_dir: &std::path::Path,
    steamapps: &std::path::Path,
    remove: &[String],
) -> Result<usize, String> {
    let mut n = 0;
    let mut dirs = std::collections::BTreeSet::new();
    for f in remove {
        let Some(rel) = safe_rel(f) else { continue };
        let p = game_dir.join(rel);
        if p.is_file() && std::fs::remove_file(&p).is_ok() {
            n += 1;
        }
        // Folders that held deleted files are candidates for removal if they became empty.
        let mut parent = p.parent();
        while let Some(dir) = parent {
            if dir == game_dir || !dir.starts_with(game_dir) {
                break;
            }
            dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    // Deepest first; remove_dir does not delete non-empty folders.
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(&d);
    }
    let _ = std::fs::remove_dir_all(steamapps.join("downloading").join(appid.to_string()));
    for f in [
        steamapps.join(format!("appmanifest_{appid}.acf")),
        steamapps.join(format!("appmanifest_{appid}{}", crate::local::STASH_EXT)),
    ] {
        if f.exists() {
            std::fs::remove_file(&f).map_err(|e| trf!("не удалось удалить {}: {e}", "could not delete {}: {e}", f.display()))?;
        }
    }
    Ok(n)
}

/// A depot file name in the form used for comparing: lower case, forward slashes
/// (NTFS does not tell "Data/Voice.pak" from "data/voice.pak").
pub(crate) fn file_key(f: &str) -> String {
    f.replace('\\', "/").to_lowercase()
}

/// A depot file name as a path inside the game folder; None for anything that could leave it
/// (an absolute path, a drive, "..").
pub(crate) fn safe_rel(f: &str) -> Option<std::path::PathBuf> {
    let f = f.replace('/', "\\");
    let p = std::path::Path::new(&f);
    if f.is_empty() || f.starts_with('\\') || p.is_absolute() {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::Normal(x) => out.push(x),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn depot_paths() {
        assert_eq!(super::file_key(r"Data\Voice.PAK"), "data/voice.pak");
        assert!(super::safe_rel("data/voice.pak").is_some());
        for bad in ["../x", "a/../../x", r"C:\Windows\x", r"\x", "/x", ""] {
            assert!(super::safe_rel(bad).is_none(), "{bad}");
        }
    }
}
