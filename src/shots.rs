//! Скриншоты Steam: «На этом ПК» (папка Steam userdata\<аккаунт>\760\remote\<игра>\screenshots)
//! и «В облаке Steam» (загруженные скриншоты - PublishedFile.GetUserFiles, фильтр 4).
//! Плитка уменьшенных копий (у локальных - thumbnails Steam), щелчок - полный размер,
//! правый щелчок - показать в папке / страница в Steam / скачать на компьютер.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TILE: egui::Vec2 = egui::vec2(220.0, 124.0);
const ROW_H: f32 = 124.0 + 24.0;

/// Скриншот на этом ПК.
#[derive(Clone)]
pub struct LocalShot {
    pub appid: u64,
    pub path: PathBuf,
    pub thumb: Option<PathBuf>,
    /// Время изменения файла (unix).
    pub time: u64,
    /// Скачан из облака Vaporust (ИзображенияVaporust<игра>): название игры - по имени папки.
    pub game: Option<String>,
}

pub struct ShotsWin {
    /// 0 - на этом ПК, 1 - в облаке Steam.
    pub tab: u8,
    /// Только эта игра (None - все).
    pub filter: Option<u32>,
    local: Option<Vec<LocalShot>>,
    /// Для какого фильтра уже просили облачные: (чьи, игра) - чтобы не просить каждый кадр.
    cloud_asked: Option<(u64, u32)>,
    /// Скриншоты друга (SteamID64, имя); None - свои.
    pub user: Option<(u64, String)>,
    /// Ход скачивания на компьютер.
    status: Arc<Mutex<String>>,
    /// Скачивание закончилось - перечитать «На этом ПК».
    rescan: Arc<std::sync::atomic::AtomicBool>,
    /// Игры, встречавшиеся в облачных скриншотах (для фильтра).
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

/// Папка пользователя Steam (userdata\<аккаунт>) - того, чей localconfig.vdf свежее.
fn user_dir(steam_dir: &Path) -> Option<PathBuf> {
    crate::launchopts::find_localconfig(steam_dir)?.parent()?.parent().map(Path::to_path_buf)
}

/// Все скриншоты на этом ПК, новые первыми.
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

/// Уменьшенные копии скачанных скриншотов (в папке данных Vaporust, не в «Изображениях»).
fn thumbs_dir() -> Option<PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join("shot_thumbs"))
}

/// Скриншоты, скачанные из облака в ИзображенияVaporust<игра><appid>_<id>.jpg.
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

/// Папка для скачанных скриншотов: Изображения\Vaporust\<игра>.
fn download_dir(game: &str) -> PathBuf {
    let safe: String = game.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let pics = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Pictures");
    pics.join("Vaporust").join(safe.trim())
}

/// Открыть файл программой по умолчанию (просмотр фотографий).
fn open_file(p: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
}

/// Показать файл в Проводнике (выделенным).
fn show_in_folder(p: &Path) {
    // Проводник понимает только /select,"путь" - с кавычками вокруг пути, а не всего аргумента,
    // поэтому собираем строку сами (иначе путь с пробелами открывает «Документы»).
    let mut cmd = std::process::Command::new("explorer.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/select,\"{}\"", p.display()));
    }
    let _ = cmd.spawn();
}

impl LauncherApp {
    /// Открыть «Скриншоты» (с фильтром по игре, если задан).
    pub(crate) fn open_shots(&mut self, filter: Option<u32>) {
        self.close_extras(crate::View::Screenshots);
        // Были открыты скриншоты друга - начинаем заново со своих.
        if self.shots_win.as_ref().is_some_and(|w| w.user.is_some()) {
            self.shots_win = None;
        }
        let w = self.shots_win.get_or_insert_with(|| ShotsWin::new(filter));
        w.filter = filter;
        self.view = crate::View::Screenshots;
    }

    /// Скриншоты друга (те, что он показывает вам).
    pub(crate) fn open_friend_shots(&mut self, id: u64, name: String) {
        self.close_extras(crate::View::Screenshots);
        let mut w = ShotsWin::new(None);
        w.user = Some((id, name));
        w.tab = 1;
        self.shots_win = Some(w);
        self.view = crate::View::Screenshots;
    }

    /// Название игры по appid: библиотека, затем кэш названий.
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
        format!("Игра {appid}")
    }

    pub(crate) fn shots_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(mut w) = self.shots_win.take() else { return };
        if w.rescan.swap(false, std::sync::atomic::Ordering::Relaxed) {
            w.local = None;
        }
        if w.local.is_none() {
            // Папка скриншотов Steam + скачанные Vaporust из облака; новые первыми.
            let mut v = self.steam.as_ref().map(|s| scan_local(&s.dir)).unwrap_or_default();
            v.extend(scan_downloads());
            v.sort_by(|a, b| b.time.cmp(&a.time));
            w.local = Some(v);
        }
        // Игры из облачных скриншотов - в фильтр (запоминаем, пока открыт раздел).
        if let Some(Ok((_, _, list))) = &net.cloud_shots {
            for s in list {
                let name = if s.app_name.is_empty() { self.game_name(s.appid as u64, net) } else { s.app_name.clone() };
                w.cloud_games.entry(s.appid).or_insert(name);
            }
        }
        let uid = w.user.as_ref().map_or(0, |u| u.0);
        if uid != 0 {
            w.tab = 1; // у друга - только облако
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
                ui.heading(format!("Скриншоты: {name}"));
                if let Some((total, _)) = &cloud_all {
                    ui.weak(format!("({total})"));
                }
            } else {
                ui.heading("Скриншоты");
                ui.separator();
                ui.selectable_value(&mut w.tab, 0, format!("На этом ПК ({})", local.len()));
                let cloud_label = match &cloud_all {
                    Some((total, _)) => format!("В облаке Steam ({total})"),
                    None => "В облаке Steam".to_string(),
                };
                ui.selectable_value(&mut w.tab, 1, cloud_label);
            }
            ui.separator();
            // Фильтр по игре: игры, у которых есть скриншоты на ПК, плюс текущий фильтр.
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
                .unwrap_or_else(|| "Все игры".into());
            egui::ComboBox::from_id_salt("shots_game").selected_text(format!("Игра: {current}")).show_ui(ui, |ui| {
                ui.selectable_value(&mut w.filter, None, "Все игры");
                for (a, n) in &games {
                    ui.selectable_value(&mut w.filter, Some(*a), n);
                }
            });
            if w.tab == 0 && ui.button("Обновить").on_hover_text("Перечитать папку скриншотов").clicked() {
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
                    "Скриншотов на этом ПК нет. Steam сохраняет их по клавише F12 в игре (папка Steam\\userdata\\…\\760). \
                     Загруженные в облако - на вкладке «В облаке Steam»; скачанные оттуда появятся и здесь.",
                );
            } else {
                self.shots_grid(ui, local.len(), |app, ui, i| {
                    let s = &local[i];
                    let uri = file_uri(s.thumb.as_ref().unwrap_or(&s.path));
                    let resp = ui.add(
                        egui::Image::new(uri).fit_to_exact_size(TILE).sense(egui::Sense::click()),
                    );
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Открыть в полном размере");
                    if resp.clicked() {
                        open_file(&s.path);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Открыть").clicked() {
                            open_file(&s.path);
                            ui.close_menu();
                        }
                        if ui.button("Показать в папке").clicked() {
                            show_in_folder(&s.path);
                            ui.close_menu();
                        }
                    });
                    let name = s.game.clone().unwrap_or_else(|| app.game_name(s.appid, net));
                    let from = if s.game.is_some() { " · скачан" } else { "" };
                    ui.add(egui::Label::new(egui::RichText::new(format!("{name} · {}{from}", crate::fmt_day(s.time))).small()).truncate());
                });
            }
        } else {
            match (&net.cloud_shots, &cloud_all) {
                (_, Some((total, list))) => {
                    if list.is_empty() {
                        ui.weak(if uid != 0 {
                            "У друга нет скриншотов, которые он показывает вам."
                        } else {
                            "В облаке Steam нет загруженных скриншотов. Загрузить можно в самом Steam (менеджер скриншотов)."
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
                                .on_hover_text(if s.caption.is_empty() { "Открыть в полном размере".to_string() } else { s.caption.clone() });
                            if resp.clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&s.file_url));
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Открыть в браузере").clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(&s.file_url));
                                    ui.close_menu();
                                }
                                if ui.button("Страница в Steam").clicked() {
                                    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", s.id);
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                                    ui.close_menu();
                                }
                                if ui.button("Скачать на компьютер").clicked() {
                                    download(s.clone(), status.clone(), rescan.clone(), ui.ctx().clone());
                                    ui.close_menu();
                                }
                                if ui.button("Комментарии и оценка").clicked() {
                                    let owner = if uid != 0 { uid } else { net.my_id.unwrap_or(0) };
                                    app.open_shot_comments(owner, s.id, s.caption.clone(), s.preview_url.clone());
                                    ui.close_menu();
                                }
                            });
                            let name = if s.app_name.is_empty() { app.game_name(s.appid as u64, net) } else { s.app_name.clone() };
                            let vis = match s.visibility {
                                1 => " · для друзей",
                                2 => " · только я",
                                3 => " · скрыт",
                                _ => "",
                            };
                            let social = match (s.votes_up, s.comments) {
                                (0, 0) => String::new(),
                                (v, 0) => format!(" · оценок {v}"),
                                (0, c) => format!(" · комм. {c}"),
                                (v, c) => format!(" · оценок {v}, комм. {c}"),
                            };
                            ui.add(
                                egui::Label::new(egui::RichText::new(format!("{name} · {}{vis}{social}", crate::fmt_day(s.time as u64))).small())
                                    .truncate(),
                            );
                        });
                        if (list.len() as u32) < *total {
                            ui.horizontal(|ui| {
                                if ui.add_enabled(!net.cloud_shots_busy, egui::Button::new("Показать еще")).clicked() {
                                    let page = list.len() as u32 / 50 + 1;
                                    let _ = self.tx.send(Cmd::CloudShots { user: uid, appid: w.filter.unwrap_or(0), page });
                                }
                            });
                        }
                    }
                }
                (Some(Err(e)), _) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), format!("Не удалось получить список: {e}"));
                }
                _ if net.account.is_none() => {
                    ui.weak("Нужен вход в аккаунт.");
                }
                _ => {
                    crate::spin::spinner(ui);
                }
            }
        }
        self.shots_win = Some(w);
    }

    /// Плитка: строки по ширине окна, рисуются только видимые.
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

/// Скачать облачный скриншот в Изображения\Vaporust\<игра>.
fn download(
    s: aurelia::steam_client::CloudScreenshot,
    status: Arc<Mutex<String>>,
    rescan: Arc<std::sync::atomic::AtomicBool>,
    ctx: egui::Context,
) {
    *status.lock().unwrap() = "Скачиваю...".into();
    std::thread::spawn(move || {
        let dir = download_dir(if s.app_name.is_empty() { "Steam" } else { &s.app_name });
        // <appid>_<id>.jpg - чтобы на вкладке «На этом ПК» знать игру.
        let file = dir.join(format!("{}_{}.jpg", s.appid, s.id));
        // Уменьшенная копия для плитки - в папку данных Vaporust.
        if let Some(td) = thumbs_dir() {
            if let Ok(r) = ehttp::fetch_blocking(&ehttp::Request::get(&s.preview_url)) {
                if r.ok && std::fs::create_dir_all(&td).is_ok() {
                    let _ = std::fs::write(td.join(format!("{}.jpg", s.id)), &r.bytes);
                }
            }
        }
        let msg = match ehttp::fetch_blocking(&ehttp::Request::get(&s.file_url)) {
            Ok(r) if r.ok => match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &r.bytes)) {
                Ok(()) => {
                    show_in_folder(&file);
                    format!("Сохранено: {}", file.display())
                }
                Err(e) => format!("Не удалось сохранить: {e}"),
            },
            Ok(r) => format!("Не удалось скачать: {} {}", r.status, r.status_text),
            Err(e) => format!("Не удалось скачать: {e}"),
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
    /// Живой тест (только чтение): сколько скриншотов показывают несколько друзей. Печатает только числа.
    #[test]
    #[ignore]
    fn friend_shots_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            let mut any = false;
            for f in friends.iter().filter(|f| f.relationship == 3).take(8) {
                match c.user_screenshots(f.steam_id, 0, 1, 50).await {
                    Ok((total, v)) => {
                        any |= !v.is_empty();
                        println!("друг: всего {total}, на странице {}, с картинкой {}", v.len(), v.iter().filter(|s| !s.preview_url.is_empty()).count());
                    }
                    Err(e) => println!("друг: ошибка {e:#}"),
                }
            }
            assert!(any, "ни у кого из друзей нет видимых скриншотов");
        });
    }
}
