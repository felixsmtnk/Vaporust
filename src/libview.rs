//! Отрисовка библиотеки: компактный список (как раньше) или плитка с обложками.
//! Кнопки действий у обоих видов общие (`main_button`), поэтому код не дублируется.

use crate::covers::{self, Cover, Covers};
use crate::settings::Sort;
use crate::{fmt_day, fmt_hours, net, theme, Row};
use eframe::egui;

/// Что пользователь нажал у игры.
pub enum Action {
    Play(u32),
    Update(u32),
    Install(u32, String),
    Props(u32, String),
    Achievements(u32, String),
    /// Открыть страницу игры (в библиотеке).
    Page(u32),
    /// Добавить (true) или убрать игру из коллекции.
    Collection { id: String, appid: u32, add: bool },
    /// Создать коллекцию с этой игрой.
    NewCollection { name: String, appid: u32 },
    /// Открыть страницу игры в магазине.
    Store(u32),
    /// Открыть Мастерскую игры (отдельное окно).
    Workshop(u32, String),
    /// Обсуждения и руководства игры.
    Community(u32, String),
    /// Скриншоты этой игры.
    Shots(u32),
    /// Игра уже скачана - указать папку с файлами.
    Import(u32, String),
    /// Открыть папку игры в Проводнике.
    Folder(u32),
}

/// Состояние, нужное для кнопок (одинаковое для всех строк кадра).
pub struct Ctx<'a> {
    pub launching: bool,
    pub installing: bool,
    pub logged_in: bool,
    pub net: &'a net::NetState,
    /// Игры в очереди загрузок (не на паузе).
    pub queued: &'a std::collections::HashSet<u32>,
    /// Коллекции Steam (для меню по правому щелчку).
    pub collections: &'a [crate::colls::Coll],
    /// Обновлять через Steam (для подсказки кнопки «Обновить»).
    pub update_via_steam: bool,
}

impl Ctx<'_> {
    fn needs_update(&self, r: &Row) -> bool {
        r.needs_update || self.net.updates.as_ref().is_some_and(|u| u.contains(&r.appid))
    }
    fn installing_now(&self, appid: u32) -> bool {
        self.net.install.as_ref().is_some_and(|i| i.active() && i.appid == appid)
    }
}

/// Компактный вид: таблица игр (значок, название, состояние, часы, последний запуск, размер).
/// Щелчок по заголовку - сортировка, щелчок по строке - страница игры, двойной - «Играть»,
/// правый - меню игры. Кнопок в строках нет: все действия на странице игры и в меню.
pub fn list(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, sort: &mut Sort, icons: bool) -> Option<Action> {
    let mut out = None;
    let w = ui.available_width() - 14.0;
    let (state_w, hours_w, last_w, size_w) = (130.0, 80.0, 110.0, 90.0);
    let name_w = (w - state_w - hours_w - last_w - size_w).max(120.0);
    let xs = [0.0, name_w, name_w + state_w, name_w + state_w + hours_w, name_w + state_w + hours_w + last_w];
    let small = egui::FontId::proportional(12.5);

    // Заголовок: щелчок по столбцу с сортировкой - сортировать по нему.
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
        ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, *label, egui::FontId::proportional(11.0), color);
        if let Some(s) = s {
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *sort = *s;
            }
        }
    }
    ui.painter().hline(head.x_range(), head.bottom(), egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 57, 69)));

    let row_h = 28.0;
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
            let (state, color) = if c.installing_now(r.appid) {
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
            } else if resp.clicked() {
                out = Some(Action::Page(r.appid));
            }
        }
    });
    out
}

/// Плитка с обложками. Рисуются (и грузятся) только видимые ряды.
pub fn grid(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers) -> Option<Action> {
    let mut out = None;
    // Сколько обложек шириной не меньше 140 точек помещается, и растянуть их на всю ширину -
    // справа не остается пустого места при любом размере окна.
    let gap = 12.0;
    let avail = ui.available_width() - 16.0;
    let min_w = covers::W as f32;
    let cols = ((avail + gap) / (min_w + gap)).floor().max(1.0) as usize;
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).clamp(min_w, 260.0);
    let tile = egui::vec2(w, w * 1.5);
    let row_h = tile.y + gap;
    let n_rows = rows.len().div_ceil(cols);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, n_rows, |ui, range| {
        for row in range {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for r in rows.iter().skip(row * cols).take(cols) {
                    ui.allocate_ui(egui::vec2(tile.x, row_h), |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(tile.x);
                            tile_ui(ui, r, c, covers, tile, &mut out);
                        });
                    });
                }
            });
        }
    });
    out
}

fn tile_ui(ui: &mut egui::Ui, r: &Row, c: &Ctx, covers: &Covers, tile: egui::Vec2, out: &mut Option<Action>) {
    let (rect, resp) = ui.allocate_exact_size(tile, egui::Sense::click());
    let painter = ui.painter_at(rect);
    match covers.get(r.appid) {
        Cover::Ready(tex) => {
            let tint = if r.installed { egui::Color32::WHITE } else { egui::Color32::from_gray(110) };
            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        cover => {
            // Пока грузится или обложки нет - темная карточка с названием.
            painter.rect_filled(rect, 4.0, egui::Color32::from_gray(40));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if matches!(cover, Cover::Loading) { "…" } else { r.name.as_str() },
                egui::FontId::proportional(13.0),
                egui::Color32::from_gray(180),
            );
        }
    }
    if c.installing_now(r.appid) {
        painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "загрузка…", egui::FontId::proportional(14.0), egui::Color32::WHITE);
    }
    collections_menu(&resp, r, c, out);
    if resp.hovered() {
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(110, 160, 230)));
    }
    // Как в Steam: под обложкой ничего нет; при наведении - зеленая кнопка «Играть» в углу
    // (у установленных), щелчок по ней запускает игру, по остальной обложке - страница игры.
    let hover = resp.hovered();
    let mut play_hit = false;
    if hover && r.ready && !c.launching && !c.installing {
        let b = egui::Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.bottom() - 42.0), egui::vec2(34.0, 34.0));
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| b.contains(p));
        painter.rect_filled(b, 4.0, if over { egui::Color32::from_rgb(112, 190, 38) } else { theme::PLAY });
        let (cx, cy) = (b.center().x + 2.0, b.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![egui::pos2(cx - 6.0, cy - 8.0), egui::pos2(cx + 8.0, cy), egui::pos2(cx - 6.0, cy + 8.0)],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
        play_hit = over;
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
        *out = Some(if play_hit { Action::Play(r.appid) } else { Action::Page(r.appid) });
    }
}

/// Меню по правому щелчку: в какие коллекции входит игра, и «Новая коллекция».
fn collections_menu(resp: &egui::Response, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        ui.label(egui::RichText::new(&r.name).strong());
        ui.separator();
        let mut item = |ui: &mut egui::Ui, enabled: bool, text: &str, act: Action| {
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                *out = Some(act);
                ui.close_menu();
            }
        };
        if r.installed {
            item(ui, r.ready && !c.launching, "Играть", Action::Play(r.appid));
        } else if c.logged_in {
            item(ui, !c.installing_now(r.appid), "Установить", Action::Install(r.appid, r.name.clone()));
        }
        item(ui, true, "Страница игры", Action::Page(r.appid));
        if r.installed {
            item(ui, true, "Свойства...", Action::Props(r.appid, r.name.clone()));
            item(ui, true, "Открыть папку игры", Action::Folder(r.appid));
        }
        if crate::custom::is_custom(r.appid) {
            return;
        }
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
                    *out = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                    ui.close_menu();
                }
            }
            ui.separator();
            // Поле для имени новой коллекции живет во временной памяти egui.
            let id = egui::Id::new("new_collection_name");
            let mut name = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut name).hint_text("Новая коллекция").desired_width(140.0));
                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Создать")).clicked() {
                    *out = Some(Action::NewCollection { name: name.trim().to_string(), appid: r.appid });
                    name.clear();
                    ui.close_menu();
                }
            });
            ui.data_mut(|d| d.insert_temp(id, name));
        });
    });
}


/// Список игр в левой колонке, как в Steam: значок и название; щелчок - страница игры,
/// двойной щелчок - «Играть», правый щелчок - меню игры.
pub fn sidebar(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers, selected: Option<u32>, icons: bool) -> Option<Action> {
    let mut out = None;
    let row_h = 26.0;
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
            let (suffix, color) = if c.installing_now(r.appid) {
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
            } else if resp.clicked() {
                out = Some(Action::Page(r.appid));
            }
        }
    });
    out
}

/// Большая кнопка на странице игры: «ИГРАТЬ» (зеленая) или «УСТАНОВИТЬ» (голубая).
fn big_main_button(ui: &mut egui::Ui, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    if r.ready {
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
    } else {
        let queued = c.queued.contains(&r.appid) || c.installing_now(r.appid);
        let text = if queued {
            "В ОЧЕРЕДИ"
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

/// Столбик на полосе под фоном: подпись мелко сверху, значение под ней.
fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(egui::RichText::new(label).size(10.5).color(theme::WEAK));
        ui.label(egui::RichText::new(value).size(13.5).color(theme::TEXT));
    });
}

/// Данные для страницы игры, которые приходят из сети.
pub struct PageData<'a> {
    pub achievements: Option<&'a Result<Vec<aurelia::steam_client::GameAchievement>, String>>,
    pub news: Option<&'a Result<Vec<aurelia::web::discovery::NewsItem>, String>>,
    pub players: Option<i32>,
}

/// Страница игры в библиотеке: фон, кнопки, время, достижения, коллекции, новости.
pub fn page(
    ui: &mut egui::Ui,
    r: &Row,
    c: &Ctx,
    covers: &Covers,
    d: &PageData,
    covers_on: bool,
) -> Option<Action> {
    let mut out = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Широкий фон игры (library_hero) с названием поверх; без обложек - только полоса с названием.
        let w = ui.available_width();
        let h = if covers_on { (w / 3.1).clamp(140.0, 320.0) } else { 64.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        match covers_on.then(|| covers.hero(r.appid)) {
            Some(Cover::Ready(tex)) => {
                // Обрезаем по высоте, чтобы фон заполнил полосу без искажений.
                let s = tex.size_vec2();
                let vis = (s.x / w * h / s.y).min(1.0);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - vis) / 2.0), egui::pos2(1.0, (1.0 + vis) / 2.0));
                painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
            _ => {
                painter.rect_filled(rect, 0.0, theme::CARD);
            }
        }
        // Затемнение снизу, чтобы название читалось на любом фоне.
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

        // Полоса действий, как в Steam: большая кнопка, время, последний запуск, достижения, шестеренка.
        egui::Frame::none().fill(theme::CARD).inner_margin(egui::Margin::symmetric(14.0, 10.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Показатели: в узком окне - второй строкой, чтобы не обрезались.
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
        });

        // Ссылки, как вкладки под полосой в Steam.
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

        // Дата покупки и подсказка о возврате (условия Steam: до 14 дней и до 2 часов в игре).
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

        // Друзья в этой игре: сейчас (статус друга) и за две недели (если «Активность» загружалась).
        let playing: Vec<String> = c
            .net
            .roster
            .read()
            .unwrap()
            .values()
            .filter(|f| f.relationship == 3 && f.game_app_id == Some(r.appid))
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

        // Коллекции.
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

        // Новости.
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

/// «Семейная библиотека (владелец: имя)» - имя берем из списка друзей, если владелец там есть.
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
