#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod chatfmt;
mod chatwin;
mod icons;
mod launchopts;
mod local;
mod net;
mod notify;
mod props;
mod settings;
mod tray;

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
    /// Статус для друзей уже объявлен для текущего входа.
    persona_sent: bool,
    /// Сколько непрочитанных было в прошлом кадре (для заголовка окна).
    last_unread: u32,
    /// Когда очередь последний раз запускала установку.
    queue_started: Option<std::time::Instant>,
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

    /// Остальные сохраняемые настройки (сортировка, статус, трей, звук).
    settings: settings::Settings,
    /// Когда игры запускались последний раз (из localconfig.vdf Steam).
    last_played: HashMap<u32, u64>,

    // Установка
    libraries: Vec<std::path::PathBuf>,
    lib_idx: usize,
    seen_disk_changed: u64,
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
        notify::set_sound(saved.notify_sound);
        tray::start();

        let mut app = Self {
            steam: None,
            games: Vec::new(),
            launch_status: Arc::new(Mutex::new(String::new())),
            launching: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            rescan_needed: Arc::new(AtomicBool::new(false)),
            updates_checked: false,
            persona_sent: false,
            last_unread: 0,
            queue_started: None,
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
            install_note: String::new(),
            settings: saved,
            last_played: HashMap::new(),
        };
        app.rescan();
        // Выбранная в прошлый раз папка библиотеки.
        if let Some(lib) = app.settings.library.clone() {
            if let Some(i) = app.libraries.iter().position(|l| l.to_string_lossy() == lib) {
                app.lib_idx = i;
            }
        }
        app
    }

    fn rescan(&mut self) {
        self.steam = local::find_steam();
        self.games = self
            .steam
            .as_ref()
            .map(|s| local::scan_games(&s.dir))
            .unwrap_or_default();
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
        let _ = self.tx.send(Cmd::Install {
            appid,
            name,
            library,
        });
    }

    /// Поставить игру в очередь установки (или сразу начать, если ничего не качается).
    fn enqueue(&mut self, appid: u32, name: String, net: &NetState) {
        if let Some(q) = self.settings.downloads.iter_mut().find(|q| q.appid == appid) {
            q.paused = false; // уже в очереди на паузе — «Докачать» снимает паузу
            return;
        }
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        let waiting = self.settings.downloads.iter().any(|q| !q.paused);
        if !busy && !waiting && !self.settings.queue_paused {
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
            Some((t, v)) if now.duration_since(t) < std::time::Duration::from_secs(2) => v,
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
    pub(crate) fn start_update(&mut self, appid: u32, ctx: &egui::Context) {
        self.steam_task(appid, ctx, true);
    }

    /// «Играть» или «Обновить»: обе задачи поднимают скрытый Steam, поэтому идут по одной.
    fn steam_task(&mut self, appid: u32, ctx: &egui::Context, update: bool) {
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            return;
        }
        let Some(steam) = self.steam.clone() else {
            return;
        };
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else {
            return;
        };
        let opts = local::LaunchOptions {
            freeze_webhelper: self.freeze_webhelper,
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

    fn rows(&self, net: &NetState) -> Vec<Row> {
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
                });
        }
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<Row> = map
            .into_values()
            .filter(|r| !self.only_installed || r.installed)
            .filter(|r| r.name.to_lowercase().contains(&filter))
            .map(|mut r| {
                r.last_played = self.last_played.get(&r.appid).copied().unwrap_or(0);
                r
            })
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
                ui.add_space(4.0);
            }
        });
        if let Some((id, name)) = open {
            let ctx = ui.ctx().clone();
            self.open_chat(&ctx, id, name, net);
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
        // Если окно чата уже открыто, вынести его на передний план.
        ctx.send_viewport_cmd_to(chatwin::viewport_id(), egui::ViewportCommand::Focus);
    }

    fn account_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if let Some(name) = &net.account {
            ui.horizontal(|ui| {
                ui.label(format!("Аккаунт: {name}"));
                if ui
                    .add_enabled(!net.busy, egui::Button::new("Обновить библиотеку"))
                    .clicked()
                {
                    let _ = self.tx.send(Cmd::RefreshLibrary);
                }
                if ui.add_enabled(!net.busy, egui::Button::new("Выйти")).clicked() {
                    let _ = self.tx.send(Cmd::Logout);
                }
                let unread: u32 = net.chats.values().map(|c| c.unread).sum();
                let label = if unread > 0 {
                    format!("Друзья ({unread} новых)")
                } else {
                    "Друзья".to_string()
                };
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
fn draw_seg(ui: &mut egui::Ui, seg: chatfmt::Seg, anims: &mut anim::Cache) {
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
        // Снимок сетевого состояния на кадр, чтобы не держать блокировку.
        let net = self.net.lock().unwrap().clone();

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
        // Крестик прячет окно в трей, если так настроено (кроме «Выход» из меню трея).
        if ctx.input(|i| i.viewport().close_requested())
            && self.settings.close_to_tray
            && !tray::exit_requested()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            tray::hide_main();
        }

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
        // Сохранённый статус для друзей — после каждого входа.
        if net.account.is_none() {
            self.persona_sent = false;
        } else if !self.persona_sent {
            self.persona_sent = true;
            let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
        }
        // Проверка обновлений один раз после входа (и после работы Steam).
        if net.account.is_none() {
            self.updates_checked = false;
        } else if !self.updates_checked && !self.games.is_empty() {
            self.updates_checked = true;
            let list = self.games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
            let _ = self.tx.send(Cmd::CheckUpdates(list));
        }
        let installing = net.install.as_ref().is_some_and(|i| i.active());
        // Перерисовка раз в 1–2 с: полоса прогресса и кнопка «Закрыть Steam» не должны устаревать.
        ctx.request_repaint_after(std::time::Duration::from_secs(if installing { 1 } else { 2 }));

        egui::TopBottomPanel::top("account").show(ctx, |ui| {
            ui.add_space(4.0);
            self.account_panel(ui, &net);
            ui.add_space(4.0);
        });

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
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled(
                    !self.launching.load(Ordering::Relaxed),
                    egui::Checkbox::new(&mut self.freeze_webhelper, "Замораживать steamwebhelper"),
                )
                .on_hover_text("Во время игры и обновления (через 20 с) приостанавливает steamwebhelper и сбрасывает его память на диск. Потом размораживает.");
                if ui
                    .checkbox(&mut self.settings.notify_sound, "Звук сообщений")
                    .on_hover_text("Короткий звук, когда пришло сообщение, а окна Vaporust не активны.")
                    .changed()
                {
                    notify::set_sound(self.settings.notify_sound);
                }
                if ui
                    .small_button("проверить")
                    .on_hover_text("Сыграть звук сообщения (Windows: «Уведомление о мгновенном сообщении»).")
                    .clicked()
                {
                    notify::play_message_sound();
                }
                ui.checkbox(&mut self.settings.close_to_tray, "Крестик — в трей")
                    .on_hover_text("Крестик окна прячет Vaporust в область уведомлений (у часов), чат продолжает работать. Вернуть — щелчком по иконке, выйти — правый щелчок → «Выход».");
            });
            ui.add_space(2.0);
        });

        if net.account.is_some() && self.show_friends {
            egui::SidePanel::right("friends")
                .resizable(true)
                .default_width(230.0)
                .show(ctx, |ui| self.friends_panel(ui, &net));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Библиотека");
            ui.horizontal(|ui| {
                ui.label("Поиск:");
                ui.text_edit_singleline(&mut self.filter);
                ui.checkbox(&mut self.only_installed, "Только установленные");
                egui::ComboBox::from_id_salt("sort")
                    .selected_text(format!("Сортировка: {}", self.settings.sort.label()))
                    .show_ui(ui, |ui| {
                        for s in settings::Sort::ALL {
                            ui.selectable_value(&mut self.settings.sort, s, s.label());
                        }
                    });
                if ui.button("Пересканировать диск").clicked() {
                    self.rescan();
                }
            });
            if net.account.is_some() && self.libraries.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label("Устанавливать в:");
                    let label = |p: &std::path::PathBuf| {
                        let free = local::free_space(p)
                            .map(|f| format!(" (свободно {})", net::fmt_bytes(f)))
                            .unwrap_or_default();
                        format!("{}{free}", p.display())
                    };
                    let current = self.libraries.get(self.lib_idx).map(label).unwrap_or_default();
                    egui::ComboBox::from_id_salt("library")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (i, p) in self.libraries.iter().enumerate() {
                                ui.selectable_value(&mut self.lib_idx, i, label(p));
                            }
                        });
                });
            }
            ui.separator();

            let launching =
                self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
            let rows = self.rows(&net);
            let mut to_play = None;
            let mut to_update = None;
            let mut to_install = None;
            let mut to_ach = None;
            let mut to_opts = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for r in &rows {
                    ui.horizontal(|ui| {
                        if r.ready {
                            let play = ui
                                .add_enabled(!launching && !installing, egui::Button::new("Играть"))
                                .on_disabled_hover_text("Недоступно, пока идёт запуск или установка");
                            if play.clicked() {
                                to_play = Some(r.appid);
                            }
                            let needs = r.needs_update
                                || net.updates.as_ref().is_some_and(|u| u.contains(&r.appid));
                            if needs
                                && ui
                                    .add_enabled(
                                        !launching && !installing,
                                        egui::Button::new(egui::RichText::new("Обновить").color(egui::Color32::from_rgb(230, 170, 60))),
                                    )
                                    .on_hover_text("Есть обновление. Steam откроется незаметно, скачает его и закроется.")
                                    .clicked()
                            {
                                to_update = Some(r.appid);
                            }
                        } else {
                            let queued = self.settings.downloads.iter().any(|q| q.appid == r.appid && !q.paused)
                                || net.install.as_ref().is_some_and(|i| i.active() && i.appid == r.appid);
                            let text = if queued {
                                "В очереди"
                            } else if r.installed {
                                "Докачать"
                            } else {
                                "Установить"
                            };
                            let btn = ui
                                .add_enabled(net.account.is_some() && !launching && !queued, egui::Button::new(text))
                                .on_hover_text(if installing {
                                    "Сейчас идёт другая загрузка — игра встанет в очередь"
                                } else {
                                    "Скачать игру"
                                })
                                .on_disabled_hover_text("Нужен вход в аккаунт; игра уже в очереди или качается");
                            if btn.clicked() {
                                to_install = Some((r.appid, r.name.clone()));
                            }
                        }
                        if r.installed && ui.button("⚙").on_hover_text("Свойства игры: параметры запуска, язык, беты, файлы, DLC, облако").clicked() {
                            to_opts = Some((r.appid, r.name.clone()));
                        }
                        if net.account.is_some() && ui.button("🏆").on_hover_text("Достижения").clicked() {
                            to_ach = Some((r.appid, r.name.clone()));
                        }
                        let this_installing = net
                            .install
                            .as_ref()
                            .is_some_and(|i| i.active() && i.appid == r.appid);
                        if this_installing {
                            ui.strong(&r.name);
                            ui.spinner();
                        } else if r.ready {
                            ui.label(&r.name);
                        } else if r.installed {
                            ui.label(&r.name).on_hover_text("Установка не закончена");
                            ui.weak("(недокачана)");
                        } else {
                            ui.weak(&r.name).on_hover_text("Не установлена");
                        }
                        if let Some(m) = r.playtime_min.filter(|m| *m > 0) {
                            ui.weak(fmt_hours(m));
                        }
                        match self.settings.sort {
                            settings::Sort::Size if r.size > 0 => {
                                ui.weak(net::fmt_bytes(r.size));
                            }
                            settings::Sort::LastPlayed if r.last_played > 0 => {
                                ui.weak(format!("запуск: {}", fmt_day(r.last_played)));
                            }
                            _ => {}
                        }
                    });
                }
            });
            if let Some(appid) = to_play {
                self.start(appid, ctx);
            }
            if let Some(appid) = to_update {
                self.start_update(appid, ctx);
            }
            if let Some((appid, name)) = to_install {
                self.enqueue(appid, name, &net);
            }
            self.run_queue(&net);
            if let Some((appid, name)) = to_opts {
                self.open_props(appid, name);
            }
            if let Some((appid, name)) = to_ach {
                if !net.achievements.contains_key(&appid) {
                    let _ = self.tx.send(Cmd::Achievements(appid));
                }
                self.ach_open = Some((appid, name));
            }
        });

        self.achievements_window(ctx, &net);
        self.props_window(ctx, &net);
        chatwin::show(ctx, &self.chat, &self.net, &self.tx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 600.0])
            // Постоянный идентификатор: по нему eframe хранит настройки (%APPDATA%Vaporust),
            // поэтому они не теряются при смене версии в заголовке.
            .with_app_id("Vaporust")
            .with_icon(icons::icon(icons::Kind::Launcher)),
        ..Default::default()
    };
    eframe::run_native(
        concat!("Vaporust ", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}
