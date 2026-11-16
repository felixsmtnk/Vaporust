//! "Game properties" window (as in Steam), a separate Windows window with tabs:
//! General (launch options, language), Versions and betas, Files, DLC, Cloud.
//!
//! Language and beta are written to the game file's UserConfig (appmanifest); the files for them
//! are downloaded by Steam itself on the next "Play" (the launcher shows progress). Steam files can be edited
//! only while Steam is closed - otherwise it overwrites them on exit.

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
    // Launch options
    launch_text: String,
    launch_saved: String,
    lc_path: Option<std::path::PathBuf>,
    // Choice in the drop-down lists (None - as in the game file)
    lang_choice: Option<String>,
    beta_choice: Option<String>,
    move_idx: usize,
    confirm_uninstall: bool,
    /// Game folder size if appmanifest does not have it (an unfinished game).
    dir_size: Option<u64>,
    note: Option<String>,
    // Workshop: subscriptions already requested.
    ws_loaded: bool,
    // Rollback: entered version numbers per depot.
    dg_inputs: std::collections::BTreeMap<u32, String>,
    // Protection check for launching without Steam (runs in a background thread).
    drm: Option<std::sync::Arc<std::sync::Mutex<Option<crate::direct::DrmReport>>>>,
    // Which save backup restore is being confirmed.
    restore_confirm: Option<std::path::PathBuf>,
}

const YELLOW: egui::Color32 = egui::Color32::from_rgb(230, 170, 60);
const GREEN: egui::Color32 = egui::Color32::from_rgb(110, 200, 110);

/// Russian name of a Steam language (keys as in appinfo).
fn lang_name(key: &str) -> String {
    let n = match key {
        "english" => tr!("Английский", "English"),
        "russian" => tr!("Русский", "Russian"),
        "ukrainian" => tr!("Украинский", "Ukrainian"),
        "german" => tr!("Немецкий", "German"),
        "french" => tr!("Французский", "French"),
        "italian" => tr!("Итальянский", "Italian"),
        "spanish" => tr!("Испанский (Испания)", "Spanish (Spain)"),
        "latam" => tr!("Испанский (Латинская Америка)", "Spanish (Latin America)"),
        "brazilian" => tr!("Португальский (Бразилия)", "Portuguese (Brazil)"),
        "portuguese" => tr!("Португальский", "Portuguese"),
        "polish" => tr!("Польский", "Polish"),
        "czech" => tr!("Чешский", "Czech"),
        "turkish" => tr!("Турецкий", "Turkish"),
        "japanese" => tr!("Японский", "Japanese"),
        "koreana" => tr!("Корейский", "Korean"),
        "schinese" => tr!("Китайский (упрощенный)", "Chinese (Simplified)"),
        "tchinese" => tr!("Китайский (традиционный)", "Chinese (Traditional)"),
        "dutch" => tr!("Нидерландский", "Dutch"),
        "hungarian" => tr!("Венгерский", "Hungarian"),
        "swedish" => tr!("Шведский", "Swedish"),
        "finnish" => tr!("Финский", "Finnish"),
        "danish" => tr!("Датский", "Danish"),
        "norwegian" => tr!("Норвежский", "Norwegian"),
        "romanian" => tr!("Румынский", "Romanian"),
        "bulgarian" => tr!("Болгарский", "Bulgarian"),
        "greek" => tr!("Греческий", "Greek"),
        "thai" => tr!("Тайский", "Thai"),
        "vietnamese" => tr!("Вьетнамский", "Vietnamese"),
        "arabic" => tr!("Арабский", "Arabic"),
        "indonesian" => tr!("Индонезийский", "Indonesian"),
        other => return other.to_string(),
    };
    n.to_string()
}

fn branch_name(key: &str) -> String {
    if key.is_empty() || key == "public" {
        tr!("Основная версия (public)", "Main version (public)").into()
    } else {
        key.to_string()
    }
}

/// Readable depot name: its own name, DLC, language, OS or "base files".
fn depot_desc(d: Option<&net::DepotDesc>) -> String {
    let Some((name, os, lang, dlc)) = d else { return tr!("Файлы игры", "Game files").into() };
    if let Some(n) = name {
        return n.clone();
    }
    if let Some(id) = dlc {
        return trf!("Дополнение (DLC {id})", "DLC ({id})");
    }
    if let Some(l) = lang {
        return trf!("Язык: {}", "Language: {}", lang_name(l));
    }
    match os.as_deref() {
        Some(o) if o.contains("windows") => tr!("Файлы для Windows", "Windows files").into(),
        Some(o) if o.contains("linux") => tr!("Файлы для Linux", "Linux files").into(),
        Some(o) if o.contains("macos") => tr!("Файлы для macOS", "macOS files").into(),
        _ => tr!("Основные файлы игры", "Main game files").into(),
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
            None => (String::new(), Some(tr!("Не найден файл настроек Steam (localconfig.vdf).", "Steam settings file (localconfig.vdf) not found.").to_string())),
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
            dir_size: None,
            note,
            ws_loaded: false,
            dg_inputs: Default::default(),
            drm: None,
            restore_confirm: None,
        });
        let _ = self.tx.send(Cmd::LoadProps(appid));
        // Properties - a page of the main window (as in Steam: "Properties" opens over the library).
        self.close_extras(crate::View::Props);
        self.view = crate::View::Props;
    }

    pub(crate) fn props_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let ctx = &ui.ctx().clone();
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
        let mut do_uninstall = false;
        // Beta change: without Steam (like the "Update" button in the library).
        let mut do_update_smart = false;
        // Pin (true) or unpin (false) - done after drawing.
        let mut pin_action: Option<bool> = None;
        let pinned_real = self.settings.pinned.get(&appid).cloned();
        let logged_in = net.account.is_some();
        let my_id = net.my_id;
        let steam_dir = self.steam.as_ref().map(|s| s.dir.clone());
        let mut back = false;

        egui::SidePanel::left("props_tabs")
            .resizable(false)
            .exact_width(150.0)
            .show_inside(ui, |ui| {
                ui.add_space(6.0);
                if ui.button(tr!("< К игре", "< Back to game")).clicked() {
                    back = true;
                }
                ui.add_space(6.0);
                for (tab, label) in [
                    (Tab::General, tr!("Общие", "General")),
                    (Tab::Betas, tr!("Версии и беты", "Versions and betas")),
                    (Tab::Files, tr!("Установленные файлы", "Installed files")),
                    (Tab::Dlc, "DLC"),
                    (Tab::Cloud, tr!("Облако Steam", "Steam Cloud")),
                    (Tab::Workshop, tr!("Мастерская", "Workshop")),
                ] {
                    ui.selectable_value(&mut w.tab, tab, label);
                }
            });

        // Bottom row: result of the last action (panels are declared before the central one).
        if let Some(n) = &w.note {
            egui::TopBottomPanel::bottom("props_note").show_inside(ui, |ui| {
                ui.colored_label(GREEN, n);
            });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading(&w.name);
            if steam_running {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        YELLOW,
                        tr!("Steam открыт: менять настройки и файлы игры можно, когда он закрыт.", "Steam is open: game settings and files can be changed when it is closed."),
                    );
                    if !busy && ui.button(tr!("Закрыть Steam", "Close Steam")).clicked() {
                        close_steam = true;
                    }
                });
            }
            let locked = steam_running || busy || op_active;
            ui.separator();
            let Some(game) = game else {
                ui.label(tr!("Игра не установлена.", "The game is not installed."));
                return;
            };
            let acf_path = local::appmanifest_path(&game);
            let acf = local::read_acf(&acf_path);
            // For unfinished games (especially those hidden from Steam) the appmanifest size is 0 - count by the folder.
            let disk_size = if acf.size_on_disk > 0 && game.ready {
                acf.size_on_disk
            } else {
                *w.dir_size.get_or_insert_with(|| local::dir_size(&game.dir))
            };

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match w.tab {
                Tab::General => {
                    ui.strong(tr!("Параметры запуска", "Launch options"));
                    ui.add(
                        egui::TextEdit::multiline(&mut w.launch_text)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .hint_text(tr!("например: -novid -high -w 1280 -h 720", "for example: -novid -high -w 1280 -h 720")),
                    );
                    ui.weak(tr!("Можно использовать %command%, как в Steam.", "You can use %command%, as in Steam."));
                    ui.horizontal(|ui| {
                        let changed = w.launch_text.trim() != w.launch_saved.trim();
                        if ui
                            .add_enabled(w.lc_path.is_some() && !locked && changed, egui::Button::new(tr!("Сохранить", "Save")))
                            .clicked()
                        {
                            if let Some(p) = &w.lc_path {
                                match launchopts::write(p, w.appid, &w.launch_text) {
                                    Ok(()) => {
                                        w.launch_saved = w.launch_text.trim().to_string();
                                        w.note = Some(tr!("Параметры запуска сохранены.", "Launch options saved.").into());
                                    }
                                    Err(e) => w.note = Some(e),
                                }
                            }
                        }
                        if ui.add_enabled(changed, egui::Button::new(tr!("Вернуть как было", "Revert"))).clicked() {
                            w.launch_text = w.launch_saved.clone();
                        }
                    });

                    // Auto-update of this game (the AutoUpdateBehavior field in appmanifest, as in
                    // Steam's "Properties -> Updates"). Change only while Steam is closed.
                    ui.add_space(12.0);
                    ui.strong(tr!("Обновления", "Updates"));
                    let cur: u8 = local::acf_top(&acf_path, "AutoUpdateBehavior").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let names = [
                        (0u8, tr!("Всегда обновлять эту игру", "Always keep this game updated")),
                        (1, tr!("Обновлять только при запуске", "Only update when I launch it")),
                        (2, tr!("Высокий приоритет (обновлять раньше других)", "High priority (update before others)")),
                    ];
                    let mut pick = cur;
                    ui.add_enabled_ui(!locked, |ui| {
                        egui::ComboBox::from_id_salt("auto_update")
                            .width(320.0)
                            .selected_text(names.iter().find(|n| n.0 == cur).map_or(tr!("Всегда обновлять эту игру", "Always keep this game updated"), |n| n.1))
                            .show_ui(ui, |ui| {
                                for (v, l) in names {
                                    ui.selectable_value(&mut pick, v, l);
                                }
                            });
                    })
                    .response
                    .on_disabled_hover_text(tr!("Сначала закройте Steam", "Close Steam first"));
                    if pick != cur {
                        w.note = Some(match local::set_acf_top(&acf_path, "AutoUpdateBehavior", &pick.to_string()) {
                            Ok(()) => tr!("Настройка обновлений сохранена.", "Update setting saved.").into(),
                            Err(e) => e,
                        });
                    }
                    ui.weak(tr!("Так же, как в Steam. Vaporust сам игры в фоне не обновляет: кнопка «Обновить» появляется, когда обновление есть.", "The same as in Steam. Vaporust does not update games in the background itself: the \"Update\" button appears when an update is available."));

                    ui.add_space(12.0);
                    ui.strong(tr!("Запуск без Steam", "Launch without Steam"));
                    // Family games (others', from the family library) - only through Steam: launching around
                    // Steam would bypass family sharing rules (for example the lock while the owner plays).
                    let family = crate::is_family_only(net, appid);
                    let mut direct = self.settings.no_steam.contains(&appid) && !family;
                    if ui
                        .add_enabled(!family, egui::Checkbox::new(&mut direct, tr!("Запускать эту игру без Steam", "Launch this game without Steam")))
                        .on_hover_text(tr!("Быстрее и легче: Steam не запускается вовсе. Только для игр без защиты Steam.", "Faster and lighter: Steam does not start at all. Only for games without Steam protection."))
                        .on_disabled_hover_text(tr!("Игра из семейной библиотеки: запускается только через Steam, чтобы соблюдались правила семейного доступа.", "A game from the family library: it is launched only through Steam so that family sharing rules are followed."))
                        .changed()
                    {
                        if direct {
                            self.settings.no_steam.insert(appid);
                        } else {
                            self.settings.no_steam.remove(&appid);
                        }
                    }
                    // Protection check: game files (SteamStub, steam_api) and store notes.
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
                                ui.weak(tr!("Проверяю защиту игры...", "Checking the game protection..."));
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
                                ui.weak(trf!("Защищенные файлы: {}", "Protected files: {}", r.steamstub.join(", ")));
                            }
                            if let Some(n) = &r.drm_notice {
                                ui.weak(trf!("Магазин Steam: защита {n}", "Steam store: protection {n}"));
                            }
                            if let Some(n) = &r.account_notice {
                                ui.weak(trf!("Магазин Steam: нужен {n}", "Steam store: requires {n}"));
                            }
                            if direct && ok == Some(false) {
                                ui.colored_label(YELLOW, tr!("Галочку лучше снять: игра будет запускаться через Steam.", "Better uncheck it: the game will be launched through Steam."));
                            }
                            if ui.small_button(tr!("Проверить еще раз", "Check again")).clicked() {
                                w.drm = None;
                            }
                        }
                    }
                    ui.weak(tr!("Без Steam не работают достижения, облачные сохранения, оверлей и учет времени в игре.", "Without Steam, achievements, cloud saves, the overlay and playtime tracking do not work."));

                    ui.add_space(12.0);
                    ui.strong(tr!("Скрипты запуска", "Launch scripts"));
                    let mut sc = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
                    let before = sc.clone();
                    egui::Grid::new("scripts").num_columns(2).show(ui, |ui| {
                        ui.label(tr!("Перед запуском", "Before launch"));
                        ui.add(
                            egui::TextEdit::singleline(&mut sc.before)
                                .desired_width(380.0)
                                .hint_text(tr!("например: start \"\" \"C:\\Tools\\mod.exe\"", "for example: start \"\" \"C:\\Tools\\mod.exe\"")),
                        );
                        ui.end_row();
                        ui.label(tr!("После выхода", "After exit"));
                        ui.add(egui::TextEdit::singleline(&mut sc.after).desired_width(380.0));
                        ui.end_row();
                    });
                    ui.weak(
                        tr!("Команды как в cmd.exe, выполняются в папке игры. Игра запускается, когда команда «перед \
                         запуском» закончится (не дольше 2 минут). Есть переменные %VAPORUST_APPID%, \
                         %VAPORUST_GAME_NAME%, %VAPORUST_GAME_DIR%.", "Commands as in cmd.exe, run in the game folder. The game starts when the \"before launch\" command finishes (at most 2 minutes). Variables: %VAPORUST_APPID%, %VAPORUST_GAME_NAME%, %VAPORUST_GAME_DIR%."),
                    );
                    if sc != before {
                        if sc.is_empty() {
                            self.settings.scripts.remove(&appid);
                        } else {
                            self.settings.scripts.insert(appid, sc);
                        }
                    }

                    ui.add_space(12.0);
                    ui.strong(tr!("Язык игры", "Game language"));
                    match &data.languages {
                        None => {
                            crate::spin::spinner(ui);
                        }
                        Some(Err(e)) => {
                            ui.label(trf!("Не удалось узнать языки: {e}", "Could not get the languages: {e}"));
                        }
                        Some(Ok(langs)) if langs.is_empty() => {
                            ui.weak(tr!("Steam не сообщает список языков этой игры.", "Steam does not report the language list of this game."));
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
                                    .add_enabled(!locked && *choice != current, egui::Button::new(tr!("Применить", "Apply")))
                                    .clicked()
                                {
                                    w.note = Some(match local::set_user_config(&acf_path, "language", choice) {
                                        Ok(()) => trf!(
                                            "Язык: {}. Нажмите «Обновить сейчас» (или Steam докачает файлы при следующем «Играть»).", "Language: {}. Press \"Update now\" (or Steam downloads the files at the next \"Play\").",
                                            lang_name(choice)
                                        ),
                                        Err(e) => e,
                                    });
                                }
                            });
                            if !acf.mounted_language.is_empty() && acf.user_language != acf.mounted_language && !acf.user_language.is_empty() {
                                ui.colored_label(YELLOW, trf!(
                                    "Сейчас установлен: {}. Переключение ждет загрузки через Steam.", "Installed now: {}. The switch waits for a download through Steam.",
                                    lang_name(&acf.mounted_language)
                                ));
                                if ui
                                    .add_enabled(!locked, egui::Button::new(tr!("Обновить сейчас", "Update now")))
                                    .on_hover_text(tr!("Скачать файлы выбранного языка. Steam не запускается (если в «Настройках» не выбрано обновление через Steam).", "Download the files of the selected language. Steam is not started (unless updates via Steam are chosen in \"Settings\")."))
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
                    ui.label(trf!("Установлена: {}", "Installed: {}", branch_name(&mounted)));
                    if acf.user_beta != mounted {
                        ui.colored_label(YELLOW, trf!(
                            "Выбрана: {}. Ждет загрузки через Steam.", "Selected: {}. Waits for a download through Steam.",
                            branch_name(&acf.user_beta)
                        ));
                        if ui
                            .add_enabled(!locked, egui::Button::new(tr!("Обновить сейчас", "Update now")))
                            .on_hover_text(tr!("Скачать файлы выбранной версии. Steam не запускается (если в «Настройках» не выбрано обновление через Steam).", "Download the files of the selected version. Steam is not started (unless updates via Steam are chosen in \"Settings\")."))
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
                            ui.label(trf!("Не удалось получить ветки: {e}", "Could not get the branches: {e}"));
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
                            ui.weak(tr!("Закрытые беты с паролем пока включаются только в самом Steam (Свойства игры -> Бета-версии).", "Private betas with a password can only be turned on in Steam itself for now (game Properties -> Betas)."));
                            if ui
                                .add_enabled(!locked && *choice != current, egui::Button::new(tr!("Переключить", "Switch")))
                                .clicked()
                            {
                                let key = if choice == "public" { "" } else { choice.as_str() };
                                w.note = Some(match local::set_user_config(&acf_path, "BetaKey", key) {
                                    Ok(()) => trf!(
                                        "Выбрана {}. Нажмите «Обновить сейчас» (или Steam скачает ее при следующем «Играть»).", "{} selected. Press \"Update now\" (or Steam downloads it at the next \"Play\").",
                                        branch_name(choice)
                                    ),
                                    Err(e) => e,
                                });
                            }
                        }
                    }

                    // ---------- Pinning and rollback ----------
                    ui.add_space(14.0);
                    ui.separator();
                    ui.strong(tr!("Версия игры", "Game version"));
                    match &pinned_real {
                        Some(_) => {
                            ui.colored_label(GREEN, tr!("Версия закреплена: Steam не будет обновлять эту игру.", "Version pinned: Steam will not update this game."));
                            ui.weak(
                                tr!("Перед каждым запуском Vaporust сообщает Steam, что игра уже обновлена. \
                                 Проверка файлов и обновление недоступны, пока версия закреплена.", "Before every launch Vaporust tells Steam that the game is already up to date. File verification and updates are unavailable while the version is pinned."),
                            );
                            if ui.add_enabled(!locked, egui::Button::new(tr!("Открепить", "Unpin"))).clicked() {
                                pin_action = Some(false);
                            }
                        }
                        None => {
                            ui.weak(
                                tr!("Закрепление не дает Steam обновлять игру - например, чтобы после \
                                 обновления не сломались моды или сохранения.", "Pinning keeps Steam from updating the game - for example so that mods or saves do not break after an update."),
                            );
                            if ui
                                .add_enabled(!locked && logged_in && crate::exp::on(), egui::Button::new(tr!("Закрепить текущую версию", "Pin the current version")))
                                .on_disabled_hover_text(crate::exp::HINT())
                                .clicked()
                            {
                                pin_action = Some(true);
                            }
                        }
                    }

                    ui.add_space(8.0);
                    if !crate::exp::on() {
                        crate::exp::note(ui, tr!("Закрепление и откат версии", "Version pinning and rollback"));
                    }
                    egui::CollapsingHeader::new(tr!("Откатить на старую версию", "Roll back to an older version")).id_salt("downgrade").show(ui, |ui| {
                        ui.weak(
                            tr!("Steam делит игру на части (хранилища): основные файлы, языки, файлы для \
                             разных систем - у каждой своя история версий. Steam не хранит список старых \
                             версий, номер нужной (Manifest ID) есть на SteamDB: откройте ссылку, найдите \
                             дату и скопируйте число. Обычно хватает отката «Основных файлов игры». \
                             Пустое поле - часть не меняется.", "Steam splits a game into parts (depots): main files, languages, files for different systems - each has its own version history. Steam does not keep a list of old versions, the number you need (Manifest ID) is on SteamDB: open the link, find the date and copy the number. Rolling back the \"Main game files\" is usually enough. An empty field - the part does not change."),
                        );
                        ui.add_space(4.0);
                        // Real versions: a pinned game has "masking" ones written in its file.
                        let current = pinned_real.clone().unwrap_or_else(|| local::installed_depots(&acf_path));
                        if current.is_empty() {
                            ui.weak(tr!("В файле игры нет списка хранилищ.", "The game file has no depot list."));
                            return;
                        }
                        let sizes = local::installed_depot_sizes(&acf_path);
                        let descs = data.depots.as_ref().and_then(|r| r.as_ref().ok());
                        egui::Grid::new("dg_grid").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.weak(tr!("Часть игры (хранилище)", "Game part (depot)"));
                            ui.weak(tr!("Версия сейчас", "Current version"));
                            ui.weak(tr!("Нужная версия", "Wanted version"));
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
                                    .on_hover_text(tr!("История версий этого хранилища", "Version history of this depot"));
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
                            ui.colored_label(YELLOW, tr!("Номер версии - это только цифры.", "A version number has digits only."));
                        }
                        ui.add_space(4.0);
                        if ui
                            .add_enabled(
                                !locked && logged_in && !bad && !overrides.is_empty() && crate::exp::on(),
                                egui::Button::new(tr!("Скачать выбранную версию", "Download the selected version")),
                            )
                            .on_hover_text(tr!("Ход загрузки - внизу главного окна. После отката версия закрепится.", "The download progress is at the bottom of the main window. After the rollback the version is pinned."))
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
                            w.note = Some(tr!("Откат начат - ход загрузки внизу главного окна.", "Rollback started - the download progress is at the bottom of the main window.").into());
                        }
                    });
                }

                Tab::Files => {
                    ui.horizontal(|ui| {
                        ui.label(trf!("Папка: {}", "Folder: {}", game.dir.display()));
                        if ui.button(tr!("Открыть", "Open")).clicked() {
                            local::open_folder(&game.dir);
                        }
                    });
                    ui.label(trf!("Размер на диске: {}", "Size on disk: {}", net::fmt_bytes(disk_size)));
                    if acf.last_updated > 0 {
                        ui.label(trf!("Обновлена: {}", "Updated: {}", fmt_date(acf.last_updated)));
                    }
                    if acf.updating() {
                        ui.colored_label(YELLOW, tr!("Игре нужно обновление (или смена языка/беты ждет загрузки).", "The game needs an update (or a language/beta change waits for a download)."));
                    }
                    if ui
                        .add_enabled(!locked && pinned_real.is_none(), egui::Button::new(tr!("Обновить через Steam", "Update via Steam")))
                        .on_hover_text(tr!("Steam откроется незаметно, скачает обновление и закроется. Игра не запускается.", "Steam opens unnoticed, downloads the update and closes. The game does not start."))
                        .clicked()
                    {
                        do_update = true;
                    }
                    ui.add_space(10.0);

                    if ui
                        .add_enabled(!locked && pinned_real.is_none(), egui::Button::new(tr!("Проверить целостность файлов", "Verify integrity of game files")))
                        .on_hover_text(tr!("Сравнить файлы с серверами Steam и докачать испорченные. Ход проверки - внизу главного окна.", "Compare the files with Steam servers and re-download damaged ones. The progress is at the bottom of the main window."))
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
                        ui.weak(tr!("Перенос: у Steam одна папка библиотеки, переносить некуда.", "Move: Steam has a single library folder, nowhere to move."));
                    } else {
                        w.move_idx = w.move_idx.min(others.len() - 1);
                        ui.horizontal(|ui| {
                            ui.label(tr!("Перенести в:", "Move to:"));
                            egui::ComboBox::from_id_salt("move")
                                .selected_text(others[w.move_idx].display().to_string())
                                .show_ui(ui, |ui| {
                                    for (i, l) in others.iter().enumerate() {
                                        let free = local::free_space(l)
                                            .map(|f| trf!(" (свободно {})", " ({} free)", net::fmt_bytes(f)))
                                            .unwrap_or_default();
                                        ui.selectable_value(&mut w.move_idx, i, format!("{}{free}", l.display()));
                                    }
                                });
                            if ui.add_enabled(!locked, egui::Button::new(tr!("Перенести", "Move"))).clicked() {
                                cmds.push(Cmd::Move {
                                    appid: w.appid,
                                    name: w.name.clone(),
                                    dest: others[w.move_idx].clone(),
                                });
                            }
                        });
                    }

                    // Relink: the game files already lie in another library (moved manually).
                    if let Some(steam) = self.steam.clone() {
                        for lib in local::relink_candidates(&steam.dir, &game) {
                            ui.add_space(6.0);
                            ui.weak(trf!("Папка этой игры есть и в библиотеке {}.", "This game's folder is also in library {}.", lib.display()));
                            if ui
                                .add_enabled(!locked, egui::Button::new(tr!("Привязать игру к этой библиотеке", "Link the game to this library")))
                                .on_hover_text(
                                    tr!("Если вы перенесли файлы игры вручную: Steam и Vaporust будут брать игру\n\
                                     оттуда. Файлы не копируются и не удаляются, переносится только запись\n\
                                     об установке. Старую папку потом можно удалить вручную.", "If you moved the game files manually: Steam and Vaporust will take the game\nfrom there. Files are not copied or deleted, only the installation record\nis moved. You can delete the old folder manually later."),
                                )
                                .clicked()
                            {
                                w.note = Some(match local::relink_game(&steam.dir, &game, &lib) {
                                    Ok(p) => {
                                        self.rescan_needed.store(true, std::sync::atomic::Ordering::Relaxed);
                                        trf!("Готово: игра теперь в {}.", "Done: the game is now in {}.", p.display())
                                    }
                                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                                });
                            }
                        }
                    }

                    ui.add_space(14.0);
                    if !w.confirm_uninstall {
                        if ui.add_enabled(!locked, egui::Button::new(tr!("Удалить игру...", "Uninstall game..."))).clicked() {
                            w.confirm_uninstall = true;
                        }
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), trf!(
                            "Удалить «{}» с диска? Будет удалена папка игры ({}). Игра останется на аккаунте, \
                             сохранения в облаке Steam не пострадают.", "Remove \"{}\" from the disk? The game folder ({}) will be deleted. The game stays on the account, saves in the Steam cloud are not affected.",
                            w.name,
                            net::fmt_bytes(disk_size)
                        ));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!locked, egui::Button::new(tr!("Да, удалить", "Yes, uninstall"))).clicked() {
                                do_uninstall = true;
                                w.confirm_uninstall = false;
                            }
                            if ui.button(tr!("Отмена", "Cancel")).clicked() {
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
                        ui.label(trf!("Не удалось получить DLC: {e}", "Could not get DLC: {e}"));
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        ui.label(tr!("У этой игры нет DLC.", "This game has no DLC."));
                    }
                    Some(Ok(list)) => {
                        let owned = list.iter().filter(|d| d.owned).count();
                        ui.label(trf!("DLC: {}, куплено: {owned}", "DLC: {}, owned: {owned}", list.len()));
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
                                    trf!("Дополнение без названия (№ {})", "DLC without a name (No. {})", d.app_id)
                                });
                            ui.horizontal(|ui| {
                                if d.owned && d.installed {
                                    let mut on = !d.disabled;
                                    if ui.add_enabled(!locked, egui::Checkbox::new(&mut on, &name)).changed() {
                                        cmds.push(Cmd::SetDlc { base: w.appid, dlc: d.app_id, enabled: on });
                                    }
                                } else if d.owned {
                                    ui.label(&name);
                                    if ui.add_enabled(!locked, egui::Button::new(tr!("Установить", "Install"))).clicked() {
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
                                    ui.weak(tr!("- не куплено", "- not owned"));
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
                    // Only enabling/disabling subscribed mods; search is on the game page.
                    if !w.ws_loaded {
                        w.ws_loaded = true;
                        cmds.push(Cmd::Social(crate::social::SocialCmd::WorkshopSubscribed(w.appid)));
                    }
                    let ws = net.workshop.lock().unwrap();
                    crate::workshopui::subscriptions(ui, w.appid, &ws, &mut cmds);
                }

                Tab::Cloud => {
                    // ---------- Manual sync ----------
                    ui.strong(tr!("Синхронизация вручную", "Manual sync"));
                    ui.weak(
                        tr!("Steam сам синхронизирует сохранения при запуске игры и после выхода. Здесь это можно \
                         сделать вручную, не запуская игру. Сохранения лежат в папке игры на этом ПК (она указана \
                         ниже, над списком файлов).", "Steam syncs saves itself when the game starts and after it exits. Here you can do it manually without starting the game. Saves are in the game folder on this PC (shown below, above the file list)."),
                    );
                    ui.weak(
                        tr!("«Скачать из облака» - заменить файлы на ПК облачными (например, после переустановки \
                         Windows или игры). «Отправить в облако» - наоборот. «Синхронизировать» - новые файлы \
                         в обе стороны.", "\"Download from cloud\" - replace the files on the PC with the cloud ones (for example after reinstalling Windows or the game). \"Upload to cloud\" - the other way. \"Sync\" - new files in both directions."),
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
                        if ui.add_enabled(can, egui::Button::new(tr!("Скачать из облака", "Download from cloud"))).clicked() {
                            go = Some((0, None));
                        }
                        if ui.add_enabled(can, egui::Button::new(tr!("Отправить в облако", "Upload to cloud"))).clicked() {
                            go = Some((1, None));
                        }
                        if ui
                            .add_enabled(can, egui::Button::new(tr!("Синхронизировать", "Sync")))
                            .on_hover_text(tr!("Новое из облака - сюда, новое с диска - в облако", "New from the cloud - here, new from the disk - to the cloud"))
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
                                ui.colored_label(GREEN, tr!("Все уже совпадает: сохранения на ПК и в облаке одинаковые.", "Everything matches already: the saves on the PC and in the cloud are the same."));
                            } else {
                                ui.colored_label(
                                    GREEN,
                                    trf!("Скачано файлов: {}, отправлено: {}.", "Files downloaded: {}, uploaded: {}.", o.downloaded, o.uploaded),
                                );
                            }
                            if o.skipped > 0 {
                                ui.weak(trf!("Пропущено (неизвестная папка сохранений): {}", "Skipped (unknown save folder): {}", o.skipped));
                            }
                            for f in &o.failed {
                                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), f);
                            }
                            if !o.conflicts.is_empty() {
                                ui.colored_label(
                                    YELLOW,
                                    trf!(
                                        "Расходятся (изменены и у вас, и в облаке): {}. Какую копию оставить?", "Differ (changed both on your PC and in the cloud): {}. Which copy to keep?",
                                        o.conflicts.len()
                                    ),
                                );
                                for (name, local, cloud) in &o.conflicts {
                                    ui.weak(trf!(
                                        "{name}: у вас {}, в облаке {}", "{name}: yours {}, cloud {}",
                                        net::fmt_bytes(*local),
                                        net::fmt_bytes(*cloud)
                                    ));
                                }
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(can, egui::Button::new(tr!("Взять из облака", "Take from cloud"))).clicked() {
                                        go = Some((2, Some(true)));
                                    }
                                    if ui.add_enabled(can, egui::Button::new(tr!("Оставить мои", "Keep mine"))).clicked() {
                                        go = Some((2, Some(false)));
                                    }
                                });
                            }
                        }
                        Some(Err(e)) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), trf!("Не получилось: {e}", "Failed: {e}"));
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
                        ui.label(trf!("Не удалось получить список: {e}", "Could not get the list: {e}"));
                    }
                    Some(Ok(files)) if files.is_empty() => {
                        ui.label(tr!("В облаке Steam нет сохранений этой игры.", "The Steam cloud has no saves of this game."));
                    }
                    Some(Ok(files)) => {
                        let total: u64 = files.iter().map(|f| f.size).sum();
                        // Where these files live on this PC: %WinAppDataLocalLow% and the like -> real folders.
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
                                ui.label(tr!("Сохранения на этом ПК:", "Saves on this PC:"));
                                ui.monospace(b.display().to_string());
                                if ui.small_button(tr!("Открыть папку", "Open folder")).clicked() {
                                    let dir = if b.is_dir() { b.clone() } else { b.parent().unwrap_or(b).to_path_buf() };
                                    local::open_folder(&dir);
                                }
                            });
                        }
                        // Save backups (zip in Documents\Vaporust\Сохранения).
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(tr!("Резервные копии", "Backups"));
                            if ui
                                .button(tr!("Сделать копию", "Make a backup"))
                                .on_hover_text(tr!("Сложить сохранения с этого ПК в zip-архив (Документы\\Vaporust\\Сохранения)", "Put the saves from this PC into a zip archive (Documents\\Vaporust\\Сохранения)"))
                                .clicked()
                            {
                                let paths: Vec<std::path::PathBuf> = local.iter().flatten().cloned().collect();
                                w.note = Some(match crate::backup::backup(&w.name, &paths) {
                                    Ok((p, n)) => trf!("Копия сделана: {} (файлов: {n}).", "Backup made: {} (files: {n}).", p.display()),
                                    Err(e) => e,
                                });
                            }
                            if ui.small_button(tr!("Открыть папку копий", "Open the backups folder")).clicked() {
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
                                    ui.colored_label(YELLOW, tr!("Текущие сохранения будут заменены. Восстановить?", "The current saves will be replaced. Restore?"));
                                    if ui.add_enabled(!locked, egui::Button::new(tr!("Да", "Yes"))).clicked() {
                                        // First - a backup of the current state so it is possible to go back.
                                        // If the backup failed (no space, no access) - replace nothing:
                                        // otherwise the current saves would be lost for good.
                                        let paths: Vec<std::path::PathBuf> = local.iter().flatten().cloned().collect();
                                        let has_current = paths.iter().any(|p| p.is_file());
                                        let safety = if has_current { crate::backup::backup(&w.name, &paths).map(|_| true) } else { Ok(false) };
                                        w.note = Some(match safety {
                                            Err(e) => trf!("Не восстановлено: не удалось сначала сохранить копию текущих сохранений ({e}). Файлы игры не тронуты.", "Not restored: could not save a backup of the current saves first ({e}). The game files are untouched."),
                                            Ok(saved) => match crate::backup::restore(&zip) {
                                                Ok(n) if saved => trf!("Восстановлено файлов: {n}. Прежние сохранения - в новой копии сверху.", "Files restored: {n}. The previous saves are in the new backup at the top."),
                                                Ok(n) => trf!("Восстановлено файлов: {n}.", "Files restored: {n}."),
                                                Err(e) => e,
                                            },
                                        });
                                        w.restore_confirm = None;
                                    }
                                    if ui.button(tr!("Нет", "No")).clicked() {
                                        w.restore_confirm = None;
                                    }
                                } else if ui
                                    .add_enabled(!locked, egui::Button::new(tr!("Восстановить...", "Restore...")))
                                    .on_disabled_hover_text(tr!("Сначала закройте Steam и игру", "Close Steam and the game first"))
                                    .clicked()
                                {
                                    w.restore_confirm = Some(zip.clone());
                                }
                            });
                        }
                        ui.weak(
                            tr!("Steam при запуске игры может сам заменить файлы версией из облака. Чтобы оставить \
                             восстановленные, после восстановления нажмите выше «Отправить в облако».", "When the game starts, Steam may replace the files with the cloud version itself. To keep the restored ones, press \"Upload to cloud\" above after restoring."),
                        );
                        ui.separator();
                        ui.label(trf!("Файлов в облаке: {}, всего {}", "Files in the cloud: {}, total {}", files.len(), net::fmt_bytes(total)));
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
                                    Some(p) if p.is_file() => ui.weak(tr!("есть на диске", "on disk")),
                                    Some(_) => ui.colored_label(YELLOW, tr!("нет на диске", "not on disk")),
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
        if do_uninstall {
            let name = self.props.as_ref().map(|p| p.name.clone()).unwrap_or_default();
            self.uninstall_game(appid, name);
        }
        if close_steam {
            self.close_steam(ctx);
        }
        if do_update {
            // "Update via Steam" and a language change (other depots needed) - through Steam.
            self.start_update_steam(appid, ctx);
        }
        if do_update_smart {
            self.start_update(appid, ctx);
        }
        if back {
            self.props = None;
            self.view = crate::View::Library;
            self.lib_page = Some(appid);
            return;
        }
        if let Some(on) = pin_action {
            let res = if on { self.pin_game(appid) } else { self.unpin_game(appid) };
            if let Some(w) = self.props.as_mut() {
                w.note = Some(res.unwrap_or_else(|e| e));
            }
        }
    }
}

/// Common folder for all paths (the deepest common ancestor).
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
