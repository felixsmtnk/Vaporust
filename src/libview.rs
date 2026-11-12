//! Отрисовка библиотеки: компактный список (как раньше) или плитка с обложками.
//! Кнопки действий у обоих видов общие (`main_button`), поэтому код не дублируется.

use crate::covers::{self, Cover, Covers};
use crate::settings::Sort;
use crate::{fmt_day, fmt_hours, net, Row};
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
    /// Вернуться со страницы игры к списку.
    Back,
    /// Открыть страницу игры в магазине.
    Store(u32),
    /// Открыть Мастерскую игры (отдельное окно).
    Workshop(u32, String),
    /// Скриншоты этой игры.
    Shots(u32),
    /// Игра уже скачана — указать папку с файлами.
    Import(u32, String),
}

/// Состояние, нужное для кнопок (одинаковое для всех строк кадра).
pub struct Ctx<'a> {
    pub launching: bool,
    pub installing: bool,
    pub logged_in: bool,
    pub net: &'a net::NetState,
    /// Игры в очереди загрузок (не на паузе).
    pub queued: &'a std::collections::HashSet<u32>,
    pub sort: Sort,
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

/// Главная кнопка игры: «Играть» (+ «Обновить») или «Установить» / «Докачать» / «В очереди».
fn main_button(ui: &mut egui::Ui, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    if r.ready {
        let play = ui
            .add_enabled(!c.launching && !c.installing, egui::Button::new("Играть"))
            .on_disabled_hover_text("Недоступно, пока идёт запуск или установка");
        if play.clicked() {
            *out = Some(Action::Play(r.appid));
        }
        if c.needs_update(r)
            && ui
                .add_enabled(
                    !c.launching && !c.installing,
                    egui::Button::new(egui::RichText::new("Обновить").color(egui::Color32::from_rgb(230, 170, 60))),
                )
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
            "В очереди"
        } else if r.installed {
            "Докачать"
        } else {
            "Установить"
        };
        let btn = ui
            .add_enabled(c.logged_in && !c.launching && !queued, egui::Button::new(text))
            .on_hover_text(if c.installing {
                "Сейчас идёт другая загрузка — игра встанет в очередь"
            } else {
                "Скачать игру"
            })
            .on_disabled_hover_text("Нужен вход в аккаунт; игра уже в очереди или качается");
        if btn.clicked() {
            *out = Some(Action::Install(r.appid, r.name.clone()));
        }
    }
}

/// Компактный список — как в прошлых версиях.
pub fn list(ui: &mut egui::Ui, rows: &[Row], c: &Ctx) -> Option<Action> {
    let mut out = None;
    let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        for r in &rows[range] {
            ui.horizontal(|ui| {
                main_button(ui, r, c, &mut out);
                if r.installed
                    && ui.button("⚙").on_hover_text("Свойства игры: параметры запуска, язык, беты, файлы, DLC, облако").clicked()
                {
                    out = Some(Action::Props(r.appid, r.name.clone()));
                }
                if c.logged_in && ui.button("🏆").on_hover_text("Достижения").clicked() {
                    out = Some(Action::Achievements(r.appid, r.name.clone()));
                }
                if c.installing_now(r.appid) {
                    ui.strong(&r.name);
                    ui.spinner();
                } else {
                    let mut text = egui::RichText::new(&r.name);
                    if !r.installed {
                        text = text.color(ui.visuals().weak_text_color());
                    }
                    let resp = ui
                        .add(egui::Label::new(text).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Открыть страницу игры");
                    collections_menu(&resp, r, c, &mut out);
                    if resp.clicked() {
                        out = Some(Action::Page(r.appid));
                    }
                    if r.installed && !r.ready {
                        ui.weak("(недокачана)");
                    }
                    if r.family {
                        ui.weak("(семья)").on_hover_text(family_text(r, c));
                    }
                }
                if let Some(m) = r.playtime_min.filter(|m| *m > 0) {
                    ui.weak(fmt_hours(m));
                }
                match c.sort {
                    Sort::Size if r.size > 0 => {
                        ui.weak(net::fmt_bytes(r.size));
                    }
                    Sort::LastPlayed if r.last_played > 0 => {
                        ui.weak(format!("запуск: {}", fmt_day(r.last_played)));
                    }
                    _ => {}
                }
            });
        }
    });
    out
}

/// Плитка с обложками. Рисуются (и грузятся) только видимые ряды.
pub fn grid(ui: &mut egui::Ui, rows: &[Row], c: &Ctx, covers: &Covers) -> Option<Action> {
    let mut out = None;
    let tile = egui::vec2(covers::W as f32, covers::H as f32);
    let gap = 12.0;
    let cols = (((ui.available_width() - 16.0) + gap) / (tile.x + gap)).floor().max(1.0) as usize;
    let row_h = tile.y + 62.0;
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
            // Пока грузится или обложки нет — тёмная карточка с названием.
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
        *out = Some(Action::Page(r.appid));
    }
    // Название в одну строку (обрезается), под ним — кнопки.
    ui.add(egui::Label::new(egui::RichText::new(&r.name).small()).truncate());
    ui.horizontal(|ui| {
        main_button(ui, r, c, out);
        if r.installed && ui.small_button("⚙").on_hover_text("Свойства игры").clicked() {
            *out = Some(Action::Props(r.appid, r.name.clone()));
        }
    });
}

/// Меню по правому щелчку: в какие коллекции входит игра, и «Новая коллекция».
fn collections_menu(resp: &egui::Response, r: &Row, c: &Ctx, out: &mut Option<Action>) {
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new(&r.name).strong());
        ui.separator();
        if c.collections.is_empty() {
            ui.weak("Коллекции ещё загружаются…");
        }
        for coll in c.collections {
            let mut inside = coll.apps.contains(&r.appid);
            let resp = ui.add_enabled(!coll.dynamic, egui::Checkbox::new(&mut inside, &coll.name));
            if coll.dynamic {
                resp.on_disabled_hover_text("Динамическая коллекция (по фильтру) — меняется только в Steam");
            } else if resp.changed() {
                *out = Some(Action::Collection { id: coll.id.clone(), appid: r.appid, add: inside });
                ui.close_menu();
            }
        }
        ui.separator();
        // Поле для имени новой коллекции живёт во временной памяти egui.
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
) -> Option<Action> {
    let mut out = None;
    if ui.button("< Библиотека").clicked() {
        out = Some(Action::Back);
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Широкий фон игры (library_hero) с названием поверх.
        let w = ui.available_width();
        let h = (w / 3.1).clamp(120.0, 320.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        match covers.hero(r.appid) {
            Cover::Ready(tex) => {
                // Обрезаем по высоте, чтобы фон заполнил полосу без искажений.
                let s = tex.size_vec2();
                let vis = (s.x / w * h / s.y).min(1.0);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - vis) / 2.0), egui::pos2(1.0, (1.0 + vis) / 2.0));
                painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
            _ => {
                painter.rect_filled(rect, 4.0, egui::Color32::from_gray(35));
            }
        }
        let band = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 44.0), rect.max);
        painter.rect_filled(band, 0.0, egui::Color32::from_black_alpha(150));
        painter.text(
            band.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &r.name,
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            main_button(ui, r, c, &mut out);
            if r.installed && ui.button("Свойства").clicked() {
                out = Some(Action::Props(r.appid, r.name.clone()));
            }
            if !r.installed
                && c.logged_in
                && ui
                    .button("Уже скачана...")
                    .on_hover_text("Указать папку с уже скачанными файлами игры (например, скопированной с другого ПК)")
                    .clicked()
            {
                out = Some(Action::Import(r.appid, r.name.clone()));
            }
            if ui.button("Страница в магазине").clicked() {
                out = Some(Action::Store(r.appid));
            }
            if ui.button("Мастерская").clicked() {
                out = Some(Action::Workshop(r.appid, r.name.clone()));
            }
            if ui.button("Скриншоты").on_hover_text("Ваши скриншоты этой игры: на ПК и в облаке Steam").clicked() {
                out = Some(Action::Shots(r.appid));
            }
        });

        ui.add_space(8.0);
        egui::Grid::new("game_info").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            if let Some(m) = r.playtime_min.filter(|m| *m > 0) {
                ui.weak("В игре");
                ui.label(fmt_hours(m));
                ui.end_row();
            }
            if r.last_played > 0 {
                ui.weak("Последний запуск");
                ui.label(fmt_day(r.last_played));
                ui.end_row();
            }
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

        // Достижения: прогресс и кнопка полного списка.
        ui.add_space(8.0);
        match d.achievements {
            Some(Ok(list)) if !list.is_empty() => {
                let done = list.iter().filter(|a| a.unlocked).count();
                ui.horizontal(|ui| {
                    ui.strong("Достижения");
                    ui.add(
                        egui::ProgressBar::new(done as f32 / list.len() as f32)
                            .desired_width(220.0)
                            .text(format!("{done} из {}", list.len())),
                    );
                    if ui.button("Все достижения").clicked() {
                        out = Some(Action::Achievements(r.appid, r.name.clone()));
                    }
                });
            }
            Some(Ok(_)) => {
                ui.weak("У этой игры нет достижений.");
            }
            Some(Err(_)) => {}
            None if c.logged_in => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Загружаю достижения...");
                });
            }
            None => {}
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
                ui.spinner();
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
                    let text = crate::store::plain_text(&n.contents);
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

/// «Семейная библиотека (владелец: имя)» — имя берём из списка друзей, если владелец там есть.
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
