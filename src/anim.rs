//! Анимированные стикеры Steam (APNG). Загрузчик egui показывает у APNG только первый кадр,
//! поэтому кадры раскладываем сами: байты берем через тот же http-загрузчик egui
//! (ctx.try_load_bytes), декодируем image::codecs::png::ApngDecoder, кадры - в текстуры.

use eframe::egui;
use std::collections::HashMap;

/// Сколько кадров читаем из файла максимум.
const MAX_FRAMES: usize = 300;
/// Сколько кадров храним (каждый 150x150 ≈ 90 КБ): 64 кадра ≈ 5.5 МБ на стикер.
const KEEP_FRAMES: usize = 64;
/// Чаще не перерисовываем ради анимации.
const MAX_FPS: f64 = 12.0;

pub struct Anim {
    frames: Vec<(egui::TextureHandle, f64)>, // текстура и длительность кадра, с
    total: f64,
}

pub enum State {
    Ready(Anim),
    /// Не анимация или не удалось разобрать - рисуем обычной картинкой.
    Static,
}

#[derive(Default)]
pub struct Cache(HashMap<String, State>);

impl Cache {
    /// Нарисовать картинку по адресу размером `size`, с анимацией, если это APNG.
    pub fn show(&mut self, ui: &mut egui::Ui, url: &str, size: egui::Vec2) -> egui::Response {
        if !self.0.contains_key(url) {
            match ui.ctx().try_load_bytes(url) {
                Ok(egui::load::BytesPoll::Ready { bytes, .. }) => {
                    let st = decode(ui.ctx(), url, &bytes).map_or(State::Static, State::Ready);
                    self.0.insert(url.to_string(), st);
                }
                Ok(egui::load::BytesPoll::Pending { .. }) => {
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
                    egui::Spinner::new().paint_at(ui, rect.shrink(size.x * 0.3));
                    // Загрузчик будит только главное окно, поэтому окно чата проверяет сам.
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    return resp;
                }
                Err(_) => {
                    self.0.insert(url.to_string(), State::Static);
                }
            }
        }
        match self.0.get(url) {
            Some(State::Ready(a)) => {
                let t = ui.input(|i| i.time) % a.total;
                let mut acc = 0.0;
                let mut idx = 0;
                for (i, (_, d)) in a.frames.iter().enumerate() {
                    acc += d;
                    if t < acc {
                        idx = i;
                        break;
                    }
                }
                let resp = ui.add(egui::Image::new((a.frames[idx].0.id(), size)));
                // Экономия видеокарты: анимируем, только когда стикер виден и окно активно
                // (или над ним мышь), и не чаще MAX_FPS. Иначе стоит текущий кадр.
                let active = ui.ctx().input(|i| i.focused || i.pointer.has_pointer());
                if active && ui.is_rect_visible(resp.rect) {
                    let wait = (acc - t).max(1.0 / MAX_FPS);
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
                }
                resp
            }
            _ => ui.add(egui::Image::new(url).fit_to_exact_size(size)),
        }
    }
}

fn decode(ctx: &egui::Context, url: &str, bytes: &[u8]) -> Option<Anim> {
    use image::codecs::png::PngDecoder;
    use image::AnimationDecoder;
    let dec = PngDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    if !dec.is_apng().ok()? {
        return None;
    }
    // Сначала все кадры (ApngDecoder уже собирает их в полные картинки), затем прореживание.
    let mut raw = Vec::new();
    for f in dec.apng().ok()?.into_frames().take(MAX_FRAMES) {
        let f = f.ok()?;
        let (n, d) = f.delay().numer_denom_ms();
        // Кадры с нулевой задержкой браузеры показывают ~0.1 с.
        let secs = if n == 0 { 0.1 } else { n as f64 / d.max(1) as f64 / 1000.0 }.max(0.02);
        raw.push((f.into_buffer(), secs));
    }
    if raw.len() < 2 {
        return None;
    }
    // Не больше KEEP_FRAMES кадров: пропущенные кадры прибавляют свое время к оставленному.
    let step = raw.len().div_ceil(KEEP_FRAMES);
    let mut frames = Vec::new();
    let mut total = 0.0;
    for (i, chunk) in raw.chunks(step).enumerate() {
        let secs: f64 = chunk.iter().map(|(_, s)| s).sum();
        let buf = &chunk[0].0;
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [buf.width() as usize, buf.height() as usize],
            buf.as_raw(),
        );
        frames.push((ctx.load_texture(format!("{url}#{i}"), img, Default::default()), secs));
        total += secs;
    }
    Some(Anim { frames, total })
}

#[cfg(test)]
mod tests {
    /// VAPORUST_APNG=<файл стикера> cargo test apng_frames -- --ignored --nocapture
    #[test]
    #[ignore]
    fn apng_frames() {
        let bytes = std::fs::read(std::env::var("VAPORUST_APNG").unwrap()).unwrap();
        let ctx = eframe::egui::Context::default();
        let a = super::decode(&ctx, "test", &bytes).expect("должна быть анимация");
        println!("frames={} total={:.2}s", a.frames.len(), a.total);
    }
}
