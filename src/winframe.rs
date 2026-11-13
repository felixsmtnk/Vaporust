//! Своя рамка окон Vaporust: заголовок с перетаскиванием, кнопки «свернуть / развернуть /
//! закрыть», изменение размера за края, тонкая обводка. В «Настройки -> Окно» можно включить
//! обычную рамку Windows (переключается сразу, без перезапуска).
//!
//! Значки кнопок рисуются линиями, а не символами шрифта (в шрифте их может не быть).

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

static SYSTEM: AtomicBool = AtomicBool::new(false);

/// Высота заголовка.
pub const HEIGHT: f32 = 30.0;

pub fn set_system(on: bool) {
    SYSTEM.store(on, Ordering::Relaxed);
}

/// true - обычная рамка Windows, false - своя.
pub fn system() -> bool {
    SYSTEM.load(Ordering::Relaxed)
}

fn bar_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(22, 26, 33)
    } else {
        egui::Color32::from_rgb(222, 226, 232)
    }
}

/// Заголовок отдельного окна с его собственным названием (из with_title).
pub fn title_bar_auto(ctx: &egui::Context) {
    if system() {
        return;
    }
    let title = ctx.input(|i| i.viewport().title.clone()).unwrap_or_else(|| "Vaporust".to_string());
    title_bar(ctx, &title);
}

/// Заголовок окна (и края для изменения размера). Вызывать в окне ПЕРВЫМ, до остальных панелей.
/// При системной рамке ничего не рисует.
pub fn title_bar(ctx: &egui::Context, title: &str) {
    if system() {
        return;
    }
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let vp = ctx.viewport_id();
    if !maximized {
        resize_edges(ctx);
        // Обводка окна: без системной рамки края иначе сливаются с фоном.
        let rect = ctx.screen_rect();
        let stroke = egui::Stroke::new(1.0, ctx.style().visuals.widgets.noninteractive.bg_stroke.color);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("vaporust_frame_border")))
            .rect_stroke(rect.shrink(0.5), 0.0, stroke);
    }
    let fill = bar_color(&ctx.style().visuals);
    egui::TopBottomPanel::top(egui::Id::new(("vaporust_titlebar", vp)))
        .exact_height(HEIGHT)
        .frame(egui::Frame::none().fill(fill))
        .show_separator_line(false)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            // Вся полоса - для перетаскивания; кнопки, добавленные позже, перекрывают ее.
            let drag = ui.interact(rect, egui::Id::new(("vaporust_titlebar_drag", vp)), egui::Sense::click_and_drag());
            if drag.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            let title = crate::richtext::fix(ui, title);
            ui.painter().text(
                rect.left_center() + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(13.5),
                ui.visuals().text_color(),
            );
            // Кнопки справа налево: закрыть, развернуть, свернуть.
            let w = 46.0;
            let close = egui::Rect::from_min_max(egui::pos2(rect.right() - w, rect.top()), rect.right_bottom());
            let max = close.translate(egui::vec2(-w, 0.0));
            let min = max.translate(egui::vec2(-w, 0.0));
            if button(ui, close, "vaporust_tb_close", Glyph::Close, "Закрыть") {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            let (g, hint) = if maximized { (Glyph::Restore, "Свернуть в окно") } else { (Glyph::Maximize, "Развернуть") };
            if button(ui, max, "vaporust_tb_max", g, hint) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if button(ui, min, "vaporust_tb_min", Glyph::Minimize, "Свернуть") {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        });
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn button(ui: &mut egui::Ui, rect: egui::Rect, id: &str, glyph: Glyph, hint: &str) -> bool {
    let r = ui.interact(rect, egui::Id::new((id, ui.ctx().viewport_id())), egui::Sense::click()).on_hover_text(hint);
    let p = ui.painter();
    let mut color = ui.visuals().text_color();
    if r.hovered() {
        let bg = if glyph == Glyph::Close {
            color = egui::Color32::WHITE;
            egui::Color32::from_rgb(196, 43, 28)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        p.rect_filled(rect, 0.0, bg);
    }
    let c = rect.center();
    let s = egui::Stroke::new(1.0, color);
    let h = 5.0;
    match glyph {
        Glyph::Minimize => {
            p.line_segment([egui::pos2(c.x - h, c.y), egui::pos2(c.x + h, c.y)], s);
        }
        Glyph::Maximize => {
            p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(2.0 * h, 2.0 * h)), 0.0, s);
        }
        Glyph::Restore => {
            let back = egui::Rect::from_center_size(c + egui::vec2(2.0, -2.0), egui::vec2(8.0, 8.0));
            let front = egui::Rect::from_center_size(c + egui::vec2(-1.0, 1.0), egui::vec2(8.0, 8.0));
            p.line_segment([back.left_top(), back.right_top()], s);
            p.line_segment([back.right_top(), back.right_bottom()], s);
            p.rect_filled(front, 0.0, if r.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { bar_color(ui.visuals()) });
            p.rect_stroke(front, 0.0, s);
        }
        Glyph::Close => {
            p.line_segment([c + egui::vec2(-h, -h), c + egui::vec2(h, h)], s);
            p.line_segment([c + egui::vec2(-h, h), c + egui::vec2(h, -h)], s);
        }
    }
    r.clicked()
}

/// Края окна (6 пикселей): курсор-стрелка и изменение размера мышью.
fn resize_edges(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let rect = ctx.screen_rect();
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let m = 6.0;
    let (l, r, t, b) = (p.x < rect.left() + m, p.x > rect.right() - m, p.y < rect.top() + m, p.y > rect.bottom() - m);
    let dir = match (l, r, t, b) {
        (true, _, true, _) => Some((D::NorthWest, C::ResizeNwSe)),
        (_, true, true, _) => Some((D::NorthEast, C::ResizeNeSw)),
        (true, _, _, true) => Some((D::SouthWest, C::ResizeNeSw)),
        (_, true, _, true) => Some((D::SouthEast, C::ResizeNwSe)),
        (true, ..) => Some((D::West, C::ResizeHorizontal)),
        (_, true, ..) => Some((D::East, C::ResizeHorizontal)),
        (_, _, true, _) => Some((D::North, C::ResizeVertical)),
        (_, _, _, true) => Some((D::South, C::ResizeVertical)),
        _ => None,
    };
    if let Some((d, icon)) = dir {
        ctx.set_cursor_icon(icon);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}
