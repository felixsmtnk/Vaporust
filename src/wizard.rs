//! Мастер первого запуска: несколько шагов с главными настройками (их же можно поменять
//! потом в «Настройках»). Показывается один раз; «Пропустить» тоже считается.

use crate::{settings, tray, LauncherApp};
use eframe::egui;

const STEPS: usize = 4;

impl LauncherApp {
    pub(crate) fn wizard(&mut self, ctx: &egui::Context) {
        if self.settings.wizard_done || crate::notify::in_tray() || self.tour.is_some() {
            return;
        }
        let mut done = false;
        let step = self.wizard_step.min(STEPS - 1);
        egui::Window::new("Добро пожаловать в Vaporust")
            .id(egui::Id::new("wizard"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .fixed_size(egui::vec2(460.0, 260.0))
            .show(ctx, |ui| {
                ui.weak(format!("Шаг {} из {STEPS}", step + 1));
                ui.add_space(6.0);
                match step {
                    0 => {
                        ui.label(egui::RichText::new("Легкий клиент Steam").size(17.0).strong());
                        ui.add_space(4.0);
                        ui.label(
                            "Vaporust показывает библиотеку, магазин, друзей и чат без тяжелого интерфейса Steam. \
                             Игры запускаются через настоящий Steam, поднятый незаметно, и он закрывается вместе с игрой.",
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "Неофициальный проект, не связан с Valve. Программа меняет файлы Steam; используйте на свой страх и риск.",
                            )
                            .color(egui::Color32::from_rgb(230, 190, 90)),
                        );
                        ui.add_space(6.0);
                        ui.weak("Дальше - несколько настроек. Все их можно поменять потом: меню «Vaporust» -> «Настройки».");
                    }
                    1 => {
                        ui.label(egui::RichText::new("Вид").size(17.0).strong());
                        ui.add_space(6.0);
                        egui::Grid::new("wiz_view").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui.label("Библиотека");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.settings.view, settings::View::Grid, "Плитка (обложки)");
                                ui.radio_value(&mut self.settings.view, settings::View::List, "Таблица (компактно)");
                            });
                            ui.end_row();
                            ui.label("Масштаб");
                            ui.horizontal(|ui| {
                                for (v, l) in [(0.9f32, "90%"), (1.0, "100%"), (1.1, "110%"), (1.25, "125%")] {
                                    if ui.selectable_label((self.settings.ui_scale - v).abs() < 0.01, l).clicked() {
                                        self.settings.ui_scale = v;
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label("При запуске");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.settings.start_view, 0, "Библиотека");
                                ui.radio_value(&mut self.settings.start_view, 1, "Магазин");
                            });
                            ui.end_row();
                        });
                    }
                    2 => {
                        ui.label(egui::RichText::new("Окно и уведомления").size(17.0).strong());
                        ui.add_space(6.0);
                        let mut tray_on = tray::close_to_tray();
                        if ui.checkbox(&mut tray_on, "Крестик сворачивает Vaporust в трей (чат и загрузки работают)").changed() {
                            tray::set_close_to_tray(tray_on);
                        }
                        ui.checkbox(&mut self.settings.notify_on, "Всплывающие уведомления (сообщения, друзья, скидки)");
                        ui.add_enabled(self.settings.notify_on, egui::Checkbox::new(&mut self.settings.notify_sound, "Со звуком"));
                    }
                    _ => {
                        ui.label(egui::RichText::new("Steam").size(17.0).strong());
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut self.settings.freeze_webhelper,
                            "Во время игры замораживать интерфейс Steam (steamwebhelper) - меньше памяти",
                        );
                        ui.add_space(4.0);
                        ui.label("Обновление игр:");
                        ui.radio_value(&mut self.settings.update_via_steam, false, "Без Steam (быстрее, Steam не запускается)");
                        ui.radio_value(&mut self.settings.update_via_steam, true, "Через Steam (как в обычном клиенте)");
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if step > 0 && ui.button("< Назад").clicked() {
                        self.wizard_step = step - 1;
                    }
                    if step + 1 < STEPS {
                        if ui.button("Далее >").clicked() {
                            self.wizard_step = step + 1;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.link("Пропустить").clicked() {
                                done = true;
                            }
                        });
                    } else if ui.button("Готово").clicked() {
                        done = true;
                    }
                });
            });
        if done {
            self.settings.wizard_done = true;
            if self.settings.start_view == 1 && self.view == crate::View::Library {
                self.open_store(None, false);
            }
        }
    }
}
