//! World state as dense arrays: index = agent ID.
//!
//! Iteration 0 is a placeholder: a day runs as one block so the hash has
//! something to check. From iteration 1 on, the event queue replaces
//! `step_day`.

use crate::hash::{StateHash, StateHasher};
use crate::rng;

/// Game minutes since the world began.
pub type Minute = u64;
pub const MINUTES_PER_DAY: Minute = 24 * 60;

/// Needs live in `0..=NEED_MAX`.
pub const NEED_MAX: u32 = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldConfig {
    pub agents: u32,
    pub start_money: i64,
    /// Hunger increase per game hour.
    pub hunger_per_hour: u32,
    /// Price of one meal.
    pub meal_price: i64,
    /// Daily wage if the agent finds work today.
    pub daily_wage: i64,
    /// Chance of finding work per day, in per mille.
    pub work_chance_permille: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    seed: u64,
    now: Minute,
    config: WorldConfig,
    hunger: Vec<u32>,
    money: Vec<i64>,
    /// Running event number per agent, input to [`rng::roll`].
    event_no: Vec<u64>,
}

impl World {
    pub fn new(seed: u64, config: WorldConfig) -> Self {
        let n = config.agents as usize;
        Self {
            seed,
            now: 0,
            hunger: vec![0; n],
            money: vec![config.start_money; n],
            event_no: vec![0; n],
            config,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn now(&self) -> Minute {
        self.now
    }

    pub fn day(&self) -> u64 {
        self.now / MINUTES_PER_DAY
    }

    pub fn agent_count(&self) -> u32 {
        self.config.agents
    }

    pub fn hunger(&self) -> &[u32] {
        &self.hunger
    }

    pub fn money(&self) -> &[i64] {
        &self.money
    }

    fn next_roll(&mut self, agent: u32, n: u32) -> u32 {
        let i = agent as usize;
        let v = rng::below(self.seed, agent, self.event_no[i], n);
        self.event_no[i] += 1;
        v
    }

    /// Placeholder day: maybe work, get hungry, eat if affordable.
    pub fn step_day(&mut self) {
        let c = self.config.clone();
        for agent in 0..c.agents {
            let i = agent as usize;
            if self.next_roll(agent, 1000) < c.work_chance_permille {
                self.money[i] += c.daily_wage;
            }
            self.hunger[i] = (self.hunger[i] + c.hunger_per_hour * 24).min(NEED_MAX);
            while self.hunger[i] >= NEED_MAX / 2 && self.money[i] >= c.meal_price {
                self.money[i] -= c.meal_price;
                self.hunger[i] -= NEED_MAX / 2;
            }
        }
        self.now += MINUTES_PER_DAY;
    }
}

impl StateHash for World {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.seed);
        h.write_u64(self.now);
        h.write_u32_slice(&self.hunger);
        h.write_i64_slice(&self.money);
        h.write_bytes(&[0xff]);
        for &e in &self.event_no {
            h.write_u64(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WorldConfig {
        WorldConfig {
            agents: 50,
            start_money: 20,
            hunger_per_hour: 20,
            meal_price: 3,
            daily_wage: 8,
            work_chance_permille: 700,
        }
    }

    fn hashes(seed: u64, days: u32) -> Vec<u64> {
        let mut w = World::new(seed, config());
        (0..days)
            .map(|_| {
                w.step_day();
                w.state_hash()
            })
            .collect()
    }

    #[test]
    fn same_seed_same_history() {
        assert_eq!(hashes(1, 365), hashes(1, 365));
    }

    #[test]
    fn different_seed_different_history() {
        assert_ne!(hashes(1, 30).last(), hashes(2, 30).last());
    }

    #[test]
    fn clone_continues_identically() {
        let mut a = World::new(9, config());
        for _ in 0..10 {
            a.step_day();
        }
        let mut b = a.clone();
        for _ in 0..10 {
            a.step_day();
            b.step_day();
        }
        assert_eq!(a, b);
        assert_eq!(a.day(), 20);
    }

    #[test]
    fn needs_stay_in_bounds() {
        let mut w = World::new(3, config());
        for _ in 0..365 {
            w.step_day();
            assert!(w.hunger().iter().all(|&h| h <= NEED_MAX));
        }
    }
}
