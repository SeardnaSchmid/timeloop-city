//! Professions with levels, after Ages of Echo: every kind of business is a
//! profession (named after the good it makes). Work gives experience in it;
//! each level needs a fixed share more experience than the one before.
//! Levels raise how much a worker makes per hour and what they are paid.

/// How experience turns into levels and what levels are worth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CareerConfig {
    /// Experience per hour worked.
    pub xp_per_hour: u32,
    /// Experience from level 0 to 1.
    pub level_xp_base: u32,
    /// Each further level needs this many percent more than the last.
    pub level_xp_growth_percent: u32,
    /// Output per level, in thousandths (20 = +2 % a level).
    pub output_per_level_permille: u32,
    /// Wage per level, in thousandths.
    pub wage_per_level_permille: u32,
    pub max_level: u32,
}

impl CareerConfig {
    /// `t[l]` = total experience needed for level `l` (`t[0]` = 0).
    pub fn thresholds(&self) -> Vec<u64> {
        let mut t = vec![0u64];
        let mut step = self.level_xp_base.max(1) as u64;
        for _ in 0..self.max_level {
            let last = *t.last().expect("starts with 0");
            t.push(last.saturating_add(step));
            step = step.saturating_mul(100 + self.level_xp_growth_percent as u64) / 100;
        }
        t
    }

    pub fn output_permille(&self, level: u32) -> u64 {
        1000 + level as u64 * self.output_per_level_permille as u64
    }

    pub fn wage_permille(&self, level: u32) -> u64 {
        1000 + level as u64 * self.wage_per_level_permille as u64
    }
}

/// Level reached with `xp`, given [`CareerConfig::thresholds`].
pub fn level(thresholds: &[u64], xp: u64) -> u32 {
    (thresholds.partition_point(|&t| t <= xp) - 1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CareerConfig {
        CareerConfig {
            xp_per_hour: 1,
            level_xp_base: 50,
            level_xp_growth_percent: 6,
            output_per_level_permille: 20,
            wage_per_level_permille: 20,
            max_level: 100,
        }
    }

    #[test]
    fn each_level_needs_more() {
        let t = config().thresholds();
        assert_eq!(t.len(), 101);
        assert_eq!(&t[..4], [0, 50, 103, 159]);
        assert!(t.windows(3).all(|w| w[2] - w[1] >= w[1] - w[0]));
    }

    #[test]
    fn levels_from_experience() {
        let t = config().thresholds();
        assert_eq!(level(&t, 0), 0);
        assert_eq!(level(&t, 49), 0);
        assert_eq!(level(&t, 50), 1);
        assert_eq!(level(&t, 158), 2);
        assert_eq!(level(&t, u64::MAX), 100);
    }

    #[test]
    fn levels_pay_and_produce_more() {
        let c = config();
        assert_eq!(c.output_permille(0), 1000);
        assert_eq!(c.output_permille(25), 1500);
        assert_eq!(c.wage_permille(10), 1200);
    }
}
