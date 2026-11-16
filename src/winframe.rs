//! Vaporust's own window frame: a draggable title bar, "minimize / maximize /
//! close" buttons, resizing by the edges, a thin outline. In "Settings -> Window" the normal
//! Windows frame can be enabled (switches right away, without a restart).
//!
//! Button icons are drawn with lines, not font characters (the font may lack them).

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

static SYSTEM: AtomicBool = AtomicBool::new(false);

/// Title bar height.
pub const HEIGHT: f32 = 30.0;

pub fn set_system(on: bool) {
    SYSTEM.store(on, Ordering::Relaxed);
}

/// true - normal Windows frame, false - our own.
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

/// Title bar of a separate window with its own name (from with_title).
pub fn title_bar_auto(ctx: &egui::Context) {
    if system() {
        return;
    }
    let title = ctx.input(|i| i.viewport().title.clone()).unwrap_or_else(|| "Vaporust".to_string());
    title_bar(ctx, &title);
}

/// Window title bar (and edges for resizing). Call FIRST in a window, before other panels.
/// Draws nothing with the system frame.
pub fn title_bar(ctx: &egui::Context, title: &str) {
    if system() {
        return;
    }
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let vp = ctx.viewport_id();
    if !maximized {
        resize_edges(ctx);
        // Window outline: without the system frame the edges otherwise blend into the background.
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
            // The whole bar is for dragging; buttons added later overlap it.
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
            // Buttons from right to left: close, maximize, minimize.
            let w = 46.0;
            let close = egui::Rect::from_min_max(egui::pos2(rect.right() - w, rect.top()), rect.right_bottom());
            let max = close.translate(egui::vec2(-w, 0.0));
            let min = max.translate(egui::vec2(-w, 0.0));
            if button(ui, close, "vaporust_tb_close", Glyph::Close, tr!("Закрыть", "Close")) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            let (g, hint) = if maximized { (Glyph::Restore, tr!("Свернуть в окно", "Restore down")) } else { (Glyph::Maximize, tr!("Развернуть", "Maximize")) };
            if button(ui, max, "vaporust_tb_max", g, hint) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if button(ui, min, "vaporust_tb_min", Glyph::Minimize, tr!("Свернуть", "Collapse")) {
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

/// Window edges (6 pixels): arrow cursor and resizing with the mouse.
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

/// Self-healing of the main window size. On some PCs (seen on a laptop with Intel graphics
/// after an update) the window without the system frame was drawn shifted down: a black bar on top
/// as tall as the Windows title bar, and the mouse missed the buttons. The cause - the program
/// thinks the window client area is smaller than it is. We compare it with Windows and, if they differ
/// for longer than half a second, make Windows send the size again (SetWindowPos there and back).
#[cfg(windows)]
pub fn heal_client_size(ctx: &egui::Context) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsIconic, SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOZORDER,
    };
    // (since when they differ, when it was last fixed)
    static STATE: Mutex<(Option<Instant>, Option<Instant>)> = Mutex::new((None, None));
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() {
        return;
    }
    let hwnd = hwnd as HWND;
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    if unsafe { IsIconic(hwnd) } != 0 || unsafe { GetClientRect(hwnd, &mut rc) } == 0 {
        return;
    }
    let (cw, ch) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
    if cw < 50.0 || ch < 50.0 {
        return;
    }
    let ppp = ctx.pixels_per_point();
    let screen = ctx.input(|i| i.screen_rect());
    let (ew, eh) = (screen.width() * ppp, screen.height() * ppp);
    let mut st = STATE.lock().unwrap();
    if (cw - ew).abs() <= 2.0 && (ch - eh).abs() <= 2.0 {
        st.0 = None;
        return;
    }
    let now = Instant::now();
    let since = *st.0.get_or_insert(now);
    // While the user drags a window edge, the sizes briefly differ - that is normal.
    ctx.request_repaint_after(Duration::from_millis(300));
    if now.duration_since(since) < Duration::from_millis(500) {
        return;
    }
    if st.1.is_some_and(|t| now.duration_since(t) < Duration::from_secs(3)) {
        return;
    }
    st.1 = Some(now);
    st.0 = None;
    let mut wr: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut wr) } == 0 {
        return;
    }
    let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
    let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
    unsafe {
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, w + 1, h, flags);
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, w, h, flags);
    }
    crate::errlog::write(
        tr!("окно", "window"),
        &trf!("размер рабочей области расходился (Windows {cw}x{ch}, программа {ew:.0}x{eh:.0}), исправлено", "the client area size differed (Windows {cw}x{ch}, program {ew:.0}x{eh:.0}), fixed"),
    );
}

#[cfg(not(windows))]
pub fn heal_client_size(_ctx: &egui::Context) {}

/// Windows draws the window title bar itself although the frame is ours (seen on a laptop with Intel graphics:
/// a black bar with system buttons on top, and our title bar under it). The sign: the window is taller than
/// its client area by the title bar height. First we ask Windows to recompute the frame; if
/// the title bar stays for longer than 1.5 s - true (the caller switches to the normal Windows frame).
#[cfg(windows)]
pub fn caption_forced() -> Option<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsIconic, IsZoomed, SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER,
    };
    // (since when the title bar is visible, whether a frame recompute was already requested)
    static STATE: Mutex<(Option<Instant>, bool)> = Mutex::new((None, false));
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() || system() {
        return None;
    }
    let hwnd = hwnd as HWND;
    if unsafe { IsIconic(hwnd) } != 0 || unsafe { IsZoomed(hwnd) } != 0 {
        return None;
    }
    let (mut wr, mut cr): (RECT, RECT) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    if unsafe { GetWindowRect(hwnd, &mut wr) } == 0 || unsafe { GetClientRect(hwnd, &mut cr) } == 0 {
        return None;
    }
    let extra = (wr.bottom - wr.top) - (cr.bottom - cr.top);
    let mut st = STATE.lock().unwrap();
    if extra <= 20 {
        st.0 = None;
        return None;
    }
    let now = Instant::now();
    let since = *st.0.get_or_insert(now);
    if !st.1 {
        st.1 = true;
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, flags) };
        return None;
    }
    if now.duration_since(since) < Duration::from_millis(1500) {
        return None;
    }
    Some(trf!(
        "окно {}x{}, рабочая область {}x{}", "window {}x{}, client area {}x{}",
        wr.right - wr.left,
        wr.bottom - wr.top,
        cr.right - cr.left,
        cr.bottom - cr.top
    ))
}

#[cfg(not(windows))]
pub fn caption_forced() -> Option<String> {
    None
}

/// Main window without a separate title bar (as in Steam): only edges for resizing
/// and a thin outline. Window buttons are in the section row (window_buttons), dragging -
/// by the empty space of that row (drag_behind).
pub fn frame_only(ctx: &egui::Context) {
    if system() {
        return;
    }
    if !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        resize_edges(ctx);
        let rect = ctx.screen_rect();
        let stroke = egui::Stroke::new(1.0, ctx.style().visuals.widgets.noninteractive.bg_stroke.color);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("vaporust_frame_border")))
            .rect_stroke(rect.shrink(0.5), 0.0, stroke);
    }
}

/// Dragging the window by the empty space of a bar. The area is taken from the previous frame and
/// registered BEFORE the bar's buttons - so the buttons on top of it stay clickable.
/// Double click - maximize / restore. After the bar call remember_drag(ui, id, rect).
pub fn drag_behind(ui: &mut egui::Ui, id: egui::Id) {
    if system() {
        return;
    }
    let Some(rect) = ui.ctx().data(|d| d.get_temp::<egui::Rect>(id)) else { return };
    let r = ui.interact(rect, id.with("drag"), egui::Sense::click_and_drag());
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    if r.double_clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

pub fn remember_drag(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(id, rect));
}

/// Window buttons "close / maximize / minimize" in the current row (right-to-left layout).
pub fn window_buttons(ui: &mut egui::Ui, height: f32) {
    if system() {
        return;
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let size = egui::vec2(46.0, height);
    let (close, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if button(ui, close, "vaporust_tb_close", Glyph::Close, tr!("Закрыть", "Close")) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (max, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let (g, hint) = if maximized { (Glyph::Restore, tr!("Свернуть в окно", "Restore down")) } else { (Glyph::Maximize, tr!("Развернуть", "Maximize")) };
    if button(ui, max, "vaporust_tb_max", g, hint) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    let (min, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if button(ui, min, "vaporust_tb_min", Glyph::Minimize, tr!("Свернуть", "Collapse")) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    });
}

/// Once per run (3 s after start) - a line about the window in the log: Windows style, window and
/// client area sizes, frame. Needed to understand the black bar on top on some PCs.
#[cfg(windows)]
pub fn diag_once(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowLongW, GetWindowRect, IsZoomed, GWL_EXSTYLE, GWL_STYLE};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::Relaxed) || ctx.input(|i| i.time) < 3.0 {
        return;
    }
    let hwnd = crate::notify::main_hwnd();
    if hwnd == 0 || crate::notify::in_tray() {
        return;
    }
    DONE.store(true, Ordering::Relaxed);
    let hwnd = hwnd as HWND;
    let (mut wr, mut cr): (RECT, RECT) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    unsafe {
        GetWindowRect(hwnd, &mut wr);
        GetClientRect(hwnd, &mut cr);
    }
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    let screen = ctx.input(|i| i.screen_rect());
    crate::errlog::write(
        tr!("окно", "window"),
        &trf!(
            "рамка {}, стиль {style:08x}, доп. стиль {ex:08x}, окно {}x{} ({},{}), рабочая область {}x{}, \
             программа {:.0}x{:.0} при масштабе {:.2}, развернуто {}", "frame {}, style {style:08x}, ex style {ex:08x}, window {}x{} ({},{}), client area {}x{}, program {:.0}x{:.0} at scale {:.2}, maximized {}",
            if system() { "Windows" } else { tr!("своя", "own") },
            wr.right - wr.left,
            wr.bottom - wr.top,
            wr.left,
            wr.top,
            cr.right - cr.left,
            cr.bottom - cr.top,
            screen.width(),
            screen.height(),
            ctx.pixels_per_point(),
            unsafe { IsZoomed(hwnd) } != 0
        ),
    );
}

#[cfg(not(windows))]
pub fn diag_once(_ctx: &egui::Context) {}

/// Whether this GPU needs the normal Windows frame (by the OpenGL VENDOR / RENDERER strings).
pub fn needs_system_frame(vendor: &str, renderer: &str) -> bool {
    let v = format!("{vendor} {renderer}").to_lowercase();
    v.contains("intel")
}

#[cfg(test)]
mod gpu_tests {
    #[test]
    fn intel_detect() {
        assert!(super::needs_system_frame("Intel", "Intel(R) HD Graphics 4000"));
        assert!(!super::needs_system_frame("NVIDIA Corporation", "NVIDIA GeForce GTX 1060/PCIe/SSE2"));
        assert!(!super::needs_system_frame("ATI Technologies Inc.", "AMD Radeon RX 580"));
    }
}
