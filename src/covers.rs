//! Обложки и фоны игр для библиотеки.
//!
//! Берутся из локального кэша Steam (`appcache\librarycache\<appid>\…`) - без интернета;
//! если там нет - скачиваются с CDN Steam. Картинки уменьшаются до размера показа:
//! обложка 140x210 (~115 КБ в памяти вместо ~2 МБ у 600x900), фон страницы игры - до 960 px.
//! Грузит один фоновый поток, по очереди, и только то, что видно на экране.

use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

/// Размер текстуры обложки (пропорции 2:3, как у library_600x900).
pub const W: u32 = 140;
pub const H: u32 = 210;
/// Ширина текстуры фона страницы игры.
const HERO_W: u32 = 960;

#[derive(Clone)]
pub enum Cover {
    Loading,
    Ready(egui::TextureHandle),
    /// Картинки нет ни в кэше, ни на CDN - рисуем заглушку.
    Missing,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Cover,
    Hero,
    /// Маленький значок игры для списка слева (32x32 в кэше Steam).
    Icon,
}

pub struct Covers {
    map: Arc<Mutex<HashMap<(Kind, u32), Cover>>>,
    tx: mpsc::Sender<(Kind, u32)>,
}

impl Covers {
    pub fn new(ctx: &egui::Context, steam_dir: Option<PathBuf>) -> Self {
        let map: Arc<Mutex<HashMap<(Kind, u32), Cover>>> = Default::default();
        let (tx, rx) = mpsc::channel::<(Kind, u32)>();
        let (map2, ctx) = (map.clone(), ctx.clone());
        std::thread::spawn(move || {
            while let Ok((kind, appid)) = rx.recv() {
                let cover = load(&ctx, steam_dir.as_deref(), kind, appid);
                map2.lock().unwrap().insert((kind, appid), cover);
                ctx.request_repaint();
            }
        });
        Self { map, tx }
    }

    fn get_kind(&self, kind: Kind, appid: u32) -> Cover {
        let mut map = self.map.lock().unwrap();
        match map.get(&(kind, appid)) {
            Some(c) => c.clone(),
            None => {
                // Фоны крупные (~1 МБ в памяти): держим только последние открытые.
                if kind == Kind::Hero {
                    let heroes: Vec<u32> = map.keys().filter(|(k, _)| *k == Kind::Hero).map(|(_, a)| *a).collect();
                    if heroes.len() >= 3 {
                        for a in heroes {
                            map.remove(&(Kind::Hero, a));
                        }
                    }
                }
                // Обложки: держим не больше предела (в режиме экономии меньше), видимые
                // тут же загрузятся снова с диска.
                if kind == Kind::Cover {
                    let limit = if crate::imgload::ECONOMY.load(std::sync::atomic::Ordering::Relaxed) { 48 } else { 120 };
                    if map.keys().filter(|(k, _)| *k == Kind::Cover).count() >= limit {
                        map.retain(|(k, _), _| *k != Kind::Cover);
                    }
                }
                map.insert((kind, appid), Cover::Loading);
                let _ = self.tx.send((kind, appid));
                Cover::Loading
            }
        }
    }

    /// Обложка игры; если еще не загружена - ставит в очередь загрузки.
    pub fn get(&self, appid: u32) -> Cover {
        self.get_kind(Kind::Cover, appid)
    }

    /// Широкий фон для страницы игры.
    pub fn hero(&self, appid: u32) -> Cover {
        self.get_kind(Kind::Hero, appid)
    }

    /// Значок игры для списка (только из кэша Steam, без интернета).
    pub fn icon(&self, appid: u32) -> Cover {
        self.get_kind(Kind::Icon, appid)
    }

    /// Забыть все картинки (освободить память; видимое загрузится снова).
    pub fn clear(&self) {
        self.map.lock().unwrap().retain(|_, c| matches!(c, Cover::Loading));
    }
}

/// Найти файл в кэше Steam: сначала в папке игры, затем в ее подпапках
/// (новые версии Steam кладут файлы в подпапки с хэшем).
fn local_file(steam_dir: &Path, appid: u32, file: &str) -> Option<Vec<u8>> {
    let dir = steam_dir.join("appcache").join("librarycache").join(appid.to_string());
    if let Ok(b) = std::fs::read(dir.join(file)) {
        return Some(b);
    }
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Ok(b) = std::fs::read(p.join(file)) {
                return Some(b);
            }
        }
    }
    None
}

fn cdn_file(appid: u32, file: &str) -> Option<Vec<u8>> {
    let url = format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/{file}");
    let resp = ehttp::fetch_blocking(&ehttp::Request::get(url)).ok()?;
    (resp.ok && !resp.bytes.is_empty()).then_some(resp.bytes)
}

/// Значок игры в кэше Steam: файл с именем-хэшем (40 шестнадцатеричных знаков).jpg.
fn local_icon(steam_dir: &Path, appid: u32) -> Option<Vec<u8>> {
    let dir = steam_dir.join("appcache").join("librarycache").join(appid.to_string());
    let is_icon = |p: &Path| {
        p.extension().is_some_and(|e| e == "jpg")
            && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let mut dirs = vec![dir.clone()];
    dirs.extend(std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    for d in dirs {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if is_icon(&p) {
                return std::fs::read(p).ok();
            }
        }
    }
    None
}

fn load(ctx: &egui::Context, steam_dir: Option<&Path>, kind: Kind, appid: u32) -> Cover {
    // Сторонняя игра (не из Steam): картинок в Steam нет.
    if crate::custom::is_custom(appid) {
        return Cover::Missing;
    }
    if kind == Kind::Icon {
        let Some(bytes) = steam_dir.and_then(|d| local_icon(d, appid)) else { return Cover::Missing };
        let Ok(img) = image::load_from_memory(&bytes) else { return Cover::Missing };
        let small = image::imageops::resize(&img.to_rgba8(), 32, 32, image::imageops::FilterType::Triangle);
        let color = egui::ColorImage::from_rgba_unmultiplied([32, 32], small.as_raw());
        return Cover::Ready(ctx.load_texture(format!("icon{appid}"), color, egui::TextureOptions::LINEAR));
    }
    let file = match kind {
        Kind::Cover => "library_600x900.jpg",
        _ => "library_hero.jpg",
    };
    let bytes = steam_dir.and_then(|d| local_file(d, appid, file)).or_else(|| cdn_file(appid, file));
    let Some(bytes) = bytes else { return Cover::Missing };
    let Ok(img) = image::load_from_memory(&bytes) else { return Cover::Missing };
    let rgba = img.to_rgba8();
    let (w, h) = match kind {
        Kind::Cover => {
            // Плитка тянется по ширине окна (до ~200 точек): обложка 180x270, в режиме экономии 140x210.
            if crate::imgload::ECONOMY.load(std::sync::atomic::Ordering::Relaxed) { (W, H) } else { (180, 270) }
        }
        _ => {
            let hw = if crate::imgload::ECONOMY.load(std::sync::atomic::Ordering::Relaxed) { 640 } else { HERO_W };
            let w = hw.min(rgba.width());
            (w, (rgba.height() as f32 * w as f32 / rgba.width() as f32) as u32)
        }
    };
    let small = image::imageops::resize(&rgba, w, h.max(1), image::imageops::FilterType::Triangle);
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h.max(1) as usize], small.as_raw());
    let name = format!("{}{appid}", if kind == Kind::Cover { "cover" } else { "hero" });
    Cover::Ready(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}
