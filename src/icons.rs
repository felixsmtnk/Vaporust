//! Иконки окон, нарисованные в коде (без файлов-картинок): 64x64 RGBA.

use eframe::egui;
use std::sync::Arc;

pub enum Kind {
    /// Главное окно: кнопка «играть».
    Launcher,
    /// Окно чата: облачко сообщения.
    Chat,
    /// Иконка трея с красной точкой: есть непрочитанные сообщения.
    LauncherUnread,
}

pub fn icon(kind: Kind) -> Arc<egui::IconData> {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Фон: скруглённый квадрат с вертикальным градиентом (тёмно-синий -> синий).
            let bg = rounded_rect(fx, fy, 2.0, 2.0, 62.0, 62.0, 14.0);
            let t = fy / N as f32;
            let base = [
                (20.0 + 30.0 * t) as u8,
                (40.0 + 70.0 * t) as u8,
                (80.0 + 110.0 * t) as u8,
            ];
            // Белый знак поверх фона.
            let mark = match kind {
                Kind::Launcher | Kind::LauncherUnread => {
                    triangle(fx, fy, (24.0, 16.0), (24.0, 48.0), (50.0, 32.0))
                }
                Kind::Chat => {
                    let bubble = rounded_rect(fx, fy, 12.0, 14.0, 52.0, 42.0, 9.0);
                    let tail = triangle(fx, fy, (20.0, 38.0), (32.0, 41.0), (17.0, 52.0));
                    bubble.max(tail)
                }
            };
            let i = (y * N + x) * 4;
            for c in 0..3 {
                rgba[i + c] = (base[c] as f32 * (1.0 - mark) + 255.0 * mark) as u8;
            }
            rgba[i + 3] = (bg * 255.0) as u8;
            // Красная точка в правом верхнем углу: есть непрочитанные сообщения.
            if matches!(kind, Kind::LauncherUnread) {
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

/// Покрытие (0..1) точки скруглённым прямоугольником, со сглаживанием краёв.
fn rounded_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r + 0.5 - d).clamp(0.0, 1.0)
}

/// Покрытие точки треугольником (по знакам расстояний до сторон, со сглаживанием).
fn triangle(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let edge = |p: (f32, f32), q: (f32, f32)| {
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt();
        ((x - p.0) * dy - (y - p.1) * dx) / len
    };
    let (e1, e2, e3) = (edge(a, b), edge(b, c), edge(c, a));
    // Ориентация треугольника может быть любой: берём ту, где все знаки совпадают.
    let inside = if e1 + e2 + e3 < 0.0 { -e1.max(e2).max(e3) } else { e1.min(e2).min(e3) };
    (inside + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    /// VAPORUST_ICONS=<папка> cargo test save_icons -- --ignored
    #[test]
    #[ignore]
    fn save_icons() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_ICONS").unwrap());
        for (k, n) in [(super::Kind::Launcher, "launcher"), (super::Kind::Chat, "chat")] {
            let i = super::icon(k);
            let img = image::RgbaImage::from_raw(i.width, i.height, i.rgba.clone()).unwrap();
            let big = image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Nearest);
            big.save(dir.join(format!("{n}.png"))).unwrap();
        }
    }
}
