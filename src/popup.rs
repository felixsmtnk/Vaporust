//! Свои всплывающие уведомления Vaporust: небольшие карточки в правом нижнем углу экрана
//! (над панелью задач), поверх всех окон, не забирают фокус. До 3 штук стопкой, каждая
//! живет 6 с (пока над ней мышь - не исчезает). Щелчок - действие (чат с другом, страница
//! игры), крестик - закрыть. У каждого вида уведомления свой короткий звук (синтезируется в
//! памяти, без файлов).
//!
//! Добавлять можно из любого потока (`push`), рисует главный цикл (`LauncherApp::popups_ui`).

use crate::LauncherApp;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Что за уведомление и что делать по щелчку.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Новое сообщение (SteamID друга) - открыть чат.
    Message(u64),
    /// Друг зашел в игру - открыть чат.
    FriendGame(u64),
    /// Друг появился в сети - открыть чат.
    FriendOnline(u64),
    /// Скидка на игру из желаемого - страница в магазине (0 - несколько игр: список желаемого).
    Sale(u32),
    /// Игра скачана - страница игры в библиотеке.
    Download(u32),
    /// Просто сообщение (проверка) - открыть Vaporust.
    Info,
    /// Сообщение об игре (удалена, закрыта, ошибка запуска) - страница игры.
    Game(u32),
}

struct Popup {
    kind: Kind,
    title: String,
    body: String,
    /// Картинка справа (смайлик или стикер из сообщения).
    image: Option<String>,
    /// Цвет полоски и имени; None - по виду уведомления.
    accent: Option<egui::Color32>,
    /// Когда показали (сбрасывается, пока над карточкой мышь).
    shown: Instant,
}

const LIFE: Duration = Duration::from_secs(6);
const MAX: usize = 3;
const W: f32 = 340.0;
const H: f32 = 88.0;
const GAP: f32 = 8.0;

static QUEUE: Mutex<Vec<Popup>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();
static SOUND: AtomicBool = AtomicBool::new(true);
/// Щелчки по карточкам - забирает главный цикл.
static CLICKED: Mutex<Vec<Kind>> = Mutex::new(Vec::new());

pub fn init(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

pub fn set_sound(on: bool) {
    SOUND.store(on, Ordering::Relaxed);
}

/// Показать уведомление (из любого потока). Повтор того же вида (например, второе сообщение
/// от того же друга) заменяет старую карточку, а не добавляет новую.
pub fn push(kind: Kind, title: impl Into<String>, body: impl Into<String>) {
    push_full(kind, title.into(), body.into(), None, None);
}

/// То же с картинкой и своим цветом (статус друга: в игре - зеленый, в сети - синий).
pub fn push_full(kind: Kind, title: String, body: String, image: Option<String>, accent: Option<egui::Color32>) {
    let p = Popup {
        kind,
        title,
        body,
        image,
        accent,
        shown: Instant::now(),
    };
    {
        let mut q = QUEUE.lock().unwrap();
        q.retain(|x| x.kind != kind || kind == Kind::Info);
        q.push(p);
        let extra = q.len().saturating_sub(MAX);
        q.drain(..extra);
    }
    if SOUND.load(Ordering::Relaxed) {
        sound::play(kind);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// Щелчки по уведомлениям с прошлого кадра.
pub fn take_clicks() -> Vec<Kind> {
    std::mem::take(&mut *CLICKED.lock().unwrap())
}

/// Рабочая область основного монитора (без панели задач), физические пиксели.
#[cfg(windows)]
fn work_area() -> Option<egui::Rect> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut _ as _, 0) == 0 {
            return None;
        }
        Some(egui::Rect::from_min_max(
            egui::pos2(r.left as f32, r.top as f32),
            egui::pos2(r.right as f32, r.bottom as f32),
        ))
    }
}

#[cfg(not(windows))]
fn work_area() -> Option<egui::Rect> {
    None
}

impl LauncherApp {
    /// Нарисовать уведомления (отдельным окном без рамки в углу экрана).
    pub(crate) fn popups_ui(&mut self, ctx: &egui::Context) {
        let n = {
            let mut q = QUEUE.lock().unwrap();
            q.retain(|p| p.shown.elapsed() < LIFE);
            q.len()
        };
        if n == 0 {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        // Аватары друзей из уведомлений - из списка друзей.
        let avatars: std::collections::HashMap<u64, String> = {
            let roster = self.net.lock().unwrap().roster.clone();
            let roster = roster.read().unwrap();
            roster
                .values()
                .filter_map(|f| f.avatar_hash.as_ref().map(|h| (f.steam_id, format!("https://avatars.fastly.steamstatic.com/{h}_medium.jpg"))))
                .collect()
        };
        let ppp = ctx.pixels_per_point();
        let size = egui::vec2(W, n as f32 * (H + GAP) - GAP);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Vaporust: уведомления")
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top()
            // Прозрачный фон окна - видны скругленные углы карточек и промежутки между ними.
            .with_transparent(true)
            .with_taskbar(false)
            .with_active(false)
            .with_inner_size(size);
        if let Some(wa) = work_area() {
            // Правый нижний угол рабочей области, отступ 12 точек.
            let pos = egui::pos2(wa.max.x / ppp - size.x - 12.0, wa.max.y / ppp - size.y - 12.0);
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("vaporust_popups"), builder, |ctx, _| {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    let mut q = QUEUE.lock().unwrap();
                    let mut close = None;
                    for (i, p) in q.iter_mut().enumerate() {
                        if i > 0 {
                            ui.add_space(GAP);
                        }
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, H), egui::Sense::click());
                        let hovered = resp.hovered();
                        if hovered {
                            p.shown = Instant::now(); // пока над карточкой мышь - не исчезает
                        }
                        let painter = ui.painter_at(rect);
                        // Карточка в цветах программы: темная, тонкая рамка, при наведении светлее.
                        let accent_color = p.accent.unwrap_or_else(|| color(p.kind));
                        // Как остальные окна программы: темная карточка с цветной полоской слева (вид уведомления).
                        let bg = if hovered { egui::Color32::from_rgb(42, 50, 63) } else { crate::theme::CARD };
                        painter.rect_filled(rect, 8.0, bg);
                        // Легкий светлый блик сверху - карточка не выглядит плоской и пустой.
                        let top = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() * 0.45));
                        painter.rect_filled(top, egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 }, egui::Color32::from_white_alpha(6));
                        painter.rect_stroke(rect, 8.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 68, 86)));
                        // У друзей цвет статуса - только рамка аватара; у остальных - полоска слева.
                        let friend = matches!(p.kind, Kind::Message(_) | Kind::FriendGame(_) | Kind::FriendOnline(_));
                        if !friend {
                            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
                            painter.rect_filled(bar, egui::Rounding { nw: 8.0, sw: 8.0, ne: 0.0, se: 0.0 }, accent_color);
                        }
                        // Слева, как в Steam: аватар друга в рамке цвета статуса или картинка игры.
                        let icon = egui::Rect::from_min_size(egui::pos2(rect.left() + 14.0, rect.top() + (H - 56.0) / 2.0), egui::vec2(56.0, 56.0));
                        match p.kind {
                            Kind::Message(id) | Kind::FriendGame(id) | Kind::FriendOnline(id) => {
                                match avatars.get(&id) {
                                    Some(u) => {
                                        egui::Image::new(u.as_str()).rounding(5.0).paint_at(ui, icon);
                                    }
                                    None => {
                                        painter.rect_filled(icon, 3.0, crate::theme::SIDEBAR);
                                    }
                                }
                                painter.rect_stroke(icon.expand(1.0), 5.0, egui::Stroke::new(2.0, accent_color));
                            }
                            Kind::Sale(a) | Kind::Download(a) | Kind::Game(a) if a != 0 => {
                                // Квадрат из середины шапки игры (460x215).
                                let url = format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg");
                                let k = 215.0 / 460.0;
                                egui::Image::new(url)
                                    .uv(egui::Rect::from_min_max(egui::pos2(0.5 - k / 2.0, 0.0), egui::pos2(0.5 + k / 2.0, 1.0)))
                                    .rounding(5.0)
                                    .paint_at(ui, icon);
                            }
                            _ => {
                                painter.rect_filled(icon, 3.0, crate::theme::SIDEBAR);
                                painter.circle_filled(icon.center(), 9.0, accent_color);
                            }
                        }
                        // Смайлик или стикер из сообщения - справа, перед крестиком.
                        let mut text_w = W - 82.0 - 38.0;
                        if let Some(url) = &p.image {
                            let img_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.right() - 38.0 - 52.0, rect.top() + (H - 52.0) / 2.0 + 6.0),
                                egui::vec2(52.0, 52.0),
                            );
                            egui::Image::new(url.as_str()).paint_at(ui, img_rect);
                            text_w -= 58.0;
                        }
                        // Сверху мелко - что это за уведомление, ниже имя (или заголовок) и текст.
                        let title_color = egui::Color32::WHITE;
                        let kind_text = text(ui, kind_label(p.kind), 10.5, crate::theme::WEAK, text_w, 1);
                        let show_title = !(p.title == "Vaporust" && matches!(p.kind, Kind::Game(_) | Kind::Info));
                        let title = text(ui, if show_title { &p.title } else { "" }, 15.0, title_color, text_w, 1);
                        let body = text(ui, &p.body, 12.5, crate::theme::TEXT, text_w, if show_title { 2 } else { 3 });
                        let x = rect.left() + 82.0;
                        let gap = 3.0;
                        let title_h = if show_title { title.size().y + gap } else { 0.0 };
                        let th = kind_text.size().y + gap + title_h + body.size().y;
                        let y = rect.center().y - th / 2.0;
                        let kind_h = kind_text.size().y + gap;
                        painter.galley(egui::pos2(x, y), kind_text, title_color);
                        if show_title {
                            painter.galley(egui::pos2(x, y + kind_h), title, title_color);
                        }
                        painter.galley(egui::pos2(x, y + kind_h + title_h), body, crate::theme::TEXT);
                        // Крестик: крупнее, с подсветкой при наведении.
                        let xr = egui::Rect::from_center_size(egui::pos2(rect.right() - 18.0, rect.top() + 18.0), egui::vec2(26.0, 26.0));
                        let over_x = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| xr.contains(h));
                        if over_x {
                            painter.rect_filled(xr, 4.0, egui::Color32::from_rgb(70, 80, 98));
                        }
                        let xc = if over_x { egui::Color32::WHITE } else if hovered { egui::Color32::from_gray(185) } else { egui::Color32::from_gray(125) };
                        let (c, d) = (xr.center(), 5.0);
                        painter.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], egui::Stroke::new(1.6, xc));
                        painter.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], egui::Stroke::new(1.6, xc));
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            if !over_x {
                                CLICKED.lock().unwrap().push(p.kind);
                            }
                            close = Some(i);
                        }
                    }
                    if let Some(i) = close {
                        q.remove(i);
                    }
                });
        });
    }
}

/// Текст не шире `width` и не больше `rows` строк (лишнее обрезается многоточием).
fn text(ui: &egui::Ui, s: &str, size: f32, color: egui::Color32, width: f32, rows: usize) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_string(), egui::FontId::proportional(size), color, width);
    job.wrap.max_rows = rows;
    ui.fonts(|f| f.layout_job(job))
}

/// Подпись вида уведомления (мелко над заголовком).
fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Message(_) => "НОВОЕ СООБЩЕНИЕ",
        Kind::FriendGame(_) => "ДРУГ ИГРАЕТ",
        Kind::FriendOnline(_) => "ДРУГ В СЕТИ",
        Kind::Sale(_) => "СКИДКА В СПИСКЕ ЖЕЛАЕМОГО",
        Kind::Download(_) => "ЗАГРУЗКА ЗАВЕРШЕНА",
        Kind::Game(_) => "БИБЛИОТЕКА",
        Kind::Info => "VAPORUST",
    }
}

fn color(k: Kind) -> egui::Color32 {
    match k {
        Kind::Message(_) => egui::Color32::from_rgb(90, 160, 240),
        Kind::FriendGame(_) => egui::Color32::from_rgb(140, 200, 80),
        Kind::FriendOnline(_) => egui::Color32::from_rgb(90, 190, 220),
        Kind::Sale(_) => egui::Color32::from_rgb(240, 180, 60),
        Kind::Download(_) => egui::Color32::from_rgb(120, 210, 140),
        Kind::Info => egui::Color32::from_gray(150),
        Kind::Game(_) => egui::Color32::from_gray(150),
    }
}

/// Короткие мелодичные звуки, синтезированные в памяти (WAV), свой для каждого вида.
mod sound {
    use super::Kind;
    use std::sync::OnceLock;

    const RATE: u32 = 44_100;

    /// Ноты: (частота Гц, начало мс, длительность мс).
    fn notes(k: Kind) -> &'static [(f32, u32, u32)] {
        match k {
            // Сообщение: два быстрых тона вверх.
            Kind::Message(_) => &[(1046.5, 0, 160), (1568.0, 90, 260)],
            // Друг в игре: три ноты мажорного аккорда.
            Kind::FriendGame(_) => &[(523.3, 0, 260), (659.3, 90, 260), (784.0, 180, 380)],
            // Друг в сети: одна мягкая нота.
            Kind::FriendOnline(_) => &[(880.0, 0, 300)],
            // Скидка: звонкое «дзинь-дзинь».
            Kind::Sale(_) => &[(1318.5, 0, 200), (1760.0, 110, 200), (2093.0, 220, 380)],
            // Загрузка готова: вверх через октаву.
            Kind::Download(_) => &[(392.0, 0, 220), (587.3, 120, 220), (784.0, 240, 450)],
            Kind::Info | Kind::Game(_) => &[(987.8, 0, 260)],
        }
    }

    /// WAV (16 бит, моно) из нот: синус с обертоном, мягкая атака и затухание.
    pub fn wav(k: Kind) -> Vec<u8> {
        let ns = notes(k);
        let total_ms = ns.iter().map(|n| n.1 + n.2).max().unwrap_or(0) + 30;
        let len = (RATE * total_ms / 1000) as usize;
        let mut buf = vec![0f32; len];
        for &(f, start, dur) in ns {
            let s0 = (RATE * start / 1000) as usize;
            let n = (RATE * dur / 1000) as usize;
            for i in 0..n {
                let t = i as f32 / RATE as f32;
                let attack = (i as f32 / (RATE as f32 * 0.006)).min(1.0);
                let decay = (-t * 9.0).exp();
                let v = (2.0 * std::f32::consts::PI * f * t).sin() + 0.25 * (4.0 * std::f32::consts::PI * f * t).sin();
                if let Some(b) = buf.get_mut(s0 + i) {
                    *b += v * attack * decay;
                }
            }
        }
        let gain = 0.22;
        let mut out = Vec::with_capacity(44 + len * 2);
        let data_len = (len * 2) as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // моно
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for v in buf {
            let s = (v * gain).clamp(-1.0, 1.0);
            out.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
        }
        out
    }

    fn cached(k: Kind) -> &'static [u8] {
        static C: [OnceLock<Vec<u8>>; 6] =
            [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
        let i = match k {
            Kind::Message(_) => 0,
            Kind::FriendGame(_) => 1,
            Kind::FriendOnline(_) => 2,
            Kind::Sale(_) => 3,
            Kind::Download(_) => 4,
            Kind::Info | Kind::Game(_) => 5,
        };
        C[i].get_or_init(|| wav(k))
    }

    #[cfg(windows)]
    pub fn play(k: Kind) {
        use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
        let data = cached(k);
        unsafe {
            PlaySoundW(data.as_ptr() as _, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
        }
    }

    #[cfg(not(windows))]
    pub fn play(_k: Kind) {}
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn wav_header() {
        let w = super::sound::wav(Kind::Message(1));
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert!(w.len() > 1000);
    }

    /// cargo test sounds_live -- --ignored: проиграть все звуки по очереди.
    #[test]
    #[ignore]
    fn sounds_live() {
        for k in [Kind::Message(1), Kind::FriendGame(1), Kind::FriendOnline(1), Kind::Sale(1), Kind::Download(1)] {
            super::sound::play(k);
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
    }
}

impl LauncherApp {
    /// Щелчок по уведомлению: показать Vaporust и открыть нужное.
    pub(crate) fn popup_clicks(&mut self, ctx: &egui::Context, net: &crate::net::NetState) {
        for k in take_clicks() {
            crate::tray::show_main();
            match k {
                Kind::Message(id) | Kind::FriendGame(id) | Kind::FriendOnline(id) => {
                    let name = net
                        .roster
                        .read()
                        .ok()
                        .and_then(|r| r.get(&id).and_then(|f| f.persona_name.clone()))
                        .unwrap_or_else(|| id.to_string());
                    self.open_chat(ctx, id, name, net);
                }
                Kind::Sale(0) => self.open_wishlist(),
                Kind::Sale(appid) => self.open_store(Some(appid), false),
                Kind::Download(appid) | Kind::Game(appid) => {
                    self.view = crate::View::Library;
                    self.lib_page = Some(appid);
                    let _ = self.tx.send(crate::net::Cmd::Store(crate::store::StoreCmd::Page(appid)));
                }
                Kind::Info => {}
            }
        }
    }
}

/// Цвет статуса друга, как в списке друзей: в игре - зеленый, в сети - синий, отошел - серо-синий.
pub fn status_color(state: Option<u32>, in_game: bool) -> Option<egui::Color32> {
    Some(match (state.unwrap_or(0), in_game) {
        (_, true) => egui::Color32::from_rgb(140, 200, 80),
        (0, _) => return None,
        (2, _) => egui::Color32::from_rgb(220, 120, 110),
        (3 | 4, _) => egui::Color32::from_rgb(110, 140, 170),
        _ => egui::Color32::from_rgb(90, 170, 230),
    })
}
