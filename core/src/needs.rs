//! Lazy needs: stored as `base + rate × (now − since)`, never ticked.
//!
//! A need only changes in memory when something happens to it (eating,
//! sleeping). In between, its value is computed on demand, so fast-forwarding
//! costs nothing.

use crate::hash::StateHasher;
use crate::world::Minute;

/// Needs live in `0..=NEED_MAX`.
pub const NEED_MAX: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LazyNeed {
    base: u32,
    since: Minute,
}

impl LazyNeed {
    pub fn new(value: u32, now: Minute) -> Self {
        Self {
            base: value.min(NEED_MAX),
            since: now,
        }
    }

    /// Value at `now`, growing by `per_hour` and capped at [`NEED_MAX`].
    pub fn value(&self, now: Minute, per_hour: u32) -> u32 {
        debug_assert!(now >= self.since, "time went backwards");
        let grown = per_hour as u64 * (now - self.since) / 60;
        (self.base as u64 + grown).min(NEED_MAX as u64) as u32
    }

    pub fn set(&mut self, now: Minute, value: u32) {
        *self = Self::new(value, now);
    }

    /// First minute at which the value is `>= threshold`, or `None` if it never gets there.
    pub fn reaches(&self, threshold: u32, per_hour: u32) -> Option<Minute> {
        if self.base >= threshold {
            return Some(self.since);
        }
        if per_hour == 0 || threshold > NEED_MAX {
            return None;
        }
        let missing = (threshold - self.base) as u64 * 60;
        Some(self.since + missing.div_ceil(per_hour as u64))
    }

    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_u32(self.base);
        h.write_u64(self.since);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_linearly_and_caps() {
        let n = LazyNeed::new(100, 60);
        assert_eq!(n.value(60, 30), 100);
        assert_eq!(n.value(120, 30), 130);
        assert_eq!(n.value(90, 30), 115);
        assert_eq!(n.value(1_000_000, 30), NEED_MAX);
    }

    #[test]
    fn reaches_is_the_first_matching_minute() {
        let n = LazyNeed::new(0, 0);
        let t = n.reaches(500, 40).unwrap();
        assert!(n.value(t, 40) >= 500);
        assert!(n.value(t - 1, 40) < 500);
    }

    #[test]
    fn reaches_edge_cases() {
        assert_eq!(LazyNeed::new(600, 7).reaches(500, 40), Some(7));
        assert_eq!(LazyNeed::new(0, 0).reaches(500, 0), None);
    }
}
