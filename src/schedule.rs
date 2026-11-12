//! Расписание загрузок: часы, в которые можно качать, и своя скорость в эти часы.
//!
//! Окно задается часами «с from:00 до to:00» и может переходить через полночь
//! (с 23 до 7). from == to - круглые сутки.

use crate::settings::Settings;
use crate::LauncherApp;
use chrono::Timelike;

/// Входит ли час `hour` в окно [from, to).
pub fn in_window(from: u8, to: u8, hour: u8) -> bool {
    if from == to {
        true
    } else if from < to {
        hour >= from && hour < to
    } else {
        hour >= from || hour < to
    }
}

fn hour_now() -> u8 {
    chrono::Local::now().hour() as u8
}

/// Сейчас окно расписания?
pub fn window_now(s: &Settings) -> bool {
    in_window(s.sched_from, s.sched_to, hour_now())
}

/// Можно ли сейчас начинать загрузки.
pub fn may_download(s: &Settings) -> bool {
    !s.sched_only || window_now(s)
}

/// Ограничение скорости, которое действует сейчас (КБ/с, 0 - без ограничения).
pub fn effective_limit_kb(s: &Settings) -> u64 {
    if s.sched_limit_on && window_now(s) {
        s.sched_limit_kb
    } else {
        s.download_limit_kb
    }
}

/// «01:00».
pub fn hh(h: u8) -> String {
    format!("{h:02}:00")
}

impl LauncherApp {
    /// Раз в 5 с: смена скорости по часам и пауза загрузки вне окна расписания.
    pub(crate) fn schedule_tick(&mut self, net: &crate::net::NetState) {
        if self.sched_check.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
            return;
        }
        self.sched_check = Some(std::time::Instant::now());

        if net.account.is_some() && self.sent_limit != Some(effective_limit_kb(&self.settings)) {
            self.send_aurelia_config();
        }

        if may_download(&self.settings) {
            return;
        }
        // Вне окна: идущую установку - на паузу, но в очереди не помечаем паузой, чтобы
        // в начале окна она продолжилась сама.
        if let Some(inst) = net.install.as_ref().filter(|i| i.active() && i.done_text == "установлена.") {
            inst.abort.store(true, std::sync::atomic::Ordering::Relaxed);
            self.settings.downloads.retain(|q| q.appid != inst.appid);
            self.settings.downloads.insert(
                0,
                crate::settings::QueueItem { appid: inst.appid, name: inst.name.clone(), paused: false },
            );
            self.install_note = format!(
                "«{}»: загрузка остановлена по расписанию, продолжится в {}.",
                inst.name,
                hh(self.settings.sched_from)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::in_window;

    #[test]
    fn window() {
        assert!(in_window(1, 7, 1));
        assert!(in_window(1, 7, 6));
        assert!(!in_window(1, 7, 7));
        assert!(!in_window(1, 7, 0));
        // через полночь
        assert!(in_window(23, 7, 23));
        assert!(in_window(23, 7, 0));
        assert!(in_window(23, 7, 6));
        assert!(!in_window(23, 7, 12));
        // круглые сутки
        assert!(in_window(5, 5, 13));
    }
}
