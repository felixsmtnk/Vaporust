//! "Downloads", as in Steam: a button in the middle of the bottom bar with the current download's
//! progress, and a page: on top the current download with a speed graph, below "Up next" (queue) and
//! "Completed" (what was downloaded or interrupted during this session).

use crate::net::{self, NetState};
use crate::{local, schedule, settings, theme, LauncherApp, View};
use eframe::egui;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// Game image for downloads (Steam store header).
fn header_url(appid: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg")
}

/// How many seconds of speed history the graph shows.
const GRAPH_SECS: usize = 120;

/// Speed history of the current download (once per second).
struct Graph {
    /// Which download (the address of its cancel flag is unique per operation).
    op: usize,
    samples: VecDeque<f32>,
    peak: f32,
    last: Option<std::time::Instant>,
}

static GRAPH: Mutex<Graph> = Mutex::new(Graph { op: 0, samples: VecDeque::new(), peak: 0.0, last: None });

/// A download completed during the session (or another operation on game files).
#[derive(Clone)]
struct Done {
    appid: u32,
    name: String,
    /// Ok - success text; Err - error or cancellation.
    result: Result<String, String>,
    /// This is an install (not a verify, move or uninstall).
    install: bool,
    when: String,
}

/// (last recorded operation, the "Completed" list - newest on top)
static DONE: Mutex<(usize, Vec<Done>)> = Mutex::new((0, Vec::new()));

fn op_id(i: &net::Install) -> usize {
    std::sync::Arc::as_ptr(&i.abort) as usize
}

/// Record the speed once per second; a finished operation goes to "Completed".
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
    // Stopped ones (pause, cancel) are not "completed": they are in the queue or in "Not finished".
    let _ = queue;
    if cancelled {
        return;
    }
    let result = match &i.result {
        Some(Ok(())) => Ok(i.done_text.clone()),
        Some(Err(e)) => Err(e.clone()),
        None => return,
    };
    // Result of a file operation (uninstall, verify, move) - also as a popup notification:
    // a downloaded game is announced by notices.rs.
    if !i.download {
        // The operation name contains the action kind ("Удаление: игра"): only the game goes into the notification.
        // Both languages: the interface language may have changed while the operation ran.
        let name = [
            "Смена языка: ", "Language change: ", "Обновление: ", "Update: ", "Проверка файлов: ", "Verifying files: ",
            "Перенос: ", "Moving: ", "Добавление: ", "Adding: ", "Откат версии: ", "Version rollback: ", "Удаление: ", "Uninstalling: ",
        ]
            .iter()
            .find_map(|p| i.name.strip_prefix(p))
            .unwrap_or(&i.name);
        let body = match &result {
            Ok(t) => trf!("«{name}» {t}", "\"{name}\" {t}"),
            Err(e) => trf!("«{name}»: {e}", "\"{name}\": {e}"),
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
            install: i.download,
            when: chrono::Local::now().format("%H:%M").to_string(),
        },
    );
}

/// Section header, as in Steam: small, uppercase, with a line to the edge.
fn section(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text).font(theme::semibold(16.0)).color(theme::TEXT));
        add(ui);
        let r = ui.available_rect_before_wrap();
        let y = r.center().y;
        ui.painter().hline(r.left() + 6.0..=r.right(), y, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(52, 60, 74)));
    });
    ui.add_space(6.0);
}

/// Steam progress bar: thin, no text.
fn thin_bar(ui: &mut egui::Ui, frac: f32, width: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, egui::Color32::from_rgb(45, 52, 64));
    let mut d = r;
    d.set_width((r.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { 6.0 } else { 0.0 }));
    ui.painter().rect_filled(d, 3.0, theme::ACCENT);
}

/// Stat column to the right of the graph.
fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(egui::RichText::new(label).size(10.5).color(theme::WEAK));
        ui.label(egui::RichText::new(value).size(14.0).color(theme::TEXT));
    });
}

/// Speed graph for the last 2 minutes.
fn graph(ui: &mut egui::Ui, height: f32) {
    let g = GRAPH.lock().unwrap();
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 2.0, egui::Color32::from_rgb(20, 24, 31));
    // Grid: 4 horizontal lines.
    for k in 1..4 {
        let y = r.top() + r.height() * k as f32 / 4.0;
        p.hline(r.x_range(), y, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(34, 40, 50)));
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
    // Fill under the line - vertical strips (there are no convex polygons here).
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
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, theme::ACCENT)));
}

impl LauncherApp {
    /// "Downloads" button label: downloading - with percent, a queue - with the game count.
    pub(crate) fn downloads_label(&self, net: &NetState) -> String {
        if let Some(i) = net.install.as_ref().filter(|i| i.result.is_none()) {
            if i.total > 0 {
                let pct = (i.done as f64 / i.total as f64 * 100.0).clamp(0.0, 100.0);
                return trf!("Загрузки: {} - {pct:.0}%", "Downloads: {} - {pct:.0}%", i.name);
            }
            return trf!("Загрузки: {}", "Downloads: {}", i.name);
        }
        match self.settings.downloads.len() {
            0 => tr!("Управление загрузками", "Manage Downloads").into(),
            n => trf!("Загрузки ({n} в очереди)", "Downloads ({n} queued)"),
        }
    }

    /// Button in the middle of the bottom bar; while downloading, a thin progress bar under it.
    pub(crate) fn downloads_button(&mut self, ui: &mut egui::Ui, net: &NetState) {
        sample(net, &self.settings.downloads);
        let label = self.downloads_label(net);
        let on = self.view == View::Downloads;
        // The same button as "Add a game" and "Friends and chat" (theme::bar_button), without an icon.
        let resp = theme::bar_button(ui, &label, Some(theme::icon_download), None, on);
        if let Some(i) = net.install.as_ref().filter(|i| i.result.is_none() && i.total > 0) {
            let frac = (i.done as f32 / i.total as f32).clamp(0.0, 1.0);
            let r = resp.rect;
            let bar = egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - 1.0), egui::pos2(r.right(), r.bottom() + 1.0));
            ui.painter().rect_filled(bar, 0.0, egui::Color32::from_rgb(45, 52, 64));
            let mut done = bar;
            done.set_width(bar.width() * frac);
            ui.painter().rect_filled(done, 0.0, theme::ACCENT);
        }
        if resp.on_hover_text(tr!("Текущая загрузка и очередь", "Current download and queue")).clicked() {
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
            crate::theme::page_title(ui, tr!("Загрузки", "Downloads"));
            ui.add_space(8.0);
            if !self.install_note.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), &self.install_note);
            }
            if !schedule::may_download(&self.settings) {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 170, 60),
                    trf!(
                        "Сейчас загрузки ждут расписания (с {}). Расписание - в «Настройки» -> «Загрузки».", "Downloads are waiting for the schedule (from {}). The schedule is in \"Settings\" -> \"Downloads\".",
                        schedule::hh(self.settings.sched_from)
                    ),
                );
            }

            // On top - the current download with a graph, as in Steam.
            let active = net.install.as_ref().filter(|i| i.result.is_none());
            let library = active.and_then(|i| {
                self.games
                    .iter()
                    .find(|g| g.appid == i.appid)
                    .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
                    .or_else(|| self.libraries.get(self.lib_idx).cloned())
            })
            .or_else(|| self.libraries.get(self.lib_idx).cloned());
            // Right-click menu - under the card (area from the previous frame) so the buttons
            // on top of it stay clickable.
            let card_id = ui.id().with("dl_card");
            let bg = active.and_then(|_| menu_area(ui, card_id));
            let card = egui::Frame::none().fill(theme::CARD).inner_margin(egui::Margin::same(14.0)).rounding(4.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let Some(inst) = active else {
                    // Idle, as in Steam: the empty graph with the network, peak and disk numbers.
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 162.0).max(120.0));
                            graph(ui, 70.0);
                        });
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 5.0;
                            let peak = GRAPH.lock().unwrap().peak;
                            stat(ui, tr!("СЕТЬ", "NETWORK"), trf!("{}/с", "{}/s", net::fmt_bytes(0)));
                            stat(ui, tr!("ПИКОВАЯ", "PEAK"), trf!("{}/с", "{}/s", net::fmt_bytes(peak as u64)));
                            if let Some(free) = library.as_deref().and_then(local::free_space) {
                                stat(ui, tr!("СВОБОДНО НА ДИСКЕ", "FREE ON DISK"), net::fmt_bytes(free));
                            }
                        });
                    });
                    return;
                };
                let is_install = inst.download;
                let frac = if inst.total > 0 { (inst.done as f32 / inst.total as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.horizontal(|ui| {
                    if !crate::custom::is_custom(inst.appid) {
                        ui.add(egui::Image::new(header_url(inst.appid)).fit_to_exact_size(egui::vec2(230.0, 107.0)).rounding(3.0));
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&inst.name).font(crate::theme::semibold(19.0)).color(egui::Color32::WHITE));
                        let what = if is_install { tr!("ЗАГРУЗКА", "DOWNLOADING") } else { tr!("РАБОТА С ФАЙЛАМИ", "WORKING WITH FILES") };
                        let head = if inst.total > 0 { format!("{what} - {:.0}%", frac * 100.0) } else { what.to_string() };
                        ui.label(egui::RichText::new(head).size(12.0).strong().color(theme::ACCENT));
                        ui.label(egui::RichText::new(&inst.stage).size(12.0).color(theme::WEAK));
                        ui.add_space(4.0);
                        thin_bar(ui, frac, (ui.available_width() - 190.0).max(160.0));
                        if inst.total > 0 {
                            let mut t = trf!("{} из {}", "{} of {}", net::fmt_bytes(inst.done), net::fmt_bytes(inst.total));
                            if inst.speed > 1.0 {
                                let left = inst.total.saturating_sub(inst.done) as f64 / inst.speed;
                                t += &trf!("  ·  осталось ~{}", "  ·  ~{} left", crate::fmt_eta(left as u64));
                            }
                            ui.label(egui::RichText::new(t).size(12.0).color(theme::TEXT));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr!("Отмена", "Cancel")).on_hover_text(tr!("Остановить загрузку. Скачанное сохранится, ее можно продолжить.", "Stop the download. What was downloaded is kept, it can be resumed.")).clicked() {
                            acts.push(Act::Stop(false));
                        }
                        if is_install
                            && ui
                                .button(tr!("Пауза", "Pause"))
                                .on_hover_text(tr!("Поставить на паузу только эту игру: скачанное сохранится. Следующие игры из очереди начнут качаться.", "Pause only this game: what was downloaded is kept. The next games in the queue start downloading."))
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
                        stat(ui, tr!("ТЕКУЩАЯ СКОРОСТЬ", "CURRENT SPEED"), trf!("{}/с", "{}/s", net::fmt_bytes(inst.speed.max(0.0) as u64)));
                        stat(ui, tr!("ПИКОВАЯ", "PEAK"), trf!("{}/с", "{}/s", net::fmt_bytes(peak as u64)));
                        if inst.total > 0 {
                            stat(ui, tr!("ОСТАЛОСЬ СКАЧАТЬ", "LEFT TO DOWNLOAD"), net::fmt_bytes(inst.total.saturating_sub(inst.done)));
                        }
                        if let Some(free) = library.as_deref().and_then(local::free_space) {
                            stat(ui, tr!("СВОБОДНО НА ДИСКЕ", "FREE ON DISK"), net::fmt_bytes(free));
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
                    if ui.button(tr!("Пауза", "Pause")).clicked() {
                        acts.push(Act::Stop(true));
                        ui.close_menu();
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        acts.push(Act::Stop(false));
                        ui.close_menu();
                    }
                });
            }

            // Up next: the queue.
            let n = self.settings.downloads.len();
            let queue_paused = self.settings.queue_paused;
            section(ui, &trf!("Далее ({n})", "Up Next ({n})"), |ui| {
                if n > 0 {
                    let (label, hint) = if queue_paused {
                        (tr!("Продолжить очередь", "Resume queue"), tr!("Снова качать игры из очереди по порядку.", "Download the queued games in order again."))
                    } else {
                        (tr!("Пауза очереди", "Pause queue"), tr!("Не начинать следующие игры из очереди (текущая загрузка продолжится).", "Do not start the next queued games (the current download continues)."))
                    };
                    if ui.small_button(label).on_hover_text(hint).clicked() {
                        acts.push(Act::ToggleQueue);
                    }
                }
            });
            if n == 0 {
                ui.weak(tr!("Очередь пуста. Игры встают сюда, если нажать «Установить», пока качается другая.", "The queue is empty. Games go here if you press \"Install\" while another one is downloading."));
            }
            for (i, q) in self.settings.downloads.iter().enumerate() {
                let partial = self.games.iter().find(|g| g.appid == q.appid && !g.ready);
                let mut state = if q.paused {
                    tr!("На паузе", "Paused").to_string()
                } else if steam_open && (!self.settings.download_with_steam || (partial.is_some() && !self.settings.staged.contains_key(&q.appid) && self.movable_partial(q.appid).is_none())) {
                    tr!("Ждет закрытия Steam", "Waiting for Steam to close").to_string()
                } else {
                    tr!("В очереди", "Queued").to_string()
                };
                if let Some(g) = partial {
                    state += &trf!(" · скачано {}", " · {} downloaded", net::fmt_bytes(partial_size(g)));
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
                        if ui.small_button(tr!("Убрать", "Remove")).on_hover_text(tr!("Убрать из очереди (скачанное останется на диске)", "Remove from the queue (the downloaded files stay on disk)")).clicked() {
                            acts.push(Act::Unqueue(i));
                        }
                        if paused {
                            if ui.small_button(tr!("Продолжить", "Resume")).clicked() {
                                acts.push(Act::SetPaused(i, false));
                            }
                        } else if ui.small_button(tr!("Пауза", "Pause")).on_hover_text(tr!("Пропускать эту игру, пока не продолжите", "Skip this game until you resume it")).clicked() {
                            acts.push(Act::SetPaused(i, true));
                        }
                        if i > 0 && n > 1 && ui.small_button(tr!("Выше", "Up")).on_hover_text(tr!("Скачать раньше", "Download earlier")).clicked() {
                            acts.push(Act::Move(i, i - 1));
                        }
                    },
                    |ui, acts| {
                        menu_common(ui, q.appid, &q.name, partial.is_some(), acts);
                        ui.separator();
                        if i > 0 && ui.button(tr!("Скачать первой", "Download first")).clicked() {
                            acts.push(Act::Move(i, 0));
                            ui.close_menu();
                        }
                        if i + 1 < n && ui.button(tr!("Ниже", "Down")).clicked() {
                            acts.push(Act::Move(i, i + 1));
                            ui.close_menu();
                        }
                        if ui.button(if paused { tr!("Продолжить", "Resume") } else { tr!("Пауза", "Pause") }).clicked() {
                            acts.push(Act::SetPaused(i, !paused));
                            ui.close_menu();
                        }
                        if ui.button(tr!("Убрать из очереди", "Remove from queue")).clicked() {
                            acts.push(Act::Unqueue(i));
                            ui.close_menu();
                        }
                        if partial.is_some() && ui.button(tr!("Удалить скачанное...", "Delete downloaded files...")).clicked() {
                            acts.push(Act::Uninstall(q.appid, q.name.clone()));
                            ui.close_menu();
                        }
                    },
                );
            }

            // Not finished: started downloads outside the queue (stopped, removed from the queue).
            let queued: std::collections::HashSet<u32> = self.settings.downloads.iter().map(|q| q.appid).collect();
            let partials: Vec<&local::Game> = self
                .games
                .iter()
                .filter(|g| !g.ready && !queued.contains(&g.appid) && Some(g.appid) != active_id)
                .collect();
            if !partials.is_empty() {
                section(ui, &trf!("Не докачаны ({})", "Not finished ({})", partials.len()), |_| {});
                for g in partials {
                    let state = trf!("Скачано {} · файлы лежат на диске", "Downloaded {} · the files are on disk", net::fmt_bytes(partial_size(g)));
                    row(
                        ui,
                        ("p", g.appid),
                        g.appid,
                        &g.name,
                        &state,
                        theme::WEAK,
                        &mut acts,
                        |ui, acts| {
                            if ui.small_button(tr!("Удалить", "Delete")).on_hover_text(tr!("Удалить скачанные файлы", "Delete the downloaded files")).clicked() {
                                acts.push(Act::Uninstall(g.appid, g.name.clone()));
                            }
                            if ui.small_button(tr!("Продолжить", "Resume")).on_hover_text(tr!("Докачать с того места, где остановилась", "Continue from where it stopped")).clicked() {
                                acts.push(Act::Resume(g.appid, g.name.clone()));
                            }
                        },
                        |ui, acts| {
                            menu_common(ui, g.appid, &g.name, true, acts);
                            ui.separator();
                            if ui.button(tr!("Продолжить загрузку", "Resume download")).clicked() {
                                acts.push(Act::Resume(g.appid, g.name.clone()));
                                ui.close_menu();
                            }
                            if ui.button(tr!("Удалить скачанное...", "Delete downloaded files...")).clicked() {
                                acts.push(Act::Uninstall(g.appid, g.name.clone()));
                                ui.close_menu();
                            }
                        },
                    );
                }
            }

            // Completed during this session.
            let done = DONE.lock().unwrap().1.clone();
            if !done.is_empty() {
                section(ui, &trf!("Завершено ({})", "Completed ({})", done.len()), |ui| {
                    if ui.small_button(tr!("Очистить", "Clear")).clicked() {
                        acts.push(Act::ClearDone);
                    }
                });
                let installed: std::collections::HashSet<u32> =
                    self.games.iter().filter(|g| g.ready).map(|g| g.appid).collect();
                for d in &done {
                    let (text, color) = match &d.result {
                        Ok(t) => (
                            trf!("{} в {}: {}", "{} at {}: {}", if d.install { tr!("Загрузка завершена", "Download complete") } else { tr!("Готово", "Done") }, d.when, t),
                            egui::Color32::from_rgb(110, 200, 110),
                        ),
                        Err(e) => (trf!("Ошибка в {}: {e}", "Error at {}: {e}", d.when), egui::Color32::from_rgb(230, 110, 110)),
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
                            if ui.small_button(tr!("Убрать", "Remove")).on_hover_text(tr!("Убрать из этого списка", "Remove from this list")).clicked() {
                                acts.push(Act::Forget(d.appid));
                            }
                            if can_play
                                && ui
                                    .add(egui::Button::new(egui::RichText::new(tr!("Играть", "Play")).color(egui::Color32::WHITE)).fill(theme::PLAY))
                                    .clicked()
                            {
                                acts.push(Act::Play(d.appid));
                            }
                        },
                        |ui, acts| {
                            if can_play && ui.button(tr!("Играть", "Play")).clicked() {
                                acts.push(Act::Play(d.appid));
                                ui.close_menu();
                            }
                            menu_common(ui, d.appid, &d.name, on_disk, acts);
                            ui.separator();
                            if ui.button(tr!("Убрать из списка", "Remove from list")).clicked() {
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
                        // Only a download goes back to the queue: a language switch, a rollback or a check would
                        // come back from the queue as a plain install of the latest version.
                        if pause && inst.download {
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

/// What was clicked on the downloads page (handled after drawing).
enum Act {
    /// Stop the current download: true - pause (to the front of the queue), false - cancel.
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

/// Common row menu items: game page and folder.
fn menu_common(ui: &mut egui::Ui, appid: u32, name: &str, on_disk: bool, acts: &mut Vec<Act>) {
    ui.set_min_width(200.0);
    ui.label(egui::RichText::new(name).strong());
    ui.separator();
    if ui.button(tr!("Страница игры", "Game page")).clicked() {
        acts.push(Act::Page(appid));
        ui.close_menu();
    }
    if on_disk && ui.button(tr!("Открыть папку игры", "Open game folder")).clicked() {
        acts.push(Act::Folder(appid));
        ui.close_menu();
    }
}

/// How much of an unfinished game is downloaded (folder size; recomputed at most every 10 s).
/// Counted in a background thread: walking a large game folder on a hard disk took long enough
/// to freeze the window. Until the first count is ready - 0.
fn partial_size(g: &local::Game) -> u64 {
    // appid -> (size, when counted, counting now)
    static CACHE: Mutex<Vec<(u32, u64, Option<std::time::Instant>, bool)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let i = match c.iter().position(|e| e.0 == g.appid) {
        Some(i) => i,
        None => {
            c.push((g.appid, 0, None, false));
            c.len() - 1
        }
    };
    let stale = c[i].2.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(10));
    if stale && !c[i].3 {
        c[i].3 = true;
        let (appid, dir) = (g.appid, g.dir.clone());
        std::thread::spawn(move || {
            let size = local::dir_size(&dir);
            let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(e) = c.iter_mut().find(|e| e.0 == appid) {
                *e = (appid, size, Some(std::time::Instant::now()), false);
            }
        });
    }
    c[i].1
}

/// Area for the right-click menu: the block's area from the previous frame. Registered BEFORE
/// the block's buttons - so the buttons on top stay clickable (otherwise it would steal clicks).
fn menu_area(ui: &mut egui::Ui, id: egui::Id) -> Option<egui::Response> {
    let rect = ui.ctx().data(|d| d.get_temp::<egui::Rect>(id))?;
    Some(ui.interact(rect, id, egui::Sense::click()))
}

fn remember_area(ui: &egui::Ui, id: egui::Id, rect: egui::Rect) {
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
}

/// List row: game header, name, state, buttons on the right and a right-click menu.
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
