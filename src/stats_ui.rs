//! "Statistics": playtime. Steam keeps only "total" and "two weeks", so
//! Vaporust records the daily history itself: on every library refresh it compares
//! each game's total time with the previous one and attributes the difference to today.
//! File: <Vaporust data folder>\playtime_<SteamID>.json - each account has its own history.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct History {
    /// Whose history (SteamID). Each account has its own file: otherwise the second account's total time
    /// would count as "played today".
    pub owner: u64,
    /// The old shared history is already merged into this one (merging again would double the hours).
    pub legacy_imported: bool,
    /// Day recording started (YYYY-MM-DD).
    pub since: String,
    /// Last known total time: appid -> minutes.
    pub last: BTreeMap<u32, u32>,
    /// Day -> (appid -> minutes played that day).
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

/// History of account `owner` (own file). The old shared history (playtime.json from versions before
/// the per-account split) is not given to anyone by the program itself: whose it is cannot be known reliably.
/// The user moves it in "Statistics" (take_legacy) or declines (decline_legacy).
pub fn load_for(owner: u64) -> History {
    file_for(owner).and_then(|p| read(&p)).map(|h| History { owner, ..h }).unwrap_or(History { owner, ..Default::default() })
}

fn legacy_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("playtime.json"))
}

/// The old shared history, if not moved and not declined yet.
pub fn legacy() -> Option<History> {
    legacy_path().and_then(|p| read(&p))
}

/// Move the old history into the history of account `owner` (at the user's request): days
/// are added to those already recorded, the last known times are taken from the new one. The old file
/// is renamed only after a successful write.
/// Returns the history and a warning (if the move succeeded but the old file could not be removed).
pub fn take_legacy(owner: u64, current: &History) -> Result<(History, Option<String>), String> {
    let old_path = legacy_path().ok_or(tr!("нет папки данных", "no data folder"))?;
    let old = read(&old_path).ok_or(tr!("старой истории больше нет", "the old history is gone"))?;
    if current.legacy_imported {
        // Already moved (for example the old file could not be removed last time) - do not add it twice.
        let _ = std::fs::rename(&old_path, old_path.with_extension("json.migrated"));
        return Ok((current.clone(), None));
    }
    let mut h = current.clone();
    h.owner = owner;
    h.legacy_imported = true;
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
    // First write the moved history (marked legacy_imported), then remove the old file.
    // If removal fails - the move still happened, and the mark prevents adding the hours twice.
    save(&h)?;
    let warn = std::fs::rename(&old_path, old_path.with_extension("json.migrated"))
        .err()
        .map(|e| trf!("История перенесена, но старый файл убрать не удалось ({e}). Повторно часы не сложатся.", "The history was moved, but the old file could not be removed ({e}). The hours will not be added twice."));
    Ok((h, warn))
}


/// Whether the old shared file looks like the history of an account with these games: at least 80% of the file's games
/// are owned, and none has more time in the file than now (playtime does not decrease).
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
    let p = file_for(h.owner).filter(|_| h.owner != 0).ok_or(tr!("нет папки данных", "no data folder"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(h).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Take new total times (appid, minutes) into account. Returns true if something changed.
/// The first time it only remembers (otherwise all past hours would land on one day).
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
                // A new game on the account: its past time is not attributed to today.
                h.last.insert(appid, total);
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

/// Minutes per day for the last `n` days (oldest first): (day, minutes).
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
    /// The last accounted game list (shows that the library was refreshed).
    pub seen: usize,
    pub seen_sum: u64,
    /// When we last asked to refresh the library for the statistics.
    pub refresh_at: Option<std::time::Instant>,
    /// For which account the old shared history was already checked and whether it exists.
    pub legacy_checked: Option<u64>,
    pub legacy_present: bool,
}

fn hours(min: u32) -> String {
    if min < 60 {
        trf!("{min} мин", "{min} min")
    } else {
        trf!("{:.1} ч", "{:.1} h", min as f32 / 60.0)
    }
}

/// Chart row: name on the left, bar, time on the right (the caption does not overlap the bar).
fn bar(ui: &mut egui::Ui, frac: f32, name: &str, value: String) {
    let w = ui.available_width().min(640.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::hover());
    let name_w = 230.0f32.min(w * 0.45);
    let value_w = 70.0;
    let p = ui.painter();
    let font = egui::FontId::proportional(12.5);
    let name = crate::richtext::fix(ui, name);
    // The name is cut to the column width.
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

    /// Every frame (cheap): the library was refreshed - record the time difference.
    /// Every 30 minutes while Vaporust is open and logged in - refresh the library (playtime).
    pub(crate) fn stats_tick(&mut self, net: &NetState) {
        if net.account.is_none() || net.owned.is_empty() {
            return;
        }
        let Some(me) = net.my_id else { return };
        // The account changed - its own history and "what is already accounted" from scratch.
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
                    crate::errlog::write(tr!("статистика", "statistics"), &trf!("не записана: {e}", "not written: {e}"));
                }
            }
        }
        let due = self.stats_ui.refresh_at.get_or_insert_with(std::time::Instant::now).elapsed() >= std::time::Duration::from_secs(1800);
        if due && !net.busy {
            self.stats_ui.refresh_at = Some(std::time::Instant::now());
            let _ = self.tx.send(Cmd::RefreshLibrary);
        }
    }

    /// The old shared history (from versions before the per-account split): ask whose it is.
    fn legacy_offer(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some(me) = net.my_id else { return };
        // A refusal is per account: the offer stays for the real owner.
        if self.settings.legacy_declined.contains(&me) || self.stats_ui.history.as_ref().is_some_and(|h| h.legacy_imported) {
            return;
        }
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
            ui.label(egui::RichText::new(tr!("Найдена история времени из прошлых версий Vaporust", "Playtime history from earlier Vaporust versions found")).strong());
            ui.label(trf!(
                "Ведется с {}, дней с записями: {}. Раньше она была общей для всех аккаунтов на этом ПК, поэтому Vaporust не знает, чья она.", "Kept since {}, days with records: {}. It used to be shared by all accounts on this PC, so Vaporust does not know whose it is.",
                if old.since.is_empty() { "?" } else { &old.since },
                old.days.len()
            ));
            if !likely {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr!("Похоже, это история другого аккаунта: игры или время в ней не совпадают с этим аккаунтом.", "This looks like another account's history: its games or times do not match this account."));
            }
            ui.horizontal(|ui| {
                if ui.button(tr!("Это моя история - перенести", "This is my history - move it")).clicked() {
                    let cur = self.stats_ui.history.clone().unwrap_or(History { owner: me, ..Default::default() });
                    match take_legacy(me, &cur) {
                        Ok((h, warn)) => {
                            self.stats_ui.history = Some(h);
                            self.stats_ui.legacy_present = false;
                            if let Some(w) = warn {
                                crate::popup::push(crate::popup::Kind::Info, tr!("Статистика", "Statistics"), w);
                            }
                        }
                        Err(e) => crate::popup::push(crate::popup::Kind::Info, tr!("Статистика", "Statistics"), trf!("Не перенесено: {e}", "Not moved: {e}")),
                    }
                }
                if ui.button(tr!("Это не моя", "It is not mine")).clicked() {
                    // Only for this account: the file stays, other accounts will see the offer.
                    self.settings.legacy_declined.push(me);
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
        ui.heading(tr!("Статистика", "Statistics"));
        self.legacy_offer(ui, net);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let total: u32 = net.owned.iter().map(|g| g.playtime_forever_minutes).sum();
            let played = net.owned.iter().filter(|g| g.playtime_forever_minutes > 0).count();
            ui.label(trf!(
                "Всего в играх: {} · игр на аккаунте: {} · запускали: {} · ни разу: {}", "Total playtime: {} · games on the account: {} · launched: {} · never: {}",
                hours(total),
                net.owned.len(),
                played,
                net.owned.len() - played
            ));
            ui.add_space(8.0);

            // By day - our own record.
            ui.strong(tr!("По дням (последние 30)", "By day (last 30)"));
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
                    tr!("Пока пусто: график наполнится, когда вы поиграете", "Empty for now: the chart fills up as you play"),
                    egui::FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );
            }
            if let Some((d, m)) = hover_day {
                resp.on_hover_text(format!("{}: {}", d.format("%d.%m.%Y"), hours(m)));
            }
            if h.since.is_empty() {
                ui.weak(tr!("История начнет записываться после входа в аккаунт.", "The history starts recording after you log in."));
            } else {
                ui.weak(trf!(
                    "Vaporust записывает историю с {} (Steam хранит только общее время). Учитывается время, которое \
                     Steam засчитал, в том числе при игре на других ПК.", "Vaporust records the history since {} (Steam keeps only the total time). The time Steam counted is included, also when playing on other PCs.",
                    h.since
                ));
            }
            ui.add_space(10.0);

            // The last two weeks - from Steam.
            ui.strong(tr!("Последние две недели (по данным Steam)", "Last two weeks (Steam data)"));
            match &net.my_recent {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.weak(trf!("Не удалось узнать: {e}", "Could not find out: {e}"));
                }
                Some(Ok(v)) if v.is_empty() => {
                    ui.weak(tr!("За две недели игр не было.", "No games in the last two weeks."));
                }
                Some(Ok(v)) => {
                    let max = v.iter().map(|g| g.two_weeks).max().unwrap_or(1).max(1);
                    for g in v {
                        bar(ui, g.two_weeks as f32 / max as f32, &g.name, hours(g.two_weeks));
                    }
                }
            }
            ui.add_space(10.0);

            // The most hours of all time.
            ui.strong(tr!("Больше всего часов", "Most hours"));
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

    /// QA-04: the old shared file goes to an account only if these are its games.
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
