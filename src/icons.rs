//! Window icons drawn in code (no image files): 64x64 RGBA.

use eframe::egui;
use std::sync::Arc;

pub enum Kind {
    /// Main window: "play" button.
    Launcher,
    /// Chat window: speech bubble.
    Chat,
    /// Tray icon with a red dot: there are unread messages.
    LauncherUnread,
    /// Tray icon: white silhouette of the logo.
    Tray,
    /// Tray icon with a red dot.
    TrayUnread,
}

pub fn icon(kind: Kind) -> Arc<egui::IconData> {
    icon_sized(kind, 64)
}

/// The same icon at any size (drawn in a 64x64 field and scaled).
pub fn icon_sized(kind: Kind, n: usize) -> Arc<egui::IconData> {
    #[allow(non_snake_case)]
    let N = n;
    let s = 64.0 / n as f32;
    let mut rgba = vec![0u8; N * N * 4];
    let ink = if matches!(kind, Kind::Tray | Kind::TrayUnread) { tray_ink() } else { 255 };
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = ((x as f32 + 0.5) * s, (y as f32 + 0.5) * s);
            // Background: rounded square with a vertical gradient (dark blue -> blue).
            let bg = rounded_rect(fx, fy, 2.0, 2.0, 62.0, 62.0, 14.0);
            let t = fy / 64.0;
            let base = [
                (20.0 + 30.0 * t) as u8,
                (40.0 + 70.0 * t) as u8,
                (80.0 + 110.0 * t) as u8,
            ];
            // White mark on top of the background.
            let mark = match kind {
                Kind::Launcher | Kind::LauncherUnread | Kind::Tray | Kind::TrayUnread => 0.0,
                Kind::Chat => {
                    let bubble = rounded_rect(fx, fy, 12.0, 14.0, 52.0, 42.0, 9.0);
                    let tail = triangle(fx, fy, (20.0, 38.0), (32.0, 41.0), (17.0, 52.0));
                    bubble.max(tail)
                }
            };
            let i = (y * N + x) * 4;
            if matches!(kind, Kind::Tray | Kind::TrayUnread) {
                // Tray: white silhouette of the mark without a background, like Windows and other programs' icons.
                let mut cov = 0.0f32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = (x as f32 + (sx as f32 + 0.5) / 4.0) * s;
                        let py = (y as f32 + (sy as f32 + 0.5) / 4.0) * s;
                        cov += crate::logo::silhouette(px, py) / 16.0;
                    }
                }
                let c = ink;
                rgba[i] = c;
                rgba[i + 1] = c;
                rgba[i + 2] = c;
                rgba[i + 3] = (cov * 255.0) as u8;
            } else if matches!(kind, Kind::Launcher | Kind::LauncherUnread) {
                // Window and exe - rounded square.
                let style = crate::logo::DEFAULT;
                // Vaporust logo (src/logo.rs): a V made of a connecting rod with a piston and a blue lever on a copper gear.
                // 4x4 samples per pixel for smooth edges.
                let mut acc = [0.0f32; 4];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = (x as f32 + (sx as f32 + 0.5) / 4.0) * s;
                        let py = (y as f32 + (sy as f32 + 0.5) / 4.0) * s;
                        if let Some(col) = crate::logo::mark(px, py, style) {
                            for c in 0..3 {
                                acc[c] += col[c] / 16.0;
                            }
                            acc[3] += 1.0 / 16.0;
                        }
                    }
                }
                for c in 0..3 {
                    rgba[i + c] = if acc[3] > 0.0 { (acc[c] / acc[3]).min(255.0) as u8 } else { 0 };
                }
                rgba[i + 3] = (acc[3] * 255.0) as u8;
            } else {
                for c in 0..3 {
                    rgba[i + c] = (base[c] as f32 * (1.0 - mark) + 255.0 * mark) as u8;
                }
                rgba[i + 3] = (bg * 255.0) as u8;
            }
            // Red dot in the top right corner: there are unread messages.
            if matches!(kind, Kind::LauncherUnread | Kind::TrayUnread) {
                let d = ((fx - 50.0).powi(2) + (fy - 14.0).powi(2)).sqrt();
                let dot = (13.5 - d).clamp(0.0, 1.0);
                if dot > 0.0 {
                    let red = [230.0, 50.0, 50.0];
                    for c in 0..3 {
                        rgba[i + c] = (rgba[i + c] as f32 * (1.0 - dot) + red[c] * dot) as u8;
                    }
                    rgba[i + 3] = rgba[i + 3].max((dot * 255.0) as u8);
                }
            }
        }
    }
    Arc::new(egui::IconData { rgba, width: N as u32, height: N as u32 })
}

/// Coverage (0..1) of a point by a rounded rectangle, with anti-aliased edges.
fn rounded_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r + 0.5 - d).clamp(0.0, 1.0)
}

/// Coverage of a point by a triangle (by signs of distances to the sides, anti-aliased).
fn triangle(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let edge = |p: (f32, f32), q: (f32, f32)| {
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt();
        ((x - p.0) * dy - (y - p.1) * dx) / len
    };
    let (e1, e2, e3) = (edge(a, b), edge(b, c), edge(c, a));
    // The triangle can have any orientation: take the one where all signs match.
    let inside = if e1 + e2 + e3 < 0.0 { -e1.max(e2).max(e3) } else { e1.min(e2).min(e3) };
    (inside + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    /// VAPORUST_ICONS=<folder> cargo test save_icons -- --ignored
    #[test]
    #[ignore]
    fn save_icons() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_ICONS").unwrap());
        for (k, n) in [(super::Kind::Launcher, "launcher"), (super::Kind::Chat, "chat")] {
            let i = super::icon(k);
            let img = image::RgbaImage::from_raw(i.width, i.height, i.rgba.clone()).unwrap();
            let big = image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Nearest);
            big.save(dir.join(format!("{n}.png"))).unwrap();
            let _ = image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Lanczos3).save(dir.join(format!("{n}_smooth.png")));
        }
    }
}

#[cfg(test)]
mod ico {
    /// Build assets/vaporust.ico (exe icon): PNG frames 16..256 in an ICO container.
    /// VAPORUST_WRITE_ICO=1 cargo test write_ico -- --ignored
    #[test]
    #[ignore]
    fn write_ico() {
        assert!(std::env::var("VAPORUST_WRITE_ICO").is_ok());
        let sizes = [16usize, 24, 32, 48, 64, 128, 256];
        let mut pngs = Vec::new();
        for n in sizes {
            let i = super::icon_sized(super::Kind::Launcher, n);
            let img = image::RgbaImage::from_raw(i.width, i.height, i.rgba.clone()).unwrap();
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            pngs.push((n, buf.into_inner()));
        }
        let mut out = Vec::new();
        out.extend_from_slice(&[0, 0, 1, 0]);
        out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * pngs.len() as u32;
        for (n, data) in &pngs {
            let b = if *n >= 256 { 0 } else { *n as u8 };
            out.extend_from_slice(&[b, b, 0, 0]);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += data.len() as u32;
        }
        for (_, data) in &pngs {
            out.extend_from_slice(data);
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("vaporust.ico");
        std::fs::write(&path, out).unwrap();
        println!("{} байт", std::fs::metadata(path).unwrap().len());
    }
}


#[cfg(test)]
mod variants {
    //! Logo variants to choose from (without the gear): VAPORUST_LOGOS=<folder> cargo test logo_variants -- --ignored
    use std::f32::consts::{PI, TAU};

    type Rgba = [f32; 4];
    const WHITE: [f32; 3] = [246.0, 248.0, 252.0];

    fn c(rgb: [f32; 3]) -> Rgba {
        [rgb[0], rgb[1], rgb[2], 255.0]
    }
    fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    }
    fn in_rrect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }
    fn circ(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }
    /// Distance to a segment and the parameter t on it.
    fn seg(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
        let (vx, vy) = (b.0 - a.0, b.1 - a.1);
        let t = (((x - a.0) * vx + (y - a.1) * vy) / (vx * vx + vy * vy).max(1e-6)).clamp(0.0, 1.0);
        (((x - a.0 - vx * t).powi(2) + (y - a.1 - vy * t).powi(2)).sqrt(), t)
    }
    /// Hit test of a variable-width line: points (x, y, half-width).
    fn stroke(x: f32, y: f32, pts: &[(f32, f32, f32)]) -> bool {
        pts.windows(2).any(|w| {
            let (d, t) = seg(x, y, (w[0].0, w[0].1), (w[1].0, w[1].1));
            d <= w[0].2 + (w[1].2 - w[0].2) * t
        })
    }
    /// Curve from a function t -> (x, y, half-width), n segments.
    fn curve(f: impl Fn(f32) -> (f32, f32, f32), n: usize) -> Vec<(f32, f32, f32)> {
        (0..=n).map(|i| f(i as f32 / n as f32)).collect()
    }
    /// Cloud: three circles and a flat bottom, center (cx, cy), scale k.
    fn cloud(x: f32, y: f32, cx: f32, cy: f32, k: f32) -> bool {
        let (x, y) = ((x - cx) / k + 32.5, (y - cy) / k + 34.0);
        (circ(x, y, 26.5, 35.0, 5.2) || circ(x, y, 32.5, 31.5, 6.6) || circ(x, y, 38.0, 35.2, 4.8)
            || (y >= 34.0 && y <= 40.2 && x >= 26.5 && x <= 38.0))
            && y <= 40.2
    }
    fn square(x: f32, y: f32, top: [f32; 3], bottom: [f32; 3]) -> Option<Rgba> {
        in_rrect(x, y, 2.0, 2.0, 62.0, 62.0, 14.0).then(|| c(mix(top, bottom, (x * 0.35 + y) / 86.0)))
    }

    /// 1. "Wisps": two steam wisps rise and spread into a V.
    fn v1(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [36.0, 72.0, 150.0], [14.0, 28.0, 64.0]) else { return [0.0; 4] };
        for side in [-1.0f32, 1.0] {
            let pts = curve(
                |t| {
                    let wave = 4.2 * (t * TAU * 1.15 + if side > 0.0 { PI } else { 0.0 }).sin() * t;
                    (32.0 + side * 15.0 * t + wave, 51.0 - 39.0 * t, 4.4 - 2.4 * t)
                },
                40,
            );
            if stroke(x, y, &pts) {
                return c(WHITE);
            }
        }
        bg
    }
    /// 2. "Cloud": a white cloud with a "play" triangle inside.
    fn v2(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [64.0, 140.0, 230.0], [24.0, 52.0, 120.0]) else { return [0.0; 4] };
        if cloud(x, y, 32.5, 33.5, 1.85) {
            // Triangle: vertices (27,26) (27,42) (40,34).
            let inside = x >= 27.5 && (y - 34.0).abs() <= (40.5 - x) * 8.0 / 13.0;
            return if inside { bg } else { c(WHITE) };
        }
        bg
    }
    /// 3. "V and steam": a bold V and three puffs of steam above its right edge.
    fn v3(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [120.0, 80.0, 220.0], [40.0, 30.0, 110.0]) else { return [0.0; 4] };
        let v = stroke(x, y, &[(17.0, 22.0, 4.6), (30.0, 49.0, 4.6)]) || stroke(x, y, &[(30.0, 49.0, 4.6), (43.0, 22.0, 4.6)]);
        if v || circ(x, y, 46.5, 15.5, 3.0) || circ(x, y, 51.5, 10.0, 2.2) || circ(x, y, 41.0, 11.0, 2.0) {
            return c(WHITE);
        }
        bg
    }
    /// 4. "Puffs": three puffs of steam rise diagonally, from small to large.
    fn v4(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [30.0, 40.0, 62.0], [12.0, 16.0, 28.0]) else { return [0.0; 4] };
        if cloud(x, y, 40.0, 22.0, 1.25) {
            return c(WHITE);
        }
        if cloud(x, y, 27.0, 38.0, 0.85) || circ(x, y, 17.0, 50.0, 3.4) {
            return c([240.0, 150.0, 70.0]);
        }
        bg
    }
    /// 5. "Waves": three wavy lines of warm air on an orange background.
    fn v5(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [250.0, 150.0, 60.0], [200.0, 60.0, 50.0]) else { return [0.0; 4] };
        for (i, x0) in [21.0f32, 32.0, 43.0].iter().enumerate() {
            let ph = i as f32 * 0.9;
            let pts = curve(|t| (x0 + 3.2 * (t * TAU * 1.25 + ph).sin(), 50.0 - 36.0 * t, 3.0 - 0.8 * t), 40);
            if stroke(x, y, &pts) {
                return c(WHITE);
            }
        }
        bg
    }
    /// 6. "Cloud-V": turquoise background, white cloud with a V cut out of it.
    fn v6(x: f32, y: f32) -> Rgba {
        let Some(bg) = square(x, y, [40.0, 200.0, 190.0], [20.0, 90.0, 120.0]) else { return [0.0; 4] };
        if cloud(x, y, 32.5, 33.0, 1.85) {
            let v = stroke(x, y, &[(24.0, 25.0, 2.6), (32.0, 42.0, 2.6)]) || stroke(x, y, &[(32.0, 42.0, 2.6), (40.0, 25.0, 2.6)]);
            return if v { bg } else { c(WHITE) };
        }
        bg
    }

    fn render(n: u32, f: fn(f32, f32) -> Rgba) -> image::RgbaImage {
        let s = 64.0 / n as f32;
        image::RgbaImage::from_fn(n, n, |x, y| {
            let mut acc = [0.0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = f((x as f32 + (sx as f32 + 0.5) / 4.0) * s, (y as f32 + (sy as f32 + 0.5) / 4.0) * s);
                    let a = p[3] / 255.0;
                    for k in 0..3 {
                        acc[k] += p[k] * a / 16.0;
                    }
                    acc[3] += p[3] / 16.0;
                }
            }
            let a = acc[3] / 255.0;
            let un = |v: f32| if a > 0.0 { (v / a).min(255.0) as u8 } else { 0 };
            image::Rgba([un(acc[0]), un(acc[1]), un(acc[2]), acc[3] as u8])
        })
    }

    #[test]
    #[ignore]
    fn logo_variants() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_LOGOS").unwrap());
        let all: [fn(f32, f32) -> Rgba; 6] = [v1, v2, v3, v4, v5, v6];
        let (cell, pad) = (200u32, 24u32);
        let w = all.len() as u32 * (cell + pad) + pad;
        let h = pad + cell + pad + 64 + pad;
        let mut sheet = image::RgbaImage::from_pixel(w, h, image::Rgba([236, 239, 243, 255]));
        for y in (pad + cell + pad / 2)..h {
            for x in 0..w {
                sheet.put_pixel(x, y, image::Rgba([32, 32, 32, 255]));
            }
        }
        for (i, f) in all.iter().enumerate() {
            let x0 = pad + i as u32 * (cell + pad);
            image::imageops::overlay(&mut sheet, &render(cell, *f), x0 as i64, pad as i64);
            let y1 = pad + cell + pad + 8;
            image::imageops::overlay(&mut sheet, &render(48, *f), x0 as i64 + 24, y1 as i64);
            image::imageops::overlay(&mut sheet, &render(32, *f), x0 as i64 + 96, y1 as i64 + 8);
            image::imageops::overlay(&mut sheet, &render(16, *f), x0 as i64 + 152, y1 as i64 + 16);
            render(256, *f).save(dir.join(format!("logo2_{}.png", i + 1))).unwrap();
        }
        sheet.save(dir.join("logo_variants_2.png")).unwrap();
    }

    /// Third set: round emblems in the spirit of Steam (dark blue circle, white mechanism, steam
    /// escaping over the edge), but with an original design. VAPORUST_LOGOS=<folder> cargo test logo_variants_3 -- --ignored
    pub(super) mod round {
        use super::*;

        fn ring(x: f32, y: f32, cx: f32, cy: f32, r: f32, w: f32) -> bool {
            (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs() <= w / 2.0
        }
        /// Round backing: gradient from light blue (top left) to dark blue.
        fn badge(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> Option<Rgba> {
            circ(x, y, cx, cy, r).then(|| {
                let t = ((x - (cx - r)) * 0.45 + (y - (cy - r))) / (r * 2.9);
                c(mix([44.0, 118.0, 196.0], [12.0, 22.0, 48.0], t))
            })
        }
        fn polar(cx: f32, cy: f32, r: f32, deg: f32) -> (f32, f32) {
            let a = deg.to_radians();
            (cx + r * a.cos(), cy - r * a.sin())
        }
        /// Steam wisp: a wavy line from (x0, y0) up and to the right.
        fn steam(x: f32, y: f32, x0: f32, y0: f32, len: f32, w0: f32) -> bool {
            let pts = curve(|t| (x0 + len * 0.55 * t + 2.2 * (t * TAU * 1.3).sin(), y0 - len * t, w0 - (w0 - 0.9) * t), 30);
            stroke(x, y, &pts)
        }
        fn seg2(x: f32, y: f32, a: (f32, f32), b: (f32, f32), w: f32) -> bool {
            stroke(x, y, &[(a.0, a.1, w), (b.0, b.1, w)])
        }

        /// 1. "Gauge": a pressure dial with a needle, steam bursting out over the edge.
        pub fn r1(x: f32, y: f32) -> Rgba {
            if circ(x, y, 48.5, 13.0, 3.0) || circ(x, y, 53.5, 7.5, 2.2) || circ(x, y, 57.5, 3.5, 1.5) {
                return c(WHITE);
            }
            let Some(bg) = badge(x, y, 30.0, 34.0, 27.0) else { return [0.0; 4] };
            let (cx, cy) = (30.0, 34.0);
            if ring(x, y, cx, cy, 18.5, 4.4) || circ(x, y, cx, cy, 3.6) {
                return c(WHITE);
            }
            for i in 0..7 {
                let deg = 225.0 - i as f32 * 45.0;
                if seg2(x, y, polar(cx, cy, 11.5, deg), polar(cx, cy, 14.5, deg), 1.0) {
                    return c(WHITE);
                }
            }
            let tip = polar(cx, cy, 14.0, 40.0);
            if stroke(x, y, &[(cx, cy, 2.2), (tip.0, tip.1, 0.8)]) {
                return c([255.0, 170.0, 70.0]);
            }
            bg
        }

        /// 2. "V-joint": two levers meet in a V, joints at the ends.
        pub fn r2(x: f32, y: f32) -> Rgba {
            let Some(bg) = badge(x, y, 32.0, 32.0, 29.0) else { return [0.0; 4] };
            let (a, b, m) = ((17.0, 19.0), (47.0, 19.0), (32.0, 46.0));
            for (p, r, hole) in [(a, 6.2, 2.6), (b, 6.2, 2.6), (m, 5.6, 2.3)] {
                if circ(x, y, p.0, p.1, hole) {
                    return bg;
                }
                if circ(x, y, p.0, p.1, r) {
                    return c(WHITE);
                }
            }
            if seg2(x, y, a, m, 3.0) || seg2(x, y, b, m, 3.0) {
                return c(WHITE);
            }
            bg
        }

        /// 3. "V-joint and steam": levers in a V, steam flies out of the upper joint past the circle.
        pub fn r3(x: f32, y: f32) -> Rgba {
            if circ(x, y, 52.0, 10.5, 3.4) || circ(x, y, 57.0, 5.0, 2.4) || circ(x, y, 60.5, 1.5, 1.4) {
                return c(WHITE);
            }
            let Some(bg) = badge(x, y, 30.0, 34.0, 27.0) else { return [0.0; 4] };
            let (a, b, m) = ((17.0, 23.0), (44.0, 21.0), (30.0, 47.0));
            for (p, r, hole) in [(a, 5.8, 2.4), (b, 6.4, 2.7), (m, 5.4, 2.2)] {
                if circ(x, y, p.0, p.1, hole) {
                    return bg;
                }
                if circ(x, y, p.0, p.1, r) {
                    return c(WHITE);
                }
            }
            if seg2(x, y, a, m, 2.8) || seg2(x, y, b, m, 2.8) {
                return c(WHITE);
            }
            bg
        }

        /// 4. "Play and steam": a ring with "play" inside, steam rising over the edge.
        pub fn r4(x: f32, y: f32) -> Rgba {
            if circ(x, y, 48.5, 13.0, 3.0) || circ(x, y, 53.5, 7.5, 2.2) || circ(x, y, 57.5, 3.5, 1.5) {
                return c(WHITE);
            }
            let Some(bg) = badge(x, y, 30.0, 34.0, 27.0) else { return [0.0; 4] };
            if ring(x, y, 30.0, 34.0, 19.0, 3.6) {
                return c(WHITE);
            }
            let inside = x >= 25.0 && (y - 34.0).abs() <= (40.0 - x) * 10.0 / 15.0;
            if inside {
                return c(WHITE);
            }
            bg
        }

        /// 5. "Wisps V": two steam wisps spread into a V, a thin ring along the edge.
        pub fn r5(x: f32, y: f32) -> Rgba {
            let Some(bg) = badge(x, y, 32.0, 32.0, 29.0) else { return [0.0; 4] };
            if ring(x, y, 32.0, 32.0, 25.5, 1.6) {
                return c([150.0, 200.0, 245.0]);
            }
            for side in [-1.0f32, 1.0] {
                let pts = curve(
                    |t| {
                        let wave = 3.6 * (t * TAU * 1.15 + if side > 0.0 { PI } else { 0.0 }).sin() * t;
                        (32.0 + side * 13.5 * t + wave, 49.0 - 34.0 * t, 4.0 - 2.2 * t)
                    },
                    40,
                );
                if stroke(x, y, &pts) {
                    return c(WHITE);
                }
            }
            bg
        }

        /// 6. "Cloud in orbit": a steam cloud, a ring around it with a gap, steam escaping through the gap.
        pub fn r6(x: f32, y: f32) -> Rgba {
            let Some(bg) = badge(x, y, 32.0, 32.0, 29.0) else { return [0.0; 4] };
            if cloud(x, y, 32.0, 34.0, 1.45) {
                return c(WHITE);
            }
            let ang = (-(y - 32.0)).atan2(x - 32.0).to_degrees();
            if ring(x, y, 32.0, 32.0, 21.5, 3.0) && !(15.0..=75.0).contains(&ang) {
                return c(WHITE);
            }
            if circ(x, y, 47.0, 17.5, 2.6) || circ(x, y, 51.5, 12.5, 2.0) || circ(x, y, 55.0, 8.5, 1.4) {
                return c(WHITE);
            }
            bg
        }

        #[test]
        #[ignore]
        fn logo_variants_3() {
            let dir = std::path::PathBuf::from(std::env::var("VAPORUST_LOGOS").unwrap());
            let all: [fn(f32, f32) -> Rgba; 6] = [r1, r2, r3, r4, r5, r6];
            let (cell, pad) = (200u32, 24u32);
            let w = all.len() as u32 * (cell + pad) + pad;
            let h = pad + cell + pad + 64 + pad;
            let mut sheet = image::RgbaImage::from_pixel(w, h, image::Rgba([236, 239, 243, 255]));
            for y in (pad + cell + pad / 2)..h {
                for x in 0..w {
                    sheet.put_pixel(x, y, image::Rgba([32, 32, 32, 255]));
                }
            }
            for (i, f) in all.iter().enumerate() {
                let x0 = pad + i as u32 * (cell + pad);
                image::imageops::overlay(&mut sheet, &render(cell, *f), x0 as i64, pad as i64);
                let y1 = pad + cell + pad + 8;
                image::imageops::overlay(&mut sheet, &render(48, *f), x0 as i64 + 24, y1 as i64);
                image::imageops::overlay(&mut sheet, &render(32, *f), x0 as i64 + 96, y1 as i64 + 8);
                image::imageops::overlay(&mut sheet, &render(16, *f), x0 as i64 + 152, y1 as i64 + 16);
                render(256, *f).save(dir.join(format!("logo3_{}.png", i + 1))).unwrap();
            }
            sheet.save(dir.join("logo_variants_3.png")).unwrap();
        }
    }
}

/// Large window icon (taskbar, Alt+Tab) - from the exe's own resources. eframe sets only
/// the small icon, and Windows took the large one from its icon cache, which could still hold
/// the old logo of the same file.
#[cfg(windows)]
pub fn set_taskbar_icon(hwnd: isize) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON,
        SM_CYICON, SM_CYSMICON, WM_SETICON,
    };
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        // Icon resource #1 (see build.rs).
        let id = 1usize as *const u16;
        for (kind, cx, cy) in [(ICON_BIG, SM_CXICON, SM_CYICON), (ICON_SMALL, SM_CXSMICON, SM_CYSMICON)] {
            let icon = LoadImageW(module, id, IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_DEFAULTCOLOR);
            if !icon.is_null() {
                SendMessageW(hwnd as _, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}

#[cfg(not(windows))]
pub fn set_taskbar_icon(_hwnd: isize) {}

#[cfg(test)]
mod shape_tests {
    /// Tray icon - white silhouette without a background (transparent corner), window icon - rounded square.
    #[test]
    fn tray_is_round() {
        let tray = super::icon(super::Kind::Tray);
        let win = super::icon(super::Kind::Launcher);
        let px = |d: &egui_alpha::Data, x: usize, y: usize| d.rgba[(y * d.width as usize + x) * 4 + 3];
        // Point (8, 8): inside the rounded square (radius 14 from 3..61) but outside a radius-30 circle.
        assert_eq!(px(&tray, 8, 8), 0, "у силуэта угол прозрачный");
        assert!(px(&win, 8, 8) > 200, "у квадрата угол закрашен");
    }

    mod egui_alpha {
        pub type Data = eframe::egui::IconData;
    }
}

#[cfg(test)]
mod tray_sheet {
    /// Tray icon large and at sizes 16/20/24/32 on dark and light backgrounds:
    /// VAPORUST_TRAY=<file.png> cargo test tray_sheet -- --ignored
    #[test]
    #[ignore]
    fn tray_sheet() {
        let out = std::env::var("VAPORUST_TRAY").unwrap();
        let mut sheet = image::RgbaImage::from_pixel(560, 300, image::Rgba([32, 32, 32, 255]));
        for y in 150..300 {
            for x in 0..560 {
                sheet.put_pixel(x, y, image::Rgba([225, 228, 232, 255]));
            }
        }
        let img = |n: usize| {
            let d = super::icon_sized(super::Kind::Tray, n);
            image::RgbaImage::from_raw(n as u32, n as u32, d.rgba.clone()).unwrap()
        };
        for (row, y0) in [(0, 10i64), (1, 160)] {
            let _ = row;
            image::imageops::overlay(&mut sheet, &img(128), 10, y0);
            let mut x = 160i64;
            for n in [16usize, 20, 24, 32, 48] {
                image::imageops::overlay(&mut sheet, &img(n), x, y0 + 50);
                x += n as i64 + 40;
            }
        }
        sheet.save(out).unwrap();
    }
}

/// Tray silhouette color: white on a dark taskbar, almost black on a light one
/// (HKCU\...\Themes\Personalize, SystemUsesLightTheme = 1).
fn tray_ink() -> u8 {
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let light = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
            .is_ok_and(|v| v == 1);
        if light {
            return 24;
        }
    }
    255
}
