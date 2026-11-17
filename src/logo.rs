//! Vaporust logo drawn in code (vector, no files): the letter V made of a connecting rod with a piston
//! (steam mechanics, a nod to Steam) and a blue lever meeting on a copper gear (Rust).
//! 64x64 field; `mark` returns the point color with alpha, or None (outside the mark).

use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// Round emblem.
    Circle,
    /// Rounded square (like an app icon).
    Squircle,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub shape: Shape,
    /// Thin toothed rim around the circle (gear frame).
    pub rim: bool,
    /// A small muted puff of steam between the levers.
    pub steam: bool,
}

/// Default variant (exe, window and README icon).
pub const DEFAULT: Style = Style { shape: Shape::Squircle, rim: false, steam: false };

type C = [f32; 3];
const COPPER: C = [205.0, 105.0, 52.0];
const COPPER_LIGHT: C = [232.0, 140.0, 82.0];
const SLATE: C = [70.0, 84.0, 110.0];
const SLATE_LIGHT: C = [92.0, 108.0, 138.0];
const CYAN: C = [64.0, 182.0, 245.0];
const CYAN_LIGHT: C = [168.0, 228.0, 255.0];
const HOLE: C = [16.0, 22.0, 32.0];

fn mix(a: C, b: C, t: f32) -> C {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Point in the lever's frame: (along from a to b, across), lever length.
fn local(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32, f32) {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len = (vx * vx + vy * vy).sqrt();
    let (ux, uy) = (vx / len, vy / len);
    let (dx, dy) = (x - a.0, y - a.1);
    (dx * ux + dy * uy, -dx * uy + dy * ux, len)
}

fn dist(x: f32, y: f32, p: (f32, f32)) -> f32 {
    ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt()
}

/// Mark color at point (x, y) of the 64x64 field: [r, g, b, a] (a = 1) or None.
pub fn mark(x: f32, y: f32, st: Style) -> Option<[f32; 4]> {
    // Background.
    let inside = match st.shape {
        Shape::Circle => dist(x, y, (32.0, 32.0)) <= 30.0,
        Shape::Squircle => {
            let (x0, y0, x1, y1, r) = (3.0, 3.0, 61.0, 61.0, 14.0);
            let cx = x.clamp(x0 + r, x1 - r);
            let cy = y.clamp(y0 + r, y1 - r);
            dist(x, y, (cx, cy)) <= r
        }
    };
    if !inside {
        return None;
    }
    let t = ((x - 4.0) * 0.35 + (y - 4.0)) / 76.0;
    let mut col = mix([36.0, 48.0, 68.0], [14.0, 19.0, 28.0], t);
    // Toothed rim (circle only): a thin ring with teeth, slightly lighter than the background.
    if st.rim && st.shape == Shape::Circle {
        let d = dist(x, y, (32.0, 32.0));
        let ang = (y - 32.0).atan2(x - 32.0);
        let tooth = ((ang * 18.0 / TAU).rem_euclid(1.0) < 0.45) as i32 as f32;
        let outer = 26.6 + tooth * 1.9;
        if d >= 24.9 && d <= outer {
            col = mix(col, [60.0, 74.0, 98.0], 0.85);
        }
    }
    // Puff of steam: three circles, muted color (almost the background).
    if st.steam {
        let puff = [((35.0, 13.0), 2.9), ((38.5, 12.0), 3.2), ((41.5, 13.6), 2.4)];
        if puff.iter().any(|(p, r)| dist(x, y, *p) <= *r) {
            col = mix(col, [190.0, 205.0, 222.0], 0.35);
        }
    }
    let pivot = (32.0, 44.5);
    // Copper gear (Rust) at the bottom: 10 teeth.
    let d = dist(x, y, pivot);
    let ang = (y - pivot.1).atan2(x - pivot.0);
    // A tooth is a trapezoid: wider at the base, narrower at the tip (rem_euclid, not fract: the angle can be negative).
    let phase = (ang * 10.0 / TAU).rem_euclid(1.0);
    let off = (phase - 0.5).abs();
    let tooth_r = if off < 0.16 { 13.0 } else if off < 0.24 { 13.0 - (off - 0.16) / 0.08 * 2.6 } else { 10.4 };
    if d <= tooth_r {
        col = COPPER;
        if d > 6.9 && d < 7.9 {
            col = mix(COPPER, COPPER_LIGHT, 0.6); // light band
        }
    }
    // Left lever: a connecting rod with a piston on top (slate blue).
    let piston_top = (17.5, 16.0);
    let (s, w, len) = local(x, y, pivot, piston_top);
    if s >= 0.0 && s <= len - 5.0 && w.abs() <= 2.9 - s / len * 0.6 {
        col = if w < -0.6 { SLATE_LIGHT } else { SLATE };
    }
    // Piston: a rectangle across the rod, with two copper rings on top.
    if s >= len - 9.0 && s <= len + 0.5 && w.abs() <= 6.2 {
        col = SLATE;
        if w < -3.6 {
            col = SLATE_LIGHT;
        }
        if (s >= len - 2.6 && s <= len - 1.4) || (s >= len - 0.6 && s <= len + 0.5) {
            col = COPPER;
        }
        if dist(x, y, (pivot.0 + (piston_top.0 - pivot.0) * (len - 5.5) / len, pivot.1 + (piston_top.1 - pivot.1) * (len - 5.5) / len)) <= 1.5 {
            col = COPPER;
        }
    }
    // Right lever: light blue, lighter middle, an eye at the end.
    let tip = (46.5, 14.5);
    let (s2, w2, len2) = local(x, y, pivot, tip);
    if s2 >= 0.0 && s2 <= len2 && w2.abs() <= 2.6 {
        col = if w2.abs() <= 0.9 { CYAN_LIGHT } else { CYAN };
    }
    let de = dist(x, y, tip);
    if de <= 4.3 {
        col = if de <= 1.8 { HOLE } else { CYAN };
    }
    // Hub on top of the levers: a dark ring and a copper pin.
    if d <= 5.2 {
        col = if d <= 2.7 { COPPER_LIGHT } else if d <= 3.6 { HOLE } else { COPPER };
        if d > 4.4 {
            col = HOLE;
        }
    }
    Some([col[0], col[1], col[2], 1.0])
}

/// The same mark simplified for 16-24 px (taskbar, Explorer list, window corner): at that size
/// the piston rings, light bands and the hole in the eye turn into noise. Thicker levers, a plain
/// gear and hub, almost no margin around the background. Same 64x64 field.
pub fn mark_small(x: f32, y: f32) -> Option<[f32; 4]> {
    let (x0, y0, x1, y1, r) = (1.0, 1.0, 63.0, 63.0, 13.0);
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    if dist(x, y, (cx, cy)) > r {
        return None;
    }
    let mut col = mix([36.0, 48.0, 68.0], [14.0, 19.0, 28.0], ((x - 4.0) * 0.35 + (y - 4.0)) / 76.0);
    let pivot = (32.0, 45.0);
    let d = dist(x, y, pivot);
    let ang = (y - pivot.1).atan2(x - pivot.0);
    let tooth = ((ang * 8.0 / TAU).rem_euclid(1.0) - 0.5).abs() < 0.2;
    if d <= if tooth { 14.5 } else { 11.5 } {
        col = COPPER;
    }
    let top = (15.0, 13.0);
    let (s, w, len) = local(x, y, pivot, top);
    if s >= 0.0 && s <= len && w.abs() <= 4.6 {
        col = SLATE_LIGHT;
    }
    if s >= len - 9.0 && s <= len + 1.0 && w.abs() <= 8.0 {
        col = SLATE_LIGHT;
    }
    let tip = (49.0, 12.0);
    let (s2, w2, len2) = local(x, y, pivot, tip);
    if (s2 >= 0.0 && s2 <= len2 && w2.abs() <= 4.2) || dist(x, y, tip) <= 6.0 {
        col = CYAN;
    }
    if d <= 5.5 {
        col = COPPER_LIGHT;
    }
    Some([col[0], col[1], col[2], 1.0])
}

/// White silhouette of the mark for the tray (like Windows and other programs' icons): no background,
/// details are transparent cuts. Returns point coverage: 1 - white, 0 - transparent.
pub fn silhouette(x: f32, y: f32) -> f32 {
    // The mark is stretched over the whole field (mark bounds in the emblem - SIL_BOX), keeping proportions.
    let (x0, y0, x1, y1) = SIL_BOX;
    let k = (x1 - x0).max(y1 - y0) / 62.0;
    let (x, y) = ((x0 + x1) / 2.0 + (x - 32.0) * k, (y0 + y1) / 2.0 + (y - 32.0) * k);
    silhouette_raw(x, y)
}

/// Mark bounds in the emblem field (minx, miny, maxx, maxy): see the sil_box test.
const SIL_BOX: (f32, f32, f32, f32) = (11.3, 9.7, 51.3, 58.0);

fn silhouette_raw(x: f32, y: f32) -> f32 {
    let pivot = (32.0, 44.5);
    let d = dist(x, y, pivot);
    let ang = (y - pivot.1).atan2(x - pivot.0);
    let phase = (ang * 10.0 / TAU).rem_euclid(1.0);
    let off = (phase - 0.5).abs();
    let tooth_r = if off < 0.16 { 13.0 } else if off < 0.24 { 13.0 - (off - 0.16) / 0.08 * 2.6 } else { 10.4 };
    let gear = d <= tooth_r;
    let piston_top = (17.5, 16.0);
    let (s, w, len) = local(x, y, pivot, piston_top);
    let rod = |pad: f32| s >= 0.0 && s <= len - 5.0 && w.abs() <= 2.9 - s / len * 0.6 + pad;
    let piston = |pad: f32| s >= len - 9.0 - pad && s <= len + 0.5 + pad && w.abs() <= 6.2 + pad;
    let tip = (46.5, 14.5);
    let (s2, w2, len2) = local(x, y, pivot, tip);
    let de = dist(x, y, tip);
    let arm = |pad: f32| (s2 >= 0.0 && s2 <= len2 && w2.abs() <= 2.6 + pad) || de <= 4.3 + pad;
    // Cuts: a ring around the pin, the lever eye, a gap around the levers over the gear, a stripe on the piston.
    if de <= 1.8 || (d > 3.0 && d <= 4.6) {
        return 0.0;
    }
    let on_arms = rod(0.0) || piston(0.0) || arm(0.0);
    if on_arms {
        if piston(0.0) && s >= len - 2.6 && s <= len - 1.4 {
            return 0.0;
        }
        return 1.0;
    }
    if gear && d > 4.6 && (rod(1.6) || arm(1.6)) {
        return 0.0;
    }
    if gear || d <= 3.0 { 1.0 } else { 0.0 }
}

/// RGBA image of the mark, n x n (4x4 samples per pixel).
#[cfg(test)]
pub fn render(n: usize, st: Style) -> Vec<u8> {
    let s = 64.0 / n as f32;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0.0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.0) * s;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.0) * s;
                    if let Some(c) = mark(px, py, st) {
                        for k in 0..3 {
                            acc[k] += c[k] / 16.0;
                        }
                        acc[3] += 1.0 / 16.0;
                    }
                }
            }
            let i = (y * n + x) * 4;
            for k in 0..3 {
                out[i + k] = if acc[3] > 0.0 { (acc[k] / acc[3]).min(255.0) as u8 } else { 0 };
            }
            out[i + 3] = (acc[3] * 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod sheet {
    /// Variant sheet: VAPORUST_LOGOS=<folder> cargo test logo_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn logo_sheet() {
        use super::*;
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_LOGOS").unwrap());
        let variants = [
            Style { shape: Shape::Squircle, rim: false, steam: false },
            Style { shape: Shape::Circle, rim: false, steam: false },
            Style { shape: Shape::Circle, rim: true, steam: false },
            Style { shape: Shape::Squircle, rim: false, steam: true },
        ];
        let (cell, pad) = (256u32, 28u32);
        let w = variants.len() as u32 * (cell + pad) + pad;
        let h = pad + cell + pad + 64 + pad;
        let mut sheet = image::RgbaImage::from_pixel(w, h, image::Rgba([58, 62, 70, 255]));
        let img = |n: usize, st: Style| image::RgbaImage::from_raw(n as u32, n as u32, render(n, st)).unwrap();
        for (i, st) in variants.iter().enumerate() {
            let x0 = pad + i as u32 * (cell + pad);
            image::imageops::overlay(&mut sheet, &img(cell as usize, *st), x0 as i64, pad as i64);
            let y1 = (pad + cell + pad) as i64;
            image::imageops::overlay(&mut sheet, &img(48, *st), x0 as i64 + 40, y1);
            image::imageops::overlay(&mut sheet, &img(32, *st), x0 as i64 + 110, y1 + 8);
            image::imageops::overlay(&mut sheet, &img(16, *st), x0 as i64 + 165, y1 + 16);
        }
        sheet.save(dir.join("logo_sheet.png")).unwrap();
    }
}

#[cfg(test)]
mod assets {
    /// Redraw assets/logo.png (256, for README): VAPORUST_WRITE_LOGO=1 cargo test write_logo_png -- --ignored
    #[test]
    #[ignore]
    fn write_logo_png() {
        assert!(std::env::var("VAPORUST_WRITE_LOGO").is_ok());
        let n = 256;
        let img = image::RgbaImage::from_raw(n as u32, n as u32, super::render(n, super::DEFAULT)).unwrap();
        img.save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("logo.png")).unwrap();
    }
}

#[cfg(test)]
mod sil {
    /// Prints the silhouette bounds in the emblem field (for SIL_BOX).
    #[test]
    #[ignore]
    fn sil_box() {
        let (mut a, mut b, mut c, mut d) = (99f32, 99f32, -1f32, -1f32);
        for yi in 0..640 {
            for xi in 0..640 {
                let (x, y) = (xi as f32 / 10.0, yi as f32 / 10.0);
                if super::silhouette_raw(x, y) > 0.0 {
                    a = a.min(x); b = b.min(y); c = c.max(x); d = d.max(y);
                }
            }
        }
        println!("SIL_BOX {a} {b} {c} {d}");
    }
}
