//! Оформление в духе клиента Steam: темно-синие панели, голубые выделения, зеленая «Играть».

use eframe::egui;
use egui::Color32;

/// Верхняя полоса и заголовок окна.
pub const TOP: Color32 = Color32::from_rgb(23, 26, 33);
/// Основной фон.
pub const BG: Color32 = Color32::from_rgb(27, 32, 40);
/// Список игр слева.
pub const SIDEBAR: Color32 = Color32::from_rgb(33, 38, 47);
/// Подложки, карточки.
pub const CARD: Color32 = Color32::from_rgb(40, 46, 56);
/// Голубой акцент (выбранное, ссылки, «Установить»).
pub const ACCENT: Color32 = Color32::from_rgb(26, 159, 255);
/// Зеленая кнопка «Играть».
pub const PLAY: Color32 = Color32::from_rgb(92, 158, 30);
/// Зеленый текст «в игре» (светлее кнопки «ИГРАТЬ»).
pub const PLAY_TEXT: Color32 = Color32::from_rgb(140, 200, 70);
pub const TEXT: Color32 = Color32::from_rgb(220, 222, 224);
pub const WEAK: Color32 = Color32::from_rgb(140, 150, 162);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(30, 35, 44);
    v.extreme_bg_color = Color32::from_rgb(18, 21, 27);
    v.faint_bg_color = CARD;
    v.code_bg_color = CARD;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(35, 98, 160);
    v.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    v.override_text_color = None;
    v.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(50, 57, 69));
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG;
    w.noninteractive.weak_bg_fill = BG;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(47, 54, 66));
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    w.inactive.bg_fill = Color32::from_rgb(52, 60, 73);
    w.inactive.weak_bg_fill = Color32::from_rgb(52, 60, 73);
    w.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = Color32::from_rgb(64, 92, 124);
    w.hovered.weak_bg_fill = Color32::from_rgb(64, 92, 124);
    w.hovered.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgb(35, 98, 160);
    w.active.weak_bg_fill = Color32::from_rgb(35, 98, 160);
    w.active.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    w.open.bg_fill = Color32::from_rgb(45, 52, 64);
    w.open.weak_bg_fill = Color32::from_rgb(45, 52, 64);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.rounding = egui::Rounding::same(3.0);
        s.bg_stroke = egui::Stroke::NONE;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
    });
}

/// Большая цветная кнопка (как «Играть» / «Установить» в Steam).
pub fn big_button(ui: &mut egui::Ui, text: &str, fill: Color32, enabled: bool) -> egui::Response {
    let t = egui::RichText::new(text).size(16.0).strong().color(Color32::WHITE);
    let b = egui::Button::new(t).fill(fill).min_size(egui::vec2(150.0, 40.0)).rounding(3.0);
    ui.add_enabled(enabled, b)
}

/// Колонка не шире `max`, по центру (как страницы Steam на большом экране).
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
