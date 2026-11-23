//! Library drawing: compact list (as before) or a grid of covers.
//! Action buttons are shared by both views (`main_button`), so the code is not duplicated.

use crate::covers::{self, Cover, Covers};
use crate::settings::Sort;
use crate::{fmt_day, fmt_hours, net, theme, Row};
use eframe::egui;

/// What the user clicked on a game.
pub enum Action {
    Play(u32),
    Update(u32),
    Install(u32, String),
    Props(u32, String),
    Achievements(u32, String),
    /// Open the game page (in the library).
    Page(u32),
    /// Add (true) or remove a game from a collection.
    Collection { id: String, appid: u32, add: bool },
    /// Create a collection with this game.
    NewCollection { name: String, appid: u32 },
    /// Open the game's store page.
    Store(u32),
    /// Open the game's Workshop (separate window).
    Workshop(u32, String),
    /// Game discussions and guides.
    Community(u32, String),
    /// Screenshots of this game.
    Shots(u32),
    /// The game is already downloaded - point to the folder with the files.
    Import(u32, String),
    /// Open the game folder in Explorer.
    Folder(u32),
    /// Own note for the game (edit window).
    Note(u32, String),
    /// Desktop shortcut for the game.
    Shortcut(u32, String),
    /// Close the running game (like the window's close button).
    Stop(u32),
    /// Pause the game's running download.
    Pause(u32),
    /// Ask whether to install the game (double click on a non-installed one).
    AskInstall(u32, String),
    /// Uninstall the game from disk (with confirmation).
    Uninstall(u32, String),
}

/// State needed for the buttons (the same for all rows of a frame).
pub struct Ctx<'a> {
    /// Title of the shown list (collection, genre) for the grid header.
    pub list_title: String,
    pub launching: bool,
    pub installing: bool,
    pub logged_in: bool,
    pub net: &'a net::NetState,
    /// Games in the download queue (not paused).
    pub queued: &'a std::collections::HashSet<u32>,
    /// Paused downloads ("RESUME" button).
    pub paused: &'a std::collections::HashSet<u32>,
    /// Steam collections (for the right-click menu).
    pub collections: &'a [crate::colls::Coll],
    /// Update via Steam (for the "Update" button hint).
    pub update_via_steam: bool,
    /// Which game is running now ("STOP" button instead of "PLAY").
    pub running: Option<u32>,
    /// Which game Vaporust is launching (or closing) now and what is happening to it.
    pub status_app: Option<u32>,
    pub status: &'a str,
}

impl Ctx<'_> {
    fn needs_update(&self, r: &Row) -> bool {
        r.needs_update || self.net.updates.as_ref().is_some_and(|u| u.contains(&r.appid))
    }
    fn installing_now(&self, appid: u32) -> bool {
        self.net.install.as_ref().is_some_and(|i| i.active() && i.appid == appid)
    }
}

/// Compact view: game table (icon, name, state, hours, last played, size).
/// Click on a header - sort, click on a row - game page, double click - "Play",
/// right click - game menu. Rows have no buttons: all actions are on the game page and in the menu.
pub fn list(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, sort: &mut Sort, icons: bool) -> Option<Action> {
    let mut out = None;
    let w = ui.available_width() - 14.0;
    let (state_w, hours_w, last_w, size_w) = (130.0, 80.0, 128.0, 90.0);
    let name_w = (w - state_w - hours_w - last_w - size_w).max(120.0);
    let xs = [0.0, name_w, name_w + state_w, name_w + state_w + hours_w, name_w + state_w + hours_w + last_w];
    let small = egui::FontId::proportional(12.5);

    // Header: clicking a sortable column sorts by it.
    let (head, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
    let heads: [(&str, Option<Sort>); 5] = [
        (tr!("НАЗВАНИЕ", "NAME"), Some(Sort::Name)),
        (tr!("СОСТОЯНИЕ", "STATUS"), None),
        (tr!("В ИГРЕ", "PLAYED"), Some(Sort::Playtime)),
        (tr!("ПОСЛЕДНИЙ ЗАПУСК", "LAST PLAYED"), Some(Sort::LastPlayed)),
        (tr!("НА ДИСКЕ", "ON DISK"), Some(Sort::Size)),
    ];
    for (i, (label, s)) in heads.iter().enumerate() {
        let x0 = head.left() + xs[i] + if i == 0 && icons { 30.0 } else { 6.0 };
        let r = egui::Rect::from_min_max(egui::pos2(x0, head.top()), egui::pos2(head.left() + xs.get(i + 1).copied().unwrap_or(w), head.bottom()));
        let resp = ui.interact(r, ui.id().with(("table_head", i)), if s.is_some() { egui::Sense::click() } else { egui::Sense::hover() });
        let on = s.is_some_and(|s| s == *sort);
        let color = if on { theme::ACCENT } else if resp.hovered() && s.is_some() { egui::Color32::WHITE } else { theme::WEAK };
        // Text does not spill into the next column: draw only inside its own cell (with an 8 gap).
        let cell = egui::Rect::from_min_max(r.min, egui::pos2((r.right() - 8.0).max(r.left()), r.bottom()));
        ui.painter_at(cell).text(r.left_center(), egui::Align2::LEFT_CENTER, *label, egui::FontId::proportional(11.0), color);
        if let Some(s) = s {
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *sort = *s;
            }
        }
    }
    ui.painter().hline(head.x_range(), head.bottom(), egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(50, 57, 69)));

    let row_h = 28.0;
    // No spacing between rows, set BEFORE show_rows: it counts row height together with the spacing,
    // while rows are drawn back to back - otherwise empty space piled up at the bottom when scrolling.
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for r in &rows[range] {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::click());
            let p = ui.painter_at(rect);
            if resp.hovered() {
                p.rect_filled(rect, 2.0, egui::Color32::from_rgb(48, 55, 67));
            }
            let mut x = rect.left() + 6.0;
            if icons {
                let ir = egui::Rect::from_min_size(egui::pos2(rect.left() + 4.0, rect.top() + 4.0), egui::vec2(20.0, 20.0));
                match covers.icon(r.appid) {
                    Cover::Ready(t) => {
                        let tint = if r.installed { egui::Color32::WHITE } else { egui::Color32::from_gray(130) };
                        p.image(t.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
                    }
                    _ => {
                        p.rect_filled(ir, 2.0, egui::Color32::from_rgb(52, 60, 73));
                    }
                }
                x = ir.right() + 6.0;
            }
            let cell = |p: &egui::Painter, x: f32, max: f32, text: String, font: egui::FontId, color: egui::Color32| {
                let galley = ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(max);
                    f.layout_job(job)
                });
                p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
            };
            let name_color = if r.installed { theme::TEXT } else { theme::WEAK };
            let mut name = r.name.clone();
            if r.family {
                name.push_str(tr!("  (семья)", "  (family)"));
            }
            cell(&p, x, rect.left() + xs[1] - x - 8.0, name, egui::FontId::proportional(13.5), name_color);
            let (state, color) = if c.running == Some(r.appid) {
                (tr!("в игре", "playing"), theme::PLAY_TEXT)
            } else if c.installing_now(r.appid) {
                (tr!("загружается", "downloading"), theme::ACCENT)
            } else if c.queued.contains(&r.appid) {
                (tr!("в очереди", "queued"), theme::ACCENT)
            } else if r.ready && c.needs_update(r) {
                (tr!("есть обновление", "update available"), egui::Color32::from_rgb(230, 170, 60))
            } else if r.ready {
                (tr!("установлена", "installed"), egui::Color32::from_rgb(120, 190, 80))
            } else if r.installed {
                (tr!("недокачана", "not finished"), theme::WEAK)
            } else {
                (tr!("не установлена", "not installed"), theme::WEAK)
            };
            cell(&p, rect.left() + xs[1] + 6.0, state_w - 10.0, state.into(), small.clone(), color);
            let hours = r.playtime_min.filter(|m| *m > 0).map(fmt_hours).unwrap_or_default();
            cell(&p, rect.left() + xs[2] + 6.0, hours_w - 10.0, hours, small.clone(), theme::WEAK);
            let last = if r.last_played > 0 { fmt_day(r.last_played) } else { String::new() };
            cell(&p, rect.left() + xs[3] + 6.0, last_w - 10.0, last, small.clone(), theme::WEAK);
            let size = if r.size > 0 { net::fmt_bytes(r.size) } else { String::new() };
            cell(&p, rect.left() + xs[4] + 6.0, size_w - 10.0, size, small.clone(), theme::WEAK);
            collections_menu(&resp, r, c, &mut out);
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.double_clicked() && r.ready && !c.launching && !c.installing {
                out = Some(Action::Play(r.appid));
            } else if resp.double_clicked() && !r.ready && c.logged_in && !c.installing_now(r.appid) && !crate::custom::is_custom(r.appid) {
                // As in Steam: double click on a non-installed game - offer to install.
                out = Some(Action::AskInstall(r.appid, r.name.clone()));
            } else if resp.clicked() {
                // The page opens a moment later, so a second click can still make a double click
                // (otherwise the first click replaced the table with the page at once).
                let now = ui.input(|i| i.time);
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("table_pending_page"), (r.appid, now)));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(320));
            }
            if resp.double_clicked() {
                ui.ctx().data_mut(|d| d.remove::<(u32, f64)>(egui::Id::new("table_pending_page")));
            }
        }
    });
    let pending: Option<(u32, f64)> = ui.ctx().data(|d| d.get_temp(egui::Id::new("table_pending_page")));
    if let Some((appid, at)) = pending {
        if ui.input(|i| i.time) - at >= 0.3 {
            ui.ctx().data_mut(|d| d.remove::<(u32, f64)>(egui::Id::new("table_pending_page")));
            if out.is_none() {
                out = Some(Action::Page(appid));
            }
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
    }
    out
}

/// Grid of covers. Only visible rows are drawn (and loaded).
/// `recent` - the "Recent games" shelf as the first row (as on Steam's library home); empty - no shelf.
pub fn grid(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, recent: &[Row], next: &[Row]) -> Option<Action> {
    let mut out = None;
    // How many covers at least 140 points wide fit, stretched to the full width -
    // no empty space is left on the right at any window size.
    let gap = 12.0;
    let avail = ui.available_width() - 16.0;
    let min_w = covers::W as f32;
    let cols = ((avail + gap) / (min_w + gap)).floor().max(1.0) as usize;
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).clamp(min_w, 260.0);
    let tile = egui::vec2(w, w * 1.5);
    let row_h = tile.y + gap;
    // The "Recent games" shelf on top, the same size as the grid (same number of covers per row),
    // plus a caption. Rows differ in height, so visible rows are computed manually (show_viewport).
    let label_h = 26.0;
    // Under the shelf - a separator line: the shelf and the full list are different sections.
    // Shelves on top, then the "ALL GAMES (n)" header, as on the Steam library home.
    // The shelves: "Recent Games" and "Play Next" (owned, never launched), each a caption and one row.
    let shelves: Vec<(&str, &[Row], u8)> = [(tr!("Недавние игры", "Recent Games"), recent, 0u8), (tr!("Во что сыграть дальше", "Play Next"), next, 2u8)]
        .into_iter()
        .filter(|s| !s.1.is_empty())
        .collect();
    let one_shelf = label_h + row_h + 8.0;
    let shelf_h = shelves.len() as f32 * one_shelf + label_h;
    let n_rows = rows.len().div_ceil(cols);
    // The screenshot tour (tour.rs) can ask to open the grid at row N (stored as N + 1;
    // a large number - to the very bottom, egui clamps scrolling to the end of the list).
    let tour_row = TOUR_GRID_ROW.swap(0, std::sync::atomic::Ordering::Relaxed);
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("lib_grid");
    if tour_row >= 2000 {
        // 2000 + N: the bottom of row N at the bottom of the visible area.
        let bottom = shelf_h + (tour_row - 2000 + 1) as f32 * row_h;
        area = area.vertical_scroll_offset((bottom - ui.available_height()).max(0.0));
    } else if tour_row > 0 {
        area = area.vertical_scroll_offset(shelf_h + (tour_row - 1) as f32 * row_h);
    }
    area.show_viewport(ui, |ui, vp| {
        let total = shelf_h + n_rows as f32 * row_h;
        ui.set_min_height(total);
        let origin = ui.min_rect().min;
        let width = ui.available_width();
        let row_ui = |ui: &mut egui::Ui, y: f32, h: f32| {
            let rect = egui::Rect::from_min_size(egui::pos2(origin.x, origin.y + y), egui::vec2(width, h));
            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Min)))
        };
        for (i, (title, list, section)) in shelves.iter().enumerate() {
            let top = i as f32 * one_shelf;
            if vp.min.y < top + one_shelf && vp.max.y > top {
                let mut lab = row_ui(ui, top, label_h);
                lab.set_width(width - 16.0);
                theme::shelf_title(&mut lab, title);
                let mut child = row_ui(ui, top + label_h, row_h);
                child.spacing_mut().item_spacing.x = gap;
                for r in list.iter().take(cols) {
                    tile_ui(&mut child, r, c, covers, tile, *section, &mut out);
                }
            }
        }
        if vp.min.y < shelf_h {
            let mut lab = row_ui(ui, shelf_h - label_h, label_h);
            lab.set_width(width - 16.0);
            let mut job = theme::spaced(&c.list_title.to_uppercase(), theme::semibold(12.5), theme::SECTION, 1.0);
            job.append(&format!("  ({})", rows.len()), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: theme::DIM, ..Default::default() });
            let resp = lab.add(egui::Label::new(job).selectable(false));
            let y = resp.rect.center().y + 1.0;
            if resp.rect.right() + 10.0 < origin.x + width - 16.0 {
                lab.painter().hline(resp.rect.right() + 10.0..=origin.x + width - 16.0, y, egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(103, 112, 123, 70)));
            }
        }
        let first = (((vp.min.y - shelf_h) / row_h).floor().max(0.0)) as usize;
        let last = ((((vp.max.y - shelf_h) / row_h).ceil().max(0.0)) as usize).min(n_rows);
        for row in first..last {
            let mut child = row_ui(ui, shelf_h + row as f32 * row_h, row_h);
            child.spacing_mut().item_spacing.x = gap;
            for r in rows.iter().skip(row * cols).take(cols) {
                tile_ui(&mut child, r, c, covers, tile, 1, &mut out);
            }
        }
    });
    out
}

/// `section` - where the tile is (0 the "Recent" shelf, 1 the grid): one game can be in both, and
/// each has its own hover animation - otherwise both were highlighted.
fn tile_ui(ui: &mut egui::Ui, r: &Row, c: &Ctx, covers: &Covers, tile: egui::Vec2, section: u8, out: &mut Option<Action>) {
    // The grid slot is normal size, but clicks and hover are caught on an enlarged area: otherwise
    // at the edge of an enlarged tile (and its button) the pointer "fell out" of hover and the button vanished.
    let (base, _) = ui.allocate_exact_size(tile, egui::Sense::hover());
    let resp = ui.interact(base.expand2(base.size() * 0.03), egui::Id::new(("tile", section, r.appid)), egui::Sense::click());
    // On hover the whole tile smoothly grows a little (as in Steam) together with the button. It is the same
    // image, just bigger; repainting happens only during the animation (0.15 s).
    let t = ui.ctx().animate_bool_with_time(egui::Id::new(("tile_zoom", section, r.appid)), resp.hovered(), 0.15);
    let rect = base.expand2(base.size() * 0.03 * t);
    let painter = ui.painter().clone();
    match covers.get(r.appid) {
        Cover::Ready(tex) => {
            let tint = if r.installed { egui::Color32::WHITE } else { egui::Color32::from_gray(110 + (40.0 * t) as u8) };
            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        cover => {
            // While loading or without a cover - a dark card with the name.
            painter.rect_filled(rect, 4.0, egui::Color32::from_gray(40));
            // A long name - several lines inside the tile, centered.
            let text = if matches!(cover, Cover::Loading) { "…".to_string() } else { r.name.clone() };
            let mut job = egui::text::LayoutJob::simple(text, egui::FontId::proportional(13.0), egui::Color32::from_gray(180), rect.width() - 16.0);
            job.halign = egui::Align::Center;
            let galley = ui.fonts(|f| f.layout_job(job));
            let pos = egui::pos2(rect.center().x, rect.center().y - galley.size().y / 2.0);
            painter.galley(pos, galley, egui::Color32::from_gray(180));
        }
    }
    if c.installing_now(r.appid) {
        painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, tr!("загрузка…", "loading…"), egui::FontId::proportional(14.0), egui::Color32::WHITE);
    }
    collections_menu(&resp, r, c, out);
    // As in Steam: nothing under the cover; on hover a button in the corner: green "Play" for
    // installed games, blue "Install" for non-installed ones. Clicking the button - the action,
    // clicking the rest of the cover - the game page.
    let hover = resp.hovered();
    let mut button_hit: Option<Action> = None;
    let can_play = r.ready && !c.launching && !c.installing;
    let can_install = !r.installed && c.logged_in && !c.installing_now(r.appid) && !crate::custom::is_custom(r.appid);
    if hover && (can_play || can_install) {
        let s = 34.0 * (1.0 + 0.03 * t);
        let b = egui::Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.bottom() - 8.0 - s), egui::vec2(s, s));
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| b.contains(p));
        let (cx, cy) = (b.center().x, b.center().y);
        if can_play {
            painter.rect_filled(b, 4.0, if over { egui::Color32::from_rgb(112, 190, 38) } else { theme::PLAY });
            painter.add(egui::Shape::convex_polygon(
                vec![egui::pos2(cx - 4.0, cy - 8.0), egui::pos2(cx + 10.0, cy), egui::pos2(cx - 4.0, cy + 8.0)],
                egui::Color32::WHITE,
                egui::Stroke::NONE,
            ));
            if over {
                button_hit = Some(Action::Play(r.appid));
            }
        } else {
            // Blue button with a down arrow (download), like "Install" in Steam.
            painter.rect_filled(b, 4.0, if over { egui::Color32::from_rgb(71, 160, 240) } else { theme::ACCENT });
            let st = egui::Stroke::new(2.5_f32, egui::Color32::WHITE);
            painter.line_segment([egui::pos2(cx, cy - 9.0), egui::pos2(cx, cy + 5.0)], st);
            painter.line_segment([egui::pos2(cx - 6.0, cy - 1.0), egui::pos2(cx, cy + 5.0)], st);
            painter.line_segment([egui::pos2(cx + 6.0, cy - 1.0), egui::pos2(cx, cy + 5.0)], st);
            painter.line_segment([egui::pos2(cx - 8.0, cy + 10.0), egui::pos2(cx + 8.0, cy + 10.0)], st);
            if over {
                button_hit = Some(Action::AskInstall(r.appid, r.name.clone()));
            }
        }
    }
    if resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}{}{}",
            r.name,
            r.playtime_min.filter(|m| *m > 0).map(|m| format!("\n{}", fmt_hours(m))).unwrap_or_default(),
            if r.family { format!("\n{}", family_text(r, c)) } else { String::new() }
        ))
        .clicked()
    {
        *out = Some(button_hit.unwrap_or(Action::Page(r.appid)));
    }
}

/// Right-click menu, ordered as in Steam: main action, navigation, collections,
/// "Manage" (folder, uninstall) and "Properties" at the end.
fn collections_menu(resp: &egui::Response, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    // The selected action goes through a RefCell: both buttons and nested submenus use it.
    let pick: std::cell::RefCell<Option<Action>> = std::cell::RefCell::new(None);
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        ui.label(egui::RichText::new(&r.name).strong());
        ui.separator();
        let item = |ui: &mut egui::Ui, enabled: bool, text: &str, act: Action| {
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                *pick.borrow_mut() = Some(act);
                ui.close_menu();
            }
        };
        let custom = crate::custom::is_custom(r.appid);
        if r.installed {
            item(ui, r.ready && !c.launching, tr!("Играть", "Play"), Action::Play(r.appid));
        } else if c.logged_in && !custom {
            item(ui, !c.installing_now(r.appid), tr!("Установить", "Install"), Action::Install(r.appid, r.name.clone()));
        }
        item(ui, true, tr!("Страница игры", "Game page"), Action::Page(r.appid));
        if !custom {
            ui.separator();
            item(ui, c.logged_in, tr!("Достижения", "Achievements"), Action::Achievements(r.appid, r.name.clone()));
            item(ui, true, tr!("Скриншоты", "Screenshots"), Action::Shots(r.appid));
            item(ui, c.logged_in, tr!("Сообщество", "Community"), Action::Community(r.appid, r.name.clone()));
            item(ui, true, tr!("Страница в магазине", "Store page"), Action::Store(r.appid));
            ui.separator();
            ui.menu_button(tr!("Коллекции", "Collections"), |ui| {
                if c.collections.is_empty() {
                    ui.weak(tr!("Коллекции еще загружаются...", "Collections are still loading..."));
                }
                for coll in c.collections {
                    let mut inside = coll.apps.contains(&r.appid);
                    let resp = ui.add_enabled(!coll.dynamic, egui::Checkbox::new(&mut inside, &coll.name));
                    if coll.dynamic {
                        resp.on_disabled_hover_text(tr!("Динамическая коллекция (по фильтру) - меняется только в Steam", "Dynamic collection (by filter) - changes only in Steam"));
                    } else if resp.changed() {
                        *pick.borrow_mut() = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                        ui.close_menu();
                    }
                }
                ui.separator();
                // The new collection name field lives in egui's temporary memory.
                let id = egui::Id::new("game_menu_new_collection");
                let mut name = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr!("Новая коллекция", "New collection")).desired_width(140.0));
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr!("Создать", "Create"))).clicked() {
                        *pick.borrow_mut() = Some(Action::NewCollection { name: name.trim().to_string(), appid: r.appid });
                        name.clear();
                        ui.close_menu();
                    }
                });
                ui.data_mut(|d| d.insert_temp(id, name));
            });
        }
        if r.installed || custom {
            ui.separator();
            ui.menu_button(tr!("Управление", "Manage"), |ui| {
                ui.set_min_width(180.0);
                if r.installed && ui.button(tr!("Открыть папку игры", "Open game folder")).clicked() {
                    *pick.borrow_mut() = Some(Action::Folder(r.appid));
                    ui.close_menu();
                }
                let (text, enabled) = if custom {
                    (tr!("Убрать из библиотеки...", "Remove from library..."), c.running != Some(r.appid))
                } else {
                    (tr!("Удалить...", "Uninstall..."), !c.installing_now(r.appid) && c.running != Some(r.appid))
                };
                if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                    *pick.borrow_mut() = Some(Action::Uninstall(r.appid, r.name.clone()));
                    ui.close_menu();
                }
            });
            item(ui, true, tr!("Свойства...", "Properties..."), Action::Props(r.appid, r.name.clone()));
        }
    });
    if let Some(a) = pick.into_inner() {
        *out = Some(a);
    }
}

/// Game list in the left column, as in Steam: icon and name; click - game page,
/// double click - "Play", right click - game menu.
pub fn sidebar(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, selected: Option<u32>, icons: bool) -> Option<Action> {
    let mut out = None;
    let row_h = 22.0;
    // No spacing between rows, set BEFORE show_rows: it counts row height together with the spacing,
    // while rows are drawn back to back - otherwise empty space piled up at the bottom when scrolling.
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for r in &rows[range] {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
            let sel = selected == Some(r.appid);
            let p = ui.painter_at(rect);
            if sel {
                p.rect_filled(rect, 0.0, theme::SELECTED);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, egui::Color32::from_rgb(52, 58, 69));
            }
            let mut x = rect.left() + 12.0;
            if icons {
                let ir = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, rect.top() + 3.0), egui::vec2(16.0, 16.0));
                match covers.icon(r.appid) {
                    Cover::Ready(t) => {
                        let tint = if r.installed { egui::Color32::WHITE } else { egui::Color32::from_gray(130) };
                        p.image(t.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
                    }
                    _ => {
                        p.rect_filled(ir, 2.0, egui::Color32::from_rgb(52, 60, 73));
                    }
                }
                x = ir.right() + 7.0;
            }
            let (suffix, color) = if c.running == Some(r.appid) {
                (tr!(" - в игре", " - playing"), theme::PLAY_TEXT)
            } else if c.installing_now(r.appid) {
                (tr!(" - загрузка", " - downloading"), theme::ACCENT)
            } else if r.ready && c.needs_update(r) {
                (tr!(" - обновление", " - Update"), theme::LIGHT_BLUE)
            } else if r.installed && !r.ready {
                (tr!(" - недокачана", " - not finished"), theme::WEAK)
            } else if sel {
                ("", egui::Color32::WHITE)
            } else if r.installed {
                ("", theme::TEXT)
            } else {
                ("", theme::WEAK)
            };
            let name = format!("{}{suffix}", r.name);
            let galley = ui.fonts(|f| {
                let mut job = egui::text::LayoutJob::simple_singleline(name, egui::FontId::proportional(12.5), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - x - 4.0);
                f.layout_job(job)
            });
            // A cut-off name (narrow column, large scale): the full name on hover.
            let cut = galley.rows.iter().any(|row| row.ends_with_newline) || galley.rows.first().is_some_and(|row| row.glyphs.len() < r.name.chars().count());
            p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
            collections_menu(&resp, r, c, &mut out);
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let resp = if cut { resp.on_hover_text(&r.name) } else { resp };
            if resp.double_clicked() && r.ready && !c.launching && !c.installing {
                out = Some(Action::Play(r.appid));
            } else if resp.double_clicked() && !r.ready && c.logged_in && !c.installing_now(r.appid) && !crate::custom::is_custom(r.appid) {
                // As in Steam: double click on a non-installed game - offer to install.
                out = Some(Action::AskInstall(r.appid, r.name.clone()));
            } else if resp.clicked() {
                out = Some(Action::Page(r.appid));
            }
        }
    });
    out
}

/// The big button of the game page, as in Steam: "PLAY" (green), "INSTALL" (blue), "UPDATE", "STOP".
fn big_main_button(ui: &mut egui::Ui, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    let blue = (egui::Color32::from_rgb(71, 191, 255), egui::Color32::from_rgb(26, 68, 194));
    if c.running == Some(r.appid) {
        // While the game runs, "STOP" instead of "PLAY".
        let b = theme::action_button(ui, tr!("Остановить", "Stop"), blue.0, blue.1, None, true)
            .on_hover_text(tr!("Просит игру закрыться, как крестик окна или Alt+F4. Игра может спросить о сохранении.", "Asks the game to close, like the window's close button or Alt+F4. The game may ask about saving."));
        if b.clicked() {
            *out = Some(Action::Stop(r.appid));
        }
    } else if r.ready && c.launching && c.status_app == Some(r.appid) {
        theme::action_button(ui, tr!("Запуск...", "Starting..."), theme::PLAY, theme::PLAY_DARK, Some(theme::icon_play), false);
    } else if r.ready {
        let ok = !c.launching && !c.installing;
        let b = theme::action_button(ui, tr!("Играть", "Play"), theme::PLAY, theme::PLAY_DARK, Some(theme::icon_play), ok)
            .on_disabled_hover_text(tr!("Недоступно, пока идет запуск или установка", "Unavailable while a launch or installation is in progress"));
        if b.clicked() {
            *out = Some(Action::Play(r.appid));
        }
        if c.needs_update(r) {
            let u = theme::action_button(ui, tr!("Обновить", "Update"), blue.0, blue.1, Some(theme::icon_download), ok).on_hover_text(if c.update_via_steam {
                tr!("Есть обновление. Steam откроется незаметно, скачает его и закроется.", "An update is available. Steam opens unnoticed, downloads it and closes.")
            } else {
                tr!("Есть обновление. Vaporust скачает его сам, Steam не запускается.", "An update is available. Vaporust downloads it itself, Steam is not started.")
            });
            if u.clicked() {
                *out = Some(Action::Update(r.appid));
            }
        }
    } else if c.installing_now(r.appid) {
        let b = theme::action_button(ui, tr!("Пауза", "Pause"), blue.0, blue.1, None, true)
            .on_hover_text(tr!("Поставить загрузку на паузу: скачанное сохранится", "Pause the download: what was downloaded is kept"));
        if b.clicked() {
            *out = Some(Action::Pause(r.appid));
        }
    } else {
        let queued = c.queued.contains(&r.appid);
        let text = if queued {
            tr!("В очереди", "Queued")
        } else if c.paused.contains(&r.appid) || r.installed {
            tr!("Продолжить", "Resume")
        } else {
            tr!("Установить", "Install")
        };
        let b = theme::action_button(ui, text, blue.0, blue.1, Some(theme::icon_download), c.logged_in && !c.launching && !queued)
            .on_hover_text(if c.installing { tr!("Сейчас идет другая загрузка - игра встанет в очередь", "Another download is running - the game will be queued") } else { tr!("Скачать игру", "Download the game") })
            .on_disabled_hover_text(tr!("Нужен вход в аккаунт; игра уже в очереди или качается", "An account login is needed; the game is already queued or downloading"));
        if b.clicked() {
            *out = Some(Action::Install(r.appid, r.name.clone()));
        }
    }
}

/// A stat of the play bar: an icon, a capital label and the value under it; optionally a small bar.
fn stat(ui: &mut egui::Ui, icon: Option<theme::Icon>, label: &str, value: String, progress: Option<f32>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        if let Some(f) = icon {
            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            f(ui.painter(), r, theme::WEAK);
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(egui::Label::new(theme::spaced(&label.to_uppercase(), theme::semibold(12.0), theme::TEXT2, 0.8)).selectable(false));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(value).size(12.0).color(theme::WEAK));
                if let Some(f) = progress {
                    let (rr, _) = ui.allocate_exact_size(egui::vec2(56.0, 4.0), egui::Sense::hover());
                    ui.painter().rect_filled(rr, 2.0, egui::Color32::from_rgb(14, 20, 27));
                    let mut done = rr;
                    done.set_width(rr.width() * f.clamp(0.0, 1.0));
                    ui.painter().rect_filled(done, 2.0, theme::ACCENT);
                }
            });
        });
    });
}

/// A box of the right column of the game page: a capital title and a plate.
fn side_box(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::caps_title(ui, title, None);
    ui.add_space(4.0);
    egui::Frame::none().fill(theme::CARD).rounding(2.0).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(14.0);
}

/// A link of the game page band ("Store Page", "Workshop"...).
fn band_link(ui: &mut egui::Ui, text: &str, hint: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), egui::Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(18.0, 14.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if resp.hovered() { egui::Color32::WHITE } else { theme::TEXT2 };
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, color);
    if hint.is_empty() {
        resp
    } else {
        resp.on_hover_text(hint)
    }
}

/// Game page data that comes from the network.
pub struct PageData<'a> {
    pub achievements: Option<&'a Result<Vec<aurelia::steam_client::GameAchievement>, String>>,
    pub news: Option<&'a Result<Vec<aurelia::web::discovery::NewsItem>, String>>,
    pub players: Option<i32>,
    /// Store info (developer, publisher, release date).
    pub info: Option<&'a aurelia::steam_client::StoreAppInfo>,
    /// From the public store API: genres, trailer, support page.
    pub genres: &'a [String],
    pub trailer: Option<&'a str>,
    pub support: Option<&'a str>,
    /// Own note for the game.
    pub note: &'a str,
}

/// Game page in the library, laid out as in Steam: the background with the game logo, the play bar
/// (button, stats, gear / info / favorite), the band of links, then two columns: news and DLC on the
/// left; friends, achievements, information, collections and the note on the right.
pub fn page(ui: &mut egui::Ui, r: &Row, c: &Ctx, covers: &Covers, d: &PageData, covers_on: bool) -> Option<Action> {
    let mut out = None;
    let steam_game = !crate::custom::is_custom(r.appid);
    egui::ScrollArea::vertical().id_salt(("game_page", r.appid)).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // ----- Background with the logo -----
        let w = ui.available_width();
        let h = if covers_on { (w / 3.2).clamp(150.0, 300.0) } else { 70.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        match covers_on.then(|| covers.hero(r.appid)) {
            Some(Cover::Ready(tex)) => {
                let s = tex.size_vec2();
                let vis = (s.x / w * h / s.y).min(1.0);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - vis) / 2.0), egui::pos2(1.0, (1.0 + vis) / 2.0));
                painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
            _ => theme::vgradient_rect(&painter, rect, egui::Color32::from_rgb(42, 52, 66), theme::BG),
        }
        // The logo in the lower left, as in Steam; without it - the name.
        let logo = if covers_on { covers.logo(r.appid) } else { Cover::Missing };
        match logo {
            Cover::Ready(tex) => {
                let s = tex.size_vec2();
                let max = egui::vec2((w * 0.36).min(380.0), h * 0.55);
                let k = (max.x / s.x).min(max.y / s.y);
                let size = s * k;
                let lr = egui::Rect::from_min_size(egui::pos2(rect.left() + 24.0, rect.bottom() - 18.0 - size.y), size);
                painter.image(tex.id(), lr, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            _ => {
                theme::vgradient_rect(
                    &painter,
                    egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 60.0f32.min(h)), rect.max),
                    egui::Color32::from_black_alpha(0),
                    egui::Color32::from_black_alpha(170),
                );
                painter.text(egui::pos2(rect.left() + 20.0, rect.bottom() - 14.0), egui::Align2::LEFT_BOTTOM, &r.name, theme::semibold(26.0), egui::Color32::WHITE);
            }
        }

        // ----- Play bar -----
        let bar = egui::Frame::none().fill(egui::Color32::from_rgb(29, 34, 41)).inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 10.0 });
        bar.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let stats = |ui: &mut egui::Ui| {
                if r.last_played > 0 {
                    stat(ui, None, tr!("Последний запуск", "Last played"), fmt_day(r.last_played), None);
                }
                if let Some(m) = r.playtime_min.filter(|m| *m > 0) {
                    stat(ui, Some(theme::icon_clock), tr!("Время в игре", "Play time"), fmt_hours(m), None);
                }
                if let Some(Ok(list)) = d.achievements.filter(|a| a.as_ref().is_ok_and(|l| !l.is_empty())) {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    stat(ui, None, tr!("Достижения", "Achievements"), format!("{done}/{}", list.len()), Some(done as f32 / list.len() as f32));
                }
                if r.size > 0 {
                    stat(ui, None, tr!("На диске", "On disk"), net::fmt_bytes(r.size), None);
                }
            };
            let two_buttons = r.ready && c.needs_update(r);
            let wide = ui.available_width() >= if two_buttons { 980.0 } else { 820.0 };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    big_main_button(ui, r, c, &mut out);
                });
                if wide {
                    stats(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if steam_game {
                        let fav = c.collections.iter().find(|k| k.id == "favorite");
                        if let Some(f) = fav {
                            let on = f.apps.contains(&r.appid);
                            let hint = if on { tr!("Убрать из избранного", "Remove from favorites") } else { tr!("В избранное", "Add to favorites") };
                            if theme::icon_button(ui, theme::icon_heart, on).on_hover_text(hint).clicked() {
                                out = Some(Action::Collection { id: f.id.clone(), appid: r.appid, add: !on });
                            }
                        }
                        if theme::icon_button(ui, theme::icon_info, false).on_hover_text(tr!("Страница в магазине", "Store page")).clicked() {
                            out = Some(Action::Store(r.appid));
                        }
                    }
                    if r.installed || !steam_game {
                        if theme::icon_button(ui, theme::icon_gear, false)
                            .on_hover_text(tr!("Свойства: параметры запуска, язык, беты, файлы, DLC, облако", "Properties: launch options, language, betas, files, DLC, cloud"))
                            .clicked()
                        {
                            out = Some(Action::Props(r.appid, r.name.clone()));
                        }
                    }
                });
            });
            if !wide {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 22.0;
                    stats(ui);
                });
            }
            if c.launching && c.status_app == Some(r.appid) && !c.status.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if c.running != Some(r.appid) {
                        crate::spin::spinner(ui);
                    }
                    ui.label(egui::RichText::new(c.status).size(12.5).color(theme::WEAK));
                });
            }
        });

        // ----- Band of links -----
        let pad = 16.0;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            egui::Frame::none().fill(egui::Color32::from_rgb(38, 45, 54)).rounding(2.0).inner_margin(egui::Margin::symmetric(6.0, 0.0)).show(ui, |ui| {
                ui.set_width(ui.available_width() - pad - 12.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                    let mut link = |ui: &mut egui::Ui, text: &str, hint: &str, act: Action| {
                        if band_link(ui, text, hint).clicked() {
                            out = Some(act);
                        }
                    };
                    if steam_game {
                        link(ui, tr!("Страница в магазине", "Store Page"), "", Action::Store(r.appid));
                        link(ui, tr!("Достижения", "Achievements"), tr!("Все достижения игры, сравнение с другом", "All achievements of the game, comparison with a friend"), Action::Achievements(r.appid, r.name.clone()));
                        link(ui, tr!("Центр сообщества", "Community Hub"), tr!("Обсуждения и руководства игры", "Discussions and guides of the game"), Action::Community(r.appid, r.name.clone()));
                        link(ui, tr!("Мастерская", "Workshop"), tr!("Моды и предметы игры", "Mods and items of the game"), Action::Workshop(r.appid, r.name.clone()));
                        link(ui, tr!("Скриншоты", "Screenshots"), tr!("Ваши скриншоты этой игры: на ПК и в облаке Steam", "Your screenshots of this game: on the PC and in the Steam cloud"), Action::Shots(r.appid));
                    } else {
                        link(ui, tr!("Изменить или убрать...", "Edit or remove..."), tr!("Файл, название, параметры запуска", "File, name, launch options"), Action::Props(r.appid, r.name.clone()));
                    }
                    // The rest of the game actions - in one menu, so the band keeps the main pages only.
                    let more = ui.menu_button(egui::RichText::new(tr!("Еще...", "More...")).size(13.0).color(theme::TEXT2), |ui| {
                        ui.set_min_width(220.0);
                        let mut item = |ui: &mut egui::Ui, text: &str, hint: &str, act: Action| {
                            if ui.button(text).on_hover_text(hint).clicked() {
                                out = Some(act);
                                ui.close_menu();
                            }
                        };
                        if r.installed {
                            item(ui, tr!("Папка игры", "Browse Files"), tr!("Открыть папку игры в Проводнике", "Open the game folder in Explorer"), Action::Folder(r.appid));
                            item(ui, tr!("Ярлык на рабочем столе", "Desktop shortcut"), tr!("Ярлык на рабочем столе: запуск игры через Vaporust", "Desktop shortcut: launch the game through Vaporust"), Action::Shortcut(r.appid, r.name.clone()));
                        }
                        item(ui, tr!("Заметка", "Notes"), tr!("Своя заметка к игре (видна только вам)", "Your own note for the game (only you see it)"), Action::Note(r.appid, r.name.clone()));
                        if steam_game {
                            let support = d.support.map(str::to_string).unwrap_or_else(|| format!("https://help.steampowered.com/wizard/HelpWithGame/?appid={}", r.appid));
                            if ui.button(tr!("Поддержка", "Support")).on_hover_text(tr!("Поддержка игры: сайт разработчика или помощь Steam", "Game support: the developer's website or Steam Support")).clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(support));
                                ui.close_menu();
                            }
                            if let Some(t) = d.trailer {
                                if ui.button(tr!("Трейлер", "Trailer")).on_hover_text(tr!("Открыть трейлер игры в браузере", "Open the game trailer in the browser")).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(t));
                                    ui.close_menu();
                                }
                            }
                        }
                        if !r.installed && c.logged_in && steam_game {
                            item(
                                ui,
                                tr!("Уже скачана...", "Already downloaded..."),
                                tr!("Указать папку с уже скачанными файлами игры (например, скопированной с другого ПК)", "Point to a folder with already downloaded game files (for example copied from another PC)"),
                                Action::Import(r.appid, r.name.clone()),
                            );
                        }
                    });
                    more.response.on_hover_text(tr!("Папка, ярлык, заметка, поддержка, трейлер", "Folder, shortcut, notes, support, trailer"));
                });
            });
        });
        ui.add_space(14.0);

        // ----- Two columns -----
        let total = ui.available_width() - pad * 2.0;
        let two = total >= 720.0;
        let right_w = if two { (total * 0.36).clamp(260.0, 380.0) } else { total };
        let left_w = if two { total - right_w - 18.0 } else { total };
        ui.horizontal_top(|ui| {
            ui.add_space(pad);
            ui.vertical(|ui| {
                ui.set_width(left_w);
                ui.spacing_mut().item_spacing.y = 4.0;
                left_column(ui, r, c, d, &mut out);
                if !two {
                    ui.add_space(10.0);
                    right_column(ui, r, c, d, &mut out);
                }
            });
            if two {
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.set_width(right_w);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    right_column(ui, r, c, d, &mut out);
                });
            }
        });
        ui.add_space(16.0);
    });
    out
}

/// Left column of the game page: news (as Steam's activity), DLC, the purchase date.
fn left_column(ui: &mut egui::Ui, r: &Row, c: &Ctx, d: &PageData, out: &mut Option<Action>) {
    // DLC: how many are owned and which.
    if let Some(Ok(dlc)) = c.net.props.get(&r.appid).and_then(|p| p.dlc.as_ref()) {
        if !dlc.is_empty() {
            let owned = dlc.iter().filter(|d| d.owned).count();
            ui.horizontal(|ui| {
                theme::caps_title(ui, tr!("Дополнения", "DLC"), None);
                ui.label(egui::RichText::new(trf!("куплено {owned} из {}", "{owned} of {} owned", dlc.len())).size(12.0).color(theme::DIM));
                if r.installed
                    && ui
                        .add(egui::Button::new(egui::RichText::new(tr!("Управление", "Manage DLC")).size(12.0)).frame(false))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tr!("Включить, выключить или установить DLC - в «Свойствах»", "Enable, disable or install DLC in \"Properties\""))
                        .clicked()
                {
                    *out = Some(Action::Props(r.appid, r.name.clone()));
                }
            });
            ui.add_space(2.0);
            let mut sorted: Vec<_> = dlc.iter().filter_map(|d| c.net.app_names.get(&d.app_id).map(|n| (d, n.clone()))).collect();
            let unnamed = dlc.len() - sorted.len();
            sorted.sort_by_key(|(d, _)| !d.owned);
            egui::Frame::none().fill(theme::CARD).rounding(2.0).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (d, name) in sorted.iter().take(8) {
                    ui.horizontal(|ui| {
                        let (state, color) = match (d.owned, d.installed) {
                            (true, true) => (tr!("установлено", "installed"), theme::PLAY_TEXT),
                            (true, false) => (tr!("куплено", "owned"), theme::TEXT2),
                            _ => (tr!("не куплено", "not owned"), theme::DIM),
                        };
                        ui.label(egui::RichText::new(name).size(12.5).color(theme::TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(state).size(11.5).color(color));
                        });
                    });
                }
                let more = sorted.len().saturating_sub(8) + unnamed;
                if more > 0 {
                    ui.label(egui::RichText::new(trf!("и еще {more}", "and {more} more")).size(11.5).color(theme::DIM));
                }
            });
            ui.add_space(14.0);
        }
    }

    // News, like the activity feed of the Steam game page: date, then a card.
    theme::caps_title(ui, tr!("Новости", "News"), None);
    ui.add_space(4.0);
    match d.news {
        None if !c.logged_in => {
            ui.label(egui::RichText::new(tr!("Новости игры видны после входа в аккаунт.", "Game news is shown after signing in.")).color(theme::WEAK));
        }
        None => {
            crate::spin::spinner(ui);
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(trf!("Не удалось загрузить: {e}", "Could not load: {e}")).color(theme::WEAK));
        }
        Some(Ok(items)) if items.is_empty() => {
            ui.label(egui::RichText::new(tr!("Новостей нет.", "No news.")).color(theme::WEAK));
        }
        Some(Ok(items)) => {
            for n in items {
                ui.add(egui::Label::new(theme::spaced(&fmt_day(n.date).to_uppercase(), egui::FontId::proportional(11.0), theme::DIM, 0.6)).selectable(false));
                let card = egui::Frame::none().fill(theme::CARD).rounding(2.0).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(egui::RichText::new(&n.title).font(theme::semibold(14.0)).color(theme::TEXT)).wrap());
                    let text = crate::store::unglue(&crate::store::plain_text(&n.contents));
                    if !text.is_empty() {
                        let short: String = text.chars().take(260).collect();
                        ui.add(egui::Label::new(egui::RichText::new(short).size(12.0).color(theme::WEAK)).wrap());
                    }
                });
                let resp = ui.interact(card.response.rect, ui.id().with(("news", &n.gid)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.hovered() {
                    ui.painter().rect_stroke(card.response.rect, 2.0, egui::Stroke::new(1.0_f32, theme::BUTTON_HOVER));
                }
                if resp.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&n.url));
                }
                ui.add_space(10.0);
            }
        }
    }

    // Purchase date and a refund hint (Steam's terms: within 14 days and under 2 hours played).
    if let Some(lic) = c.net.licenses.as_ref().and_then(|l| crate::refund::find(&r.name, l)) {
        let when = chrono::DateTime::from_timestamp(lic.date, 0).map(|t| t.format("%d.%m.%Y").to_string()).unwrap_or_default();
        let how = match lic.method.as_str() {
            "Steam Store" => tr!("в магазине Steam", "in the Steam store"),
            "Complimentary" => tr!("бесплатно", "free"),
            "Gift/Guest Pass" => tr!("подарок", "gift"),
            "Retail" => tr!("ключом", "with a key"),
            other => other,
        };
        ui.label(egui::RichText::new(trf!("Получена {when} ({how})", "Acquired {when} ({how})")).size(12.0).color(theme::DIM));
        if let Some(h) = crate::refund::hint(lic, r.playtime_min.unwrap_or(0), chrono::Utc::now().timestamp()) {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), h);
                if ui.small_button(tr!("Оформить возврат", "Request a refund")).on_hover_text(tr!("Страница поддержки Steam для этой игры", "Steam Support page for this game")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://help.steampowered.com/wizard/HelpWithGame/?appid={}", r.appid)));
                }
            });
        }
    }
}

/// Right column of the game page: friends who play, achievements, information, collections, note.
fn right_column(ui: &mut egui::Ui, r: &Row, c: &Ctx, d: &PageData, out: &mut Option<Action>) {
    // Friends in this game: now (friend status) and in two weeks (if "Activity" was loaded).
    let playing: Vec<String> = c
        .net
        .roster
        .read()
        .unwrap()
        .values()
        .filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1) && f.game_app_id == Some(r.appid))
        // A name not known yet: never the account number (a SteamID is private).
        .map(|f| f.persona_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| tr!("загружается...", "loading...").to_string()))
        .collect();
    let unnamed = |n: &str| n.is_empty() || n.chars().all(|ch| ch.is_ascii_digit());
    let recent: Vec<String> = match &c.net.activity {
        Some(Ok(list)) => list
            .iter()
            .filter(|f| f.games.iter().any(|g| g.appid == r.appid))
            .filter(|f| !playing.contains(&f.name))
            .map(|f| if unnamed(&f.name) { tr!("загружается...", "loading...").to_string() } else { f.name.clone() })
            .collect(),
        _ => Vec::new(),
    };
    // Screenshots for the README (tour, private): other people's names are not shown.
    let private = std::env::var_os("VAPORUST_TOUR_PRIVATE").is_some();
    if !private && (!playing.is_empty() || !recent.is_empty()) {
        side_box(ui, tr!("Друзья, которые играют", "Friends who play"), |ui| {
            if !playing.is_empty() {
                ui.label(egui::RichText::new(trf!("Сейчас играют: {}", "Playing now: {}", playing.len())).size(12.0).color(theme::TEXT2));
                for n in &playing {
                    ui.label(egui::RichText::new(n).size(12.5).color(theme::IN_GAME));
                }
            }
            if !recent.is_empty() {
                if !playing.is_empty() {
                    ui.add_space(4.0);
                }
                ui.label(egui::RichText::new(trf!("Играли недавно: {}", "Played recently: {}", recent.len())).size(12.0).color(theme::TEXT2));
                ui.label(egui::RichText::new(recent.join(", ")).size(12.0).color(theme::ONLINE));
            }
        });
    }

    // Achievements: progress and the latest unlocked icons.
    if let Some(Ok(list)) = d.achievements.filter(|a| a.as_ref().is_ok_and(|l| !l.is_empty())) {
        let done = list.iter().filter(|a| a.unlocked).count();
        let pct = done as f32 / list.len() as f32;
        side_box(ui, tr!("Достижения", "Achievements"), |ui| {
            ui.label(egui::RichText::new(trf!("Получено {done}/{} ({:.0}%)", "You've unlocked {done}/{} ({:.0}%)", list.len(), pct * 100.0)).size(12.5).color(theme::TEXT));
            ui.add_space(4.0);
            let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 3.0, egui::Color32::from_rgb(14, 20, 27));
            let mut f = bar;
            f.set_width(bar.width() * pct);
            ui.painter().rect_filled(f, 3.0, theme::ACCENT);
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for a in list.iter().filter(|a| a.unlocked).take(10) {
                    ui.add(egui::Image::new(a.icon_unlocked.as_str()).fit_to_exact_size(egui::vec2(28.0, 28.0)).rounding(2.0)).on_hover_text(&a.name);
                }
            });
            ui.add_space(4.0);
            if ui.add(egui::Button::new(egui::RichText::new(tr!("Все достижения", "View all achievements")).size(12.0).color(theme::TEXT2)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *out = Some(Action::Achievements(r.appid, r.name.clone()));
            }
        });
    }

    // Information: size, players, developer, release, genres, access, status.
    side_box(ui, tr!("Информация", "Information"), |ui| {
        egui::Grid::new(("game_info", r.appid)).num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, k: &str, v: String| {
                ui.label(egui::RichText::new(k).size(12.0).color(theme::DIM));
                ui.add(egui::Label::new(egui::RichText::new(v).size(12.0).color(theme::TEXT2)).wrap());
                ui.end_row();
            };
            if r.size > 0 {
                row(ui, tr!("На диске", "On disk"), net::fmt_bytes(r.size));
            }
            if let Some(p) = d.players {
                row(ui, tr!("Сейчас играют", "Playing now"), p.to_string());
            }
            if let Some(i) = d.info {
                if !i.developers.is_empty() {
                    row(ui, tr!("Разработчик", "Developer"), i.developers.join(", "));
                }
                if !i.publishers.is_empty() && i.publishers != i.developers {
                    row(ui, tr!("Издатель", "Publisher"), i.publishers.join(", "));
                }
                if let Some(date) = i.release_date.as_deref().filter(|s| !s.is_empty()) {
                    row(ui, tr!("Дата выхода", "Release date"), crate::storewin::ru_date(date));
                }
            }
            if !d.genres.is_empty() {
                row(ui, tr!("Жанры", "Genres"), d.genres.join(", "));
            }
            if r.family {
                row(ui, tr!("Доступ", "Access"), family_text(r, c));
            }
            let status = if r.ready {
                if c.needs_update(r) { tr!("установлена, есть обновление", "installed, update available") } else { tr!("установлена", "installed") }
            } else if r.installed {
                tr!("недокачана", "not finished")
            } else {
                tr!("не установлена", "not installed")
            };
            row(ui, tr!("Состояние", "Status"), status.to_string());
        });
    });

    // Collections (without "Favorites": that is the heart in the play bar).
    let own: Vec<_> = c.collections.iter().filter(|k| k.id != "favorite").collect();
    if !own.is_empty() {
        side_box(ui, tr!("Коллекции", "Collections"), |ui| {
            for coll in own {
                let mut inside = coll.apps.contains(&r.appid);
                if ui.add_enabled(!coll.dynamic, egui::Checkbox::new(&mut inside, &coll.name)).changed() {
                    *out = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                }
            }
        });
    }

    if !d.note.trim().is_empty() {
        side_box(ui, tr!("Заметка", "Notes"), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(d.note).size(12.5).color(theme::TEXT2)).wrap());
            if ui.add(egui::Button::new(egui::RichText::new(tr!("Изменить", "Edit")).size(12.0).color(theme::WEAK)).frame(false)).clicked() {
                *out = Some(Action::Note(r.appid, r.name.clone()));
            }
        });
    }
}

/// "Family library (owner: name)" - the name is taken from the friend list if the owner is there.
fn family_text(r: &Row, c: &Ctx) -> String {
    let name = r.owner.and_then(|id| {
        let roster = c.net.roster.read().ok()?;
        roster.get(&id).and_then(|f| f.persona_name.clone())
    });
    match name {
        Some(n) => trf!("Семейная библиотека (владелец: {n})", "Family library (owner: {n})"),
        None => tr!("Семейная библиотека Steam", "Steam Family library").to_string(),
    }
}

/// For the screenshot tour: open the grid at row N (value N + 1, 0 - leave as is).
pub static TOUR_GRID_ROW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
