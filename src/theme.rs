//! Styling in the spirit of the Steam client: dark blue panels, light blue highlights, a green "Play".

use eframe::egui;
use egui::Color32;

/// Top bar and window title.
pub const TOP: Color32 = Color32::from_rgb(23, 26, 33);
/// Main background.
pub const BG: Color32 = Color32::from_rgb(27, 32, 40);
/// Game list on the left.
pub const SIDEBAR: Color32 = Color32::from_rgb(33, 38, 47);
/// Backings, cards.
pub const CARD: Color32 = Color32::from_rgb(40, 46, 56);
/// Light blue accent (selection, links, "Install").
pub const ACCENT: Color32 = Color32::from_rgb(26, 159, 255);
/// Green "Play" button.
pub const PLAY: Color32 = Color32::from_rgb(92, 158, 30);
/// Green "in game" text (lighter than the "PLAY" button).
pub const PLAY_TEXT: Color32 = Color32::from_rgb(140, 200, 70);
pub const TEXT: Color32 = Color32::from_rgb(220, 222, 224);
pub const WEAK: Color32 = Color32::from_rgb(140, 150, 162);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(30, 35, 44);
    // Text fields: the same dark plate as on the sign-in page.
    v.extreme_bg_color = CARD;
    v.faint_bg_color = CARD;
    v.code_bg_color = CARD;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(35, 98, 160);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    v.override_text_color = None;
    v.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(50, 57, 69));
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG;
    w.noninteractive.weak_bg_fill = BG;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(47, 54, 66));
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    // Buttons and lists: dark plates; on hover a soft lighter gray (not a bright blue), pressed - blue.
    w.inactive.bg_fill = Color32::from_rgb(44, 51, 62);
    w.inactive.weak_bg_fill = Color32::from_rgb(44, 51, 62);
    w.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = Color32::from_rgb(58, 67, 81);
    w.hovered.weak_bg_fill = Color32::from_rgb(58, 67, 81);
    w.hovered.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgb(35, 98, 160);
    w.active.weak_bg_fill = Color32::from_rgb(35, 98, 160);
    w.active.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    w.open.bg_fill = Color32::from_rgb(52, 60, 73);
    w.open.weak_bg_fill = Color32::from_rgb(52, 60, 73);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.rounding = egui::Rounding::same(4.0);
        s.bg_stroke = egui::Stroke::NONE;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
    });
}

/// A large colored button (like "Play" / "Install" in Steam).
pub fn big_button(ui: &mut egui::Ui, text: &str, fill: Color32, enabled: bool) -> egui::Response {
    let t = egui::RichText::new(text).size(16.0).strong().color(Color32::WHITE);
    let b = egui::Button::new(t).fill(fill).min_size(egui::vec2(150.0, 40.0)).rounding(3.0);
    ui.add_enabled(enabled, b)
}

/// A column no wider than `max`, centered (like Steam pages on a large screen).
pub fn centered<R>(ui: &mut egui::Ui, max: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let w = ui.available_width();
    if w <= max + 8.0 {
        return add(ui);
    }
    let pad = ((w - max) / 2.0).floor();
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(max);
            add(ui)
        })
        .inner
    })
    .inner
}

/// A small icon drawn with lines (the font has no suitable symbols).
pub type Icon = fn(&egui::Painter, egui::Rect, Color32);

/// "+" icon (Add a game).
pub fn icon_plus(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.8, c);
    let (m, d) = (r.center(), r.width() * 0.36);
    p.line_segment([m - egui::vec2(d, 0.0), m + egui::vec2(d, 0.0)], s);
    p.line_segment([m - egui::vec2(0.0, d), m + egui::vec2(0.0, d)], s);
}

/// Two people icon (Friends and chat).
pub fn icon_friends(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.5, c);
    let m = r.center();
    // The front person.
    p.circle_stroke(m + egui::vec2(-2.0, -2.5), 2.6, s);
    let body: Vec<egui::Pos2> = (0..=10)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 10.0);
            m + egui::vec2(-2.0 + a.cos() * 4.6, 6.0 + a.sin() * 4.0)
        })
        .collect();
    p.add(egui::Shape::line(body, s));
    // The one behind, on the right.
    p.circle_stroke(m + egui::vec2(4.2, -3.5), 2.1, s);
    p.line_segment([m + egui::vec2(5.0, 0.6), m + egui::vec2(7.6, 3.6)], s);
}

/// A button of the bottom bar ("+ Add a game", "Downloads", "Friends and chat"): no frame at rest,
/// on hover the background and the text light up smoothly; `active` keeps it lit (the open
/// section). An optional icon on the left or on the right of the text. All three look alike.
pub fn bar_button(ui: &mut egui::Ui, text: &str, left: Option<Icon>, right: Option<Icon>, active: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let icon = 14.0;
    let gap = 7.0;
    let pad = egui::vec2(12.0, 5.0);
    let mut w = galley.size().x + pad.x * 2.0;
    if left.is_some() {
        w += icon + gap;
    }
    if right.is_some() {
        w += icon + gap;
    }
    let h = (galley.size().y + pad.y * 2.0).max(26.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || active, 0.12);
    let rest = if active { Color32::from_rgb(30, 52, 76) } else { TOP };
    let fill = lerp_color(rest, if active { Color32::from_rgb(38, 66, 96) } else { Color32::from_rgb(46, 54, 67) }, t);
    let color = lerp_color(TEXT, Color32::WHITE, t);
    ui.painter().rect_filled(rect, 4.0, fill);
    let mut x = rect.left() + pad.x;
    let cy = rect.center().y;
    if let Some(f) = left {
        f(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + icon / 2.0, cy), egui::vec2(icon, icon)), color);
        x += icon + gap;
    }
    ui.painter().galley_with_override_text_color(egui::pos2(x, cy - galley.size().y / 2.0), galley.clone(), color);
    x += galley.size().x + gap;
    if let Some(f) = right {
        f(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + icon / 2.0, cy), egui::vec2(icon, icon)), color);
    }
    resp
}

/// Mix two colors: t = 0 - `a`, 1 - `b`.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    // Color32 keeps channels already multiplied by alpha: mixing them is mixing premultiplied
    // colors, so the result is premultiplied too (from_rgba_unmultiplied would multiply again).
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// Magnifier icon.
pub fn icon_search(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center() + egui::vec2(-1.5, -1.5);
    p.circle_stroke(m, 4.6, egui::Stroke::new(1.5, c));
    p.line_segment([m + egui::vec2(3.4, 3.4), m + egui::vec2(7.0, 7.0)], egui::Stroke::new(1.8, c));
}

/// House icon (library "Home").
pub fn icon_home(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.5, c);
    let m = r.center();
    let roof = vec![m + egui::vec2(-6.5, 0.0), m + egui::vec2(0.0, -6.0), m + egui::vec2(6.5, 0.0)];
    p.add(egui::Shape::line(roof, s));
    let body = vec![m + egui::vec2(-4.5, -1.5), m + egui::vec2(-4.5, 5.5), m + egui::vec2(4.5, 5.5), m + egui::vec2(4.5, -1.5)];
    p.add(egui::Shape::line(body, s));
}

/// A search field: a dark plate with a magnifier, like the fields of the sign-in page.
pub fn search_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    let mut out = None;
    egui::Frame::none().fill(CARD).rounding(4.0).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 0.0, bottom: 0.0 }).show(ui, |ui| {
        ui.set_width(width - 16.0);
        ui.set_height(28.0);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            icon_search(ui.painter(), r, WEAK);
            out = Some(ui.add(egui::TextEdit::singleline(text).frame(false).hint_text(hint).desired_width(ui.available_width())));
        });
    });
    out.expect("search field drawn")
}

/// An item of a side list (library "Home", settings sections): the selected one is a plate with a
/// blue marker on the left; on hover the background lights up smoothly.
pub fn side_item(ui: &mut egui::Ui, selected: bool, text: &str, icon: Option<Icon>) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let fill = if selected { CARD } else { lerp_color(Color32::TRANSPARENT, Color32::from_rgb(40, 46, 56), t) };
    ui.painter().rect_filled(rect, 4.0, fill);
    if selected {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(3.0, rect.height() - 12.0)), 1.5, ACCENT);
    }
    let color = if selected { Color32::WHITE } else { lerp_color(TEXT, Color32::WHITE, t) };
    let mut x = rect.left() + 12.0;
    if let Some(f) = icon {
        f(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), egui::vec2(14.0, 14.0)), color);
        x += 22.0;
    }
    let font = egui::FontId::proportional(14.0);
    ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, text, font, color);
    resp
}
