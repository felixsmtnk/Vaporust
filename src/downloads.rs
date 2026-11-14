//! «Загрузки», как в Steam: кнопка посередине нижней полосы с ходом текущей загрузки и
//! страница: сверху текущая загрузка с графиком скорости, ниже «Далее» (очередь) и
//! «Завершено» (что скачалось или прервалось за этот сеанс).

use crate::net::{self, NetState};
use crate::{local, schedule, settings, theme, LauncherApp, View};
use eframe::egui;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// Картинка игры для загрузок (шапка магазина Steam).
fn header_url(appid: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg")
}

/// Сколько секунд истории скорости показывать на графике.
const GRAPH_SECS: usize = 120;

/// История скорости текущей загрузки (раз в секунду).
struct Graph {
    /// Какая загрузка (адрес ее флага отмены - уникален для каждой операции).
    op: usize,
    samples: VecDeque<f32>,
    peak: f32,
    last: Option<std::time::Instant>,
}

static GRAPH: Mutex<Graph> = Mutex::new(Graph { op: 0, samples: VecDeque::new(), peak: 0.0, last: None });

/// Завершенная за сеанс загрузка (или другая операция с файлами игры).
#[derive(Clone)]
struct Done {
    appid: u32,
    name: String,
    /// Ok - текст успеха; Err - ошибка или отмена.
    result: Result<String, String>,
    /// Это установка (а не проверка, перенос, удаление).
    install: bool,
    when: String,
}

/// (последняя записанная операция, список «Завершено» - новые сверху)
static DONE: Mutex<(usize, Vec<Done>)> = Mutex::new((0, Vec::new()));

fn op_id(i: &net::Install) -> usize {
    std::sync::Arc::as_ptr(&i.abort) as usize
}

/// Раз в секунду записать скорость; закончившуюся операцию - в «Завершено».
fn sample(net: &NetState, queue: &[settings::QueueItem]) {
    let Some(i) = net.install.as_ref() else { return };
    let id = op_id(i);
    if i.result.is_none() {
        let mut g = GRAPH.lock().unwrap();
        if g.op != id {
            *g = Graph { op: id, samples: VecDeque::new(), peak: 0.0, last: None };
        }
        if g.last.map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(1)) {
            g.last = Some(std::time::Instant::now());
            let s = i.speed.max(0.0) as f32;
            g.peak = g.peak.max(s);
            g.samples.push_back(s);
            while g.samples.len() > GRAPH_SECS {
                g.samples.pop_front();
            }
        }
        return;
    }
    let mut done = DONE.lock().unwrap();
    if done.0 == id {
        return;
    }
    done.0 = id;
    let cancelled = i.abort.load(Ordering::Relaxed);
    // Остановленные (пауза, отмена) не «завершены»: они в очереди или в «Не докачаны».
    let _ = queue;
    if cancelled {
        return;
    }
    let result = match &i.result {
        Some(Ok(())) => Ok(i.done_text.clone()),
        Some(Err(e)) => Err(e.clone()),
        None => return,
    };
    // Итог операции с файлами (удаление, проверка, перенос) - еще и всплывающим уведомлением:
    // о скачанной игре сообщает notices.rs.
    if i.done_text != "установлена." {
        // В названии операции - вид действия («Удаление: игра»): в уведомление - только игра.
        let name = ["Смена языка: ", "Обновление: ", "Проверка файлов: ", "Перенос: ", "Добавление: ", "Откат версии: ", "Удаление: "]
            .iter()
            .find_map(|p| i.name.strip_prefix(p))
            .unwrap_or(&i.name);
        let body = match &result {
            Ok(t) => format!("«{name}» {t}"),
            Err(e) => format!("«{name}»: {e}"),
        };
        crate::popup::push(crate::popup::Kind::Game(i.appid), "Vaporust", body);
    }
    done.1.retain(|d| d.appid != i.appid);
    done.1.insert(
        0,
        Done {
            appid: i.appid,
            name: i.name.clone(),
            result,
            install: i.done_text == "установлена.",
            when: chrono::Local::now().format("%H:%M").to_string(),
        },
    );
}

/// Заголовок раздела, как в Steam: мелко, заглавными, с линией до края.
fn section(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text).size(12.5).strong().color(theme::WEAK));
        add(ui);
        let r = ui.available_rect_before_wrap();
        let y = r.center().y;
        ui.painter().hline(r.left() + 6.0..=r.right(), y, egui::Stroke::new(1.0, egui::Color32::from_rgb(52, 60, 74)));
    });
    ui.add_space(6.0);
}

/// Полоса прогресса Steam: тонкая, без текста.
fn thin_bar(ui: &mut egui::Ui, frac: f32, width: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, egui::Color32::from_rgb(45, 52, 64));
    let mut d = r;
    d.set_width((r.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { 6.0 } else { 0.0 }));
    ui.painter().rect_filled(d, 3.0, theme::ACCENT);
}

/// Столбик показателя справа от графика.
fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(egui::RichText::new(label).size(10.5).color(theme::WEAK));
        ui.label(egui::RichText::new(value).size(14.0).color(theme::TEXT));
    });
}

/// График скорости за последние 2 минуты.
fn graph(ui: &mut egui::Ui, height: f32) {
    let g = GRAPH.lock().unwrap();
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 2.0, egui::Color32::from_rgb(20, 24, 31));
    // Сетка: 4 горизонтальные линии.
    for k in 1..4 {
        let y = r.top() + r.height() * k as f32 / 4.0;
        p.hline(r.x_range(), y, egui::Stroke::new(1.0, egui::Color32::from_rgb(34, 40, 50)));
    }
    let n = g.samples.len();
    if n < 2 {
        return;
    }
    let max = g.peak.max(1.0) * 1.15;
    let step = r.width() / (GRAPH_SECS - 1) as f32;
    let x0 = r.right() - step * (n - 1) as f32;
    let pts: Vec<egui::Pos2> = g
        .samples
        .iter()
        .enumerate()
        .map(|(i, s)| egui::pos2(x0 + step * i as f32, r.bottom() - r.height() * (s / max)))
        .collect();
    // Заливка под линией - вертикальными полосками (выпуклых многоугольников тут нет).
    let fill = egui::Color32::from_rgba_unmultiplied(26, 159, 255, 40);
    let mut mesh = egui::Mesh::default();
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        for v in [w[0], w[1], egui::pos2(w[1].x, r.bottom()), egui::pos2(w[0].x, r.bottom())] {
            mesh.colored_vertex(v, fill);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, theme::ACCENT)));
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
        sample(net, &self.settings.downloads);
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
        sample(net, &self.settings.downloads);
        let steam_open = self.steam_running();
        let ctx = ui.ctx().clone();
        let mut acts: Vec<Act> = Vec::new();
        let active_id = net.install.as_ref().filter(|i| i.result.is_none()).map(|i| i.appid);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
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

            // Сверху - текущая загрузка с графиком, как в Steam.
            let active = net.install.as_ref().filter(|i| i.result.is_none());
            let library = active.and_then(|i| {
                self.games
                    .iter()
                    .find(|g| g.appid == i.appid)
                    .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
                    .or_else(|| self.libraries.get(self.lib_idx).cloned())
            });
            // Меню по правому щелчку - под карточкой (место с прошлого кадра), чтобы кнопки
            // поверх него нажимались.
            let card_id = ui.id().with("dl_card");
            let bg = active.and_then(|_| menu_area(ui, card_id));
            let card = egui::Frame::none().fill(theme::CARD).inner_margin(egui::Margin::same(14.0)).rounding(4.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let Some(inst) = active else {
                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Сейчас ничего не загружается").size(16.0).color(theme::WEAK));
                        ui.label(
                            egui::RichText::new("Нажмите «Установить» на странице игры - загрузка появится здесь.")
                                .size(12.0)
                                .color(theme::WEAK),
                        );
                    });
                    ui.add_space(18.0);
                    return;
                };
                let is_install = inst.done_text == "установлена.";
                let frac = if inst.total > 0 { (inst.done as f32 / inst.total as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.horizontal(|ui| {
                    if !crate::custom::is_custom(inst.appid) {
                        ui.add(egui::Image::new(header_url(inst.appid)).fit_to_exact_size(egui::vec2(230.0, 107.0)).rounding(3.0));
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&inst.name).size(19.0).strong());
                        let what = if is_install { "ЗАГРУЗКА" } else { "РАБОТА С ФАЙЛАМИ" };
                        let head = if inst.total > 0 { format!("{what} - {:.0}%", frac * 100.0) } else { what.to_string() };
                        ui.label(egui::RichText::new(head).size(12.0).strong().color(theme::ACCENT));
                        ui.label(egui::RichText::new(&inst.stage).size(12.0).color(theme::WEAK));
                        ui.add_space(4.0);
                        thin_bar(ui, frac, (ui.available_width() - 190.0).max(160.0));
                        if inst.total > 0 {
                            let mut t = format!("{} из {}", net::fmt_bytes(inst.done), net::fmt_bytes(inst.total));
                            if inst.speed > 1.0 {
                                let left = inst.total.saturating_sub(inst.done) as f64 / inst.speed;
                                t += &format!("  ·  осталось ~{}", crate::fmt_eta(left as u64));
                            }
                            ui.label(egui::RichText::new(t).size(12.0).color(theme::TEXT));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Отмена").on_hover_text("Остановить загрузку. Скачанное сохранится, ее можно продолжить.").clicked() {
                            acts.push(Act::Stop(false));
                        }
                        if is_install
                            && ui
                                .button("Пауза")
                                .on_hover_text("Поставить на паузу только эту игру: скачанное сохранится. Следующие игры из очереди начнут качаться.")
                                .clicked()
                        {
                            acts.push(Act::Stop(true));
                        }
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let stats_w = 150.0;
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - stats_w - 12.0).max(120.0));
                        graph(ui, 104.0);
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        let peak = GRAPH.lock().unwrap().peak;
                        stat(ui, "ТЕКУЩАЯ СКОРОСТЬ", format!("{}/с", net::fmt_bytes(inst.speed.max(0.0) as u64)));
                        stat(ui, "ПИКОВАЯ", format!("{}/с", net::fmt_bytes(peak as u64)));
                        if inst.total > 0 {
                            stat(ui, "ОСТАЛОСЬ СКАЧАТЬ", net::fmt_bytes(inst.total.saturating_sub(inst.done)));
                        }
                        if let Some(free) = library.as_deref().and_then(local::free_space) {
                            stat(ui, "СВОБОДНО НА ДИСКЕ", net::fmt_bytes(free));
                        }
                    });
                });
            });
            remember_area(ui, card_id, card.response.rect);
            if let (Some(bg), Some(inst)) = (bg, active) {
                let (id, name) = (inst.appid, inst.name.clone());
                bg.context_menu(|ui| {
                    menu_common(ui, id, &name, true, &mut acts);
                    ui.separator();
                    if ui.button("Пауза").clicked() {
                        acts.push(Act::Stop(true));
                        ui.close_menu();
                    }
                    if ui.button("Отмена").clicked() {
                        acts.push(Act::Stop(false));
                        ui.close_menu();
                    }
                });
            }

            // Далее: очередь.
            let n = self.settings.downloads.len();
            let queue_paused = self.settings.queue_paused;
            section(ui, &format!("ДАЛЕЕ ({n})"), |ui| {
                if n > 0 {
                    let (label, hint) = if queue_paused {
                        ("Продолжить очередь", "Снова качать игры из очереди по порядку.")
                    } else {
                        ("Пауза очереди", "Не начинать следующие игры из очереди (текущая загрузка продолжится).")
                    };
                    if ui.small_button(label).on_hover_text(hint).clicked() {
                        acts.push(Act::ToggleQueue);
                    }
                }
            });
            if n == 0 {
                ui.weak("Очередь пуста. Игры встают сюда, если нажать «Установить», пока качается другая.");
            }
            for (i, q) in self.settings.downloads.iter().enumerate() {
                let partial = self.games.iter().find(|g| g.appid == q.appid && !g.ready);
                let mut state = if q.paused {
                    "На паузе".to_string()
                } else if steam_open {
                    "Ждет закрытия Steam".to_string()
                } else {
                    "В очереди".to_string()
                };
                if let Some(g) = partial {
                    state += &format!(" · скачано {}", net::fmt_bytes(partial_size(g)));
                }
                let paused = q.paused;
                row(
                    ui,
                    ("q", q.appid),
                    q.appid,
                    &q.name,
                    &state,
                    theme::WEAK,
                    &mut acts,
                    |ui, acts| {
                        if ui.small_button("Убрать").on_hover_text("Убрать из очереди (скачанное останется на диске)").clicked() {
                            acts.push(Act::Unqueue(i));
                        }
                        if paused {
                            if ui.small_button("Продолжить").clicked() {
                                acts.push(Act::SetPaused(i, false));
                            }
                        } else if ui.small_button("Пауза").on_hover_text("Пропускать эту игру, пока не продолжите").clicked() {
                            acts.push(Act::SetPaused(i, true));
                        }
                        if i > 0 && n > 1 && ui.small_button("Выше").on_hover_text("Скачать раньше").clicked() {
                            acts.push(Act::Move(i, i - 1));
                        }
                    },
                    |ui, acts| {
                        menu_common(ui, q.appid, &q.name, partial.is_some(), acts);
                        ui.separator();
                        if i > 0 && ui.button("Скачать первой").clicked() {
                            acts.push(Act::Move(i, 0));
                            ui.close_menu();
                        }
                        if i + 1 < n && ui.button("Ниже").clicked() {
                            acts.push(Act::Move(i, i + 1));
                            ui.close_menu();
                        }
                        if ui.button(if paused { "Продолжить" } else { "Пауза" }).clicked() {
                            acts.push(Act::SetPaused(i, !paused));
                            ui.close_menu();
                        }
                        if ui.button("Убрать из очереди").clicked() {
                            acts.push(Act::Unqueue(i));
                            ui.close_menu();
                        }
                        if partial.is_some() && ui.button("Удалить скачанное...").clicked() {
                            acts.push(Act::Uninstall(q.appid, q.name.clone()));
                            ui.close_menu();
                        }
                    },
                );
            }

            // Не докачаны: начатые загрузки вне очереди (остановленные, убранные из очереди).
            let queued: std::collections::HashSet<u32> = self.settings.downloads.iter().map(|q| q.appid).collect();
            let partials: Vec<&local::Game> = self
                .games
                .iter()
                .filter(|g| !g.ready && !queued.contains(&g.appid) && Some(g.appid) != active_id)
                .collect();
            if !partials.is_empty() {
                section(ui, &format!("НЕ ДОКАЧАНЫ ({})", partials.len()), |_| {});
                for g in partials {
                    let state = format!("Скачано {} · файлы лежат на диске", net::fmt_bytes(partial_size(g)));
                    row(
                        ui,
                        ("p", g.appid),
                        g.appid,
                        &g.name,
                        &state,
                        theme::WEAK,
                        &mut acts,
                        |ui, acts| {
                            if ui.small_button("Удалить").on_hover_text("Удалить скачанные файлы").clicked() {
                                acts.push(Act::Uninstall(g.appid, g.name.clone()));
                            }
                            if ui.small_button("Продолжить").on_hover_text("Докачать с того места, где остановилась").clicked() {
                                acts.push(Act::Resume(g.appid, g.name.clone()));
                            }
                        },
                        |ui, acts| {
                            menu_common(ui, g.appid, &g.name, true, acts);
                            ui.separator();
                            if ui.button("Продолжить загрузку").clicked() {
                                acts.push(Act::Resume(g.appid, g.name.clone()));
                                ui.close_menu();
                            }
                            if ui.button("Удалить скачанное...").clicked() {
                                acts.push(Act::Uninstall(g.appid, g.name.clone()));
                                ui.close_menu();
                            }
                        },
                    );
                }
            }

            // Завершено за этот сеанс.
            let done = DONE.lock().unwrap().1.clone();
            if !done.is_empty() {
                section(ui, &format!("ЗАВЕРШЕНО ({})", done.len()), |ui| {
                    if ui.small_button("Очистить").clicked() {
                        acts.push(Act::ClearDone);
                    }
                });
                let installed: std::collections::HashSet<u32> =
                    self.games.iter().filter(|g| g.ready).map(|g| g.appid).collect();
                for d in &done {
                    let (text, color) = match &d.result {
                        Ok(t) => (
                            format!("{} в {}: {}", if d.install { "Загрузка завершена" } else { "Готово" }, d.when, t),
                            egui::Color32::from_rgb(110, 200, 110),
                        ),
                        Err(e) => (format!("Ошибка в {}: {e}", d.when), egui::Color32::from_rgb(230, 110, 110)),
                    };
                    let can_play = d.result.is_ok() && d.install && installed.contains(&d.appid);
                    let on_disk = installed.contains(&d.appid);
                    row(
                        ui,
                        ("d", d.appid),
                        d.appid,
                        &d.name,
                        &text,
                        color,
                        &mut acts,
                        |ui, acts| {
                            if ui.small_button("Убрать").on_hover_text("Убрать из этого списка").clicked() {
                                acts.push(Act::Forget(d.appid));
                            }
                            if can_play
                                && ui
                                    .add(egui::Button::new(egui::RichText::new("Играть").color(egui::Color32::WHITE)).fill(theme::PLAY))
                                    .clicked()
                            {
                                acts.push(Act::Play(d.appid));
                            }
                        },
                        |ui, acts| {
                            if can_play && ui.button("Играть").clicked() {
                                acts.push(Act::Play(d.appid));
                                ui.close_menu();
                            }
                            menu_common(ui, d.appid, &d.name, on_disk, acts);
                            ui.separator();
                            if ui.button("Убрать из списка").clicked() {
                                acts.push(Act::Forget(d.appid));
                                ui.close_menu();
                            }
                        },
                    );
                }
            }
        });

        for a in acts {
            match a {
                Act::Stop(pause) => {
                    if let Some(inst) = net.install.as_ref().filter(|i| i.result.is_none()) {
                        inst.abort.store(true, Ordering::Relaxed);
                        if pause {
                            self.settings.downloads.retain(|q| q.appid != inst.appid);
                            self.settings
                                .downloads
                                .insert(0, settings::QueueItem { appid: inst.appid, name: inst.name.clone(), paused: true });
                        }
                    }
                }
                Act::ToggleQueue => self.settings.queue_paused = !self.settings.queue_paused,
                Act::SetPaused(i, p) => {
                    if let Some(q) = self.settings.downloads.get_mut(i) {
                        q.paused = p;
                    }
                }
                Act::Move(from, to) => {
                    if from < self.settings.downloads.len() && to < self.settings.downloads.len() {
                        let q = self.settings.downloads.remove(from);
                        self.settings.downloads.insert(to, q);
                    }
                }
                Act::Unqueue(i) => {
                    if i < self.settings.downloads.len() {
                        self.settings.downloads.remove(i);
                    }
                }
                Act::Resume(appid, name) => {
                    let snap = net.clone();
                    self.enqueue(appid, name, &snap);
                }
                Act::Uninstall(appid, name) => self.ask_uninstall(appid, name),
                Act::Page(appid) => {
                    self.close_extras(View::Library);
                    self.view = View::Library;
                    self.lib_page = Some(appid);
                }
                Act::Folder(appid) => {
                    if let Some(g) = self.games.iter().find(|g| g.appid == appid) {
                        local::open_folder(&g.dir);
                    }
                }
                Act::Play(appid) => self.start(appid, &ctx),
                Act::Forget(appid) => DONE.lock().unwrap().1.retain(|d| d.appid != appid),
                Act::ClearDone => DONE.lock().unwrap().1.clear(),
            }
        }
    }
}

/// Что нажали на странице загрузок (выполняется после отрисовки).
enum Act {
    /// Остановить текущую загрузку: true - на паузу (в начало очереди), false - отмена.
    Stop(bool),
    ToggleQueue,
    SetPaused(usize, bool),
    Move(usize, usize),
    Unqueue(usize),
    Resume(u32, String),
    Uninstall(u32, String),
    Page(u32),
    Folder(u32),
    Play(u32),
    Forget(u32),
    ClearDone,
}

/// Общие пункты меню строки: страница игры и папка.
fn menu_common(ui: &mut egui::Ui, appid: u32, name: &str, on_disk: bool, acts: &mut Vec<Act>) {
    ui.set_min_width(200.0);
    ui.label(egui::RichText::new(name).strong());
    ui.separator();
    if ui.button("Страница игры").clicked() {
        acts.push(Act::Page(appid));
        ui.close_menu();
    }
    if on_disk && ui.button("Открыть папку игры").clicked() {
        acts.push(Act::Folder(appid));
        ui.close_menu();
    }
}

/// Сколько уже скачано у недокачанной игры (размер папки; пересчет не чаще раза в 10 с).
fn partial_size(g: &local::Game) -> u64 {
    static CACHE: Mutex<Vec<(u32, u64, std::time::Instant)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap();
    if let Some(e) = c.iter().find(|e| e.0 == g.appid) {
        if e.2.elapsed() < std::time::Duration::from_secs(10) {
            return e.1;
        }
    }
    let size = local::dir_size(&g.dir);
    c.retain(|e| e.0 != g.appid);
    c.push((g.appid, size, std::time::Instant::now()));
    size
}

/// Область для меню по правому щелчку: место блока с прошлого кадра. Регистрируется ДО
/// кнопок блока - поэтому кнопки поверх нее нажимаются (иначе она перехватывала бы щелчки).
fn menu_area(ui: &mut egui::Ui, id: egui::Id) -> Option<egui::Response> {
    let rect = ui.ctx().data(|d| d.get_temp::<egui::Rect>(id))?;
    Some(ui.interact(rect, id, egui::Sense::click()))
}

fn remember_area(ui: &egui::Ui, id: egui::Id, rect: egui::Rect) {
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
}

/// Строка списка: шапка игры, название, состояние, кнопки справа и меню по правому щелчку.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash,
    appid: u32,
    name: &str,
    state: &str,
    color: egui::Color32,
    acts: &mut Vec<Act>,
    buttons: impl FnOnce(&mut egui::Ui, &mut Vec<Act>),
    menu: impl FnOnce(&mut egui::Ui, &mut Vec<Act>),
) {
    let id = ui.id().with(("dl_row", key));
    let bg = menu_area(ui, id);
    let frame = egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(10.0, 6.0)).rounding(3.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if !crate::custom::is_custom(appid) {
                ui.add(egui::Image::new(header_url(appid)).fit_to_exact_size(egui::vec2(120.0, 56.0)).rounding(2.0));
            }
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 230.0).max(120.0));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(name).size(14.5));
                ui.add(egui::Label::new(egui::RichText::new(state).size(12.0).color(color)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| buttons(ui, acts));
        });
    });
    remember_area(ui, id, frame.response.rect);
    if let Some(bg) = bg {
        bg.context_menu(|ui| menu(ui, acts));
    }
    ui.add_space(4.0);
}
