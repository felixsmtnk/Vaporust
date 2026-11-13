//! «Загрузки», как в Steam: кнопка посередине нижней полосы с ходом текущей загрузки и
//! страница с текущей загрузкой и очередью.

use crate::net::{self, NetState};
use crate::{schedule, settings, theme, LauncherApp, View};
use eframe::egui;
use std::sync::atomic::Ordering;

/// Картинка игры для загрузок (шапка магазина Steam).
fn header_url(appid: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg")
}

impl LauncherApp {
    /// Подпись кнопки «Загрузки»: идет загрузка - с процентом, есть очередь - с числом игр.
    pub(crate) fn downloads_label(&self, net: &NetState) -> String {
        if let Some(i) = net.install.as_ref().filter(|i| i.result.is_none()) {
            if i.total > 0 {
                let pct = (i.done as f64 / i.total as f64 * 100.0).clamp(0.0, 100.0);
                return format!("Загрузки: {} - {pct:.0}%", i.name);
            }
            return format!("Загрузки: {}", i.name);
        }
        match self.settings.downloads.len() {
            0 => "Загрузки".into(),
            n => format!("Загрузки ({n} в очереди)"),
        }
    }

    /// Кнопка посередине нижней полосы; во время загрузки под ней тонкая полоса прогресса.
    pub(crate) fn downloads_button(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let label = self.downloads_label(net);
        let on = self.view == View::Downloads;
        let text = egui::RichText::new(label).size(13.0).color(if on { egui::Color32::WHITE } else { theme::TEXT });
        let resp = ui.add(egui::Button::new(text).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand);
        if let Some(i) = net.install.as_ref().filter(|i| i.result.is_none() && i.total > 0) {
            let frac = (i.done as f32 / i.total as f32).clamp(0.0, 1.0);
            let r = resp.rect;
            let bar = egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - 1.0), egui::pos2(r.right(), r.bottom() + 1.0));
            ui.painter().rect_filled(bar, 0.0, egui::Color32::from_rgb(45, 52, 64));
            let mut done = bar;
            done.set_width(bar.width() * frac);
            ui.painter().rect_filled(done, 0.0, theme::ACCENT);
        }
        if resp.on_hover_text("Текущая загрузка и очередь").clicked() {
            self.close_extras(View::Downloads);
            self.view = View::Downloads;
        }
    }

    pub(crate) fn downloads_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Загрузки").size(20.0).strong());
            if !self.install_note.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), &self.install_note);
            }
            if !schedule::may_download(&self.settings) {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 170, 60),
                    format!(
                        "Сейчас загрузки ждут расписания (с {}). Расписание - в «Настройки» -> «Загрузки».",
                        schedule::hh(self.settings.sched_from)
                    ),
                );
            }
            ui.add_space(8.0);

            // Текущая загрузка.
            match &net.install {
                None => {
                    ui.weak("Сейчас ничего не загружается.");
                }
                Some(inst) => {
                    let is_install = inst.done_text == "установлена.";
                    egui::Frame::none().fill(theme::CARD).inner_margin(egui::Margin::same(12.0)).rounding(4.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if !crate::custom::is_custom(inst.appid) {
                                ui.add(egui::Image::new(header_url(inst.appid)).fit_to_exact_size(egui::vec2(184.0, 86.0)).rounding(3.0));
                            }
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&inst.name).size(16.0).strong());
                                match &inst.result {
                                    None => {
                                        ui.label(egui::RichText::new(&inst.stage).color(theme::WEAK));
                                    }
                                    Some(Ok(())) => {
                                        ui.colored_label(egui::Color32::from_rgb(110, 200, 110), &inst.done_text);
                                    }
                                    Some(Err(e)) => {
                                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                                    }
                                }
                                if inst.result.is_none() && inst.total > 0 {
                                    let frac = (inst.done as f32 / inst.total as f32).clamp(0.0, 1.0);
                                    let mut text = format!(
                                        "{} из {} ({:.1}%)",
                                        net::fmt_bytes(inst.done),
                                        net::fmt_bytes(inst.total),
                                        frac * 100.0
                                    );
                                    if inst.speed > 1.0 {
                                        let left = inst.total.saturating_sub(inst.done) as f64 / inst.speed;
                                        text += &format!(" · {}/с · осталось ~{}", net::fmt_bytes(inst.speed as u64), crate::fmt_eta(left as u64));
                                    }
                                    ui.add(egui::ProgressBar::new(frac).desired_width(ui.available_width().min(520.0)).text(text));
                                }
                                ui.horizontal(|ui| {
                                    if inst.result.is_none() {
                                        if is_install
                                            && ui
                                                .button("Пауза")
                                                .on_hover_text("Поставить на паузу только эту игру: скачанное сохранится. Следующие игры из очереди начнут качаться.")
                                                .clicked()
                                        {
                                            inst.abort.store(true, Ordering::Relaxed);
                                            self.settings.downloads.retain(|q| q.appid != inst.appid);
                                            self.settings.downloads.insert(
                                                0,
                                                settings::QueueItem { appid: inst.appid, name: inst.name.clone(), paused: true },
                                            );
                                        }
                                        if ui.button("Отмена").clicked() {
                                            inst.abort.store(true, Ordering::Relaxed);
                                        }
                                    } else if ui.button("Убрать").clicked() {
                                        self.net.lock().unwrap().install = None;
                                    }
                                });
                            });
                        });
                    });
                }
            }

            // Очередь.
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("ДАЛЕЕ ({})", self.settings.downloads.len())).size(12.0).color(theme::WEAK));
                if !self.settings.downloads.is_empty() {
                    let (label, hint) = if self.settings.queue_paused {
                        ("Продолжить очередь", "Снова качать игры из очереди по порядку.")
                    } else {
                        ("Пауза очереди", "Не начинать следующие игры из очереди (текущая загрузка продолжится).")
                    };
                    if ui.small_button(label).on_hover_text(hint).clicked() {
                        self.settings.queue_paused = !self.settings.queue_paused;
                    }
                }
            });
            if self.settings.downloads.is_empty() {
                ui.weak("Очередь пуста. Игры встают сюда, если нажать «Установить», пока качается другая.");
            }
            let mut remove = None;
            let mut up = None;
            let n = self.settings.downloads.len();
            for (i, q) in self.settings.downloads.iter_mut().enumerate() {
                egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(10.0, 6.0)).rounding(3.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(header_url(q.appid)).fit_to_exact_size(egui::vec2(92.0, 43.0)).rounding(2.0));
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&q.name).size(14.0));
                            ui.label(egui::RichText::new(if q.paused { "на паузе" } else { "в очереди" }).size(12.0).color(theme::WEAK));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Убрать").on_hover_text("Убрать из очереди").clicked() {
                                remove = Some(i);
                            }
                            if q.paused {
                                if ui.small_button("Продолжить").clicked() {
                                    q.paused = false;
                                }
                            } else if ui.small_button("Пауза").on_hover_text("Пропускать эту игру, пока не продолжите").clicked() {
                                q.paused = true;
                            }
                            if i > 0 && n > 1 && ui.small_button("Выше").on_hover_text("Скачать раньше").clicked() {
                                up = Some(i);
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
            if let Some(i) = remove {
                self.settings.downloads.remove(i);
            }
            if let Some(i) = up {
                self.settings.downloads.swap(i, i - 1);
            }
        });
    }
}
