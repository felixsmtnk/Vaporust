//! Окно «Свойства игры» (как в Steam), отдельное окно Windows с вкладками:
//! Общие (параметры запуска, язык), Версии и беты, Файлы, DLC, Облако.
//!
//! Язык и бета записываются в UserConfig файла игры (appmanifest); файлы под них докачивает
//! сам Steam при следующем «Играть» (лаунчер показывает прогресс). Править файлы Steam можно
//! только при закрытом Steam — иначе он перезапишет их при выходе.

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
}

pub struct PropsWin {
    appid: u32,
    name: String,
    tab: Tab,
    // Параметры запуска
    launch_text: String,
    launch_saved: String,
    lc_path: Option<std::path::PathBuf>,
    // Выбор в выпадающих списках (None — как в файле игры)
    lang_choice: Option<String>,
    beta_choice: Option<String>,
    move_idx: usize,
    confirm_uninstall: bool,
    note: Option<String>,
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
        "schinese" => "Китайский (упрощённый)",
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
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher)),
            |ctx, _class| {
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
                    ui.strong("Язык игры");
                    match &data.languages {
                        None => {
                            ui.spinner();
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
                                    "Сейчас установлен: {}. Переключение ждёт загрузки через Steam.",
                                    lang_name(&acf.mounted_language)
                                ));
                                if ui
                                    .add_enabled(!locked, egui::Button::new("Обновить сейчас"))
                                    .on_hover_text("Steam откроется незаметно, скачает файлы языка и закроется.")
                                    .clicked()
                                {
                                    do_update = true;
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
                            "Выбрана: {}. Ждёт загрузки через Steam.",
                            branch_name(&acf.user_beta)
                        ));
                        if ui
                            .add_enabled(!locked, egui::Button::new("Обновить сейчас"))
                            .on_hover_text("Steam откроется незаметно, скачает выбранную версию и закроется.")
                            .clicked()
                        {
                            do_update = true;
                        }
                    }
                    ui.add_space(6.0);
                    match &data.branches {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(e)) => {
                            ui.label(format!("Не удалось получить ветки: {e}"));
                        }
                        Some(Ok(branches)) => {
                            let current = if acf.user_beta.is_empty() { "public".to_string() } else { acf.user_beta.clone() };
                            let choice = w.beta_choice.get_or_insert_with(|| current.clone());
                            for b in branches {
                                ui.radio_value(choice, b.clone(), branch_name(b));
                            }
                            ui.weak("Ветки с паролем (закрытые беты) пока не поддерживаются.");
                            if ui
                                .add_enabled(!locked && *choice != current, egui::Button::new("Переключить"))
                                .clicked()
                            {
                                let key = if choice == "public" { "" } else { choice.as_str() };
                                w.note = Some(match local::set_user_config(&acf_path, "BetaKey", key) {
                                    Ok(()) => format!(
                                        "Выбрана {}. Нажмите «Обновить сейчас» (или Steam скачает её при следующем «Играть»).",
                                        branch_name(choice)
                                    ),
                                    Err(e) => e,
                                });
                            }
                        }
                    }
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
                        ui.colored_label(YELLOW, "Игре нужно обновление (или смена языка/беты ждёт загрузки).");
                    }
                    if ui
                        .add_enabled(!locked, egui::Button::new("Обновить через Steam"))
                        .on_hover_text("Steam откроется незаметно, скачает обновление и закроется. Игра не запускается.")
                        .clicked()
                    {
                        do_update = true;
                    }
                    ui.add_space(10.0);

                    if ui
                        .add_enabled(!locked, egui::Button::new("Проверить целостность файлов"))
                        .on_hover_text("Сравнить файлы с серверами Steam и докачать испорченные. Ход проверки — внизу главного окна.")
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
                        ui.spinner();
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
                                    ui.weak("— не куплено");
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

                Tab::Cloud => match &data.cloud {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.label(format!("Не удалось получить список: {e}"));
                    }
                    Some(Ok(files)) if files.is_empty() => {
                        ui.label("В облаке Steam нет сохранений этой игры.");
                    }
                    Some(Ok(files)) => {
                        let total: u64 = files.iter().map(|f| f.size).sum();
                        ui.label(format!("Файлов в облаке: {}, всего {}", files.len(), net::fmt_bytes(total)));
                        ui.weak("Steam сам синхронизирует сохранения при запуске игры и после выхода из неё.");
                        ui.separator();
                        egui::Grid::new("cloud").striped(true).num_columns(3).show(ui, |ui| {
                            for f in files {
                                ui.label(&f.name);
                                ui.label(net::fmt_bytes(f.size));
                                ui.weak(fmt_date(f.time));
                                ui.end_row();
                            }
                        });
                    }
                },
            });
        });

        for c in cmds {
            let _ = self.tx.send(c);
        }
        if close_steam {
            self.close_steam(ctx);
        }
        if do_update {
            self.start_update(appid, ctx);
        }
    }
}
