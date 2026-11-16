//! Download schedule: hours in which downloading is allowed, and a separate speed in those hours.
//!
//! The window is set by the hours "from from:00 to to:00" and may cross midnight
//! (from 23 to 7). from == to - around the clock.

use crate::settings::Settings;
use crate::LauncherApp;
use chrono::Timelike;

/// Whether hour `hour` is within the window [from, to).
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

/// Is it the schedule window now?
pub fn window_now(s: &Settings) -> bool {
    in_window(s.sched_from, s.sched_to, hour_now())
}

/// Whether downloads may start now.
pub fn may_download(s: &Settings) -> bool {
    !s.sched_only || window_now(s)
}

/// The speed limit in effect now (KB/s, 0 - unlimited).
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
    /// Every 5 s: switch speed by the hour and pause the download outside the schedule window.
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
        // Outside the window: the running install is paused, but not marked as paused in the queue, so
        // it resumes by itself when the window starts.
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
        // across midnight
        assert!(in_window(23, 7, 23));
        assert!(in_window(23, 7, 0));
        assert!(in_window(23, 7, 6));
        assert!(!in_window(23, 7, 12));
        // around the clock
        assert!(in_window(5, 5, 13));
    }
}
