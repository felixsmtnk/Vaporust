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
        ("НАЗВАНИЕ", Some(Sort::Name)),
        ("СОСТОЯНИЕ", None),
        ("В ИГРЕ", Some(Sort::Playtime)),
        ("ПОСЛЕДНИЙ ЗАПУСК", Some(Sort::LastPlayed)),
        ("НА ДИСКЕ", Some(Sort::Size)),
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
    ui.painter().hline(head.x_range(), head.bottom(), egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 57, 69)));

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
                x = ir.right() + 7.0;
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
                name.push_str("  (семья)");
            }
            cell(&p, x, rect.left() + xs[1] - x - 8.0, name, egui::FontId::proportional(13.5), name_color);
            let (state, color) = if c.running == Some(r.appid) {
                ("в игре", theme::PLAY_TEXT)
            } else if c.installing_now(r.appid) {
                ("загружается", theme::ACCENT)
            } else if c.queued.contains(&r.appid) {
                ("в очереди", theme::ACCENT)
            } else if r.ready && c.needs_update(r) {
                ("есть обновление", egui::Color32::from_rgb(230, 170, 60))
            } else if r.ready {
                ("установлена", egui::Color32::from_rgb(120, 190, 80))
            } else if r.installed {
                ("недокачана", theme::WEAK)
            } else {
                ("не установлена", theme::WEAK)
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
                out = Some(Action::Page(r.appid));
            }
        }
    });
    out
}

/// Grid of covers. Only visible rows are drawn (and loaded).
/// `recent` - the "Recent games" shelf as the first row (as on Steam's library home); empty - no shelf.
pub fn grid(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, recent: &[Row]) -> Option<Action> {
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
    let shelf_h = if recent.is_empty() { 0.0 } else { label_h + row_h + label_h };
    let n_rows = rows.len().div_ceil(cols);
    // The screenshot tour (tour.rs) can ask to open the grid at row N (stored as N + 1;
    // a large number - to the very bottom, egui clamps scrolling to the end of the list).
    let tour_row = TOUR_GRID_ROW.swap(0, std::sync::atomic::Ordering::Relaxed);
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("lib_grid");
    if tour_row > 0 {
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
        if shelf_h > 0.0 && vp.min.y < shelf_h {
            let mut lab = row_ui(ui, 0.0, label_h);
            lab.label(egui::RichText::new("НЕДАВНИЕ ИГРЫ").size(12.0).strong().color(theme::WEAK));
            let mut child = row_ui(ui, label_h, row_h);
            child.spacing_mut().item_spacing.x = gap;
            for r in recent.iter().take(cols) {
                tile_ui(&mut child, r, c, covers, tile, 0, &mut out);
            }
            // Border between the shelf and all games - a thin line (the game count is already in the header).
            let y = origin.y + label_h + row_h + label_h / 2.0;
            ui.painter().hline(origin.x..=origin.x + width - 16.0, y, egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 57, 69)));
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
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "загрузка…", egui::FontId::proportional(14.0), egui::Color32::WHITE);
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
            let st = egui::Stroke::new(2.5, egui::Color32::WHITE);
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
        let mut item = |ui: &mut egui::Ui, enabled: bool, text: &str, act: Action| {
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                *pick.borrow_mut() = Some(act);
                ui.close_menu();
            }
        };
        let custom = crate::custom::is_custom(r.appid);
        if r.installed {
            item(ui, r.ready && !c.launching, "Играть", Action::Play(r.appid));
        } else if c.logged_in && !custom {
            item(ui, !c.installing_now(r.appid), "Установить", Action::Install(r.appid, r.name.clone()));
        }
        item(ui, true, "Страница игры", Action::Page(r.appid));
        if !custom {
            ui.separator();
            item(ui, c.logged_in, "Достижения", Action::Achievements(r.appid, r.name.clone()));
            item(ui, true, "Скриншоты", Action::Shots(r.appid));
            item(ui, c.logged_in, "Сообщество", Action::Community(r.appid, r.name.clone()));
            item(ui, true, "Страница в магазине", Action::Store(r.appid));
            ui.separator();
            ui.menu_button("Коллекции", |ui| {
                if c.collections.is_empty() {
                    ui.weak("Коллекции еще загружаются...");
                }
                for coll in c.collections {
                    let mut inside = coll.apps.contains(&r.appid);
                    let resp = ui.add_enabled(!coll.dynamic, egui::Checkbox::new(&mut inside, &coll.name));
                    if coll.dynamic {
                        resp.on_disabled_hover_text("Динамическая коллекция (по фильтру) - меняется только в Steam");
                    } else if resp.changed() {
                        *pick.borrow_mut() = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                        ui.close_menu();
                    }
                }
                ui.separator();
                // The new collection name field lives in egui's temporary memory.
                let id = egui::Id::new("new_collection_name");
                let mut name = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut name).hint_text("Новая коллекция").desired_width(140.0));
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Создать")).clicked() {
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
            ui.menu_button("Управление", |ui| {
                ui.set_min_width(180.0);
                if r.installed && ui.button("Открыть папку игры").clicked() {
                    *pick.borrow_mut() = Some(Action::Folder(r.appid));
                    ui.close_menu();
                }
                let (text, enabled) = if custom {
                    ("Убрать из библиотеки...", c.running != Some(r.appid))
                } else {
                    ("Удалить...", !c.installing_now(r.appid) && c.running != Some(r.appid))
                };
                if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                    *pick.borrow_mut() = Some(Action::Uninstall(r.appid, r.name.clone()));
                    ui.close_menu();
                }
            });
            item(ui, true, "Свойства...", Action::Props(r.appid, r.name.clone()));
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
    let row_h = 26.0;
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
                p.rect_filled(rect, 2.0, egui::Color32::from_rgb(62, 98, 140));
            } else if resp.hovered() {
                p.rect_filled(rect, 2.0, egui::Color32::from_rgb(48, 55, 67));
            }
            let mut x = rect.left() + 8.0;
            if icons {
                let ir = egui::Rect::from_min_size(egui::pos2(rect.left() + 4.0, rect.top() + 3.0), egui::vec2(20.0, 20.0));
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
                (" - в игре", theme::PLAY_TEXT)
            } else if c.installing_now(r.appid) {
                (" - загрузка", theme::ACCENT)
            } else if r.ready && c.needs_update(r) {
                (" - обновление", theme::ACCENT)
            } else if r.installed && !r.ready {
                (" - недокачана", theme::WEAK)
            } else if sel {
                ("", egui::Color32::WHITE)
            } else if r.installed {
                ("", theme::TEXT)
            } else {
                ("", theme::WEAK)
            };
            let name = format!("{}{suffix}", r.name);
            let galley = ui.fonts(|f| {
                let mut job = egui::text::LayoutJob::simple_singleline(name, egui::FontId::proportional(13.5), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - x - 4.0);
                f.layout_job(job)
            });
            p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
            collections_menu(&resp, r, c, &mut out);
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
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

/// Big button on the game page: "PLAY" (green) or "INSTALL" (light blue).
fn big_main_button(ui: &mut egui::Ui, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    if c.running == Some(r.appid) {
        // As in Steam: while the game runs, "STOP" instead of "PLAY".
        let b = theme::big_button(ui, "ОСТАНОВИТЬ", theme::ACCENT, true)
            .on_hover_text("Просит игру закрыться, как крестик окна или Alt+F4. Игра может спросить о сохранении.");
        if b.clicked() {
            *out = Some(Action::Stop(r.appid));
        }
    } else if r.ready && c.launching && c.status_app == Some(r.appid) {
        // The game is launching: Steam is starting, the game has not appeared yet.
        theme::big_button(ui, "ЗАПУСК...", theme::PLAY, false);
    } else if r.ready {
        let ok = !c.launching && !c.installing;
        let b = theme::big_button(ui, "ИГРАТЬ", theme::PLAY, ok).on_disabled_hover_text("Недоступно, пока идет запуск или установка");
        if b.clicked() {
            *out = Some(Action::Play(r.appid));
        }
        if c.needs_update(r)
            && theme::big_button(ui, "ОБНОВИТЬ", egui::Color32::from_rgb(196, 130, 30), ok)
                .on_hover_text(if c.update_via_steam {
                    "Есть обновление. Steam откроется незаметно, скачает его и закроется."
                } else {
                    "Есть обновление. Vaporust скачает его сам, Steam не запускается."
                })
                .clicked()
        {
            *out = Some(Action::Update(r.appid));
        }
    } else if c.installing_now(r.appid) {
        // The game is downloading - as in Steam, the button pauses it.
        let b = theme::big_button(ui, "ПАУЗА", theme::ACCENT, true)
            .on_hover_text("Поставить загрузку на паузу: скачанное сохранится, продолжить - кнопкой «ДОКАЧАТЬ»");
        if b.clicked() {
            *out = Some(Action::Pause(r.appid));
        }
    } else {
        let queued = c.queued.contains(&r.appid);
        let text = if queued {
            "В ОЧЕРЕДИ"
        } else if c.paused.contains(&r.appid) {
            "ПРОДОЛЖИТЬ"
        } else if r.installed {
            "ДОКАЧАТЬ"
        } else {
            "УСТАНОВИТЬ"
        };
        let b = theme::big_button(ui, text, theme::ACCENT, c.logged_in && !c.launching && !queued)
            .on_hover_text(if c.installing { "Сейчас идет другая загрузка - игра встанет в очередь" } else { "Скачать игру" })
            .on_disabled_hover_text("Нужен вход в аккаунт; игра уже в очереди или качается");
        if b.clicked() {
            *out = Some(Action::Install(r.appid, r.name.clone()));
        }
    }
}

/// Stat column on the bar under the background: small caption on top, value below.
fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(egui::RichText::new(label).size(10.5).color(theme::WEAK));
        ui.label(egui::RichText::new(value).size(13.5).color(theme::TEXT));
    });
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

/// Game page in the library: background, buttons, playtime, achievements, collections, news.
pub fn page(
    ui: &mut egui::Ui,
    r: &Row,
    c: &Ctx,
    covers: &Covers,
    d: &PageData,
    covers_on: bool,
) -> Option<Action> {
    let mut out = None;
    // Each game has its own scroll: a new game's page opens at the top.
    egui::ScrollArea::vertical().id_salt(("game_page", r.appid)).auto_shrink([false, false]).show(ui, |ui| {
        // Wide game background (library_hero) with the name on top; without covers - only a bar with the name.
        let w = ui.available_width();
        let h = if covers_on { (w / 3.1).clamp(140.0, 320.0) } else { 64.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        match covers_on.then(|| covers.hero(r.appid)) {
            Some(Cover::Ready(tex)) => {
                // Crop by height so the background fills the bar without distortion.
                let s = tex.size_vec2();
                let vis = (s.x / w * h / s.y).min(1.0);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - vis) / 2.0), egui::pos2(1.0, (1.0 + vis) / 2.0));
                painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
            _ => {
                painter.rect_filled(rect, 0.0, theme::CARD);
            }
        }
        // Darkening at the bottom so the name is readable on any background.
        let band_h = 56.0f32.min(h);
        let mut mesh = egui::Mesh::default();
        let band = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - band_h), rect.max);
        let (top, bottom) = (egui::Color32::from_black_alpha(0), egui::Color32::from_black_alpha(190));
        mesh.colored_vertex(band.left_top(), top);
        mesh.colored_vertex(band.right_top(), top);
        mesh.colored_vertex(band.right_bottom(), bottom);
        mesh.colored_vertex(band.left_bottom(), bottom);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(mesh);
        painter.text(
            egui::pos2(rect.left() + 16.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_BOTTOM,
            &r.name,
            egui::FontId::proportional(26.0),
            egui::Color32::WHITE,
        );

        // Action bar, as in Steam: big button, playtime, last played, achievements, gear.
        egui::Frame::none().fill(theme::CARD).inner_margin(egui::Margin::symmetric(14.0, 10.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Stats: in a narrow window - on a second line so they are not cut off.
            let stats = |ui: &mut egui::Ui| {
                if r.last_played > 0 {
                    stat(ui, "ПОСЛЕДНИЙ ЗАПУСК", fmt_day(r.last_played));
                }
                if let Some(m) = r.playtime_min.filter(|m| *m > 0) {
                    stat(ui, "ВЫ ИГРАЛИ", fmt_hours(m));
                }
                if let Some(Ok(list)) = d.achievements.filter(|a| a.as_ref().is_ok_and(|l| !l.is_empty())) {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    stat(ui, "ДОСТИЖЕНИЯ", format!("{done} из {}", list.len()));
                }
            };
            let two_buttons = r.ready && c.needs_update(r);
            let wide = ui.available_width() >= if two_buttons { 820.0 } else { 660.0 };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                big_main_button(ui, r, c, &mut out);
                if wide {
                    stats(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if r.installed
                        && ui
                            .add(egui::Button::new(egui::RichText::new("⚙").size(18.0)).min_size(egui::vec2(36.0, 36.0)))
                            .on_hover_text("Свойства: параметры запуска, язык, беты, файлы, DLC, облако")
                            .clicked()
                    {
                        out = Some(Action::Props(r.appid, r.name.clone()));
                    }
                });
            });
            if !wide {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 22.0;
                    stats(ui);
                });
            }
            // What is happening with this game now (starting Steam, waiting for the game, closing).
            if c.launching && c.status_app == Some(r.appid) && !c.status.is_empty() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if c.running != Some(r.appid) {
                        crate::spin::spinner(ui);
                    }
                    ui.label(egui::RichText::new(c.status).size(12.5).color(theme::WEAK));
                });
            }
        });

        // Links, like the tabs under the bar in Steam.
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let mut link = |ui: &mut egui::Ui, text: &str, hint: &str, act: Action| {
                let resp = ui
                    .add(egui::Button::new(egui::RichText::new(text).size(13.5)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let resp = if hint.is_empty() { resp } else { resp.on_hover_text(hint) };
                if resp.clicked() {
                    out = Some(act);
                }
            };
            let steam_game = !crate::custom::is_custom(r.appid);
            if steam_game {
                link(ui, "Страница в магазине", "", Action::Store(r.appid));
                link(ui, "Достижения", "Все достижения игры, сравнение с другом", Action::Achievements(r.appid, r.name.clone()));
                link(ui, "Сообщество", "Обсуждения и руководства игры", Action::Community(r.appid, r.name.clone()));
                link(ui, "Мастерская", "Моды и предметы игры", Action::Workshop(r.appid, r.name.clone()));
                link(ui, "Скриншоты", "Ваши скриншоты этой игры: на ПК и в облаке Steam", Action::Shots(r.appid));
            } else {
                link(ui, "Изменить или убрать...", "Файл, название, параметры запуска", Action::Props(r.appid, r.name.clone()));
            }
            if r.installed {
                link(ui, "Папка игры", "Открыть папку игры в Проводнике", Action::Folder(r.appid));
                link(ui, "Ярлык", "Ярлык на рабочем столе: запуск игры через Vaporust", Action::Shortcut(r.appid, r.name.clone()));
            }
            link(ui, "Заметка", "Своя заметка к игре (видна только вам)", Action::Note(r.appid, r.name.clone()));
            if steam_game {
                let support = d.support.map(str::to_string).unwrap_or_else(|| {
                    format!("https://help.steampowered.com/wizard/HelpWithGame/?appid={}", r.appid)
                });
                if ui
                    .add(egui::Button::new(egui::RichText::new("Поддержка").size(13.5)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Поддержка игры: сайт разработчика или помощь Steam")
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(support));
                }
                if let Some(t) = d.trailer {
                    if ui
                        .add(egui::Button::new(egui::RichText::new("Трейлер").size(13.5)).frame(false))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Открыть трейлер игры в браузере")
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(t));
                    }
                }
            }
            if !r.installed && c.logged_in && steam_game {
                link(
                    ui,
                    "Уже скачана...",
                    "Указать папку с уже скачанными файлами игры (например, скопированной с другого ПК)",
                    Action::Import(r.appid, r.name.clone()),
                );
            }
        });
        ui.separator();
        ui.add_space(8.0);
        egui::Grid::new("game_info").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            if r.size > 0 {
                ui.weak("На диске");
                ui.label(net::fmt_bytes(r.size));
                ui.end_row();
            }
            if let Some(p) = d.players {
                ui.weak("Сейчас играют");
                ui.label(p.to_string());
                ui.end_row();
            }
            if let Some(i) = d.info {
                if !i.developers.is_empty() {
                    ui.weak("Разработчик");
                    ui.label(i.developers.join(", "));
                    ui.end_row();
                }
                if !i.publishers.is_empty() && i.publishers != i.developers {
                    ui.weak("Издатель");
                    ui.label(i.publishers.join(", "));
                    ui.end_row();
                }
                if let Some(date) = i.release_date.as_deref().filter(|s| !s.is_empty()) {
                    ui.weak("Дата выхода");
                    ui.label(crate::storewin::ru_date(date));
                    ui.end_row();
                }
            }
            if !d.genres.is_empty() {
                ui.weak("Жанры");
                ui.label(d.genres.join(", "));
                ui.end_row();
            }
            if r.family {
                ui.weak("Доступ");
                ui.label(family_text(r, c));
                ui.end_row();
            }
            ui.weak("Состояние");
            ui.label(if r.ready {
                if c.needs_update(r) { "установлена, есть обновление" } else { "установлена" }
            } else if r.installed {
                "недокачана"
            } else {
                "не установлена"
            });
            ui.end_row();
        });

        if !d.note.trim().is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Заметка:").color(theme::WEAK));
                ui.label(d.note);
            });
        }

        // DLC: how many are owned and which.
        if let Some(Ok(dlc)) = c.net.props.get(&r.appid).and_then(|p| p.dlc.as_ref()) {
            if !dlc.is_empty() {
                let owned = dlc.iter().filter(|d| d.owned).count();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.strong(format!("Дополнения (DLC): куплено {owned} из {}", dlc.len()));
                    if r.installed
                        && ui
                            .add(egui::Button::new(egui::RichText::new("Управление").size(12.5)).frame(false))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Включить, выключить или установить DLC - в «Свойствах»")
                            .clicked()
                    {
                        out = Some(Action::Props(r.appid, r.name.clone()));
                    }
                });
                // Only DLC with a name: an id instead of a name ("DLC 271840") looks like
                // a bug. Unnamed ones are usually hidden in the store; we only count them.
                let mut sorted: Vec<_> = dlc.iter().filter_map(|d| c.net.app_names.get(&d.app_id).map(|n| (d, n.clone()))).collect();
                let unnamed = dlc.len() - sorted.len();
                sorted.sort_by_key(|(d, _)| !d.owned);
                for (d, name) in sorted.iter().take(8) {
                    ui.horizontal(|ui| {
                        let (state, color) = match (d.owned, d.installed) {
                            (true, true) => ("установлено", egui::Color32::from_rgb(120, 190, 80)),
                            (true, false) => ("куплено", theme::TEXT),
                            _ => ("не куплено", theme::WEAK),
                        };
                        ui.label(egui::RichText::new(name).size(13.0));
                        ui.label(egui::RichText::new(state).size(12.0).color(color));
                    });
                }
                let more = sorted.len().saturating_sub(8);
                match (more, unnamed) {
                    (0, 0) => {}
                    (m, 0) => {
                        ui.weak(format!("и еще {m}"));
                    }
                    (0, u) => {
                        ui.weak(format!("и еще {u} без названия (скрыты в магазине или еще загружаются)"));
                    }
                    (m, u) => {
                        ui.weak(format!("и еще {} (из них {u} без названия)", m + u));
                    }
                }
            }
        }

        // Purchase date and a refund hint (Steam's terms: within 14 days and under 2 hours played).
        if let Some(lic) = c.net.licenses.as_ref().and_then(|l| crate::refund::find(&r.name, l)) {
            let when = chrono::DateTime::from_timestamp(lic.date, 0).map(|t| t.format("%d.%m.%Y").to_string()).unwrap_or_default();
            let how = match lic.method.as_str() {
                "Steam Store" => "в магазине Steam",
                "Complimentary" => "бесплатно",
                "Gift/Guest Pass" => "подарок",
                "Retail" => "ключом",
                other => other,
            };
            ui.weak(format!("Получена {when} ({how})"));
            if let Some(h) = crate::refund::hint(lic, r.playtime_min.unwrap_or(0), chrono::Utc::now().timestamp()) {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(230, 170, 60), h);
                    if ui.small_button("Оформить возврат").on_hover_text("Страница поддержки Steam для этой игры").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                            "https://help.steampowered.com/wizard/HelpWithGame/?appid={}",
                            r.appid
                        )));
                    }
                });
            }
        }

        // Friends in this game: now (friend status) and in two weeks (if "Activity" was loaded).
        let playing: Vec<String> = c
            .net
            .roster
            .read()
            .unwrap()
            .values()
            .filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1) && f.game_app_id == Some(r.appid))
            .map(|f| f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string()))
            .collect();
        if !playing.is_empty() {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(140, 200, 80), format!("Сейчас играют друзья: {}", playing.join(", ")));
        }
        if let Some(Ok(list)) = &c.net.activity {
            let recent: Vec<String> = list
                .iter()
                .filter(|f| f.games.iter().any(|g| g.appid == r.appid))
                .filter(|f| !playing.contains(&f.name))
                .map(|f| f.name.clone())
                .collect();
            if !recent.is_empty() {
                ui.weak(format!("Играли за две недели: {}", recent.join(", ")));
            }
        }

        // Collections.
        if !c.collections.is_empty() {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong("Коллекции");
                for coll in c.collections {
                    let mut inside = coll.apps.contains(&r.appid);
                    if ui.add_enabled(!coll.dynamic, egui::Checkbox::new(&mut inside, &coll.name)).changed() {
                        out = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                    }
                }
            });
        }

        // News.
        ui.add_space(8.0);
        ui.strong("Новости");
        match d.news {
            None => {
                crate::spin::spinner(ui);
            }
            Some(Err(e)) => {
                ui.weak(format!("Не удалось загрузить: {e}"));
            }
            Some(Ok(items)) if items.is_empty() => {
                ui.weak("Новостей нет.");
            }
            Some(Ok(items)) => {
                for n in items {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(fmt_day(n.date));
                        ui.hyperlink_to(&n.title, &n.url);
                    });
                    let text = crate::store::unglue(&crate::store::plain_text(&n.contents));
                    if !text.is_empty() {
                        ui.weak(text.chars().take(300).collect::<String>());
                    }
                    ui.add_space(4.0);
                }
            }
        }
    });
    out
}

/// "Family library (owner: name)" - the name is taken from the friend list if the owner is there.
fn family_text(r: &Row, c: &Ctx) -> String {
    let name = r.owner.and_then(|id| {
        let roster = c.net.roster.read().ok()?;
        roster.get(&id).and_then(|f| f.persona_name.clone())
    });
    match name {
        Some(n) => format!("Семейная библиотека (владелец: {n})"),
        None => "Семейная библиотека Steam".to_string(),
    }
}

/// For the screenshot tour: open the grid at row N (value N + 1, 0 - leave as is).
pub static TOUR_GRID_ROW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
