//! A run that is advanced piece by piece while someone watches: records a
//! history row per day and a checkpoint per year, so it can jump back.
//! Advancing in pieces gives exactly the same world as one long run.

use std::fmt::Write as _;

use timeloop_core::{Allowed, Minute, StateHash, World, WorldConfig, MINUTES_PER_DAY};

pub const DAYS_PER_YEAR: u64 = timeloop_core::DAYS_PER_YEAR;
pub const YEAR: Minute = DAYS_PER_YEAR * MINUTES_PER_DAY;
/// Values per day in [`Live::history`].
pub const HISTORY_FIELDS: usize = 7;

pub struct Live {
    world: World,
    seed: u64,
    config: WorldConfig,
    end: Minute,
    /// `checkpoints[y]` is the world at the start of year `y`.
    checkpoints: Vec<World>,
    /// Per finished day: employed, homeless, food price, starving ‰, money Gini ‰, open firms, price index.
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
            s.price_index,
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
             \"historyFields\":[\"employed\",\"homeless\",\"food price\",\"starving ‰\",\"money gini ‰\",\"firms\",\"price index\"],\
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
        // Houses as agents build them: owner -1 is the town.
        t.push_str("],\"houses\":[");
        let hs = w.housing();
        for h in 0..hs.len() {
            let sep = if h > 0 { "," } else { "" };
            let hc = hs.config(h);
            let _ = write!(
                t,
                "{sep}{{\"name\":{},\"x\":{},\"y\":{},\"beds\":{},\"owner\":{},\"building\":{}}}",
                json_string(&hc.name),
                hc.pos.x,
                hc.pos.y,
                hc.beds,
                hs.owner(h).map_or(-1, |o| o as i64),
                hs.under_construction(h)
            );
        }
        let _ = write!(
            t,
            "],\"population\":{},\"immigrants\":{},\"housesBuilt\":{},\"comforts\":{},\"beds\":{}",
            w.agent_count(),
            s.immigrants_total,
            s.houses_built_total,
            s.comforts_total,
            s.beds
        );
        let _ = write!(
            t,
            ",\"rent\":{:?},\"tenants\":{:?},\"firms\":{},\"deaths\":{},\"births\":{},\
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
        // Level in the profession of the agent's job (-1: no job).
        let levels: Vec<i64> = (0..w.agent_count())
            .map(|a| {
                w.employer(a)
                    .map_or(-1, |b| w.level(a, w.economy().config(b).output) as i64)
            })
            .collect();
        let _ = write!(
            t,
            "\"levels\":{levels:?},\"meanLevel\":{},\"priceIndex\":{},\"associations\":[",
            s.mean_level_tenths as f64 / 10.0,
            s.price_index
        );
        for (a, ac) in c.associations.iter().enumerate() {
            let sep = if a > 0 { "," } else { "" };
            let values = w.association_rule_values(a as u32);
            let rules: Vec<String> = (ac.rules.iter().zip(values))
                .map(|(r, v)| format!("[{},{v}]", json_string(r.kind.name())))
                .collect();
            let _ = write!(
                t,
                "{sep}{{\"name\":{},\"cash\":{},\"members\":{},\"rules\":[{}]}}",
                json_string(&ac.name),
                s.assoc_cash[a],
                w.association_members(a as u32),
                rules.join(",")
            );
        }
        t.push_str("],");
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

/// The rules of the running world as JSON, for the rules page: goods,
/// who makes them from what, which needs they cover, and every decision
/// rule with its formulas.
pub fn rules_json(w: &World) -> String {
    let c = w.config();
    let b = &c.behaviour;
    let s = json_string;
    let f =
        |x: &Option<timeloop_core::Formula>| x.as_ref().map_or("null".into(), |x| s(x.source()));
    let mut t = String::from("{\"goods\":[");
    for (i, g) in c.goods.iter().enumerate() {
        let _ = write!(t, "{}{}", if i > 0 { "," } else { "" }, s(g));
    }
    let _ = write!(
        t,
        "],\"food\":{},\"meal_relief\":{},\"comfort\":[",
        c.food, c.meal_relief
    );
    for (i, (g, r)) in c.comfort_goods.iter().enumerate() {
        let _ = write!(t, "{}[{g},{r}]", if i > 0 { "," } else { "" });
    }
    t.push_str("],\"makers\":[");
    let makers = c
        .businesses
        .iter()
        .map(|m| (m, true))
        .chain(c.recipes.iter().map(|m| (m, false)));
    for (i, (m, town)) in makers.enumerate() {
        let input = m
            .input
            .as_ref()
            .map_or("null".into(), |x| format!("[{},{}]", x.good, x.amount));
        let _ = write!(
            t,
            "{}{{\"name\":{},\"town\":{town},\"output\":{},\"input\":{input},\"per_hour_milli\":{},\"slots\":{},\"price\":{},\"wage\":{},\"cash\":{}}}",
            if i > 0 { "," } else { "" },
            s(&m.name),
            m.output,
            m.output_milli_per_worker_hour,
            m.slots,
            m.start_price,
            m.start_wage,
            m.start_cash
        );
    }
    t.push_str("],\"actions\":[");
    for (i, a) in b.actions.iter().enumerate() {
        let _ = write!(
            t,
            "{}{{\"kind\":{},\"when\":{},\"score\":{}}}",
            if i > 0 { "," } else { "" },
            s(a.kind.name()),
            f(&a.when),
            s(a.score.source())
        );
    }
    let _ = write!(
        t,
        "],\"needs\":{{\"hunger\":{{\"per_hour\":{},\"critical\":{},\"meal_relief\":{},\"meal_minutes\":{}}},\"fatigue\":{{\"per_hour\":{},\"recovery_per_hour\":{},\"max_sleep_minutes\":{}}},\"safety\":{{\"reserve_days\":{},\"meals_per_day\":{}}},\"comfort\":{{\"per_hour\":{},\"buy_at\":{},\"spend_permille\":{}}}}}",
        c.hunger_per_hour,
        c.hunger_critical,
        c.meal_relief,
        c.meal_minutes,
        c.fatigue_per_hour,
        c.sleep_recovery_per_hour,
        c.max_sleep_minutes,
        b.safety_reserve_days,
        b.meals_per_day,
        b.comfort_per_hour,
        b.comfort_buy_at,
        b.comfort_spend_permille
    );
    let h = &b.housing;
    let _ = write!(
        t,
        ",\"housing\":{{\"when\":{},\"score\":{},\"homeless_score\":{}}}",
        f(&h.when),
        s(h.score.source()),
        s(h.homeless_score.source())
    );
    match &b.founding {
        Some(r) => {
            let _ = write!(
                t,
                ",\"founding\":{{\"when\":{},\"score\":{},\"close\":{},\"capital\":{},\"profit_tax_percent\":{},\"max_firms\":{}}}",
                f(&r.when),
                s(r.score.source()),
                s(r.close_when.source()),
                r.capital,
                r.profit_tax_percent,
                r.max_firms
            );
        }
        None => t.push_str(",\"founding\":null"),
    }
    match &b.building {
        Some(r) => {
            let _ = write!(
                t,
                ",\"building\":{{\"when\":{},\"score\":{},\"cost\":{},\"beds\":{},\"rent\":{},\"construction_days\":{},\"max_houses\":{}}}",
                f(&r.when),
                s(r.score.source()),
                r.cost,
                r.beds,
                r.start_rent,
                r.construction_days,
                r.max_houses
            );
        }
        None => t.push_str(",\"building\":null"),
    }
    let m = &c.market;
    let _ = write!(
        t,
        ",\"market\":{{\"price_change_percent\":{},\"price_min\":{},\"wage_step\":{},\"wage_min\":{},\"reserve_days\":{}}}",
        m.price_change_percent, m.price_min, m.wage_step, m.wage_min, m.reserve_days
    );
    match &c.growth {
        Some(g) => {
            let _ = write!(
                t,
                ",\"growth\":{{\"max_agents\":{},\"arrivals_per_day\":{}}}",
                g.max_agents, g.arrivals_per_day
            );
        }
        None => t.push_str(",\"growth\":null"),
    }
    let _ = write!(
        t,
        ",\"agents\":{},\"start_money\":{},\"traits\":[",
        c.agents, c.start_money
    );
    for (i, x) in c.traits.iter().enumerate() {
        let _ = write!(t, "{}{}", if i > 0 { "," } else { "" }, s(x));
    }
    t.push_str("]}");
    t
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
