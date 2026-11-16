//! First-run wizard: a few pages with the main settings (the same ones can be changed later in
//! "Settings"). Shown once, after the first login or after "Continue without login"; "Skip" also
//! counts. Drawn as a card in the middle of the window, in the style of the login page.

use crate::{settings, theme, tray, LauncherApp};
use eframe::egui;

const STEPS: usize = 4;

/// A small picture drawn on a choice card.
type Pic = fn(&egui::Painter, egui::Rect, egui::Color32);

/// Grid picture: covers in two rows.
fn pic_grid(p: &egui::Painter, r: egui::Rect, c: egui::Color32) {
    let (w, h) = ((r.width() - 3.0 * 4.0) / 4.0, (r.height() - 4.0) / 2.0);
    for row in 0..2 {
        for col in 0..4 {
            let min = r.min + egui::vec2(col as f32 * (w + 4.0), row as f32 * (h + 4.0));
            p.rect_filled(egui::Rect::from_min_size(min, egui::vec2(w, h)), 2.0, c);
        }
    }
}

/// Table picture: rows with a small icon and a text line.
fn pic_table(p: &egui::Painter, r: egui::Rect, c: egui::Color32) {
    let h = (r.height() - 3.0 * 3.0) / 4.0;
    for row in 0..4 {
        let y = r.min.y + row as f32 * (h + 3.0);
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(r.min.x, y), egui::vec2(h, h)), 2.0, c);
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(r.min.x + h + 6.0, y + h * 0.3), egui::vec2(r.width() * 0.55, h * 0.4)), 2.0, c);
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(r.max.x - r.width() * 0.18, y + h * 0.3), egui::vec2(r.width() * 0.18, h * 0.4)), 2.0, c);
    }
}

/// A selectable card: an optional picture, a title and a short description; the chosen one has
/// a blue frame.
fn choice(ui: &mut egui::Ui, selected: bool, title: &str, desc: &str, w: f32, pic: Option<Pic>) -> egui::Response {
    let fill = if selected { egui::Color32::from_rgb(28, 50, 74) } else { theme::CARD };
    let stroke = if selected { egui::Stroke::new(1.5, theme::ACCENT) } else { egui::Stroke::new(1.5, fill) };
    let r = egui::Frame::none().fill(fill).stroke(stroke).rounding(6.0).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
        ui.set_width(w - 24.0);
        ui.vertical(|ui| {
            if let Some(pic) = pic {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w - 24.0, 54.0), egui::Sense::hover());
                let c = if selected { egui::Color32::from_rgb(60, 110, 160) } else { egui::Color32::from_rgb(62, 70, 82) };
                pic(ui.painter(), rect, c);
                ui.add_space(8.0);
            }
            ui.label(egui::RichText::new(title).strong().color(egui::Color32::WHITE));
            ui.label(egui::RichText::new(desc).size(12.0).color(theme::WEAK));
        });
    });
    r.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row "title + description" with an on/off switch on the right. Returns true if changed.
fn switch_row(ui: &mut egui::Ui, on: &mut bool, enabled: bool, title: &str, desc: &str) -> bool {
    let mut changed = false;
    egui::Frame::none().fill(theme::CARD).rounding(6.0).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let text_w = ui.available_width() - 56.0;
            ui.vertical(|ui| {
                ui.set_width(text_w);
                let c = if enabled { egui::Color32::WHITE } else { theme::WEAK };
                ui.label(egui::RichText::new(title).strong().color(c));
                if !desc.is_empty() {
                    ui.label(egui::RichText::new(desc).size(12.0).color(theme::WEAK));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 20.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
                let t = ui.ctx().animate_bool(resp.id, *on);
                let bg = if !enabled {
                    theme::SIDEBAR
                } else if *on {
                    theme::ACCENT
                } else {
                    egui::Color32::from_rgb(70, 78, 90)
                };
                ui.painter().rect_filled(rect, 10.0, bg);
                let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
                ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, egui::Color32::WHITE);
                if enabled && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    *on = !*on;
                    changed = true;
                }
            });
        });
    });
    ui.add_space(6.0);
    changed
}

/// One segment of a segmented control ("90%", "100%"...).
fn segment(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let fill = if selected { theme::ACCENT } else { theme::CARD };
    let color = if selected { egui::Color32::WHITE } else { theme::TEXT };
    ui.add(egui::Button::new(egui::RichText::new(text).color(color)).fill(fill).rounding(4.0).min_size(egui::vec2(64.0, 30.0)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn title(ui: &mut egui::Ui, text: &str, sub: &str) {
    ui.label(egui::RichText::new(text).size(22.0).strong().color(egui::Color32::WHITE));
    if !sub.is_empty() {
        ui.label(egui::RichText::new(sub).color(theme::WEAK));
    }
    ui.add_space(16.0);
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.5).strong().color(theme::ACCENT));
    ui.add_space(2.0);
}

impl LauncherApp {
    /// Whether the main window shows the wizard: once, after login or the no-login choice.
    pub(crate) fn wizard_active(&self, net: &crate::net::NetState) -> bool {
        !self.settings.wizard_done
            && !crate::notify::in_tray()
            && self.tour.is_none()
            && (net.account.is_some() || self.settings.local_mode)
    }

    /// Finish the wizard (also "Skip").
    fn wizard_finish(&mut self) {
        self.settings.wizard_done = true;
        if self.settings.start_view == 1 && self.view == crate::View::Library && !self.settings.local_mode {
            self.open_store(None, false);
        }
    }

    pub(crate) fn wizard_page(&mut self, ui: &mut egui::Ui) {
        let step = self.wizard_step.min(STEPS - 1);
        let avail = ui.available_size();
        let card_w = (avail.x - 24.0).clamp(300.0, 620.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(((avail.y - 470.0) / 2.0).max(16.0));
            theme::centered(ui, card_w, |ui| {
                egui::Frame::none().fill(theme::SIDEBAR).rounding(8.0).inner_margin(egui::Margin::same(28.0)).show(ui, |ui| {
                    let inner = card_w - 56.0;
                    ui.set_width(inner);
                    // Progress: four segments, the passed ones blue.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let seg_w = ((inner - 6.0 * (STEPS as f32 - 1.0) - 90.0) / STEPS as f32).max(20.0);
                        for i in 0..STEPS {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(seg_w, 4.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 2.0, if i <= step { theme::ACCENT } else { theme::CARD });
                        }
                        ui.label(egui::RichText::new(trf!("Шаг {} из {STEPS}", "Step {} of {STEPS}", step + 1)).size(12.0).color(theme::WEAK));
                    });
                    ui.add_space(18.0);
                    match step {
                        0 => self.wizard_welcome(ui),
                        1 => self.wizard_view(ui, inner),
                        2 => self.wizard_window(ui),
                        _ => self.wizard_steam(ui, inner),
                    }
                    ui.add_space(18.0);
                    // Footer: "Back" and "Skip" quiet on the left, the main button on the right.
                    ui.allocate_ui_with_layout(egui::vec2(inner, 36.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let quiet = |ui: &mut egui::Ui, t: &str| {
                            ui.add(egui::Button::new(egui::RichText::new(t).color(theme::WEAK)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand)
                        };
                        if step > 0 && quiet(ui, tr!("< Назад", "< Back")).clicked() {
                            self.wizard_step = step - 1;
                        }
                        if step + 1 < STEPS && quiet(ui, tr!("Пропустить", "Skip")).on_hover_text(tr!("Оставить настройки по умолчанию", "Keep the default settings")).clicked() {
                            self.wizard_finish();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let last = step + 1 >= STEPS;
                            let text = if last { tr!("Готово", "Done") } else { tr!("Далее", "Next") };
                            let b = egui::Button::new(egui::RichText::new(text).size(15.0).strong().color(egui::Color32::WHITE))
                                .fill(theme::ACCENT)
                                .rounding(4.0)
                                .min_size(egui::vec2(140.0, 36.0));
                            if ui.add(b).clicked() {
                                if last {
                                    self.wizard_finish();
                                } else {
                                    self.wizard_step = step + 1;
                                }
                            }
                        });
                    });
                });
            });
            ui.add_space(16.0);
        });
    }

    fn wizard_welcome(&mut self, ui: &mut egui::Ui) {
        title(ui, tr!("Добро пожаловать в Vaporust", "Welcome to Vaporust"), "");
        ui.label(
            egui::RichText::new(tr!(
                "Библиотека, магазин, друзья и чат без тяжелого интерфейса Steam. Игры запускаются через настоящий Steam: он поднимается незаметно и закрывается вместе с игрой.",
                "The library, store, friends and chat without the heavy Steam interface. Games are launched through the real Steam: it starts unnoticed and closes together with the game."
            ))
            .color(theme::TEXT),
        );
        ui.add_space(14.0);
        // The warning as a plate with an amber edge.
        let r = egui::Frame::none().fill(theme::CARD).rounding(6.0).inner_margin(egui::Margin { left: 16.0, right: 12.0, top: 10.0, bottom: 10.0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(tr!(
                    "Неофициальный проект, не связан с Valve. Программа меняет файлы Steam; используйте на свой страх и риск.",
                    "An unofficial project, not affiliated with Valve. The program changes Steam files; use it at your own risk."
                ))
                .size(12.5)
                .color(egui::Color32::from_rgb(235, 195, 100)),
            );
        });
        let e = r.response.rect;
        ui.painter().rect_filled(egui::Rect::from_min_max(e.min, egui::pos2(e.min.x + 3.0, e.max.y)), 2.0, egui::Color32::from_rgb(230, 170, 60));
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(tr!(
                "Дальше - три коротких шага с настройками. Все можно поменять потом: меню «Vaporust» -> «Настройки».",
                "Next - three short steps with settings. Everything can be changed later: the \"Vaporust\" menu -> \"Settings\"."
            ))
            .size(12.5)
            .color(theme::WEAK),
        );
    }

    fn wizard_view(&mut self, ui: &mut egui::Ui, inner: f32) {
        title(ui, tr!("Как показывать библиотеку", "How to show the library"), tr!("Переключается и потом, прямо над списком игр.", "Can be switched later too, right above the game list."));
        let half = (inner - 10.0) / 2.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if choice(ui, self.settings.view == settings::View::Grid, tr!("Плитка", "Grid"), tr!("Обложки игр, как в Steam", "Game covers, as in Steam"), half, Some(pic_grid)).clicked() {
                self.settings.view = settings::View::Grid;
            }
            if choice(ui, self.settings.view == settings::View::List, tr!("Таблица", "Table"), tr!("Компактный список, легче для слабых ПК", "A compact list, lighter for weak PCs"), half, Some(pic_table)).clicked() {
                self.settings.view = settings::View::List;
            }
        });
        ui.add_space(16.0);
        caption(ui, tr!("МАСШТАБ ИНТЕРФЕЙСА", "INTERFACE SCALE"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (v, l) in [(0.9f32, "90%"), (1.0, "100%"), (1.1, "110%"), (1.25, "125%"), (1.5, "150%")] {
                if segment(ui, (self.settings.ui_scale - v).abs() < 0.01, l).clicked() {
                    self.settings.ui_scale = v;
                }
            }
        });
        if !self.settings.local_mode {
            ui.add_space(16.0);
            caption(ui, tr!("ПРИ ЗАПУСКЕ ОТКРЫВАТЬ", "OPEN AT STARTUP"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if segment(ui, self.settings.start_view == 0, tr!("Библиотеку", "Library")).clicked() {
                    self.settings.start_view = 0;
                }
                if segment(ui, self.settings.start_view == 1, tr!("Магазин", "Store")).clicked() {
                    self.settings.start_view = 1;
                }
            });
        }
    }

    fn wizard_window(&mut self, ui: &mut egui::Ui) {
        title(ui, tr!("Окно и уведомления", "Window and notifications"), "");
        let mut tray_on = tray::close_to_tray();
        if switch_row(
            ui,
            &mut tray_on,
            true,
            tr!("Крестик сворачивает в трей", "The close button hides to the tray"),
            tr!("Чат, загрузки и уведомления продолжают работать; выйти - через значок у часов.", "Chat, downloads and notifications keep working; exit through the icon near the clock."),
        ) {
            tray::set_close_to_tray(tray_on);
        }
        switch_row(
            ui,
            &mut self.settings.notify_on,
            true,
            tr!("Всплывающие уведомления", "Pop-up notifications"),
            tr!("Сообщения, друзья в игре, скидки из списка желаемого, скачанные игры.", "Messages, friends in game, wishlist sales, downloaded games."),
        );
        let on = self.settings.notify_on;
        switch_row(ui, &mut self.settings.notify_sound, on, tr!("Со звуком", "With sound"), "");
    }

    fn wizard_steam(&mut self, ui: &mut egui::Ui, inner: f32) {
        title(ui, tr!("Steam и игры", "Steam and games"), "");
        switch_row(
            ui,
            &mut self.freeze_webhelper,
            true,
            tr!("Замораживать интерфейс Steam во время игры", "Freeze the Steam interface during games"),
            tr!("steamwebhelper не занимает память, пока вы играете. Оверлей Steam (Shift+Tab) в это время может не открыться.", "steamwebhelper takes no memory while you play. The Steam overlay (Shift+Tab) may not open at that time."),
        );
        if switch_row(
            ui,
            &mut self.settings.light_mode,
            true,
            tr!("Легкий режим", "Light mode"),
            tr!("Меньше фоновой работы и без анимаций - для старых и слабых ПК.", "Less background work and no animations - for old and weak PCs."),
        ) && self.settings.light_mode
        {
            self.settings.animations = false;
            self.settings.economy = true;
            self.settings.trim_memory = true;
        }
        if !self.settings.local_mode {
            ui.add_space(10.0);
            caption(ui, tr!("ОБНОВЛЕНИЕ ИГР", "GAME UPDATES"));
            let half = (inner - 10.0) / 2.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if choice(ui, !self.settings.update_via_steam, tr!("Без Steam", "Without Steam"), tr!("Быстрее, Steam не запускается", "Faster, Steam is not started"), half, None).clicked() {
                    self.settings.update_via_steam = false;
                }
                if choice(ui, self.settings.update_via_steam, tr!("Через Steam", "Via Steam"), tr!("Как в обычном клиенте", "As in the normal client"), half, None).clicked() {
                    self.settings.update_via_steam = true;
                }
            });
        }
    }
}
