#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod chatfmt;
mod chatwin;
mod colls;
#[cfg(all(windows, target_arch = "x86"))]
mod compat32;
mod covers;
mod effects;
mod direct;
mod icons;
mod invent;
mod invwin;
mod launchopts;
mod libview;
mod local;
mod net;
mod notices;
mod notify;
mod pinning;
mod portable;
mod schedule;
mod props;
mod settings;
mod settingsui;
mod shots;
mod popup;
mod social;
mod socialwin;
mod stats;
mod store;
mod storewin;
mod tray;
mod updater;
mod workshopui;

/// Что показано в центре главного окна.
#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Library,
    Store,
    Inventory,
    Workshop,
    Settings,
    Screenshots,
}

use eframe::egui;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use local::{Game, SteamInfo};
use net::{Cmd, NetState, Shared};

/// Особый «фильтр-коллекция»: игры семейной библиотеки Steam.
const FAMILY: &str = "__family";

/// Одна строка библиотеки: объединение установленных и купленных игр.
struct Row {
    appid: u32,
    name: String,
    /// Есть на диске (возможно, недокачана).
    installed: bool,
    /// Полностью установлена, можно играть.
    ready: bool,
    /// Нужно обновление (по пометке Steam или проверке через Aurelia).
    needs_update: bool,
    playtime_min: Option<u32>,
    /// Размер на диске, байт (0 — не установлена).
    size: u64,
    /// Последний запуск, unix-время (0 — неизвестно).
    last_played: u64,
    /// Игра из семейной библиотеки Steam (не куплена этим аккаунтом).
    family: bool,
    /// SteamID64 владельца семейной игры, если известен.
    owner: Option<u64>,
}

struct LauncherApp {
    // Локальный Steam
    steam: Option<SteamInfo>,
    games: Vec<Game>,
    launch_status: Arc<Mutex<String>>,
    launching: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// Задача со Steam закончилась: перечитать диск и проверить обновления.
    rescan_needed: Arc<AtomicBool>,
    /// Проверка обновлений уже отправлена для текущего входа.
    updates_checked: bool,
    /// Закреплённые игры, у которых маскировка версии уже обновлена для запуска.
    pin_ok: std::collections::HashSet<u32>,
    /// Закреплённые игры, для которых уже обновляли устаревшую маскировку (раз за сеанс).
    pin_stale_sent: std::collections::HashSet<u32>,
    /// Обновления Vaporust: состояние проверки/установки; полоса скрыта кнопкой «Позже».
    upd: updater::Shared,
    upd_hidden: bool,
    /// Проверку запустил пользователь (тогда показываем и «нет обновлений», и ошибки).
    upd_manual: bool,
    /// Найденные дубли установок (вкладка «Настройки»).
    duplicates: Option<Vec<(u32, String, Vec<std::path::PathBuf>)>>,
    /// Прокси сохранён (показать «действует после перезапуска»).
    proxy_note: bool,
    /// Текст поля «Скорость загрузки» (МБ/с); None — взять из настроек.
    limit_text: Option<String>,
    /// То же для скорости по расписанию.
    sched_text: Option<String>,
    /// Статус для друзей уже объявлен для текущего входа.
    persona_sent: bool,
    /// Коллекции уже запрошены для текущего входа.
    colls_requested: bool,
    /// Выбранная коллекция в библиотеке (None — все игры).
    collection: Option<String>,
    /// Открытая страница игры в библиотеке.
    lib_page: Option<u32>,
    /// Сколько непрочитанных было в прошлом кадре (для заголовка окна).
    last_unread: u32,
    /// Когда очередь последний раз запускала установку.
    queue_started: Option<std::time::Instant>,
    /// Когда последний раз проверяли, что прятать от Steam.
    stash_check: Option<std::time::Instant>,
    freeze_webhelper: bool,

    // Сеть (Aurelia)
    net: Shared,
    tx: tokio::sync::mpsc::UnboundedSender<Cmd>,

    // Интерфейс
    filter: String,
    only_installed: bool,
    login_user: String,
    login_pass: String,
    login_code: String,
    ach_open: Option<(u32, String)>,
    /// Открытое окно «Свойства игры».
    props: Option<props::PropsWin>,
    closing: Arc<AtomicBool>,
    /// Кэш «Steam запущен?» (опрос процессов не чаще раза в 2 с).
    steam_check: Option<(std::time::Instant, bool)>,

    // Друзья и чат
    show_friends: bool,
    friend_filter: String,
    /// Открытый чат (отдельное окно Windows).
    chat: chatwin::SharedChat,
    /// appid, названия которых уже запрошены.
    requested_apps: std::collections::HashSet<u32>,

    /// Обложки для плиточного вида (грузятся в фоне, только видимые).
    covers: covers::Covers,
    /// Поиск в Мастерской на странице игры.
    ws_ui: workshopui::WsUi,
    /// Открытое окно Мастерской: (appid, название игры).
    ws_open: Option<(u32, String)>,
    /// Раздел «Скриншоты» (открыт, пока Some).
    shots_win: Option<shots::ShotsWin>,
    /// Мастерская, магазин, инвентарь открыты отдельным окном (Ctrl+щелчок).
    ws_detached: bool,
    store_detached: bool,
    inv_detached: bool,
    /// Что показано в центре главного окна.
    view: View,
    /// Окна профиля, «Добавить друга», подтверждения удаления.
    social_ui: socialwin::SocialUi,
    /// Открытое окно «Магазин Steam».
    store_win: Option<storewin::StoreWin>,
    inv_win: Option<invwin::InvWin>,
    /// Остальные сохраняемые настройки (сортировка, статус, трей, звук).
    settings: settings::Settings,
    /// Когда игры запускались последний раз (из localconfig.vdf Steam).
    last_played: HashMap<u32, u64>,

    // Установка
    libraries: Vec<std::path::PathBuf>,
    lib_idx: usize,
    seen_disk_changed: u64,
    /// Когда последний раз перечитывали диск и было ли окно активно (для auto_rescan).
    last_rescan: std::time::Instant,
    was_focused: bool,
    /// Последняя проверка расписания загрузок.
    sched_check: Option<std::time::Instant>,
    /// Уведомления о друзьях, скидках и загрузках.
    notices: notices::Notices,
    /// Ограничение скорости, отправленное в Aurelia (КБ/с).
    sent_limit: Option<u64>,
    install_note: String,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        let ctx = &cc.egui_ctx;
        // Настройки прошлого запуска (или значения по умолчанию).
        let saved: settings::Settings = cc
            .storage
            .and_then(|s| eframe::get_value::<settings::Settings>(s, settings::KEY))
            .map(|mut s| {
                s.migrate();
                s
            })
            .unwrap_or_default();
        let net: Shared = Arc::new(Mutex::new(NetState::default()));
        let tx = net::spawn_worker(net.clone(), ctx.clone());
        let _ = tx.send(Cmd::Restore);
        // Полосы прокрутки всегда видны (по умолчанию тонкие и появляются только при наведении).
        ctx.style_mut(|s| s.spacing.scroll = egui::style::ScrollStyle::solid());
        // Загрузчики картинок для чата (http + png/jpeg/gif/webp).
        egui_extras::install_image_loaders(ctx);

        // HWND главного окна — для мигания в панели задач и для трея.
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    notify::init(w.hwnd.get());
                }
            }
        }
        popup::set_sound(saved.notify_sound);
        popup::init(&cc.egui_ctx);
        tray::set_close_to_tray(saved.close_to_tray);
        tray::start();

        let mut app = Self {
            steam: None,
            games: Vec::new(),
            launch_status: Arc::new(Mutex::new(String::new())),
            launching: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            rescan_needed: Arc::new(AtomicBool::new(false)),
            updates_checked: false,
            pin_ok: Default::default(),
            pin_stale_sent: Default::default(),
            upd: Default::default(),
            upd_hidden: false,
            upd_manual: false,
            duplicates: None,
            proxy_note: false,
            limit_text: None,
            sched_text: None,
            persona_sent: false,
            colls_requested: false,
            collection: None,
            lib_page: None,
            last_unread: 0,
            queue_started: None,
            stash_check: None,
            // По умолчанию включено (по просьбе пользователя), дальше — как сохранено.
            freeze_webhelper: saved.freeze_webhelper,
            net,
            tx,
            filter: String::new(),
            only_installed: saved.only_installed,
            login_user: String::new(),
            login_pass: String::new(),
            login_code: String::new(),
            ach_open: None,
            props: None,
            closing: Arc::new(AtomicBool::new(false)),
            steam_check: None,
            show_friends: saved.show_friends,
            friend_filter: String::new(),
            chat: Default::default(),
            requested_apps: Default::default(),
            libraries: Vec::new(),
            lib_idx: 0,
            seen_disk_changed: 0,
            last_rescan: std::time::Instant::now(),
            was_focused: true,
            install_note: String::new(),
            sched_check: None,
            notices: Default::default(),
            sent_limit: None,
            covers: covers::Covers::new(ctx, local::find_steam().map(|s| s.dir)),
            social_ui: Default::default(),
            ws_ui: Default::default(),
            ws_open: None,
            shots_win: None,
            ws_detached: false,
            store_detached: false,
            inv_detached: false,
            view: View::Library,
            store_win: None,
            inv_win: None,
            settings: saved,
            last_played: HashMap::new(),
        };
        // Обновления: убрать файл старой версии и проверить GitHub (если включено).
        updater::cleanup_old();
        if app.settings.check_updates {
            updater::check(&app.upd, ctx);
        }
        app.rescan();
        // Выбранная в прошлый раз папка библиотеки.
        if let Some(lib) = app.settings.library.clone() {
            if let Some(i) = app.libraries.iter().position(|l| l.to_string_lossy() == lib) {
                app.lib_idx = i;
            }
        }
        app
    }

    /// Перечитать диск без кнопки: при возврате в окно и раз в 30 с, пока окно активно
    /// (игры могли поставить или удалить через Steam). Статус внизу меняем, только если
    /// число игр изменилось, чтобы не затирать сообщения вроде «Готово.».
    fn auto_rescan(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let regained = focused && !self.was_focused;
        self.was_focused = focused;
        let due = focused && self.last_rescan.elapsed() >= std::time::Duration::from_secs(30);
        if !(regained || due) || self.launching.load(Ordering::Relaxed) {
            return;
        }
        let before = self.games.len();
        let status = self.launch_status.lock().unwrap().clone();
        self.rescan();
        if self.games.len() == before {
            *self.launch_status.lock().unwrap() = status;
        }
    }

    fn rescan(&mut self) {
        self.last_rescan = std::time::Instant::now();
        self.steam = local::find_steam();
        if let Some(s) = &self.steam {
            effects::set_steam_dir(&s.dir);
        }
        self.games = self
            .steam
            .as_ref()
            .map(|s| local::scan_games(&s.dir))
            .unwrap_or_default();
        // Закреплённые игры не предлагаем обновлять.
        for g in &mut self.games {
            if self.settings.pinned.contains_key(&g.appid) {
                g.needs_update = false;
            }
        }
        self.libraries = self
            .steam
            .as_ref()
            .map(|s| local::library_dirs(&s.dir))
            .unwrap_or_default();
        if self.lib_idx >= self.libraries.len() {
            self.lib_idx = 0;
        }
        self.last_played = self
            .steam
            .as_ref()
            .and_then(|s| launchopts::find_localconfig(&s.dir))
            .map(|p| launchopts::read_last_played(&p))
            .unwrap_or_default();
        if !self.launching.load(Ordering::Relaxed) {
            *self.launch_status.lock().unwrap() = match &self.steam {
                Some(_) => format!("Установлено игр: {}", self.games.len()),
                None => "Steam не найден в реестре".to_string(),
            };
        }
    }

    fn install(&mut self, appid: u32, name: String) {
        self.install_note.clear();
        // Пока Steam открыт, он сам может начать качать ту же игру, и файлы испортятся.
        if local::steam_running() {
            self.install_note = "Сначала закройте Steam (и игру, если она запущена), потом нажмите «Установить» ещё раз.".into();
            return;
        }
        // Недокачанную игру продолжаем в той же библиотеке, где она лежит.
        let library = self
            .games
            .iter()
            .find(|g| g.appid == appid)
            .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
            .or_else(|| self.libraries.get(self.lib_idx).cloned());
        let Some(library) = library else {
            self.install_note = "Не найдена папка библиотеки Steam.".into();
            return;
        };
        // Пока игра не докачана, она «наша»: на паузе её appmanifest прячется от Steam.
        if !self.settings.holding.contains(&appid) {
            self.settings.holding.push(appid);
        }
        let _ = self.tx.send(Cmd::Install {
            appid,
            name,
            library,
        });
    }

    /// Прятать от Steam недокачанные загрузки Vaporust (на паузе, отменённые, прерванные),
    /// иначе запущенный ради игры Steam сам начнёт их качать. Проверка раз в 3 с.
    fn guard_partials(&mut self, net: &NetState) {
        // Докачанные — больше не «наши», дальше ими занимается Steam.
        self.settings.holding.retain(|a| !self.games.iter().any(|g| g.appid == *a && g.ready));
        if self.settings.holding.is_empty()
            || self.stash_check.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3))
        {
            return;
        }
        self.stash_check = Some(std::time::Instant::now());
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) || self.steam_running() {
            return; // при открытом Steam файлы трогать нельзя
        }
        let active = net.install.as_ref().filter(|i| i.active()).map(|i| i.appid);
        let mut changed = false;
        for &appid in &self.settings.holding {
            if Some(appid) == active {
                continue;
            }
            let lib = self
                .games
                .iter()
                .find(|g| g.appid == appid && !g.ready && !g.stashed)
                .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()));
            if let Some(lib) = lib {
                changed |= local::stash_manifest(&lib, appid);
            }
        }
        if changed {
            self.rescan();
        }
    }

    /// Поставить игру в очередь установки (или сразу начать, если ничего не качается).
    fn enqueue(&mut self, appid: u32, name: String, net: &NetState) {
        if let Some(q) = self.settings.downloads.iter_mut().find(|q| q.appid == appid) {
            q.paused = false; // уже в очереди на паузе — «Докачать» снимает паузу
            return;
        }
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        let waiting = self.settings.downloads.iter().any(|q| !q.paused);
        let allowed = schedule::may_download(&self.settings);
        if !allowed {
            self.install_note = format!(
                "«{name}» в очереди: по расписанию загрузки идут с {} до {}.",
                schedule::hh(self.settings.sched_from),
                schedule::hh(self.settings.sched_to)
            );
        }
        if !busy && !waiting && !self.settings.queue_paused && allowed {
            self.install(appid, name);
            self.queue_started = Some(std::time::Instant::now());
        } else {
            self.settings.downloads.push(settings::QueueItem { appid, name, paused: false });
        }
    }

    /// Запустить следующую игру из очереди, когда загрузка свободна.
    fn run_queue(&mut self, net: &NetState) {
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        // Команда установки доходит до сетевого потока не мгновенно: пауза 3 с между запусками.
        let cooling = self.queue_started.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
        // Первая игра в очереди, которая не на паузе.
        let next = self.settings.downloads.iter().position(|q| !q.paused);
        if busy
            || cooling
            || self.settings.queue_paused
            || !schedule::may_download(&self.settings)
            || next.is_none()
            || net.account.is_none()
            || self.launching.load(Ordering::Relaxed)
            || self.steam_running()
        {
            return;
        }
        let q = self.settings.downloads.remove(next.unwrap());
        self.install(q.appid, q.name);
        self.queue_started = Some(std::time::Instant::now());
    }

    fn install_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.install_note.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), &self.install_note);
        }
        // Очередь установок: у каждой игры своя пауза, плюс пауза всей очереди.
        if !self.settings.downloads.is_empty() {
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.label("Очередь:");
                if !schedule::may_download(&self.settings) {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 170, 60),
                        format!("ждёт расписания (с {})", schedule::hh(self.settings.sched_from)),
                    );
                }
                let (label, hint) = if self.settings.queue_paused {
                    ("Продолжить очередь", "Снова качать игры из очереди по порядку.")
                } else {
                    ("Пауза очереди", "Не начинать следующие игры из очереди (текущая загрузка продолжится).")
                };
                if ui.button(label).on_hover_text(hint).clicked() {
                    self.settings.queue_paused = !self.settings.queue_paused;
                }
                for (i, q) in self.settings.downloads.iter_mut().enumerate() {
                    ui.separator();
                    if q.paused {
                        ui.weak(format!("{} (пауза)", q.name));
                        if ui.small_button("продолжить").on_hover_text("Продолжить эту игру").clicked() {
                            q.paused = false;
                        }
                    } else {
                        ui.label(&q.name);
                        if ui.small_button("пауза").on_hover_text("Пропускать эту игру, пока не продолжите").clicked() {
                            q.paused = true;
                        }
                    }
                    if ui.small_button("✖").on_hover_text("Убрать из очереди").clicked() {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                self.settings.downloads.remove(i);
            }
        }
        let Some(inst) = &net.install else {
            return;
        };
        let is_install = inst.done_text == "установлена.";
        ui.horizontal(|ui| {
            ui.strong(&inst.name);
            match &inst.result {
                None => {
                    ui.label(format!("— {}", inst.stage));
                    if is_install
                        && ui
                            .button("Пауза")
                            .on_hover_text("Поставить на паузу только эту игру: скачанное сохранится, «продолжить» в очереди докачает с того же места. Следующие игры из очереди начнут качаться.")
                            .clicked()
                    {
                        inst.abort.store(true, Ordering::Relaxed);
                        self.settings.downloads.retain(|q| q.appid != inst.appid);
                        self.settings.downloads.insert(
                            0,
                            settings::QueueItem { appid: inst.appid, name: inst.name.clone(), paused: true },
                        );
                    }
                    if ui.button("Отмена").clicked() {
                        inst.abort.store(true, Ordering::Relaxed);
                    }
                }
                Some(Ok(())) => {
                    ui.colored_label(egui::Color32::from_rgb(110, 200, 110), format!("— {}", inst.done_text));
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), format!("— {e}"));
                }
            }
            if inst.result.is_some() && ui.small_button("Скрыть").clicked() {
                self.net.lock().unwrap().install = None;
            }
        });
        if inst.result.is_none() && inst.total > 0 {
            let frac = (inst.done as f32 / inst.total as f32).clamp(0.0, 1.0);
            let mut text = format!(
                "{} из {} ({:.1}%)",
                net::fmt_bytes(inst.done),
                net::fmt_bytes(inst.total),
                frac * 100.0
            );
            if inst.speed > 1.0 {
                let left = inst.total.saturating_sub(inst.done) as f64 / inst.speed;
                text += &format!(
                    " · {}/с · осталось ~{}",
                    net::fmt_bytes(inst.speed as u64),
                    fmt_eta(left as u64)
                );
            }
            ui.add(egui::ProgressBar::new(frac).text(text));
        }
    }

    fn steam_running(&mut self) -> bool {
        let now = std::time::Instant::now();
        match self.steam_check {
            // Снимок процессов стоит ~5–7 мс — не чаще раза в 5 с (в покое это основная нагрузка).
            Some((t, v)) if now.duration_since(t) < std::time::Duration::from_secs(5) => v,
            _ => {
                let v = local::steam_running();
                self.steam_check = Some((now, v));
                v
            }
        }
    }

    fn close_steam(&mut self, ctx: &egui::Context) {
        let Some(steam) = self.steam.clone() else {
            return;
        };
        self.closing.store(true, Ordering::Relaxed);
        let (status, closing, ctx) = (self.launch_status.clone(), self.closing.clone(), ctx.clone());
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            local::close_steam(&steam, &set);
            set("Steam закрыт.");
            closing.store(false, Ordering::Relaxed);
        });
        self.steam_check = None;
    }

    fn start(&mut self, appid: u32, ctx: &egui::Context) {
        self.steam_task(appid, ctx, false);
    }

    /// Обновить игру через скрытый Steam (без запуска игры).
    /// «Обновить» в библиотеке: без Steam (Aurelia), если можно; иначе — через скрытый Steam.
    pub(crate) fn start_update(&mut self, appid: u32, ctx: &egui::Context) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        let acf = local::appmanifest_path(&game);
        let info = local::read_acf(&acf);
        // Ждёт смены языка (выбрано в «Свойствах») — Vaporust сменит его сам (switch_language).
        let switching = !info.user_language.is_empty()
            && !info.mounted_language.is_empty()
            && info.user_language != info.mounted_language;
        // Смена беты — без Steam: Aurelia качает версии выбранной ветки для установленных хранилищ.
        let beta = (info.user_beta != info.mounted_beta).then(|| info.user_beta.clone());
        let logged_in = self.net.lock().unwrap().account.is_some();
        // Игра в двух библиотеках: Aurelia берёт первую по алфавиту копию — может обновить не ту.
        let duplicate = self
            .steam
            .as_ref()
            .is_some_and(|s| local::find_duplicates(&s.dir).iter().any(|(id, _, _)| *id == appid));
        if self.settings.update_via_steam || duplicate || !logged_in || self.settings.pinned.contains_key(&appid) {
            self.steam_task(appid, ctx, true);
            return;
        }
        if self.steam_running() {
            self.install_note = "Сначала закройте Steam (кнопка «Закрыть Steam» внизу), потом нажмите «Обновить» ещё раз.".into();
            return;
        }
        if self.net.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
            self.install_note = "Сейчас идёт другая загрузка — обновите игру, когда она закончится.".into();
            return;
        }
        self.install_note.clear();
        if switching {
            // Сначала язык (бета, если тоже выбрана, — следующим нажатием «Обновить»).
            let _ = self.tx.send(Cmd::SetLanguage { appid, name: game.name.clone(), acf, lang: info.user_language.clone() });
            return;
        }
        let _ = self.tx.send(Cmd::UpdateDirect { appid, name: game.name.clone(), acf, beta });
    }

    /// Обновить через скрытый Steam (как раньше).
    pub(crate) fn start_update_steam(&mut self, appid: u32, ctx: &egui::Context) {
        self.steam_task(appid, ctx, true);
    }

    /// «Играть» или «Обновить»: обе задачи поднимают скрытый Steam, поэтому идут по одной.
    fn steam_task(&mut self, appid: u32, ctx: &egui::Context, update: bool) {
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            return;
        }
        // Игра без защиты Steam — запускаем напрямую (галочка в «Свойствах»).
        if !update && self.settings.no_steam.contains(&appid) {
            let net = self.net.lock().unwrap().clone();
            self.direct_launch(appid, &net);
            return;
        }
        // Закреплённую версию нельзя обновлять, а перед запуском её надо подготовить.
        if self.settings.pinned.contains_key(&appid) {
            if update {
                *self.launch_status.lock().unwrap() = "Версия игры закреплена — открепите её в «Свойствах», чтобы обновить.".into();
                return;
            }
            let net = self.net.lock().unwrap().clone();
            if !self.pin_gate(appid, &net) {
                return;
            }
        }
        let Some(steam) = self.steam.clone() else {
            return;
        };
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else {
            return;
        };
        let opts = local::LaunchOptions {
            freeze_webhelper: self.freeze_webhelper,
            steam_args: self.settings.steam_args.clone(),
        };
        self.launching.store(true, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        let status = self.launch_status.clone();
        let launching = self.launching.clone();
        let cancel = self.cancel.clone();
        let rescan = self.rescan_needed.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            let res = if update {
                local::update_via_steam(&steam, &game, opts, &cancel, &set)
            } else {
                local::run_chain(&steam, &game, opts, &cancel, &set)
            };
            match res {
                Ok(msg) => set(&msg),
                Err(e) => set(&e),
            }
            launching.store(false, Ordering::Relaxed);
            // Steam мог обновить игры — перечитать диск и заново проверить обновления.
            rescan.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    /// Все игры (купленные и установленные) без фильтров и сортировки.
    fn row_map(&self, net: &NetState) -> HashMap<u32, Row> {
        let mut map: HashMap<u32, Row> = HashMap::new();
        for g in &net.owned {
            map.insert(
                g.app_id,
                Row {
                    appid: g.app_id,
                    name: g.name.clone(),
                    installed: false,
                    ready: false,
                    needs_update: false,
                    playtime_min: Some(g.playtime_forever_minutes),
                    size: 0,
                    last_played: 0,
                    family: false,
                    owner: None,
                },
            );
        }
        for g in &self.games {
            map.entry(g.appid)
                .and_modify(|r| {
                    r.installed = true;
                    r.ready = g.ready;
                    r.needs_update = g.needs_update;
                    r.size = g.size;
                })
                .or_insert(Row {
                    appid: g.appid,
                    name: g.name.clone(),
                    installed: true,
                    ready: g.ready,
                    needs_update: g.needs_update,
                    playtime_min: None,
                    size: g.size,
                    last_played: 0,
                    family: false,
                    owner: None,
                });
        }
        // Семейная библиотека: чужие игры, которых нет среди своих.
        let own: std::collections::HashSet<u32> = net.owned.iter().map(|g| g.app_id).collect();
        for f in &net.family {
            if own.contains(&f.app_id) {
                continue;
            }
            let r = map.entry(f.app_id).or_insert_with(|| Row {
                appid: f.app_id,
                name: f.name.clone(),
                installed: false,
                ready: false,
                needs_update: false,
                playtime_min: None,
                size: 0,
                last_played: 0,
                family: false,
                owner: None,
            });
            r.family = true;
            r.owner = f.owner_steamid;
        }
        for r in map.values_mut() {
            r.last_played = self.last_played.get(&r.appid).copied().unwrap_or(0);
        }
        map
    }

    /// Строки библиотеки с фильтрами (поиск, «только установленные», коллекция) и сортировкой.
    fn rows(&self, net: &NetState) -> Vec<Row> {
        let map = self.row_map(net);
        let colls = net.collections.as_deref().unwrap_or(&[]);
        let hidden: std::collections::HashSet<u32> = colls
            .iter()
            .find(|c| c.id == "hidden")
            .map(|c| c.apps.iter().copied().collect())
            .unwrap_or_default();
        let selected = self.collection.as_ref().and_then(|id| colls.iter().find(|c| &c.id == id));
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<Row> = map
            .into_values()
            .filter(|r| !self.only_installed || r.installed)
            // Коллекция: выбранная — только её игры; «все игры» — без «Скрытых», как в Steam.
            // «Семейная библиотека» — только игры семьи; в «Все игры» — по галочке в настройках.
            .filter(|r| match (self.collection.as_deref(), selected) {
                (Some(FAMILY), _) => r.family,
                (_, Some(c)) => c.apps.contains(&r.appid),
                _ => !hidden.contains(&r.appid) && (self.settings.show_family || !r.family),
            })
            .filter(|r| r.name.to_lowercase().contains(&filter))
            .collect();
        let by_name = |a: &Row, b: &Row| a.name.to_lowercase().cmp(&b.name.to_lowercase());
        match self.settings.sort {
            settings::Sort::Name => rows.sort_by(|a, b| b.installed.cmp(&a.installed).then(by_name(a, b))),
            settings::Sort::LastPlayed => {
                rows.sort_by(|a, b| b.last_played.cmp(&a.last_played).then(b.installed.cmp(&a.installed)).then(by_name(a, b)))
            }
            settings::Sort::Playtime => rows.sort_by(|a, b| {
                b.playtime_min.unwrap_or(0).cmp(&a.playtime_min.unwrap_or(0)).then(by_name(a, b))
            }),
            settings::Sort::Size => rows.sort_by(|a, b| b.size.cmp(&a.size).then(by_name(a, b))),
        }
        rows
    }

    fn friends_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Снимок списка друзей: (id, имя, статус, игра, непрочитанные).
        let mut unknown_apps: Vec<u32> = Vec::new();
        let mut list: Vec<(u64, String, u32, Option<String>, u32)> = {
            let roster = net.roster.read().unwrap();
            roster
                .values()
                .filter(|f| f.relationship == 3) // 3 = друг (EFriendRelationship)
                .map(|f| {
                    // Название игры: от Steam, из своей библиотеки, из уже узнанных, иначе номер.
                    let game = f.game_name.clone().filter(|g| !g.is_empty()).or_else(|| {
                        let a = f.game_app_id.filter(|a| *a != 0)?;
                        let known = net
                            .app_names
                            .get(&a)
                            .cloned()
                            .or_else(|| net.owned.iter().find(|g| g.app_id == a).map(|g| g.name.clone()));
                        Some(known.unwrap_or_else(|| {
                            unknown_apps.push(a);
                            format!("игра {a}")
                        }))
                    });
                    (
                        f.steam_id,
                        f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string()),
                        f.persona_state.unwrap_or(0),
                        game,
                        net.chats.get(&f.steam_id).map_or(0, |c| c.unread),
                    )
                })
                .collect()
        };
        // Неизвестные названия запрашиваем один раз.
        unknown_apps.retain(|a| self.requested_apps.insert(*a));
        if !unknown_apps.is_empty() {
            let _ = self.tx.send(Cmd::ResolveApps(unknown_apps));
        }
        let online = list.iter().filter(|f| f.2 != 0).count();
        // Порядок: с новыми сообщениями, в игре, в сети, не в сети; внутри — по имени.
        let rank = |f: &(u64, String, u32, Option<String>, u32)| {
            if f.4 > 0 {
                0
            } else if f.3.is_some() {
                1
            } else if f.2 != 0 {
                2
            } else {
                3
            }
        };
        list.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));

        ui.heading("Друзья");
        // Мой профиль, «Добавить друга», заявки в друзья.
        self.friends_header(ui, net);
        if list.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Загружаю список...");
            });
            return;
        }
        ui.weak(format!("В сети: {online} из {}", list.len()));
        ui.add(
            egui::TextEdit::singleline(&mut self.friend_filter)
                .hint_text("поиск")
                .desired_width(f32::INFINITY),
        );
        ui.separator();
        let filter = self.friend_filter.to_lowercase();
        let mut open = None;
        let mut friend_act = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (id, name, state, game, unread) in &list {
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                    continue;
                }
                let (color, status) = match (state, game) {
                    (_, Some(g)) => (egui::Color32::from_rgb(140, 200, 80), g.clone()),
                    (0, _) => (egui::Color32::GRAY, "не в сети".into()),
                    (1, _) => (egui::Color32::from_rgb(90, 170, 230), "в сети".into()),
                    (2, _) => (egui::Color32::from_rgb(220, 120, 110), "не беспокоить".into()),
                    (3 | 4, _) => (egui::Color32::from_rgb(110, 140, 170), "отошёл".into()),
                    _ => (egui::Color32::from_rgb(90, 170, 230), "в сети".into()),
                };
                let mut title = egui::RichText::new(name).color(if *state == 0 { egui::Color32::GRAY } else { color });
                if *unread > 0 {
                    title = title.strong();
                }
                // Вся строка (имя + статус) — одна кнопка на всю ширину панели.
                let block = ui
                    .vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(title);
                            if *unread > 0 {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("● {unread}"));
                            }
                        });
                        ui.label(egui::RichText::new(status).small().color(color));
                    })
                    .response;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), block.rect.top() - 2.0),
                    egui::pos2(ui.max_rect().right(), block.rect.bottom() + 2.0),
                );
                let resp = ui
                    .interact(rect, ui.id().with(("friend", *id)), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                    );
                }
                if resp.clicked() {
                    open = Some((*id, name.clone()));
                }
                // Правый щелчок: профиль, написать, удалить из друзей.
                socialwin::friend_menu(&resp, *id, name, &mut friend_act);
                ui.add_space(4.0);
            }
        });
        let ctx = ui.ctx().clone();
        if let Some((id, name)) = open {
            self.open_chat(&ctx, id, name, net);
        }
        if let Some(a) = friend_act {
            self.friend_action(a, net, &ctx);
        }
    }

    fn open_chat(&mut self, ctx: &egui::Context, id: u64, name: String, net: &NetState) {
        if !net.chats.get(&id).is_some_and(|c| c.history_loaded) {
            let _ = self.tx.send(Cmd::OpenChat(id));
        }
        // Тот же друг — просто оставляем окно; другой — переключаем окно на него.
        let mut chat = self.chat.lock().unwrap();
        if chat.as_ref().map_or(true, |c| c.id != id) {
            *chat = Some(chatwin::ChatWin::new(id, name));
        }
        drop(chat);
        // Если окно чата уже открыто, вынести его на передний план (команда несуществующему окну
        // создала бы пустое «egui window»).
        if notify::chat_open() {
            ctx.send_viewport_cmd_to(chatwin::viewport_id(), egui::ViewportCommand::Focus);
        }
    }

    /// «Уже скачана...»: выбрать папку игры в steamapps\common и добавить её в Steam.
    fn import_game(&mut self, appid: u32, name: String) {
        if self.steam_running() {
            *self.launch_status.lock().unwrap() = "Закройте Steam, чтобы добавить игру (кнопка «Закрыть Steam» внизу).".into();
            return;
        }
        let Some(dir) = rfd::FileDialog::new()
            .set_title(format!("Папка с файлами «{name}» (внутри steamapps\\common)"))
            .pick_folder()
        else {
            return;
        };
        // Ожидаем <библиотека>\steamapps\common\<папка игры>.
        let library = dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("common")))
            .and_then(|p| p.parent())
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps")))
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        match library {
            Some(library) => {
                let _ = self.tx.send(Cmd::Import { appid, name, library });
            }
            None => {
                *self.launch_status.lock().unwrap() = "Папка игры должна лежать в библиотеке Steam: …\\steamapps\\common\\<папка игры>. \
                     Перенесите её туда и попробуйте снова."
                    .into();
            }
        }
    }

    /// Раздел вместо библиотеки: магазин, инвентарь или Мастерская.
    fn section_view(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Раздел открыт отдельным окном — здесь только подсказка.
        let detached = match self.view {
            View::Store => self.store_detached.then_some("Магазин"),
            View::Inventory => self.inv_detached.then_some("Инвентарь"),
            View::Workshop => self.ws_detached.then_some("Мастерская"),
            View::Library | View::Settings | View::Screenshots => None,
        };
        if let Some(what) = detached {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(format!("{what} открыт в отдельном окне."));
                if ui.button("Показать здесь").clicked() {
                    match self.view {
                        View::Store => self.store_detached = false,
                        View::Inventory => self.inv_detached = false,
                        _ => self.ws_detached = false,
                    }
                }
            });
            return;
        }
        match self.view {
            View::Store => {
                if self.store_win.is_none() {
                    self.open_store(None, false);
                }
                self.store_contents(ui, net);
            }
            View::Inventory => {
                if self.inv_win.is_none() {
                    self.open_inventory(false);
                }
                self.inventory_contents(ui, net);
            }
            View::Workshop => {
                if ui.button("< К игре").clicked() {
                    self.view = View::Library;
                }
                self.workshop_contents(ui, net);
            }
            View::Settings => self.settings_contents(ui, net),
            View::Screenshots => self.shots_contents(ui, net),
            View::Library => {}
        }
    }

    fn account_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if net.account.is_some() {
            ui.horizontal(|ui| {
                // В левом углу — меню аккаунта.
                ui.menu_button("Аккаунт", |ui| {
                    if ui.button("Мой профиль").clicked() {
                        if let Some(me) = net.my_id {
                            self.open_profile(me, net);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Мои скриншоты").clicked() {
                        self.open_shots(None);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add_enabled(!net.busy, egui::Button::new("Выйти из аккаунта")).clicked() {
                        let _ = self.tx.send(Cmd::Logout);
                        ui.close_menu();
                    }
                });
                ui.separator();
                // Разделы: открываются в этом окне, с Ctrl — в отдельном.
                let ctrl = ui.input(|i| i.modifiers.ctrl);
                let hint = "Ctrl+щелчок — открыть в отдельном окне";
                if ui.selectable_label(self.view == View::Library, "Библиотека").clicked() {
                    self.view = View::Library;
                }
                if ui
                    .selectable_label(self.view == View::Store && !self.store_detached, "Магазин")
                    .on_hover_text(format!("Магазин Steam: поиск, скидки, список желаемого.\n{hint}"))
                    .clicked()
                {
                    self.open_store(None, ctrl);
                }
                if ui
                    .selectable_label(self.view == View::Inventory && !self.inv_detached, "Инвентарь")
                    .on_hover_text(format!("Инвентарь, кошелёк и торговая площадка Steam.\n{hint}"))
                    .clicked()
                {
                    self.open_inventory(ctrl);
                }
                if ui.selectable_label(self.view == View::Settings, "Настройки").clicked() {
                    self.view = View::Settings;
                }
                // Скриншоты — временная вкладка, пока открыты.
                if self.shots_win.is_some() {
                    ui.separator();
                    if ui.selectable_label(self.view == View::Screenshots, "Скриншоты").clicked() {
                        self.view = View::Screenshots;
                    }
                    if ui.small_button("✖").on_hover_text("Закрыть скриншоты").clicked() {
                        self.shots_win = None;
                        if self.view == View::Screenshots {
                            self.view = View::Library;
                        }
                    }
                }
                // Мастерская игры — временная вкладка, пока она открыта.
                if self.ws_open.is_some() && !self.ws_detached {
                    ui.separator();
                    if ui.selectable_label(self.view == View::Workshop, "Мастерская").clicked() {
                        self.view = View::Workshop;
                    }
                }

                // Справа (раскладка справа налево): «Друзья» в самом углу, левее — статус.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let unread: u32 = net.chats.values().map(|c| c.unread).sum();
                    let label = if unread > 0 { format!("Друзья ({unread} новых)") } else { "Друзья".to_string() };
                    ui.toggle_value(&mut self.show_friends, label);
                    // Статус для друзей; сохраняется и применяется при каждом входе.
                    let before = self.settings.persona;
                    egui::ComboBox::from_id_salt("persona")
                        .selected_text(format!("Статус: {}", settings::persona_label(before)))
                        .show_ui(ui, |ui| {
                            for (state, label) in settings::PERSONAS {
                                ui.selectable_value(&mut self.settings.persona, state, label);
                            }
                        })
                        .response
                        .on_hover_text("Как вас видят друзья, пока открыт Vaporust. «Невидимка»: вы видите их, они вас — нет.");
                    if self.settings.persona != before {
                        let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
                    }
                });
            });
        } else if net.need_login {
            ui.label("Вход в Steam (нужен для списка всех игр, времени в игре и достижений)");
            egui::Grid::new("login").num_columns(2).show(ui, |ui| {
                ui.label("Логин:");
                ui.text_edit_singleline(&mut self.login_user);
                ui.end_row();
                ui.label("Пароль:");
                ui.add(egui::TextEdit::singleline(&mut self.login_pass).password(true));
                ui.end_row();
                ui.label("Код Steam Guard:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.login_code)
                        .hint_text("если пришёл на почту или в приложение"),
                );
                ui.end_row();
            });
            ui.horizontal(|ui| {
                let can = !net.busy && !self.login_user.is_empty() && !self.login_pass.is_empty();
                if ui.add_enabled(can, egui::Button::new("Войти")).clicked() {
                    let code = self.login_code.trim().to_string();
                    let _ = self.tx.send(Cmd::Login {
                        user: self.login_user.trim().to_string(),
                        pass: std::mem::take(&mut self.login_pass),
                        code: (!code.is_empty()).then_some(code),
                    });
                    self.login_code.clear();
                }
                if ui
                    .add_enabled(!net.busy, egui::Button::new("Войти по QR-коду"))
                    .clicked()
                {
                    let _ = self.tx.send(Cmd::LoginQr);
                }
            });
            if let Some(url) = &net.qr_url {
                if net.qr_scanned {
                    ui.label("Код отсканирован, подтвердите вход в приложении Steam.");
                } else {
                    ui.label("Отсканируйте код в мобильном приложении Steam:");
                }
                draw_qr(ui, url);
            }
        }
        if !net.message.is_empty() {
            ui.label(&net.message);
        }
    }

    fn achievements_window(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some((appid, name)) = self.ach_open.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new(format!("Достижения: {name}"))
            .open(&mut open)
            .default_size([480.0, 420.0])
            .show(ctx, |ui| match net.achievements.get(&appid) {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.label(format!("Ошибка: {e}"));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label("У этой игры нет достижений.");
                }
                Some(Ok(list)) => {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    ui.label(format!("Получено {done} из {}", list.len()));
                    ui.separator();
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for a in list {
                            ui.horizontal(|ui| {
                                ui.label(if a.unlocked { "✔" } else { "✖" });
                                ui.vertical(|ui| {
                                    ui.strong(&a.name);
                                    if a.hidden && !a.unlocked && a.description.is_empty() {
                                        ui.weak("Скрытое достижение");
                                    } else if !a.description.is_empty() {
                                        ui.weak(&a.description);
                                    }
                                    ui.weak(format!("Есть у {:.1}% игроков", a.global_percent));
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            });
        if !open {
            self.ach_open = None;
        }
    }
}

fn draw_qr(ui: &mut egui::Ui, url: &str) {
    let Ok(code) = qrcode::QrCode::new(url.as_bytes()) else {
        ui.label(url);
        return;
    };
    let w = code.width();
    let colors = code.to_colors();
    let cell = 4.0;
    let quiet = 4;
    let size = (w + 2 * quiet) as f32 * cell;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    for y in 0..w {
        for x in 0..w {
            if colors[y * w + x] == qrcode::Color::Dark {
                let min = rect.min + egui::vec2((x + quiet) as f32 * cell, (y + quiet) as f32 * cell);
                painter.rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
}

fn fmt_hours(min: u32) -> String {
    if min < 60 {
        format!("{min} мин")
    } else {
        format!("{:.1} ч", min as f32 / 60.0)
    }
}

/// `:имя:` своих смайликов -> ːимяː, как их набирают в Steam.
fn emoticons_to_bbcode(text: &str, net: &NetState) -> String {
    let Some((emoticons, _)) = &net.emoticons else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for name in emoticons {
        // Steam ждёт смайлик как ːимяː (U+02D0); тег [emoticon] сервер вырезает.
        out = out.replace(&format!(":{name}:"), &format!("ː{name}ː"));
    }
    out
}

/// Нажата ли сейчас клавиша V (физически).
#[cfg(windows)]
fn key_v_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(0x56) as u16 & 0x8000) != 0 } // 0x56 = VK_V
}

#[cfg(not(windows))]
fn key_v_down() -> bool {
    false
}

/// Картинка из буфера обмена во временный PNG-файл. Подходит и картинка
/// (скриншот, «Копировать изображение» в браузере), и файл-картинка, скопированный в Проводнике.
/// Ok(None) — в буфере нет картинки.
fn clipboard_image_file() -> Result<Option<std::path::PathBuf>, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("Буфер обмена недоступен: {e}"))?;
    if let Ok(img) = cb.get_image() {
        let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
            .ok_or("Не удалось прочитать картинку из буфера обмена.")?;
        let path = std::env::temp_dir().join(format!(
            "vaporust_paste_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        rgba.save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| format!("Не удалось сохранить картинку из буфера: {e}"))?;
        return Ok(Some(path));
    }
    if let Ok(files) = cb.get().file_list() {
        let is_img = |p: &std::path::Path| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.to_lowercase().as_str()))
        };
        return Ok(files.into_iter().find(|p| is_img(p)));
    }
    Ok(None)
}

/// Один кусок сообщения чата: текст, ссылка, картинка, смайлик или стикер.
/// Картинки грузятся с серверов Steam при первом показе (egui_extras), пока грузятся — крутится значок.
fn draw_seg(ui: &mut egui::Ui, seg: chatfmt::Seg, anims: &mut anim::Cache, who: &str) {
    use chatfmt::Seg;
    match seg {
        Seg::Text(t) => {
            ui.label(t);
        }
        Seg::Link { url, label } => {
            ui.hyperlink_to(label, &url).on_hover_text(&url);
        }
        Seg::Emoticon(name) => {
            ui.add(egui::Image::new(chatfmt::emoticon_url(&name)).fit_to_exact_size(egui::vec2(30.0, 30.0)))
                .on_hover_text(format!(":{name}:"));
        }
        Seg::RoomEffect(kind) => {
            // Как в Steam: значок эффекта, «… запускает воздушные шарики!» и кнопка повтора.
            ui.add(egui::Image::new(effects::icon_url(&kind)).fit_to_exact_size(egui::vec2(28.0, 28.0)));
            ui.label(egui::RichText::new(effects::label(&kind, who)).italics());
            let known = effects::KNOWN.iter().any(|(n, _)| n.eq_ignore_ascii_case(&kind));
            if known && ui.small_button("Повторить эффект").clicked() {
                effects::request_replay(&kind);
            }
        }
        Seg::Sticker(name) => {
            // Точный размер (с max_size egui ужимает до высоты строки); APNG анимируем сами.
            anims
                .show(ui, &chatfmt::sticker_url(&name), egui::vec2(128.0, 128.0))
                .on_hover_text(format!("Стикер: {name}"));
        }
        Seg::Image { url, size } => {
            // Размер известен — сразу резервируем место, чтобы переписка не прыгала при загрузке.
            let max = egui::vec2(360.0, 280.0);
            // Показываем уменьшенную копию Steam (до 512 точек), а не оригинал: меньше памяти и работы видеокарте.
            let img = egui::Image::new(chatfmt::thumbnail_url(&url)).sense(egui::Sense::click());
            let img = match size {
                Some((w, h)) => {
                    let k = (max.x / w).min(max.y / h).min(1.0);
                    img.fit_to_exact_size(egui::vec2(w * k, h * k))
                }
                // Размер неизвестен: квадрат по меньшей стороне, картинка впишется в него.
                None => img.fit_to_exact_size(egui::vec2(max.y, max.y)),
            };
            let resp = ui
                .add(img)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Открыть в браузере в полном размере");
            if resp.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
            }
        }
    }
}

/// Дата последнего запуска: «сегодня», «вчера» или «03.11.2026».
fn fmt_day(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    let Some(t) = Local.timestamp_opt(unix as i64, 0).single() else {
        return String::new();
    };
    let days = (Local::now().date_naive() - t.date_naive()).num_days();
    match days {
        0 => "сегодня".into(),
        1 => "вчера".into(),
        _ => t.format("%d.%m.%Y").to_string(),
    }
}

/// Время сообщения: «14:05» сегодня, «03.11 14:05» раньше.
fn fmt_time(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(unix as i64, 0).single() {
        Some(t) if t.date_naive() == Local::now().date_naive() => t.format("%H:%M").to_string(),
        Some(t) => t.format("%d.%m %H:%M").to_string(),
        None => String::new(),
    }
}

fn fmt_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("{} ч {} мин", secs / 3600, secs % 3600 / 60)
    } else if secs >= 60 {
        format!("{} мин", secs / 60)
    } else {
        format!("{secs} с")
    }
}

impl eframe::App for LauncherApp {
    /// eframe вызывает при выходе и периодически (раз в 30 с).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.freeze_webhelper = self.freeze_webhelper;
        self.settings.only_installed = self.only_installed;
        self.settings.show_friends = self.show_friends;
        self.settings.library = self
            .libraries
            .get(self.lib_idx)
            .map(|l| l.to_string_lossy().into_owned());
        eframe::set_value(storage, settings::KEY, &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        // Снимок сетевого состояния на кадр, чтобы не держать блокировку.
        let mut net = self.net.lock().unwrap().clone();

        // Непрочитанные сообщения: число в заголовке окна; когда всё прочитано — убрать точку в трее.
        let unread: u32 = net.chats.values().map(|c| c.unread).sum();
        if unread != self.last_unread {
            self.last_unread = unread;
            let base = concat!("Vaporust ", env!("CARGO_PKG_VERSION"));
            let title = if unread > 0 { format!("({unread}) {base}") } else { base.to_string() };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            if unread == 0 {
                tray::set_unread(false);
            }
        }
        // Галочка «Крестик сворачивает в трей» живёт в меню трея; сохраняем её в настройках.
        self.settings.close_to_tray = tray::close_to_tray();
        // Крестик прячет окно в трей, если так настроено (кроме «Выход» из меню трея).
        if ctx.input(|i| i.viewport().close_requested())
            && tray::close_to_tray()
            && !tray::exit_requested()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            tray::hide_main();
        }

        self.auto_rescan(ctx);
        // Установка закончилась (успешно или нет) — перечитываем диск.
        if net.disk_changed != self.seen_disk_changed {
            self.seen_disk_changed = net.disk_changed;
            self.rescan();
        }
        // Steam закончил игру или обновление — перечитать диск и перепроверить обновления.
        if self.rescan_needed.swap(false, Ordering::Relaxed) {
            self.rescan();
            self.updates_checked = false;
        }
        // Коллекции Steam — один раз после входа.
        if net.account.is_none() {
            self.colls_requested = false;
        } else if !self.colls_requested {
            self.colls_requested = true;
            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Load));
        }
        // Сохранённый статус для друзей — после каждого входа.
        if net.account.is_none() {
            self.persona_sent = false;
        } else if !self.persona_sent {
            self.persona_sent = true;
            let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
            self.send_aurelia_config();
            // Свои эффекты комнаты: сразу — из сохранённого списка, свежий — из инвентаря в фоне.
            if net.chat_effects.is_none() {
                self.net.lock().unwrap().chat_effects = Some(self.settings.chat_effects.clone());
            }
            let _ = self.tx.send(Cmd::LoadChatEffects);
        }
        if let Some(e) = net.chat_effects.as_ref().filter(|e| **e != self.settings.chat_effects) {
            self.settings.chat_effects = e.clone();
        }
        // Проверка обновлений один раз после входа (и после работы Steam).
        if net.account.is_none() {
            self.updates_checked = false;
        } else if !self.updates_checked && !self.games.is_empty() {
            self.updates_checked = true;
            let list = self.games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
            let _ = self.tx.send(Cmd::CheckUpdates(list));
        }
        // Закреплённые версии: запуск после подготовки, откат, устаревшая маскировка.
        self.pin_tick(ctx, &mut net);
        // Запуск без Steam: пришла таблица запуска.
        self.direct_tick();
        let installing = net.install.as_ref().is_some_and(|i| i.active());
        // Перерисовка: во время загрузки или запуска игры — раз в секунду (полоса прогресса, статус),
        // в покое — раз в 5 с (кнопка «Закрыть Steam», если Steam запустили вручную).
        let busy = installing || self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
        ctx.request_repaint_after(std::time::Duration::from_secs(if busy { 1 } else { 5 }));

        if !notify::in_tray() {
            egui::TopBottomPanel::top("account").show(ctx, |ui| {
                ui.add_space(4.0);
                self.account_panel(ui, &net);
                ui.add_space(4.0);
            });
            self.update_banner(ctx);

            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.add_space(2.0);
                self.install_panel(ui, &net);
                ui.add_space(2.0);
                let steam_running = self.steam_running();
                ui.horizontal(|ui| {
                    let launching = self.launching.load(Ordering::Relaxed);
                    let closing = self.closing.load(Ordering::Relaxed);
                    if launching || closing {
                        ui.spinner();
                    }
                    if launching && !closing {
                        if ui
                            .button("Перестать следить")
                            .on_hover_text(
                                "Запасной выход, если лаунчер не заметил закрытия игры.\n\
                                 Лаунчер перестаёт ждать, а Steam остаётся открытым:\n\
                                 его можно закрыть кнопкой «Закрыть Steam».",
                            )
                            .clicked()
                        {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    } else if steam_running && !closing {
                        if ui
                            .button("Закрыть Steam")
                            .on_hover_text("Закрывает Steam (если он завис — принудительно).")
                            .clicked()
                        {
                            self.close_steam(ctx);
                        }
                    }
                    ui.label(self.launch_status.lock().unwrap().clone());
                });
                // Заморозка steamwebhelper и прочие галочки — в «Настройках» (меню «Аккаунт»).
                ui.add_space(2.0);
            });

            if net.account.is_some() && self.show_friends {
                egui::SidePanel::right("friends")
                    .resizable(true)
                    .default_width(230.0)
                    .show(ctx, |ui| self.friends_panel(ui, &net));
            }

            let ctrl = ctx.input(|i| i.modifiers.ctrl);
            egui::CentralPanel::default().show(ctx, |ui| {
                // Магазин, инвентарь, Мастерская — вместо библиотеки, в этом же окне.
                if self.view != View::Library {
                    self.section_view(ui, &net);
                    return;
                }
                // Одна строка: поиск и «только установленные» слева, коллекция и сортировка справа.
                // (Вид «Плитка / Список» и «Обновить библиотеку» — в «Настройках».)
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let hint = if net.owned.is_empty() {
                        "Поиск по названию".to_string()
                    } else {
                        format!("Поиск среди {} игр", net.owned.len())
                    };
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(hint).desired_width(260.0));
                    ui.checkbox(&mut self.only_installed, "Только установленные");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("sort")
                        .selected_text(format!("Сортировка: {}", self.settings.sort.label()))
                        .show_ui(ui, |ui| {
                            for s in settings::Sort::ALL {
                                ui.selectable_value(&mut self.settings.sort, s, s.label());
                            }
                        });
                    // Коллекции Steam (правый щелчок по игре — добавить в коллекцию).
                    if let Some(colls) = net.collections.as_ref() {
                        let current = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id))
                            .map(|c| c.name.clone())
                            .or_else(|| (self.collection.as_deref() == Some(FAMILY)).then(|| "Семейная библиотека".to_string()))
                            .unwrap_or_else(|| "Все игры".into());
                        let family_n = self.row_map(&net).values().filter(|r| r.family).count();
                        egui::ComboBox::from_id_salt("collection")
                            .selected_text(format!("Коллекция: {current}"))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.collection, None, "Все игры");
                                if family_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(FAMILY.to_string()),
                                        format!("Семейная библиотека ({family_n})"),
                                    )
                                    .on_hover_text("Игры, которыми с вами делятся члены семьи Steam");
                                }
                                for c in colls {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(c.id.clone()),
                                        format!("{} ({})", c.name, c.apps.len()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text("Коллекции из Steam. Добавить игру — правый щелчок по ней. «Скрытые» в «Все игры» не показываются.");
                    }
                    });
                });
                if let Some(e) = &net.collections_error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                }
                // Папка для установки новых игр выбирается в «Настройках».
                ui.separator();

                let launching =
                    self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
                let rows = self.rows(&net);
                // Список или плитка — по настройке; кнопки у обоих видов общие (libview).
                let queued: std::collections::HashSet<u32> =
                    self.settings.downloads.iter().filter(|q| !q.paused).map(|q| q.appid).collect();
                let vc = libview::Ctx {
                    launching,
                    installing,
                    logged_in: net.account.is_some(),
                    net: &net,
                    queued: &queued,
                    sort: self.settings.sort,
                    collections: net.collections.as_deref().unwrap_or(&[]),
                    update_via_steam: self.settings.update_via_steam,
                };
                let page_row = self.lib_page.and_then(|id| self.row_map(&net).remove(&id));
                let action = if let Some(r) = &page_row {
                    // Страница игры: новости и онлайн — из данных магазина, достижения — из сети.
                    let store = net.store.lock().unwrap();
                    let ex = store.extras.get(&r.appid);
                    let data = libview::PageData {
                        achievements: net.achievements.get(&r.appid),
                        news: ex.and_then(|e| e.news.as_ref()),
                        players: ex.and_then(|e| e.players),
                    };
                    libview::page(ui, r, &vc, &self.covers, &data)
                } else {
                    match self.settings.view {
                        settings::View::List => libview::list(ui, &rows, &vc),
                        settings::View::Grid => libview::grid(ui, &rows, &vc, &self.covers),
                    }
                };
                match action {
                    Some(libview::Action::Play(appid)) => self.start(appid, ctx),
                    Some(libview::Action::Update(appid)) => self.start_update(appid, ctx),
                    Some(libview::Action::Install(appid, name)) => self.enqueue(appid, name, &net),
                    Some(libview::Action::Props(appid, name)) => self.open_props(appid, name),
                    Some(libview::Action::Achievements(appid, name)) => {
                        if !net.achievements.contains_key(&appid) {
                            let _ = self.tx.send(Cmd::Achievements(appid));
                        }
                        self.ach_open = Some((appid, name));
                    }
                    Some(libview::Action::Page(appid)) => {
                        // Страница игры в библиотеке: заодно грузим достижения и новости/онлайн.
                        self.lib_page = Some(appid);
                        if net.account.is_some() && !net.achievements.contains_key(&appid) {
                            let _ = self.tx.send(Cmd::Achievements(appid));
                        }
                        let _ = self.tx.send(Cmd::Store(store::StoreCmd::Page(appid)));
                    }
                    Some(libview::Action::Back) => self.lib_page = None,
                    Some(libview::Action::Store(appid)) => self.open_store(Some(appid), ctrl),
                    Some(libview::Action::Workshop(appid, name)) => self.open_workshop(appid, name, ctrl),
                    Some(libview::Action::Shots(appid)) => self.open_shots(Some(appid)),
                    Some(libview::Action::Import(appid, name)) => self.import_game(appid, name),
                    Some(libview::Action::Collection { id, appid, add }) => {
                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Toggle { id, appid, add }));
                    }
                    Some(libview::Action::NewCollection { name, appid }) => {
                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Create { name, appid: Some(appid) }));
                    }
                    None => {}
                }
            });
        } else {
            // В трее окно за краем экрана: интерфейс библиотеки не рисуем (экономия процессора),
            // логика ниже (очередь, расписание, уведомления, чат) работает как обычно.
            egui::CentralPanel::default().show(ctx, |_| {});
        }
        self.schedule_tick(&net);
        self.notices_tick(&net);
        self.popups_ui(ctx);
        self.popup_clicks(ctx, &net);
        self.run_queue(&net);
        self.guard_partials(&net);

        self.achievements_window(ctx, &net);
        self.props_window(ctx, &net);
        self.store_window(ctx, &net);
        self.inventory_window(ctx, &net);
        self.social_windows(ctx, &net);
        self.workshop_window(ctx, &net);
        chatwin::show(ctx, &self.chat, &self.net, &self.tx);
        stats::frame(frame_start);
    }
}

fn main() -> eframe::Result<()> {
    // Портативный режим (portable.txt рядом с exe): данные — в папке VaporustData.
    let persistence_path = portable::init();
    let options = eframe::NativeOptions {
        persistence_path,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 600.0])
            // Не даём сжать окно до нечитаемого (и восстановить таким из сохранённых настроек).
            .with_min_inner_size([760.0, 460.0])
            // Постоянный идентификатор: по нему eframe хранит настройки (%APPDATA%Vaporust),
            // поэтому они не теряются при смене версии в заголовке.
            .with_app_id("Vaporust")
            .with_icon(icons::icon(icons::Kind::Launcher)),
        // Всегда открываться обычным окном 960x600 посередине экрана: размер и положение
        // из прошлого запуска не восстанавливаем (окно открывалось то крошечным, то на весь экран).
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        concat!("Vaporust ", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}
