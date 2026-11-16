//! "Storage": how much space games take in each Steam library and what is left
//! of uninstalled games (folders without an install record, unfinished downloads, shader cache
//! and Workshop mods of uninstalled games). Leftovers go to the Recycle Bin and only while Steam is closed.

use crate::{local, net, LauncherApp};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What is left on disk without a game.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// steamapps\common\<folder> that no appmanifest refers to.
    Folder,
    /// steamapps\downloading\<appid> of a game that is not downloading.
    Download,
    /// steamapps\shadercache\<appid> of an uninstalled game.
    Shaders,
    /// steamapps\workshop\content\<appid> (and appworkshop_<appid>.acf) of an uninstalled game.
    Workshop,
}

impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Folder => "Папка игры без установки",
            Kind::Download => "Незаконченная загрузка",
            Kind::Shaders => "Кэш шейдеров удаленной игры",
            Kind::Workshop => "Моды Мастерской удаленной игры",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Leftover {
    pub kind: Kind,
    pub path: PathBuf,
    /// One more file deleted together with the folder (appworkshop_<appid>.acf).
    pub extra: Option<PathBuf>,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct GameSize {
    pub appid: u32,
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct LibInfo {
    pub path: PathBuf,
    pub free: Option<u64>,
    pub total: Option<u64>,
    /// Games by size, descending.
    pub games: Vec<GameSize>,
    pub leftovers: Vec<Leftover>,
}

/// Size of the whole folder.
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Free and total space on the disk where the folder is.
#[cfg(windows)]
pub fn disk_space(dir: &Path) -> (Option<u64>, Option<u64>) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    if ok != 0 {
        (Some(free), Some(total))
    } else {
        (None, None)
    }
}

#[cfg(not(windows))]
pub fn disk_space(_dir: &Path) -> (Option<u64>, Option<u64>) {
    (None, None)
}

/// Library appmanifest records (and those hidden by Vaporust): appid -> (name, installdir, StateFlags, SizeOnDisk).
fn manifests(lib: &Path) -> HashMap<u32, (String, String, u32, u64)> {
    let mut out = HashMap::new();
    let Ok(rd) = fs::read_dir(lib.join("steamapps")) else { return out };
    for e in rd.flatten() {
        let f = e.file_name().to_string_lossy().to_string();
        if !(f.starts_with("appmanifest_") && (f.ends_with(".acf") || f.ends_with(local::STASH_EXT))) {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path()) else { continue };
        let (mut appid, mut name, mut dir, mut flags, mut size) = (None, String::new(), String::new(), 0, 0);
        for (k, v) in text.lines().filter_map(local::kv_pub) {
            match k.as_str() {
                "appid" if appid.is_none() => appid = v.parse::<u32>().ok(),
                "name" if name.is_empty() => name = v,
                "installdir" if dir.is_empty() => dir = v,
                "StateFlags" => flags = v.parse().unwrap_or(0),
                "SizeOnDisk" => size = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        if let Some(a) = appid {
            out.insert(a, (name, dir, flags, size));
        }
    }
    out
}

/// Steam folders in common that do not belong to games directly - not touched.
const KEEP: [&str; 2] = ["steamworks shared", "steam controller configs"];

/// Inspect all libraries. `busy` - games that are downloading now or waiting in the queue
/// (their unfinished downloads are not leftovers).
pub fn scan(steam_dir: &Path, busy: &HashSet<u32>) -> Vec<LibInfo> {
    let libs = local::library_dirs(steam_dir);
    let all: Vec<HashMap<u32, (String, String, u32, u64)>> = libs.iter().map(|l| manifests(l)).collect();
    let installed: HashSet<u32> = all.iter().flat_map(|m| m.keys().copied()).collect();
    let mut out = Vec::new();
    for (lib, mans) in libs.iter().zip(&all) {
        let apps = lib.join("steamapps");
        if !apps.is_dir() {
            continue;
        }
        let (free, total) = disk_space(lib);
        let mut games: Vec<GameSize> = mans
            .iter()
            .filter(|(a, _)| **a != 228980)
            .map(|(a, (name, _, _, size))| GameSize { appid: *a, name: name.clone(), size: *size })
            .collect();
        games.sort_by(|a, b| b.size.cmp(&a.size));
        let mut leftovers = Vec::new();
        // common folders without an appmanifest.
        let dirs: HashSet<String> = mans.values().map(|(_, d, _, _)| d.to_lowercase()).collect();
        if let Ok(rd) = fs::read_dir(apps.join("common")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if e.path().is_dir() && !dirs.contains(&n) && !KEEP.contains(&n.as_str()) {
                    leftovers.push(Leftover { kind: Kind::Folder, size: 0, path: e.path(), extra: None });
                }
            }
        }
        // Unfinished downloads of games that are neither updating nor downloading.
        if let Ok(rd) = fs::read_dir(apps.join("downloading")) {
            for e in rd.flatten() {
                let Ok(a) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
                let waiting = mans.get(&a).is_some_and(|m| m.2 != 4);
                if e.path().is_dir() && !waiting && !busy.contains(&a) {
                    leftovers.push(Leftover { kind: Kind::Download, size: 0, path: e.path(), extra: None });
                }
            }
        }
        // Shader cache and Workshop mods of games that are in no library.
        if let Ok(rd) = fs::read_dir(apps.join("shadercache")) {
            for e in rd.flatten() {
                let Ok(a) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
                if e.path().is_dir() && !installed.contains(&a) && !busy.contains(&a) {
                    leftovers.push(Leftover { kind: Kind::Shaders, size: 0, path: e.path(), extra: None });
                }
            }
        }
        if let Ok(rd) = fs::read_dir(apps.join("workshop").join("content")) {
            for e in rd.flatten() {
                let Ok(a) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
                if e.path().is_dir() && !installed.contains(&a) && !busy.contains(&a) {
                    let acf = apps.join("workshop").join(format!("appworkshop_{a}.acf"));
                    leftovers.push(Leftover {
                        kind: Kind::Workshop,
                        size: 0,
                        path: e.path(),
                        extra: acf.exists().then_some(acf),
                    });
                }
            }
        }
        for l in &mut leftovers {
            l.size = dir_size(&l.path);
        }
        // Empty caches (0 bytes) are not shown - nothing to clean there.
        leftovers.retain(|l| l.size > 0 || l.kind == Kind::Folder);
        leftovers.sort_by(|a, b| b.size.cmp(&a.size));
        out.push(LibInfo { path: lib.clone(), free, total, games, leftovers });
    }
    out
}

/// Send to the Recycle Bin (recoverable). Ok(()) - deleted.
#[cfg(windows)]
pub fn to_recycle_bin(paths: &[&Path]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
    };
    // A list of paths separated by \0, with one more \0 at the end.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16;
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 || op.fAnyOperationsAborted != 0 {
        return Err(format!("Windows не удалил (код {r})."));
    }
    if paths.iter().any(|p| p.exists()) {
        return Err("Часть файлов осталась на месте (заняты другой программой?).".into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn to_recycle_bin(_paths: &[&Path]) -> Result<(), String> {
    Err("Только для Windows.".into())
}

/// "Storage" tab.
pub struct StorageWin {
    pub result: Arc<Mutex<Option<Vec<LibInfo>>>>,
    /// Which leftover deletion is being confirmed (path).
    pub confirm: Option<PathBuf>,
    pub note: Option<String>,
    /// How many games to show in each library.
    pub show_all: HashSet<PathBuf>,
}

impl LauncherApp {
    /// Open "Storage" (and recount).
    pub(crate) fn open_storage(&mut self) {
        self.close_extras(crate::View::Storage);
        let Some(steam) = self.steam.clone() else { return };
        let busy: HashSet<u32> = self
            .settings
            .downloads
            .iter()
            .map(|q| q.appid)
            .chain(self.settings.holding.iter().copied())
            .chain(self.settings.current_download.as_ref().map(|q| q.appid))
            .collect();
        let result = Arc::new(Mutex::new(None));
        let (r, ctx) = (result.clone(), self.egui_ctx.clone());
        std::thread::spawn(move || {
            let v = scan(&steam.dir, &busy);
            *r.lock().unwrap() = Some(v);
            ctx.request_repaint();
        });
        let note = self.storage_win.take().and_then(|w| w.note);
        self.storage_win = Some(StorageWin { result, confirm: None, note, show_all: HashSet::new() });
        self.view = crate::View::Storage;
    }

    pub(crate) fn storage_contents(&mut self, ui: &mut egui::Ui) {
        let steam_open = self.steam_running();
        let Some(w) = self.storage_win.as_mut() else { return };
        let data = w.result.lock().unwrap().clone();
        let mut rescan = false;
        let mut open_game: Option<u32> = None;
        ui.horizontal(|ui| {
            ui.heading("Хранилище");
            if ui.button("Пересчитать").clicked() {
                rescan = true;
            }
        });
        if let Some(n) = &w.note {
            ui.label(n);
        }
        let Some(libs) = data else {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.label("Считаю, сколько занимают игры...");
            });
            return;
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for lib in &libs {
                ui.add_space(8.0);
                egui::Frame::group(ui.style()).rounding(6.0).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.strong(crate::local::pretty_path(&lib.path));
                    let games_total: u64 = lib.games.iter().map(|g| g.size).sum();
                    let left_total: u64 = lib.leftovers.iter().map(|l| l.size).sum();
                    if let (Some(free), Some(total)) = (lib.free, lib.total) {
                        let used = total.saturating_sub(free);
                        ui.add(
                            egui::ProgressBar::new(used as f32 / total.max(1) as f32)
                                .desired_width(ui.available_width().min(560.0))
                                .text(format!(
                                    "занято {} из {}, свободно {}",
                                    net::fmt_bytes(used),
                                    net::fmt_bytes(total),
                                    net::fmt_bytes(free)
                                )),
                        );
                    }
                    ui.label(format!(
                        "Игры Steam: {} ({} шт.){}",
                        net::fmt_bytes(games_total),
                        lib.games.len(),
                        if left_total > 0 { format!(", остатки: {}", net::fmt_bytes(left_total)) } else { String::new() }
                    ));
                    // Games by size.
                    let all = w.show_all.contains(&lib.path);
                    let n = if all { lib.games.len() } else { lib.games.len().min(8) };
                    egui::Grid::new(("storage_games", &lib.path)).num_columns(3).striped(true).show(ui, |ui| {
                        for g in &lib.games[..n] {
                            if ui.link(&g.name).on_hover_text("Страница игры в библиотеке").clicked() {
                                open_game = Some(g.appid);
                            }
                            ui.label(net::fmt_bytes(g.size));
                            let share = g.size as f32 / games_total.max(1) as f32;
                            ui.add(egui::ProgressBar::new(share).desired_width(120.0));
                            ui.end_row();
                        }
                    });
                    if lib.games.len() > 8 {
                        let t = if all { "Свернуть".to_string() } else { format!("Показать все ({})", lib.games.len()) };
                        if ui.small_button(t).clicked() {
                            if all {
                                w.show_all.remove(&lib.path);
                            } else {
                                w.show_all.insert(lib.path.clone());
                            }
                        }
                    }
                    // Leftovers of uninstalled games.
                    if !lib.leftovers.is_empty() {
                        ui.add_space(8.0);
                        ui.strong("Остатки, которые можно удалить");
                        if steam_open {
                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "Удаление - при закрытом Steam.");
                        }
                        // As a table: what, folder, size, buttons - in even columns.
                        egui::Grid::new(("leftovers", lib.path.to_string_lossy().to_string())).num_columns(4).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        for l in &lib.leftovers {
                            {
                                let name = l.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                ui.weak(l.kind.label());
                                ui.label(&name);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(net::fmt_bytes(l.size)));
                                ui.horizontal(|ui| {
                                if ui.small_button("Открыть").clicked() {
                                    local::open_folder(&l.path);
                                }
                                if w.confirm.as_ref() == Some(&l.path) {
                                    ui.label("В Корзину?");
                                    if ui.add_enabled(!steam_open, egui::Button::new("Да")).clicked() {
                                        let mut paths: Vec<&Path> = vec![&l.path];
                                        if let Some(x) = &l.extra {
                                            paths.push(x);
                                        }
                                        w.note = Some(match to_recycle_bin(&paths) {
                                            Ok(()) => format!("«{name}» в Корзине ({}).", net::fmt_bytes(l.size)),
                                            Err(e) => format!("«{name}» не удален: {e}"),
                                        });
                                        w.confirm = None;
                                        rescan = true;
                                    }
                                    if ui.button("Нет").clicked() {
                                        w.confirm = None;
                                    }
                                } else if ui
                                    .add_enabled(!steam_open, egui::Button::new("Удалить..."))
                                    .on_hover_text("В Корзину Windows: если нужно, можно восстановить оттуда.")
                                    .clicked()
                                {
                                    w.confirm = Some(l.path.clone());
                                }
                                });
                                ui.end_row();
                            }
                        }
                        });
                        ui.weak(
                            "Папка без установки: игра удалена или перенесена, а файлы остались (часто там \
                             сохранения или моды - загляните перед удалением кнопкой «Открыть»).",
                        );
                    }
                });
            }
        });
        if rescan {
            self.open_storage();
        }
        if let Some(a) = open_game {
            self.lib_page = Some(a);
            self.view = crate::View::Library;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_leftovers() {
        let root = std::env::temp_dir().join(format!("vaporust_storage_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let apps = root.join("steamapps");
        for d in ["common/Game", "common/Old Game", "common/Steamworks Shared", "downloading/70", "downloading/99",
            "shadercache/70", "shadercache/555", "workshop/content/555"]
        {
            fs::create_dir_all(apps.join(d)).unwrap();
        }
        for d in ["common/Old Game", "downloading/70", "shadercache/555", "workshop/content/555"] {
            fs::write(apps.join(d).join("f.bin"), [0u8; 100]).unwrap();
        }
        fs::write(apps.join("downloading/99/f.bin"), [0u8; 10]).unwrap();
        fs::write(apps.join("shadercache/70/f.bin"), [0u8; 10]).unwrap();
        fs::write(
            apps.join("appmanifest_70.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"name\"\t\t\"HL\"\n\t\"installdir\"\t\t\"Game\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"SizeOnDisk\"\t\t\"5\"\n}\n",
        )
        .unwrap();
        let busy: HashSet<u32> = [99].into();
        let libs = scan(&root, &busy);
        assert_eq!(libs.len(), 1);
        let l = &libs[0];
        assert_eq!(l.games.len(), 1);
        let kinds: Vec<(Kind, String)> = l
            .leftovers
            .iter()
            .map(|x| (x.kind.clone(), x.path.file_name().unwrap().to_string_lossy().to_string()))
            .collect();
        // Old Game - a folder without an install; 70 in downloading - the game is ready, no download;
        // 99 - downloading (busy); shaders 70 - the game is installed; 555 - an uninstalled game.
        assert!(kinds.contains(&(Kind::Folder, "Old Game".into())));
        assert!(kinds.contains(&(Kind::Download, "70".into())));
        assert!(kinds.contains(&(Kind::Shaders, "555".into())));
        assert!(kinds.contains(&(Kind::Workshop, "555".into())));
        assert_eq!(kinds.len(), 4, "{kinds:?}");
        assert_eq!(l.leftovers.iter().find(|x| x.kind == Kind::Folder).unwrap().size, 100);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore]
    fn recycle_live() {
        let d = std::env::temp_dir().join(format!("vaporust_recycle_{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("x.txt"), "x").unwrap();
        to_recycle_bin(&[&d]).unwrap();
        assert!(!d.exists());
    }

    /// Live test (read only): what is found on this PC.
    #[test]
    #[ignore]
    fn storage_live() {
        let steam = local::find_steam().unwrap();
        for l in scan(&steam.dir, &HashSet::new()) {
            println!("{} | свободно {:?} | игр {}", l.path.display(), l.free.map(net::fmt_bytes), l.games.len());
            for x in &l.leftovers {
                println!("   {:?} {} {}", x.kind, x.path.display(), net::fmt_bytes(x.size));
            }
        }
    }
}
