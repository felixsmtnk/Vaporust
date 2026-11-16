//! Логотип Vaporust, нарисованный в коде (векторно, без файлов): буква V из шатуна с поршнем
//! (механика пара, отсылка к Steam) и голубого рычага, сходящихся на медной шестерне (Rust).
//! Поле 64x64; `mark` возвращает цвет точки с прозрачностью или None (вне знака).

use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// Круглая эмблема.
    Circle,
    /// Скругленный квадрат (как значок приложения).
    Squircle,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub shape: Shape,
    /// Тонкий зубчатый обод вокруг круга (шестерня-рамка).
    pub rim: bool,
    /// Небольшое приглушенное облачко пара между рычагами.
    pub steam: bool,
}

/// Вариант по умолчанию (значок exe, окна, трея и README).
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

/// Точка в системе рычага: (вдоль от a к b, поперек), длина рычага.
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

/// Цвет знака в точке (x, y) поля 64x64: [r, g, b, a] (a = 1) или None.
pub fn mark(x: f32, y: f32, st: Style) -> Option<[f32; 4]> {
    // Фон.
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
    // Зубчатый обод (только у круга): тонкое кольцо с зубцами, чуть светлее фона.
    if st.rim && st.shape == Shape::Circle {
        let d = dist(x, y, (32.0, 32.0));
        let ang = (y - 32.0).atan2(x - 32.0);
        let tooth = ((ang * 18.0 / TAU).rem_euclid(1.0) < 0.45) as i32 as f32;
        let outer = 26.6 + tooth * 1.9;
        if d >= 24.9 && d <= outer {
            col = mix(col, [60.0, 74.0, 98.0], 0.85);
        }
    }
    // Облачко пара: три круга, приглушенный цвет (почти фон).
    if st.steam {
        let puff = [((35.0, 13.0), 2.9), ((38.5, 12.0), 3.2), ((41.5, 13.6), 2.4)];
        if puff.iter().any(|(p, r)| dist(x, y, *p) <= *r) {
            col = mix(col, [190.0, 205.0, 222.0], 0.35);
        }
    }
    let pivot = (32.0, 44.5);
    // Медная шестерня (Rust) внизу: 8 зубцов.
    let d = dist(x, y, pivot);
    let ang = (y - pivot.1).atan2(x - pivot.0);
    // Зубец - трапеция: у основания шире, к вершине уже (rem_euclid, а не fract: угол бывает отрицательным).
    let phase = (ang * 10.0 / TAU).rem_euclid(1.0);
    let off = (phase - 0.5).abs();
    let tooth_r = if off < 0.16 { 13.0 } else if off < 0.24 { 13.0 - (off - 0.16) / 0.08 * 2.6 } else { 10.4 };
    if d <= tooth_r {
        col = if d <= 7.6 { mix(COPPER, COPPER_LIGHT, 0.0) } else { COPPER };
        if d > 6.9 && d < 7.9 {
            col = mix(COPPER, COPPER_LIGHT, 0.6); // светлый поясок
        }
    }
    // Левый рычаг: шатун с поршнем вверху (серо-синий).
    let piston_top = (17.5, 16.0);
    let (s, w, len) = local(x, y, pivot, piston_top);
    if s >= 0.0 && s <= len - 5.0 && w.abs() <= 2.9 - s / len * 0.6 {
        col = if w < -0.6 { SLATE_LIGHT } else { SLATE };
    }
    // Поршень: прямоугольник поперек шатуна, с двумя медными кольцами сверху.
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
    // Правый рычаг: голубой, светлая середина, проушина на конце.
    let tip = (46.5, 14.5);
    let (s2, w2, len2) = local(x, y, pivot, tip);
    if s2 >= 0.0 && s2 <= len2 && w2.abs() <= 2.6 {
        col = if w2.abs() <= 0.9 { CYAN_LIGHT } else { CYAN };
    }
    let de = dist(x, y, tip);
    if de <= 4.3 {
        col = if de <= 1.8 { HOLE } else { CYAN };
    }
    // Ступица поверх рычагов: темное кольцо и медная ось.
    if d <= 5.2 {
        col = if d <= 2.7 { COPPER_LIGHT } else if d <= 3.6 { HOLE } else { COPPER };
        if d > 4.4 {
            col = HOLE;
        }
    }
    Some([col[0], col[1], col[2], 1.0])
}

/// RGBA-картинка знака размером n x n (4x4 выборки на пиксель).
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
    /// Лист вариантов: VAPORUST_LOGOS=<папка> cargo test logo_sheet -- --ignored --nocapture
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
    /// Перерисовать assets/logo.png (256, для README): VAPORUST_WRITE_LOGO=1 cargo test write_logo_png -- --ignored
    #[test]
    #[ignore]
    fn write_logo_png() {
        assert!(std::env::var("VAPORUST_WRITE_LOGO").is_ok());
        let n = 256;
        let img = image::RgbaImage::from_raw(n as u32, n as u32, super::render(n, super::DEFAULT)).unwrap();
        img.save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("logo.png")).unwrap();
    }
}
