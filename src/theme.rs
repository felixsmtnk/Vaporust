//! The look of the Steam desktop client, made compact: the same palette (taken from the Steam client's
//! own stylesheet), the same structure of panels and controls, smaller paddings and buttons.

use eframe::egui;
use egui::Color32;

// ---------- Palette (Steam client values) ----------

/// Top bars, bottom bar, the head of the game list: Steam #171d25.
pub const TOP: Color32 = Color32::from_rgb(23, 29, 37);
/// Main page background.
pub const BG: Color32 = Color32::from_rgb(32, 36, 43);
/// Game list on the left: Steam #23262e.
pub const SIDEBAR: Color32 = Color32::from_rgb(35, 38, 46);
/// Boxes on pages (right column, links band, cards).
pub const CARD: Color32 = Color32::from_rgb(40, 47, 56);
/// Secondary buttons and separators: Steam #3d4450.
pub const BUTTON: Color32 = Color32::from_rgb(61, 68, 80);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(82, 91, 106);
/// Selected row of a list (the game list in the library).
pub const SELECTED: Color32 = Color32::from_rgb(62, 78, 104);
/// Steam blue: links, the selected section, progress bars: #1a9fff.
pub const ACCENT: Color32 = Color32::from_rgb(26, 159, 255);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(71, 191, 255);
/// The green "PLAY" button: Steam #59bf40 (gradient to PLAY_DARK).
pub const PLAY: Color32 = Color32::from_rgb(89, 191, 64);
pub const PLAY_DARK: Color32 = Color32::from_rgb(62, 160, 44);
/// Green "in game" text.
pub const PLAY_TEXT: Color32 = Color32::from_rgb(145, 194, 87);
/// Main text: Steam #dcdedf.
pub const TEXT: Color32 = Color32::from_rgb(220, 222, 223);
/// Secondary text: Steam #b8bcbf.
pub const TEXT2: Color32 = Color32::from_rgb(184, 188, 191);
/// Weak text: Steam #8b929a.
pub const WEAK: Color32 = Color32::from_rgb(139, 146, 154);
/// The dimmest text (labels of empty values): Steam #67707b.
pub const DIM: Color32 = Color32::from_rgb(103, 112, 123);
/// Section headers ("FRIENDS WHO PLAY", "Recent Games").
pub const SECTION: Color32 = Color32::from_rgb(156, 173, 183);
/// Light blue (an available update next to a game name, online friends): Steam #74d1ff / #6dcff6.
pub const LIGHT_BLUE: Color32 = Color32::from_rgb(116, 209, 255);
/// A friend online (Steam's blue) and in game (Steam's green).
pub const ONLINE: Color32 = Color32::from_rgb(109, 207, 246);
pub const IN_GAME: Color32 = Color32::from_rgb(144, 186, 60);
pub const OFFLINE: Color32 = Color32::from_rgb(137, 137, 137);
/// Menus and pop-ups.
pub const MENU: Color32 = Color32::from_rgb(61, 68, 80);

/// The semibold font (Segoe UI Semibold) for the top sections, titles and the "PLAY" button.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = MENU;
    v.extreme_bg_color = Color32::from_rgb(24, 28, 35);
    v.faint_bg_color = Color32::from_rgb(37, 42, 50);
    v.code_bg_color = Color32::from_rgb(24, 28, 35);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(35, 98, 160);
    v.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    v.override_text_color = None;
    v.warn_fg_color = Color32::from_rgb(230, 190, 90);
    v.error_fg_color = Color32::from_rgb(235, 110, 100);
    v.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(28, 32, 38));
    v.window_rounding = egui::Rounding::same(3.0);
    v.menu_rounding = egui::Rounding::same(3.0);
    v.window_shadow = egui::epaint::Shadow { offset: egui::vec2(0.0, 6.0), blur: 16.0, spread: 0.0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = egui::epaint::Shadow { offset: egui::vec2(0.0, 4.0), blur: 10.0, spread: 0.0, color: Color32::from_black_alpha(140) };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG;
    w.noninteractive.weak_bg_fill = BG;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(52, 58, 68));
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    // Buttons, as in Steam: a flat slate plate (#3d4450), lighter on hover, blue when pressed or open.
    w.inactive.bg_fill = BUTTON;
    w.inactive.weak_bg_fill = BUTTON;
    w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    w.hovered.bg_fill = BUTTON_HOVER;
    w.hovered.weak_bg_fill = BUTTON_HOVER;
    w.hovered.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    w.active.bg_fill = ACCENT;
    w.active.weak_bg_fill = ACCENT;
    w.active.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    w.open.bg_fill = Color32::from_rgb(73, 81, 95);
    w.open.weak_bg_fill = Color32::from_rgb(73, 81, 95);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.rounding = egui::Rounding::same(2.0);
        s.bg_stroke = egui::Stroke::NONE;
        s.expansion = 0.0;
    }
    // The dark theme always: with ThemePreference::System and a light Windows theme egui switched to
    // its light style after the first frame, and our dark panels got dark text and white buttons.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Small, FontId::new(11.0, Proportional)),
            (TextStyle::Body, FontId::new(13.0, Proportional)),
            (TextStyle::Button, FontId::new(13.0, Proportional)),
            (TextStyle::Heading, FontId::new(18.0, egui::FontFamily::Name("semibold".into()))),
            (TextStyle::Monospace, FontId::new(12.0, egui::FontFamily::Monospace)),
        ]
        .into();
        s.spacing.button_padding = egui::vec2(10.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        s.spacing.menu_margin = egui::Margin::same(4.0);
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
    let s = egui::Stroke::new(1.8_f32, c);
    let (m, d) = (r.center(), r.width() * 0.36);
    p.line_segment([m - egui::vec2(d, 0.0), m + egui::vec2(d, 0.0)], s);
    p.line_segment([m - egui::vec2(0.0, d), m + egui::vec2(0.0, d)], s);
}

/// Two people icon (Friends and chat).
pub fn icon_friends(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.5_f32, c);
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
    let font = egui::FontId::proportional(12.5);
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
    let h = (galley.size().y + pad.y * 2.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || active, 0.12);
    // As in Steam: no plate at rest, a faint one on hover; the open section keeps white text.
    let fill = lerp_color(Color32::TRANSPARENT, Color32::from_rgba_unmultiplied(61, 68, 80, 150), if active { t.max(0.5) } else { t });
    let color = if active { Color32::WHITE } else { lerp_color(TEXT2, Color32::WHITE, t) };
    ui.painter().rect_filled(rect, 2.0, fill);
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
    p.circle_stroke(m, 4.6, egui::Stroke::new(1.5_f32, c));
    p.line_segment([m + egui::vec2(3.4, 3.4), m + egui::vec2(7.0, 7.0)], egui::Stroke::new(1.8_f32, c));
}

/// House icon (library "Home").
pub fn icon_home(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.5_f32, c);
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
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    // As in Steam settings: the selected item is a slate plate, the hovered one a fainter one.
    let fill = if selected { BUTTON } else { lerp_color(Color32::TRANSPARENT, Color32::from_rgb(45, 51, 61), t) };
    ui.painter().rect_filled(rect, 2.0, fill);
    let color = if selected { Color32::WHITE } else { lerp_color(TEXT, Color32::WHITE, t) };
    let mut x = rect.left() + 12.0;
    if let Some(f) = icon {
        f(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), egui::vec2(14.0, 14.0)), color);
        x += 22.0;
    }
    let font = egui::FontId::proportional(13.5);
    ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, text, font, color);
    resp
}

// ---------- Steam-like building blocks ----------

/// Text with extra space between the letters (Steam capital headers use letter-spacing).
pub fn spaced(text: &str, font: egui::FontId, color: Color32, spacing: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, egui::TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    job
}

/// A section header in capitals ("FRIENDS WHO PLAY", "ACHIEVEMENTS"), with an optional count after it.
pub fn caps_title(ui: &mut egui::Ui, text: &str, count: Option<usize>) -> egui::Response {
    let mut job = spaced(&text.to_uppercase(), semibold(12.5), SECTION, 1.0);
    if let Some(n) = count {
        job.append(&format!("  ({n})"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: DIM, ..Default::default() });
    }
    ui.add(egui::Label::new(job).selectable(false))
}

/// A shelf header of the library home ("Recent Games"): normal case, a thin line to the right.
pub fn shelf_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui.add(egui::Label::new(egui::RichText::new(text).font(semibold(15.0)).color(SECTION)).selectable(false));
    let y = resp.rect.center().y + 1.0;
    let x0 = resp.rect.right() + 10.0;
    let x1 = ui.max_rect().right();
    if x1 > x0 {
        ui.painter().line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(103, 112, 123, 70)));
    }
    resp
}

/// The Steam on/off switch (settings).
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(34.0, 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    // A disabled switch is dimmed and shows no hand cursor.
    let enabled = ui.is_enabled();
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let dim = if enabled { 1.0 } else { 0.4 };
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let track = lerp_color(Color32::from_rgb(70, 77, 88), ACCENT, t).gamma_multiply(dim);
    ui.painter().rect_filled(rect, 9.0, track);
    let x = egui::lerp(rect.left() + 9.0..=rect.right() - 9.0, t);
    let knob = if resp.hovered() && enabled { Color32::WHITE } else { Color32::from_rgb(236, 238, 240) }.gamma_multiply(dim);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 6.5, knob);
    resp
}

/// A square icon button of the play bar (settings gear, info, favorite).
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    ui.painter().rect_filled(rect, 2.0, lerp_color(BUTTON, BUTTON_HOVER, t));
    let c = if on { ACCENT } else { lerp_color(TEXT2, Color32::WHITE, t) };
    icon(ui.painter(), rect.shrink(7.0), c);
    resp
}

/// The blue Steam button (the main action of a dialog).
pub fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(ACCENT))
}

/// The big Steam action button ("PLAY", "INSTALL", "UPDATE"): a gradient plate, an icon and capitals.
/// `fill` is the left color, `dark` the right one. Compact: 34 px high.
pub fn action_button(ui: &mut egui::Ui, text: &str, fill: Color32, dark: Color32, icon: Option<Icon>, enabled: bool) -> egui::Response {
    let galley = ui.painter().layout_job(spaced(&text.to_uppercase(), semibold(15.0), Color32::WHITE, 0.8));
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let w = (galley.size().x + 40.0 + icon_w).max(128.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let t = if enabled { ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1) } else { 0.0 };
    let (l, r) = if enabled { (lerp_color(fill, Color32::WHITE, t * 0.15), lerp_color(dark, fill, t * 0.6)) } else { (BUTTON, BUTTON) };
    gradient_rect(ui.painter(), rect, l, r);
    let x = rect.center().x - (galley.size().x + icon_w) / 2.0;
    if let Some(f) = icon {
        f(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), egui::vec2(14.0, 14.0)), Color32::WHITE);
    }
    let color = if enabled { Color32::WHITE } else { WEAK };
    ui.painter().galley_with_override_text_color(egui::pos2(x + icon_w, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// A horizontal gradient rectangle (left color to right color).
pub fn gradient_rect(p: &egui::Painter, rect: egui::Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

/// A vertical gradient (top color to bottom color).
pub fn vgradient_rect(p: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

/// "Play" triangle.
pub fn icon_play(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let s = r.height() * 0.42;
    p.add(egui::Shape::convex_polygon(vec![m + egui::vec2(-s * 0.7, -s), m + egui::vec2(s, 0.0), m + egui::vec2(-s * 0.7, s)], c, egui::Stroke::NONE));
}

/// Download arrow into a tray ("Install", "Manage downloads").
pub fn icon_download(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.6_f32, c);
    let m = r.center();
    let h = r.height() * 0.5;
    p.line_segment([m + egui::vec2(0.0, -h), m + egui::vec2(0.0, h * 0.35)], s);
    p.add(egui::Shape::line(vec![m + egui::vec2(-h * 0.5, -h * 0.15), m + egui::vec2(0.0, h * 0.35), m + egui::vec2(h * 0.5, -h * 0.15)], s));
    p.add(egui::Shape::line(vec![m + egui::vec2(-h, h * 0.5), m + egui::vec2(-h, h), m + egui::vec2(h, h), m + egui::vec2(h, h * 0.5)], s));
}

/// Gear (game properties).
pub fn icon_gear(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let rad = r.height() * 0.32;
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let d = egui::vec2(a.cos(), a.sin());
        p.line_segment([m + d * rad, m + d * rad * 1.55], egui::Stroke::new(rad * 0.55, c));
    }
    p.circle_filled(m, rad * 1.1, c);
    p.circle_filled(m, rad * 0.45, BUTTON);
}

/// "i" in a circle (game information).
pub fn icon_info(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let rad = r.height() * 0.5;
    p.circle_filled(m, rad, c);
    p.circle_filled(m + egui::vec2(0.0, -rad * 0.45), rad * 0.16, BUTTON);
    p.line_segment([m + egui::vec2(0.0, -rad * 0.1), m + egui::vec2(0.0, rad * 0.55)], egui::Stroke::new(rad * 0.3, BUTTON));
}

/// Heart (favorites).
pub fn icon_heart(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let s = r.height() * 0.26;
    p.circle_filled(m + egui::vec2(-s, -s * 0.3), s * 1.05, c);
    p.circle_filled(m + egui::vec2(s, -s * 0.3), s * 1.05, c);
    p.add(egui::Shape::convex_polygon(
        vec![m + egui::vec2(-s * 2.0, -s * 0.05), m + egui::vec2(s * 2.0, -s * 0.05), m + egui::vec2(0.0, s * 2.1)],
        c,
        egui::Stroke::NONE,
    ));
}

/// Four squares (library collections view).
pub fn icon_grid(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = r.width() / 2.0 - 1.0;
    for (dx, dy) in [(0.0, 0.0), (s + 2.0, 0.0), (0.0, s + 2.0), (s + 2.0, s + 2.0)] {
        p.rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(dx, dy), egui::vec2(s, s)), 1.0, c);
    }
}

/// Clock (recent games first).
pub fn icon_clock(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let rad = r.height() * 0.45;
    p.circle_stroke(m, rad, egui::Stroke::new(1.4_f32, c));
    p.line_segment([m, m + egui::vec2(0.0, -rad * 0.6)], egui::Stroke::new(1.4_f32, c));
    p.line_segment([m, m + egui::vec2(rad * 0.45, rad * 0.2)], egui::Stroke::new(1.4_f32, c));
}

/// Play in a circle (ready-to-play filter).
pub fn icon_play_circle(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let rad = r.height() * 0.45;
    p.circle_stroke(m, rad, egui::Stroke::new(1.4_f32, c));
    icon_play(p, egui::Rect::from_center_size(m + egui::vec2(0.8, 0.0), egui::vec2(rad, rad)), c);
}

/// Filter lines (advanced filters).
pub fn icon_filter(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.4_f32, c);
    let m = r.center();
    let w = r.width() * 0.5;
    for (i, k) in [1.0, 0.65, 0.3].iter().enumerate() {
        let y = m.y - 4.0 + i as f32 * 4.0;
        p.line_segment([egui::pos2(m.x - w * k, y), egui::pos2(m.x + w * k, y)], s);
    }
}

/// Arrow left (back).
pub fn icon_back(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.8_f32, c);
    let m = r.center();
    let h = r.height() * 0.4;
    p.line_segment([m + egui::vec2(-h, 0.0), m + egui::vec2(h, 0.0)], s);
    p.add(egui::Shape::line(vec![m + egui::vec2(-h * 0.1, -h * 0.85), m + egui::vec2(-h, 0.0), m + egui::vec2(-h * 0.1, h * 0.85)], s));
}

/// Arrow right (forward).
pub fn icon_forward(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = egui::Stroke::new(1.8_f32, c);
    let m = r.center();
    let h = r.height() * 0.4;
    p.line_segment([m + egui::vec2(-h, 0.0), m + egui::vec2(h, 0.0)], s);
    p.add(egui::Shape::line(vec![m + egui::vec2(h * 0.1, -h * 0.85), m + egui::vec2(h, 0.0), m + egui::vec2(h * 0.1, h * 0.85)], s));
}

/// Small down arrow (drop-down menus).
pub fn icon_caret(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let m = r.center();
    let s = r.height() * 0.3;
    p.add(egui::Shape::convex_polygon(vec![m + egui::vec2(-s, -s * 0.5), m + egui::vec2(s, -s * 0.5), m + egui::vec2(0.0, s * 0.6)], c, egui::Stroke::NONE));
}

/// A clickable icon without a plate (title bar arrows, list header icons).
pub fn bare_icon(ui: &mut egui::Ui, icon: Icon, size: f32, enabled: bool, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let t = if enabled { ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1) } else { 0.0 };
    let c = if !enabled {
        BUTTON
    } else if on {
        ACCENT
    } else {
        lerp_color(WEAK, Color32::WHITE, t)
    };
    icon(ui.painter(), rect.shrink(size * 0.18), c);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A thin Steam progress bar (dark track, blue fill) with an optional caption to the right.
pub fn progress(ui: &mut egui::Ui, frac: f32, width: f32, text: Option<String>) -> egui::Response {
    ui.horizontal(|ui| {
        let (bar, resp) = ui.allocate_exact_size(egui::vec2(width.max(20.0), 6.0), egui::Sense::hover());
        ui.painter().rect_filled(bar, 3.0, Color32::from_rgb(14, 20, 27));
        let mut f = bar;
        f.set_width(bar.width() * frac.clamp(0.0, 1.0));
        ui.painter().rect_filled(f, 3.0, ACCENT);
        if let Some(t) = text {
            ui.label(egui::RichText::new(t).size(12.0).color(TEXT2));
        }
        resp
    })
    .inner
}

/// A Steam switch with a caption that can be turned off (`enabled` false - greyed, not clickable).
/// The caption click switches too. Returns the switch response (`changed()` after a switch).
pub fn switch(ui: &mut egui::Ui, enabled: bool, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            let mut r = toggle(ui, on);
            let l = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
            if l.clicked() {
                *on = !*on;
                r.mark_changed();
            }
            r.union(l)
        })
        .inner
    })
    .inner
}

/// A page tab as in Steam (profile, community, inventory): text, white with a blue underline when
/// selected, lighter on hover. No plate.
pub fn tab(ui: &mut egui::Ui, on: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let galley = text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::FontId(egui::FontId::proportional(14.0)));
    let pad = egui::vec2(8.0, 5.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let color = if on { Color32::WHITE } else { lerp_color(TEXT2, Color32::WHITE, t) };
    ui.painter().galley_with_override_text_color(rect.min + pad, galley, color);
    if on {
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left() + pad.x, rect.bottom() - 2.0), egui::pos2(rect.right() - pad.x, rect.bottom())), 1.0, ACCENT);
    }
    resp
}

/// [`tab`] bound to a value, like `selectable_value`.
pub fn tab_value<V: PartialEq>(ui: &mut egui::Ui, current: &mut V, value: V, text: impl Into<egui::WidgetText>) -> egui::Response {
    let mut r = tab(ui, *current == value, text);
    if r.clicked() && *current != value {
        *current = value;
        r.mark_changed();
    }
    r
}

// ---------- Page building blocks (store, community, library subpages) ----------

/// A page title as in the Steam client ("YOUR SHOPPING CART", "Manage Downloads"): light, large, capitals.
pub fn page_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(spaced(&text.to_uppercase(), egui::FontId::proportional(24.0), Color32::WHITE, 1.2)).selectable(false))
}

/// A dark block of a page (the Steam "panel"): slightly lighter than the page, square corners.
pub fn block<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 60))
        .rounding(3.0)
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A plain text link (Steam's "Remove", "View all"): grey, white and underlined on hover.
pub fn text_link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), WEAK);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let c = if resp.hovered() { Color32::WHITE } else { WEAK };
    if resp.hovered() {
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(1.0_f32, c));
    }
    ui.painter().galley_with_override_text_color(rect.min, galley, c);
    resp
}

/// The green Steam purchase button ("Continue to payment", "Add to Cart").
pub fn buy_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    action_button(ui, text, Color32::from_rgb(117, 183, 44), Color32::from_rgb(88, 141, 33), None, true)
}

/// Esc was pressed this frame: dialogs close on it, as "Cancel".
pub fn esc(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}
