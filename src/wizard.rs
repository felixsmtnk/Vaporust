//! First-run wizard: a few steps with the main settings (the same ones can be changed
//! later in "Settings"). Shown once; "Skip" also counts.

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
        egui::Window::new(tr!("Добро пожаловать в Vaporust", "Welcome to Vaporust"))
            .id(egui::Id::new("wizard"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .fixed_size(egui::vec2(460.0, 300.0))
            .show(ctx, |ui| {
                ui.weak(trf!("Шаг {} из {STEPS}", "Step {} of {STEPS}", step + 1));
                ui.add_space(6.0);
                match step {
                    0 => {
                        ui.label(egui::RichText::new(tr!("Легкий клиент Steam", "A light Steam client")).size(17.0).strong());
                        ui.add_space(4.0);
                        ui.label(
                            tr!("Vaporust показывает библиотеку, магазин, друзей и чат без тяжелого интерфейса Steam. \
                             Игры запускаются через настоящий Steam, поднятый незаметно, и он закрывается вместе с игрой.", "Vaporust shows the library, store, friends and chat without the heavy Steam interface. Games are launched through the real Steam, started unnoticed, and it closes together with the game."),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                tr!("Неофициальный проект, не связан с Valve. Программа меняет файлы Steam; используйте на свой страх и риск.", "An unofficial project, not affiliated with Valve. The program changes Steam files; use it at your own risk."),
                            )
                            .color(egui::Color32::from_rgb(230, 190, 90)),
                        );
                        ui.add_space(6.0);
                        // Interface language: both names in their own language.
                        ui.horizontal(|ui| {
                            ui.label("Язык / Language:");
                            let en = crate::lang::en();
                            if ui.selectable_label(!en, "Русский").clicked() {
                                self.settings.language = "ru".into();
                                crate::lang::set_en(false);
                            }
                            if ui.selectable_label(en, "English").clicked() {
                                self.settings.language = "en".into();
                                crate::lang::set_en(true);
                            }
                        });
                        ui.add_space(6.0);
                        // How to work: with an account login or without one (no Steam password is entered).
                        let before = self.settings.local_mode;
                        ui.radio_value(&mut self.settings.local_mode, false, tr!("Войти в аккаунт Steam: все возможности", "Log in to a Steam account: all features"));
                        ui.radio_value(&mut self.settings.local_mode, true, tr!("Без входа: только установленные игры, пароль не нужен", "Without login: installed games only, no password needed"));
                        if self.settings.local_mode != before {
                            if self.settings.local_mode {
                                let mut s = self.net.lock().unwrap();
                                s.need_login = false;
                                s.message.clear();
                            } else {
                                let _ = self.tx.send(crate::net::Cmd::Restore);
                            }
                        }
                        ui.add_space(6.0);
                        ui.weak(tr!("Дальше - несколько настроек. Все их можно поменять потом: меню «Vaporust» -> «Настройки».", "Next - a few settings. All of them can be changed later: the \"Vaporust\" menu -> \"Settings\"."));
                    }
                    1 => {
                        ui.label(egui::RichText::new(tr!("Вид", "View")).size(17.0).strong());
                        ui.add_space(6.0);
                        egui::Grid::new("wiz_view").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui.label(tr!("Библиотека", "Library"));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.settings.view, settings::View::Grid, tr!("Плитка (обложки)", "Grid (covers)"));
                                ui.radio_value(&mut self.settings.view, settings::View::List, tr!("Таблица (компактно)", "Table (compact)"));
                            });
                            ui.end_row();
                            ui.label(tr!("Масштаб", "Scale"));
                            ui.horizontal(|ui| {
                                for (v, l) in [(0.9f32, "90%"), (1.0, "100%"), (1.1, "110%"), (1.25, "125%")] {
                                    if ui.selectable_label((self.settings.ui_scale - v).abs() < 0.01, l).clicked() {
                                        self.settings.ui_scale = v;
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label(tr!("При запуске", "At startup"));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.settings.start_view, 0, tr!("Библиотека", "Library"));
                                ui.radio_value(&mut self.settings.start_view, 1, tr!("Магазин", "Store"));
                            });
                            ui.end_row();
                        });
                    }
                    2 => {
                        ui.label(egui::RichText::new(tr!("Окно и уведомления", "Window and notifications")).size(17.0).strong());
                        ui.add_space(6.0);
                        let mut tray_on = tray::close_to_tray();
                        if ui.checkbox(&mut tray_on, tr!("Крестик сворачивает Vaporust в трей (чат и загрузки работают)", "The close button minimizes Vaporust to the tray (chat and downloads keep working)")).changed() {
                            tray::set_close_to_tray(tray_on);
                        }
                        ui.checkbox(&mut self.settings.notify_on, tr!("Всплывающие уведомления (сообщения, друзья, скидки)", "Pop-up notifications (messages, friends, discounts)"));
                        ui.add_enabled(self.settings.notify_on, egui::Checkbox::new(&mut self.settings.notify_sound, tr!("Со звуком", "With sound")));
                    }
                    _ => {
                        ui.label(egui::RichText::new("Steam").size(17.0).strong());
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut self.freeze_webhelper,
                            tr!("Во время игры замораживать интерфейс Steam (steamwebhelper) - меньше памяти", "Freeze the Steam interface (steamwebhelper) during games - less memory"),
                        );
                        ui.add_space(4.0);
                        ui.label(tr!("Обновление игр:", "Game updates:"));
                        ui.radio_value(&mut self.settings.update_via_steam, false, tr!("Без Steam (быстрее, Steam не запускается)", "Without Steam (faster, Steam is not started)"));
                        ui.radio_value(&mut self.settings.update_via_steam, true, tr!("Через Steam (как в обычном клиенте)", "Via Steam (as in the normal client)"));
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if step > 0 && ui.button(tr!("< Назад", "< Back")).clicked() {
                        self.wizard_step = step - 1;
                    }
                    if step + 1 < STEPS {
                        if ui.button(tr!("Далее >", "Next >")).clicked() {
                            self.wizard_step = step + 1;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.link(tr!("Пропустить", "Skip")).clicked() {
                                done = true;
                            }
                        });
                    } else if ui.button(tr!("Готово", "Done")).clicked() {
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
