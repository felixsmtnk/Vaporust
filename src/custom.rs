//! Non-Steam games: "Add a game" -> "Add a non-Steam game...", as in
//! Steam. The game is stored in Vaporust settings (Steam files are not touched), shown in
//! the library and launched directly, without Steam.

use crate::{LauncherApp, Row};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

/// Non-Steam game ids start here (like Steam shortcuts - the high bit) so they do not
/// collide with real appids.
pub const BASE: u32 = 0x8000_0000;

pub fn is_custom(appid: u32) -> bool {
    appid >= BASE
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CustomGame {
    pub id: u32,
    pub name: String,
    pub exe: String,
    pub args: String,
    /// Last launch, unix time.
    pub last_played: u64,
    /// Minutes played (counted by Vaporust while the game process runs).
    pub minutes: u32,
}

/// Window for adding or editing a non-Steam game.
#[derive(Default, Clone)]
pub struct Edit {
    /// Id of the edited game; None - a new one.
    pub id: Option<u32>,
    pub name: String,
    pub exe: String,
    pub args: String,
    pub confirm_delete: bool,
    /// Also add the new game to the real Steam.
    pub to_steam: bool,
}

/// Minutes counted by the threads of running non-Steam games: (id, minutes).
static PLAYED: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());


/// Non-Steam game files by id - for the list icon (read by the covers thread).
pub static EXES: std::sync::Mutex<Vec<(u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Path to the exe of a non-Steam game.
pub fn exe_of(id: u32) -> Option<String> {
    EXES.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, e)| e.clone())
}

/// Program icon from its exe (as in Explorer): RGBA and the square's side.
#[cfg(windows)]
pub fn exe_icon(path: &str) -> Option<(usize, Vec<u8>)> {
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut large: HICON = null_mut();
    let n = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, null_mut(), 1) };
    if n == 0 || large.is_null() {
        return None;
    }
    let mut info: ICONINFO = unsafe { zeroed() };
    if unsafe { GetIconInfo(large, &mut info) } == 0 {
        unsafe { DestroyIcon(large) };
        return None;
    }
    let cleanup = |info: &ICONINFO| unsafe {
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask);
        }
        DestroyIcon(large);
    };
    if info.hbmColor.is_null() {
        cleanup(&info);
        return None;
    }
    let mut bm: BITMAP = unsafe { zeroed() };
    unsafe { GetObjectW(info.hbmColor, size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut _) };
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 256 || w != h {
        cleanup(&info);
        return None;
    }
    let read = |bmp| -> Vec<u8> {
        let mut bi: BITMAPINFO = unsafe { zeroed() };
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h; // top to bottom
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            GetDIBits(dc, bmp, 0, h as u32, buf.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
            DeleteDC(dc);
        }
        buf
    };
    let mut px = read(info.hbmColor);
    // Old icons without alpha in the color: transparency comes from the mask (white is transparent).
    if px.chunks(4).all(|p| p[3] == 0) {
        let mask = if info.hbmMask.is_null() { Vec::new() } else { read(info.hbmMask) };
        for (i, p) in px.chunks_mut(4).enumerate() {
            p[3] = if mask.get(i * 4).is_some_and(|m| *m != 0) { 0 } else { 255 };
        }
    }
    for p in px.chunks_mut(4) {
        p.swap(0, 2); // BGRA -> RGBA
    }
    cleanup(&info);
    Some((w as usize, px))
}

#[cfg(not(windows))]
pub fn exe_icon(_path: &str) -> Option<(usize, Vec<u8>)> {
    None
}

#[cfg(all(test, windows))]
mod icon_tests {
    #[test]
    fn notepad_icon() {
        let (side, px) = super::exe_icon(r"C:\Windows\System32\notepad.exe").expect("Notepad icon");
        assert!(side >= 16);
        assert_eq!(px.len(), side * side * 4);
        assert!(px.chunks(4).any(|p| p[3] > 0), "the icon is not empty");
    }
}

impl LauncherApp {
    /// Library rows for non-Steam games.
    pub(crate) fn custom_rows(&self) -> Vec<Row> {
        {
            let mut exes = EXES.lock().unwrap();
            exes.clear();
            exes.extend(self.settings.custom_games.iter().map(|g| (g.id, g.exe.clone())));
        }
        self.settings
            .custom_games
            .iter()
            .map(|g| Row {
                appid: g.id,
                name: g.name.clone(),
                installed: true,
                ready: exe_exists(&g.exe),
                needs_update: false,
                playtime_min: Some(g.minutes),
                size: 0,
                last_played: g.last_played,
                family: false,
                owner: None,
            })
            .collect()
    }

    /// Launch a non-Steam game directly.
    pub(crate) fn launch_custom(&mut self, appid: u32) {
        if self.launching.load(Ordering::Relaxed) {
            *self.launch_status.lock().unwrap() = tr!("Сначала дождитесь закрытия запущенной игры.", "Wait until the running game closes first.").into();
            return;
        }
        let Some(g) = self.settings.custom_games.iter_mut().find(|g| g.id == appid) else { return };
        let exe = std::path::PathBuf::from(&g.exe);
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let args: Vec<String> = crate::direct::split_args(&g.args);
        match std::process::Command::new(&exe).args(&args).current_dir(&dir).spawn() {
            Ok(mut child) => {
                g.last_played = chrono::Utc::now().timestamp() as u64;
                let name = g.name.clone();
                *self.launch_status.lock().unwrap() = trf!("Запущена «{name}».", "\"{name}\" started.");
                let (status, launching) = (self.launch_status.clone(), self.launching.clone());
                launching.store(true, Ordering::Relaxed);
                // Playtime: wait for the process to exit in its own thread.
                let started = std::time::Instant::now();
                std::thread::spawn(move || {
                    {
                        // While the game runs, "Close game" is available at the bottom and on its page.
                        let _running = crate::local::RunningGuard::set(appid, &name, &dir);
                        let _ = child.wait();
                    }
                    let min = (started.elapsed().as_secs() / 60) as u32;
                    if min > 0 {
                        PLAYED.lock().unwrap().push((appid, min));
                    }
                    *status.lock().unwrap() = trf!("«{name}» закрыта.", "\"{name}\" closed.");
                    launching.store(false, Ordering::Relaxed);
                });
            }
            Err(e) => {
                *self.launch_status.lock().unwrap() = trf!("Не удалось запустить «{}»: {e}", "Could not start \"{}\": {e}", g.name);
            }
        }
    }

    /// Every frame: add up non-Steam playtime, add/edit windows.
    pub(crate) fn custom_tick(&mut self, ctx: &egui::Context) {
        // Identical games (same exe) - one entry, playtime is summed.
        let mut seen: Vec<(String, u32)> = Vec::new();
        let mut merged: Vec<(u32, u32, u64)> = Vec::new();
        for g in &self.settings.custom_games {
            let key = norm_path(&g.exe);
            match seen.iter().find(|(k, _)| *k == key) {
                Some((_, first)) => merged.push((*first, g.minutes, g.last_played)),
                None => seen.push((key, g.id)),
            }
        }
        if !merged.is_empty() {
            let keep: Vec<u32> = seen.iter().map(|(_, id)| *id).collect();
            for (first, min, last) in merged {
                if let Some(g) = self.settings.custom_games.iter_mut().find(|g| g.id == first) {
                    g.minutes += min;
                    g.last_played = g.last_played.max(last);
                }
            }
            self.settings.custom_games.retain(|g| keep.contains(&g.id));
        }
        // Deferred "add to Steam" - when Steam is closed (checked at most once per 5 s).
        if !self.settings.pending_shortcuts.is_empty() && PENDING_CHECK.with(|t| t.get().is_none_or(|t| t.elapsed().as_secs() >= 5)) {
            PENDING_CHECK.with(|t| t.set(Some(std::time::Instant::now())));
            if !crate::local::steam_running() {
                for id in std::mem::take(&mut self.settings.pending_shortcuts) {
                    let msg = self.export_to_steam(id);
                    if !msg.is_empty() && !msg.contains(tr!("уже есть", "already added")) {
                        crate::popup::push(crate::popup::Kind::Info, tr!("Сторонние игры", "Non-Steam games"), msg);
                    }
                }
            }
        }
        for (id, min) in std::mem::take(&mut *PLAYED.lock().unwrap()) {
            if let Some(g) = self.settings.custom_games.iter_mut().find(|g| g.id == id) {
                g.minutes += min;
            }
        }
        let Some(mut e) = self.custom_edit.take() else { return };
        let steam_open = self.steam_running();
        let mut keep = true;
        let title = if e.id.is_some() { tr!("Сторонняя игра", "Non-Steam game") } else { tr!("Добавить стороннюю игру", "Add a non-Steam game") };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak(tr!("Любая программа или игра не из Steam. Она появится в библиотеке Vaporust и будет запускаться напрямую.", "Any program or game not from Steam. It appears in the Vaporust library and is launched directly."));
                ui.add_space(6.0);
                egui::Grid::new("custom_game").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(tr!("Файл игры", "Game file"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut e.exe).desired_width(300.0).hint_text("C:\\Games\\...\\game.exe"));
                        if ui.button(tr!("Обзор...", "Browse...")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter(tr!("Программа", "Program"), &["exe", "bat", "cmd", "lnk"]).pick_file() {
                                if e.name.trim().is_empty() {
                                    e.name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                }
                                e.exe = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr!("Название", "Name"));
                    ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(300.0));
                    ui.end_row();
                    ui.label(tr!("Параметры запуска", "Launch options"));
                    ui.add(egui::TextEdit::singleline(&mut e.args).desired_width(300.0).hint_text(tr!("необязательно", "optional")));
                    ui.end_row();
                });
                if e.id.is_none() {
                    ui.checkbox(&mut e.to_steam, tr!("Добавить и в Steam", "Add to Steam too"))
                        .on_hover_text(tr!("Игра появится и в настоящем Steam. Если Steam открыт - добавится, когда вы его закроете.", "The game also appears in the real Steam. If Steam is open, it is added when you close Steam."));
                    if e.to_steam && steam_open {
                        ui.colored_label(egui::Color32::from_rgb(230, 190, 90), tr!("Steam открыт: в Steam игра появится, когда вы его закроете.", "Steam is open: the game appears in Steam when you close it."));
                    }
                }
                ui.add_space(6.0);
                let ok = !e.name.trim().is_empty() && std::path::Path::new(e.exe.trim()).exists();
                ui.horizontal(|ui| {
                    if ui.add_enabled(ok, egui::Button::new(if e.id.is_some() { tr!("Сохранить", "Save") } else { tr!("Добавить", "Add") })).clicked() {
                        let (name, exe, args) = (e.name.trim().to_string(), e.exe.trim().to_string(), e.args.trim().to_string());
                        match e.id.and_then(|id| self.settings.custom_games.iter_mut().find(|g| g.id == id)) {
                            Some(g) => {
                                g.name = name;
                                g.exe = exe;
                                g.args = args;
                            }
                            None => {
                                let id = self.settings.custom_games.iter().map(|g| g.id + 1).max().unwrap_or(BASE).max(BASE);
                                self.settings.custom_games.push(CustomGame { id, name, exe, args, ..Default::default() });
                                self.lib_page = Some(id);
                                if e.to_steam {
                                    self.settings.pending_shortcuts.push(id);
                                }
                            }
                        }
                        keep = false;
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        keep = false;
                    }
                    if let Some(id) = e.id {
                        if steam_open {
                            ui.colored_label(egui::Color32::from_rgb(230, 190, 90), tr!("Steam открыт", "Steam is open"));
                        }
                        if ui
                            .button(tr!("Добавить в Steam", "Add to Steam"))
                            .on_hover_text(tr!("Игра появится и в настоящем Steam (как «Добавить стороннюю игру» в Steam). Steam должен быть закрыт.", "The game also appears in the real Steam (like \"Add a Non-Steam Game\" in Steam). Steam must be closed."))
                            .clicked()
                        {
                            // Steam is open - remember: it will be added when Steam is closed.
                            let msg = if steam_open {
                                if !self.settings.pending_shortcuts.contains(&id) {
                                    self.settings.pending_shortcuts.push(id);
                                }
                                tr!("Игра появится в Steam, когда вы его закроете.", "The game appears in Steam when you close it.").to_string()
                            } else {
                                self.export_to_steam(id)
                            };
                            if !msg.is_empty() {
                                crate::popup::push(crate::popup::Kind::Info, tr!("Сторонние игры", "Non-Steam games"), msg);
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !e.confirm_delete {
                                if ui.button(tr!("Убрать из библиотеки", "Remove from library")).clicked() {
                                    e.confirm_delete = true;
                                }
                            } else if ui.button(tr!("Точно убрать? Файлы игры не удаляются", "Really remove? The game files are not deleted")).clicked() {
                                self.settings.custom_games.retain(|g| g.id != id);
                                if self.lib_page == Some(id) {
                                    self.lib_page = None;
                                }
                                keep = false;
                            }
                        });
                    }
                });
                if !e.exe.trim().is_empty() && !std::path::Path::new(e.exe.trim()).exists() {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), tr!("Такого файла нет.", "No such file."));
                }
            });
        if keep {
            self.custom_edit = Some(e);
        }
    }

    /// Import non-Steam games from the real Steam (shortcuts.vdf) into the Vaporust library.
    pub(crate) fn import_steam_shortcuts(&mut self) {
        let msg = match self.steam.as_ref().and_then(|s| crate::shortcuts::path(&s.dir)) {
            None => tr!("Не найдена папка аккаунта Steam.", "Steam account folder not found.").to_string(),
            Some(p) => match crate::shortcuts::read(&p) {
                Err(e) => trf!("Не удалось прочитать ярлыки Steam: {e}", "Could not read Steam shortcuts: {e}"),
                Ok(list) => {
                    let mut added = 0;
                    for s in list {
                        let have = self.settings.custom_games.iter().any(|g| same_path(&g.exe, &s.exe));
                        if have || s.exe.is_empty() {
                            continue;
                        }
                        let id = self.settings.custom_games.iter().map(|g| g.id + 1).max().unwrap_or(BASE).max(BASE);
                        self.settings.custom_games.push(CustomGame {
                            id,
                            name: if s.name.is_empty() { s.exe.clone() } else { s.name },
                            exe: s.exe,
                            args: s.args,
                            ..Default::default()
                        });
                        added += 1;
                    }
                    if added == 0 {
                        tr!("Новых сторонних игр в Steam нет.", "No new non-Steam games in Steam.").to_string()
                    } else {
                        trf!("Из Steam добавлено сторонних игр: {added}.", "Non-Steam games added from Steam: {added}.")
                    }
                }
            },
        };
        crate::popup::push(crate::popup::Kind::Info, tr!("Сторонние игры", "Non-Steam games"), msg);
    }

    /// Add a Vaporust non-Steam game to the real Steam (only while Steam is closed).
    pub(crate) fn export_to_steam(&mut self, id: u32) -> String {
        if crate::local::steam_running() {
            return tr!("Сначала закройте Steam: иначе он перезапишет список своих сторонних игр.", "Close Steam first: otherwise it overwrites its list of non-Steam games.").into();
        }
        let Some(g) = self.settings.custom_games.iter().find(|g| g.id == id).cloned() else { return String::new() };
        match self.steam.as_ref().and_then(|s| crate::shortcuts::path(&s.dir)) {
            None => tr!("Не найдена папка аккаунта Steam.", "Steam account folder not found.").into(),
            Some(p) => match crate::shortcuts::add(&p, &g.name, &g.exe, &g.args) {
                Ok(true) => trf!("«{}» добавлена в Steam. Появится там при следующем запуске Steam.", "\"{}\" added to Steam. It appears there the next time Steam starts.", g.name),
                Ok(false) => tr!("Эта игра уже есть в Steam.", "This game is already in Steam.").into(),
                Err(e) => trf!("Не удалось: {e}", "Failed: {e}"),
            },
        }
    }

    /// Open the edit window of a non-Steam game.
    pub(crate) fn edit_custom(&mut self, appid: u32) {
        if let Some(g) = self.settings.custom_games.iter().find(|g| g.id == appid) {
            self.custom_edit = Some(Edit { id: Some(g.id), name: g.name.clone(), exe: g.exe.clone(), args: g.args.clone(), confirm_delete: false, to_steam: false });
        }
    }
}

thread_local! {
    static PENDING_CHECK: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Exe path for comparison: no quotes, backslashes, case-insensitive.
fn norm_path(p: &str) -> String {
    p.trim().trim_matches('"').replace('/', "\\").to_lowercase()
}

fn same_path(a: &str, b: &str) -> bool {
    norm_path(a) == norm_path(b)
}

/// Whether the exe is on disk, remembered for a minute: on a disconnected network drive the check
/// itself hangs for seconds, and the library list is rebuilt often.
fn exe_exists(exe: &str) -> bool {
    static SEEN: std::sync::Mutex<Vec<(String, bool, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());
    if let Some(e) = SEEN.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|e| e.0 == exe) {
        if e.2.elapsed() < std::time::Duration::from_secs(60) {
            return e.1;
        }
    }
    let ok = std::path::Path::new(exe).exists();
    let mut s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    s.retain(|e| e.0 != exe);
    s.push((exe.to_string(), ok, std::time::Instant::now()));
    ok
}
