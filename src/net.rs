//! Работа с сетью Steam через библиотеку Aurelia.
//! Отдельный поток с tokio: интерфейс шлёт команды, поток обновляет общее состояние.

use aurelia::core::models::{
    DepotPlatform, DownloadProgressState, DownloadState, OwnedGame, QrEvent, SessionState,
};
use aurelia::core::secret::Secret;
use aurelia::steam_client::{GameAchievement, InstallOptions, Roster, SteamClient};
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub enum Cmd {
    Restore,
    Login {
        user: String,
        pass: String,
        code: Option<String>,
    },
    LoginQr,
    RefreshLibrary,
    Achievements(u32),
    /// Установить игру в указанную библиотеку Steam.
    Install {
        appid: u32,
        name: String,
        library: PathBuf,
    },
    /// Загрузить историю переписки с другом.
    OpenChat(u64),
    /// Отправить сообщение другу.
    SendChat { to: u64, text: String },
    /// Узнать названия игр по appid (для игр, в которые играют друзья).
    ResolveApps(Vec<u32>),
    /// Загрузить свои смайлики и стикеры (для панели выбора).
    LoadEmoticons,
    /// Отправить картинку из файла.
    SendImage { to: u64, path: PathBuf },
    /// Команда магазина Steam (поиск, страница игры, список желаемого...).
    Store(crate::store::StoreCmd),
    /// Статус для друзей (EPersonaState: 1 в сети, 2 не беспокоить, 3 отошёл, 7 невидимка).
    SetPersona(u32),
    /// Проверить, каким установленным играм (appid, папка) нужно обновление.
    CheckUpdates(Vec<(u32, PathBuf)>),
    /// Загрузить данные для окна «Свойства».
    LoadProps(u32),
    /// Включить/выключить DLC (base — игра, для перезагрузки списка).
    SetDlc { base: u32, dlc: u32, enabled: bool },
    /// Проверить целостность файлов игры.
    Verify { appid: u32, name: String },
    /// Перенести игру в другую библиотеку Steam.
    Move { appid: u32, name: String, dest: PathBuf },
    /// Удалить игру с диска.
    Uninstall { appid: u32, name: String },
    Logout,
}

/// Одно сообщение в переписке.
#[derive(Clone)]
pub struct ChatLine {
    pub from_self: bool,
    pub text: String,
    /// Unix-время, секунды.
    pub time: u64,
}

/// Переписка с одним другом.
#[derive(Clone, Default)]
pub struct Conversation {
    pub lines: Vec<ChatLine>,
    /// Непрочитанные входящие.
    pub unread: u32,
    pub history_loaded: bool,
    /// Ошибка загрузки/отправки, если была.
    pub error: Option<String>,
    /// Когда друг последний раз печатал (для «печатает...»).
    pub typing_at: Option<Instant>,
    /// Что сейчас происходит (например, «Отправляю картинку...»).
    pub status: Option<String>,
}

/// Данные окна «Свойства» одной игры; None — ещё грузится.
#[derive(Clone, Default)]
pub struct PropsData {
    pub languages: Option<Result<Vec<String>, String>>,
    pub branches: Option<Result<Vec<String>, String>>,
    pub dlc: Option<Result<Vec<DlcRow>, String>>,
    pub cloud: Option<Result<Vec<CloudFile>, String>>,
    /// Ошибка последнего действия (например, переключения DLC).
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct DlcRow {
    pub app_id: u32,
    pub owned: bool,
    pub installed: bool,
    pub disabled: bool,
}

#[derive(Clone)]
pub struct CloudFile {
    pub name: String,
    pub size: u64,
    pub time: u64,
}

/// Ход установки одной игры.
#[derive(Clone)]
pub struct Install {
    pub appid: u32,
    pub name: String,
    pub stage: String,
    pub done: u64,
    pub total: u64,
    /// Скорость, байт/с (сглаженная).
    pub speed: f64,
    /// None, пока идёт; Some(Ok) — готово; Some(Err) — ошибка или отмена.
    pub result: Option<Result<(), String>>,
    pub abort: Arc<AtomicBool>,
    /// Что написать при успехе («установлена.», «файлы в порядке.»...).
    pub done_text: String,
}

impl Install {
    pub fn active(&self) -> bool {
        self.result.is_none()
    }
}

#[derive(Default, Clone)]
pub struct NetState {
    /// Имя вошедшего аккаунта; None, если не вошли.
    pub account: Option<String>,
    /// Показать форму входа.
    pub need_login: bool,
    pub busy: bool,
    pub message: String,
    pub qr_url: Option<String>,
    pub qr_scanned: bool,
    pub owned: Vec<OwnedGame>,
    pub achievements: HashMap<u32, Result<Vec<GameAchievement>, String>>,
    /// Текущая или последняя установка.
    pub install: Option<Install>,
    /// Растёт, когда на диске что-то поменялось и библиотеку надо пересканировать.
    pub disk_changed: u64,
    /// Друзья (обновляет фоновая задача Aurelia).
    pub roster: Arc<RwLock<Roster>>,
    /// Переписки по SteamID64 друга.
    pub chats: HashMap<u64, Conversation>,
    /// Названия игр, узнанные по appid.
    pub app_names: HashMap<u32, String>,
    /// Свои смайлики и стикеры; None — ещё не загружены.
    pub emoticons: Option<(Vec<String>, Vec<String>)>,
    /// Данные окон «Свойства» по appid.
    pub props: HashMap<u32, PropsData>,
    /// Игры, которым нужно обновление (None — ещё не проверяли).
    pub updates: Option<std::collections::HashSet<u32>>,
    /// Данные магазина (отдельно: их много, а NetState копируется каждый кадр).
    pub store: crate::store::SharedStore,
}

pub type Shared = Arc<Mutex<NetState>>;

fn upd(st: &Shared, ctx: &egui::Context, f: impl FnOnce(&mut NetState)) {
    f(&mut st.lock().unwrap());
    ctx.request_repaint();
    // Окно чата — отдельный viewport: его тоже будим (новые сообщения, статусы).
    ctx.request_repaint_of(crate::chatwin::viewport_id());
}

pub fn spawn_worker(st: Shared, ctx: egui::Context) -> UnboundedSender<Cmd> {
    let (tx, mut rx) = unbounded_channel::<Cmd>();
    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                upd(&st, &ctx, |s| s.message = format!("Не удалось запустить сеть: {e}"));
                return;
            }
        };
        rt.block_on(async move {
            let mut client = match SteamClient::new() {
                Ok(c) => c,
                Err(e) => {
                    upd(&st, &ctx, |s| s.message = format!("Ошибка Aurelia: {e:#}"));
                    return;
                }
            };
            while let Some(cmd) = rx.recv().await {
                handle(&mut client, cmd, &st, &ctx).await;
            }
        });
    });
    tx
}

async fn handle(client: &mut SteamClient, cmd: Cmd, st: &Shared, ctx: &egui::Context) {
    match cmd {
        Cmd::Restore => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = "Вход в Steam...".into();
            });
            match client.restore_session().await {
                Ok(session) => logged_in(client, session, st, ctx).await,
                Err(_) => upd(st, ctx, |s| {
                    s.busy = false;
                    s.need_login = true;
                    s.message.clear();
                }),
            }
        }
        Cmd::Login { user, pass, code } => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = "Вход... Если включён мобильный аутентификатор, подтвердите вход в приложении Steam.".into();
            });
            match client.login(user, Secret::new(pass), code, false).await {
                Ok(session) => logged_in(client, session, st, ctx).await,
                Err(e) => {
                    let needs_code = !client.pending_confirmations().is_empty();
                    upd(st, ctx, |s| {
                        s.busy = false;
                        s.message = if needs_code {
                            "Steam просит код подтверждения. Введите код из письма или приложения \
                             и снова нажмите «Войти» (пароль тоже нужно ввести заново)."
                                .into()
                        } else {
                            format!("Не удалось войти: {e:#}")
                        };
                    });
                }
            }
        }
        Cmd::LoginQr => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = "Получаю QR-код...".into();
            });
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let result = client
                .login_qr(Duration::from_secs(300), move |ev| match ev {
                    QrEvent::Challenge(url) => upd(&st2, &ctx2, |s| {
                        s.qr_url = Some(url.to_string());
                        s.qr_scanned = false;
                        s.message.clear();
                    }),
                    QrEvent::Scanned => upd(&st2, &ctx2, |s| s.qr_scanned = true),
                })
                .await;
            upd(st, ctx, |s| {
                s.qr_url = None;
                s.qr_scanned = false;
            });
            match result {
                Ok(session) => logged_in(client, session, st, ctx).await,
                Err(e) => upd(st, ctx, |s| {
                    s.busy = false;
                    s.message = format!("Вход по QR не удался: {e:#}");
                }),
            }
        }
        Cmd::RefreshLibrary => {
            upd(st, ctx, |s| s.busy = true);
            load_owned(client, st, ctx).await;
        }
        Cmd::Achievements(appid) => {
            let res = client
                .fetch_achievements(appid, "russian")
                .await
                .or_else(|e| {
                    let msg = format!("{e:#}");
                    // У игры нет достижений: Steam отвечает ошибкой NoMatch, а не пустым списком.
                    if msg.contains("NoMatch") { Ok(Vec::new()) } else { Err(msg) }
                });
            upd(st, ctx, |s| {
                s.achievements.insert(appid, res);
            });
        }
        Cmd::Install {
            appid,
            name,
            library,
        } => {
            if st.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
                return;
            }
            let abort = Arc::new(AtomicBool::new(false));
            upd(st, ctx, |s| {
                s.install = Some(Install {
                    appid,
                    name,
                    stage: "Подготовка...".into(),
                    done: 0,
                    total: 0,
                    speed: 0.0,
                    result: None,
                    abort: abort.clone(),
                    done_text: "установлена.".into(),
                });
            });
            // Установка идёт в отдельной задаче, чтобы остальные команды не ждали.
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let res = run_install(client, appid, library, abort, &st, &ctx).await;
                upd(&st, &ctx, |s| {
                    if let Some(i) = s.install.as_mut() {
                        i.result = Some(res);
                        i.speed = 0.0;
                    }
                    s.disk_changed += 1;
                });
            });
        }
        Cmd::OpenChat(friend) => {
            let res = client.recent_chat_messages(friend, 50).await;
            upd(st, ctx, |s| {
                let conv = s.chats.entry(friend).or_default();
                conv.unread = 0;
                match res {
                    Ok(msgs) => {
                        // Steam отдаёт от новых к старым; живые сообщения, пришедшие
                        // до загрузки истории, уже входят в неё.
                        conv.lines = msgs
                            .into_iter()
                            .rev()
                            .map(|m| ChatLine {
                                from_self: m.from_self,
                                text: m.message,
                                time: m.timestamp as u64,
                            })
                            .collect();
                        conv.history_loaded = true;
                        conv.error = None;
                    }
                    Err(e) => conv.error = Some(format!("Не удалось загрузить историю: {e:#}")),
                }
            });
        }
        Cmd::ResolveApps(ids) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                for id in ids {
                    let (name, _) = client.resolve_install_game_info(id).await;
                    // Aurelia возвращает «App N», если название не нашлось.
                    if !name.starts_with("App ") {
                        upd(&st, &ctx, |s| {
                            s.app_names.insert(id, name);
                        });
                    }
                }
            });
        }
        Cmd::SendChat { to, text } => {
            let res = client.send_chat_message(to, text.clone()).await;
            upd(st, ctx, |s| {
                let conv = s.chats.entry(to).or_default();
                match res {
                    Ok(sent) => {
                        let time = if sent.server_timestamp > 0 {
                            sent.server_timestamp
                        } else {
                            now_unix()
                        };
                        // Команды (/sticker) и смайлики сервер переписывает; показываем сразу,
                        // а точный вид подтянется из истории ниже.
                        let special = text.starts_with('/') || text.contains('ː');
                        conv.lines.push(ChatLine {
                            from_self: true,
                            text: sent.modified_message.unwrap_or(text),
                            time,
                        });
                        conv.error = None;
                        if special {
                            conv.status = Some("Отправлено, обновляю...".into());
                        }
                    }
                    Err(e) => conv.error = Some(format!("Не отправлено: {e:#}")),
                }
            });
            let refresh = st.lock().unwrap().chats.get(&to).is_some_and(|c| c.status.is_some());
            if refresh {
                reload_history(client, to, st, ctx).await;
                upd(st, ctx, |s| s.chats.entry(to).or_default().status = None);
            }
        }
        Cmd::Store(cmd) => {
            let store = st.lock().unwrap().store.clone();
            crate::store::handle(client, cmd, &store, ctx);
        }
        Cmd::SetPersona(state) => {
            let c = client.clone();
            tokio::spawn(async move {
                // Наблюдатель друзей при входе сам объявляет статус из конфига Aurelia
                // (невидимка); ждём его и объявляем свой поверх.
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = c.announce_persona(state).await;
            });
        }
        Cmd::CheckUpdates(games) => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                if let Ok(ids) = c.games_needing_update(&games).await {
                    upd(&st, &ctx, |s| s.updates = Some(ids.into_iter().collect()));
                }
            });
        }
        Cmd::LoadProps(appid) => load_props(client, appid, st, ctx),
        Cmd::SetDlc { base, dlc, enabled } => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let res = c.set_dlc_enabled(dlc, enabled).await.map_err(|e| format!("{e:#}"));
                let list = load_dlc(&c, base).await;
                upd(&st, &ctx, |s| {
                    let p = s.props.entry(base).or_default();
                    p.dlc = Some(list);
                    p.error = res.err().map(|e| format!("DLC не переключено: {e}"));
                });
            });
        }
        Cmd::Verify { appid, name } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, format!("Проверка файлов: {name}"), "файлы в порядке.", st, ctx, move |abort| async move {
                let state = Arc::new(RwLock::new(DownloadState::default()));
                if let Ok(mut s) = state.write() {
                    s.abort_signal = abort.clone();
                }
                let rx = c
                    .verify_game(appid, state)
                    .await
                    .map_err(|e| format!("Не удалось начать проверку: {e:#}"))?;
                drain_progress(rx, &abort, "Проверка отменена.", &st2, &ctx2).await
            });
        }
        Cmd::Move { appid, name, dest } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, format!("Перенос: {name}"), "перенесена.", st, ctx, move |abort| async move {
                let rx = c
                    .move_install(appid, dest)
                    .await
                    .map_err(|e| format!("Не удалось начать перенос: {e:#}"))?;
                drain_progress(rx, &abort, "Перенос отменён (исходные файлы на месте).", &st2, &ctx2).await
            });
        }
        Cmd::Uninstall { appid, name } => {
            let c = client.clone();
            start_op(appid, format!("Удаление: {name}"), "удалена.", st, ctx, move |_| async move {
                c.uninstall_game(appid, false).await.map_err(|e| format!("Не удалось удалить: {e:#}"))
            });
        }
        Cmd::LoadEmoticons => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let lists = client.fetch_emoticons_and_stickers().await.unwrap_or_default();
                upd(&st, &ctx, |s| s.emoticons = Some(lists));
            });
        }
        Cmd::SendImage { to, path } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let set_status = |v: Option<String>, err: Option<String>| {
                    upd(&st, &ctx, |s| {
                        let c = s.chats.entry(to).or_default();
                        c.status = v;
                        if err.is_some() {
                            c.error = err;
                        }
                    })
                };
                set_status(Some("Отправляю картинку...".into()), None);
                let prepared = tokio::task::spawn_blocking(move || prepare_image(&path))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                let (bytes, ext, w, h) = match prepared {
                    Ok(p) => p,
                    Err(e) => return set_status(None, Some(e)),
                };
                let url = match client.upload_chat_image(to, bytes, ext, w, h).await {
                    Ok(u) => u,
                    Err(e) => return set_status(None, Some(format!("Картинка не отправлена: {e:#}"))),
                };
                // Похоже, при подтверждении загрузки Steam сам публикует картинку в чат.
                // Проверяем: если её нет в истории, отправляем сообщение сами.
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let posted = client
                    .recent_chat_messages(to, 5)
                    .await
                    .map(|m| m.iter().any(|m| m.message.contains(&url)))
                    .unwrap_or(false);
                if !posted {
                    let bb = format!("[img src={url} width={w} height={h}][url={url}]{url}[/url][/img]");
                    if let Err(e) = client.send_chat_message(to, bb).await {
                        return set_status(None, Some(format!("Картинка загружена, но не отправлена: {e:#}")));
                    }
                }
                reload_history(&client, to, &st, &ctx).await;
                set_status(None, None);
            });
        }
        Cmd::Logout => {
            let _ = client.logout().await;
            upd(st, ctx, |s| {
                *s = NetState {
                    need_login: true,
                    message: "Вы вышли из аккаунта.".into(),
                    ..Default::default()
                };
            });
        }
    }
}

async fn logged_in(client: &mut SteamClient, session: SessionState, st: &Shared, ctx: &egui::Context) {
    let name = session
        .persona_name
        .clone()
        .or(session.account_name.clone())
        .unwrap_or_else(|| "?".into());
    let roster = Arc::new(RwLock::new(Roster::new()));
    upd(st, ctx, |s| {
        s.account = Some(name);
        s.need_login = false;
        s.roster = roster.clone();
        s.chats.clear();
    });
    start_social(client, roster, st, ctx);
    load_owned(client, st, ctx).await;
}

/// Перечитать последние сообщения переписки (после отправки картинки).
async fn reload_history(client: &SteamClient, friend: u64, st: &Shared, ctx: &egui::Context) {
    if let Ok(msgs) = client.recent_chat_messages(friend, 50).await {
        upd(st, ctx, |s| {
            let conv = s.chats.entry(friend).or_default();
            conv.lines = msgs
                .into_iter()
                .rev()
                .map(|m| ChatLine {
                    from_self: m.from_self,
                    text: m.message,
                    time: m.timestamp as u64,
                })
                .collect();
            conv.history_loaded = true;
        });
    }
}

/// Прочитать картинку для отправки: (байты, тип для Steam, ширина, высота).
/// PNG/JPEG/GIF/WebP уходят как есть, остальное (BMP и т.п.) перекодируется в PNG.
fn prepare_image(path: &std::path::Path) -> Result<(Vec<u8>, &'static str, u32, u32), String> {
    use image::ImageFormat;
    let bytes = std::fs::read(path).map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("Файл больше 20 МБ — Steam такой не примет.".into());
    }
    let format = image::guess_format(&bytes).map_err(|_| "Это не картинка (или формат не поддерживается).".to_string())?;
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| format!("Не удалось прочитать картинку: {e}"))?;
    let ext = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| format!("Не удалось прочитать картинку: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, ImageFormat::Png)
                .map_err(|e| format!("Не удалось перекодировать картинку: {e}"))?;
            return Ok((out.into_inner(), "png", w, h));
        }
    };
    Ok((bytes, ext, w, h))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Фоновые задачи на время сессии: список друзей и входящие сообщения.
/// Обе заканчиваются сами, когда соединение закрывается (выход из аккаунта).
fn start_social(client: &SteamClient, roster: Arc<RwLock<Roster>>, st: &Shared, ctx: &egui::Context) {
    // Друзья: Aurelia сама объявляет статус (по умолчанию «невидимка») и следит за изменениями.
    {
        let (client, ctx) = (client.clone(), ctx.clone());
        tokio::spawn(async move {
            // Перерисовка раз в 2 с, пока работает наблюдатель, чтобы статусы друзей обновлялись.
            let ctx2 = ctx.clone();
            let ticker = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    ctx2.request_repaint();
                }
            });
            let _ = client.run_friends_watcher(roster).await;
            ticker.abort();
        });
    }
    // Входящие сообщения.
    let Ok(chat) = client.chat_client() else {
        return;
    };
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        use steam_vent_chat::ChatEvent;
        use tokio_stream::StreamExt;
        let mut events = std::pin::pin!(chat.listen());
        while let Some(ev) = events.next().await {
            let Ok(ev) = ev else { continue }; // пропущенные уведомления — слушаем дальше
            let (from_self, m) = match ev {
                ChatEvent::Message(m) => (false, m),
                // Своё сообщение, отправленное с другого устройства (например, из Steam на телефоне).
                ChatEvent::EchoMessage(m) => (true, m),
                ChatEvent::GameInvite(mut m) => {
                    m.message = "[приглашение в игру]".into();
                    m.message_no_bbcode = None;
                    (false, m)
                }
                ChatEvent::Typing(t) => {
                    let friend = u64::from(t.source);
                    upd(&st, &ctx, |s| {
                        s.chats.entry(friend).or_default().typing_at = Some(Instant::now());
                    });
                    continue;
                }
                _ => continue,
            };
            let friend = u64::from(m.source);
            let line = ChatLine {
                from_self,
                text: m.message, // с BBCode: разбирает chatfmt
                time: if m.server_timestamp > 0 { m.server_timestamp } else { now_unix() },
            };
            upd(&st, &ctx, |s| {
                let conv = s.chats.entry(friend).or_default();
                conv.lines.push(line);
                conv.typing_at = None;
                if !from_self {
                    conv.unread += 1;
                }
            });
            if !from_self {
                // Мигание в панели задач, звук и точка на иконке трея, если окна неактивны.
                crate::notify::new_message(friend);
            }
        }
    });
}

async fn load_owned(client: &mut SteamClient, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| s.message = "Загружаю библиотеку...".into());
    match client.fetch_owned_games().await {
        Ok(games) => upd(st, ctx, |s| {
            s.message = format!("Игр на аккаунте: {}", games.len());
            s.owned = games;
            s.busy = false;
        }),
        Err(e) => upd(st, ctx, |s| {
            s.message = format!("Не удалось получить список игр: {e:#}");
            s.busy = false;
        }),
    }
}

fn set_install(st: &Shared, ctx: &egui::Context, f: impl FnOnce(&mut Install)) {
    upd(st, ctx, |s| {
        if let Some(i) = s.install.as_mut() {
            f(i);
        }
    });
}

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    const MB: f64 = 1024.0 * 1024.0;
    if b >= 1024.0 * MB {
        format!("{:.2} ГБ", b / 1024.0 / MB)
    } else if b >= MB {
        format!("{:.1} МБ", b / MB)
    } else {
        format!("{:.0} КБ", b / 1024.0)
    }
}

const CANCELLED: &str = "Установка отменена. Скачанное сохранено, её можно продолжить.";

async fn run_install(
    client: SteamClient,
    appid: u32,
    library: PathBuf,
    abort: Arc<AtomicBool>,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    set_install(st, ctx, |i| i.stage = "Узнаю размер игры...".into());
    let est = client
        .estimate_install_size(appid, DepotPlatform::Windows)
        .await
        .map_err(|e| format!("Не удалось узнать размер игры: {e:#}"))?;
    // Уже скачанное (продолжение установки) место не требует.
    let game_files = crate::local::installed_size(&library, appid);
    if let Some(free) = crate::local::free_space(&library) {
        let need = est.disk_size.saturating_sub(game_files);
        if need > free {
            return Err(format!(
                "Не хватает места на диске: нужно {}, свободно {}.",
                fmt_bytes(need),
                fmt_bytes(free)
            ));
        }
    }
    set_install(st, ctx, |i| i.total = est.disk_size);
    if abort.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    let state = Arc::new(RwLock::new(DownloadState::default()));
    // Флаг отмены Aurelia подменяем на наш, чтобы кнопка «Отмена» работала сразу.
    if let Ok(mut s) = state.write() {
        s.abort_signal = abort.clone();
    }
    let opts = InstallOptions {
        library_override: Some(library.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let rx = client
        .install_game(appid, DepotPlatform::Windows, opts, state)
        .await
        .map_err(|e| format!("Не удалось начать установку: {e:#}"))?;
    drain_progress(rx, &abort, CANCELLED, st, ctx).await
}

/// Общий цикл прогресса Aurelia (установка, проверка файлов, перенос): обновляет полосу
/// прогресса внизу окна, считает скорость. `cancelled` — текст на случай отмены.
async fn drain_progress(
    mut rx: tokio::sync::mpsc::Receiver<aurelia::core::models::DownloadProgress>,
    abort: &Arc<AtomicBool>,
    cancelled: &str,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    // Скорость: замер раз в ~2 с со сглаживанием.
    let mut last: Option<(Instant, u64)> = None;
    let mut speed = 0.0f64;
    while let Some(p) = rx.recv().await {
        match p.state {
            DownloadProgressState::Queued => {
                set_install(st, ctx, |i| i.stage = "Подготовка загрузки...".into());
            }
            DownloadProgressState::Downloading
            | DownloadProgressState::Verifying
            | DownloadProgressState::Moving => {
                let net_bytes = p.bytes_downloaded.saturating_sub(p.reused_bytes);
                let now = Instant::now();
                match last {
                    Some((t, b)) if now.duration_since(t) >= Duration::from_secs(2) => {
                        let secs = now.duration_since(t).as_secs_f64();
                        let inst = net_bytes.saturating_sub(b) as f64 / secs;
                        speed = if speed == 0.0 { inst } else { speed * 0.6 + inst * 0.4 };
                        last = Some((now, net_bytes));
                    }
                    None => last = Some((now, net_bytes)),
                    _ => {}
                }
                let stage = match p.state {
                    DownloadProgressState::Verifying => "Проверка файлов",
                    DownloadProgressState::Moving => "Перенос файлов",
                    _ => "Загрузка",
                };
                set_install(st, ctx, |i| {
                    i.stage = stage.into();
                    i.done = p.bytes_downloaded;
                    if p.total_bytes > 0 {
                        i.total = p.total_bytes;
                    }
                    i.speed = speed;
                });
            }
            DownloadProgressState::Completed => return Ok(()),
            DownloadProgressState::Failed => {
                return Err(if abort.load(Ordering::Relaxed) {
                    cancelled.into()
                } else {
                    format!("Ошибка: {}", p.current_file)
                });
            }
        }
    }
    Err(if abort.load(Ordering::Relaxed) {
        cancelled.into()
    } else {
        "Операция прервалась без результата.".into()
    })
}

/// Длинная операция над игрой в слоте `install` (одна за раз): полоса прогресса внизу,
/// по окончании — текст результата и перескан диска.
fn start_op<F, Fut>(appid: u32, name: String, done_text: &str, st: &Shared, ctx: &egui::Context, f: F)
where
    F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    if st.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
        return;
    }
    let abort = Arc::new(AtomicBool::new(false));
    upd(st, ctx, |s| {
        s.install = Some(Install {
            appid,
            name,
            stage: "Подготовка...".into(),
            done: 0,
            total: 0,
            speed: 0.0,
            result: None,
            abort: abort.clone(),
            done_text: done_text.into(),
        });
    });
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        let res = f(abort).await;
        upd(&st, &ctx, |s| {
            if let Some(i) = s.install.as_mut() {
                i.result = Some(res);
                i.speed = 0.0;
            }
            s.disk_changed += 1;
        });
    });
}

/// Ошибку Aurelia (anyhow) в текст с причинами.
fn es<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Загрузить всё для окна «Свойства» (параллельно, по мере готовности).
fn load_props(client: &SteamClient, appid: u32, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| {
        s.props.insert(appid, PropsData::default());
    });
    let set = move |st: &Shared, ctx: &egui::Context, f: Box<dyn FnOnce(&mut PropsData) + Send>| {
        upd(st, ctx, |s| f(s.props.entry(appid).or_default()));
    };

    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        tokio::spawn(async move {
            let r = c.fetch_game_languages(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.languages = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        tokio::spawn(async move {
            let r = c.fetch_branches(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.branches = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        tokio::spawn(async move {
            let r = load_dlc(&c, appid).await;
            set(&st, &ctx, Box::new(move |p| p.dlc = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        tokio::spawn(async move {
            let r = async {
                let mut files = c.cloud_client().map_err(es)?.get_file_list(appid).await.map_err(es)?;
                files.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                Ok(files
                    .into_iter()
                    .map(|f| CloudFile { name: f.filename, size: f.size, time: f.timestamp as u64 })
                    .collect())
            }
            .await;
            set(&st, &ctx, Box::new(move |p| p.cloud = Some(r)));
        });
    }
}

async fn load_dlc(c: &SteamClient, appid: u32) -> Result<Vec<DlcRow>, String> {
    let ext = c.get_extended_app_info(appid).await.map_err(|e| format!("{e:#}"))?;
    let states = c.dlc_states(appid, &ext.dlcs).await.map_err(|e| format!("{e:#}"))?;
    Ok(states
        .into_iter()
        .map(|s| DlcRow { app_id: s.app_id, owned: s.owned, installed: s.installed, disabled: s.disabled })
        .collect())
}

#[cfg(test)]
mod live {
    /// Ручная проверка на настоящем аккаунте (только чтение):
    /// cargo test live_emoticons -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_emoticons() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (e, s) = c.fetch_emoticons_and_stickers().await.unwrap();
            println!("emoticons {}: {:?}", e.len(), &e[..e.len().min(15)]);
            println!("stickers {}: {:?}", s.len(), &s[..s.len().min(15)]);
        });
    }
}

#[cfg(test)]
mod live_props {
    /// cargo test live_props_data -- --ignored --nocapture (только чтение)
    #[test]
    #[ignore]
    fn live_props_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for app in [70u32, 2280] {
                println!("== {app}");
                println!("langs {:?}", c.fetch_game_languages(app).await.map_err(|e| e.to_string()));
                println!("branches {:?}", c.fetch_branches(app).await.map_err(|e| e.to_string()));
                let ext = c.get_extended_app_info(app).await.unwrap();
                println!("dlcs {:?} branch {}", ext.dlcs, ext.active_branch);
                let st = c.dlc_states(app, &ext.dlcs).await.map_err(|e| e.to_string());
                println!("dlc states {:?}", st.map(|v| v.iter().map(|s| (s.app_id, s.owned, s.installed, s.disabled)).collect::<Vec<_>>()));
                let cloud = c.cloud_client().unwrap().get_file_list(app).await.map(|f| f.len()).map_err(|e| e.to_string());
                println!("cloud files {:?}", cloud);
            }
        });
    }
}

#[cfg(test)]
mod live_updates {
    /// cargo test live_updates_check -- --ignored --nocapture (только чтение)
    #[test]
    #[ignore]
    fn live_updates_check() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let list: Vec<_> = games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let t = std::time::Instant::now();
            let ids = c.games_needing_update(&list).await.unwrap();
            let names: Vec<_> = ids.iter().filter_map(|i| games.iter().find(|g| g.appid == *i)).map(|g| g.name.clone()).collect();
            println!("checked {} in {:.1}s, need update: {:?}", list.len(), t.elapsed().as_secs_f32(), names);
        });
    }
}

#[cfg(test)]
mod live_store {
    /// cargo test live_store_data -- --ignored --nocapture (только чтение)
    #[test]
    #[ignore]
    fn live_store_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let cc = c.store_country();
            println!("country {cc} me {:?}", c.my_steam_id());
            let s = c.search_store("doom", 5, "russian", &cc).await.unwrap();
            println!("search {:?} total {}", s.app_ids, s.total);
            let apps = c.fetch_store_apps(&[70, 2280], "russian", &cc).await.unwrap();
            for a in &apps {
                println!("== {} {} price {:?} orig {:?} disc {} review {:?} rel {:?}", a.app_id, a.name, a.price, a.original_price, a.discount_pct, a.review_summary, a.release_date);
                println!("dev {:?} pub {:?} tags {:?}", a.developers, a.publishers, a.tags.iter().take(5).map(|t| t.name.clone()).collect::<Vec<_>>());
                println!("assets {:?}", a.assets);
                println!("shots {} first {:?}", a.screenshots.len(), a.screenshots.first());
                println!("short {:?}", a.short_description.chars().take(150).collect::<String>());
                println!("full {:?}", a.full_description.chars().take(300).collect::<String>());
            }
            let w = c.wishlist(c.my_steam_id().unwrap()).await.map(|w| w.len());
            println!("wishlist {:?}", w.map_err(|e| e.to_string()));
            let d = c.query_deals(aurelia::steam_client::DealsScope::DiscountedTopSellers, 1, 0, 10, "russian", &cc).await.map(|p| p.app_ids.len());
            println!("deals {:?}", d.map_err(|e| e.to_string()));
            let sim = c.similar_apps(70, 5, "russian", &cc).await.map(|p| p.app_ids);
            println!("similar {:?}", sim.map_err(|e| e.to_string()));
            println!("players {:?}", c.current_players(70).await.map_err(|e| e.to_string()));
            let http = aurelia::core::net::steam_web_client().unwrap();
            let news = aurelia::web::discovery::fetch_news(&http, 70, 3, 300).await.map(|n| n.iter().map(|x| x.title.clone()).collect::<Vec<_>>());
            println!("news {:?}", news.map_err(|e| e.to_string()));
        });
    }
}
