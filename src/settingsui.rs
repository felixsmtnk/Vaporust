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
    pub(crate) fn send_aurelia_config(&mut self) {
        let proxy = Some(self.settings.proxy.trim().to_string()).filter(|p| !p.is_empty());
        // С учётом расписания: в его часы может действовать своя скорость.
        let kb = crate::schedule::effective_limit_kb(&self.settings);
        self.sent_limit = Some(kb);
        let _ = self.tx.send(Cmd::AureliaConfig { limit_kbit: kb * 8, proxy });
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

                let mut apply = false;
                row(ui, "Ограничить скорость до", |ui| {
                    apply |= limit_field(ui, &mut self.limit_text, &mut self.settings.download_limit_kb);
                });
                note(
                    ui,
                    "Пусто или 0 — без ограничения. Действует сразу, даже на идущую загрузку. Только для загрузок \
                     через Vaporust (установка, откат); Steam при обновлении игр качает по своим настройкам.",
                );

                // ---------- Расписание загрузок ----------
                row(ui, "Расписание загрузок", |ui| {
                    ui.label("с");
                    apply |= hour_box(ui, "sched_from", &mut self.settings.sched_from);
                    ui.label("до");
                    apply |= hour_box(ui, "sched_to", &mut self.settings.sched_to);
                    if crate::schedule::window_now(&self.settings) {
                        ui.weak("(сейчас эти часы)");
                    }
                });
                row(ui, "", |ui| {
                    ui.checkbox(&mut self.settings.sched_only, "Качать только в эти часы");
                });
                row(ui, "", |ui| {
                    apply |= ui.checkbox(&mut self.settings.sched_limit_on, "В эти часы скорость до").changed();
                    ui.add_enabled_ui(self.settings.sched_limit_on, |ui| {
                        apply |= limit_field(ui, &mut self.sched_text, &mut self.settings.sched_limit_kb);
                    });
                });
                note(
                    ui,
                    "Например, с 01:00 до 07:00. «Качать только в эти часы»: в другое время очередь ждёт, а начатая \
                     загрузка встаёт на паузу и сама продолжится в начале этих часов. «Скорость»: например, ночью \
                     без ограничения (пусто), а днём — по полю выше. Vaporust должен быть запущен (можно в трее). \
                     Если «с» и «до» совпадают — круглые сутки.",
                );
                if apply {
                    self.send_aurelia_config();
                }

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
                    note(
                        ui,
                        "Если на ПК несколько папок библиотек Steam (например, на дисках C: и D:), одна и та же игра \
                         могла оказаться установленной дважды и занимать место. Кнопка ищет такие копии; лишнюю \
                         можно удалить. Если библиотека одна, дублей не бывает.",
                    );
                }

                row(ui, "Семейная библиотека", |ui| {
                    ui.checkbox(&mut self.settings.show_family, "Показывать игры семьи в «Все игры»");
                    let n = net.family.len();
                    if n > 0 {
                        ui.weak(format!("доступно игр: {n}"));
                    }
                });
                note(
                    ui,
                    "Игры, которыми с вами делятся члены семьи Steam. Их можно установить и запустить, но только \
                     пока в них не играет кто-то другой из семьи. Отдельно их можно открыть пунктом «Семейная \
                     библиотека» в списке коллекций.",
                );

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
                row(ui, "Обновление игр", |ui| {
                    ui.radio_value(&mut self.settings.update_via_steam, false, "Без Steam");
                    ui.radio_value(&mut self.settings.update_via_steam, true, "Через Steam");
                });
                note(
                    ui,
                    "Без Steam — Vaporust сам скачивает обновления, файлы другого языка или беты (выбранных в \
                     «Свойствах»); Steam не запускается — легче для старого ПК. Настройки игры сохраняются. \
                     Через Steam — как раньше: скрытый Steam обновит игру и закроется.",
                );

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

            // ---------- Уведомления ----------
            card(ui, "Уведомления", |ui| {
                row(ui, "Уведомления", |ui| {
                    ui.checkbox(&mut self.settings.notify_on, "Включены");
                    ui.add_enabled(self.settings.notify_on, egui::Checkbox::new(&mut self.settings.notify_sound, "Со звуком"));
                    if ui.small_button("Проверить").on_hover_text("Показать пробное уведомление").clicked() {
                        crate::popup::push(crate::popup::Kind::Info, "Vaporust", "Так будут выглядеть уведомления.");
                    }
                });
                note(
                    ui,
                    "Небольшие карточки в правом нижнем углу экрана на 6 секунд (пока над карточкой мышь — не \
                     исчезают). Щелчок открывает чат с другом или страницу игры, крестик — закрыть. У каждого вида \
                     свой звук. Работают и когда Vaporust свёрнут в трей.",
                );
                let s = &mut self.settings;
                ui.add_enabled_ui(s.notify_on, |ui| {
                    row(ui, "Сообщения от друзей", |ui| {
                        ui.checkbox(&mut s.notify_msg, "Новое сообщение");
                    });
                    row(ui, "Друзья", |ui| {
                        ui.checkbox(&mut s.notify_friend_game, "Друг зашёл в игру");
                        ui.checkbox(&mut s.notify_friend_online, "Друг появился в сети");
                    });
                    row(ui, "Магазин", |ui| {
                        ui.checkbox(&mut s.notify_wishlist, "Скидка на игру из списка желаемого");
                    });
                    row(ui, "Загрузки", |ui| {
                        ui.checkbox(&mut s.notify_download, "Игра скачана");
                    });
                });
                note(
                    ui,
                    "О сообщении — если не открыт чат именно с этим другом. Скидки проверяются при входе и раз в 6 часов, о каждой скидке — один раз.",
                );
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
                ui.add_space(4.0);
                ui.weak("Неофициальный проект, не связан с Valve. Лицензия GPL-3.0-or-later. Основан на Aurelia (Drackrath).");
            });
        });
    }
}

/// КБ/с -> «1.5» (МБ/с, без лишних нулей).
fn fmt_mb(kb: u64) -> String {
    let s = format!("{:.2}", kb as f64 / 1000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Текст поля (МБ/с, допускается запятая) -> КБ/с. Пусто = 0. None — не число.
fn parse_mb(text: &str) -> Option<u64> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() {
        return Some(0);
    }
    let v: f64 = t.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some((v * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mb_field() {
        assert_eq!(super::parse_mb(""), Some(0));
        assert_eq!(super::parse_mb("1,5"), Some(1500));
        assert_eq!(super::parse_mb(" 0.25 "), Some(250));
        assert_eq!(super::parse_mb("abc"), None);
        assert_eq!(super::parse_mb("-1"), None);
        assert_eq!(super::fmt_mb(1000), "1");
        assert_eq!(super::fmt_mb(1500), "1.5");
        assert_eq!(super::fmt_mb(250), "0.25");
    }
}

/// Поле «__ МБ/с» + «Применить». Возвращает true, когда значение применено.
fn limit_field(ui: &mut egui::Ui, buf: &mut Option<String>, value: &mut u64) -> bool {
    let kb = *value;
    let text = buf.get_or_insert_with(|| if kb == 0 { String::new() } else { fmt_mb(kb) });
    let resp = ui.add(egui::TextEdit::singleline(text).desired_width(70.0).hint_text("нет"));
    ui.label("МБ/с");
    let parsed = parse_mb(text);
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let changed = parsed.is_some_and(|v| v != kb);
    let mut applied = false;
    if ui.add_enabled(changed, egui::Button::new("Применить")).clicked() || (enter && changed) {
        *value = parsed.unwrap_or(0);
        applied = true;
    }
    if parsed.is_none() {
        ui.colored_label(egui::Color32::from_rgb(240, 120, 100), "введите число, например 2 или 0.5");
    } else if *value == 0 {
        ui.weak("сейчас без ограничения");
    } else {
        ui.weak(format!("сейчас до {} МБ/с", fmt_mb(*value)));
    }
    applied
}

/// Выбор часа «01:00». Возвращает true при изменении.
fn hour_box(ui: &mut egui::Ui, id: &str, h: &mut u8) -> bool {
    let before = *h;
    egui::ComboBox::from_id_salt(id).width(64.0).selected_text(crate::schedule::hh(*h)).show_ui(ui, |ui| {
        for x in 0..24u8 {
            ui.selectable_value(h, x, crate::schedule::hh(x));
        }
    });
    *h != before
}
