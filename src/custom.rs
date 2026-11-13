//! Сторонние игры (не из Steam): кнопка «Добавить игру» -> «Добавить стороннюю игру...», как в
//! Steam. Игра хранится в настройках Vaporust (Steam-файлы не трогаем), показывается в
//! библиотеке и запускается напрямую, без Steam.

use crate::{LauncherApp, Row};
use eframe::egui;
use serde::{Deserialize, Serialize};

/// Номера сторонних игр начинаются отсюда (как у ярлыков в Steam - старший бит), чтобы не
/// пересекаться с настоящими appid.
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
    /// Последний запуск, unix-время.
    pub last_played: u64,
    /// Сколько минут сыграно (считает Vaporust, пока процесс игры работает).
    pub minutes: u32,
}

/// Окно добавления или правки сторонней игры.
#[derive(Default, Clone)]
pub struct Edit {
    /// Номер правимой игры; None - новая.
    pub id: Option<u32>,
    pub name: String,
    pub exe: String,
    pub args: String,
    pub confirm_delete: bool,
}

/// Минуты, досчитанные потоками запущенных сторонних игр: (номер, минут).
static PLAYED: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

impl LauncherApp {
    /// Строки библиотеки для сторонних игр.
    pub(crate) fn custom_rows(&self) -> Vec<Row> {
        self.settings
            .custom_games
            .iter()
            .map(|g| Row {
                appid: g.id,
                name: g.name.clone(),
                installed: true,
                ready: std::path::Path::new(&g.exe).exists(),
                needs_update: false,
                playtime_min: Some(g.minutes),
                size: 0,
                last_played: g.last_played,
                family: false,
                owner: None,
            })
            .collect()
    }

    /// Запустить стороннюю игру напрямую.
    pub(crate) fn launch_custom(&mut self, appid: u32) {
        let Some(g) = self.settings.custom_games.iter_mut().find(|g| g.id == appid) else { return };
        let exe = std::path::PathBuf::from(&g.exe);
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let args: Vec<String> = crate::direct::split_args(&g.args);
        match std::process::Command::new(&exe).args(&args).current_dir(&dir).spawn() {
            Ok(mut child) => {
                g.last_played = chrono::Utc::now().timestamp() as u64;
                *self.launch_status.lock().unwrap() = format!("Запущена «{}».", g.name);
                // Время в игре: ждем завершения процесса в своем потоке.
                let started = std::time::Instant::now();
                std::thread::spawn(move || {
                    let _ = child.wait();
                    let min = (started.elapsed().as_secs() / 60) as u32;
                    if min > 0 {
                        PLAYED.lock().unwrap().push((appid, min));
                    }
                });
            }
            Err(e) => {
                *self.launch_status.lock().unwrap() = format!("Не удалось запустить «{}»: {e}", g.name);
            }
        }
    }

    /// Каждый кадр: досчитать время сторонних игр, окна добавления и правки.
    pub(crate) fn custom_tick(&mut self, ctx: &egui::Context) {
        for (id, min) in std::mem::take(&mut *PLAYED.lock().unwrap()) {
            if let Some(g) = self.settings.custom_games.iter_mut().find(|g| g.id == id) {
                g.minutes += min;
            }
        }
        let Some(mut e) = self.custom_edit.take() else { return };
        let mut keep = true;
        let title = if e.id.is_some() { "Сторонняя игра" } else { "Добавить стороннюю игру" };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.weak("Любая программа или игра не из Steam. Она появится в библиотеке Vaporust и будет запускаться напрямую.");
                ui.add_space(6.0);
                egui::Grid::new("custom_game").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Файл игры");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut e.exe).desired_width(300.0).hint_text("C:\\Games\\...\\game.exe"));
                        if ui.button("Обзор...").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Программа", &["exe", "bat", "cmd", "lnk"]).pick_file() {
                                if e.name.trim().is_empty() {
                                    e.name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                }
                                e.exe = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Название");
                    ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(300.0));
                    ui.end_row();
                    ui.label("Параметры запуска");
                    ui.add(egui::TextEdit::singleline(&mut e.args).desired_width(300.0).hint_text("необязательно"));
                    ui.end_row();
                });
                ui.add_space(6.0);
                let ok = !e.name.trim().is_empty() && std::path::Path::new(e.exe.trim()).exists();
                ui.horizontal(|ui| {
                    if ui.add_enabled(ok, egui::Button::new(if e.id.is_some() { "Сохранить" } else { "Добавить" })).clicked() {
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
                            }
                        }
                        keep = false;
                    }
                    if ui.button("Отмена").clicked() {
                        keep = false;
                    }
                    if let Some(id) = e.id {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !e.confirm_delete {
                                if ui.button("Убрать из библиотеки").clicked() {
                                    e.confirm_delete = true;
                                }
                            } else if ui.button("Точно убрать? Файлы игры не удаляются").clicked() {
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
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), "Такого файла нет.");
                }
            });
        if keep {
            self.custom_edit = Some(e);
        }
    }

    /// Открыть окно правки сторонней игры.
    pub(crate) fn edit_custom(&mut self, appid: u32) {
        if let Some(g) = self.settings.custom_games.iter().find(|g| g.id == appid) {
            self.custom_edit = Some(Edit { id: Some(g.id), name: g.name.clone(), exe: g.exe.clone(), args: g.args.clone(), confirm_delete: false });
        }
    }
}
