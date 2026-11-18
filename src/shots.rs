//! Steam screenshots: "On this PC" (Steam folder userdata\<account>\760\remote\<game>\screenshots)
//! and "In the Steam cloud" (uploaded screenshots - PublishedFile.GetUserFiles, filter 4).
//! A grid of thumbnails (Steam thumbnails for local ones), click - full size,
//! right click - show in folder / page in Steam / download to the computer.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TILE: egui::Vec2 = egui::vec2(220.0, 124.0);
const ROW_H: f32 = 124.0 + 24.0;

/// A screenshot on this PC.
#[derive(Clone)]
pub struct LocalShot {
    pub appid: u64,
    pub path: PathBuf,
    pub thumb: Option<PathBuf>,
    /// File modification time (unix).
    pub time: u64,
    /// Downloaded from the cloud by Vaporust (Pictures\Vaporust\<game>): the game name comes from the folder name.
    pub game: Option<String>,
}

pub struct ShotsWin {
    /// 0 - on this PC, 1 - in the Steam cloud.
    pub tab: u8,
    /// Only this game (None - all).
    pub filter: Option<u32>,
    local: Option<Vec<LocalShot>>,
    /// For which filter cloud ones were already requested: (whose, game) - so we do not ask every frame.
    cloud_asked: Option<(u64, u32)>,
    /// A friend's screenshots (SteamID64, name); None - own.
    pub user: Option<(u64, String)>,
    /// Download-to-computer progress.
    status: Arc<Mutex<String>>,
    /// The download finished - reread "On this PC".
    rescan: Arc<std::sync::atomic::AtomicBool>,
    /// Games seen in cloud screenshots (for the filter).
    cloud_games: std::collections::BTreeMap<u32, String>,
}

impl ShotsWin {
    pub fn new(filter: Option<u32>) -> Self {
        Self {
            tab: 0,
            filter,
            local: None,
            cloud_asked: None,
            user: None,
            status: Default::default(),
            rescan: Default::default(),
            cloud_games: Default::default(),
        }
    }
}

/// Steam user folder (userdata\<account>) - the one whose localconfig.vdf is the most recent.
fn user_dir(steam_dir: &Path) -> Option<PathBuf> {
    crate::launchopts::find_localconfig(steam_dir)?.parent()?.parent().map(Path::to_path_buf)
}

/// All screenshots on this PC, newest first.
pub fn scan_local(steam_dir: &Path) -> Vec<LocalShot> {
    let mut out = Vec::new();
    let Some(remote) = user_dir(steam_dir).map(|u| u.join("760").join("remote")) else { return out };
    let Ok(apps) = std::fs::read_dir(&remote) else { return out };
    for app in apps.flatten() {
        let Some(appid) = app.file_name().to_str().and_then(|s| s.parse::<u64>().ok()) else { continue };
        let dir = app.path().join("screenshots");
        let Ok(files) = std::fs::read_dir(&dir) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !["jpg", "jpeg", "png"].contains(&ext.as_str()) {
                continue;
            }
            let thumb = path.file_name().map(|n| dir.join("thumbnails").join(n)).filter(|t| t.exists());
            let time = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(LocalShot { appid, path, thumb, time, game: None });
        }
    }
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}

/// Thumbnails of downloaded screenshots (in the Vaporust data folder, not in "Pictures").
fn thumbs_dir() -> Option<PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join("shot_thumbs"))
}

/// Screenshots downloaded from the cloud to Pictures\Vaporust\<game>\<appid>_<id>.jpg.
pub fn scan_downloads() -> Vec<LocalShot> {
    let mut out = Vec::new();
    let Ok(games) = std::fs::read_dir(download_dir("")) else { return out };
    for g in games.flatten() {
        let game = g.file_name().to_string_lossy().to_string();
        let Ok(files) = std::fs::read_dir(g.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("png")) {
                continue;
            }
            let (appid, id) = match stem.split_once('_') {
                Some((a, i)) => (a.parse().unwrap_or(0), i.to_string()),
                None => (0, stem.clone()),
            };
            let thumb = thumbs_dir().map(|d| d.join(format!("{id}.jpg"))).filter(|t| t.exists());
            let time = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(LocalShot { appid, path, thumb, time, game: Some(game.clone()) });
        }
    }
    out
}

fn file_uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

/// Folder for downloaded screenshots: Pictures\Vaporust\<game>.
fn download_dir(game: &str) -> PathBuf {
    let safe: String = game.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let pics = crate::util::pictures();
    pics.join("Vaporust").join(safe.trim())
}

/// Open a file with the default program (photo viewer).
fn open_file(p: &Path) {
    let _ = std::process::Command::new(crate::util::system_exe("explorer.exe")).arg(p).spawn();
}

/// Show a file in Explorer (selected).
fn show_in_folder(p: &Path) {
    // Explorer understands only /select,"path" - with quotes around the path, not the whole argument,
    // so we build the string ourselves (otherwise a path with spaces opens "Documents").
    let mut cmd = std::process::Command::new(crate::util::system_exe("explorer.exe"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/select,\"{}\"", p.display()));
    }
    let _ = cmd.spawn();
}

impl LauncherApp {
    /// Open "Screenshots" (filtered by game, if given).
    pub(crate) fn open_shots(&mut self, filter: Option<u32>) {
        self.close_extras(crate::View::Screenshots);
        // A friend's screenshots were open - start over with own ones.
        if self.shots_win.as_ref().is_some_and(|w| w.user.is_some()) {
            self.shots_win = None;
        }
        let w = self.shots_win.get_or_insert_with(|| ShotsWin::new(filter));
        w.filter = filter;
        self.view = crate::View::Screenshots;
    }

    /// A friend's screenshots (those they show to you).
    pub(crate) fn open_friend_shots(&mut self, id: u64, name: String) {
        self.close_extras(crate::View::Screenshots);
        let mut w = ShotsWin::new(None);
        w.user = Some((id, name));
        w.tab = 1;
        self.shots_win = Some(w);
        self.view = crate::View::Screenshots;
    }

    /// Game name by appid: the library, then the name cache.
    fn game_name(&self, appid: u64, net: &NetState) -> String {
        let id = appid as u32;
        if appid <= u32::MAX as u64 {
            if let Some(g) = self.games.iter().find(|g| g.appid == id) {
                return g.name.clone();
            }
            if let Some(g) = net.owned.iter().find(|g| g.app_id == id) {
                return g.name.clone();
            }
            if let Some(n) = net.app_names.get(&id) {
                return n.clone();
            }
            if let Some(f) = net.family.iter().find(|f| f.app_id == id) {
                return f.name.clone();
            }
        }
        trf!("Игра {appid}", "Game {appid}")
    }

    pub(crate) fn shots_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(mut w) = self.shots_win.take() else { return };
        if w.rescan.swap(false, std::sync::atomic::Ordering::Relaxed) {
            w.local = None;
        }
        if w.local.is_none() {
            // Steam screenshot folder + those downloaded by Vaporust from the cloud; newest first.
            let mut v = self.steam.as_ref().map(|s| scan_local(&s.dir)).unwrap_or_default();
            v.extend(scan_downloads());
            v.sort_by(|a, b| b.time.cmp(&a.time));
            w.local = Some(v);
        }
        // Games from cloud screenshots go into the filter (remembered while the section is open).
        if let Some(Ok((_, _, list))) = &net.cloud_shots {
            for s in list {
                let name = if s.app_name.is_empty() { self.game_name(s.appid as u64, net) } else { s.app_name.clone() };
                w.cloud_games.entry(s.appid).or_insert(name);
            }
        }
        let uid = w.user.as_ref().map_or(0, |u| u.0);
        if uid != 0 {
            w.tab = 1; // for a friend - only the cloud
        }
        if w.tab == 1 && net.account.is_some() && w.cloud_asked != Some((uid, w.filter.unwrap_or(0))) {
            w.cloud_asked = Some((uid, w.filter.unwrap_or(0)));
            let _ = self.tx.send(Cmd::CloudShots { user: uid, appid: w.filter.unwrap_or(0), page: 1 });
        }
        let local: Vec<LocalShot> = w
            .local
            .as_ref()
            .unwrap()
            .iter()
            .filter(|s| w.filter.map_or(true, |f| s.appid == f as u64))
            .cloned()
            .collect();
        let cloud_all = match &net.cloud_shots {
            Some(Ok((a, total, v))) if *a == w.filter.unwrap_or(0) && net.cloud_shots_user == uid => Some((*total, v.clone())),
            _ => None,
        };

        ui.horizontal(|ui| {
            if let Some((_, name)) = &w.user {
                crate::theme::page_title(ui, &trf!("Скриншоты: {name}", "Screenshots: {name}"));
                if let Some((total, _)) = &cloud_all {
                    ui.weak(format!("({total})"));
                }
            } else {
                crate::theme::page_title(ui, tr!("Скриншоты", "Screenshots"));
                ui.separator();
                crate::theme::tab_value(ui, &mut w.tab, 0, trf!("На этом ПК ({})", "On this PC ({})", local.len()));
                let cloud_label = match &cloud_all {
                    Some((total, _)) => trf!("В облаке Steam ({total})", "In the Steam cloud ({total})"),
                    None => tr!("В облаке Steam", "In the Steam cloud").to_string(),
                };
                crate::theme::tab_value(ui, &mut w.tab, 1, cloud_label);
            }
            ui.separator();
            // Game filter: games that have screenshots on the PC, plus the current filter.
            let mut games: Vec<(u32, String)> = w
                .local
                .as_ref()
                .unwrap()
                .iter()
                .filter(|s| s.appid <= u32::MAX as u64)
                .map(|s| s.appid as u32)
                .chain(w.filter)
                .chain(w.cloud_games.keys().copied())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|a| (a, w.cloud_games.get(&a).cloned().unwrap_or_else(|| self.game_name(a as u64, net))))
                .collect();
            games.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            let current = w
                .filter
                .map(|f| w.cloud_games.get(&f).cloned().unwrap_or_else(|| self.game_name(f as u64, net)))
                .unwrap_or_else(|| tr!("Все игры", "All games").into());
            egui::ComboBox::from_id_salt("shots_game").selected_text(trf!("Игра: {current}", "Game: {current}")).show_ui(ui, |ui| {
                ui.selectable_value(&mut w.filter, None, tr!("Все игры", "All games"));
                for (a, n) in &games {
                    ui.selectable_value(&mut w.filter, Some(*a), n);
                }
            });
            if w.tab == 0 && crate::theme::text_link(ui, tr!("Обновить", "Refresh")).on_hover_text(tr!("Перечитать папку скриншотов", "Reread the screenshot folder")).clicked() {
                w.local = None;
            }
        });
        let status = w.status.lock().unwrap().clone();
        if !status.is_empty() {
            ui.weak(status);
        }
        ui.separator();

        if w.tab == 0 {
            if local.is_empty() {
                ui.weak(
                    tr!("Скриншотов на этом ПК нет. Steam сохраняет их по клавише F12 в игре (папка Steam\\userdata\\…\\760). \
                     Загруженные в облако - на вкладке «В облаке Steam»; скачанные оттуда появятся и здесь.", "There are no screenshots on this PC. Steam saves them with the F12 key in a game (folder Steam\\userdata\\…\\760). Uploaded ones are on the \"In the Steam cloud\" tab; downloaded from there, they appear here too."),
                );
            } else {
                self.shots_grid(ui, local.len(), |app, ui, i| {
                    let s = &local[i];
                    let uri = file_uri(s.thumb.as_ref().unwrap_or(&s.path));
                    let resp = ui.add(
                        egui::Image::new(uri).fit_to_exact_size(TILE).sense(egui::Sense::click()),
                    );
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr!("Открыть в полном размере", "Open full size"));
                    if resp.clicked() {
                        open_file(&s.path);
                    }
                    resp.context_menu(|ui| {
                        if ui.button(tr!("Открыть", "Open")).clicked() {
                            open_file(&s.path);
                            ui.close_menu();
                        }
                        if ui.button(tr!("Показать в папке", "Show in folder")).clicked() {
                            show_in_folder(&s.path);
                            ui.close_menu();
                        }
                    });
                    let name = s.game.clone().unwrap_or_else(|| app.game_name(s.appid, net));
                    let from = if s.game.is_some() { tr!(" · скачан", " · downloaded") } else { "" };
                    ui.add(egui::Label::new(egui::RichText::new(format!("{name} · {}{from}", crate::fmt_day(s.time))).small()).truncate());
                });
            }
        } else {
            match (&net.cloud_shots, &cloud_all) {
                (_, Some((total, list))) => {
                    if list.is_empty() {
                        ui.weak(if uid != 0 {
                            tr!("У друга нет скриншотов, которые он показывает вам.", "The friend has no screenshots shown to you.")
                        } else {
                            tr!("В облаке Steam нет загруженных скриншотов. Загрузить можно в самом Steam (менеджер скриншотов).", "The Steam cloud has no uploaded screenshots. You can upload them in Steam itself (the screenshot manager).")
                        });
                    } else {
                        let list = list.clone();
                        let status = w.status.clone();
                        let rescan = w.rescan.clone();
                        self.shots_grid(ui, list.len(), |app, ui, i| {
                            let s = &list[i];
                            let resp = ui.add(
                                egui::Image::new(if s.preview_url.is_empty() { &s.file_url } else { &s.preview_url }).fit_to_exact_size(TILE).sense(egui::Sense::click()),
                            );
                            let resp = resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if s.caption.is_empty() { tr!("Открыть в полном размере", "Open full size").to_string() } else { s.caption.clone() });
                            if resp.clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&s.file_url));
                            }
                            resp.context_menu(|ui| {
                                if ui.button(tr!("Открыть в браузере", "Open in browser")).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(&s.file_url));
                                    ui.close_menu();
                                }
                                if ui.button(tr!("Страница в Steam", "Page on Steam")).clicked() {
                                    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", s.id);
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                                    ui.close_menu();
                                }
                                if ui.button(tr!("Скачать на компьютер", "Download to the computer")).clicked() {
                                    download(s.clone(), status.clone(), rescan.clone(), ui.ctx().clone());
                                    ui.close_menu();
                                }
                                if ui.button(tr!("Комментарии и оценка", "Comments and rating")).clicked() {
                                    let owner = if uid != 0 { uid } else { net.my_id.unwrap_or(0) };
                                    app.open_shot_comments(owner, s.id, s.caption.clone(), s.preview_url.clone());
                                    ui.close_menu();
                                }
                            });
                            let name = if s.app_name.is_empty() { app.game_name(s.appid as u64, net) } else { s.app_name.clone() };
                            let vis = match s.visibility {
                                1 => tr!(" · для друзей", " · friends"),
                                2 => tr!(" · только я", " · only me"),
                                3 => tr!(" · скрыт", " · hidden"),
                                _ => "",
                            };
                            let social = match (s.votes_up, s.comments) {
                                (0, 0) => String::new(),
                                (v, 0) => trf!(" · оценок {v}", " · {v} votes"),
                                (0, c) => trf!(" · комм. {c}", " · {c} comm."),
                                (v, c) => trf!(" · оценок {v}, комм. {c}", " · {v} votes, {c} comm."),
                            };
                            ui.add(
                                egui::Label::new(egui::RichText::new(format!("{name} · {}{vis}{social}", crate::fmt_day(s.time as u64))).small())
                                    .truncate(),
                            );
                        });
                        if (list.len() as u32) < *total {
                            ui.horizontal(|ui| {
                                if ui.add_enabled(!net.cloud_shots_busy, egui::Button::new(tr!("Показать еще", "Show more"))).clicked() {
                                    let page = list.len() as u32 / 50 + 1;
                                    let _ = self.tx.send(Cmd::CloudShots { user: uid, appid: w.filter.unwrap_or(0), page });
                                }
                            });
                        }
                    }
                }
                (Some(Err(e)), _) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), trf!("Не удалось получить список: {e}", "Could not get the list: {e}"));
                }
                _ if net.account.is_none() => {
                    ui.weak(tr!("Нужен вход в аккаунт.", "An account login is needed."));
                }
                _ => {
                    crate::spin::spinner(ui);
                }
            }
        }
        self.shots_win = Some(w);
    }

    /// Grid: rows by window width, only visible ones are drawn.
    fn shots_grid(&mut self, ui: &mut egui::Ui, n: usize, mut tile: impl FnMut(&mut LauncherApp, &mut egui::Ui, usize)) {
        let cols = ((ui.available_width() + 8.0) / (TILE.x + 8.0)).floor().max(1.0) as usize;
        let rows = n.div_ceil(cols);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let i = r * cols + c;
                        if i >= n {
                            break;
                        }
                        ui.allocate_ui(egui::vec2(TILE.x, ROW_H - 6.0), |ui| {
                            ui.vertical(|ui| tile(self, ui, i));
                        });
                    }
                });
            }
        });
    }
}

/// Download a cloud screenshot to Pictures\Vaporust\<game>.
fn download(
    s: aurelia::steam_client::CloudScreenshot,
    status: Arc<Mutex<String>>,
    rescan: Arc<std::sync::atomic::AtomicBool>,
    ctx: egui::Context,
) {
    *status.lock().unwrap() = tr!("Скачиваю...", "Downloading...").into();
    std::thread::spawn(move || {
        let dir = download_dir(if s.app_name.is_empty() { "Steam" } else { &s.app_name });
        // <appid>_<id>.jpg - so the "On this PC" tab knows the game.
        let file = dir.join(format!("{}_{}.jpg", s.appid, s.id));
        // A thumbnail for the grid - into the Vaporust data folder.
        if let Some(td) = thumbs_dir() {
            if let Ok((bytes, _)) = crate::util::http_get_limited(&s.preview_url, 8 << 20, &|| true) {
                if std::fs::create_dir_all(&td).is_ok() {
                    let _ = std::fs::write(td.join(format!("{}.jpg", s.id)), &bytes);
                }
            }
        }
        let msg = match crate::util::http_get_limited(&s.file_url, 100 << 20, &|| true) {
            Ok((bytes, _)) => match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &bytes)) {
                Ok(()) => {
                    show_in_folder(&file);
                    trf!("Сохранено: {}", "Saved: {}", file.display())
                }
                Err(e) => trf!("Не удалось сохранить: {e}", "Could not save: {e}"),
            },
            Err(e) => trf!("Не удалось скачать: {e}", "Could not download: {e}"),
        };
        *status.lock().unwrap() = msg;
        rescan.store(true, std::sync::atomic::Ordering::Relaxed);
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_local_layout() {
        let root = std::env::temp_dir().join(format!("vaporust_shots_{}", std::process::id()));
        let user = root.join("userdata").join("12345");
        std::fs::create_dir_all(user.join("config")).unwrap();
        std::fs::write(user.join("config").join("localconfig.vdf"), "\"UserLocalConfigStore\"\n{\n}\n").unwrap();
        let dir = user.join("760").join("remote").join("70").join("screenshots");
        std::fs::create_dir_all(dir.join("thumbnails")).unwrap();
        std::fs::write(dir.join("20240101_1.jpg"), b"x").unwrap();
        std::fs::write(dir.join("thumbnails").join("20240101_1.jpg"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let v = super::scan_local(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].appid, 70);
        assert!(v[0].thumb.is_some());
    }
}

#[cfg(test)]
mod friend_live {
    /// Live test (read only): how many screenshots several friends show. Prints numbers only.
    #[test]
    #[ignore]
    fn friend_shots_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            let mut any = false;
            for f in friends.iter().filter(|f| f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1).take(8) {
                match c.user_screenshots(f.steam_id, 0, 1, 50).await {
                    Ok((total, v)) => {
                        any |= !v.is_empty();
                        println!("friend: total {total}, on the page {}, with an image {}", v.len(), v.iter().filter(|s| !s.preview_url.is_empty()).count());
                    }
                    Err(e) => println!("friend: error {e:#}"),
                }
            }
            assert!(any, "no friend has visible screenshots");
        });
    }
}
