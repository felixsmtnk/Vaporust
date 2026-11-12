//! Эффекты комнаты чата Steam ([roomeffect type="balloons"] и т.п.) - перенос из клиента Steam.
//!
//! Откуда что взято (Steam\steamui\sp.js и css\sp.css, модуль эффектов чата):
//! - список эффектов и распродаж, к которым относятся их картинки (winter2019 / lny2020);
//! - картинки - с того же CDN, что у Steam: assets/<распродажа>/roomeffects/<эффект>/<файл>;
//! - траектории (CSS keyframes), длительности, количество частиц и их разброс - те же числа.
//! В Steam каждый эффект - отдельная программа в коде клиента: новый эффект появляется только с
//! обновлением клиента. Неизвестный нам эффект показываем значком и подписью без анимации.
//!
//! Отличия от Steam (ради легкости): кадры фейерверка уменьшены вдвое; «hue-rotate» (сдвиг цвета)
//! у фейерверка и снежинок заменен подкраской; фигурные ленты конфетти - прямоугольниками.
//! Картинки кэшируются на диске (папка данных Vaporust\roomeffects), текстуры живут, только пока
//! идет эффект. Кадр - не чаще 30 раз в секунду и только во время эффекта.

use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const CDN: &str = "https://cdn.fastly.steamstatic.com/steamcommunity/public/assets/";
const FPS: u64 = 30;

/// Эффекты, которые знает клиент Steam (имя для /roomeffect, распродажа с картинками).
pub const KNOWN: [(&str, &str); 9] = [
    ("balloons", "winter2019"),
    ("confetti", "winter2019"),
    ("goldfetti", "winter2019"),
    ("firework", "winter2019"),
    ("snow", "winter2019"),
    ("snowball", "winter2019"),
    ("lny2020_lanterns", "lny2020"),
    ("lny2020_confetti", "lny2020"),
    ("lny2020_firework", "lny2020"),
];

fn sale_of(name: &str) -> &'static str {
    if name.starts_with("lny2020") {
        "lny2020"
    } else {
        "winter2019"
    }
}

/// Значок эффекта (96 px) - как в панели эффектов Steam.
pub fn icon_url(name: &str) -> String {
    format!("{CDN}{}/roomeffects/96px/{}.png", sale_of(name), name.to_lowercase())
}

// ---------- Подписи из локализации Steam ----------

static STEAM_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
static LABELS: OnceLock<HashMap<String, String>> = OnceLock::new();

pub fn set_steam_dir(dir: &std::path::Path) {
    let _ = STEAM_DIR.set(dir.to_path_buf());
}

/// «%1$s запускает воздушные шарики!» из файла перевода Steam (steamui\localization).
fn labels() -> &'static HashMap<String, String> {
    LABELS.get_or_init(|| {
        let mut map = HashMap::new();
        let Some(dir) = STEAM_DIR.get() else { return map };
        let Ok(text) = std::fs::read_to_string(dir.join("steamui/localization/friendsui_russian-json.js")) else {
            return map;
        };
        let key = "\"ChatRoom_RoomEffect";
        let mut rest = text.as_str();
        while let Some(p) = rest.find(key) {
            rest = &rest[p + key.len()..];
            let Some(q) = rest.find('"') else { break };
            let name = rest[..q].to_lowercase();
            let after = &rest[q..];
            if let Some(v) = after.strip_prefix("\":\"") {
                if let Some(end) = v.find('"') {
                    if !name.is_empty() {
                        map.insert(name, unescape(&v[..end]));
                    }
                }
            }
        }
        map
    })
}

/// \uXXXX -> символ (так записан JSON в файлах Steam).
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' && it.peek() == Some(&'u') {
            it.next();
            let hex: String = it.by_ref().take(4).collect();
            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                out.push(ch);
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// «Друг запускает воздушные шарики!» (как в Steam), иначе «эффект комнаты: balloons».
pub fn label(name: &str, sender: &str) -> String {
    let text = match labels().get(&name.to_lowercase()) {
        Some(t) => t.replace("%1$s", sender),
        None => format!("{sender}: эффект комнаты «{name}»"),
    };
    if sender != "Вы" {
        return text;
    }
    // Steam пишет имя; у нас «Вы» - глагол во втором лице: «Вы запускаете конфетти!».
    let mut text = text;
    for (a, b) in [("запускает", "запускаете"), ("пускает", "пускаете"), ("бросает", "бросаете")] {
        if text.contains(a) {
            text = text.replacen(a, b, 1);
            break;
        }
    }
    text
}

/// Без имени отправителя: «воздушные шарики» и т.п. (для уведомлений и панели выбора).
pub fn short_name(name: &str) -> String {
    if name.eq_ignore_ascii_case("snow") {
        return "снегопад".into(); // у Steam «пускает снежок» - путается со «снежком» (snowball)
    }
    match labels().get(&name.to_lowercase()) {
        Some(t) => t
            .replace("%1$s", "")
            .trim()
            .trim_end_matches('!')
            .split_once(' ')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_else(|| t.clone()),
        None => name.to_string(),
    }
}

// ---------- «Показать еще раз» ----------

static REPLAY: Mutex<Option<String>> = Mutex::new(None);

pub fn request_replay(kind: &str) {
    *REPLAY.lock().unwrap() = Some(kind.to_string());
}

pub fn take_replay() -> Option<String> {
    REPLAY.lock().unwrap().take()
}

// ---------- Картинки ----------

/// Загрузка картинок эффекта в фоне: диск -> CDN, уменьшение, кэш на диске.
#[derive(Default)]
struct Assets {
    tex: HashMap<String, TextureHandle>,
    requested: HashSet<String>,
    ready: Arc<Mutex<Vec<(String, egui::ColorImage)>>>,
    failed: Arc<Mutex<HashSet<String>>>,
}

fn cache_dir() -> Option<std::path::PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join("roomeffects"))
}

impl Assets {
    /// Запросить картинку `rel` (путь после assets/). `scale` - во сколько раз уменьшить.
    fn want(&mut self, ctx: &egui::Context, rel: &str, scale: f32) {
        if !self.requested.insert(rel.to_string()) {
            return;
        }
        let (rel, ready, failed, ctx) = (rel.to_string(), self.ready.clone(), self.failed.clone(), ctx.clone());
        std::thread::spawn(move || {
            let file = cache_dir().map(|d| d.join(rel.replace('/', "_")));
            let img = file
                .as_ref()
                .and_then(|f| image::open(f).ok())
                .map(|i| i.to_rgba8())
                .or_else(|| {
                    let resp = ehttp::fetch_blocking(&ehttp::Request::get(format!("{CDN}{rel}"))).ok()?;
                    if !resp.ok {
                        return None;
                    }
                    let mut img = image::load_from_memory(&resp.bytes).ok()?.to_rgba8();
                    if scale > 1.0 {
                        let (w, h) = ((img.width() as f32 / scale) as u32, (img.height() as f32 / scale) as u32);
                        img = image::imageops::resize(&img, w.max(1), h.max(1), image::imageops::FilterType::Triangle);
                    }
                    if let Some(f) = &file {
                        let _ = std::fs::create_dir_all(f.parent()?);
                        let _ = img.save_with_format(f, image::ImageFormat::Png);
                    }
                    Some(img)
                });
            match img {
                Some(img) => {
                    let size = [img.width() as usize, img.height() as usize];
                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    ready.lock().unwrap().push((rel, ci));
                }
                None => {
                    failed.lock().unwrap().insert(rel);
                }
            }
            ctx.request_repaint();
        });
    }

    fn pump(&mut self, ctx: &egui::Context) {
        for (rel, ci) in self.ready.lock().unwrap().drain(..) {
            let t = ctx.load_texture(format!("fx:{rel}"), ci, egui::TextureOptions::LINEAR);
            self.tex.insert(rel, t);
        }
    }

    /// Все запрошенные картинки готовы (или не загрузились).
    fn all_done(&self) -> bool {
        let failed = self.failed.lock().unwrap();
        self.requested.iter().all(|r| self.tex.contains_key(r) || failed.contains(r))
    }

    fn get(&self, rel: &str) -> Option<&TextureHandle> {
        self.tex.get(rel)
    }
}

// ---------- Случайные числа ----------

/// xorshift: Math.random() Steam заменяем своим генератором.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Rng(s | 1)
    }
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Целое из [a, b] (c.Tg в коде Steam).
    fn int(&mut self, a: i32, b: i32) -> i32 {
        a + (self.f() * (b - a + 1) as f32).floor() as i32
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.f() * (i + 1) as f32) as usize;
            v.swap(i, j.min(i));
        }
    }
}

/// Генератор снежинок Steam: frac(sin(seed++) * 1000) - тот же, что в клиенте.
struct SinRng(f64);

impl SinRng {
    fn next(&mut self) -> f32 {
        let v = (self.0).sin() * 1000.0;
        self.0 += 1.0;
        (v - v.floor()) as f32
    }
}

// ---------- Эффекты ----------

enum Kind {
    Balloons(Vec<Balloon>),
    Fireworks(Vec<Burst>),
    Confetti(Vec<Particle>),
    Snow(Vec<Flake>),
    Snowball(Snowball),
}

pub struct Effect {
    name: String,
    sale: &'static str,
    kind: Option<Kind>,
    assets: Assets,
    /// Время отсчета: запускается, когда загружены картинки (как в Steam: пока не загрузились - не видно).
    start: Option<Instant>,
    created: Instant,
    len: f32,
    /// Масштаб фейерверка от ширины окна (как в Steam: 400..1080 px -> 0.5..2).
    fw_scale: f32,
}

struct Balloon {
    anim: &'static [Key],
    image: String,
    glow: Option<(String, f32)>,
    dur: f32,
    offset: f32,
    z: i32,
}

/// Кадр CSS-анимации: доля времени, left %, top %, масштаб, сдвиг X в % ширины шарика.
#[derive(Clone, Copy)]
struct Key {
    at: f32,
    left: Option<f32>,
    top: Option<f32>,
    scale: Option<f32>,
    tx: Option<f32>,
}

const fn k(at: f32, left: Option<f32>, top: Option<f32>, scale: Option<f32>, tx: Option<f32>) -> Key {
    Key { at, left, top, scale, tx }
}

// Траектории шариков из sp.css (balloon_small_1..3, balloon_big_1..2, balloon_move_left/right).
const SMALL_1: [Key; 4] = [
    k(0.0, Some(50.0), Some(120.0), Some(0.7), Some(0.0)),
    k(0.4, None, None, Some(0.7), Some(-1.0)),
    k(0.8, None, None, Some(0.7), Some(1.0)),
    k(1.0, Some(40.0), Some(-80.0), Some(0.7), Some(0.0)),
];
const SMALL_2: [Key; 4] = [
    k(0.0, Some(20.0), Some(120.0), Some(0.7), Some(0.0)),
    k(0.4, None, None, Some(0.7), Some(-1.0)),
    k(0.8, None, None, Some(0.7), Some(1.0)),
    k(1.0, Some(20.0), Some(-80.0), Some(0.7), Some(0.0)),
];
const SMALL_3: [Key; 4] = [
    k(0.0, Some(5.0), Some(120.0), Some(0.7), Some(0.0)),
    k(0.4, None, None, Some(0.7), Some(-1.0)),
    k(0.8, None, None, Some(0.7), Some(1.0)),
    k(1.0, Some(10.0), Some(-80.0), Some(0.7), Some(0.0)),
];
const BIG_1: [Key; 4] = [
    k(0.0, Some(0.0), Some(140.0), Some(1.7), Some(0.0)),
    k(0.4, None, None, Some(1.7), Some(1.0)),
    k(0.8, None, None, Some(1.7), Some(-1.0)),
    k(1.0, Some(10.0), Some(-100.0), Some(1.7), Some(0.0)),
];
const BIG_2: [Key; 4] = [
    k(0.0, Some(10.0), Some(100.0), Some(1.4), Some(0.0)),
    k(0.4, None, None, Some(1.4), Some(1.0)),
    k(0.8, None, None, Some(1.4), Some(-1.0)),
    k(1.0, Some(0.0), Some(-100.0), Some(1.4), Some(0.0)),
];
const MOVE_RIGHT: [Key; 6] = [
    k(0.0, Some(30.0), Some(120.0), Some(1.0), Some(0.0)),
    k(0.3, Some(40.0), None, None, None),
    k(0.4, None, None, Some(1.0), Some(1.0)),
    k(0.6, Some(50.0), None, None, None),
    k(0.8, None, None, Some(1.0), Some(-1.0)),
    k(1.0, Some(60.0), Some(-80.0), Some(1.0), Some(0.0)),
];
const MOVE_LEFT: [Key; 6] = [
    k(0.0, Some(60.0), Some(120.0), Some(1.0), Some(0.0)),
    k(0.3, Some(50.0), None, None, None),
    k(0.4, None, None, Some(1.0), Some(-1.0)),
    k(0.6, Some(40.0), None, None, None),
    k(0.8, None, None, Some(1.0), Some(1.0)),
    k(1.0, Some(30.0), Some(-80.0), Some(1.0), Some(0.0)),
];

/// Значение свойства на доле времени t (линейно между кадрами, где оно задано).
fn prop(keys: &[Key], t: f32, get: impl Fn(&Key) -> Option<f32>) -> f32 {
    let pts: Vec<(f32, f32)> = keys.iter().filter_map(|k| get(k).map(|v| (k.at, v))).collect();
    let Some(&(_, first)) = pts.first() else { return 0.0 };
    let mut prev = (0.0, first);
    for &(at, v) in &pts {
        if t <= at {
            if at <= prev.0 {
                return v;
            }
            return prev.1 + (v - prev.1) * (t - prev.0) / (at - prev.0);
        }
        prev = (at, v);
    }
    prev.1
}

struct Burst {
    kind: u8, // 0 long, 1 big, 2 small
    x: (f32, f32), // (% ширины, px)
    y: (f32, f32),
    delay: f32,
    tint: Color32,
}

struct Particle {
    rot: f32,
    ry: f32,
    rx: f32,
    rz: f32,
    y: (f32, f32),
    x: (f32, f32),
    scale: f32,
    color: Color32,
}

struct Flake {
    left: f32,
    dur: f32,
    delay: f32,
    path: (f32, f32), // сдвиг X (px) и поворот (градусы) за все падение
    imgs: [(String, f32, f32, f32); 2], // картинка, размер, прозрачность, отступ
}

struct Snowball {
    x: f32,
    y: f32,
    rot: f32,
    splat_rots: [f32; 3],
    path_rot: f32,
}

// 20 траекторий снежинок из sp.css (translate(X, 110vh) rotate(R)).
const FLAKE_PATHS: [(f32, f32); 20] = [
    (73.0, -349.0), (188.0, 423.0), (120.0, 140.0), (-8.0, 311.0), (3.0, 114.0),
    (67.0, 394.0), (131.0, 162.0), (-53.0, -4.0), (-79.0, -260.0), (-179.0, 405.0),
    (155.0, 370.0), (-184.0, 278.0), (-4.0, 440.0), (-42.0, -265.0), (-114.0, -393.0),
    (-10.0, -333.0), (-108.0, 192.0), (-138.0, -172.0), (-64.0, 185.0), (88.0, 241.0),
];

fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let h = h.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color32::from_rgb((f(h + 1.0 / 3.0) * 255.0) as u8, (f(h) * 255.0) as u8, (f(h - 1.0 / 3.0) * 255.0) as u8)
}

/// «Подкраска» вместо hue-rotate: светлый оттенок нужного цвета (белые части остаются светлыми).
fn tint_for_hue(deg: f32) -> Color32 {
    let c = hsl(deg, 1.0, 0.75);
    Color32::from_rgb(c.r(), c.g(), c.b())
}

impl Effect {
    /// Эффект по имени из [roomeffect type=...]. None - неизвестный (анимации нет).
    pub fn new(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        let (len, known) = match name.as_str() {
            "balloons" | "lny2020_lanterns" => (5.0, true),
            "firework" | "lny2020_firework" => (7.0, true),
            "confetti" | "goldfetti" | "lny2020_confetti" => (8.0, true),
            "snow" => (10.0, true),
            "snowball" => (3.01, true),
            _ => (0.0, false),
        };
        if !known {
            return None;
        }
        Some(Self {
            sale: sale_of(&name),
            name,
            kind: None,
            assets: Assets::default(),
            start: None,
            created: Instant::now(),
            len,
            fw_scale: 1.0,
        })
    }

    /// Подготовить частицы под размер окна и запросить картинки.
    fn setup(&mut self, ctx: &egui::Context, area: Rect) {
        let mut r = Rng::new();
        let (w, h) = (area.width(), area.height());
        let sale = self.sale;
        let name = self.name.clone();
        let kind = match name.as_str() {
            "balloons" | "lny2020_lanterns" => {
                let lny = name == "lny2020_lanterns";
                let mut imgs: Vec<&str> = if lny {
                    vec!["orange", "peach", "yellow"]
                } else {
                    vec!["balloon_blue", "balloon_cyan", "balloon_green", "balloon_purple", "balloon_red", "balloon_yellow"]
                };
                r.shuffle(&mut imgs);
                let mut next = 0usize;
                let mut list = Vec::new();
                let mut add = |anim: &'static [Key], offset: f32, z: i32, r: &mut Rng| {
                    let img = imgs[next % imgs.len()];
                    next += 1;
                    list.push(Balloon {
                        anim,
                        image: format!("{sale}/roomeffects/{name}/{img}.png"),
                        glow: lny.then(|| (format!("{sale}/roomeffects/{name}/{img}-glow.png"), 1.0 + r.f() * 4.0)),
                        dur: 4.0 + r.f(),
                        offset,
                        z,
                    });
                };
                for anim in [&SMALL_1[..], &SMALL_2[..], &SMALL_3[..]] {
                    let o = r.int(-10, 10) as f32;
                    add(anim, o, 0, &mut r);
                }
                let mut big = vec![70.0, 20.0];
                r.shuffle(&mut big);
                let left_forced = big[1] == 20.0;
                let o = r.int(-10, 10) as f32 + big.pop().unwrap();
                add(&BIG_1[..], o, 10, &mut r);
                let o = r.int(-10, 10) as f32 + big.pop().unwrap();
                add(&BIG_2[..], o, 9, &mut r);
                let mv: &'static [Key] = if left_forced || r.int(0, 1) == 0 { &MOVE_LEFT[..] } else { &MOVE_RIGHT[..] };
                let o = r.int(-10, 10) as f32;
                add(mv, o, 5, &mut r);
                list.sort_by_key(|b| b.z);
                for b in &list {
                    self.assets.want(ctx, &b.image, 1.0);
                    if let Some((g, _)) = &b.glow {
                        self.assets.want(ctx, g, 1.0);
                    }
                }
                Kind::Balloons(list)
            }
            "firework" | "lny2020_firework" => {
                let lny = name == "lny2020_firework";
                let hue = |r: &mut Rng| if lny { 360.0 + r.int(-30, 10) as f32 } else { r.int(0, 360) as f32 };
                let scale = (0.5 + (w - 400.0) / (1080.0 - 400.0) * 1.5).clamp(0.5, 2.0);
                let mut v = Vec::new();
                let mut delay = 0.0f32;
                let cluster = |v: &mut Vec<Burst>, delay: &mut f32, r: &mut Rng| {
                    let (kx, vy) = (r.int(-5, 50) as f32, r.int(-10, 50) as f32);
                    let t = hue(r);
                    v.push(Burst { kind: 1, x: (kx, 0.0), y: (vy, 0.0), delay: *delay, tint: tint_for_hue(t) });
                    let mut he = r.f() * std::f32::consts::TAU;
                    let te = ((128.0 - 64.0) * scale).floor();
                    let q = (60.0 * scale).floor();
                    let le = (10.0 * scale).floor();
                    for ze in 0..3 {
                        let g = he.cos() * q;
                        let ee = he.sin() * q + le;
                        if ze < 2 {
                            *delay += 0.2;
                        }
                        let t = hue(r);
                        v.push(Burst { kind: 2, x: (kx, te + g), y: (vy, te + ee), delay: *delay, tint: tint_for_hue(t) });
                        he += 2.09;
                    }
                };
                let long = |v: &mut Vec<Burst>, delay: f32, r: &mut Rng| {
                    let (kx, vy) = (r.int(20, 60) as f32, r.int(-5, 50) as f32);
                    let t = hue(r);
                    v.push(Burst { kind: 0, x: (kx, 0.0), y: (vy, 0.0), delay, tint: tint_for_hue(t) });
                };
                // Порядок и паузы - как в CreateFireworks клиента Steam.
                cluster(&mut v, &mut delay, &mut r);
                delay += 0.2;
                long(&mut v, delay, &mut r);
                delay += 0.2;
                cluster(&mut v, &mut delay, &mut r);
                delay += 0.6;
                cluster(&mut v, &mut delay, &mut r);
                delay += 0.2;
                long(&mut v, delay, &mut r);
                delay += 0.2;
                long(&mut v, delay, &mut r);
                delay += 0.2;
                long(&mut v, delay, &mut r);
                cluster(&mut v, &mut delay, &mut r);
                long(&mut v, delay, &mut r);
                delay += 0.2;
                cluster(&mut v, &mut delay, &mut r);
                self.fw_scale = scale;
                for f in ["long_sheet", "big_sheet", "small_sheet"] {
                    self.assets.want(ctx, &format!("{sale}/roomeffects/fireworks/{f}.png"), 2.0);
                }
                Kind::Fireworks(v)
            }
            "confetti" | "goldfetti" | "lny2020_confetti" => {
                let gold = name != "confetti";
                let mut v = Vec::with_capacity(200);
                for _ in 0..200 {
                    let n = r.f();
                    let kk = r.f() * n;
                    let vv = r.f() * kk;
                    let x = (r.f() - 0.5) * 4.0 * std::f32::consts::PI * 20.0;
                    let he = r.f() + 0.5;
                    let y = (r.f() * h - h - 20.0, h + 20.0 + r.f() * h * he);
                    let z = if w <= 1000.0 { 1.0 } else { w / 1000.0 };
                    let scale = he * z * if name == "goldfetti" { 2.0 } else { 1.0 };
                    let q = (r.f() - 0.5) * w;
                    let ae = (r.f() - 0.5) * w;
                    let mut hue = if gold { 51.0 - r.f() * 16.0 } else { r.f() * 360.0 };
                    if name == "lny2020_confetti" {
                        hue += 360.0 + r.int(-30, 10) as f32;
                    }
                    let color = if gold { hsl(hue, 0.93, 0.54) } else { hsl(hue, 1.0, 0.40) };
                    v.push(Particle { rot: x, ry: kk, rx: n, rz: vv, y, x: (ae, q + ae), scale, color });
                }
                Kind::Confetti(v)
            }
            "snow" => {
                let mut s = SinRng(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(1.0));
                let mut v = Vec::with_capacity(150);
                for i in 0..150 {
                    let he = s.next();
                    let left = s.next() * 100.0;
                    let dur = (1.0 - he) * 3.0 + 4.0;
                    let delay = he + s.next() * 4.0;
                    let img = |u: f32| {
                        let mut n = SinRng(u as f64);
                        let op = (n.next() * 0.8 + 0.5).min(1.0);
                        let size = (n.next() + u) * 10.0 + 10.0;
                        let margin = n.next() * 30.0 + 15.0;
                        let _hue = n.next();
                        let f = 1 + (n.next() * 5.0).floor() as i32;
                        (format!("winter2019/roomeffects/96px/flake_{f}.png"), size, op, margin)
                    };
                    v.push(Flake { left, dur, delay, path: FLAKE_PATHS[i % 20], imgs: [img(he), img(he + 1.0)] });
                }
                for f in 1..=5 {
                    self.assets.want(ctx, &format!("winter2019/roomeffects/96px/flake_{f}.png"), 1.0);
                }
                Kind::Snow(v)
            }
            _ => {
                // snowball
                let paths = [0.0, 30.0, -30.0, -5.0, 10.0, -15.0];
                let sb = Snowball {
                    x: r.int(0, 70) as f32,
                    y: r.int(0, 70) as f32,
                    rot: (r.f() * 90.0).floor() - 45.0,
                    splat_rots: [r.f() * 360.0, r.f() * 360.0, r.f() * 360.0],
                    path_rot: paths[r.int(0, 5) as usize],
                };
                for f in ["snowball", "splat1", "splat2", "splat3", "snowball_1", "snowball_2", "snowball_3", "snowball_4"] {
                    self.assets.want(ctx, &format!("winter2019/roomeffects/snowball/{f}.png"), 2.0);
                }
                Kind::Snowball(sb)
            }
        };
        self.kind = Some(kind);
    }

    /// Нарисовать кадр поверх окна. false - эффект закончился.
    pub fn draw(&mut self, ctx: &egui::Context) -> bool {
        let area = ctx.screen_rect();
        if self.kind.is_none() {
            self.setup(ctx, area);
        }
        self.assets.pump(ctx);
        // Ждем картинки (первый раз качаются с CDN; не дольше 20 с), потом отсчет времени эффекта.
        if self.start.is_none() {
            if self.assets.all_done() || self.created.elapsed() > Duration::from_secs(20) {
                self.start = Some(Instant::now());
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
                return true;
            }
        }
        let t = self.start.unwrap().elapsed().as_secs_f32();
        if t > self.len {
            return false;
        }
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("room_effect")));
        let p = p.with_clip_rect(area);
        match self.kind.as_ref().unwrap() {
            Kind::Balloons(list) => self.draw_balloons(&p, area, t, list),
            Kind::Fireworks(list) => self.draw_fireworks(&p, area, t, list),
            Kind::Confetti(list) => draw_confetti(&p, area, t / 8.0, list),
            Kind::Snow(list) => self.draw_snow(&p, area, t, list),
            Kind::Snowball(sb) => self.draw_snowball(&p, area, t, sb),
        }
        ctx.request_repaint_after(Duration::from_millis(1000 / FPS));
        true
    }

    /// Картинка в прямоугольнике с поворотом (градусы) вокруг центра.
    fn image(&self, p: &egui::Painter, rel: &str, rect: Rect, rot: f32, tint: Color32) {
        let Some(t) = self.assets.get(rel) else { return };
        let mut mesh = egui::Mesh::with_texture(t.id());
        mesh.add_rect_with_uv(rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        if rot != 0.0 {
            mesh.rotate(egui::emath::Rot2::from_angle(rot.to_radians()), rect.center());
        }
        p.add(egui::Shape::mesh(mesh));
    }

    fn draw_balloons(&self, p: &egui::Painter, a: Rect, t: f32, list: &[Balloon]) {
        for b in list {
            if t > b.dur {
                continue;
            }
            let f = t / b.dur;
            let Some(tex) = self.assets.get(&b.image) else { continue };
            let bw = a.width() * 0.15;
            let size = tex.size_vec2();
            let bh = bw * size.y / size.x;
            let scale = prop(b.anim, f, |k| k.scale);
            let tx = prop(b.anim, f, |k| k.tx) / 100.0 * bw;
            let left = a.left() + a.width() * (prop(b.anim, f, |k| k.left) + b.offset) / 100.0;
            let top = a.top() + a.height() * prop(b.anim, f, |k| k.top) / 100.0;
            let center = Pos2::new(left + bw / 2.0 + tx, top + bh / 2.0);
            let rect = Rect::from_center_size(center, egui::vec2(bw, bh) * scale);
            p.image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            if let Some((g, period)) = &b.glow {
                if let Some(gt) = self.assets.get(g) {
                    // Мерцание свечения фонарика (keyframes lantern_glow: 0.5..0.98).
                    let phase = (t / period).fract();
                    let op = 0.5 + 0.48 * ((phase * 40.0).sin() * 0.5 + 0.5) * (phase * std::f32::consts::PI).sin();
                    let c = Color32::from_white_alpha((op * 255.0) as u8);
                    p.image(gt.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), c);
                }
            }
        }
    }

    fn draw_fireworks(&self, p: &egui::Painter, a: Rect, t: f32, list: &[Burst]) {
        // (файл, колонок, строк, длительность, длительность строки)
        const SHEETS: [(&str, u32, u32, f32, f32); 3] =
            [("long_sheet", 9, 10, 2.0, 0.2), ("big_sheet", 9, 4, 0.8, 0.2), ("small_sheet", 10, 6, 0.9, 0.15)];
        for b in list {
            let (name, cols, rows, dur, row_dur) = SHEETS[b.kind as usize];
            let lt = t - b.delay;
            if lt < 0.0 || lt >= dur {
                continue;
            }
            let Some(tex) = self.assets.get(&format!("{}/roomeffects/fireworks/{name}.png", self.sale)) else { continue };
            let row = ((lt / dur) * rows as f32).floor().min(rows as f32 - 1.0);
            let col = ((lt / row_dur).fract() * cols as f32).floor().min(cols as f32 - 1.0);
            let uv = Rect::from_min_size(
                egui::pos2(col / cols as f32, row / rows as f32),
                egui::vec2(1.0 / cols as f32, 1.0 / rows as f32),
            );
            let s = if b.kind == 2 { self.fw_scale / 2.0 } else { self.fw_scale };
            let min = Pos2::new(a.left() + a.width() * b.x.0 / 100.0 + b.x.1, a.top() + a.height() * b.y.0 / 100.0 + b.y.1);
            p.image(tex.id(), Rect::from_min_size(min, egui::Vec2::splat(256.0 * s)), uv, b.tint);
        }
    }

    fn draw_snow(&self, p: &egui::Painter, a: Rect, t: f32, list: &[Flake]) {
        for f in list {
            let lt = t - f.delay;
            if lt < 0.0 || lt > f.dur {
                continue;
            }
            // ease-out (cubic-bezier(0, 0, 0.58, 1)) - приблизительно.
            let x = lt / f.dur;
            let e = 1.0 - (1.0 - x).powf(1.7);
            let dx = f.path.0 * e;
            let dy = 1.1 * a.height() * e;
            let rot = f.path.1 * e;
            let base = Pos2::new(a.left() + a.width() * f.left / 100.0 + dx, a.top() - 200.0 + dy);
            let mut x0 = base.x;
            for (img, size, op, margin) in &f.imgs {
                let c = Pos2::new(x0 + margin + size / 2.0, base.y + margin + size / 2.0);
                let tint = Color32::from_white_alpha((op * 255.0) as u8);
                self.image(p, img, Rect::from_center_size(c, egui::Vec2::splat(*size)), rot, tint);
                x0 += size + margin * 2.0;
            }
        }
    }

    fn draw_snowball(&self, p: &egui::Painter, a: Rect, t: f32, sb: &Snowball) {
        let base = |n: &str| format!("winter2019/roomeffects/snowball/{n}.png");
        let w = a.width() * 0.3;
        let left = a.left() + a.width() * sb.x / 100.0;
        let top = a.top() + a.height() * sb.y / 100.0;
        let center = Pos2::new(left + w / 2.0, top + w / 2.0);
        // 0..1 с: снежок летит «на нас» - растет из точки, вращается; путь - сдвиг снизу.
        if t < 1.0 {
            let e = cubic(t, 0.47, 0.0, 0.745, 0.715);
            let path = 1.0 - (1.0 - t).powi(2);
            let c = center + egui::vec2(0.0, w * 0.5 * (1.0 - path));
            let spin = if sb.rot > -5.0 && sb.rot < 5.0 { 0.0 } else if sb.rot < 0.0 { -270.0 } else { 270.0 };
            let rot = sb.rot + spin * (1.0 - e) + sb.path_rot * (1.0 - path);
            let tint = Color32::from_white_alpha((e * 255.0) as u8);
            self.image(p, &base("snowball"), Rect::from_center_size(c, egui::Vec2::splat(w * e.max(0.01))), rot, tint);
            return;
        }
        // Удар: три кляксы (масштаб 2, гаснут за 2 с) и четыре куска, разлетающиеся вниз.
        let lt = t - 1.01;
        if lt < 0.0 {
            return;
        }
        let fade = (1.0 - lt / 2.0).clamp(0.0, 1.0);
        for (i, rot) in sb.splat_rots.iter().enumerate() {
            let tint = Color32::from_white_alpha((fade * 255.0) as u8);
            self.image(p, &base(&format!("splat{}", i + 1)), Rect::from_center_size(center, egui::Vec2::splat(w * 2.0 * 0.5)), *rot + sb.rot, tint);
        }
        // Куски: (длительность, сдвиг X %, сдвиг Y %, поворот) - из keyframes snowball-chunk-1..4.
        let chunks = [(0.2, 100.0, 200.0, 360.0), (1.7, 200.0, 200.0, 120.0), (1.9, -120.0, 200.0, 60.0), (2.0, -420.0, 200.0, -420.0)];
        let cw = w * 0.3;
        for (i, (d, dx, dy, r)) in chunks.iter().enumerate() {
            let x = (lt / d).min(1.0);
            if x >= 1.0 {
                continue;
            }
            let e = cubic(x, 0.25, 0.46, 0.45, 0.94);
            let c = center + egui::vec2(cw * dx / 100.0 * e, cw * dy / 100.0 * e);
            let tint = Color32::from_white_alpha(((1.0 - e) * 255.0) as u8);
            self.image(p, &base(&format!("snowball_{}", i + 1)), Rect::from_center_size(c, egui::Vec2::splat(cw)), r * e, tint);
        }
    }
}

/// Кривая cubic-bezier(x1, y1, x2, y2) для доли времени x (поиск t бинарным делением).
fn cubic(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let bez = |t: f32, a: f32, b: f32| 3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, y1, y2)
}

/// Конфетти: 200 прямоугольников 10x5, вращаются в 3D (проекция без перспективы), как в Steam.
fn draw_confetti(p: &egui::Painter, a: Rect, te: f32, list: &[Particle]) {
    let origin = Pos2::new(a.center().x, a.top());
    // Все кусочки - одной сеткой треугольников без сглаживания: у повернутого «изнанкой»
    // многоугольника сглаживание egui давало длинные полосы через все окно.
    let mut mesh = egui::Mesh::default();
    for q in list {
        let x = te * (q.x.1 - q.x.0) + q.x.0;
        let y = te * (q.y.1 - q.y.0) + q.y.0;
        let (ay, ax, az) = (te * q.rot * q.ry, te * q.rot * q.rx, te * q.rot * q.rz);
        let corners = [(-5.0, -2.5), (5.0, -2.5), (5.0, 2.5), (-5.0, 2.5)];
        let pts: Vec<Pos2> = corners
            .iter()
            .map(|&(cx, cy)| {
                // scale -> rotateZ -> rotateX -> rotateY (как применяет CSS transform справа налево).
                let (mut px, mut py, mut pz) = (cx * q.scale, cy * q.scale, 0.0f32);
                (px, py) = (px * az.cos() - py * az.sin(), px * az.sin() + py * az.cos());
                (py, pz) = (py * ax.cos() - pz * ax.sin(), py * ax.sin() + pz * ax.cos());
                (px, _) = (px * ay.cos() + pz * ay.sin(), -px * ay.sin() + pz * ay.cos());
                origin + egui::vec2(x + px + 5.0, y + py + 2.5)
            })
            .collect();
        let i = mesh.vertices.len() as u32;
        for pt in pts {
            mesh.colored_vertex(pt, q.color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    p.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    #[test]
    fn steam_labels() {
        super::set_steam_dir(std::path::Path::new(r"C:\Program Files (x86)\Steam"));
        let l = super::label("balloons", "Друг");
        println!("{l} | {}", super::short_name("lny2020_firework"));
        if super::labels().is_empty() {
            return; // Steam не установлен (например, на сборочной машине GitHub)
        }
        assert!(l.starts_with("Друг "));
        assert!(!l.contains("%1$s"));
        assert!(super::label("confetti", "Вы").starts_with("Вы запускаете"));
    }

    #[test]
    fn known_effects_build() {
        for (n, _) in super::KNOWN {
            assert!(super::Effect::new(n).is_some(), "{n}");
        }
        assert!(super::Effect::new("unknown_new_effect").is_none());
    }
}

#[cfg(test)]
mod live {
    /// cargo test effects_run_live -- --ignored --nocapture: каждый эффект целиком без окна
    /// (картинки с CDN Steam, кадры, длительность).
    #[test]
    #[ignore]
    fn effects_run_live() {
        let ctx = eframe::egui::Context::default();
        for (n, _) in super::KNOWN {
            let mut e = super::Effect::new(n).unwrap();
            let t0 = std::time::Instant::now();
            let mut frames = 0;
            loop {
                let input = eframe::egui::RawInput {
                    screen_rect: Some(eframe::egui::Rect::from_min_size(Default::default(), eframe::egui::vec2(520.0, 580.0))),
                    ..Default::default()
                };
                let mut alive = true;
                let _ = ctx.run(input, |ctx| alive = e.draw(ctx));
                frames += 1;
                if !alive || t0.elapsed().as_secs() > 25 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
            println!("{n}: {} картинок, {frames} кадров, {:.1} с", e.assets.tex.len(), t0.elapsed().as_secs_f32());
        }
    }
}

#[cfg(test)]
mod decode_check {
    #[test]
    #[ignore]
    fn decode_icon() {
        let p = std::env::var("VAPORUST_DUMP").unwrap();
        let b = std::fs::read(p).unwrap();
        println!("{:?}", image::load_from_memory(&b).map(|i| (i.width(), i.height())));
    }
}

/// Название предмета инвентаря («Balloons», «Gold Confetti», «Fireworks»…) -> имя для /roomeffect.
pub fn from_item_name(item: &str) -> String {
    let n: String = item.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let alias = |n: &str| -> Option<&'static str> {
        Some(match n {
            "balloon" | "balloons" => "balloons",
            "confetti" => "confetti",
            "goldconfetti" | "goldfetti" | "goldenconfetti" => "goldfetti",
            "firework" | "fireworks" => "firework",
            "snow" | "snowfall" | "snowflakes" => "snow",
            "snowball" | "snowballs" => "snowball",
            _ if n.contains("lantern") => "lny2020_lanterns",
            _ if n.contains("lunar") && n.contains("confetti") => "lny2020_confetti",
            _ if n.contains("lunar") && n.contains("firework") => "lny2020_firework",
            _ => return None,
        })
    };
    alias(&n).map(str::to_string).unwrap_or(n)
}

#[cfg(test)]
mod confetti_check {
    #[test]
    fn confetti_draws_triangles() {
        let ctx = eframe::egui::Context::default();
        let mut e = super::Effect::new("confetti").unwrap();
        let mut tris = 0;
        for _ in 0..3 {
            let input = eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(Default::default(), eframe::egui::vec2(520.0, 580.0))),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                e.draw(ctx);
            });
            let prims = ctx.tessellate(out.shapes, 1.0);
            tris = prims
                .iter()
                .map(|p| match &p.primitive {
                    eframe::epaint::Primitive::Mesh(m) => m.indices.len() / 3,
                    _ => 0,
                })
                .sum::<usize>();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        println!("треугольников: {tris}");
        assert!(tris >= 100, "конфетти не рисуется: {tris}");
    }
}
