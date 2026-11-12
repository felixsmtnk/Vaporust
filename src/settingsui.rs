//! Вкладка «Настройки»: библиотека, запуск игр, окно, магазин, о программе.
//! Всё сохраняется автоматически (eframe persistence). Статус и звук сообщений —
//! в панели друзей, поэтому здесь их нет.

use crate::net::{self, Cmd, NetState};
use crate::{local, portable, settings, store, tray, LauncherApp};
use eframe::egui;

const GITHUB: &str = "https://github.com/felixsmtnk/Vaporust";
/// Ширина колонки с названиями настроек.
const LABEL_W: f32 = 210.0;

/// Карточка раздела: рамка, заголовок, содержимое.
fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .rounding(6.0)
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(16.0).strong());
            ui.add_space(8.0);
            add(ui);
        });
    ui.add_space(10.0);
}

/// Строка «название — значение»: название слева фиксированной ширины.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([LABEL_W, 20.0], egui::Label::new(label).halign(egui::Align::LEFT));
        add(ui);
    });
    ui.add_space(4.0);
}

/// Пояснение под строкой, с отступом на ширину колонки названий.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
        ui.add(egui::Label::new(egui::RichText::new(text).weak().small()).wrap());
    });
    ui.add_space(6.0);
}

impl LauncherApp {
    /// Передать в Aurelia ограничение скорости загрузки и прокси.
    pub(crate) fn send_aurelia_config(&self) {
        let proxy = Some(self.settings.proxy.trim().to_string()).filter(|p| !p.is_empty());
        let _ = self.tx.send(Cmd::AureliaConfig { limit_kbit: self.settings.download_limit_kb * 8, proxy });
    }

    pub(crate) fn settings_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            ui.set_max_width(780.0);

            // ---------- Библиотека ----------
            card(ui, "Библиотека", |ui| {
                row(ui, "Вид", |ui| {
                    ui.selectable_value(&mut self.settings.view, settings::View::Grid, "Плитка");
                    ui.selectable_value(&mut self.settings.view, settings::View::List, "Список");
                });
                note(ui, "Плитка — обложки игр, список — компактные строки.");

                row(ui, "Устанавливать новые игры в", |ui| {
                    if self.libraries.is_empty() {
                        ui.weak("папки библиотек Steam не найдены");
                        return;
                    }
                    let label = |p: &std::path::PathBuf| {
                        let free = local::free_space(p)
                            .map(|f| format!("  (свободно {})", net::fmt_bytes(f)))
                            .unwrap_or_default();
                        format!("{}{free}", p.display())
                    };
                    let current = self.libraries.get(self.lib_idx).map(label).unwrap_or_default();
                    egui::ComboBox::from_id_salt("settings_library").selected_text(current).show_ui(ui, |ui| {
                        for (i, p) in self.libraries.iter().enumerate() {
                            ui.selectable_value(&mut self.lib_idx, i, label(p));
                        }
                    });
                    if let Some(p) = self.libraries.get(self.lib_idx) {
                        if ui.button("Открыть").on_hover_text("Открыть папку в Проводнике").clicked() {
                            local::open_folder(p);
                        }
                    }
                });
                note(ui, "Новую папку библиотеки пока можно добавить только в самом Steam (Настройки -> Хранилище).");

                row(ui, "Скорость загрузки", |ui| {
                    let before = self.settings.download_limit_kb;
                    let label = |kb: u64| {
                        if kb == 0 { "без ограничения".to_string() } else { format!("до {} МБ/с", kb as f64 / 1000.0) }
                    };
                    egui::ComboBox::from_id_salt("settings_limit").selected_text(label(before)).show_ui(ui, |ui| {
                        for kb in [0u64, 500, 1000, 2000, 5000, 10000, 20000] {
                            ui.selectable_value(&mut self.settings.download_limit_kb, kb, label(kb));
                        }
                    });
                    if self.settings.download_limit_kb != before {
                        self.send_aurelia_config();
                    }
                });
                note(ui, "Для загрузок через Vaporust (установка, откат). Обновления через Steam идут по настройкам Steam.");

                row(ui, "Дубли установок", |ui| {
                    if ui.button("Найти").clicked() {
                        self.duplicates = self.steam.as_ref().map(|s| local::find_duplicates(&s.dir));
                    }
                    match &self.duplicates {
                        Some(d) if d.is_empty() => {
                            ui.weak("дублей нет");
                        }
                        Some(d) => {
                            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("найдено: {}", d.len()));
                        }
                        None => {}
                    }
                });
                if let Some(d) = self.duplicates.clone().filter(|d| !d.is_empty()) {
                    for (_, name, libs) in d {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
                            ui.label(format!("{name}:"));
                            for l in libs {
                                if ui.small_button(l.display().to_string()).on_hover_text("Открыть папку").clicked() {
                                    local::open_folder(&l.join("steamapps"));
                                }
                            }
                        });
                    }
                    note(ui, "Игра лежит в двух библиотеках сразу — Steam из-за этого может без конца её обновлять. Удалите лишнюю копию (папку steamapps\\common\\<игра> и файл appmanifest_<номер>.acf) при закрытом Steam.");
                } else {
                    note(ui, "Ищет игры, установленные сразу в нескольких библиотеках Steam.");
                }

                row(ui, "Список игр аккаунта", |ui| {
                    if ui
                        .add_enabled(net.account.is_some() && !net.busy, egui::Button::new("Обновить библиотеку"))
                        .clicked()
                    {
                        let _ = self.tx.send(Cmd::RefreshLibrary);
                    }
                    if net.busy {
                        ui.spinner();
                    }
                });
                note(ui, "Заново загрузить из Steam список купленных игр и время в игре (например, после покупки).");
            });

            // ---------- Запуск игр ----------
            card(ui, "Запуск игр", |ui| {
                row(ui, "Экономия памяти", |ui| {
                    ui.add_enabled(
                        !self.launching.load(std::sync::atomic::Ordering::Relaxed),
                        egui::Checkbox::new(&mut self.freeze_webhelper, "Замораживать steamwebhelper"),
                    );
                });
                note(
                    ui,
                    "Через 20 с после старта игры или обновления приостанавливает встроенный браузер Steam \
                     и сбрасывает его память на диск, потом размораживает.",
                );

                row(ui, "Флаги скрытого Steam", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.settings.steam_args).desired_width(330.0));
                    let is_default = self.settings.steam_args.trim() == local::DEFAULT_STEAM_ARGS;
                    if ui.add_enabled(!is_default, egui::Button::new("По умолчанию")).clicked() {
                        self.settings.steam_args = local::DEFAULT_STEAM_ARGS.to_string();
                    }
                });
                note(
                    ui,
                    "-silent — без окна; -no-browser и -cef-disable-gpu — облегчённый встроенный браузер; \
                     -nofriendsui — без окна друзей Steam. Действует со следующего запуска Steam.",
                );
            });

            // ---------- Окно ----------
            card(ui, "Окно", |ui| {
                row(ui, "Крестик окна", |ui| {
                    let mut tray_on = tray::close_to_tray();
                    if ui.checkbox(&mut tray_on, "Сворачивает Vaporust в трей").changed() {
                        tray::set_close_to_tray(tray_on);
                    }
                });
                note(ui, "Чат продолжает работать. Вернуть — щелчок по иконке у часов, выйти — правый щелчок -> «Выход».");

                row(ui, "Язык интерфейса", |ui| {
                    egui::ComboBox::from_id_salt("settings_lang").selected_text("Русский").show_ui(ui, |ui| {
                        let _ = ui.selectable_label(true, "Русский");
                        ui.add_enabled(false, egui::SelectableLabel::new(false, "English (в бете)"));
                    });
                });
            });

            // ---------- Сеть ----------
            card(ui, "Сеть", |ui| {
                row(ui, "Прокси", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.proxy)
                            .hint_text("http://хост:порт или socks5://хост:порт")
                            .desired_width(300.0),
                    );
                    if ui.button("Применить").clicked() {
                        self.send_aurelia_config();
                        self.proxy_note = true;
                    }
                });
                note(
                    ui,
                    if self.proxy_note {
                        "Сохранено. Действует после перезапуска Vaporust."
                    } else {
                        "Для запросов магазина, загрузок и сообщества. Пусто — без прокси. Подключение к самому \
                         Steam (вход, друзья, чат) через прокси не идёт."
                    },
                );
            });

            // ---------- Магазин ----------
            card(ui, "Магазин", |ui| {
                let current = self.settings.store_country.clone();
                let label = current
                    .as_deref()
                    .and_then(|cc| store::COUNTRIES.iter().find(|(c, _)| *c == cc))
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| "по стране подключения".into());
                row(ui, "Регион цен", |ui| {
                    egui::ComboBox::from_id_salt("settings_country").selected_text(label).show_ui(ui, |ui| {
                        for (cc, name) in store::COUNTRIES {
                            if ui.selectable_label(current.as_deref() == Some(cc), name).clicked()
                                && current.as_deref() != Some(cc)
                            {
                                self.settings.store_country = Some(cc.to_string());
                                let _ = self.tx.send(Cmd::Store(store::StoreCmd::Country(cc.to_string())));
                                // Списки магазина перезагрузятся с новыми ценами.
                                self.store_win = None;
                            }
                        }
                    });
                });
            });

            // ---------- О программе ----------
            card(ui, "О программе", |ui| {
                row(ui, "Версия", |ui| {
                    ui.label(concat!("Vaporust ", env!("CARGO_PKG_VERSION"), " (альфа)"));
                    ui.label(if cfg!(target_pointer_width = "64") { "64 бит" } else { "32 бит" });
                    ui.hyperlink_to("GitHub", GITHUB);
                });
                row(ui, "Обновления", |ui| {
                    ui.checkbox(&mut self.settings.check_updates, "Проверять при запуске");
                    let checking = matches!(*self.upd.lock().unwrap(), crate::updater::State::Checking);
                    if ui.add_enabled(!checking, egui::Button::new("Проверить сейчас")).clicked() {
                        self.check_updates_now(ui.ctx());
                    }
                    if checking {
                        ui.spinner();
                    }
                });
                note(ui, "Новые версии берутся со страницы Releases на GitHub; установка — одной кнопкой с перезапуском.");
                row(ui, "Папки", |ui| {
                    if let Some(s) = &self.steam {
                        if ui.button("Steam").clicked() {
                            local::open_folder(&s.dir);
                        }
                    }
                    let dir = if portable::enabled() { portable::data_dir() } else { portable::normal_dir() };
                    if let Some(dir) = dir {
                        if ui.button("Данные Vaporust").on_hover_text(dir.display().to_string()).clicked() {
                            local::open_folder(&dir);
                        }
                    }
                });
                row(ui, "Хранение данных", |ui| {
                    if portable::enabled() {
                        ui.label("портативно (рядом с программой)");
                        if ui.button("Выключить").clicked() {
                            self.portable_note = Some(portable::disable().map(|_| String::new()));
                        }
                    } else {
                        ui.label("в профиле Windows");
                        if ui.button("Портативный режим").clicked() {
                            self.portable_note = Some(portable::enable().map(|_| String::new()));
                        }
                    }
                });
                note(
                    ui,
                    "Портативный режим: настройки и вход хранятся в папке VaporustData рядом с vaporust.exe — \
                     удобно для флешки. Действует после перезапуска.",
                );
                match &self.portable_note {
                    Some(Ok(_)) => {
                        let mut restart = false;
                        ui.horizontal(|ui| {
                            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
                            ui.colored_label(egui::Color32::from_rgb(110, 200, 110), "Готово. Нужен перезапуск.");
                            restart = ui.button("Перезапустить Vaporust").clicked();
                        });
                        if restart {
                            match portable::restart() {
                                Ok(()) => {
                                    tray::request_exit();
                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                Err(e) => self.portable_note = Some(Err(e)),
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let e = e.clone();
                        note(ui, &e);
                    }
                    None => {}
                }
                ui.add_space(4.0);
                ui.weak("Неофициальный проект, не связан с Valve. Лицензия GPL-3.0-or-later. Основан на Aurelia (Drackrath).");
            });
        });
    }
}
