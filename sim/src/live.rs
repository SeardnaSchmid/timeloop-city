//! A run that is advanced piece by piece while someone watches: records a
//! history row per day and a checkpoint per year, so it can jump back.
//! Advancing in pieces gives exactly the same world as one long run.

use std::fmt::Write as _;

use timeloop_core::{Allowed, Minute, StateHash, World, WorldConfig, MINUTES_PER_DAY};

pub const DAYS_PER_YEAR: u64 = timeloop_core::DAYS_PER_YEAR;
pub const YEAR: Minute = DAYS_PER_YEAR * MINUTES_PER_DAY;
/// Values per day in [`Live::history`].
pub const HISTORY_FIELDS: usize = 6;

pub struct Live {
    world: World,
    seed: u64,
    config: WorldConfig,
    end: Minute,
    /// `checkpoints[y]` is the world at the start of year `y`.
    checkpoints: Vec<World>,
    /// Per finished day: employed, homeless, food price, starving ‰, money Gini ‰, open firms.
    history: Vec<i64>,
    starving_before: u64,
    /// Bumped whenever history is cut (rewind, restart), so viewers refetch it.
    epoch: u64,
}

impl Live {
    pub fn new(seed: u64, config: WorldConfig, end: Minute) -> Self {
        let world = World::new(seed, config.clone());
        Self {
            checkpoints: vec![world.clone()],
            world,
            seed,
            config,
            end,
            history: Vec::new(),
            starving_before: 0,
            epoch: 0,
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn end(&self) -> Minute {
        self.end
    }

    /// Starts over from day 0, with a new seed if given.
    pub fn restart(&mut self, seed: Option<u64>) {
        let epoch = self.epoch + 1;
        *self = Self::new(seed.unwrap_or(self.seed), self.config.clone(), self.end);
        self.epoch = epoch;
    }

    /// Runs up to `minutes` further, never past the end.
    pub fn advance(&mut self, minutes: Minute) {
        let target = (self.world.now() + minutes).min(self.end);
        while self.world.now() < target {
            let next_day = (self.world.now() / MINUTES_PER_DAY + 1) * MINUTES_PER_DAY;
            let until = next_day.min(target);
            self.world.run_until(until, &mut |_| {});
            if until == next_day {
                self.record_day();
            }
        }
    }

    fn record_day(&mut self) {
        let w = &self.world;
        let s = w.stats();
        let agent_minutes = (w.agent_count() as u64 * MINUTES_PER_DAY).max(1);
        let starving = (s.starving_minutes_total - self.starving_before) * 1000 / agent_minutes;
        self.starving_before = s.starving_minutes_total;
        self.history.extend([
            s.employed as i64,
            s.homeless as i64,
            food_price(w),
            starving as i64,
            s.money_gini_permille as i64,
            s.firms as i64,
        ]);
        if w.now().is_multiple_of(YEAR) && self.checkpoints.len() as u64 == w.now() / YEAR {
            self.checkpoints.push(w.clone());
        }
    }

    /// If `to` lies in the past, goes back to the year checkpoint at or
    /// before it. Cheap; the way forward from there is up to the caller.
    pub fn rewind(&mut self, to: Minute) {
        if to < self.world.now() {
            let y = ((to / YEAR) as usize).min(self.checkpoints.len() - 1);
            self.world = self.checkpoints[y].clone();
            self.checkpoints.truncate(y + 1);
            self.history
                .truncate(y * DAYS_PER_YEAR as usize * HISTORY_FIELDS);
            self.starving_before = self.world.stats().starving_minutes_total;
            self.epoch += 1;
        }
    }

    /// The fixed parts of the town as JSON: map, buildings, traits.
    pub fn layout_json(&self) -> String {
        let w = &self.world;
        let c = w.config();
        let m = &c.map;
        let mut t = String::new();
        let _ = write!(
            t,
            "{{\"w\":{},\"h\":{},\"landBase\":{},\"landRadius\":{},\"end\":{},\"agents\":{},\
             \"actions\":[\"idle\",\"eat\",\"sleep\",\"work\",\"overtime\",\"wander\",\"travel\"],\
             \"historyFields\":[\"employed\",\"homeless\",\"food price\",\"starving ‰\",\"money gini ‰\",\"firms\"],\
             \"businesses\":[",
            m.width,
            m.height,
            m.land_price_base,
            m.land_price_radius,
            self.end,
            w.agent_count()
        );
        for (i, b) in c.businesses.iter().enumerate() {
            let sep = if i > 0 { "," } else { "" };
            let _ = write!(
                t,
                "{sep}{{\"name\":{},\"good\":{},\"x\":{},\"y\":{},\"slots\":{}}}",
                json_string(&b.name),
                json_string(&c.goods[b.output as usize]),
                b.pos.x,
                b.pos.y,
                b.slots
            );
        }
        t.push_str("],\"houses\":[");
        for (i, h) in c.houses.iter().enumerate() {
            let sep = if i > 0 { "," } else { "" };
            let _ = write!(
                t,
                "{sep}{{\"name\":{},\"x\":{},\"y\":{},\"beds\":{}}}",
                json_string(&h.name),
                h.pos.x,
                h.pos.y,
                h.beds
            );
        }
        let traits: Vec<String> = c.traits.iter().map(|s| json_string(s)).collect();
        let _ = write!(t, "],\"traits\":[{}],\"agentTraits\":[", traits.join(","));
        for a in 0..w.agent_count() {
            let sep = if a > 0 { "," } else { "" };
            let _ = write!(t, "{sep}{:?}", w.traits(a));
        }
        t.push_str("]}");
        t
    }

    /// The world now. Agents are flat, 7 values each: x and y in tenths of a
    /// parcel, action, hunger, money, home and employer (-1 for none).
    /// Businesses come with their state, as firms open and close.
    pub fn state_json(&self) -> String {
        let w = &self.world;
        let s = w.stats();
        let mut t = String::new();
        let _ = write!(
            t,
            "{{\"now\":{},\"hash\":\"{:016x}\",\"seed\":{},\"epoch\":{},\"days\":{},\"employed\":{},\"homeless\":{},\"money\":{},\"agents\":[",
            w.now(),
            w.state_hash(),
            self.seed,
            self.epoch,
            self.history.len() / HISTORY_FIELDS,
            s.employed,
            s.homeless,
            s.money_total
        );
        for a in 0..w.agent_count() {
            let (x, y) = w.position_tenths(a);
            let sep = if a > 0 { "," } else { "" };
            let _ = write!(
                t,
                "{sep}{x},{y},{},{},{},{},{}",
                w.action(a).kind as u8,
                w.hunger(a),
                w.money(a),
                w.home(a).map_or(-1, |h| h as i64),
                w.employer(a).map_or(-1, |b| b as i64)
            );
        }
        let e = w.economy();
        let c = w.config();
        t.push_str("],\"businesses\":[");
        for b in 0..e.len() {
            let sep = if b > 0 { "," } else { "" };
            let bc = e.config(b);
            let _ = write!(
                t,
                "{sep}{{\"name\":{},\"good\":{},\"x\":{},\"y\":{},\"slots\":{},\"open\":{},\
                 \"owner\":{},\"cash\":{},\"price\":{},\"wage\":{},\"workers\":{},\"since\":{}}}",
                json_string(&bc.name),
                json_string(&c.goods[bc.output as usize]),
                bc.pos.x,
                bc.pos.y,
                bc.slots,
                e.is_open(b),
                e.owner(b).map_or(-1, |o| o as i64),
                e.cash(b),
                e.price(b),
                e.wage(b),
                e.workers(b),
                e.opened_day(b)
            );
        }
        let _ = write!(
            t,
            "],\"rent\":{:?},\"tenants\":{:?},\"firms\":{},\"deaths\":{},\"births\":{},\
             \"newcomers\":{},\"meanAge\":{},\"inEducation\":{},\"retired\":{},",
            s.rents,
            s.tenants,
            s.firms,
            s.deaths_total,
            s.births_total,
            s.arrivals_total,
            s.mean_age,
            s.in_education,
            s.retired
        );
        // Per agent: age in years, life stage, parent (-1: none), generation.
        let ages: Vec<u64> = (0..w.agent_count())
            .map(|a| w.age_days(a) / DAYS_PER_YEAR)
            .collect();
        let stages: Vec<u8> = (0..w.agent_count()).map(|a| w.stage(a) as u8).collect();
        let parents: Vec<i64> = (0..w.agent_count())
            .map(|a| w.parent(a).map_or(-1, |p| p as i64))
            .collect();
        let gens: Vec<u32> = (0..w.agent_count()).map(|a| w.generation(a)).collect();
        let traits: Vec<&[u32]> = (0..w.agent_count()).map(|a| w.traits(a)).collect();
        let _ = write!(
            t,
            "\"ages\":{ages:?},\"stages\":{stages:?},\"parents\":{parents:?},\"generations\":{gens:?},\"traits\":{traits:?},\"land\":["
        );
        let m = &w.config().map;
        for y in 0..m.height {
            for x in 0..m.width {
                let sep = if x > 0 || y > 0 { "," } else { "" };
                let _ = write!(t, "{sep}{}", w.land_price(timeloop_core::Pos::new(x, y)));
            }
        }
        t.push_str("]}");
        t
    }

    /// History rows from day `from` on, flat.
    pub fn history_json(&self, from: usize) -> String {
        let rows = self.history.get(from * HISTORY_FIELDS..).unwrap_or(&[]);
        format!(
            "{{\"epoch\":{},\"from\":{from},\"rows\":{rows:?}}}",
            self.epoch
        )
    }

    /// Why `agent` would do what it does right now.
    pub fn explain_json(&self, agent: u32) -> String {
        if agent >= self.world.agent_count() {
            return "null".into();
        }
        let e = self.world.explain(agent);
        let rules = &self.world.config().behaviour.actions;
        let mut t = String::from("{\"vars\":[");
        for (i, (name, v)) in e.vars.iter().enumerate() {
            let sep = if i > 0 { "," } else { "" };
            let _ = write!(t, "{sep}[{},{v}]", json_string(name));
        }
        t.push_str("],\"candidates\":[");
        for (i, (c, rule)) in e.candidates.iter().zip(rules).enumerate() {
            let sep = if i > 0 { "," } else { "" };
            let allowed = match c.allowed {
                Allowed::Yes => "allowed",
                Allowed::NotPossible => "not possible",
                Allowed::RuleSaysNo => "rule says no",
            };
            let _ = write!(
                t,
                "{sep}{{\"kind\":{},\"score\":{},\"allowed\":\"{allowed}\",\"formula\":{}}}",
                json_string(c.kind.name()),
                c.score,
                json_string(rule.score.source())
            );
        }
        let _ = write!(t, "],\"winner\":{}}}", json_string(e.winner.name()));
        t
    }
}

fn food_price(w: &World) -> i64 {
    let food = w.config().food;
    (0..w.economy().len())
        .filter(|&b| w.economy().config(b).output == food)
        .map(|b| w.economy().price(b))
        .min()
        .unwrap_or(0)
}

pub fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> Live {
        Live::new(1, timeloop_content::default_world(), 3 * YEAR)
    }

    #[test]
    fn advancing_in_pieces_equals_one_run() {
        let mut l = live();
        for m in [1, 59, 1440, 7, 100_000, 3] {
            l.advance(m);
        }
        let mut w = World::new(1, timeloop_content::default_world());
        w.run_until(l.world().now(), &mut |_| {});
        assert_eq!(l.world().state_hash(), w.state_hash());
    }

    #[test]
    fn stops_at_the_end_and_records_every_day() {
        let mut l = live();
        l.advance(10 * YEAR);
        assert_eq!(l.world().now(), 3 * YEAR);
        assert_eq!(l.history.len(), 3 * 365 * HISTORY_FIELDS);
        assert_eq!(l.checkpoints.len(), 4);
    }

    #[test]
    fn seeking_back_replays_identically() {
        let mut l = live();
        l.advance(2 * YEAR + 500);
        let ahead = l.world().state_hash();
        l.rewind(YEAR + 20);
        l.advance(YEAR + 20 - l.world().now());
        assert_eq!(l.world().now(), YEAR + 20);
        assert_eq!(l.epoch, 1);
        assert_eq!(l.history.len(), 365 * HISTORY_FIELDS);
        l.advance(2 * YEAR + 500 - l.world().now());
        assert_eq!(l.world().state_hash(), ahead);
    }

    #[test]
    fn restart_begins_again() {
        let mut l = live();
        l.advance(YEAR);
        l.restart(Some(7));
        assert_eq!((l.world().now(), l.seed, l.epoch), (0, 7, 1));
        assert!(l.history.is_empty());
    }

    #[test]
    fn json_has_the_expected_shape() {
        let mut l = live();
        l.advance(MINUTES_PER_DAY + 600);
        assert!(l.layout_json().starts_with("{\"w\":24,"));
        assert!(l.state_json().contains("\"days\":1,"));
        assert!(l.history_json(0).contains("\"rows\":["));
        assert_eq!(l.history_json(99), "{\"epoch\":0,\"from\":99,\"rows\":[]}");
        assert!(l.explain_json(3).contains("\"winner\":"));
        assert_eq!(l.explain_json(999), "null");
    }
}
