//! Life course: age, education, retirement, death, inheritance, and new
//! people taking the place of the dead.
//!
//! The number of agents stays fixed: a person who dies frees the slot (the
//! agent ID) and the same day someone new takes it, either a descendant of a
//! living agent or a newcomer to town. Childhood is not simulated; new people
//! enter at `entry_age` with no money, so the money loop stays closed.

use crate::rng;

pub const DAYS_PER_YEAR: u64 = 365;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifeConfig {
    /// Ages of the first inhabitants are spread evenly over this range, in years.
    pub start_age_min: u32,
    pub start_age_max: u32,
    /// Age at which new people enter the world.
    pub entry_age: u32,
    /// Until this age people are in education and take no job.
    pub education_end_age: u32,
    /// From this age people stop working.
    pub retirement_age: u32,
    /// Each person's lifespan is rolled once, evenly in this range, in years.
    pub lifespan_min: u32,
    pub lifespan_max: u32,
    /// Chance in percent that a free place goes to a descendant of a living
    /// agent (aged `parent_age_min..=parent_age_max`) instead of a newcomer.
    pub birth_percent: u32,
    pub parent_age_min: u32,
    pub parent_age_max: u32,
    /// Percent of every wage paid into the pension fund, which is shared
    /// out daily in equal parts to everyone in education or retired.
    pub pension_percent: u32,
    /// Percent of an estate (money) the town takes for welfare before the
    /// children share the rest.
    pub inheritance_tax_percent: u32,
}

/// Where in life an agent is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Education,
    Working,
    Retired,
}

impl LifeConfig {
    pub fn stage(&self, age_days: u64) -> Stage {
        let years = age_days / DAYS_PER_YEAR;
        if years < self.education_end_age as u64 {
            Stage::Education
        } else if years >= self.retirement_age as u64 {
            Stage::Retired
        } else {
            Stage::Working
        }
    }
}

/// Salt for life rolls, so they never use up an agent's roll numbers.
const LIFE_SALT: u64 = 0x6c69_6665_0000_0001;

/// Roll `k` for the person in slot `agent`, generation `gen`: uniform in `lo..=hi`.
pub(crate) fn roll(seed: u64, agent: u32, gen: u32, k: u64, lo: u64, hi: u64) -> u64 {
    let span = (hi.saturating_sub(lo) + 1).min(u32::MAX as u64) as u32;
    lo + rng::below(seed ^ LIFE_SALT, agent, ((gen as u64) << 8) | k, span) as u64
}

/// Lifespan in days, rolled for one person; always beyond `age_days`.
pub(crate) fn lifespan(c: &LifeConfig, seed: u64, agent: u32, gen: u32, age_days: u64) -> u64 {
    let (lo, hi) = (
        c.lifespan_min as u64 * DAYS_PER_YEAR,
        c.lifespan_max as u64 * DAYS_PER_YEAR,
    );
    let days = roll(seed, agent, gen, 1, lo, hi.max(lo));
    if days > age_days {
        days
    } else {
        // Older than the range at the start: a few more years.
        age_days + roll(seed, agent, gen, 2, 1, 5 * DAYS_PER_YEAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LifeConfig {
        LifeConfig {
            start_age_min: 16,
            start_age_max: 70,
            entry_age: 16,
            education_end_age: 18,
            retirement_age: 65,
            lifespan_min: 70,
            lifespan_max: 90,
            birth_percent: 70,
            parent_age_min: 20,
            parent_age_max: 50,
            pension_percent: 20,
            inheritance_tax_percent: 50,
        }
    }

    #[test]
    fn stages_follow_age() {
        let c = config();
        let y = DAYS_PER_YEAR;
        assert_eq!(c.stage(17 * y + 364), Stage::Education);
        assert_eq!(c.stage(18 * y), Stage::Working);
        assert_eq!(c.stage(64 * y + 364), Stage::Working);
        assert_eq!(c.stage(65 * y), Stage::Retired);
    }

    #[test]
    fn lifespans_stay_in_range_and_beyond_age() {
        let c = config();
        let y = DAYS_PER_YEAR;
        for a in 0..1000 {
            let l = lifespan(&c, 1, a, 0, 20 * y);
            assert!((70 * y..=90 * y).contains(&l));
            assert!(lifespan(&c, 1, a, 0, 95 * y) > 95 * y);
        }
    }
}
