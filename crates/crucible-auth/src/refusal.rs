use std::time::{Duration, SystemTime};

/// A digest refused without a store read from `at` for `lasts`. The entry
/// outlives its window so the next refusal of the same digest can double it.
pub(crate) struct Refusal {
    pub at: SystemTime,
    pub lasts: Duration,
}

impl Refusal {
    pub fn active(&self, now: SystemTime) -> bool {
        within(now, self.at, self.lasts)
    }
}

/// `start <= now < start + length`, without adding to a `SystemTime` (which can
/// overflow). A clock that moved backwards before `start` counts as outside,
/// so a cached answer is re-read rather than kept.
pub(crate) fn within(now: SystemTime, start: SystemTime, length: Duration) -> bool {
    now.duration_since(start)
        .is_ok_and(|elapsed| elapsed < length)
}
