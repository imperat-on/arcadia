//! Injectable clock used by sessions and the launch log.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub trait Clock {
    fn now_ms(&self) -> i64;
}

/// Wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0)
    }
}

/// Deterministic clock for tests and state-machine replay.
#[derive(Debug, Default)]
pub struct ManualClock {
    now_ms: Cell<i64>,
}

impl ManualClock {
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: Cell::new(now_ms),
        }
    }

    pub fn set(&self, now_ms: i64) {
        self.now_ms.set(now_ms);
    }

    pub fn advance(&self, delta_ms: i64) {
        self.now_ms.set(self.now_ms.get().saturating_add(delta_ms));
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.now_ms.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now_ms(), 1_500);
        clock.set(7);
        assert_eq!(clock.now_ms(), 7);
    }
}
