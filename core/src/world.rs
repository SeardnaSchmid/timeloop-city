//! Event-driven world. State lives in dense arrays (index = agent ID).
//!
//! An agent only computes when its `Wake` event fires: it settles the action
//! it was doing, picks the next one and schedules its next wake-up. Between
//! wake-ups nothing is ticked; needs grow lazily.

use std::fmt;

use crate::hash::{StateHash, StateHasher};
use crate::needs::LazyNeed;
use crate::queue::{Event, EventQueue};
use crate::rng;

/// Game minutes since the world began.
pub type Minute = u64;
pub const MINUTES_PER_HOUR: Minute = 60;
pub const MINUTES_PER_DAY: Minute = 24 * MINUTES_PER_HOUR;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldConfig {
    pub agents: u32,
    pub start_money: i64,

    pub hunger_per_hour: u32,
    /// Agents eat once hunger reaches this (if they can pay).
    pub hunger_eat_at: u32,
    /// Hunger at which a running action is interrupted to go eat.
    pub hunger_critical: u32,

    pub fatigue_per_hour: u32,
    /// Agents go to sleep at any hour once fatigue reaches this.
    pub fatigue_sleep_at: u32,
    pub sleep_recovery_per_hour: u32,

    pub meal_price: i64,
    pub meal_relief: u32,
    pub meal_minutes: Minute,

    pub hourly_wage: i64,
    pub work_start_hour: u32,
    pub work_end_hour: u32,

    pub wander_min_minutes: u32,
    pub wander_max_minutes: u32,
}

/// Fatigue above which agents go to bed when it is night.
const NIGHT_SLEEP_AT: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Idle,
    Eat,
    Sleep,
    Work,
    Wander,
}

impl ActionKind {
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Idle => "idle",
            ActionKind::Eat => "eat",
            ActionKind::Sleep => "sleep",
            ActionKind::Work => "work",
            ActionKind::Wander => "wander",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub started: Minute,
    pub ends: Minute,
    /// Fatigue when the action started; sleep recovers from this value.
    pub start_fatigue: u32,
}

impl Action {
    const IDLE: Action = Action {
        kind: ActionKind::Idle,
        started: 0,
        ends: 0,
        start_fatigue: 0,
    };
}

/// One line of the event log: what happened, and the agent's state right after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogEntry {
    DayStart {
        at: Minute,
    },
    Started {
        at: Minute,
        agent: u32,
        action: Action,
        interrupted: Option<ActionKind>,
        hunger: u32,
        fatigue: u32,
        money: i64,
    },
}

fn clock(at: Minute) -> String {
    let day = at / MINUTES_PER_DAY;
    let m = at % MINUTES_PER_DAY;
    format!("d{day:04} {:02}:{:02}", m / 60, m % 60)
}

impl fmt::Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            LogEntry::DayStart { at } => write!(f, "{} -- day starts", clock(at)),
            LogEntry::Started {
                at,
                agent,
                action,
                interrupted,
                hunger,
                fatigue,
                money,
            } => {
                write!(
                    f,
                    "{} a{agent} {:<6} until {} h={hunger:>4} f={fatigue:>4} m={money}",
                    clock(at),
                    action.kind.name(),
                    clock(action.ends),
                )?;
                if let Some(kind) = interrupted {
                    write!(f, " (interrupted {})", kind.name())?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct World {
    seed: u64,
    now: Minute,
    config: WorldConfig,
    queue: EventQueue,
    hunger: Vec<LazyNeed>,
    fatigue: Vec<LazyNeed>,
    money: Vec<i64>,
    action: Vec<Action>,
    /// Running roll number per agent, input to [`rng::roll`].
    event_no: Vec<u64>,
}

impl World {
    pub fn new(seed: u64, config: WorldConfig) -> Self {
        let n = config.agents as usize;
        let mut queue = EventQueue::default();
        queue.push(0, Event::DayStart);
        for agent in 0..config.agents {
            queue.push(0, Event::Wake { agent });
        }
        Self {
            seed,
            now: 0,
            queue,
            hunger: vec![LazyNeed::new(0, 0); n],
            fatigue: vec![LazyNeed::new(0, 0); n],
            money: vec![config.start_money; n],
            action: vec![Action::IDLE; n],
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

    pub fn pending_events(&self) -> usize {
        self.queue.len()
    }

    pub fn hunger(&self, agent: u32) -> u32 {
        self.hunger[agent as usize].value(self.now, self.config.hunger_per_hour)
    }

    pub fn fatigue(&self, agent: u32) -> u32 {
        let i = agent as usize;
        if self.action[i].kind == ActionKind::Sleep {
            self.sleep_fatigue(i, self.now)
        } else {
            self.fatigue[i].value(self.now, self.config.fatigue_per_hour)
        }
    }

    pub fn money(&self, agent: u32) -> i64 {
        self.money[agent as usize]
    }

    pub fn action(&self, agent: u32) -> Action {
        self.action[agent as usize]
    }

    /// Processes every event due up to and including `until`, then sets the clock to `until`.
    pub fn run_until(&mut self, until: Minute, log: &mut impl FnMut(LogEntry)) {
        assert!(until >= self.now, "cannot run backwards");
        while let Some(s) = self.queue.pop_due(until) {
            self.now = s.at;
            match s.event {
                Event::DayStart => {
                    self.queue.push(s.at + MINUTES_PER_DAY, Event::DayStart);
                    log(LogEntry::DayStart { at: s.at });
                }
                Event::Wake { agent } => log(self.wake(agent)),
            }
        }
        self.now = until;
    }

    /// Runs to the end of the given number of days from now, without logging.
    pub fn run_days(&mut self, days: u64) {
        let until = self.now + days * MINUTES_PER_DAY;
        self.run_until(until, &mut |_| {});
    }

    fn next_roll(&mut self, agent: u32, n: u32) -> u32 {
        let i = agent as usize;
        let v = rng::below(self.seed, agent, self.event_no[i], n);
        self.event_no[i] += 1;
        v
    }

    fn sleep_fatigue(&self, i: usize, now: Minute) -> u32 {
        let a = self.action[i];
        let slept = now - a.started;
        let recovered = self.config.sleep_recovery_per_hour as u64 * slept / 60;
        (a.start_fatigue as u64).saturating_sub(recovered) as u32
    }

    fn wake(&mut self, agent: u32) -> LogEntry {
        let i = agent as usize;
        let now = self.now;
        let previous = self.action[i];
        let interrupted =
            (previous.kind != ActionKind::Idle && now < previous.ends).then_some(previous.kind);

        self.settle(i, now);
        let action = self.decide(agent, now);
        if action.kind == ActionKind::Eat {
            self.money[i] -= self.config.meal_price;
        }
        self.action[i] = action;
        let wake_at = self.wake_time(i, action);
        self.queue.push(wake_at, Event::Wake { agent });

        LogEntry::Started {
            at: now,
            agent,
            action,
            interrupted,
            hunger: self.hunger[i].value(now, self.config.hunger_per_hour),
            fatigue: self.fatigue[i].value(now, self.config.fatigue_per_hour),
            money: self.money[i],
        }
    }

    /// Applies the effects of the action that just ended (fully or partly).
    fn settle(&mut self, i: usize, now: Minute) {
        let c = &self.config;
        let a = self.action[i];
        let elapsed = now - a.started;
        match a.kind {
            ActionKind::Idle | ActionKind::Wander => {}
            ActionKind::Eat => {
                let h = self.hunger[i].value(now, c.hunger_per_hour);
                self.hunger[i].set(now, h.saturating_sub(c.meal_relief));
            }
            ActionKind::Work => {
                self.money[i] += c.hourly_wage * elapsed as i64 / 60;
            }
            ActionKind::Sleep => {
                let f = self.sleep_fatigue(i, now);
                self.fatigue[i].set(now, f);
            }
        }
    }

    fn decide(&mut self, agent: u32, now: Minute) -> Action {
        let i = agent as usize;
        let c = &self.config;
        let hunger = self.hunger[i].value(now, c.hunger_per_hour);
        let fatigue = self.fatigue[i].value(now, c.fatigue_per_hour);
        let minute_of_day = now % MINUTES_PER_DAY;
        let hour = (minute_of_day / 60) as u32;
        let night = !(6..22).contains(&hour);
        let work_start = c.work_start_hour as Minute * 60;
        let work_end = c.work_end_hour as Minute * 60;

        let (kind, minutes) = if hunger >= c.hunger_eat_at && self.money[i] >= c.meal_price {
            (ActionKind::Eat, c.meal_minutes)
        } else if fatigue >= c.fatigue_sleep_at || (night && fatigue >= NIGHT_SLEEP_AT) {
            let minutes = (fatigue as u64 * 60).div_ceil(c.sleep_recovery_per_hour as u64);
            (ActionKind::Sleep, minutes.max(30))
        } else if (c.work_start_hour..c.work_end_hour).contains(&hour) {
            (ActionKind::Work, work_end - minute_of_day)
        } else {
            let (lo, hi) = (c.wander_min_minutes, c.wander_max_minutes);
            let minutes = (lo + self.next_roll(agent, hi - lo + 1)) as Minute;
            // Never wander past the start of the work day.
            let until_work = work_start.saturating_sub(minute_of_day);
            let minutes = if until_work > 0 {
                minutes.min(until_work)
            } else {
                minutes
            };
            (ActionKind::Wander, minutes)
        };

        Action {
            kind,
            started: now,
            ends: now + minutes,
            start_fatigue: fatigue,
        }
    }

    /// Normally the action's end. Earlier if hunger turns critical first and
    /// the agent could actually go and eat.
    fn wake_time(&self, i: usize, action: Action) -> Minute {
        let c = &self.config;
        if action.kind == ActionKind::Eat || self.money[i] < c.meal_price {
            return action.ends;
        }
        match self.hunger[i].reaches(c.hunger_critical, c.hunger_per_hour) {
            Some(t) if t > action.started && t < action.ends => t,
            _ => action.ends,
        }
    }
}

impl StateHash for World {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.seed);
        h.write_u64(self.now);
        self.queue.hash_into(h);
        h.write_u64(self.hunger.len() as u64);
        self.hunger.iter().for_each(|n| n.hash_into(h));
        self.fatigue.iter().for_each(|n| n.hash_into(h));
        h.write_i64_slice(&self.money);
        for a in &self.action {
            h.write_bytes(&[a.kind as u8]);
            h.write_u64(a.started);
            h.write_u64(a.ends);
            h.write_u32(a.start_fatigue);
        }
        self.event_no.iter().for_each(|&e| h.write_u64(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::needs::NEED_MAX;

    fn config() -> WorldConfig {
        WorldConfig {
            agents: 20,
            start_money: 20,
            hunger_per_hour: 40,
            hunger_eat_at: 300,
            hunger_critical: 800,
            fatigue_per_hour: 40,
            fatigue_sleep_at: 700,
            sleep_recovery_per_hour: 80,
            meal_price: 3,
            meal_relief: 300,
            meal_minutes: 30,
            hourly_wage: 2,
            work_start_hour: 8,
            work_end_hour: 18,
            wander_min_minutes: 30,
            wander_max_minutes: 120,
        }
    }

    fn day_hashes(seed: u64, days: u64) -> Vec<u64> {
        let mut w = World::new(seed, config());
        (0..days)
            .map(|_| {
                w.run_days(1);
                w.state_hash()
            })
            .collect()
    }

    fn log_lines(w: &mut World, until: Minute) -> Vec<String> {
        let mut lines = Vec::new();
        w.run_until(until, &mut |e| lines.push(e.to_string()));
        lines
    }

    #[test]
    fn same_seed_same_history() {
        assert_eq!(day_hashes(1, 365), day_hashes(1, 365));
    }

    #[test]
    fn different_seed_different_history() {
        assert_ne!(day_hashes(1, 30).last(), day_hashes(2, 30).last());
    }

    #[test]
    fn clone_continues_identically() {
        let mut a = World::new(9, config());
        a.run_days(10);
        let mut b = a.clone();
        a.run_days(10);
        b.run_days(10);
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.day(), 20);
    }

    #[test]
    fn running_in_chunks_equals_running_at_once() {
        let mut once = World::new(4, config());
        once.run_until(3 * MINUTES_PER_DAY + 17, &mut |_| {});
        let mut chunks = World::new(4, config());
        for until in [1, 59, 600, 601, 2000, 3 * MINUTES_PER_DAY + 17] {
            chunks.run_until(until, &mut |_| {});
        }
        assert_eq!(once.state_hash(), chunks.state_hash());
    }

    #[test]
    fn one_wake_per_agent_plus_day_start() {
        let mut w = World::new(1, config());
        w.run_days(5);
        assert_eq!(w.pending_events(), w.agent_count() as usize + 1);
    }

    #[test]
    fn needs_stay_in_bounds_and_agents_eat() {
        let mut w = World::new(3, config());
        let mut meals = 0;
        w.run_until(30 * MINUTES_PER_DAY, &mut |e| {
            if let LogEntry::Started {
                action,
                hunger,
                fatigue,
                ..
            } = e
            {
                assert!(hunger <= NEED_MAX && fatigue <= NEED_MAX);
                meals += (action.kind == ActionKind::Eat) as u32;
            }
        });
        // 20 agents, 30 days: roughly three meals a day each.
        assert!((1200..2400).contains(&meals), "meals = {meals}");
    }

    #[test]
    fn critical_hunger_interrupts_work() {
        let mut c = config();
        c.agents = 1;
        c.hunger_eat_at = 900;
        c.hunger_critical = 500;
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 2 * MINUTES_PER_DAY);
        assert!(
            lines.iter().any(|l| l.contains("(interrupted work)")),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn agents_are_at_work_from_work_start() {
        let mut w = World::new(5, config());
        let start = config().work_start_hour as Minute * 60;
        for day in 0..30 {
            // A minute after work start: nobody wanders, the rested ones work.
            w.run_until(day * MINUTES_PER_DAY + start + 1, &mut |_| {});
            for agent in 0..w.agent_count() {
                assert_ne!(
                    w.action(agent).kind,
                    ActionKind::Wander,
                    "day {day} a{agent}"
                );
            }
        }
    }

    #[test]
    fn broke_agents_are_not_interrupted() {
        let mut c = config();
        c.agents = 1;
        c.start_money = 0;
        c.hourly_wage = 0;
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 3 * MINUTES_PER_DAY);
        assert!(lines.iter().all(|l| !l.contains("interrupted")));
        assert!(lines.iter().all(|l| !l.contains(" eat ")));
    }
}
