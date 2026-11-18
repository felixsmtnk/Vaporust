//! Vaporust's own popup notifications: small cards in the bottom right corner of the screen
//! (above the taskbar), on top of all windows, they do not take focus. Up to 3 stacked, each
//! lives 6 s (does not disappear while hovered). Click - the action (chat with a friend, game
//! page), cross - close. Each notification kind has its own short sound (synthesized in
//! memory, no files).
//!
//! Can be added from any thread (`push`), drawn by the main loop (`LauncherApp::popups_ui`).

use crate::LauncherApp;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What the notification is and what to do on click.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// New message (friend SteamID) - open the chat.
    Message(u64),
    /// A friend started a game - open the chat.
    FriendGame(u64),
    /// A friend came online - open the chat.
    FriendOnline(u64),
    /// Discount on a wishlisted game - store page (0 - several games: the wishlist).
    Sale(u32),
    /// Game downloaded - game page in the library.
    Download(u32),
    /// Just a message (a test) - open Vaporust.
    Info,
    /// Message about a game (uninstalled, closed, launch error) - game page.
    Game(u32),
}

struct Popup {
    kind: Kind,
    title: String,
    body: String,
    /// Image on the right (emoticon or sticker from the message).
    image: Option<String>,
    /// Color of the stripe and name; None - by notification kind.
    accent: Option<egui::Color32>,
    /// When it was shown (reset while the card is hovered).
    shown: Instant,
}

const LIFE: Duration = Duration::from_secs(6);
const MAX: usize = 4;
const W: f32 = 300.0;
const H: f32 = 68.0;
/// Notifications are stacked back to back (a thin line between them).
const GAP: f32 = 0.0;

static QUEUE: Mutex<Vec<Popup>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();
static SOUND: AtomicBool = AtomicBool::new(true);
/// Clicks on cards - taken by the main loop.
static CLICKED: Mutex<Vec<Kind>> = Mutex::new(Vec::new());

pub fn init(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

pub fn set_sound(on: bool) {
    SOUND.store(on, Ordering::Relaxed);
}

/// Show a notification (from any thread). A repeat of the same kind (for example a second message
/// from the same friend) replaces the old card instead of adding a new one.
pub fn push(kind: Kind, title: impl Into<String>, body: impl Into<String>) {
    push_full(kind, title.into(), body.into(), None, None);
}

/// The same with an image and its own color (friend status: in game - green, online - blue).
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
        // As in Steam: each notification separately, new ones at the bottom, old ones move up.
        // Only an exact repeat is removed.
        q.retain(|x| !(x.kind == p.kind && x.title == p.title && x.body == p.body));
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

/// Clicks on notifications from the previous frame.
pub fn take_clicks() -> Vec<Kind> {
    std::mem::take(&mut *CLICKED.lock().unwrap())
}

/// Work area of the primary monitor (without the taskbar), physical pixels.
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
    /// Draw notifications (a separate frameless window in the screen corner).
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
        // Friend avatars in notifications - from the friend list.
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
            .with_title(tr!("Vaporust: уведомления", "Vaporust: notifications"))
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(false)
            .with_inner_size(size);
        let mut pos = None;
        if let Some(wa) = work_area() {
            // As in Steam: snug against the bottom right corner of the work area (above the taskbar clock).
            let p = egui::pos2(wa.max.x / ppp - size.x, wa.max.y / ppp - size.y);
            builder = builder.with_position(p);
            pos = Some(p);
        }
        // The position from with_position is set only when the window is created: when another
        // notification arrives, the window grows down under the taskbar. So on every change of the
        // card count we explicitly move and resize the window.
        static LAST_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let moved = LAST_N.swap(n, Ordering::Relaxed) != n;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("vaporust_popups"), builder, |ctx, _| {
            if moved {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                if let Some(p) = pos {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
                }
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(crate::theme::CARD))
                .show(ctx, |ui| {
                    // Cards back to back: without egui's standard item spacing (otherwise the stack is
                    // taller than the window and the bottom card is cut off by the taskbar).
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let mut q = QUEUE.lock().unwrap();
                    let mut close = None;
                    for (i, p) in q.iter_mut().enumerate() {
                        if i > 0 {
                            ui.add_space(GAP);
                        }
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, H), egui::Sense::click());
                        let hovered = resp.hovered();
                        if hovered {
                            p.shown = Instant::now(); // does not disappear while the card is hovered
                        }
                        let painter = ui.painter_at(rect);
                        // A card in the program's colors: dark, thin border, lighter on hover.
                        let accent_color = p.accent.unwrap_or_else(|| color(p.kind));
                        // Like the program's other windows: a dark card with a colored stripe on the left (notification kind).
                        let bg = if hovered { egui::Color32::from_rgb(42, 50, 63) } else { crate::theme::CARD };
                        painter.rect_filled(rect, 0.0, bg);
                        // A light highlight at the top - the card does not look flat and empty.
                        let top = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() * 0.45));
                        painter.rect_filled(top, 0.0, egui::Color32::from_white_alpha(6));
                        painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 68, 86)));
                        // A colored stripe on the left of all notifications: kind (discount, download) or friend status.
                        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
                        painter.rect_filled(bar, 0.0, accent_color);
                        // On the left, as in Steam: friend avatar in a status-colored frame, or the game image.
                        let icon = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, rect.top() + (H - 44.0) / 2.0), egui::vec2(44.0, 44.0));
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
                                // A square from the middle of the game header (460x215).
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
                        // Emoticon or sticker from the message - on the right, before the cross.
                        let mut text_w = W - 66.0 - 32.0;
                        if let Some(url) = &p.image {
                            let img_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.right() - 32.0 - 40.0, rect.top() + (H - 40.0) / 2.0 + 4.0),
                                egui::vec2(40.0, 40.0),
                            );
                            egui::Image::new(url.as_str()).paint_at(ui, img_rect);
                            text_w -= 46.0;
                        }
                        // On top in small print - what kind of notification this is, below the name (or title) and text.
                        let title_color = egui::Color32::WHITE;
                        let kind_text = text(ui, kind_label(p.kind), 9.5, crate::theme::WEAK, text_w, 1);
                        let show_title = !(p.title == "Vaporust" && matches!(p.kind, Kind::Game(_) | Kind::Info));
                        let title = text(ui, if show_title { &p.title } else { "" }, 13.5, title_color, text_w, 1);
                        let body = text(ui, &p.body, 12.0, crate::theme::TEXT, text_w, if show_title { 1 } else { 2 });
                        let x = rect.left() + 66.0;
                        let gap = 1.0;
                        let title_h = if show_title { title.size().y + gap } else { 0.0 };
                        let th = kind_text.size().y + gap + title_h + body.size().y;
                        let y = rect.center().y - th / 2.0;
                        let kind_h = kind_text.size().y + gap;
                        painter.galley(egui::pos2(x, y), kind_text, title_color);
                        if show_title {
                            painter.galley(egui::pos2(x, y + kind_h), title, title_color);
                        }
                        painter.galley(egui::pos2(x, y + kind_h + title_h), body, crate::theme::TEXT);
                        // Cross: larger, highlighted on hover.
                        let xr = egui::Rect::from_center_size(egui::pos2(rect.right() - 15.0, rect.top() + 15.0), egui::vec2(22.0, 22.0));
                        let over_x = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| xr.contains(h));
                        if over_x {
                            painter.rect_filled(xr, 4.0, egui::Color32::from_rgb(70, 80, 98));
                        }
                        let xc = if over_x { egui::Color32::WHITE } else if hovered { egui::Color32::from_gray(185) } else { egui::Color32::from_gray(125) };
                        let (c, d) = (xr.center(), 4.5);
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

/// Text no wider than `width` and at most `rows` lines (the rest is cut with an ellipsis).
fn text(ui: &egui::Ui, s: &str, size: f32, color: egui::Color32, width: f32, rows: usize) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_string(), egui::FontId::proportional(size), color, width);
    job.wrap.max_rows = rows;
    ui.fonts(|f| f.layout_job(job))
}

/// Caption of the notification kind (small, above the title).
fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Message(_) => tr!("НОВОЕ СООБЩЕНИЕ", "NEW MESSAGE"),
        Kind::FriendGame(_) => tr!("ДРУГ ИГРАЕТ", "FRIEND IS PLAYING"),
        Kind::FriendOnline(_) => tr!("ДРУГ В СЕТИ", "FRIEND ONLINE"),
        Kind::Sale(_) => tr!("СКИДКА В СПИСКЕ ЖЕЛАЕМОГО", "WISHLIST DISCOUNT"),
        Kind::Download(_) => tr!("ЗАГРУЗКА ЗАВЕРШЕНА", "DOWNLOAD COMPLETE"),
        Kind::Game(_) => tr!("БИБЛИОТЕКА", "LIBRARY"),
        Kind::Info => "VAPORUST",
    }
}

fn color(k: Kind) -> egui::Color32 {
    match k {
        Kind::Message(_) => egui::Color32::from_rgb(90, 160, 240),
        Kind::FriendGame(_) => crate::theme::IN_GAME,
        Kind::FriendOnline(_) => egui::Color32::from_rgb(90, 190, 220),
        Kind::Sale(_) => egui::Color32::from_rgb(240, 180, 60),
        Kind::Download(_) => egui::Color32::from_rgb(120, 210, 140),
        Kind::Info => egui::Color32::from_gray(150),
        Kind::Game(_) => egui::Color32::from_gray(150),
    }
}

/// Short melodic sounds synthesized in memory (WAV), one per kind.
mod sound {
    use super::Kind;
    use std::sync::OnceLock;

    const RATE: u32 = 44_100;

    /// Notes: (frequency Hz, start ms, duration ms).
    fn notes(k: Kind) -> &'static [(f32, u32, u32)] {
        match k {
            // Message: two quick rising tones.
            Kind::Message(_) => &[(1046.5, 0, 160), (1568.0, 90, 260)],
            // Friend in game: three notes of a major chord.
            Kind::FriendGame(_) => &[(523.3, 0, 260), (659.3, 90, 260), (784.0, 180, 380)],
            // Friend online: one soft note.
            Kind::FriendOnline(_) => &[(880.0, 0, 300)],
            // Discount: a ringing "ding-ding".
            Kind::Sale(_) => &[(1318.5, 0, 200), (1760.0, 110, 200), (2093.0, 220, 380)],
            // Download ready: up an octave.
            Kind::Download(_) => &[(392.0, 0, 220), (587.3, 120, 220), (784.0, 240, 450)],
            Kind::Info | Kind::Game(_) => &[(987.8, 0, 260)],
        }
    }

    /// WAV (16 bit, mono) from notes: a sine with an overtone, soft attack and decay.
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
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
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

    /// cargo test sounds_live -- --ignored: play all sounds in turn.
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
    /// Click on a notification: show Vaporust and open what is needed.
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
                        .unwrap_or_else(crate::social::unknown_name);
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

/// Friend status color, as in the friend list: in game - green, online - blue, away - slate blue.
pub fn status_color(state: Option<u32>, in_game: bool) -> Option<egui::Color32> {
    Some(match (state.unwrap_or(0), in_game) {
        (_, true) => crate::theme::IN_GAME,
        (0, _) => return None,
        (2, _) => egui::Color32::from_rgb(220, 120, 110),
        (3 | 4, _) => egui::Color32::from_rgb(110, 140, 170),
        _ => crate::theme::ONLINE,
    })
}

/// For development: VAPORUST_TEST_POPUPS=N - show N test notifications at startup
/// (checking the stack position above the taskbar).
pub fn test_from_env() {
    let n: usize = std::env::var("VAPORUST_TEST_POPUPS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    // One per second - the way real messages arrive.
    std::thread::spawn(move || {
        for i in 0..n {
            push(Kind::Info, "Vaporust", trf!("Пробное уведомление {}", "Test notification {}", i + 1));
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}
