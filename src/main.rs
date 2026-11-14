#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod anim;
mod autostart;
mod backup;
mod badges;
mod chatfmt;
mod chatwin;
mod colls;
mod community;
#[cfg(all(windows, target_arch = "x86"))]
mod compat32;
mod covers;
mod effects;
mod groupchat;
mod errlog;
mod export;
mod family;
mod feed;
mod direct;
mod icons;
mod fonts;
mod imgload;
mod winframe;
mod invent;
mod invwin;
mod launchopts;
mod libview;
mod local;
mod net;
mod newsfeed;
mod notices;
mod notify;
mod pinning;
mod points;
mod portable;
mod schedule;
mod scripts;
mod profile_extra;
mod props;
mod refund;
mod richtext;
mod sessions;
mod settings;
mod settingsui;
mod shotcomments;
mod shots;
mod popup;
mod keys;
mod social;
mod socialwin;
mod stats;
mod stats_ui;
mod steamnotif;
mod storage;
mod store;
mod spin;
mod storewin;
mod tour;
mod theme;
mod nav;
mod custom;
mod friendspage;
mod downloads;
mod tray;
mod uninstall;
mod updater;
mod webapi;
mod workshopui;

/// Что показано в центре главного окна.
#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Library,
    Profile,
    Friends,
    Downloads,
    Store,
    Inventory,
    Workshop,
    Settings,
    Screenshots,
    Storage,
    News,
    Badges,
    Points,
    Family,
    Activity,
    Stats,
    Community,
    Groups,
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
/// Особая «коллекция»: свои игры, которые ни разу не запускались.
const UNPLAYED: &str = "__unplayed";

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
    /// Размер на диске, байт (0 - не установлена).
    size: u64,
    /// Последний запуск, unix-время (0 - неизвестно).
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
    /// Закрепленные игры, у которых маскировка версии уже обновлена для запуска.
    pin_ok: std::collections::HashSet<u32>,
    /// Закрепленные игры, для которых уже обновляли устаревшую маскировку (раз за сеанс).
    pin_stale_sent: std::collections::HashSet<u32>,
    /// Обновления Vaporust: состояние проверки/установки; полоса скрыта кнопкой «Позже».
    upd: updater::Shared,
    upd_hidden: bool,
    /// Проверку запустил пользователь (тогда показываем и «нет обновлений», и ошибки).
    upd_manual: bool,
    /// Найденные дубли установок (вкладка «Настройки»).
    duplicates: Option<Vec<(u32, String, Vec<std::path::PathBuf>)>>,
    /// Прокси сохранен (показать «действует после перезапуска»).
    proxy_note: bool,
    /// Текст поля «Скорость загрузки» (МБ/с); None - взять из настроек.
    limit_text: Option<String>,
    /// То же для скорости по расписанию.
    sched_text: Option<String>,
    /// Статус для друзей уже объявлен для текущего входа.
    persona_sent: bool,
    /// Коллекции уже запрошены для текущего входа.
    colls_requested: bool,
    /// Выбранная коллекция в библиотеке (None - все игры).
    collection: Option<String>,
    /// Какая рамка окна уже применена (Some(true) - обычная Windows).
    pub(crate) frame_applied: Option<bool>,
    /// Обход разделов со снимками (VAPORUST_TOUR, для разработки).
    pub(crate) tour: Option<tour::Tour>,
    /// Раздел на прошлом кадре (для очистки кэша картинок при переходе).
    pub(crate) mem_place: Option<(View, Option<u32>)>,
    /// Открытая страница игры в библиотеке.
    pub(crate) lib_page: Option<u32>,
    /// Для какой страницы игры уже запрошены достижения и новости.
    pub(crate) page_loaded: Option<u32>,
    /// Сколько непрочитанных было в прошлом кадре (для заголовка окна).
    last_unread: u32,
    /// Когда очередь последний раз запускала установку.
    queue_started: Option<std::time::Instant>,
    /// Когда последний раз проверяли, что прятать от Steam.
    stash_check: Option<std::time::Instant>,
    freeze_webhelper: bool,

    // Сеть (Aurelia)
    net: Shared,
    /// Копия сетевого состояния для кадров и номер изменения, с которого она снята.
    net_snap: NetState,
    net_gen: u64,
    net_snap_at: std::time::Instant,
    /// Свой профиль запрошен (имя для верхней полосы).
    own_profile_asked: bool,
    /// Память уже отдана системе (окно в трее или идет игра) и когда последний раз.
    trimmed: bool,
    /// Окно добавления или правки сторонней игры.
    custom_edit: Option<custom::Edit>,
    /// Подтверждение удаления игры: (appid, название, размер на диске).
    pub(crate) uninstall_ask: Option<(u32, String, u64)>,
    /// Какую игру запускает Vaporust (ее состояние показывается на странице игры).
    status_app: Option<u32>,
    /// Последнее показанное состояние (новое - всплывающим уведомлением).
    status_seen: String,
    /// Страница «Друзья».
    friends_page: friendspage::FriendsPage,
    trim_at: std::time::Instant,
    tx: tokio::sync::mpsc::UnboundedSender<Cmd>,

    // Интерфейс
    filter: String,
    only_installed: bool,
    login_user: String,
    login_pass: String,
    login_code: String,
    ach_open: Option<(u32, String)>,
    ach_state: AchState,
    /// Открытое окно «Свойства игры».
    props: Option<props::PropsWin>,
    closing: Arc<AtomicBool>,
    /// Когда нажали «Закрыть игру» (appid, время): через 10 с можно закрыть принудительно.
    stop_asked: Option<(u32, std::time::Instant)>,
    /// Сколько раз подряд загрузка продолжалась после обрыва связи.
    resume_tries: u8,
    /// Сведения об аккаунте уже запрошены (карточка в «Настройках»).
    account_asked: bool,
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
    /// Вкладка «Хранилище» (пока открыта).
    storage_win: Option<storage::StorageWin>,
    /// Окно «Активировать ключ»: введенный текст (None - окно закрыто).
    key_input: Option<String>,
    key_refreshed: bool,
    /// Вкладка «Новости».
    news_ui: newsfeed::NewsUi,
    /// Уведомления Steam: когда спрашивали и какие уже видели (для всплывающих о новых).
    notifs_at: Option<std::time::Instant>,
    notifs_seen: std::collections::HashSet<String>,
    /// Вкладка «Значки».
    badges_ui: badges::BadgesUi,
    /// Вкладка «Очки Steam».
    points_ui: points::PointsUi,
    /// Вкладка «Семья».
    family_ui: family::FamilyUi,
    /// Вкладка «Активность друзей».
    activity_ui: activity::ActivityUi,
    /// Вкладка «Статистика» и запись истории времени в играх.
    stats_ui: stats_ui::StatsUi,
    /// Окно комментариев к скриншоту.
    shot_comments: Option<shotcomments::ShotComments>,
    /// Вкладка «Сообщество» игры.
    community_ui: community::CommunityUi,
    /// Вкладка «Групповые чаты».
    groups_ui: groupchat::GroupsUi,
    /// Запуск с --tray: спрятать окно в трей, как только оно появится.
    start_hidden: bool,
    /// Автозапуск с Windows (кэш значения из реестра для галочки).
    autostart_on: Option<bool>,
    /// Итог экспорта списка игр.
    export_note: Option<String>,
    /// Лицензии (даты покупок) уже запрошены.
    licenses_asked: bool,
    /// Список «Где выполнен вход» раскрыт.
    sessions_open: bool,
    /// Открытый раздел настроек (settingsui::SECTIONS).
    settings_section: u8,
    /// Контекст egui (для фоновых потоков, которым нужно разбудить окно).
    egui_ctx: egui::Context,
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
    /// Последняя попытка переподключиться в автономном режиме.
    offline_retry: Option<std::time::Instant>,
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
        let net: Shared = Arc::default();
        let tx = net::spawn_worker(net.clone(), ctx.clone());
        let _ = tx.send(Cmd::Restore);
        // Цвета в духе клиента Steam.
        theme::apply(ctx);
        // Полосы прокрутки всегда видны (по умолчанию тонкие и появляются только при наведении).
        ctx.style_mut(|s| {
            s.spacing.scroll = egui::style::ScrollStyle::solid();
            // Без вертушек на загружаемых картинках: они перерисовывают окно каждый кадр; загрузчик
            // сам перерисует окно, когда картинка придет.
            s.visuals.image_loading_spinners = false;
        });
        // Загрузчики картинок для чата (http + png/jpeg/gif/webp).
        egui_extras::install_image_loaders(ctx);
        // Свой загрузчик поверх: картинки уменьшаются и раскодируются в фоне (экономия памяти).
        imgload::install(ctx);
        // Запасные шрифты Windows для символов, которых нет во встроенных (иначе квадратики).
        fonts::install(ctx);

        // HWND главного окна - для мигания в панели задач и для трея.
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    notify::init(w.hwnd.get());
                    icons::set_taskbar_icon(w.hwnd.get());
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
            mem_place: None,
            page_loaded: None,
            // Окно создается без рамки: команду шлем, только если в настройках обычная рамка.
            frame_applied: Some(false),
            tour: tour::Tour::from_env(),
            last_unread: 0,
            queue_started: None,
            stash_check: None,
            // По умолчанию включено (по просьбе пользователя), дальше - как сохранено.
            freeze_webhelper: saved.freeze_webhelper,
            net,
            net_snap: NetState::default(),
            net_gen: u64::MAX,
            net_snap_at: std::time::Instant::now(),
            own_profile_asked: false,
            trimmed: false,
            custom_edit: None,
            uninstall_ask: None,
            status_app: None,
            status_seen: String::new(),
            friends_page: Default::default(),
            trim_at: std::time::Instant::now(),
            tx,
            filter: String::new(),
            only_installed: saved.only_installed,
            login_user: String::new(),
            login_pass: String::new(),
            login_code: String::new(),
            ach_open: None,
            ach_state: AchState::default(),
            props: None,
            closing: Arc::new(AtomicBool::new(false)),
            stop_asked: None,
            resume_tries: 0,
            account_asked: false,
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
            offline_retry: None,
            notices: Default::default(),
            sent_limit: None,
            covers: covers::Covers::new(ctx, local::find_steam().map(|s| s.dir)),
            social_ui: Default::default(),
            ws_ui: Default::default(),
            ws_open: None,
            shots_win: None,
            storage_win: None,
            key_input: None,
            key_refreshed: false,
            news_ui: Default::default(),
            notifs_at: None,
            notifs_seen: Default::default(),
            badges_ui: Default::default(),
            points_ui: Default::default(),
            family_ui: Default::default(),
            activity_ui: Default::default(),
            stats_ui: Default::default(),
            shot_comments: None,
            community_ui: Default::default(),
            groups_ui: Default::default(),
            start_hidden: autostart::started_in_tray(),
            autostart_on: None,
            export_note: None,
            licenses_asked: false,
            sessions_open: false,
            settings_section: 0,
            egui_ctx: cc.egui_ctx.clone(),
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
        // Закрепленные игры не предлагаем обновлять.
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
                Some(_) => String::new(),
                None => "Steam не найден в реестре".to_string(),
            };
        }
    }

    fn install(&mut self, appid: u32, name: String) {
        self.install_note.clear();
        // Пока Steam открыт, он сам может начать качать ту же игру, и файлы испортятся.
        if local::steam_running() {
            self.install_note = "Сначала закройте Steam (и игру, если она запущена), потом нажмите «Установить» еще раз.".into();
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
        // Пока игра не докачана, она «наша»: на паузе ее appmanifest прячется от Steam.
        if !self.settings.holding.contains(&appid) {
            self.settings.holding.push(appid);
        }
        // Если Vaporust закроют посреди загрузки, при следующем запуске она продолжится.
        self.settings.current_download = Some(settings::QueueItem { appid, name: name.clone(), paused: false });
        let _ = self.tx.send(Cmd::Install {
            appid,
            name,
            library,
        });
    }

    /// Прятать от Steam недокачанные загрузки Vaporust (на паузе, отмененные, прерванные),
    /// иначе запущенный ради игры Steam сам начнет их качать. Проверка раз в 3 с.
    fn guard_partials(&mut self, net: &NetState) {
        // Докачанные - больше не «наши», дальше ими занимается Steam.
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
            q.paused = false; // уже в очереди на паузе - «Докачать» снимает паузу
            return;
        }
        // Пока открыт Steam, Vaporust не качает (Steam мог бы сам начать качать ту же игру):
        // игра встает в очередь и начнется сама, как только Steam закроют. Раньше нажатие
        // «Установить» в этом случае ничего видимого не делало.
        if self.steam_running() && !self.launching.load(Ordering::Relaxed) {
            self.settings.downloads.push(settings::QueueItem { appid, name: name.clone(), paused: false });
            popup::push(
                popup::Kind::Game(appid),
                "Загрузка ждет закрытия Steam",
                format!("«{name}» в очереди: начнется сама, когда вы закроете Steam (меню «Vaporust» - «Закрыть Steam»)."),
            );
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

    /// Загрузка закончилась: больше не «текущая». Если ее оборвала сеть (а не «Отмена» или
    /// «Пауза»), она встает первой в очередь и продолжится, когда связь вернется (до 3 раз подряд).
    fn finish_tick(&mut self, net: &NetState) {
        let Some(cur) = self.settings.current_download.clone() else { return };
        let Some(inst) = net.install.as_ref().filter(|i| i.appid == cur.appid) else { return };
        let Some(res) = &inst.result else { return };
        self.settings.current_download = None;
        let user_stop = inst.abort.load(Ordering::Relaxed);
        if let (Err(e), false) = (res, user_stop) {
            crate::errlog::write("загрузка", &format!("{}: {e}", cur.name));
        }
        match res {
            Err(e) if !user_stop && network_error(e) && self.resume_tries < 3 => {
                self.resume_tries += 1;
                if !self.settings.downloads.iter().any(|q| q.appid == cur.appid) {
                    self.settings.downloads.insert(0, cur.clone());
                }
                self.install_note = format!(
                    "Загрузка «{}» прервалась из-за связи: продолжится сама, когда Steam снова будет доступен.",
                    cur.name
                );
                // Не сразу: дать сети время вернуться.
                self.queue_started = Some(std::time::Instant::now() + std::time::Duration::from_secs(27));
            }
            Ok(()) => self.resume_tries = 0,
            _ => {}
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

    /// Сообщения о результате (удалена, закрыта, ошибка запуска) - всплывающим уведомлением:
    /// в нижней полосе состояния больше нет, а ход запуска виден на странице игры.
    fn status_popup(&mut self) {
        let st = self.launch_status.lock().unwrap().clone();
        if st == self.status_seen {
            return;
        }
        let busy = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
        // «Удаляю...», «Прошу закрыться...» - это ход дела, а не итог: не показываем.
        let in_progress = st.ends_with("...") || st.ends_with('…');
        if !busy && !in_progress && !st.is_empty() && st != "Готово." {
            // Игра из «кавычек» - ее картинка слева, щелчок открывает ее страницу.
            let game = st
                .split_once('«')
                .and_then(|(_, rest)| rest.split_once('»'))
                .map(|(name, _)| name)
                .and_then(|name| {
                    self.games
                        .iter()
                        .find(|g| g.name == name)
                        .map(|g| g.appid)
                });
            let kind = game.map_or(popup::Kind::Info, popup::Kind::Game);
            popup::push(kind, "Vaporust", st.clone());
        }
        self.status_seen = st;
    }

    /// Закрыть запущенную игру: мягко (как крестик окна) или принудительно.
    fn stop_running(&mut self, appid: u32, force: bool) {
        let Some((id, name, dir)) = local::RUNNING.lock().unwrap().clone() else { return };
        if id != appid {
            return;
        }
        let n = local::stop_game(&dir, force);
        if !force {
            self.stop_asked = Some((appid, std::time::Instant::now()));
        }
        *self.launch_status.lock().unwrap() = match (force, n > 0) {
            (true, _) => format!("«{name}» закрыта принудительно."),
            (false, true) => format!("Прошу «{name}» закрыться..."),
            (false, false) => "Процессы игры не найдены.".into(),
        };
    }

    fn steam_running(&mut self) -> bool {
        let now = std::time::Instant::now();
        match self.steam_check {
            // Снимок процессов стоит ~5-7 мс - не чаще раза в 5 с (в покое это основная нагрузка).
            Some((t, v)) if now.duration_since(t) < std::time::Duration::from_secs(5) => v,
            _ => {
                let v = local::steam_running();
                self.steam_check = Some((now, v));
                v
            }
        }
    }

    /// Автономный режим: снять флаг автономности Steam, когда Steam закрыт; без связи -
    /// раз в минуту пробовать войти снова.
    fn offline_tick(&mut self, net: &NetState) {
        if self.settings.steam_offline_set
            && !self.launching.load(Ordering::Relaxed)
            && !self.closing.load(Ordering::Relaxed)
            && !self.steam_running()
        {
            if let Some(s) = self.steam.clone() {
                if local::set_steam_offline(&s.dir, false).is_ok() {
                    self.settings.steam_offline_set = false;
                }
            }
        }
        if net.offline && net.account.is_none() && !net.busy {
            let due = self.offline_retry.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60));
            if due {
                self.offline_retry = Some(std::time::Instant::now());
                let _ = self.tx.send(Cmd::Restore);
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
        self.status_app = Some(appid);
        if custom::is_custom(appid) {
            self.launch_custom(appid);
            return;
        }
        self.steam_task(appid, ctx, false);
    }

    /// Обновить игру через скрытый Steam (без запуска игры).
    /// «Обновить» в библиотеке: без Steam (Aurelia), если можно; иначе - через скрытый Steam.
    pub(crate) fn start_update(&mut self, appid: u32, ctx: &egui::Context) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        let acf = local::appmanifest_path(&game);
        let info = local::read_acf(&acf);
        // Ждет смены языка (выбрано в «Свойствах») - Vaporust сменит его сам (switch_language).
        let switching = !info.user_language.is_empty()
            && !info.mounted_language.is_empty()
            && info.user_language != info.mounted_language;
        // Смена беты - без Steam: Aurelia качает версии выбранной ветки для установленных хранилищ.
        let beta = (info.user_beta != info.mounted_beta).then(|| info.user_beta.clone());
        let logged_in = self.net.lock().unwrap().account.is_some();
        // Игра в двух библиотеках: Aurelia берет первую по алфавиту копию - может обновить не ту.
        let duplicate = self
            .steam
            .as_ref()
            .is_some_and(|s| local::find_duplicates(&s.dir).iter().any(|(id, _, _)| *id == appid));
        if self.settings.update_via_steam || duplicate || !logged_in || self.settings.pinned.contains_key(&appid) {
            self.steam_task(appid, ctx, true);
            return;
        }
        if self.steam_running() {
            self.install_note = "Сначала закройте Steam (кнопка «Закрыть Steam» внизу), потом нажмите «Обновить» еще раз.".into();
            return;
        }
        if self.net.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
            self.install_note = "Сейчас идет другая загрузка - обновите игру, когда она закончится.".into();
            return;
        }
        self.install_note.clear();
        if switching {
            // Сначала язык (бета, если тоже выбрана, - следующим нажатием «Обновить»).
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
        // Игра без защиты Steam - запускаем напрямую (галочка в «Свойствах»).
        if !update && self.settings.no_steam.contains(&appid) {
            let net = self.net.lock().unwrap().clone();
            self.direct_launch(appid, &net);
            return;
        }
        // Закрепленную версию нельзя обновлять, а перед запуском ее надо подготовить.
        if self.settings.pinned.contains_key(&appid) {
            if update {
                *self.launch_status.lock().unwrap() = "Версия игры закреплена - открепите ее в «Свойствах», чтобы обновить.".into();
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
        // Нет связи со Steam: пусть скрытый Steam сразу стартует автономно (без окна с вопросом).
        // Флаг снимается, когда Steam снова закрыт (offline_tick).
        if self.net.lock().unwrap().offline && !self.steam_running() {
            match local::set_steam_offline(&steam.dir, true) {
                Ok(_) => self.settings.steam_offline_set = true,
                Err(e) => *self.launch_status.lock().unwrap() = format!("Автономный режим Steam не включен: {e}"),
            }
        }
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
        let script = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            let res = if update {
                local::update_via_steam(&steam, &game, opts, &cancel, &set)
            } else {
                // Скрипты запуска: команда перед запуском (ждем ее), после выхода (не ждем).
                if !script.before.trim().is_empty() {
                    set("Выполняю команду перед запуском...");
                }
                let warn = scripts::run_before(&script, game.appid, &game.name, &game.dir);
                let res = local::run_chain(&steam, &game, opts, &cancel, &set);
                let after = if res.is_ok() && !cancel.load(Ordering::Relaxed) {
                    scripts::run_after(&script, game.appid, &game.name, &game.dir)
                } else {
                    None
                };
                match (warn.or(after), res) {
                    (Some(w), Ok(msg)) => Ok(format!("{msg} {w}")),
                    (_, r) => r,
                }
            };
            match res {
                Ok(msg) => set(&msg),
                Err(e) => {
                    crate::errlog::write(if update { "обновление" } else { "запуск" }, &format!("{}: {e}", game.name));
                    set(&e)
                }
            }
            launching.store(false, Ordering::Relaxed);
            // Steam мог обновить игры - перечитать диск и заново проверить обновления.
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
        // Сторонние игры (кнопка «Добавить игру»).
        for r in self.custom_rows() {
            map.insert(r.appid, r);
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
            // Коллекция: выбранная - только ее игры; «все игры» - без «Скрытых», как в Steam.
            // «Семейная библиотека» - только игры семьи; в «Все игры» - по галочке в настройках.
            .filter(|r| match (self.collection.as_deref(), selected) {
                (Some(FAMILY), _) => r.family,
                (Some(UNPLAYED), _) => !r.family && r.playtime_min == Some(0) && !hidden.contains(&r.appid),
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
        // Порядок: с новыми сообщениями, в игре, в сети, не в сети; внутри - по имени.
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
        // Избранные - наверху (после друзей с новыми сообщениями).
        let favs = self.settings.fav_friends.clone();
        list.sort_by(|a, b| {
            let fa = a.4 == 0 && favs.contains(&a.0);
            let fb = b.4 == 0 && favs.contains(&b.0);
            (a.4 == 0).cmp(&(b.4 == 0)).then(fb.cmp(&fa)).then(rank(a).cmp(&rank(b))).then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        // Аватарки (хэш из статуса друга): avatars.fastly.steamstatic.com/<хэш>.jpg - 32x32.
        let avatars: HashMap<u64, String> = net
            .roster
            .read()
            .unwrap()
            .values()
            .filter_map(|f| f.avatar_hash.as_ref().map(|h| (f.steam_id, format!("https://avatars.fastly.steamstatic.com/{h}.jpg"))))
            .collect();
        let notes = self.settings.friend_notes.clone();

        // Заголовок с меню, заявки в друзья.
        self.friends_header(ui, net);
        if list.is_empty() {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak("Загружаю список...");
            });
            return;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("ДРУЗЬЯ В СЕТИ  {online} / {}", list.len())).size(11.0).color(theme::WEAK));
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
                let note = notes.get(id).cloned().unwrap_or_default();
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) && !note.to_lowercase().contains(&filter) {
                    continue;
                }
                let (color, status) = match (state, game) {
                    (_, Some(g)) => (egui::Color32::from_rgb(140, 200, 80), g.clone()),
                    (0, _) => (egui::Color32::GRAY, "не в сети".into()),
                    (1, _) => (egui::Color32::from_rgb(90, 170, 230), "в сети".into()),
                    (2, _) => (egui::Color32::from_rgb(220, 120, 110), "не беспокоить".into()),
                    (3 | 4, _) => (egui::Color32::from_rgb(110, 140, 170), "отошел".into()),
                    _ => (egui::Color32::from_rgb(90, 170, 230), "в сети".into()),
                };
                let mut title = egui::RichText::new(crate::richtext::fix(ui, name)).color(if *state == 0 { egui::Color32::GRAY } else { color });
                if *unread > 0 {
                    title = title.strong();
                }
                // Вся строка (аватар, имя, статус) - одна кнопка на всю ширину панели.
                let block = ui
                    .horizontal(|ui| {
                    // Аватар с рамкой цвета статуса, как в Steam.
                    let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    match avatars.get(id) {
                        Some(u) => {
                            egui::Image::new(u.as_str()).rounding(2.0).paint_at(ui, ar);
                        }
                        None => {
                            ui.painter().rect_filled(ar, 2.0, theme::CARD);
                        }
                    }
                    ui.painter().rect_stroke(ar.expand(1.0), 2.0, egui::Stroke::new(1.5, if *state == 0 { egui::Color32::from_gray(90) } else { color }));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(title).truncate());
                            if favs.contains(id) {
                                ui.label(egui::RichText::new("избранный").small().color(egui::Color32::from_rgb(230, 190, 80)));
                            }
                            if *unread > 0 {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("● {unread}"));
                            }
                        });
                        ui.add(egui::Label::new(egui::RichText::new(status).small().color(color)).truncate());
                        if !note.is_empty() {
                            ui.label(egui::RichText::new(&note).small().italics().weak());
                        }
                    });
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
                socialwin::friend_menu(&resp, *id, name, favs.contains(id), &mut friend_act);
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
        // Тот же друг - просто оставляем окно; другой - переключаем окно на него.
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

    /// «Уже скачана...»: выбрать папку игры в steamapps\common и добавить ее в Steam.
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
                     Перенесите ее туда и попробуйте снова."
                    .into();
            }
        }
    }

    /// Раздел вместо библиотеки: магазин, инвентарь или Мастерская.
    fn section_view(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Раздел открыт отдельным окном - здесь только подсказка.
        let detached = match self.view {
            View::Store => self.store_detached.then_some("Магазин"),
            View::Inventory => self.inv_detached.then_some("Инвентарь"),
            View::Workshop => self.ws_detached.then_some("Мастерская"),
            _ => None,
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
            View::Workshop => self.workshop_contents(ui, net),
            View::Settings => self.settings_contents(ui, net),
            View::Screenshots => self.shots_contents(ui, net),
            View::Storage => self.storage_contents(ui),
            View::News => self.news_contents(ui, net),
            View::Badges => self.badges_contents(ui, net),
            View::Points => self.points_contents(ui, net),
            View::Family => self.family_contents(ui, net),
            View::Activity => self.activity_contents(ui, net),
            View::Stats => self.stats_contents(ui, net),
            View::Community => self.community_contents(ui, net),
            View::Groups => self.groups_contents(ui, net),
            View::Profile => self.profile_contents(ui, net),
            View::Friends => self.friends_page_contents(ui, net),
            View::Downloads => self.downloads_contents(ui, net),
            View::Library => {}
        }
    }

    /// Левая колонка библиотеки, как в Steam: «Главная», поиск, коллекция, фильтры и список игр.
    /// Поиск и фильтры библиотеки: в левой колонке или (toolbar) строкой над таблицей.
    fn library_filters(&mut self, ui: &mut egui::Ui, net: &NetState, toolbar: bool) {
        let hint = if net.owned.is_empty() { "Поиск".to_string() } else { format!("Поиск среди {} игр", net.owned.len()) };
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(hint).desired_width(if toolbar { 220.0 } else { f32::INFINITY }));
                    // Коллекции Steam (правый щелчок по игре - добавить в коллекцию).
                    if let Some(colls) = net.collections.as_ref() {
                        let current = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id))
                            .map(|c| c.name.clone())
                            .or_else(|| (self.collection.as_deref() == Some(FAMILY)).then(|| "Семейная библиотека".to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(UNPLAYED)).then(|| "Ни разу не запускались".to_string()))
                            .unwrap_or_else(|| "Все игры".into());
                        let family_n = self.row_map(&net).values().filter(|r| r.family).count();
                        // Своя (не встроенная и не по фильтру) коллекция: переименовать, удалить.
                        let own = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id && c.id.starts_with("uc-") && !c.dynamic))
                            .cloned();
                        let own_coll = own.is_some();
                        ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("collection")
                            .width(if toolbar { 190.0 } else { ui.available_width() - if own_coll { 34.0 } else { 4.0 } })
                            .truncate()
                            .selected_text(current)
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
                                let unplayed_n = net.owned.iter().filter(|g| g.playtime_forever_minutes == 0).count();
                                if unplayed_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(UNPLAYED.to_string()),
                                        format!("Ни разу не запускались ({unplayed_n})"),
                                    )
                                    .on_hover_text("Купленные игры, в которых 0 минут");
                                }
                                ui.separator();
                                for c in colls {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(c.id.clone()),
                                        format!("{} ({})", c.name, c.apps.len()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text("Коллекции из Steam. Добавить игру - правый щелчок по ней. «Скрытые» в «Все игры» не показываются.");
                        if let Some(c) = own {
                            ui.menu_button("...", |ui| {
                                let key = egui::Id::new("coll_rename");
                                let mut name: String =
                                    ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| c.name.clone());
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0));
                                    let ok = !name.trim().is_empty() && name.trim() != c.name;
                                    if ui.add_enabled(ok, egui::Button::new("Переименовать")).clicked() {
                                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Rename {
                                            id: c.id.clone(),
                                            name: name.trim().to_string(),
                                        }));
                                        ui.data_mut(|d| d.remove::<String>(key));
                                        ui.close_menu();
                                    }
                                });
                                ui.data_mut(|d| d.insert_temp(key, name));
                                ui.separator();
                                let confirm = egui::Id::new("coll_delete_confirm");
                                let asked = ui.data(|d| d.get_temp::<String>(confirm)) == Some(c.id.clone());
                                if !asked {
                                    if ui.button("Удалить коллекцию...").clicked() {
                                        ui.data_mut(|d| d.insert_temp(confirm, c.id.clone()));
                                    }
                                } else {
                                    ui.label(format!("Удалить «{}»? Игры останутся в библиотеке.", c.name));
                                    ui.horizontal(|ui| {
                                        if ui.button("Удалить").clicked() {
                                            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Delete { id: c.id.clone() }));
                                            self.collection = None;
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                            ui.close_menu();
                                        }
                                        if ui.button("Нет").clicked() {
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                        }
                                    });
                                }
                            })
                            .response
                            .on_hover_text("Переименовать или удалить эту коллекцию (изменится и в Steam)");
                        }
                        });
                    }
        if !toolbar {
        egui::ComboBox::from_id_salt("sort")
            .width(ui.available_width() - 4.0)
            .truncate()
            .selected_text(format!("Сортировка: {}", self.settings.sort.label()))
            .show_ui(ui, |ui| {
                for s in settings::Sort::ALL {
                    ui.selectable_value(&mut self.settings.sort, s, s.label());
                }
            });
        }
        ui.checkbox(&mut self.only_installed, "Только установленные");
        if let Some(e) = &net.collections_error {
            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
        }
    }

    fn library_sidebar(&mut self, ui: &mut egui::Ui, net: &NetState, rows: &[Row], vc: &libview::Ctx) -> Option<libview::Action> {
        let mut out = None;
        let home = self.lib_page.is_none();
        let text = egui::RichText::new("Главная").size(14.0).strong().color(if home { egui::Color32::WHITE } else { theme::TEXT });
        let fill = if home { theme::CARD } else { egui::Color32::TRANSPARENT };
        if ui.add_sized([ui.available_width(), 28.0], egui::Button::new(text).fill(fill)).clicked() {
            self.lib_page = None;
        }
        ui.add_space(4.0);
        self.library_filters(ui, net, false);
        ui.add_space(2.0);
        // В табличном виде игры уже показаны таблицей справа - второй список не нужен.
        let table = self.settings.view == settings::View::List || !self.settings.covers;
        if table {
            return out;
        }
        ui.add_space(2.0);
        if let Some(a) = libview::sidebar(ui, rows, vc, &self.covers, self.lib_page, self.settings.covers) {
            out = Some(a);
        }
        out
    }

    /// В фоне (окно в трее или идет игра, а Vaporust не активен) - выгрузить картинки и отдать
    /// память системе: она нужнее игре. Повторяем раз в минуту, пока Vaporust в фоне.
    fn memory_tick(&mut self, ctx: &egui::Context) {
        let game = local::RUNNING.lock().unwrap().is_some();
        let background = notify::in_tray() || (game && !ctx.input(|i| i.focused));
        if !(self.settings.trim_memory && background) {
            self.trimmed = false;
            return;
        }
        if !self.trimmed {
            self.trimmed = true;
            ctx.forget_all_images();
            self.covers.clear();
            imgload::trim_working_set();
            self.trim_at = std::time::Instant::now();
        } else if self.trim_at.elapsed() >= std::time::Duration::from_secs(60) {
            imgload::trim_working_set();
            self.trim_at = std::time::Instant::now();
        }
    }

    fn account_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if net.account.is_some() {
            self.top_nav(ui, net);
        } else if net.offline {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "Нет связи со Steam.")
                    .on_hover_text(&net.offline_reason);
                ui.label("Автономный режим: игры из сохраненного списка, запуск через Steam без сети.");
                if ui.add_enabled(!net.busy, egui::Button::new("Переподключиться")).clicked() {
                    self.offline_retry = Some(std::time::Instant::now());
                    let _ = self.tx.send(Cmd::Restore);
                }
                if ui
                    .add_enabled(!net.busy, egui::Button::new("Войти заново"))
                    .on_hover_text("Если дело не в сети: войти в аккаунт логином и паролем или QR-кодом")
                    .clicked()
                {
                    let mut s = self.net.lock().unwrap();
                    s.offline = false;
                    s.need_login = true;
                }
                if net.busy {
                    crate::spin::spinner(ui);
                }
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
                        .hint_text("если пришел на почту или в приложение"),
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
        let mut ask_friend: Option<u64> = None;
        let st = &mut self.ach_state;
        egui::Window::new(format!("Достижения: {name}"))
            .open(&mut open)
            .default_size([560.0, 480.0])
            .show(ctx, |ui| match net.achievements.get(&appid) {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.label(format!("Ошибка: {e}"));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label("У этой игры нет достижений.");
                }
                Some(Ok(list)) => {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    ui.horizontal(|ui| {
                        ui.label(format!("Получено {done} из {}", list.len()));
                        ui.add(
                            egui::ProgressBar::new(done as f32 / list.len().max(1) as f32)
                                .desired_width(160.0)
                                .text(format!("{}%", done * 100 / list.len().max(1))),
                        );
                    });
                    // Порядок, фильтр, сравнение с другом.
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("ach_sort")
                            .selected_text(ACH_SORTS[st.sort as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_SORTS.iter().enumerate() {
                                    ui.selectable_value(&mut st.sort, i as u8, *s);
                                }
                            });
                        egui::ComboBox::from_id_salt("ach_filter")
                            .selected_text(ACH_FILTERS[st.filter as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_FILTERS.iter().enumerate() {
                                    ui.selectable_value(&mut st.filter, i as u8, *s);
                                }
                            });
                        let roster = net.roster.read().unwrap();
                        let mut friends: Vec<(u64, String)> = roster
                            .values()
                            .filter(|f| f.relationship == 3)
                            .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string())))
                            .collect();
                        drop(roster);
                        friends.sort_by_key(|f| f.1.to_lowercase());
                        let current = st
                            .friend
                            .and_then(|id| friends.iter().find(|f| f.0 == id))
                            .map(|f| format!("Сравнить с: {}", f.1))
                            .unwrap_or_else(|| "Сравнить с другом".into());
                        egui::ComboBox::from_id_salt("ach_friend").selected_text(current).height(320.0).show_ui(ui, |ui| {
                            if ui.selectable_label(st.friend.is_none(), "Без сравнения").clicked() {
                                st.friend = None;
                            }
                            for (id, n) in &friends {
                                if ui.selectable_label(st.friend == Some(*id), n).clicked() {
                                    st.friend = Some(*id);
                                    ask_friend = Some(*id);
                                }
                            }
                        });
                    });
                    // Достижения друга: ✔/✖ по имени API.
                    let friend_map: Option<HashMap<&str, bool>> = match st.friend.map(|f| net.friend_achievements.get(&(appid, f))) {
                        Some(Some(Ok(v))) => {
                            let got = v.iter().filter(|a| a.unlocked).count();
                            if got == 0 {
                                ui.weak("У друга нет полученных достижений (не играл или скрыл данные об играх).");
                            } else {
                                ui.label(format!("У друга {got} из {}", v.len()));
                            }
                            Some(v.iter().map(|a| (a.api_name.as_str(), a.unlocked)).collect())
                        }
                        Some(Some(Err(e))) => {
                            ui.weak(format!("Достижения друга не получены: {e}"));
                            None
                        }
                        Some(None) => {
                            crate::spin::spinner(ui);
                            None
                        }
                        None => None,
                    };
                    ui.separator();
                    let mut rows: Vec<&aurelia::steam_client::GameAchievement> = list
                        .iter()
                        .filter(|a| match st.filter {
                            1 => a.unlocked,
                            2 => !a.unlocked,
                            _ => true,
                        })
                        .collect();
                    match st.sort {
                        1 => rows.sort_by(|a, b| a.global_percent.total_cmp(&b.global_percent)),
                        2 => rows.sort_by(|a, b| b.global_percent.total_cmp(&a.global_percent)),
                        3 => rows.sort_by_key(|a| std::cmp::Reverse(a.unlock_time.unwrap_or(0))),
                        _ => {}
                    }
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for a in rows {
                            ui.horizontal(|ui| {
                                let icon = if a.unlocked { &a.icon_unlocked } else { &a.icon_locked };
                                if !icon.is_empty() {
                                    ui.add(egui::Image::new(icon.as_str()).fit_to_exact_size(egui::vec2(40.0, 40.0)));
                                } else {
                                    ui.label(if a.unlocked { "✔" } else { "✖" });
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        if a.unlocked {
                                            ui.strong(&a.name);
                                        } else {
                                            ui.label(&a.name);
                                        }
                                        if a.global_percent > 0.0 && a.global_percent < 10.0 {
                                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "редкое");
                                        }
                                    });
                                    if a.hidden && !a.unlocked && a.description.is_empty() {
                                        ui.weak("Скрытое достижение");
                                    } else if !a.description.is_empty() {
                                        ui.weak(&a.description);
                                    }
                                    let mut line = format!("Есть у {:.1}% игроков", a.global_percent);
                                    if let Some(t) = a.unlock_time.filter(|_| a.unlocked) {
                                        line.push_str(&format!(" · получено {}", crate::socialwin::fmt_date(t as u64)));
                                    }
                                    if let Some(m) = &friend_map {
                                        match m.get(a.api_name.as_str()) {
                                            Some(true) => line.push_str(" · у друга есть"),
                                            Some(false) => line.push_str(" · у друга нет"),
                                            None => {}
                                        }
                                    }
                                    ui.weak(line);
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            });
        if let Some(f) = ask_friend {
            if !net.friend_achievements.contains_key(&(appid, f)) {
                let _ = self.tx.send(Cmd::FriendAchievements(appid, f));
            }
        }
        if !open {
            self.ach_open = None;
            self.ach_state = AchState::default();
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
        // Steam ждет смайлик как ːимяː (U+02D0); тег [emoticon] сервер вырезает.
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
/// Ok(None) - в буфере нет картинки.
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
/// Картинки грузятся с серверов Steam при первом показе (egui_extras), пока грузятся - крутится значок.
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
            // Размер известен - сразу резервируем место, чтобы переписка не прыгала при загрузке.
            let max = egui::vec2(360.0, 280.0);
            // Показываем уменьшенную копию Steam (до 512 точек), а не оригинал: меньше памяти и работы видеокарте.
            let img = egui::Image::new(imgload::still_if_needed(&chatfmt::thumbnail_url(&url))).sense(egui::Sense::click());
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
    /// Фон под панелями: полностью прозрачный (у окна уведомлений видны скругленные углы;
    /// у обычных окон фон и так закрыт панелями, а прозрачность окна выключена).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

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
        // Память: при переходе в другой раздел картинки прежнего не нужны - чистим кэш;
        // и следим за пределом кэша картинок.
        let place = (self.view, self.lib_page);
        if self.mem_place != Some(place) {
            if self.mem_place.is_some() {
                imgload::forget_large(ctx);
            }
            self.mem_place = Some(place);
        }
        imgload::ANIMATIONS.store(self.settings.animations, Ordering::Relaxed);
        imgload::ECONOMY.store(self.settings.economy, Ordering::Relaxed);
        imgload::budget_tick(ctx);
        self.memory_tick(ctx);
        // Снимок сетевого состояния на кадр, чтобы не держать блокировку. Копируем заново только
        // когда состояние поменялось (и на всякий случай раз в 2 с), иначе берем прошлую копию.
        let gen = self.net.gen();
        if gen != self.net_gen || self.net_snap_at.elapsed() >= std::time::Duration::from_secs(2) {
            self.net_snap = self.net.lock().unwrap().clone();
            self.net_gen = gen;
            self.net_snap_at = std::time::Instant::now();
        }
        let mut net = std::mem::take(&mut self.net_snap);
        let net_gen = std::mem::replace(&mut self.net_gen, u64::MAX);
        self.tour_tick(ctx, &net);

        // Непрочитанные сообщения: число в заголовке окна; когда все прочитано - убрать точку в трее.
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
        // Автозапуск с Windows (--tray): окно сразу в трей, как только у него появился HWND.
        if self.start_hidden && notify::main_hwnd() != 0 {
            self.start_hidden = false;
            tray::hide_main();
        }
        // Галочка «Крестик сворачивает в трей» живет в меню трея; сохраняем ее в настройках.
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
        // Установка закончилась (успешно или нет) - перечитываем диск.
        if net.disk_changed != self.seen_disk_changed {
            self.seen_disk_changed = net.disk_changed;
            self.rescan();
        }
        // Steam закончил игру или обновление - перечитать диск и перепроверить обновления.
        if self.rescan_needed.swap(false, Ordering::Relaxed) {
            self.rescan();
            self.updates_checked = false;
        }
        // Коллекции Steam - один раз после входа.
        if net.account.is_none() {
            self.colls_requested = false;
        } else if !self.colls_requested {
            self.colls_requested = true;
            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Load));
        }
        // Сохраненный статус для друзей - после каждого входа.
        if net.account.is_none() {
            self.persona_sent = false;
        } else if !self.persona_sent {
            self.persona_sent = true;
            let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
            self.send_aurelia_config();
            // Свои эффекты комнаты: сразу - из сохраненного списка, свежий - из инвентаря в фоне.
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
        // Закрепленные версии: запуск после подготовки, откат, устаревшая маскировка.
        self.pin_tick(ctx, &mut net);
        // Запуск без Steam: пришла таблица запуска.
        self.direct_tick();
        let installing = net.install.as_ref().is_some_and(|i| i.active());
        // Перерисовка: во время загрузки или запуска игры - раз в секунду (полоса прогресса, статус),
        // в покое - раз в 5 с (кнопка «Закрыть Steam», если Steam запустили вручную).
        // Пока идет игра, а окно Vaporust не активно, - реже (статус в фоне никто не смотрит).
        let game_bg = local::RUNNING.lock().unwrap().is_some() && !ctx.input(|i| i.focused);
        let busy = installing || (!game_bg && (self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed)));
        ctx.request_repaint_after(std::time::Duration::from_secs(if busy { 1 } else { 5 }));

        // Рамка окон: своя или обычная Windows (переключается сразу).
        winframe::set_system(self.settings.system_frame);
        if self.frame_applied != Some(self.settings.system_frame) {
            self.frame_applied = Some(self.settings.system_frame);
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(self.settings.system_frame));
        }
        winframe::heal_client_size(ctx);
        // Windows упорно рисует свой заголовок поверх своей рамки - включить обычную рамку Windows.
        if let Some(m) = winframe::caption_forced() {
            self.settings.system_frame = true;
            errlog::write("окно", &format!("Windows рисует свой заголовок ({m}): включена обычная рамка Windows"));
            ctx.request_repaint();
        }
        // Проверка рамки - в первые 10 с после запуска (дальше перерисовка как обычно, без лишних кадров).
        if !self.settings.system_frame && ctx.input(|i| i.time) < 10.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        if !notify::in_tray() {
            let title = if unread > 0 {
                format!("({unread}) Vaporust {}", env!("CARGO_PKG_VERSION"))
            } else {
                concat!("Vaporust ", env!("CARGO_PKG_VERSION")).to_string()
            };
            let _ = &title; // название окна - на панели задач (ViewportCommand::Title выше)
            // Как в Steam: отдельной полосы заголовка нет - кнопки окна в строке разделов справа,
            // окно перетаскивается за пустое место этой строки.
            winframe::frame_only(ctx);
            let own_frame = !winframe::system();
            let top_frame = egui::Frame::none().fill(theme::TOP).inner_margin(if own_frame {
                egui::Margin { left: 10.0, right: 0.0, top: 0.0, bottom: 2.0 }
            } else {
                egui::Margin::symmetric(10.0, 6.0)
            });
            let drag_id = egui::Id::new("vaporust_top_drag");
            let top = egui::TopBottomPanel::top("account").frame(top_frame).show_separator_line(false).show(ctx, |ui| {
                winframe::drag_behind(ui, drag_id);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        winframe::window_buttons(ui, 34.0);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            self.account_panel(ui, &net);
                        });
                    });
                });
            });
            winframe::remember_drag(ctx, drag_id, top.response.rect);
            if net.account.is_some() {
                let sub_frame = egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 0.0, bottom: 5.0 });
                egui::TopBottomPanel::top("subnav").frame(sub_frame).show(ctx, |ui| self.sub_nav(ui, &net));
            }
            self.update_banner(ctx);

            let bottom_frame = egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin::symmetric(8.0, 3.0));
            egui::TopBottomPanel::bottom("status").frame(bottom_frame).show(ctx, |ui| {
                ui.add_space(2.0);
                let full = ui.max_rect();
                let row = ui.horizontal(|ui| {
                    // Слева, как в Steam, - «Добавить игру».
                    ui.menu_button(egui::RichText::new("+ Добавить игру").size(13.0), |ui| {
                        ui.set_min_width(220.0);
                        if ui.button("Добавить стороннюю игру...").on_hover_text("Игра или программа не из Steam").clicked() {
                            self.custom_edit = Some(custom::Edit::default());
                            ui.close_menu();
                        }
                        if ui.button("Активировать в Steam...").on_hover_text("Ключ продукта Steam: игра, дополнение, набор").clicked() {
                            self.key_input = Some(String::new());
                            self.key_refreshed = false;
                            self.net.lock().unwrap().key_result = None;
                            ui.close_menu();
                        }
                    });
                    // Справа, как в Steam, - «Друзья и чат».
                    if net.account.is_some() {
                        let unread: u32 = net.chats.values().map(|c| c.unread).sum();
                        let label = if unread > 0 { format!("Друзья и чат ({unread})") } else { "Друзья и чат".to_string() };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.toggle_value(&mut self.show_friends, label);
                        });
                    }
                }).response.rect;
                // Посередине, как в Steam, - «Загрузки».
                let center = egui::Rect::from_center_size(egui::pos2(full.center().x, row.center().y), egui::vec2(280.0, row.height()));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(center).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)), |ui| {
                    self.downloads_button(ui, &net);
                });
                // Заморозка steamwebhelper и прочие галочки - в «Настройках» (меню «Аккаунт»).
                ui.add_space(2.0);
            });

            if net.account.is_some() && self.show_friends {
                egui::SidePanel::right("friends")
                    .resizable(true)
                    .default_width(230.0)
                    .width_range(200.0..=380.0)
                    .show(ctx, |ui| self.friends_panel(ui, &net));
            }

            let ctrl = ctx.input(|i| i.modifiers.ctrl);
            // Библиотека как в Steam: слева список игр с поиском и фильтрами, справа «Главная»
            // (плитка или список) или страница выбранной игры.
            let launching = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
            let rows = if self.view == View::Library { self.rows(&net) } else { Vec::new() };
            let queued: std::collections::HashSet<u32> =
                self.settings.downloads.iter().filter(|q| !q.paused).map(|q| q.appid).collect();
            let status_text = self.launch_status.lock().unwrap().clone();
            let vc = libview::Ctx {
                launching,
                installing,
                logged_in: net.account.is_some(),
                net: &net,
                queued: &queued,
                collections: net.collections.as_deref().unwrap_or(&[]),
                update_via_steam: self.settings.update_via_steam,
                running: local::RUNNING.lock().unwrap().as_ref().map(|r| r.0),
                status_app: self.status_app,
                status: &status_text,
            };
            let mut side_action = None;
            // Компактный вид (таблица): без левой колонки, фильтры - строкой над таблицей.
            let table_mode = self.settings.view == settings::View::List || !self.settings.covers;
            if self.view == View::Library && !table_mode {
                let side = egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(6.0, 6.0));
                egui::SidePanel::left("games")
                    .frame(side)
                    .resizable(true)
                    .default_width(250.0)
                    .width_range(180.0..=420.0)
                    .show(ctx, |ui| side_action = self.library_sidebar(ui, &net, &rows, &vc));
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                // Магазин, инвентарь, Мастерская - вместо библиотеки, в этом же окне.
                if self.view != View::Library {
                    self.section_view(ui, &net);
                    return;
                }
                let page_row = self.lib_page.and_then(|id| self.row_map(&net).remove(&id));
                // Для строки «играли за две недели» на странице игры - один раз загрузить активность друзей.
                if page_row.is_some() && !self.activity_ui.asked && net.account.is_some() && !net.roster.read().unwrap().is_empty() {
                    self.activity_ui.asked = true;
                    self.ask_activity(&net);
                }
                if page_row.is_some() && !self.licenses_asked && net.account.is_some() {
                    self.licenses_asked = true;
                    let _ = self.tx.send(Cmd::Licenses);
                }
                // В табличном виде левой колонки нет - вернуться к таблице кнопкой.
                if page_row.is_some() && table_mode && ui.button("< Все игры").clicked() {
                    self.lib_page = None;
                }
                let action = if let Some(r) = &page_row {
                    // Страница игры: новости и онлайн - из данных магазина, достижения - из сети.
                    let store = net.store.lock().unwrap();
                    let ex = store.extras.get(&r.appid);
                    let data = libview::PageData {
                        achievements: net.achievements.get(&r.appid),
                        news: ex.and_then(|e| e.news.as_ref()),
                        players: ex.and_then(|e| e.players),
                    };
                    libview::page(ui, r, &vc, &self.covers, &data, self.settings.covers)
                } else {
                    // «Главная»: заголовок, вид, «Во что поиграть?», затем плитка или список.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let title = match &self.collection {
                            None => "Все игры".to_string(),
                            Some(id) if id == FAMILY => "Семейная библиотека".to_string(),
                            Some(id) if id == UNPLAYED => "Ни разу не запускались".to_string(),
                            Some(id) => net
                                .collections
                                .as_ref()
                                .and_then(|c| c.iter().find(|c| &c.id == id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| "Коллекция".into()),
                        };
                        ui.label(egui::RichText::new(format!("{title} ({})", rows.len())).size(17.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.settings.covers {
                                ui.selectable_value(&mut self.settings.view, settings::View::List, "Таблица")
                                    .on_hover_text("Компактный вид: игры таблицей, без обложек");
                                ui.selectable_value(&mut self.settings.view, settings::View::Grid, "Плитка");
                            }
                            if self.settings.random_button
                                && !rows.is_empty()
                                && ui
                                    .button("Во что поиграть?")
                                    .on_hover_text("Случайная игра из показанных (с учетом поиска, коллекции и фильтров)")
                                    .clicked()
                            {
                                let seed = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.subsec_nanos() as usize ^ d.as_secs() as usize)
                                    .unwrap_or(0);
                                self.lib_page = Some(rows[seed % rows.len()].appid);
                            }
                        });
                    });
                    ui.add_space(4.0);
                    // Таблица: поиск и фильтры строкой под заголовком (в плитке они в левой колонке, тоже под «Главная»).
                    if table_mode {
                        ui.horizontal(|ui| self.library_filters(ui, &net, true));
                        ui.add_space(4.0);
                    }
                    // Без обложек (настройка «Производительность») - всегда список.
                    if self.settings.view == settings::View::List || !self.settings.covers {
                        libview::list(ui, &rows, &vc, &self.covers, &mut self.settings.sort, self.settings.covers)
                    } else {
                        libview::grid(ui, &rows, &vc, &self.covers)
                    }
                };
                let action = side_action.take().or(action);
                // Страница игры открыта (любым путем: щелчок, уведомление, меню) - один раз
                // запросить достижения, новости и онлайн.
                if let Some(p) = self.lib_page {
                    if self.page_loaded != Some(p) && net.account.is_some() && !custom::is_custom(p) {
                        self.page_loaded = Some(p);
                        if net.account.is_some() && !net.achievements.contains_key(&p) {
                            let _ = self.tx.send(Cmd::Achievements(p));
                        }
                        let _ = self.tx.send(Cmd::Store(store::StoreCmd::Page(p)));
                    }
                } else {
                    self.page_loaded = None;
                }
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
                        // Достижения и новости запросятся выше, при показе страницы.
                        self.lib_page = Some(appid);
                    }
                    Some(libview::Action::Store(appid)) => self.open_store(Some(appid), ctrl),
                    Some(libview::Action::Workshop(appid, name)) => self.open_workshop(appid, name, ctrl),
                    Some(libview::Action::Community(appid, name)) => self.open_community(appid, name),
                    Some(libview::Action::Shots(appid)) => self.open_shots(Some(appid)),
                    Some(libview::Action::Folder(appid)) => {
                        if let Some(g) = self.games.iter().find(|g| g.appid == appid) {
                            local::open_folder(&g.dir);
                        } else if let Some(dir) = self.settings.custom_games.iter().find(|g| g.id == appid).and_then(|g| std::path::Path::new(&g.exe).parent().map(|p| p.to_path_buf())) {
                            local::open_folder(&dir);
                        }
                    }
                    Some(libview::Action::Import(appid, name)) => self.import_game(appid, name),
                    Some(libview::Action::Stop(appid)) => self.stop_running(appid, false),
                    Some(libview::Action::Uninstall(appid, name)) => self.ask_uninstall(appid, name),
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
        self.offline_tick(&net);
        self.notices_tick(&net);
        self.steam_notifs_tick(&net);
        self.stats_tick(&net);
        self.popups_ui(ctx);
        self.custom_tick(ctx);
        self.uninstall_dialog(ctx, &net);
        self.status_popup();
        self.popup_clicks(ctx, &net);
        self.finish_tick(&net);
        self.run_queue(&net);
        self.guard_partials(&net);

        self.achievements_window(ctx, &net);
        self.props_window(ctx, &net);
        self.store_window(ctx, &net);
        self.inventory_window(ctx, &net);
        self.social_windows(ctx, &net);
        self.key_window(ctx, &net);
        self.shot_comments_window(ctx, &net);
        self.workshop_window(ctx, &net);
        chatwin::show(ctx, &self.chat, &self.net, &self.tx);
        stats::frame(frame_start);
        self.net_snap = net;
        self.net_gen = net_gen;
    }
}

fn main() -> eframe::Result<()> {
    // Портативный режим (portable.txt рядом с exe): данные - в папке VaporustData.
    let persistence_path = portable::init();
    errlog::install_panic_hook();
    let options = eframe::NativeOptions {
        persistence_path,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 630.0])
            // Не даем сжать окно до нечитаемого (и восстановить таким из сохраненных настроек).
            .with_min_inner_size([760.0, 460.0])
            // Постоянный идентификатор: по нему eframe хранит настройки (%APPDATA%Vaporust),
            // поэтому они не теряются при смене версии в заголовке.
            .with_app_id("Vaporust")
            // Своя рамка (winframe); обычная Windows включается в настройках с первого кадра.
            .with_decorations(false)
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

/// Ошибка загрузки похожа на обрыв связи (а не на нехватку места, доступ и т.п.).
fn network_error(e: &str) -> bool {
    let t = e.to_lowercase();
    ["timed out", "timeout", "connection", "network", "cdn", "dns", "reset", "unreachable", "сет", "связ", "offline"]
        .iter()
        .any(|w| t.contains(w))
}

/// Окно достижений: порядок, фильтр, друг для сравнения.
#[derive(Default)]
struct AchState {
    sort: u8,
    filter: u8,
    friend: Option<u64>,
}

const ACH_SORTS: [&str; 4] = ["Как в игре", "Сначала редкие", "Сначала частые", "Недавно полученные"];
const ACH_FILTERS: [&str; 3] = ["Все", "Полученные", "Не полученные"];

impl LauncherApp {
    /// Дополнительные разделы (скриншоты, хранилище, значки...) открываются по одному: открыть
    /// новый - закрыть прежний, чтобы вкладки не копились в верхней панели. `keep` - открываемый.
    pub(crate) fn close_extras(&mut self, keep: View) {
        if keep != View::Screenshots {
            self.shots_win = None;
        }
        if keep != View::Storage {
            self.storage_win = None;
        }
        if keep != View::Badges {
            self.badges_ui.open = false;
        }
        if keep != View::Points {
            self.points_ui.open = false;
        }
        if keep != View::Family {
            self.family_ui.open = false;
        }
        if keep != View::Activity {
            self.activity_ui.open = false;
        }
        if keep != View::Stats {
            self.stats_ui.open = false;
        }
        if keep != View::Community {
            self.community_ui.appid = None;
        }
        if keep != View::Groups {
            self.groups_ui.open = false;
        }
    }
}
