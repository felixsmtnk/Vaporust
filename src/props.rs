//! Окно «Свойства игры» (как в Steam), отдельное окно Windows с вкладками:
//! Общие (параметры запуска, язык), Версии и беты, Файлы, DLC, Облако.
//!
//! Язык и бета записываются в UserConfig файла игры (appmanifest); файлы под них докачивает
//! сам Steam при следующем «Играть» (лаунчер показывает прогресс). Править файлы Steam можно
//! только при закрытом Steam - иначе он перезапишет их при выходе.

use crate::net::{self, Cmd, NetState};
use crate::{launchopts, local, LauncherApp};
use eframe::egui;
use std::sync::atomic::Ordering;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    General,
    Betas,
    Files,
    Dlc,
    Cloud,
    Workshop,
}

pub struct PropsWin {
    appid: u32,
    name: String,
    tab: Tab,
    // Параметры запуска
    launch_text: String,
    launch_saved: String,
    lc_path: Option<std::path::PathBuf>,
    // Выбор в выпадающих списках (None - как в файле игры)
    lang_choice: Option<String>,
    beta_choice: Option<String>,
    move_idx: usize,
    confirm_uninstall: bool,
    note: Option<String>,
    // Мастерская: подписки уже запрошены.
    ws_loaded: bool,
    // Откат: введенные номера версий по хранилищам.
    dg_inputs: std::collections::BTreeMap<u32, String>,
    // Проверка защиты для запуска без Steam (идет в фоновом потоке).
    drm: Option<std::sync::Arc<std::sync::Mutex<Option<crate::direct::DrmReport>>>>,
    // Какую резервную копию сохранений подтверждаем восстановить.
    restore_confirm: Option<std::path::PathBuf>,
}

const YELLOW: egui::Color32 = egui::Color32::from_rgb(230, 170, 60);
const GREEN: egui::Color32 = egui::Color32::from_rgb(110, 200, 110);

/// Русское название языка Steam (ключи как в appinfo).
fn lang_name(key: &str) -> String {
    let n = match key {
        "english" => "Английский",
        "russian" => "Русский",
        "ukrainian" => "Украинский",
        "german" => "Немецкий",
        "french" => "Французский",
        "italian" => "Итальянский",
        "spanish" => "Испанский (Испания)",
        "latam" => "Испанский (Латинская Америка)",
        "brazilian" => "Португальский (Бразилия)",
        "portuguese" => "Португальский",
        "polish" => "Польский",
        "czech" => "Чешский",
        "turkish" => "Турецкий",
        "japanese" => "Японский",
        "koreana" => "Корейский",
        "schinese" => "Китайский (упрощенный)",
        "tchinese" => "Китайский (традиционный)",
        "dutch" => "Нидерландский",
        "hungarian" => "Венгерский",
        "swedish" => "Шведский",
        "finnish" => "Финский",
        "danish" => "Датский",
        "norwegian" => "Норвежский",
        "romanian" => "Румынский",
        "bulgarian" => "Болгарский",
        "greek" => "Греческий",
        "thai" => "Тайский",
        "vietnamese" => "Вьетнамский",
        "arabic" => "Арабский",
        "indonesian" => "Индонезийский",
        other => return other.to_string(),
    };
    n.to_string()
}

fn branch_name(key: &str) -> String {
    if key.is_empty() || key == "public" {
        "Основная версия (public)".into()
    } else {
        key.to_string()
    }
}

/// Понятное название хранилища: свое имя, DLC, язык, система или «основные файлы».
fn depot_desc(d: Option<&net::DepotDesc>) -> String {
    let Some((name, os, lang, dlc)) = d else { return "Файлы игры".into() };
    if let Some(n) = name {
        return n.clone();
    }
    if let Some(id) = dlc {
        return format!("Дополнение (DLC {id})");
    }
    if let Some(l) = lang {
        return format!("Язык: {}", lang_name(l));
    }
    match os.as_deref() {
        Some(o) if o.contains("windows") => "Файлы для Windows".into(),
        Some(o) if o.contains("linux") => "Файлы для Linux".into(),
        Some(o) if o.contains("macos") => "Файлы для macOS".into(),
        _ => "Основные файлы игры".into(),
    }
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default()
}

impl LauncherApp {
    pub(crate) fn open_props(&mut self, appid: u32, name: String) {
        if crate::custom::is_custom(appid) {
            self.edit_custom(appid);
            return;
        }
        let lc_path = self.steam.as_ref().and_then(|s| launchopts::find_localconfig(&s.dir));
        let (launch, note) = match &lc_path {
            None => (String::new(), Some("Не найден файл настроек Steam (localconfig.vdf).".to_string())),
            Some(p) => match launchopts::read(p, appid) {
                Ok(v) => (v, None),
                Err(e) => (String::new(), Some(e)),
            },
        };
        self.props = Some(PropsWin {
            appid,
            name,
            tab: Tab::General,
            launch_saved: launch.clone(),
            launch_text: launch,
            lc_path,
            lang_choice: None,
            beta_choice: None,
            move_idx: 0,
            confirm_uninstall: false,
            note,
            ws_loaded: false,
            dg_inputs: Default::default(),
            drm: None,
            restore_confirm: None,
        });
        let _ = self.tx.send(Cmd::LoadProps(appid));
    }

    pub(crate) fn props_window(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(title) = self.props.as_ref().map(|p| format!("Свойства: {}", p.name)) else {
            return;
        };
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_props"),
            egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([680.0, 500.0])
                .with_min_inner_size([520.0, 360.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher))
                .with_decorations(crate::winframe::system()),
            |ctx, _class| {
                crate::winframe::title_bar_auto(ctx);
                self.props_contents(ctx, net);
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.props = None;
        }
    }

    fn props_contents(&mut self, ctx: &egui::Context, net: &NetState) {
        let steam_running = self.steam_running();
        let busy = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
        let op_active = net.install.as_ref().is_some_and(|i| i.active());
        let Some(w) = self.props.as_mut() else { return };
        let appid = w.appid;
        let game = self.games.iter().find(|g| g.appid == w.appid).cloned();
        let data = net.props.get(&w.appid).cloned().unwrap_or_default();
        let mut close_steam = false;
        let mut cmds: Vec<Cmd> = Vec::new();
        let mut do_update = false;
        // Смена беты: без Steam (как кнопка «Обновить» в библиотеке).
        let mut do_update_smart = false;
        // Закрепить (true) или открепить (false) - выполняется после отрисовки.
        let mut pin_action: Option<bool> = None;
        let pinned_real = self.settings.pinned.get(&appid).cloned();
        let logged_in = net.account.is_some();
        let my_id = net.my_id;
        let steam_dir = self.steam.as_ref().map(|s| s.dir.clone());

        egui::SidePanel::left("props_tabs")
            .resizable(false)
            .exact_width(150.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                for (tab, label) in [
                    (Tab::General, "Общие"),
                    (Tab::Betas, "Версии и беты"),
                    (Tab::Files, "Установленные файлы"),
                    (Tab::Dlc, "DLC"),
                    (Tab::Cloud, "Облако Steam"),
                    (Tab::Workshop, "Мастерская"),
                ] {
                    ui.selectable_value(&mut w.tab, tab, label);
                }
            });

        // Нижняя строка: результат последнего действия (панели объявляются до центральной).
        if let Some(n) = &w.note {
            egui::TopBottomPanel::bottom("props_note").show(ctx, |ui| {
                ui.colored_label(GREEN, n);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(&w.name);
            if steam_running {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        YELLOW,
                        "Steam открыт: менять настройки и файлы игры можно, когда он закрыт.",
                    );
                    if !busy && ui.button("Закрыть Steam").clicked() {
                        close_steam = true;
                    }
                });
            }
            let locked = steam_running || busy || op_active;
            ui.separator();
            let Some(game) = game else {
                ui.label("Игра не установлена.");
                return;
            };
            let acf_path = local::appmanifest_path(&game);
            let acf = local::read_acf(&acf_path);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match w.tab {
                Tab::General => {
                    ui.strong("Параметры запуска");
                    ui.add(
                        egui::TextEdit::multiline(&mut w.launch_text)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .hint_text("например: -novid -high -w 1280 -h 720"),
                    );
                    ui.weak("Можно использовать %command%, как в Steam.");
                    ui.horizontal(|ui| {
                        let changed = w.launch_text.trim() != w.launch_saved.trim();
                        if ui
                            .add_enabled(w.lc_path.is_some() && !locked && changed, egui::Button::new("Сохранить"))
                            .clicked()
                        {
                            if let Some(p) = &w.lc_path {
                                match launchopts::write(p, w.appid, &w.launch_text) {
                                    Ok(()) => {
                                        w.launch_saved = w.launch_text.trim().to_string();
                                        w.note = Some("Параметры запуска сохранены.".into());
                                    }
                                    Err(e) => w.note = Some(e),
                                }
                            }
                        }
                        if ui.add_enabled(changed, egui::Button::new("Вернуть как было")).clicked() {
                            w.launch_text = w.launch_saved.clone();
                        }
                    });

                    ui.add_space(12.0);
                    ui.strong("Запуск без Steam");
                    let mut direct = self.settings.no_steam.contains(&appid);
                    if ui
                        .checkbox(&mut direct, "Запускать эту игру без Steam")
                        .on_hover_text("Быстрее и легче: Steam не запускается вовсе. Только для игр без защиты Steam.")
                        .changed()
                    {
                        if direct {
                            self.settings.no_steam.insert(appid);
                        } else {
                            self.settings.no_steam.remove(&appid);
                        }
                    }
                    // Проверка защиты: файлы игры (SteamStub, steam_api) и пометки магазина.
                    let slot = w
                        .drm
                        .get_or_insert_with(|| {
                            let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
                            let (s, dir, ctx2) = (slot.clone(), game.dir.clone(), ui.ctx().clone());
                            std::thread::spawn(move || {
                                let r = crate::direct::check_drm(appid, &dir);
                                *s.lock().unwrap() = Some(r);
                                ctx2.request_repaint();
                            });
                            slot
                        })
                        .clone();
                    let report = slot.lock().unwrap().clone();
                    match report {
                        None => {
                            ui.horizontal(|ui| {
                                crate::spin::spinner(ui);
                                ui.weak("Проверяю защиту игры...");
                            });
                        }
                        Some(r) => {
                            let (ok, text) = r.verdict();
                            let color = match ok {
                                Some(true) => GREEN,
                                Some(false) => egui::Color32::from_rgb(230, 110, 110),
                                None => YELLOW,
                            };
                            ui.colored_label(color, text);
                            if !r.steamstub.is_empty() {
                                ui.weak(format!("Защищенные файлы: {}", r.steamstub.join(", ")));
                            }
                            if let Some(n) = &r.drm_notice {
                                ui.weak(format!("Магазин Steam: защита {n}"));
                            }
                            if let Some(n) = &r.account_notice {
                                ui.weak(format!("Магазин Steam: нужен {n}"));
                            }
                            if direct && ok == Some(false) {
                                ui.colored_label(YELLOW, "Галочку лучше снять: игра будет запускаться через Steam.");
                            }
                            if ui.small_button("Проверить еще раз").clicked() {
                                w.drm = None;
                            }
                        }
                    }
                    ui.weak("Без Steam не работают достижения, облачные сохранения, оверлей и учет времени в игре.");

                    ui.add_space(12.0);
                    ui.strong("Скрипты запуска");
                    let mut sc = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
                    let before = sc.clone();
                    egui::Grid::new("scripts").num_columns(2).show(ui, |ui| {
                        ui.label("Перед запуском");
                        ui.add(
                            egui::TextEdit::singleline(&mut sc.before)
                                .desired_width(380.0)
                                .hint_text("например: start \"\" \"C:\\Tools\\mod.exe\""),
                        );
                        ui.end_row();
                        ui.label("После выхода");
                        ui.add(egui::TextEdit::singleline(&mut sc.after).desired_width(380.0));
                        ui.end_row();
                    });
                    ui.weak(
                        "Команды как в cmd.exe, выполняются в папке игры. Игра запускается, когда команда «перед \
                         запуском» закончится (не дольше 2 минут). Есть переменные %VAPORUST_APPID%, \
                         %VAPORUST_GAME_NAME%, %VAPORUST_GAME_DIR%.",
                    );
                    if sc != before {
                        if sc.is_empty() {
                            self.settings.scripts.remove(&appid);
                        } else {
                            self.settings.scripts.insert(appid, sc);
                        }
                    }

                    ui.add_space(12.0);
                    ui.strong("Язык игры");
                    match &data.languages {
                        None => {
                            crate::spin::spinner(ui);
                        }
                        Some(Err(e)) => {
                            ui.label(format!("Не удалось узнать языки: {e}"));
                        }
                        Some(Ok(langs)) if langs.is_empty() => {
                            ui.weak("Steam не сообщает список языков этой игры.");
                        }
                        Some(Ok(langs)) => {
                            let current = if !acf.user_language.is_empty() {
                                acf.user_language.clone()
                            } else if !acf.mounted_language.is_empty() {
                                acf.mounted_language.clone()
                            } else {
                                "english".into()
                            };
                            let choice = w.lang_choice.get_or_insert_with(|| current.clone());
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("lang")
                                    .selected_text(lang_name(choice))
                                    .width(240.0)
                                    .show_ui(ui, |ui| {
                                        for l in langs {
                                            ui.selectable_value(choice, l.clone(), lang_name(l));
                                        }
                                    });
                                if ui
                                    .add_enabled(!locked && *choice != current, egui::Button::new("Применить"))
                                    .clicked()
                                {
                                    w.note = Some(match local::set_user_config(&acf_path, "language", choice) {
                                        Ok(()) => format!(
                                            "Язык: {}. Нажмите «Обновить сейчас» (или Steam докачает файлы при следующем «Играть»).",
                                            lang_name(choice)
                                        ),
                                        Err(e) => e,
                                    });
                                }
                            });
                            if !acf.mounted_language.is_empty() && acf.user_language != acf.mounted_language && !acf.user_language.is_empty() {
                                ui.colored_label(YELLOW, format!(
                                    "Сейчас установлен: {}. Переключение ждет загрузки через Steam.",
                                    lang_name(&acf.mounted_language)
                                ));
                                if ui
                                    .add_enabled(!locked, egui::Button::new("Обновить сейчас"))
                                    .on_hover_text("Скачать файлы выбранного языка. Steam не запускается (если в «Настройках» не выбрано обновление через Steam).")
                                    .clicked()
                                {
                                    do_update_smart = true;
                                }
                            }
                        }
                    }
                }

                Tab::Betas => {
                    let mounted = acf.mounted_beta.clone();
                    ui.label(format!("Установлена: {}", branch_name(&mounted)));
                    if acf.user_beta != mounted {
                        ui.colored_label(YELLOW, format!(
                            "Выбрана: {}. Ждет загрузки через Steam.",
                            branch_name(&acf.user_beta)
                        ));
                        if ui
                            .add_enabled(!locked, egui::Button::new("Обновить сейчас"))
                            .on_hover_text("Скачать файлы выбранной версии. Steam не запускается (если в «Настройках» не выбрано обновление через Steam).")
                            .clicked()
                        {
                            do_update_smart = true;
                        }
                    }
                    ui.add_space(6.0);
                    match &data.branches {
                        None => {
                            crate::spin::spinner(ui);
                        }
                        Some(Err(e)) => {
                            ui.label(format!("Не удалось получить ветки: {e}"));
                        }
                        Some(Ok(branches)) => {
                            let current = if acf.user_beta.is_empty() { "public".to_string() } else { acf.user_beta.clone() };
                            let choice = w.beta_choice.get_or_insert_with(|| current.clone());
                            for (b, desc) in branches {
                                ui.horizontal(|ui| {
                                    ui.radio_value(choice, b.clone(), branch_name(b));
                                    if let Some(d) = desc {
                                        ui.weak(format!("- {d}"));
                                    }
                                });
                            }
                            ui.weak("Закрытые беты с паролем пока включаются только в самом Steam (Свойства игры -> Бета-версии).");
                            if ui
                                .add_enabled(!locked && *choice != current, egui::Button::new("Переключить"))
                                .clicked()
                            {
                                let key = if choice == "public" { "" } else { choice.as_str() };
                                w.note = Some(match local::set_user_config(&acf_path, "BetaKey", key) {
                                    Ok(()) => format!(
                                        "Выбрана {}. Нажмите «Обновить сейчас» (или Steam скачает ее при следующем «Играть»).",
                                        branch_name(choice)
                                    ),
                                    Err(e) => e,
                                });
                            }
                        }
                    }

                    // ---------- Закрепление и откат ----------
                    ui.add_space(14.0);
                    ui.separator();
                    ui.strong("Версия игры");
                    match &pinned_real {
                        Some(_) => {
                            ui.colored_label(GREEN, "Версия закреплена: Steam не будет обновлять эту игру.");
                            ui.weak(
                                "Перед каждым запуском Vaporust сообщает Steam, что игра уже обновлена. \
                                 Проверка файлов и обновление недоступны, пока версия закреплена.",
                            );
                            if ui.add_enabled(!locked, egui::Button::new("Открепить")).clicked() {
                                pin_action = Some(false);
                            }
                        }
                        None => {
                            ui.weak(
                                "Закрепление не дает Steam обновлять игру - например, чтобы после \
                                 обновления не сломались моды или сохранения.",
                            );
                            if ui
                                .add_enabled(!locked && logged_in, egui::Button::new("Закрепить текущую версию"))
                                .clicked()
                            {
                                pin_action = Some(true);
                            }
                        }
                    }

                    ui.add_space(8.0);
                    egui::CollapsingHeader::new("Откатить на старую версию").id_salt("downgrade").show(ui, |ui| {
                        ui.weak(
                            "Steam делит игру на части (хранилища): основные файлы, языки, файлы для \
                             разных систем - у каждой своя история версий. Steam не хранит список старых \
                             версий, номер нужной (Manifest ID) есть на SteamDB: откройте ссылку, найдите \
                             дату и скопируйте число. Обычно хватает отката «Основных файлов игры». \
                             Пустое поле - часть не меняется.",
                        );
                        ui.add_space(4.0);
                        // Настоящие версии: у закрепленной игры в файле записаны «маскировочные».
                        let current = pinned_real.clone().unwrap_or_else(|| local::installed_depots(&acf_path));
                        if current.is_empty() {
                            ui.weak("В файле игры нет списка хранилищ.");
                            return;
                        }
                        let sizes = local::installed_depot_sizes(&acf_path);
                        let descs = data.depots.as_ref().and_then(|r| r.as_ref().ok());
                        egui::Grid::new("dg_grid").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.weak("Часть игры (хранилище)");
                            ui.weak("Версия сейчас");
                            ui.weak("Нужная версия");
                            ui.end_row();
                            for (depot, m) in &current {
                                ui.vertical(|ui| {
                                    ui.label(depot_desc(descs.and_then(|d| d.get(depot))));
                                    let size = sizes.get(depot).map(|s| format!(" · {}", net::fmt_bytes(*s))).unwrap_or_default();
                                    ui.weak(format!("№ {depot}{size}"));
                                });
                                ui.weak(m.to_string());
                                let input = w.dg_inputs.entry(*depot).or_default();
                                ui.add(egui::TextEdit::singleline(input).desired_width(170.0).hint_text("Manifest ID"));
                                ui.hyperlink_to("SteamDB", format!("https://steamdb.info/depot/{depot}/manifests/"))
                                    .on_hover_text("История версий этого хранилища");
                                ui.end_row();
                            }
                        });
                        let overrides: std::collections::HashMap<u32, u64> = w
                            .dg_inputs
                            .iter()
                            .filter_map(|(d, s)| s.trim().parse::<u64>().ok().map(|m| (*d, m)))
                            .filter(|(d, m)| current.get(d) != Some(m))
                            .collect();
                        let bad = w.dg_inputs.values().any(|s| !s.trim().is_empty() && s.trim().parse::<u64>().is_err());
                        if bad {
                            ui.colored_label(YELLOW, "Номер версии - это только цифры.");
                        }
                        ui.add_space(4.0);
                        if ui
                            .add_enabled(
                                !locked && logged_in && !bad && !overrides.is_empty(),
                                egui::Button::new("Скачать выбранную версию"),
                            )
                            .on_hover_text("Ход загрузки - внизу главного окна. После отката версия закрепится.")
                            .clicked()
                        {
                            let library = game
                                .dir
                                .parent()
                                .and_then(|p| p.parent())
                                .and_then(|p| p.parent())
                                .map(|p| p.to_path_buf())
                                .unwrap_or_default();
                            cmds.push(Cmd::Downgrade { appid, name: w.name.clone(), library, overrides });
                            w.dg_inputs.clear();
                            w.note = Some("Откат начат - ход загрузки внизу главного окна.".into());
                        }
                    });
                }

                Tab::Files => {
                    ui.horizontal(|ui| {
                        ui.label(format!("Папка: {}", game.dir.display()));
                        if ui.button("Открыть").clicked() {
                            local::open_folder(&game.dir);
                        }
                    });
                    ui.label(format!("Размер на диске: {}", net::fmt_bytes(acf.size_on_disk)));
                    if acf.last_updated > 0 {
                        ui.label(format!("Обновлена: {}", fmt_date(acf.last_updated)));
                    }
                    if acf.updating() {
                        ui.colored_label(YELLOW, "Игре нужно обновление (или смена языка/беты ждет загрузки).");
                    }
                    if ui
                        .add_enabled(!locked && pinned_real.is_none(), egui::Button::new("Обновить через Steam"))
                        .on_hover_text("Steam откроется незаметно, скачает обновление и закроется. Игра не запускается.")
                        .clicked()
                    {
                        do_update = true;
                    }
                    ui.add_space(10.0);

                    if ui
                        .add_enabled(!locked && pinned_real.is_none(), egui::Button::new("Проверить целостность файлов"))
                        .on_hover_text("Сравнить файлы с серверами Steam и докачать испорченные. Ход проверки - внизу главного окна.")
                        .clicked()
                    {
                        cmds.push(Cmd::Verify { appid: w.appid, name: w.name.clone() });
                    }

                    ui.add_space(10.0);
                    let others: Vec<_> = self
                        .libraries
                        .iter()
                        .filter(|l| !game.dir.starts_with(l))
                        .cloned()
                        .collect();
                    if others.is_empty() {
                        ui.weak("Перенос: у Steam одна папка библиотеки, переносить некуда.");
                    } else {
                        w.move_idx = w.move_idx.min(others.len() - 1);
                        ui.horizontal(|ui| {
                            ui.label("Перенести в:");
                            egui::ComboBox::from_id_salt("move")
                                .selected_text(others[w.move_idx].display().to_string())
                                .show_ui(ui, |ui| {
                                    for (i, l) in others.iter().enumerate() {
                                        let free = local::free_space(l)
                                            .map(|f| format!(" (свободно {})", net::fmt_bytes(f)))
                                            .unwrap_or_default();
                                        ui.selectable_value(&mut w.move_idx, i, format!("{}{free}", l.display()));
                                    }
                                });
                            if ui.add_enabled(!locked, egui::Button::new("Перенести")).clicked() {
                                cmds.push(Cmd::Move {
                                    appid: w.appid,
                                    name: w.name.clone(),
                                    dest: others[w.move_idx].clone(),
                                });
                            }
                        });
                    }

                    // Перепривязка: файлы игры уже лежат в другой библиотеке (перенесли вручную).
                    if let Some(steam) = self.steam.clone() {
                        for lib in local::relink_candidates(&steam.dir, &game) {
                            ui.add_space(6.0);
                            ui.weak(format!("Папка этой игры есть и в библиотеке {}.", lib.display()));
                            if ui
                                .add_enabled(!locked, egui::Button::new("Привязать игру к этой библиотеке"))
                                .on_hover_text(
                                    "Если вы перенесли файлы игры вручную: Steam и Vaporust будут брать игру\n\
                                     оттуда. Файлы не копируются и не удаляются, переносится только запись\n\
                                     об установке. Старую папку потом можно удалить вручную.",
                                )
                                .clicked()
                            {
                                w.note = Some(match local::relink_game(&steam.dir, &game, &lib) {
                                    Ok(p) => {
                                        self.rescan_needed.store(true, std::sync::atomic::Ordering::Relaxed);
                                        format!("Готово: игра теперь в {}.", p.display())
                                    }
                                    Err(e) => format!("Не получилось: {e}"),
                                });
                            }
                        }
                    }

                    ui.add_space(14.0);
                    if !w.confirm_uninstall {
                        if ui.add_enabled(!locked, egui::Button::new("Удалить игру...")).clicked() {
                            w.confirm_uninstall = true;
                        }
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), format!(
                            "Удалить «{}» с диска? Будет удалена папка игры ({}). Сохранения в облаке Steam останутся.",
                            w.name,
                            net::fmt_bytes(acf.size_on_disk)
                        ));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!locked, egui::Button::new("Да, удалить")).clicked() {
                                cmds.push(Cmd::Uninstall { appid: w.appid, name: w.name.clone() });
                                w.confirm_uninstall = false;
                            }
                            if ui.button("Отмена").clicked() {
                                w.confirm_uninstall = false;
                            }
                        });
                    }
                }

                Tab::Dlc => match &data.dlc {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Err(e)) => {
                        ui.label(format!("Не удалось получить DLC: {e}"));
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        ui.label("У этой игры нет DLC.");
                    }
                    Some(Ok(list)) => {
                        let owned = list.iter().filter(|d| d.owned).count();
                        ui.label(format!("DLC: {}, куплено: {owned}", list.len()));
                        ui.separator();
                        let mut unknown = Vec::new();
                        for d in list {
                            let name = net
                                .app_names
                                .get(&d.app_id)
                                .cloned()
                                .or_else(|| net.owned.iter().find(|g| g.app_id == d.app_id).map(|g| g.name.clone()))
                                .unwrap_or_else(|| {
                                    unknown.push(d.app_id);
                                    format!("DLC {}", d.app_id)
                                });
                            ui.horizontal(|ui| {
                                if d.owned && d.installed {
                                    let mut on = !d.disabled;
                                    if ui.add_enabled(!locked, egui::Checkbox::new(&mut on, &name)).changed() {
                                        cmds.push(Cmd::SetDlc { base: w.appid, dlc: d.app_id, enabled: on });
                                    }
                                } else if d.owned {
                                    ui.label(&name);
                                    if ui.add_enabled(!locked, egui::Button::new("Установить")).clicked() {
                                        if let Some(lib) = game.dir.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
                                            cmds.push(Cmd::Install {
                                                appid: d.app_id,
                                                name: name.clone(),
                                                library: lib.to_path_buf(),
                                            });
                                        }
                                    }
                                } else {
                                    ui.weak(&name);
                                    ui.weak("- не куплено");
                                }
                            });
                        }
                        unknown.retain(|a| self.requested_apps.insert(*a));
                        if !unknown.is_empty() {
                            cmds.push(Cmd::ResolveApps(unknown));
                        }
                        if let Some(e) = &data.error {
                            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                        }
                    }
                },

                Tab::Workshop => {
                    // Только включение/выключение подписанных модов; поиск - на странице игры.
                    if !w.ws_loaded {
                        w.ws_loaded = true;
                        cmds.push(Cmd::Social(crate::social::SocialCmd::WorkshopSubscribed(w.appid)));
                    }
                    let ws = net.workshop.lock().unwrap();
                    crate::workshopui::subscriptions(ui, w.appid, &ws, &mut cmds);
                }

                Tab::Cloud => {
                    // ---------- Синхронизация вручную ----------
                    ui.strong("Синхронизация вручную");
                    ui.weak(
                        "Steam сам синхронизирует сохранения при запуске игры и после выхода. Здесь это можно \
                         сделать вручную, не запуская игру. Сохранения лежат в папке игры на этом ПК (она указана \
                         ниже, над списком файлов).",
                    );
                    ui.weak(
                        "«Скачать из облака» - заменить файлы на ПК облачными (например, после переустановки \
                         Windows или игры). «Отправить в облако» - наоборот. «Синхронизировать» - новые файлы \
                         в обе стороны.",
                    );
                    let remote = match (&steam_dir, my_id) {
                        (Some(s), Some(id)) => Some(
                            s.join("userdata").join((id as u32).to_string()).join(appid.to_string()).join("remote"),
                        ),
                        _ => None,
                    };
                    let can = !locked && logged_in && remote.is_some() && !data.cloud_busy;
                    let mut go: Option<(u8, Option<bool>)> = None;
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(can, egui::Button::new("Скачать из облака")).clicked() {
                            go = Some((0, None));
                        }
                        if ui.add_enabled(can, egui::Button::new("Отправить в облако")).clicked() {
                            go = Some((1, None));
                        }
                        if ui
                            .add_enabled(can, egui::Button::new("Синхронизировать"))
                            .on_hover_text("Новое из облака - сюда, новое с диска - в облако")
                            .clicked()
                        {
                            go = Some((2, None));
                        }
                        if data.cloud_busy {
                            crate::spin::spinner(ui);
                        }
                    });
                    match &data.cloud_sync {
                        Some(Ok(o)) => {
                            if o.downloaded == 0 && o.uploaded == 0 && o.conflicts.is_empty() && o.failed.is_empty() {
                                ui.colored_label(GREEN, "Все уже совпадает: сохранения на ПК и в облаке одинаковые.");
                            } else {
                                ui.colored_label(
                                    GREEN,
                                    format!("Скачано файлов: {}, отправлено: {}.", o.downloaded, o.uploaded),
                                );
                            }
                            if o.skipped > 0 {
                                ui.weak(format!("Пропущено (неизвестная папка сохранений): {}", o.skipped));
                            }
                            for f in &o.failed {
                                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), f);
                            }
                            if !o.conflicts.is_empty() {
                                ui.colored_label(
                                    YELLOW,
                                    format!(
                                        "Расходятся (изменены и у вас, и в облаке): {}. Какую копию оставить?",
                                        o.conflicts.len()
                                    ),
                                );
                                for (name, local, cloud) in &o.conflicts {
                                    ui.weak(format!(
                                        "{name}: у вас {}, в облаке {}",
                                        net::fmt_bytes(*local),
                                        net::fmt_bytes(*cloud)
                                    ));
                                }
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(can, egui::Button::new("Взять из облака")).clicked() {
                                        go = Some((2, Some(true)));
                                    }
                                    if ui.add_enabled(can, egui::Button::new("Оставить мои")).clicked() {
                                        go = Some((2, Some(false)));
                                    }
                                });
                            }
                        }
                        Some(Err(e)) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), format!("Не получилось: {e}"));
                        }
                        None => {}
                    }
                    if let (Some((direction, take)), Some(remote)) = (go, remote.clone()) {
                        cmds.push(Cmd::CloudSync { appid, remote, dir: game.dir.clone(), direction, take });
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    match &data.cloud {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Err(e)) => {
                        ui.label(format!("Не удалось получить список: {e}"));
                    }
                    Some(Ok(files)) if files.is_empty() => {
                        ui.label("В облаке Steam нет сохранений этой игры.");
                    }
                    Some(Ok(files)) => {
                        let total: u64 = files.iter().map(|f| f.size).sum();
                        // Где эти файлы лежат на этом ПК: %WinAppDataLocalLow% и т.п. -> настоящие папки.
                        let resolver = remote.clone().map(|r| {
                            aurelia::library::cloud_sync::CloudPathResolver::new(r, Some(game.dir.clone()))
                        });
                        let local: Vec<Option<std::path::PathBuf>> = files
                            .iter()
                            .map(|f| resolver.as_ref().and_then(|r| r.resolve(&f.name).ok()))
                            .collect();
                        let base = common_dir(local.iter().flatten());
                        if let Some(b) = &base {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Сохранения на этом ПК:");
                                ui.monospace(b.display().to_string());
                                if ui.small_button("Открыть папку").clicked() {
                                    let dir = if b.is_dir() { b.clone() } else { b.parent().unwrap_or(b).to_path_buf() };
                                    local::open_folder(&dir);
                                }
                            });
                        }
                        // Резервные копии сохранений (zip в Документы\Vaporust\Сохранения).
                        ui.horizontal_wrapped(|ui| {
                            ui.strong("Резервные копии");
                            if ui
                                .button("Сделать копию")
                                .on_hover_text("Сложить сохранения с этого ПК в zip-архив (Документы\\Vaporust\\Сохранения)")
                                .clicked()
                            {
                                let paths: Vec<std::path::PathBuf> = local.iter().flatten().cloned().collect();
                                w.note = Some(match crate::backup::backup(&w.name, &paths) {
                                    Ok((p, n)) => format!("Копия сделана: {} (файлов: {n}).", p.display()),
                                    Err(e) => e,
                                });
                            }
                            if ui.small_button("Открыть папку копий").clicked() {
                                let d = crate::backup::game_dir(&w.name);
                                let _ = std::fs::create_dir_all(&d);
                                local::open_folder(&d);
                            }
                        });
                        for (zip, time, size) in crate::backup::list(&w.name).into_iter().take(8) {
                            ui.horizontal(|ui| {
                                let when: chrono::DateTime<chrono::Local> = time.into();
                                ui.label(when.format("%d.%m.%Y %H:%M").to_string());
                                ui.weak(net::fmt_bytes(size));
                                if w.restore_confirm.as_ref() == Some(&zip) {
                                    ui.colored_label(YELLOW, "Текущие сохранения будут заменены. Восстановить?");
                                    if ui.add_enabled(!locked, egui::Button::new("Да")).clicked() {
                                        // Сначала - копия текущего состояния, чтобы можно было вернуться.
                                        let paths: Vec<std::path::PathBuf> = local.iter().flatten().cloned().collect();
                                        let _ = crate::backup::backup(&w.name, &paths);
                                        w.note = Some(match crate::backup::restore(&zip) {
                                            Ok(n) => format!("Восстановлено файлов: {n}. Прежние сохранения - в новой копии сверху."),
                                            Err(e) => e,
                                        });
                                        w.restore_confirm = None;
                                    }
                                    if ui.button("Нет").clicked() {
                                        w.restore_confirm = None;
                                    }
                                } else if ui
                                    .add_enabled(!locked, egui::Button::new("Восстановить..."))
                                    .on_disabled_hover_text("Сначала закройте Steam и игру")
                                    .clicked()
                                {
                                    w.restore_confirm = Some(zip.clone());
                                }
                            });
                        }
                        ui.weak(
                            "Steam при запуске игры может сам заменить файлы версией из облака. Чтобы оставить \
                             восстановленные, после восстановления нажмите выше «Отправить в облако».",
                        );
                        ui.separator();
                        ui.label(format!("Файлов в облаке: {}, всего {}", files.len(), net::fmt_bytes(total)));
                        ui.separator();
                        egui::Grid::new("cloud").striped(true).num_columns(4).show(ui, |ui| {
                            for (f, p) in files.iter().zip(&local) {
                                let short = match (p, &base) {
                                    (Some(p), Some(b)) => p
                                        .strip_prefix(b)
                                        .map(|r| r.display().to_string())
                                        .unwrap_or_else(|_| p.display().to_string()),
                                    (Some(p), None) => p.display().to_string(),
                                    (None, _) => f.name.clone(),
                                };
                                let resp = ui.label(short);
                                if let Some(p) = p {
                                    resp.on_hover_text(p.display().to_string());
                                }
                                ui.label(net::fmt_bytes(f.size));
                                ui.weak(fmt_date(f.time));
                                match p {
                                    Some(p) if p.is_file() => ui.weak("есть на диске"),
                                    Some(_) => ui.colored_label(YELLOW, "нет на диске"),
                                    None => ui.weak(""),
                                };
                                ui.end_row();
                            }
                        });
                    }
                    }
                }
            });
        });

        for c in cmds {
            let _ = self.tx.send(c);
        }
        if close_steam {
            self.close_steam(ctx);
        }
        if do_update {
            // «Обновить через Steam» и смена языка (нужны другие хранилища) - через Steam.
            self.start_update_steam(appid, ctx);
        }
        if do_update_smart {
            self.start_update(appid, ctx);
        }
        if let Some(on) = pin_action {
            let res = if on { self.pin_game(appid) } else { self.unpin_game(appid) };
            if let Some(w) = self.props.as_mut() {
                w.note = Some(res.unwrap_or_else(|e| e));
            }
        }
    }
}

/// Общая папка для всех путей (самый глубокий общий предок).
fn common_dir<'a>(paths: impl Iterator<Item = &'a std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let mut base: Option<std::path::PathBuf> = None;
    for p in paths {
        let dir = p.parent()?.to_path_buf();
        base = Some(match base {
            None => dir,
            Some(b) => {
                let mut c = std::path::PathBuf::new();
                for (x, y) in b.components().zip(dir.components()) {
                    if x != y {
                        break;
                    }
                    c.push(x);
                }
                c
            }
        });
    }
    base.filter(|b| b.components().count() > 1)
}
