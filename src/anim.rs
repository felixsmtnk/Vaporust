//! Animated Steam stickers (APNG). The egui loader shows only the first frame of an APNG,
//! so we lay out the frames ourselves: the bytes come from the same egui http loader
//! (ctx.try_load_bytes), decoded with image::codecs::png::ApngDecoder, frames become textures.

use eframe::egui;
use std::collections::HashMap;

/// Maximum number of frames read from a file.
const MAX_FRAMES: usize = 300;
/// How many frames we keep (each 150x150 ≈ 90 KB): 64 frames ≈ 5.5 MB per sticker.
const KEEP_FRAMES: usize = 64;
/// We do not repaint more often than this for animation.
const MAX_FPS: f64 = 12.0;

pub struct Anim {
    frames: Vec<(egui::TextureHandle, f64)>, // texture and frame duration, s
    total: f64,
}

pub enum State {
    Ready(Anim),
    /// Not an animation or failed to parse - draw as a plain image.
    Static,
}

#[derive(Default)]
pub struct Cache(HashMap<String, State>);

impl Cache {
    /// Draw the image at the URL with size `size`, animated if it is an APNG.
    pub fn show(&mut self, ui: &mut egui::Ui, url: &str, size: egui::Vec2) -> egui::Response {
        // Animations disabled in settings: first frame, without decoding all frames.
        if !crate::imgload::animations() {
            return ui.add(egui::Image::new(url).fit_to_exact_size(size));
        }
        if !self.0.contains_key(url) {
            match ui.ctx().try_load_bytes(url) {
                Ok(egui::load::BytesPoll::Ready { bytes, .. }) => {
                    let st = decode(ui.ctx(), url, &bytes).map_or(State::Static, State::Ready);
                    self.0.insert(url.to_string(), st);
                }
                Ok(egui::load::BytesPoll::Pending { .. }) => {
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().circle_stroke(rect.center(), size.x * 0.15, ui.visuals().widgets.noninteractive.fg_stroke);
                    // The loader wakes only the main window, so the chat window checks by itself.
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
                // Saving the GPU: animate only when the sticker is visible and the window is active
                // (or hovered), and no faster than MAX_FPS. Otherwise the current frame stays.
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
    // First all frames (ApngDecoder already composes full images), then thinning.
    let mut raw = Vec::new();
    for f in dec.apng().ok()?.into_frames().take(MAX_FRAMES) {
        let f = f.ok()?;
        let (n, d) = f.delay().numer_denom_ms();
        // Browsers show zero-delay frames for ~0.1 s.
        let secs = if n == 0 { 0.1 } else { n as f64 / d.max(1) as f64 / 1000.0 }.max(0.02);
        raw.push((f.into_buffer(), secs));
    }
    if raw.len() < 2 {
        return None;
    }
    // At most KEEP_FRAMES frames: skipped frames add their time to the kept one.
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
    /// VAPORUST_APNG=<sticker file> cargo test apng_frames -- --ignored --nocapture
    #[test]
    #[ignore]
    fn apng_frames() {
        let bytes = std::fs::read(std::env::var("VAPORUST_APNG").unwrap()).unwrap();
        let ctx = eframe::egui::Context::default();
        let a = super::decode(&ctx, "test", &bytes).expect("must be an animation");
        println!("frames={} total={:.2}s", a.frames.len(), a.total);
    }
}
