//! Light mode for weak PCs: less background work, not just fewer sections.
//! Repaints less often when idle, polls the network less often, skips optional loads
//! (news shelf, friends activity on the game page) and turns animations off.
//! The flag is read from background threads too, so it lives in an atomic.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static ON: AtomicBool = AtomicBool::new(false);

pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// `normal` in the usual mode, `light` in light mode.
pub fn pick(normal: Duration, light: Duration) -> Duration {
    if on() { light } else { normal }
}

/// Seconds variant of `pick`.
pub fn secs(normal: u64, light: u64) -> Duration {
    Duration::from_secs(if on() { light } else { normal })
}

#[cfg(test)]
mod tests {
    #[test]
    fn pick_follows_flag() {
        super::set(true);
        assert_eq!(super::secs(4, 15).as_secs(), 15);
        super::set(false);
        assert_eq!(super::secs(4, 15).as_secs(), 4);
    }
}
