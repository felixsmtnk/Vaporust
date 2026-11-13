//! Вкладка «Настройки»: библиотека, запуск игр, окно, магазин, о программе.
//! Все сохраняется автоматически (eframe persistence). Статус и звук сообщений -
//! в панели друзей, поэтому здесь их нет.

use crate::net::{self, Cmd, NetState};
use crate::{local, portable, settings, tray, LauncherApp};
use eframe::egui;

const GITHUB: &str = "https://github.com/felixsmtnk/Vaporust";
/// Ширина колонки с названиями настроек.
const LABEL_W: f32 = 200.0;

/// Разделы настроек (слева), по порядку.
pub const SECTIONS: [&str; 9] = ["Аккаунт", "Библиотека", "Загрузки", "Запуск игр", "Окно", "Производительность", "Уведомления", "Сеть", "О программе"];

thread_local! {
    /// Какой раздел сейчас открыт и какие заголовки уже нарисованы в этом кадре.
    static CURRENT: std::cell::RefCell<(String, Vec<String>)> = const { std::cell::RefCell::new((String::new(), Vec::new())) };
}

/// Часть раздела `title`: рисуется, только если этот раздел выбран слева. Заголовок - один
/// раз, даже если раздел собран из нескольких частей.
fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let (show, first) = CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        let show = c.0 == title;
        let first = show && !c.1.iter().any(|t| t == title);
        if first {
            c.1.push(title.to_string());
        }
        (show, first)
    });
    if !show {
        return;
    }
    if first {
        ui.label(egui::RichText::new(title).size(20.0).strong());
        ui.add_space(12.0);
    } else {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
    }
    add(ui);
    ui.add_space(8.0);
}

/// Строка «название - значение».
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([LABEL_W, 22.0], egui::Label::new(egui::RichText::new(label).strong()).halign(egui::Align::LEFT));
        add(ui);
    });
    ui.add_space(6.0);
}

/// Пояснение под строкой, с отступом на ширину колонки названий.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
        // Отдельная колонка: в горизонтальной строке длинный текст не переносится и обрезается.
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(text).weak().small()).wrap());
        });
    });
    ui.add_space(10.0);
}

impl LauncherApp {
    /// Передать в Aurelia ограничение скорости загрузки и прокси.
    pub(crate) fn send_aurelia_config(&mut self) {
        let proxy = Some(self.settings.proxy.trim().to_string()).filter(|p| !p.is_empty());
        // С учетом расписания: в его часы может действовать своя скорость.
        let kb = crate::schedule::effective_limit_kb(&self.settings);
        self.sent_limit = Some(kb);
        let _ = self.tx.send(Cmd::AureliaConfig { limit_kbit: kb * 8, proxy });
    }

    pub(crate) fn settings_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Слева - разделы (как в настройках Steam), справа - выбранный раздел.
        let current = SECTIONS.get(self.settings_section as usize).copied().unwrap_or(SECTIONS[0]);
        CURRENT.with(|c| *c.borrow_mut() = (current.to_string(), Vec::new()));
        egui::SidePanel::left("settings_sections").resizable(false).exact_width(170.0).show_inside(ui, |ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Настройки").size(20.0).strong());
            ui.add_space(10.0);
            for (i, s) in SECTIONS.iter().enumerate() {
                let sel = self.settings_section as usize == i;
                let text = if sel { egui::RichText::new(*s).strong() } else { egui::RichText::new(*s) };
                if ui.add_sized([150.0, 26.0], egui::SelectableLabel::new(sel, text)).clicked() {
                    self.settings_section = i as u8;
                }
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(8.0);
            // Не шире видимой области: иначе пояснения переносятся по 820 px и обрезаются справа.
            ui.set_max_width(ui.available_width().min(820.0));
            if current == "Аккаунт" && net.account.is_none() {
                ui.label(egui::RichText::new("Аккаунт").size(20.0).strong());
                ui.weak("Войдите в аккаунт Steam, чтобы увидеть сведения о нем.");
            }

            // ---------- Аккаунт ----------
            if let (Some(login), Some(me)) = (net.account.clone(), net.my_id) {
                if !self.account_asked {
                    self.account_asked = true;
                    let _ = self.tx.send(Cmd::AccountInfo);
                    if !net.profiles.contains_key(&me) {
                        let _ = self.tx.send(Cmd::Social(crate::social::SocialCmd::Profile(me)));
                    }
                    let has_wallet = net.inv.lock().unwrap().wallet.as_ref().is_some_and(|w| w.is_ok());
                    if !has_wallet {
                        let _ = self.tx.send(Cmd::Inv(crate::invent::InvCmd::Wallet));
                    }
                }
                let profile = net.profiles.get(&me).and_then(|p| p.as_ref().ok());
                card(ui, "Аккаунт", |ui| {
                    row(ui, "Логин", |ui| {
                        ui.label(&login);
                    });
                    if let Some(name) = profile.and_then(|p| p.persona_name.as_ref()) {
                        row(ui, "Имя в Steam", |ui| {
                            ui.label(name);
                        });
                    }
                    row(ui, "SteamID", |ui| {
                        ui.label(me.to_string());
                        if ui.small_button("Копировать").clicked() {
                            ui.ctx().copy_text(me.to_string());
                        }
                    });
                    let code = me.saturating_sub(crate::social::STEAMID64_BASE);
                    row(ui, "Код друга", |ui| {
                        ui.label(code.to_string());
                        if ui.small_button("Копировать").clicked() {
                            ui.ctx().copy_text(code.to_string());
                        }
                    });
                    if let Some(c) = profile.and_then(|p| p.created) {
                        row(ui, "В Steam с", |ui| {
                            ui.label(crate::socialwin::fmt_date(c));
                        });
                    }
                    if profile.is_some_and(|p| p.is_limited) {
                        row(ui, "Ограничения", |ui| {
                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "ограниченный аккаунт")
                                .on_hover_text("Steam ограничивает аккаунты, на которых потрачено меньше 5 $: нельзя добавлять друзей, торговать и т.п.");
                        });
                    }
                    if let Some(d) = &net.account_data {
                        if !d.country.is_empty() {
                            row(ui, "Страна по подключению", |ui| {
                                ui.label(&d.country);
                            });
                        }
                    }
                    let wallet = net.inv.lock().unwrap().wallet.clone();
                    row(ui, "Кошелек Steam", |ui| match &wallet {
                        Some(Ok(w)) => {
                            ui.label(crate::invent::money(w.balance_cents, w.currency));
                        }
                        Some(Err(e)) => {
                            ui.weak("не удалось узнать").on_hover_text(e);
                        }
                        None => {
                            crate::spin::spinner(ui);
                        }
                    });
                    row(ui, "Игр", |ui| {
                        let own = net.owned.len();
                        let fam = net.family.iter().filter(|f| !net.owned.iter().any(|o| o.app_id == f.app_id)).count();
                        let mut t = format!("{own} на аккаунте");
                        if fam > 0 {
                            t.push_str(&format!(", еще {fam} в семейной библиотеке"));
                        }
                        t.push_str(&format!(", установлено {}", self.games.len()));
                        ui.label(t);
                    });
                    row(ui, "Где выполнен вход", |ui| {
                        if ui.button(if self.sessions_open { "Скрыть" } else { "Показать" }).clicked() {
                            self.sessions_open = !self.sessions_open;
                            if self.sessions_open {
                                let _ = self.tx.send(Cmd::Sessions);
                            }
                        }
                        if ui.small_button("Управление на сайте").on_hover_text("Выйти на устройстве - на сайте Steam").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/account/authorizeddevices"));
                        }
                    });
                    if self.sessions_open {
                        match &net.sessions {
                            None => {
                                crate::spin::spinner(ui);
                            }
                            Some(Err(e)) => {
                                ui.weak(format!("Не удалось узнать: {e}"));
                            }
                            Some(Ok(list)) => {
                                egui::Grid::new("sessions").num_columns(4).striped(true).show(ui, |ui| {
                                    for s in list {
                                        let first = if s.first_time > 0 { format!("Впервые: {}", crate::socialwin::fmt_date(s.first_time)) } else { String::new() };
                                        ui.label(if s.logged_in { format!("{} (вход активен)", s.name) } else { s.name.clone() }).on_hover_text(first);
                                        ui.weak(&s.kind);
                                        ui.weak(if s.last_time > 0 { crate::socialwin::fmt_date(s.last_time) } else { String::new() });
                                        ui.weak(&s.last_place);
                                        ui.end_row();
                                    }
                                });
                                note(ui, "Незнакомое устройство - смените пароль и выйдите на всех устройствах на сайте Steam.");
                            }
                        }
                    }
                    row(ui, "", |ui| {
                        if ui.button("Профиль").clicked() {
                            self.open_profile(me, net);
                        }
                        if ui.button("Аккаунт на сайте Steam").on_hover_text("Почта, пароль, Steam Guard, история покупок").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/account/"));
                        }
                        if ui.button("Обновить").clicked() {
                            self.account_asked = false;
                            net.inv.lock().unwrap().wallet = None;
                        }
                    });
                });
            }

            // ---------- Библиотека ----------
            card(ui, "Библиотека", |ui| {
                row(ui, "Вид", |ui| {
                    ui.selectable_value(&mut self.settings.view, settings::View::Grid, "Плитка");
                    ui.selectable_value(&mut self.settings.view, settings::View::List, "Список");
                });
                note(ui, "Плитка - обложки игр, список - компактные строки.");

                row(ui, "Кнопка случайной игры", |ui| {
                    ui.checkbox(&mut self.settings.random_button, "Показывать «Во что поиграть?» над библиотекой");
                });

                row(ui, "Устанавливать новые игры в", |ui| {
                    if self.libraries.is_empty() {
                        ui.weak("папки библиотек Steam не найдены");
                        return;
                    }
                    let label = |p: &std::path::PathBuf| {
                        let free = local::free_space(p)
                            .map(|f| format!("  (свободно {})", net::fmt_bytes(f)))
                            .unwrap_or_default();
                        format!("{}{free}", crate::local::pretty_path(p))
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
            });

            // ---------- Загрузки ----------
            card(ui, "Загрузки", |ui| {
                let mut apply = false;
                row(ui, "Ограничить скорость до", |ui| {
                    apply |= limit_field(ui, &mut self.limit_text, &mut self.settings.download_limit_kb);
                });
                note(
                    ui,
                    "Пусто или 0 - без ограничения. Действует сразу, даже на идущую загрузку. Только для загрузок \
                     через Vaporust (установка, откат); Steam при обновлении игр качает по своим настройкам.",
                );

                row(ui, "Прерванные загрузки", |ui| {
                    ui.checkbox(&mut self.settings.resume_downloads, "Продолжать сами");
                });
                note(
                    ui,
                    "Если Vaporust закрыли посреди загрузки, при следующем запуске она продолжится с того же \
                     места (без галочки - встанет в очередь на паузе). Загрузка, оборванная пропажей связи, \
                     продолжится, когда Steam снова будет доступен.",
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
                    "Например, с 01:00 до 07:00. «Качать только в эти часы»: в другое время очередь ждет, а начатая \
                     загрузка встает на паузу и сама продолжится в начале этих часов. «Скорость»: например, ночью \
                     без ограничения (пусто), а днем - по полю выше. Vaporust должен быть запущен (можно в трее). \
                     Если «с» и «до» совпадают - круглые сутки.",
                );
                if apply {
                    self.send_aurelia_config();
                }
            });

            // ---------- Библиотека (продолжение) ----------
            card(ui, "Библиотека", |ui| {
                row(ui, "Список игр", |ui| {
                    if ui.button("Сохранить в таблицу (CSV)").on_hover_text("Все игры: установлена ли, размер, часы, коллекции - для Excel").clicked() {
                        if let Some(m) = self.export_library(net) {
                            self.export_note = Some(m);
                        }
                    }
                });
                if let Some(m) = &self.export_note {
                    note(ui, m);
                }
                row(ui, "Хранилище", |ui| {
                    if ui.button("Открыть").on_hover_text("Сколько места занимают игры на каждом диске и что осталось от удаленных игр").clicked() {
                        self.open_storage();
                    }
                });
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
                                if ui.small_button(crate::local::pretty_path(&l)).on_hover_text("Открыть папку").clicked() {
                                    local::open_folder(&l.join("steamapps"));
                                }
                            }
                        });
                    }
                    note(ui, "Игра лежит в двух библиотеках сразу - Steam из-за этого может без конца ее обновлять. Удалите лишнюю копию (папку steamapps\\common\\<игра> и файл appmanifest_<номер>.acf) при закрытом Steam.");
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
                        crate::spin::spinner(ui);
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
                    "Без Steam - Vaporust сам скачивает обновления, файлы другого языка или беты (выбранных в \
                     «Свойствах»); Steam не запускается - легче для старого ПК. Настройки игры сохраняются. \
                     Через Steam - как раньше: скрытый Steam обновит игру и закроется.",
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
                    "-silent - без окна; -no-browser и -cef-disable-gpu - облегченный встроенный браузер; \
                     -nofriendsui - без окна друзей Steam. Действует со следующего запуска Steam.",
                );
            });

            // ---------- Производительность ----------
            card(ui, "Производительность", |ui| {
                row(ui, "Анимации", |ui| {
                    ui.checkbox(&mut self.settings.animations, "Стикеры, GIF, эффекты в чате, значки загрузки");
                });
                note(ui, "Без анимаций стикеры и GIF показываются первым кадром, эффекты чата сами не запускаются. Меньше работы видеокарте.");

                row(ui, "Обложки игр", |ui| {
                    ui.checkbox(&mut self.settings.covers, "Обложки, фоны и значки игр в библиотеке");
                });
                note(ui, "Без обложек библиотека - простой список, а у страницы игры нет большого фона. Так меньше всего памяти.");

                row(ui, "Экономия памяти", |ui| {
                    if ui.checkbox(&mut self.settings.economy, "Картинки мельче, кэш картинок меньше").changed() {
                        ui.ctx().forget_all_images();
                        self.covers.clear();
                    }
                });
                note(ui, "Картинки в магазине, Мастерской и профилях хранятся уменьшенными и чаще выгружаются из памяти.");

                row(ui, "Память в фоне", |ui| {
                    ui.checkbox(&mut self.settings.trim_memory, "Отдавать память системе в трее и во время игры");
                });
                note(ui, "Когда Vaporust свернут в трей или идет игра, он выгружает картинки и отдает свою память Windows: \
                     ее получит игра. При возврате в окно картинки загрузятся снова.");

                row(ui, "Сейчас", |ui| {
                    let (ws, _) = crate::stats::memory();
                    ui.label(format!(
                        "Занято {:.0} МБ (как в Диспетчере задач), из них картинки {:.0} МБ",
                        ws as f64 / 1048576.0,
                        crate::imgload::cache_bytes(ui.ctx()) as f64 / 1048576.0
                    ));
                    if ui.button("Освободить").on_hover_text("Выгрузить картинки и отдать память системе").clicked() {
                        ui.ctx().forget_all_images();
                        self.covers.clear();
                        crate::imgload::trim_working_set();
                    }
                });
                note(ui, "Часть памяти занимает сам драйвер видеокарты (на ПК с видеокартой NVIDIA до 30-40 МБ даже у пустого окна).");
            });

            // ---------- Окно ----------
            card(ui, "Окно", |ui| {
                row(ui, "Рамка окон", |ui| {
                    ui.radio_value(&mut self.settings.system_frame, false, "Своя (Vaporust)");
                    ui.radio_value(&mut self.settings.system_frame, true, "Обычная Windows");
                });
                note(ui, "Своя рамка - темный заголовок в стиле программы. Обычная - стандартная рамка Windows. Меняется сразу.");

                row(ui, "Крестик окна", |ui| {
                    let mut tray_on = tray::close_to_tray();
                    if ui.checkbox(&mut tray_on, "Сворачивает Vaporust в трей").changed() {
                        tray::set_close_to_tray(tray_on);
                    }
                });
                note(ui, "Чат продолжает работать. Вернуть - щелчок по иконке у часов, выйти - правый щелчок -> «Выход».");

                row(ui, "Автозапуск", |ui| {
                    let mut on = *self.autostart_on.get_or_insert_with(crate::autostart::enabled);
                    if ui.checkbox(&mut on, "Запускать вместе с Windows (сразу в трей)").changed() {
                        match crate::autostart::set(on) {
                            Ok(()) => self.autostart_on = Some(on),
                            Err(e) => {
                                self.autostart_on = None;
                                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                            }
                        }
                    }
                });
                note(
                    ui,
                    "Удобно для уведомлений и расписания загрузок. Если перенесете vaporust.exe в другую папку - \
                     включите галочку заново.",
                );

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
                    "Небольшие карточки в правом нижнем углу экрана на 6 секунд (пока над карточкой мышь - не \
                     исчезают). Щелчок открывает чат с другом или страницу игры, крестик - закрыть. У каждого вида \
                     свой звук. Работают и когда Vaporust свернут в трей.",
                );
                let s = &mut self.settings;
                ui.add_enabled_ui(s.notify_on, |ui| {
                    row(ui, "Сообщения от друзей", |ui| {
                        ui.checkbox(&mut s.notify_msg, "Новое сообщение");
                    });
                    row(ui, "Друзья", |ui| {
                        ui.checkbox(&mut s.notify_friend_game, "Друг зашел в игру");
                        ui.checkbox(&mut s.notify_friend_online, "Друг появился в сети");
                    });
                    row(ui, "Магазин", |ui| {
                        ui.checkbox(&mut s.notify_wishlist, "Скидка на игру из списка желаемого");
                    });
                    row(ui, "Загрузки", |ui| {
                        ui.checkbox(&mut s.notify_download, "Игра скачана");
                        ui.checkbox(&mut s.notify_steam, "Уведомления Steam (подарки, предметы, обмены)");
                    });
                });
                note(
                    ui,
                    "О сообщении - если не открыт чат именно с этим другом. Скидки проверяются при входе и раз в 6 часов, о каждой скидке - один раз.",
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
                        "Для запросов магазина, загрузок и сообщества. Пусто - без прокси. Подключение к самому \
                         Steam (вход, друзья, чат) через прокси не идет."
                    },
                );
            });

            // ---------- О программе ----------
            // (Регион цен выбирается в самом магазине.)
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
                        crate::spin::spinner(ui);
                    }
                });
                note(ui, "Новые версии берутся со страницы Releases на GitHub; установка - одной кнопкой с перезапуском.");
                row(ui, "Папки", |ui| {
                    if let Some(s) = &self.steam {
                        if ui.button("Steam").clicked() {
                            local::open_folder(&s.dir);
                        }
                    }
                    let dir = if portable::enabled() { portable::data_dir() } else { portable::normal_dir() };
                    if let Some(dir) = dir {
                        // Корневая папка (настройки, кэш, копии) и папка с данными программы.
                        if let Some(root) = dir.parent().filter(|r| r.ends_with("Vaporust")) {
                            if ui.button("Vaporust").on_hover_text(root.display().to_string()).clicked() {
                                local::open_folder(root);
                            }
                        }
                    }
                    let saves = crate::backup::root();
                    if saves.is_dir() && ui.button("Копии сохранений").on_hover_text(saves.display().to_string()).clicked() {
                        local::open_folder(&saves);
                    }
                });
                row(ui, "Ошибки", |ui| {
                    if let Some(p) = crate::errlog::path() {
                        if ui.button("Журнал").on_hover_text("Ошибки Vaporust (без логина и SteamID)").clicked() {
                            if !p.exists() {
                                crate::errlog::write("журнал", "журнал создан");
                            }
                            let _ = std::process::Command::new("notepad.exe").arg(&p).spawn();
                        }
                    }
                    if ui
                        .button("Сообщить об ошибке")
                        .on_hover_text("Страница GitHub: опишите, что случилось, и приложите журнал")
                        .clicked()
                    {
                        let title = "Ошибка в Vaporust";
                        let body = format!(
                            "Версия: {} ({})%0A%0AЧто случилось:%0A%0AЧто делали перед этим:%0A%0A(приложите vaporust.log: Настройки -> О программе -> Журнал)",
                            env!("CARGO_PKG_VERSION"),
                            if cfg!(target_pointer_width = "64") { "64 бит" } else { "32 бит" }
                        );
                        let url = format!("{GITHUB}/issues/new?title={title}&body={body}").replace(' ', "%20");
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                });
                ui.add_space(4.0);
                ui.weak("Неофициальный проект, не связан с Valve. Лицензия GPL-3.0-or-later. Основан на Aurelia (Drackrath).");
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Благодарности").size(16.0).strong());
                ui.weak("Vaporust стал возможен благодаря работе этих людей и проектов. Спасибо!");
                ui.add_space(4.0);
                let thanks: [(&str, &str, &str); 9] = [
                    ("Aurelia", "https://github.com/Drackrath/Aurelia", "Drackrath: основа Vaporust - вход, загрузка игр, друзья, чат, достижения"),
                    ("SteamFlow", "https://github.com/weter11/SteamFlow", "weter11: проект, из которого выросла Aurelia"),
                    ("steam-vent", "https://codeberg.org/steam-vent/steam-vent", "разбор и реализация сетевого протокола Steam"),
                    ("OpenSteamClient", "https://github.com/OpenSteamClient/OpenSteamClient", "Rosentti и участники: открытый клиент Steam, один из вдохновителей"),
                    ("SteamDatabase / Protobufs", "https://github.com/SteamDatabase/Protobufs", "описания сообщений Steam"),
                    ("SteamKit2", "https://github.com/SteamRE/SteamKit", "исследования протокола Steam"),
                    ("node-steamcommunity", "https://github.com/DoctorMcKay/node-steamcommunity", "DoctorMcKay: как отправлять картинки и стикеры в чат"),
                    ("egui / eframe", "https://github.com/emilk/egui", "emilk: библиотека интерфейса"),
                    ("Rust и библиотеки", "https://github.com/felixsmtnk/Vaporust#использованные-проекты", "tokio, image, windows-rs и другие - полный список в README"),
                ];
                for (name, url, what) in thanks {
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to(name, url);
                        ui.weak(what);
                    });
                }
            });
        });
        });
    }
}

/// КБ/с -> «1.5» (МБ/с, без лишних нулей).
fn fmt_mb(kb: u64) -> String {
    let s = format!("{:.2}", kb as f64 / 1000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Текст поля (МБ/с, допускается запятая) -> КБ/с. Пусто = 0. None - не число.
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
