//! «Статистика»: время в играх. Steam хранит только «всего» и «за две недели», поэтому
//! историю по дням Vaporust записывает сам: при каждом обновлении библиотеки сравнивает
//! общее время каждой игры с прошлым разом и разницу относит к сегодняшнему дню.
//! Файл: <папка данных Vaporust>\playtime_<SteamID>.json - у каждого аккаунта своя история.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct History {
    /// Чья история (SteamID). У каждого аккаунта свой файл: иначе общее время второго аккаунта
    /// засчиталось бы как «сыграно сегодня».
    pub owner: u64,
    /// С какого дня ведется запись (ГГГГ-ММ-ДД).
    pub since: String,
    /// Последнее известное общее время: appid -> минут.
    pub last: BTreeMap<u32, u32>,
    /// День -> (appid -> минут, сыгранных в этот день).
    pub days: BTreeMap<String, BTreeMap<u32, u32>>,
}

fn data_dir() -> Option<PathBuf> {
    if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() }
}

fn file_for(owner: u64) -> Option<PathBuf> {
    data_dir().map(|d| d.join(format!("playtime_{owner}.json")))
}

fn read(p: &std::path::Path) -> Option<History> {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// История аккаунта `owner` (свой файл). Старую общую историю (playtime.json из версий до
/// разделения по аккаунтам) сама программа никому не отдает: чья она, достоверно не узнать.
/// Ее переносит пользователь в «Статистике» (take_legacy) или отказывается (decline_legacy).
pub fn load_for(owner: u64) -> History {
    file_for(owner).and_then(|p| read(&p)).map(|h| History { owner, ..h }).unwrap_or(History { owner, ..Default::default() })
}

fn legacy_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("playtime.json"))
}

/// Старая общая история, если она еще не перенесена и не отклонена.
pub fn legacy() -> Option<History> {
    legacy_path().and_then(|p| read(&p))
}

/// Перенести старую историю в историю аккаунта `owner` (по просьбе пользователя): дни
/// складываются с уже записанными, последние известные времена берутся новые. Старый файл
/// переименовывается только после успешной записи.
pub fn take_legacy(owner: u64, current: &History) -> Result<History, String> {
    let old_path = legacy_path().ok_or("нет папки данных")?;
    let old = read(&old_path).ok_or("старой истории больше нет")?;
    let mut h = current.clone();
    h.owner = owner;
    for (day, games) in old.days {
        let d = h.days.entry(day).or_default();
        for (app, min) in games {
            *d.entry(app).or_default() += min;
        }
    }
    if h.since.is_empty() || (!old.since.is_empty() && old.since < h.since) {
        h.since = old.since;
    }
    for (app, total) in old.last {
        h.last.entry(app).or_insert(total);
    }
    save(&h)?;
    std::fs::rename(&old_path, old_path.with_extension("json.migrated")).map_err(|e| e.to_string())?;
    Ok(h)
}

/// «Это не моя история»: старый файл больше не предлагать (он остается на диске как .declined).
pub fn decline_legacy() -> Result<(), String> {
    let p = legacy_path().ok_or("нет папки данных")?;
    std::fs::rename(&p, p.with_extension("json.declined")).map_err(|e| e.to_string())
}

/// Похож ли старый общий файл на историю аккаунта с такими играми: не меньше 80% игр из файла
/// есть у аккаунта, и ни у одной время в файле не больше нынешнего (время в игре не убывает).
pub fn adopt_legacy(h: &History, totals: &[(u32, u32)]) -> bool {
    if h.last.is_empty() {
        return false;
    }
    let now: std::collections::HashMap<u32, u32> = totals.iter().copied().collect();
    let mine = h.last.keys().filter(|id| now.contains_key(id)).count();
    let consistent = h.last.iter().all(|(id, old)| now.get(id).is_none_or(|cur| old <= cur));
    mine * 10 >= h.last.len() * 8 && consistent
}

fn save(h: &History) -> Result<(), String> {
    let p = file_for(h.owner).filter(|_| h.owner != 0).ok_or("нет папки данных")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(h).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Учесть новые общие времена (appid, минут). Возвращает true, если что-то изменилось.
/// Первый раз только запоминает (иначе все прошлые часы попали бы в один день).
pub fn record(h: &mut History, today: &str, totals: &[(u32, u32)]) -> bool {
    if h.since.is_empty() {
        h.since = today.to_string();
        h.last = totals.iter().copied().collect();
        return true;
    }
    let mut changed = false;
    for &(appid, total) in totals {
        let prev = h.last.get(&appid).copied();
        match prev {
            Some(p) if total > p => {
                *h.days.entry(today.to_string()).or_default().entry(appid).or_default() += total - p;
                h.last.insert(appid, total);
                changed = true;
            }
            None => {
                // Новая игра на аккаунте: ее прошлое время не относим к сегодня.
                h.last.insert(appid, total);
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

/// Минут по дням за последние `n` дней (старые первыми): (день, минут).
pub fn per_day(h: &History, today: chrono::NaiveDate, n: i64) -> Vec<(chrono::NaiveDate, u32)> {
    (0..n)
        .rev()
        .map(|i| {
            let d = today - chrono::Duration::days(i);
            let key = d.format("%Y-%m-%d").to_string();
            (d, h.days.get(&key).map(|m| m.values().sum()).unwrap_or(0))
        })
        .collect()
}

#[derive(Default)]
pub struct StatsUi {
    pub open: bool,
    pub asked: bool,
    pub history: Option<History>,
    /// Последний учтенный список игр (по нему видно, что библиотека обновилась).
    pub seen: usize,
    pub seen_sum: u64,
    /// Когда последний раз просили обновить библиотеку ради статистики.
    pub refresh_at: Option<std::time::Instant>,
    /// Для какого аккаунта уже проверяли старую общую историю и есть ли она.
    pub legacy_checked: Option<u64>,
    pub legacy_present: bool,
}

fn hours(min: u32) -> String {
    if min < 60 {
        format!("{min} мин")
    } else {
        format!("{:.1} ч", min as f32 / 60.0)
    }
}

/// Строка графика: название слева, полоса, время справа (подпись не налезает на полосу).
fn bar(ui: &mut egui::Ui, frac: f32, name: &str, value: String) {
    let w = ui.available_width().min(640.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::hover());
    let name_w = 230.0f32.min(w * 0.45);
    let value_w = 70.0;
    let p = ui.painter();
    let font = egui::FontId::proportional(12.5);
    let name = crate::richtext::fix(ui, name);
    // Название обрезается по ширине колонки.
    let galley = ui.fonts(|f| {
        let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), font.clone(), ui.visuals().text_color());
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_w - 8.0);
        f.layout_job(job)
    });
    p.galley(egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, ui.visuals().text_color());
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left() + name_w, rect.top() + 4.0),
        egui::pos2(rect.right() - value_w, rect.bottom() - 4.0),
    );
    p.rect_filled(track, 3.0, ui.visuals().faint_bg_color);
    let mut fill = track;
    fill.set_width((track.width() * frac.clamp(0.0, 1.0)).max(3.0));
    p.rect_filled(fill, 3.0, egui::Color32::from_rgb(70, 130, 200));
    p.text(egui::pos2(rect.right() - value_w + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, value, font, ui.visuals().weak_text_color());
    resp.on_hover_text(name);
}

impl LauncherApp {
    pub(crate) fn open_stats(&mut self) {
        self.close_extras(crate::View::Stats);
        self.stats_ui.open = true;
        self.stats_ui.asked = false;
        self.view = crate::View::Stats;
    }

    /// Каждый кадр (дешево): библиотека обновилась - записать разницу во времени.
    /// Раз в 30 минут, пока Vaporust открыт и вошел, - обновить библиотеку (время в играх).
    pub(crate) fn stats_tick(&mut self, net: &NetState) {
        if net.account.is_none() || net.owned.is_empty() {
            return;
        }
        let Some(me) = net.my_id else { return };
        // Сменился аккаунт - своя история и заново «что уже учтено».
        if self.stats_ui.history.as_ref().is_some_and(|h| h.owner != me) {
            self.stats_ui.history = None;
            self.stats_ui.seen = 0;
            self.stats_ui.seen_sum = 0;
        }
        let sum: u64 = net.owned.iter().map(|g| g.playtime_forever_minutes as u64).sum();
        if self.stats_ui.seen != net.owned.len() || self.stats_ui.seen_sum != sum {
            self.stats_ui.seen = net.owned.len();
            self.stats_ui.seen_sum = sum;
            let h = self.stats_ui.history.get_or_insert_with(|| load_for(me));
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let totals: Vec<(u32, u32)> = net.owned.iter().map(|g| (g.app_id, g.playtime_forever_minutes)).collect();
            if record(h, &today, &totals) {
                if let Err(e) = save(h) {
                    crate::errlog::write("статистика", &format!("не записана: {e}"));
                }
            }
        }
        let due = self.stats_ui.refresh_at.get_or_insert_with(std::time::Instant::now).elapsed() >= std::time::Duration::from_secs(1800);
        if due && !net.busy {
            self.stats_ui.refresh_at = Some(std::time::Instant::now());
            let _ = self.tx.send(Cmd::RefreshLibrary);
        }
    }

    /// Старая общая история (из версий до разделения по аккаунтам): спросить, чья она.
    fn legacy_offer(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(me) = net.my_id else { return };
        if self.stats_ui.legacy_checked == Some(me) && !self.stats_ui.legacy_present {
            return;
        }
        if self.stats_ui.legacy_checked != Some(me) {
            self.stats_ui.legacy_checked = Some(me);
            self.stats_ui.legacy_present = legacy().is_some();
        }
        let Some(old) = legacy() else {
            self.stats_ui.legacy_present = false;
            return;
        };
        let totals: Vec<(u32, u32)> = net.owned.iter().map(|g| (g.app_id, g.playtime_forever_minutes)).collect();
        let likely = adopt_legacy(&old, &totals);
        egui::Frame::none().fill(crate::theme::CARD).rounding(4.0).inner_margin(10.0).show(ui, |ui| {
            ui.label(egui::RichText::new("Найдена история времени из прошлых версий Vaporust").strong());
            ui.label(format!(
                "Ведется с {}, дней с записями: {}. Раньше она была общей для всех аккаунтов на этом ПК, поэтому Vaporust не знает, чья она.",
                if old.since.is_empty() { "?" } else { &old.since },
                old.days.len()
            ));
            if !likely {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "Похоже, это история другого аккаунта: игры или время в ней не совпадают с этим аккаунтом.");
            }
            ui.horizontal(|ui| {
                if ui.button("Это моя история - перенести").clicked() {
                    let cur = self.stats_ui.history.clone().unwrap_or(History { owner: me, ..Default::default() });
                    match take_legacy(me, &cur) {
                        Ok(h) => {
                            self.stats_ui.history = Some(h);
                            self.stats_ui.legacy_present = false;
                        }
                        Err(e) => crate::popup::push(crate::popup::Kind::Info, "Статистика", format!("Не перенесено: {e}")),
                    }
                }
                if ui.button("Это не моя").clicked() {
                    match decline_legacy() {
                        Ok(()) => self.stats_ui.legacy_present = false,
                        Err(e) => crate::popup::push(crate::popup::Kind::Info, "Статистика", format!("Не получилось: {e}")),
                    }
                }
            });
        });
        ui.add_space(8.0);
    }

    pub(crate) fn stats_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.stats_ui.asked && net.account.is_some() {
            self.stats_ui.asked = true;
            let _ = self.tx.send(Cmd::MyRecent);
        }
        let h = match (net.my_id, self.stats_ui.history.as_ref()) {
            (Some(me), Some(h)) if h.owner == me => h.clone(),
            (Some(me), _) => self.stats_ui.history.insert(load_for(me)).clone(),
            _ => History::default(),
        };
        ui.heading("Статистика");
        self.legacy_offer(ui, net);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let total: u32 = net.owned.iter().map(|g| g.playtime_forever_minutes).sum();
            let played = net.owned.iter().filter(|g| g.playtime_forever_minutes > 0).count();
            ui.label(format!(
                "Всего в играх: {} · игр на аккаунте: {} · запускали: {} · ни разу: {}",
                hours(total),
                net.owned.len(),
                played,
                net.owned.len() - played
            ));
            ui.add_space(8.0);

            // По дням - своя запись.
            ui.strong("По дням (последние 30)");
            let today = chrono::Local::now().date_naive();
            let days = per_day(&h, today, 30);
            let max = days.iter().map(|d| d.1).max().unwrap_or(0).max(60);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(620.0), 120.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
            let w = rect.width() / days.len() as f32;
            let mut hover_day = None;
            for (i, (d, m)) in days.iter().enumerate() {
                let x = rect.left() + i as f32 * w;
                let hgt = (rect.height() - 4.0) * (*m as f32 / max as f32);
                let r = egui::Rect::from_min_max(egui::pos2(x + 1.0, rect.bottom() - hgt), egui::pos2(x + w - 1.0, rect.bottom()));
                p.rect_filled(r, 1.0, egui::Color32::from_rgb(70, 130, 200));
                if resp.hover_pos().is_some_and(|pos| pos.x >= x && pos.x < x + w) {
                    hover_day = Some((*d, *m));
                }
            }
            if days.iter().all(|d| d.1 == 0) {
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Пока пусто: график наполнится, когда вы поиграете",
                    egui::FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );
            }
            if let Some((d, m)) = hover_day {
                resp.on_hover_text(format!("{}: {}", d.format("%d.%m.%Y"), hours(m)));
            }
            if h.since.is_empty() {
                ui.weak("История начнет записываться после входа в аккаунт.");
            } else {
                ui.weak(format!(
                    "Vaporust записывает историю с {} (Steam хранит только общее время). Учитывается время, которое \
                     Steam засчитал, в том числе при игре на других ПК.",
                    h.since
                ));
            }
            ui.add_space(10.0);

            // Последние две недели - от Steam.
            ui.strong("Последние две недели (по данным Steam)");
            match &net.my_recent {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.weak(format!("Не удалось узнать: {e}"));
                }
                Some(Ok(v)) if v.is_empty() => {
                    ui.weak("За две недели игр не было.");
                }
                Some(Ok(v)) => {
                    let max = v.iter().map(|g| g.two_weeks).max().unwrap_or(1).max(1);
                    for g in v {
                        bar(ui, g.two_weeks as f32 / max as f32, &g.name, hours(g.two_weeks));
                    }
                }
            }
            ui.add_space(10.0);

            // Больше всего часов за все время.
            ui.strong("Больше всего часов");
            let mut top: Vec<_> = net.owned.iter().filter(|g| g.playtime_forever_minutes > 0).collect();
            top.sort_by_key(|g| std::cmp::Reverse(g.playtime_forever_minutes));
            let max = top.first().map(|g| g.playtime_forever_minutes).unwrap_or(1).max(1);
            for g in top.into_iter().take(15) {
                bar(ui, g.playtime_forever_minutes as f32 / max as f32, &g.name, hours(g.playtime_forever_minutes));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_deltas() {
        let mut h = History::default();
        assert!(record(&mut h, "2026-11-13", &[(70, 600), (10, 0)]));
        assert!(h.days.is_empty(), "первый раз - только запоминаем");
        assert!(!record(&mut h, "2026-11-13", &[(70, 600), (10, 0)]));
        assert!(record(&mut h, "2026-11-13", &[(70, 660), (10, 0), (20, 300)]));
        assert_eq!(h.days["2026-11-13"].get(&70), Some(&60));
        assert_eq!(h.days["2026-11-13"].get(&20), None, "новая игра не считается сыгранной сегодня");
        record(&mut h, "2026-11-14", &[(70, 700), (20, 330)]);
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let d = per_day(&h, today, 3);
        assert_eq!(d.iter().map(|x| x.1).collect::<Vec<_>>(), vec![0, 60, 70]);
    }

    /// QA-04: старый общий файл достается аккаунту, только если это его игры.
    #[test]
    fn legacy_file_owner() {
        let mut h = History::default();
        record(&mut h, "2026-11-13", &[(70, 10), (10, 5), (20, 1), (30, 0), (40, 0)]);
        assert!(adopt_legacy(&h, &[(70, 12), (10, 5), (20, 1), (30, 0), (40, 0), (50, 3)]), "все игры свои, время не убыло");
        assert!(adopt_legacy(&h, &[(70, 10), (10, 5), (20, 1), (30, 0)]), "4 из 5 = 80%");
        assert!(!adopt_legacy(&h, &[(70, 10), (10, 5), (99, 1)]), "чужой аккаунт");
        assert!(!adopt_legacy(&h, &[(70, 3), (10, 5), (20, 1), (30, 0), (40, 0)]), "время в файле больше нынешнего - файл чужой");
        assert!(!adopt_legacy(&History::default(), &[(70, 1)]), "пустой файл");
    }
}
