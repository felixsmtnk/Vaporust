//! A light loading icon. The usual egui::Spinner repaints the window every frame
//! (up to 60 times a second) while visible: on an old laptop that is a noticeable load, and
//! network loading can take a while. Here the icon updates 12 times a second.

use eframe::egui;

pub fn spinner(ui: &mut egui::Ui) -> egui::Response {
    let size = ui.style().spacing.interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let time = ui.input(|i| i.time);
        // Rotation in steps of 1/12 turn: a repaint is needed only on each step.
        // Without animations (setting) - a still icon, the window is not repainted for it.
        let anim = crate::imgload::animations();
        let step = if anim { (time * 12.0).floor() } else { 0.0 };
        let start = step / 12.0 * std::f64::consts::TAU;
        let radius = rect.height() / 2.0 - 2.0;
        let n = 20;
        let points: Vec<egui::Pos2> = (0..n)
            .map(|i| {
                let a = start + (i as f64 / n as f64) * std::f64::consts::PI * 1.5;
                rect.center() + radius * egui::vec2(a.cos() as f32, a.sin() as f32)
            })
            .collect();
        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color())));
        if anim {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(83));
        }
    }
    response
}
