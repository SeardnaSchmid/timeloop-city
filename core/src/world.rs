//! Event-driven world. State lives in dense arrays (index = agent ID).
//!
//! An agent only computes when its `Wake` event fires: it settles the action
//! it was doing, picks the next one and schedules its next wake-up. Between
//! wake-ups nothing is ticked; needs grow lazily. Once a day, `DayStart` runs
//! the market: restocking, prices, wages, layoffs and hiring.

use std::fmt;

use crate::behaviour::{var, Allowed, Behaviour, Candidate, Explanation, Population, MAX_VARS};
use crate::economy::{BusinessConfig, BusinessId, Economy, GoodId, MarketConfig};
use crate::hash::{StateHash, StateHasher};
use crate::housing::{HouseConfig, Housing, HousingConfig};
use crate::map::{Building, HouseId, Map, MapConfig, Pos};
use crate::needs::{LazyNeed, NEED_MAX};
use crate::queue::{Event, EventQueue};
use crate::rng;

/// Game minutes since the world began.
pub type Minute = u64;
pub const MINUTES_PER_HOUR: Minute = 60;
pub const MINUTES_PER_DAY: Minute = 24 * MINUTES_PER_HOUR;

const NO_EMPLOYER: BusinessId = BusinessId::MAX;
const NO_HOME: HouseId = HouseId::MAX;

/// Every business keeps this many workers no matter its cash: they are never
/// laid off, accept partial pay and are hired even when the till is empty.
/// Stands in for owners until firms get them (iteration 4); without it a
/// business with no cash can never produce again.
const CORE_CREW: u32 = 1;

/// Hire only with this many days of payroll in cash; lay off below one day.
/// The gap keeps businesses from firing and rehiring on alternate days.
const HIRE_RESERVE_DAYS: i64 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldConfig {
    pub agents: u32,
    pub start_money: i64,

    pub goods: Vec<String>,
    /// The good agents eat.
    pub food: GoodId,
    pub businesses: Vec<BusinessConfig>,
    pub market: MarketConfig,

    pub map: MapConfig,
    pub houses: Vec<HouseConfig>,
    pub housing: HousingConfig,

    /// Trait names; agents carry one 0-1000 value per trait.
    pub traits: Vec<String>,
    /// Agents are split over these in ID order, by weight.
    pub populations: Vec<Population>,
    pub behaviour: Behaviour,

    pub hunger_per_hour: u32,
    /// Hunger at which a running action is interrupted to go eat.
    pub hunger_critical: u32,

    pub fatigue_per_hour: u32,
    pub sleep_recovery_per_hour: u32,
    /// Longest sleep; agents wake up then even if not rested.
    pub max_sleep_minutes: Minute,

    pub meal_relief: u32,
    pub meal_minutes: Minute,

    pub work_start_hour: u32,
    pub work_end_hour: u32,
    /// Overtime runs from `work_end_hour` to here.
    pub overtime_end_hour: u32,
    /// Work is done in blocks of at most this long, deciding again after each.
    pub work_block_minutes: Minute,

    pub wander_min_minutes: u32,
    pub wander_max_minutes: u32,
}

impl WorldConfig {
    pub fn work_hours(&self) -> i64 {
        (self.work_end_hour - self.work_start_hour) as i64
    }
}

/// Salts for random streams that must not use up an agent's roll numbers.
const TRAIT_SALT: u64 = 0x7472_6169_7473_0001;
const TIE_SALT: u64 = 0x7469_6573_0000_0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Idle,
    Eat,
    Sleep,
    Work,
    Overtime,
    Wander,
    /// On the way to where the chosen action happens.
    Travel,
}

impl ActionKind {
    /// Actions an agent can choose, in evaluation order.
    pub const CHOOSABLE: [ActionKind; 5] = [
        ActionKind::Eat,
        ActionKind::Sleep,
        ActionKind::Work,
        ActionKind::Overtime,
        ActionKind::Wander,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Idle => "idle",
            ActionKind::Eat => "eat",
            ActionKind::Sleep => "sleep",
            ActionKind::Work => "work",
            ActionKind::Overtime => "overtime",
            ActionKind::Wander => "wander",
            ActionKind::Travel => "travel",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::CHOOSABLE.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub started: Minute,
    pub ends: Minute,
    /// Fatigue when the action started; sleep recovers from this value.
    pub start_fatigue: u32,
    /// Sleep: fatigue recovered per hour.
    pub recovery_per_hour: u32,
    /// Travel: where to.
    pub target: Pos,
}

impl Action {
    const IDLE: Action = Action {
        kind: ActionKind::Idle,
        started: 0,
        ends: 0,
        start_fatigue: 0,
        recovery_per_hour: 0,
        target: Pos::new(0, 0),
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeChange {
    MovedIn,
    MovedOut,
    /// Could not pay the rent.
    Evicted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobChange {
    Hired,
    LaidOff,
    /// Quit because the employer could not pay the full wage.
    QuitUnpaid,
}

/// One line of the event log.
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
    Job {
        at: Minute,
        agent: u32,
        business: BusinessId,
        change: JobChange,
    },
    Home {
        at: Minute,
        agent: u32,
        house: HouseId,
        change: HomeChange,
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
                    "{} a{agent} {:<8} until {} h={hunger:>4} f={fatigue:>4} m={money}",
                    clock(at),
                    action.kind.name(),
                    clock(action.ends),
                )?;
                if action.kind == ActionKind::Travel {
                    write!(f, " to {}", action.target)?;
                }
                if let Some(kind) = interrupted {
                    write!(f, " (interrupted {})", kind.name())?;
                }
                Ok(())
            }
            LogEntry::Job {
                at,
                agent,
                business,
                change,
            } => {
                let what = match change {
                    JobChange::Hired => "hired by",
                    JobChange::LaidOff => "laid off by",
                    JobChange::QuitUnpaid => "quit unpaid at",
                };
                write!(f, "{} a{agent} {what} b{business}", clock(at))
            }
            LogEntry::Home {
                at,
                agent,
                house,
                change,
            } => {
                let what = match change {
                    HomeChange::MovedIn => "moved into",
                    HomeChange::MovedOut => "moved out of",
                    HomeChange::Evicted => "evicted from",
                };
                write!(f, "{} a{agent} {what} h{house}", clock(at))
            }
        }
    }
}

/// Totals for one moment, for statistics output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub minute: Minute,
    pub money_agents: i64,
    pub money_businesses: i64,
    /// Output price per business, in business order.
    pub prices: Vec<i64>,
    /// Wage per business, in business order.
    pub wages: Vec<i64>,
    /// Cash per business, in business order.
    pub cash: Vec<i64>,
    /// Workers per business, in business order.
    pub workers: Vec<u32>,
    /// Stock per good, summed over all businesses.
    pub stock: Vec<u32>,
    pub employed: u32,
    pub unemployed: u32,
    /// Agents at or above critical hunger.
    pub hungry: u32,
    /// Meals eaten since the world began.
    pub meals_total: u64,
    /// Agent-minutes spent at or above critical hunger since the world
    /// began, counted up to each agent's last wake-up.
    pub starving_minutes_total: u64,
    /// Minutes worked (including overtime) since the world began.
    pub work_minutes_total: u64,
    /// Inequality of agent money, 0 (equal) to 1000 (one agent has all).
    pub money_gini_permille: u32,
    /// Rent per house, in house order.
    pub rents: Vec<i64>,
    /// Tenants per house, in house order.
    pub tenants: Vec<u32>,
    pub homeless: u32,
    /// Mean distance home to work of employed agents with a home, in hundredths of a parcel.
    pub commute_centi: u32,
}

#[derive(Clone, Debug)]
pub struct World {
    seed: u64,
    now: Minute,
    config: WorldConfig,
    queue: EventQueue,
    economy: Economy,
    map: Map,
    housing: Housing,
    hunger: Vec<LazyNeed>,
    fatigue: Vec<LazyNeed>,
    money: Vec<i64>,
    employer: Vec<BusinessId>,
    home: Vec<HouseId>,
    pos: Vec<Pos>,
    action: Vec<Action>,
    /// Running roll number per agent, input to [`rng::roll`].
    event_no: Vec<u64>,
    /// Trait values, `traits.len()` per agent.
    traits: Vec<u32>,
    population: Vec<u32>,
    work_minutes: Vec<u64>,
    /// Minutes spent at or above critical hunger, summed over all agents.
    starving_minutes: u64,
    /// `work_minutes` when the current day started.
    work_at_day_start: Vec<u64>,
    /// Smoothed minutes worked per day (3/4 old + 1/4 yesterday).
    work_recent: Vec<u64>,
    meals_total: u64,
}

impl World {
    pub fn new(seed: u64, config: WorldConfig) -> Self {
        let n = config.agents as usize;
        let mut queue = EventQueue::default();
        queue.push(0, Event::DayStart);
        for agent in 0..config.agents {
            queue.push(0, Event::Wake { agent });
        }
        let economy = Economy::new(
            config.goods.len(),
            config.businesses.clone(),
            config.market.clone(),
        );
        let map = Map::new(
            config.map.clone(),
            (config.businesses.iter().enumerate())
                .map(|(b, c)| (c.pos, Building::Business(b as BusinessId)))
                .chain(
                    (config.houses.iter().enumerate())
                        .map(|(h, c)| (c.pos, Building::House(h as HouseId))),
                ),
        );
        let housing = Housing::new(config.houses.clone(), config.housing.clone());
        let center = config.map.center();
        let population = assign_populations(config.agents, &config.populations);
        let k = config.traits.len();
        let mut traits = vec![NEED_MAX / 2; n * k];
        for (agent, &p) in population.iter().enumerate() {
            for (t, &(lo, hi)) in config.populations[p as usize].traits.iter().enumerate() {
                let roll = rng::below(seed ^ TRAIT_SALT, agent as u32, t as u64, hi - lo + 1);
                traits[agent * k + t] = lo + roll;
            }
        }
        Self {
            seed,
            now: 0,
            queue,
            economy,
            map,
            housing,
            hunger: vec![LazyNeed::new(0, 0); n],
            fatigue: vec![LazyNeed::new(0, 0); n],
            money: vec![config.start_money; n],
            employer: vec![NO_EMPLOYER; n],
            home: vec![NO_HOME; n],
            pos: vec![center; n],
            action: vec![Action::IDLE; n],
            event_no: vec![0; n],
            traits,
            population,
            work_minutes: vec![0; n],
            starving_minutes: 0,
            work_at_day_start: vec![0; n],
            work_recent: vec![0; n],
            meals_total: 0,
            config,
        }
    }

    /// Replaces the decision rules while running (hot reload). Needs, money
    /// and traits stay as they are; the next wake-up uses the new rules.
    pub fn set_behaviour(&mut self, behaviour: Behaviour) {
        self.config.behaviour = behaviour;
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

    pub fn config(&self) -> &WorldConfig {
        &self.config
    }

    pub fn economy(&self) -> &Economy {
        &self.economy
    }

    pub fn map(&self) -> &Map {
        &self.map
    }

    pub fn housing(&self) -> &Housing {
        &self.housing
    }

    pub fn home(&self, agent: u32) -> Option<HouseId> {
        let h = self.home[agent as usize];
        (h != NO_HOME).then_some(h)
    }

    /// Parcel the agent is on (or last passed, while travelling).
    pub fn pos(&self, agent: u32) -> Pos {
        self.pos[agent as usize]
    }

    /// Where the agent is right now, in tenths of a parcel. Travellers are
    /// placed along their way (x first, then y), for smooth display.
    pub fn position_tenths(&self, agent: u32) -> (u32, u32) {
        let i = agent as usize;
        let (p, a) = (self.pos[i], self.action[i]);
        if a.kind != ActionKind::Travel {
            return (p.x as u32 * 10, p.y as u32 * 10);
        }
        let rate = self.map.config().travel_minutes_per_parcel.max(1);
        let tenths = (self.now - a.started) * 10 / rate;
        let (whole, frac) = ((tenths / 10) as u32, (tenths % 10) as i64);
        let (p1, p2) = (p.toward(a.target, whole), p.toward(a.target, whole + 1));
        let lerp = |a: u16, b: u16| (a as i64 * 10 + (b as i64 - a as i64) * frac) as u32;
        (lerp(p1.x, p2.x), lerp(p1.y, p2.y))
    }

    /// Where the agent works, or the town centre if unemployed.
    pub fn work_pos(&self, agent: u32) -> Pos {
        match self.employer[agent as usize] {
            NO_EMPLOYER => self.map.config().center(),
            b => self.economy.config(b).pos,
        }
    }

    /// Land price of a parcel: the base price plus the rent of every house
    /// within the radius, weighted by closeness. Derived, never stored.
    pub fn land_price(&self, p: Pos) -> i64 {
        let c = self.map.config();
        let r = c.land_price_radius as i64;
        let near: i64 = (0..self.housing.len())
            .map(|h| {
                let d = self.housing.config(h).pos.distance(p) as i64;
                self.housing.rent(h) * (r - d).max(0)
            })
            .sum();
        c.land_price_base + if r > 0 { near * 10 / r } else { 0 }
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

    pub fn employer(&self, agent: u32) -> Option<BusinessId> {
        let e = self.employer[agent as usize];
        (e != NO_EMPLOYER).then_some(e)
    }

    pub fn action(&self, agent: u32) -> Action {
        self.action[agent as usize]
    }

    /// Trait values of `agent`, in [`WorldConfig::traits`] order.
    pub fn traits(&self, agent: u32) -> &[u32] {
        let k = self.config.traits.len();
        &self.traits[agent as usize * k..][..k]
    }

    /// Index into [`WorldConfig::populations`].
    pub fn population(&self, agent: u32) -> u32 {
        self.population[agent as usize]
    }

    /// Minutes `agent` has worked since the world began.
    pub fn work_minutes(&self, agent: u32) -> u64 {
        self.work_minutes[agent as usize]
    }

    /// Safety need: 1000 with no savings, 0 once savings cover the reserve.
    pub fn safety(&self, agent: u32) -> u32 {
        let b = &self.config.behaviour;
        let target = b.safety_reserve_days * b.meals_per_day * self.food_list_price();
        if target <= 0 {
            return 0;
        }
        let covered = self.money[agent as usize].max(0).min(target) * NEED_MAX as i64 / target;
        NEED_MAX - covered as u32
    }

    /// Money held by agents and businesses together. Constant over time.
    pub fn money_supply(&self) -> i64 {
        self.money.iter().sum::<i64>() + self.economy.total_cash() + self.housing.total_cash()
    }

    pub fn stats(&self) -> Stats {
        let e = &self.economy;
        let employed = self.employer.iter().filter(|&&b| b != NO_EMPLOYER).count() as u32;
        Stats {
            minute: self.now,
            money_agents: self.money.iter().sum(),
            money_businesses: e.total_cash(),
            prices: (0..e.len()).map(|b| e.price(b)).collect(),
            wages: (0..e.len()).map(|b| e.wage(b)).collect(),
            cash: (0..e.len()).map(|b| e.cash(b)).collect(),
            workers: (0..e.len()).map(|b| e.workers(b)).collect(),
            stock: (0..self.config.goods.len() as GoodId)
                .map(|g| e.total_stock(g))
                .collect(),
            employed,
            unemployed: self.config.agents - employed,
            hungry: (0..self.config.agents)
                .filter(|&a| self.hunger(a) >= self.config.hunger_critical)
                .count() as u32,
            meals_total: self.meals_total,
            starving_minutes_total: self.starving_minutes,
            work_minutes_total: self.work_minutes.iter().sum(),
            money_gini_permille: gini_permille(&self.money),
            rents: (0..self.housing.len())
                .map(|h| self.housing.rent(h))
                .collect(),
            tenants: (0..self.housing.len())
                .map(|h| self.housing.tenants(h))
                .collect(),
            homeless: self.home.iter().filter(|&&h| h == NO_HOME).count() as u32,
            commute_centi: self.commute_centi(),
        }
    }

    fn commute_centi(&self) -> u32 {
        let (mut sum, mut n) = (0u64, 0u64);
        for a in 0..self.config.agents {
            if let (Some(h), Some(_)) = (self.home(a), self.employer(a)) {
                sum += self.housing.config(h).pos.distance(self.work_pos(a)) as u64;
                n += 1;
            }
        }
        if n == 0 {
            0
        } else {
            (sum * 100 / n) as u32
        }
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
                    self.day_start(log);
                }
                Event::Wake { agent } => self.wake(agent, log),
            }
        }
        self.now = until;
    }

    /// Runs the given number of days from now, without logging.
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
        let recovered = a.recovery_per_hour as u64 * slept / 60;
        (a.start_fatigue as u64).saturating_sub(recovered) as u32
    }

    /// Daily market: restock inputs, adjust prices and wages, lay off, hire.
    fn day_start(&mut self, log: &mut impl FnMut(LogEntry)) {
        let hours = self.config.work_hours();
        let at = self.now;
        for i in 0..self.work_minutes.len() {
            let yesterday = self.work_minutes[i] - self.work_at_day_start[i];
            self.work_recent[i] = (3 * self.work_recent[i] + yesterday) / 4;
            self.work_at_day_start[i] = self.work_minutes[i];
        }
        self.collect_rent(at, log);
        self.housing.adjust_rents();
        self.choose_homes(at, log);
        self.economy.restock_inputs(hours);
        self.economy.adjust_prices();
        let job_seekers = self.employer.contains(&NO_EMPLOYER);
        self.economy.adjust_wages(hours, job_seekers);

        for b in 0..self.economy.len() {
            // Lay off while one day of payroll is not covered.
            while self.economy.workers(b) > CORE_CREW
                && self.economy.cash(b)
                    < self
                        .economy
                        .payroll_per_day(b, self.economy.workers(b), hours)
            {
                let agent = self.layoff_candidate(b);
                self.leave_job(agent, b, JobChange::LaidOff, at, log);
            }
            // Overstocked: shrink by one worker a day until stock comes down.
            if self.overstocked(b) && self.economy.workers(b) > CORE_CREW {
                let agent = self.layoff_candidate(b);
                self.leave_job(agent, b, JobChange::LaidOff, at, log);
            }
        }

        // Core crews first, so every link of the chain can produce.
        for b in 0..self.economy.len() {
            while self.economy.workers(b) < CORE_CREW.min(self.economy.config(b).slots) {
                let Some(agent) = self.hire_candidate() else {
                    break;
                };
                self.hire(agent, b, at, log);
            }
        }
        // Then one hire per business per round, so no business takes everyone.
        'rounds: loop {
            let mut hired = false;
            for b in 0..self.economy.len() {
                if !self.wants_to_hire(b, hours) {
                    continue;
                }
                let Some(agent) = self.hire_candidate() else {
                    break 'rounds;
                };
                self.hire(agent, b, at, log);
                hired = true;
            }
            if !hired {
                break;
            }
        }
        self.pay_dividends();
    }

    /// Every tenant pays a day's rent; who cannot is evicted.
    fn collect_rent(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        for agent in 0..self.config.agents {
            let i = agent as usize;
            let h = self.home[i];
            if h == NO_HOME {
                continue;
            }
            if self.money[i] >= self.housing.rent(h) {
                self.money[i] -= self.housing.receive_rent(h);
            } else {
                self.set_home(agent, NO_HOME, HomeChange::Evicted, at, log);
            }
        }
    }

    /// Scores of `agent`'s housing options: (house, score, has a free bed or
    /// is its own), plus the homeless score. Options its rule rejects are left out.
    fn housing_options(&self, agent: u32) -> (Vec<(HouseId, i64, bool)>, i64) {
        let rule = &self.config.behaviour.housing;
        let mut v = self.vars(agent, self.now);
        let work = self.work_pos(agent);
        let mut out = Vec::with_capacity(self.housing.len() as usize);
        for h in 0..self.housing.len() {
            let current = self.home[agent as usize] == h;
            v[var::HOUSE_RENT] = self.housing.rent(h);
            v[var::HOUSE_DISTANCE] = self.housing.config(h).pos.distance(work) as i64;
            v[var::HOUSE_CURRENT] = current as i64;
            if rule.when.as_ref().is_some_and(|f| f.eval(&v) <= 0) {
                continue;
            }
            let open = current || self.housing.free_beds(h) > 0;
            out.push((h, rule.score.eval(&v), open));
        }
        let mut v = self.vars(agent, self.now);
        v[var::HOUSE_RENT] = 0;
        v[var::HOUSE_DISTANCE] = 0;
        v[var::HOUSE_CURRENT] = (self.home[agent as usize] == NO_HOME) as i64;
        (out, rule.homeless_score.eval(&v))
    }

    /// Every agent takes its best housing option. A full house that would
    /// have been better counts as demand for it. The agent who chooses first
    /// rotates daily, so nobody always gets the first pick.
    fn choose_homes(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let n = self.config.agents;
        if n == 0 {
            return;
        }
        let first = (self.day() % n as u64) as u32;
        for k in 0..n {
            let agent = (first + k) % n;
            let (options, homeless) = self.housing_options(agent);
            let current = self.home[agent as usize];
            // Ties: the current home, then the lowest house ID.
            let better = |a: (i64, bool), b: (i64, bool)| a.0 > b.0 || (a.0 == b.0 && a.1 && !b.1);
            let mut best = (NO_HOME, (homeless, current == NO_HOME));
            let mut wanted: Option<(HouseId, i64)> = None;
            for &(h, score, open) in &options {
                let key = (score, h == current);
                if open {
                    if better(key, best.1) {
                        best = (h, key);
                    }
                } else if wanted.is_none_or(|(_, s)| score > s) {
                    wanted = Some((h, score));
                }
            }
            if let Some((h, score)) = wanted {
                if score > best.1 .0 {
                    self.housing.turn_away(h);
                }
            }
            if best.0 != current {
                if best.0 == NO_HOME {
                    self.set_home(agent, NO_HOME, HomeChange::MovedOut, at, log);
                } else {
                    self.set_home(agent, best.0, HomeChange::MovedIn, at, log);
                }
            }
        }
    }

    fn set_home(
        &mut self,
        agent: u32,
        house: HouseId,
        change: HomeChange,
        at: Minute,
        log: &mut impl FnMut(LogEntry),
    ) {
        let i = agent as usize;
        let old = self.home[i];
        if old != NO_HOME {
            self.housing.move_out(old);
        }
        if house != NO_HOME {
            self.housing.move_in(house);
        }
        self.home[i] = house;
        log(LogEntry::Home {
            at,
            agent,
            house: if house == NO_HOME { old } else { house },
            change,
        });
    }

    fn overstocked(&self, b: BusinessId) -> bool {
        let c = self.economy.config(b);
        self.economy.stock(b, c.output) > 2 * c.output_target
    }

    /// Room, no glut, and cash for `HIRE_RESERVE_DAYS` of the bigger payroll.
    fn wants_to_hire(&self, b: BusinessId, hours: i64) -> bool {
        let e = &self.economy;
        let workers = e.workers(b);
        workers < e.config(b).slots
            && !self.overstocked(b)
            && e.cash(b) >= HIRE_RESERVE_DAYS * e.payroll_per_day(b, workers + 1, hours)
    }

    fn hire(&mut self, agent: u32, b: BusinessId, at: Minute, log: &mut impl FnMut(LogEntry)) {
        self.employer[agent as usize] = b;
        self.economy.add_worker(b);
        log(LogEntry::Job {
            at,
            agent,
            business: b,
            change: JobChange::Hired,
        });
    }

    /// Workers with savings above the safety reserve go first, as they can
    /// afford it; among them whoever worked least lately, then the most
    /// savings, then the highest ID.
    fn layoff_candidate(&self, b: BusinessId) -> u32 {
        (0..self.config.agents)
            .filter(|&a| self.employer[a as usize] == b)
            .max_by_key(|&a| {
                let i = a as usize;
                let can_afford = self.safety(a) == 0;
                (
                    can_afford,
                    std::cmp::Reverse(self.work_recent[i]),
                    self.money[i],
                    a,
                )
            })
            .expect("worker count matches employers")
    }

    /// The job seeker with the least money is hired first (ties: lowest ID).
    fn hire_candidate(&self) -> Option<u32> {
        (0..self.config.agents)
            .filter(|&a| self.employer[a as usize] == NO_EMPLOYER)
            .min_by_key(|&a| (self.money[a as usize], a))
    }

    /// Pays every business's surplus cash out in equal shares to agents whose
    /// savings are below the safety reserve (everyone, if nobody is). Stands
    /// in for owners and town welfare until those exist; without it money
    /// piles up where nobody spends it and demand dies.
    fn pay_dividends(&mut self) {
        let hours = self.config.work_hours();
        let needy: Vec<usize> = (0..self.config.agents)
            .filter(|&a| self.safety(a) > 0)
            .map(|a| a as usize)
            .collect();
        let recipients: Vec<usize> = if needy.is_empty() {
            (0..self.config.agents as usize).collect()
        } else {
            needy
        };
        let n = recipients.len() as i64;
        if n == 0 {
            return;
        }
        for b in 0..self.economy.len() {
            let surplus = self.economy.take_surplus(b, hours);
            let share = surplus / n;
            recipients.iter().for_each(|&i| self.money[i] += share);
            self.economy.add_cash(b, surplus - share * n);
        }
        // Houses have no costs yet: all rent flows back the same way.
        let mut rent = 0;
        for h in 0..self.housing.len() {
            rent += self.housing.take_cash(h);
        }
        let share = rent / n;
        recipients.iter().for_each(|&i| self.money[i] += share);
        if !self.housing.is_empty() {
            // The remainder stays with the first house until tomorrow.
            self.housing.add_cash(0, rent - share * n);
        }
    }

    fn leave_job(
        &mut self,
        agent: u32,
        b: BusinessId,
        change: JobChange,
        at: Minute,
        log: &mut impl FnMut(LogEntry),
    ) {
        self.employer[agent as usize] = NO_EMPLOYER;
        self.economy.remove_worker(b);
        log(LogEntry::Job {
            at,
            agent,
            business: b,
            change,
        });
    }

    fn wake(&mut self, agent: u32, log: &mut impl FnMut(LogEntry)) {
        let i = agent as usize;
        let now = self.now;
        let previous = self.action[i];
        let interrupted =
            (previous.kind != ActionKind::Idle && now < previous.ends).then_some(previous.kind);

        self.settle(agent, now, log);
        let action = self.decide(agent, now);
        self.event_no[i] += 1;
        if action.kind == ActionKind::Eat {
            let seller = self
                .economy
                .cheapest_seller(self.config.food)
                .expect("decide only picks eat when food is for sale");
            self.money[i] -= self.economy.sell_one(seller);
            self.meals_total += 1;
        }
        self.action[i] = action;
        let wake_at = self.wake_time(i, action);
        self.queue.push(wake_at, Event::Wake { agent });

        log(LogEntry::Started {
            at: now,
            agent,
            action,
            interrupted,
            hunger: self.hunger[i].value(now, self.config.hunger_per_hour),
            fatigue: self.fatigue[i].value(now, self.config.fatigue_per_hour),
            money: self.money[i],
        });
    }

    /// Applies the effects of the action that just ended (fully or partly).
    fn settle(&mut self, agent: u32, now: Minute, log: &mut impl FnMut(LogEntry)) {
        let i = agent as usize;
        let a = self.action[i];
        let elapsed = now - a.started;
        // Hunger only drops when a meal ends, so it rose steadily since `started`.
        let c = &self.config;
        if let Some(t) = self.hunger[i].reaches(c.hunger_critical, c.hunger_per_hour) {
            self.starving_minutes += now.saturating_sub(t.max(a.started));
        }
        match a.kind {
            ActionKind::Idle | ActionKind::Wander => {}
            ActionKind::Travel => {
                let rate = self.map.config().travel_minutes_per_parcel.max(1);
                self.pos[i] = if now >= a.ends {
                    a.target
                } else {
                    self.pos[i].toward(a.target, (elapsed / rate) as u32)
                };
            }
            ActionKind::Eat => {
                let h = self.hunger[i].value(now, self.config.hunger_per_hour);
                self.hunger[i].set(now, h.saturating_sub(self.config.meal_relief));
            }
            ActionKind::Work | ActionKind::Overtime => {
                let b = self.employer[i];
                if b == NO_EMPLOYER {
                    return;
                }
                self.work_minutes[i] += elapsed;
                self.economy.produce(b, elapsed);
                let owed = self.economy.wage(b) * elapsed as i64 / 60;
                let paid = self.economy.pay(b, owed);
                self.money[i] += paid;
                if paid < owed && self.economy.workers(b) > CORE_CREW {
                    self.leave_job(agent, b, JobChange::QuitUnpaid, now, log);
                }
            }
            ActionKind::Sleep => {
                let f = self.sleep_fatigue(i, now);
                self.fatigue[i].set(now, f);
            }
        }
    }

    /// Price of the cheapest food for sale, if any is in stock.
    fn food_price(&self) -> Option<i64> {
        self.economy
            .cheapest_seller(self.config.food)
            .map(|b| self.economy.price(b))
    }

    fn can_buy_food(&self, i: usize) -> bool {
        self.food_price().is_some_and(|p| self.money[i] >= p)
    }

    /// Lowest food price, whether in stock or not. 0 if nobody makes food.
    fn food_list_price(&self) -> i64 {
        (0..self.economy.len())
            .filter(|&b| self.economy.config(b).output == self.config.food)
            .map(|b| self.economy.price(b))
            .min()
            .unwrap_or(0)
    }

    /// Fills the variable table formulas read. Only the first
    /// `BASE_VARS.len() + traits` entries are meaningful.
    fn vars(&self, agent: u32, now: Minute) -> [i64; MAX_VARS] {
        let i = agent as usize;
        let c = &self.config;
        let hour = ((now % MINUTES_PER_DAY) / 60) as u32;
        let b = self.employer[i];
        let employed = b != NO_EMPLOYER;
        let mut v = [0i64; MAX_VARS];
        v[var::HUNGER] = self.hunger[i].value(now, c.hunger_per_hour) as i64;
        v[var::FATIGUE] = if self.action[i].kind == ActionKind::Sleep {
            self.sleep_fatigue(i, now)
        } else {
            self.fatigue[i].value(now, c.fatigue_per_hour)
        } as i64;
        v[var::SAFETY] = self.safety(agent) as i64;
        v[var::MONEY] = self.money[i];
        v[var::WAGE] = if employed { self.economy.wage(b) } else { 0 };
        v[var::FOOD_PRICE] = self.food_list_price();
        v[var::CAN_EAT] = self.can_buy_food(i) as i64;
        v[var::EMPLOYED] = employed as i64;
        v[var::HOUR] = hour as i64;
        v[var::NIGHT] = !(6..22).contains(&hour) as i64;
        v[var::WORK_TIME] = (c.work_start_hour..c.work_end_hour).contains(&hour) as i64;
        v[var::OVERTIME_TIME] = (c.work_end_hour..c.overtime_end_hour).contains(&hour) as i64;
        let h = self.home[i];
        if h != NO_HOME {
            v[var::HAS_HOME] = 1;
            v[var::HOUSE_RENT] = self.housing.rent(h);
            v[var::HOUSE_DISTANCE] =
                self.housing.config(h).pos.distance(self.work_pos(agent)) as i64;
            v[var::HOUSE_CURRENT] = 1;
        }
        for (t, &value) in self.traits(agent).iter().enumerate() {
            v[var::TRAITS + t] = value as i64;
        }
        v
    }

    /// Built-in preconditions that data cannot override.
    fn possible(kind: ActionKind, v: &[i64]) -> bool {
        match kind {
            ActionKind::Eat => v[var::CAN_EAT] != 0,
            ActionKind::Work => v[var::EMPLOYED] != 0 && v[var::WORK_TIME] != 0,
            ActionKind::Overtime => v[var::EMPLOYED] != 0 && v[var::OVERTIME_TIME] != 0,
            ActionKind::Sleep | ActionKind::Wander => true,
            ActionKind::Idle | ActionKind::Travel => false,
        }
    }

    /// Scores every action. The winner is the best allowed one; ties go to a
    /// seed-based roll. Wander wins when nothing is allowed.
    fn candidates(&self, agent: u32, now: Minute) -> ([i64; MAX_VARS], Vec<Candidate>, ActionKind) {
        let v = self.vars(agent, now);
        let event_no = self.event_no[agent as usize];
        let mut best: Option<((i64, u64), ActionKind)> = None;
        let mut out = Vec::with_capacity(self.config.behaviour.actions.len());
        for rule in &self.config.behaviour.actions {
            let allowed = if !Self::possible(rule.kind, &v) {
                Allowed::NotPossible
            } else if rule.when.as_ref().is_some_and(|f| f.eval(&v) <= 0) {
                Allowed::RuleSaysNo
            } else {
                Allowed::Yes
            };
            let score = rule.score.eval(&v);
            if allowed == Allowed::Yes {
                let tie = rng::roll(
                    seed_tie(self.seed),
                    agent,
                    (event_no << 8) | rule.kind as u64,
                );
                let key = (score, tie);
                if best.is_none_or(|(k, _)| key > k) {
                    best = Some((key, rule.kind));
                }
            }
            out.push(Candidate {
                kind: rule.kind,
                allowed,
                score,
            });
        }
        (v, out, best.map_or(ActionKind::Wander, |(_, k)| k))
    }

    /// What `agent` would pick if it decided right now, and why.
    pub fn explain(&self, agent: u32) -> Explanation {
        let (v, candidates, winner) = self.candidates(agent, self.now);
        let names = crate::behaviour::variable_names(&self.config.traits);
        Explanation {
            agent,
            minute: self.now,
            vars: names.into_iter().zip(v).collect(),
            candidates,
            winner,
        }
    }

    /// Where `kind` has to happen, if anywhere in particular.
    fn place(&self, agent: u32, kind: ActionKind) -> Option<Pos> {
        let i = agent as usize;
        match kind {
            ActionKind::Work | ActionKind::Overtime => Some(self.work_pos(agent)),
            ActionKind::Sleep if self.home[i] != NO_HOME => {
                Some(self.housing.config(self.home[i]).pos)
            }
            _ => None,
        }
    }

    fn decide(&mut self, agent: u32, now: Minute) -> Action {
        let i = agent as usize;
        let (v, _, kind) = self.candidates(agent, now);
        let fatigue = v[var::FATIGUE] as u32;
        let mut action = Action {
            kind,
            started: now,
            ends: now,
            start_fatigue: fatigue,
            recovery_per_hour: 0,
            target: self.pos[i],
        };
        if let Some(target) = self.place(agent, kind).filter(|&t| t != self.pos[i]) {
            let minutes = self.map.travel_minutes(self.pos[i], target);
            if minutes > 0 {
                action.kind = ActionKind::Travel;
                action.target = target;
                action.ends = now + minutes;
                return action;
            }
            self.pos[i] = target;
        }
        let c = &self.config;
        let recovery = if self.home[i] == NO_HOME {
            c.sleep_recovery_per_hour * c.housing.homeless_recovery_percent / 100
        } else {
            c.sleep_recovery_per_hour
        }
        .max(1);
        let minute_of_day = now % MINUTES_PER_DAY;
        let block =
            |end_hour: u32| (end_hour as Minute * 60 - minute_of_day).min(c.work_block_minutes);

        let minutes = match kind {
            ActionKind::Eat => c.meal_minutes,
            ActionKind::Sleep => {
                action.recovery_per_hour = recovery;
                let m = (fatigue as u64 * 60).div_ceil(recovery as u64);
                m.clamp(30, c.max_sleep_minutes.max(30))
            }
            ActionKind::Work => block(c.work_end_hour),
            ActionKind::Overtime => block(c.overtime_end_hour),
            ActionKind::Wander | ActionKind::Idle | ActionKind::Travel => {
                let (lo, hi) = (c.wander_min_minutes, c.wander_max_minutes);
                let work_start = c.work_start_hour as Minute * 60;
                let minutes = (lo + self.next_roll(agent, hi - lo + 1)) as Minute;
                // Never wander past the start of the work day.
                let until_work = work_start.saturating_sub(minute_of_day);
                if until_work > 0 {
                    minutes.min(until_work)
                } else {
                    minutes
                }
            }
        };

        action.ends = now + minutes;
        action
    }

    /// Normally the action's end. Earlier if hunger turns critical first and
    /// the agent could actually go and buy food.
    fn wake_time(&self, i: usize, action: Action) -> Minute {
        let c = &self.config;
        if action.kind == ActionKind::Eat || !self.can_buy_food(i) {
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
        self.economy.hash_into(h);
        h.write_u64(self.hunger.len() as u64);
        self.hunger.iter().for_each(|n| n.hash_into(h));
        self.fatigue.iter().for_each(|n| n.hash_into(h));
        h.write_i64_slice(&self.money);
        h.write_u32_slice(&self.employer);
        for a in &self.action {
            h.write_bytes(&[a.kind as u8]);
            h.write_u64(a.started);
            h.write_u64(a.ends);
            h.write_u32(a.start_fatigue);
            h.write_u32(a.recovery_per_hour);
            a.target.hash_into(h);
        }
        self.housing.hash_into(h);
        h.write_u32_slice(&self.home);
        self.pos.iter().for_each(|p| p.hash_into(h));
        self.event_no.iter().for_each(|&e| h.write_u64(e));
        h.write_u32_slice(&self.traits);
        h.write_u32_slice(&self.population);
        self.work_minutes.iter().for_each(|&m| h.write_u64(m));
        self.work_at_day_start.iter().for_each(|&m| h.write_u64(m));
        self.work_recent.iter().for_each(|&m| h.write_u64(m));
        h.write_u64(self.meals_total);
        h.write_u64(self.starving_minutes);
    }
}

fn seed_tie(seed: u64) -> u64 {
    seed ^ TIE_SALT
}

/// Splits `agents` into contiguous ID blocks by population weight.
fn assign_populations(agents: u32, pops: &[Population]) -> Vec<u32> {
    let total: u64 = pops.iter().map(|p| p.weight as u64).sum();
    if total == 0 {
        return vec![0; agents as usize];
    }
    let mut out = Vec::with_capacity(agents as usize);
    let mut cumulative = 0u64;
    for (p, pop) in pops.iter().enumerate() {
        cumulative += pop.weight as u64;
        let end = (agents as u64 * cumulative / total) as usize;
        out.resize(end, p as u32);
    }
    out
}

/// Gini coefficient of non-negative amounts, in thousandths.
fn gini_permille(amounts: &[i64]) -> u32 {
    let mut v: Vec<i64> = amounts.iter().map(|&m| m.max(0)).collect();
    v.sort_unstable();
    let n = v.len() as i128;
    let sum: i128 = v.iter().map(|&x| x as i128).sum();
    if n == 0 || sum == 0 {
        return 0;
    }
    let weighted: i128 = v
        .iter()
        .enumerate()
        .map(|(i, &x)| (2 * (i as i128 + 1) - n - 1) * x as i128)
        .sum();
    (weighted * 1000 / (n * sum)) as u32
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::behaviour::{variable_names, ActionRule, HousingRule};
    use crate::economy::tests::{BREAD, FLOUR, GRAIN};
    use crate::economy::RecipeInput;
    use crate::formula::Formula;

    /// The bundled bread town (content/data/world.toml), restated here so the
    /// core can be tested without the content crate.
    /// The bundled houses: (name, x, y, beds).
    pub(crate) const HOUSES: [(&str, u16, u16, u32); 5] = [
        ("farmstead", 2, 5, 12),
        ("mill row", 11, 2, 12),
        ("bakers lane", 14, 11, 12),
        ("market court", 8, 8, 12),
        ("outskirts", 1, 14, 12),
    ];

    pub(crate) fn config() -> WorldConfig {
        let business =
            |name: &str, output, input: Option<GoodId>, rate, slots, price, stock, x, y| {
                BusinessConfig {
                    name: name.into(),
                    pos: Pos::new(x, y),
                    output,
                    input: input.map(|good| RecipeInput { good, amount: 1 }),
                    output_milli_per_worker_hour: rate,
                    slots,
                    start_cash: 3000,
                    start_price: price,
                    start_wage: 10,
                    start_stock: stock,
                    output_target: 100,
                    input_target: if input.is_some() { 200 } else { 0 },
                }
            };
        WorldConfig {
            agents: 50,
            start_money: 200,
            goods: vec!["grain".into(), "flour".into(), "bread".into()],
            food: BREAD,
            businesses: vec![
                business("field", GRAIN, None, 750, 22, 15, 200, 3, 3),
                business("mill", FLOUR, Some(GRAIN), 1500, 12, 25, 200, 12, 4),
                business("bakery", BREAD, Some(FLOUR), 1500, 12, 35, 150, 13, 13),
            ],
            market: MarketConfig {
                price_step: 1,
                price_min: 1,
                wage_step: 1,
                wage_min: 5,
                reserve_days: 3,
            },
            map: MapConfig {
                width: 16,
                height: 16,
                travel_minutes_per_parcel: 2,
                land_price_base: 100,
                land_price_radius: 4,
            },
            houses: HOUSES
                .iter()
                .map(|&(name, x, y, beds)| HouseConfig {
                    name: name.into(),
                    pos: Pos::new(x, y),
                    beds,
                    start_rent: 5,
                })
                .collect(),
            housing: HousingConfig {
                rent_step: 1,
                rent_min: 1,
                homeless_recovery_percent: 50,
            },
            traits: traits(),
            populations: vec![Population {
                name: "townsfolk".into(),
                weight: 1,
                traits: vec![(0, 1000), (0, 1000)],
            }],
            behaviour: behaviour(&RULES),
            hunger_per_hour: 40,
            hunger_critical: 800,
            fatigue_per_hour: 40,
            sleep_recovery_per_hour: 80,
            max_sleep_minutes: 600,
            meal_relief: 300,
            meal_minutes: 30,
            work_start_hour: 8,
            work_end_hour: 18,
            overtime_end_hour: 21,
            work_block_minutes: 120,
            wander_min_minutes: 30,
            wander_max_minutes: 120,
        }
    }

    fn traits() -> Vec<String> {
        vec!["greed".into(), "work_life".into()]
    }

    /// The bundled rules: (action, when, score).
    pub(crate) const RULES: [(&str, &str, &str); 5] = [
        ("eat", "need.hunger >= 300", "4 * need.hunger"),
        (
            "sleep",
            "need.fatigue >= 700 || (night && need.fatigue >= 200)",
            "3 * need.fatigue",
        ),
        (
            "work",
            "1",
            "400 + need.safety / 2 + trait.greed / 4 - need.fatigue * trait.work_life / 1000",
        ),
        (
            "overtime",
            "1",
            "need.safety / 2 + trait.greed / 2 - need.fatigue * trait.work_life / 1000",
        ),
        ("wander", "1", "300 + trait.work_life / 4"),
    ];

    /// The bundled housing rule: (when, score, homeless_score).
    pub(crate) const HOUSING: (&str, &str, &str) = (
        "money >= house.rent * 2",
        "400 - house.rent * 10 - house.distance * 15 + house.current * 30",
        "0",
    );

    pub(crate) fn behaviour(rules: &[(&str, &str, &str)]) -> Behaviour {
        let names = variable_names(&traits());
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let compile = |src: &str| Formula::compile(src, &names).unwrap();
        Behaviour {
            actions: rules
                .iter()
                .map(|&(kind, when, score)| ActionRule {
                    kind: ActionKind::from_name(kind).unwrap(),
                    when: Some(Formula::compile(when, &names).unwrap()),
                    score: Formula::compile(score, &names).unwrap(),
                })
                .collect(),
            safety_reserve_days: 5,
            meals_per_day: 3,
            housing: HousingRule {
                when: Some(compile(HOUSING.0)),
                score: compile(HOUSING.1),
                homeless_score: compile(HOUSING.2),
            },
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
    fn money_is_neither_created_nor_destroyed() {
        let mut w = World::new(2, config());
        let supply = w.money_supply();
        for _ in 0..365 {
            w.run_days(1);
            assert_eq!(w.money_supply(), supply, "day {}", w.day());
        }
    }

    #[test]
    fn worker_counts_match_employers() {
        let mut w = World::new(6, config());
        for _ in 0..200 {
            w.run_days(1);
            for b in 0..w.economy().len() {
                let listed = (0..w.agent_count())
                    .filter(|&a| w.employer(a) == Some(b))
                    .count() as u32;
                assert_eq!(w.economy().workers(b), listed);
                assert!(listed <= w.economy().config(b).slots);
            }
        }
    }

    #[test]
    fn bread_chain_feeds_the_town() {
        let mut w = World::new(3, config());
        w.run_until(60 * MINUTES_PER_DAY, &mut |e| {
            if let LogEntry::Started {
                hunger, fatigue, ..
            } = e
            {
                assert!(hunger <= NEED_MAX && fatigue <= NEED_MAX);
            }
        });
        let s = w.stats();
        // 50 agents, 60 days, three meals a day: most get eaten even while the market settles.
        assert!(s.meals_total > 50 * 60 * 2, "meals = {}", s.meals_total);
    }

    #[test]
    fn agents_are_at_work_from_work_start() {
        let mut w = World::new(5, config());
        let start = config().work_start_hour as Minute * 60;
        for day in 0..30 {
            w.run_until(day * MINUTES_PER_DAY + start + 1, &mut |_| {});
            for agent in 0..w.agent_count() {
                if w.employer(agent).is_some() {
                    assert_ne!(
                        w.action(agent).kind,
                        ActionKind::Wander,
                        "day {day} a{agent}"
                    );
                }
            }
        }
    }

    #[test]
    fn critical_hunger_interrupts_work() {
        let mut c = config();
        let mut rules = RULES;
        rules[0].1 = "need.hunger >= 900";
        c.behaviour = behaviour(&rules);
        c.hunger_critical = 500;
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 5 * MINUTES_PER_DAY);
        assert!(lines.iter().any(|l| l.contains("(interrupted work)")));
    }

    #[test]
    fn broke_agents_neither_eat_nor_get_interrupted() {
        let mut c = config();
        c.start_money = 0;
        c.businesses.iter_mut().for_each(|b| b.start_cash = 0);
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 3 * MINUTES_PER_DAY);
        assert!(lines.iter().all(|l| !l.contains("interrupted")));
        assert!(lines.iter().all(|l| !l.contains(" eat ")));
    }

    #[test]
    fn populations_split_by_weight_in_id_order() {
        let pop = |weight| Population {
            name: String::new(),
            weight,
            traits: vec![],
        };
        assert_eq!(assign_populations(6, &[pop(1), pop(2)]), [0, 0, 1, 1, 1, 1]);
        assert_eq!(assign_populations(3, &[pop(0), pop(1)]), [1, 1, 1]);
        assert_eq!(assign_populations(0, &[pop(1)]), Vec::<u32>::new());
    }

    #[test]
    fn traits_stay_in_their_population_range() {
        let mut c = config();
        c.populations = vec![
            Population {
                name: "a".into(),
                weight: 1,
                traits: vec![(100, 200), (900, 900)],
            },
            Population {
                name: "b".into(),
                weight: 1,
                traits: vec![(700, 800), (0, 0)],
            },
        ];
        let w = World::new(1, c);
        for a in 0..w.agent_count() {
            let t = w.traits(a);
            if w.population(a) == 0 {
                assert!((100..=200).contains(&t[0]) && t[1] == 900, "a{a} {t:?}");
            } else {
                assert!((700..=800).contains(&t[0]) && t[1] == 0, "a{a} {t:?}");
            }
        }
        assert_eq!(w.population(0), 0);
        assert_eq!(w.population(49), 1);
    }

    #[test]
    fn gini_measures_inequality() {
        assert_eq!(gini_permille(&[5, 5, 5, 5]), 0);
        assert_eq!(gini_permille(&[0, 0, 0, 100]), 750);
        assert_eq!(gini_permille(&[]), 0);
        assert_eq!(gini_permille(&[0, 0]), 0);
    }

    #[test]
    fn explain_matches_the_decision() {
        let mut w = World::new(3, config());
        w.run_until(2 * MINUTES_PER_DAY + 9 * 60, &mut |_| {});
        let e = w.explain(7);
        assert_eq!(e.candidates.len(), RULES.len());
        assert_eq!(e.vars.len(), crate::behaviour::BASE_VARS.len() + 2);
        let best = e
            .candidates
            .iter()
            .filter(|c| c.allowed == Allowed::Yes)
            .map(|c| c.score)
            .max()
            .unwrap();
        let winner = e.candidates.iter().find(|c| c.kind == e.winner).unwrap();
        assert_eq!(winner.allowed, Allowed::Yes);
        assert_eq!(winner.score, best);
        // Explaining changes nothing.
        let before = w.state_hash();
        w.explain(7);
        assert_eq!(w.state_hash(), before);
    }

    #[test]
    fn new_rules_take_effect_while_running() {
        let mut w = World::new(1, config());
        w.run_days(2);
        let mut rules = RULES;
        rules[2].2 = "-1000";
        rules[3].2 = "-1000";
        w.set_behaviour(behaviour(&rules));
        let lines = log_lines(&mut w, 4 * MINUTES_PER_DAY);
        assert!(lines
            .iter()
            .all(|l| !l.contains(" work ") && !l.contains(" overtime ")));
        assert!(lines.iter().any(|l| l.contains(" wander ")));
    }

    #[test]
    fn greedy_agents_work_overtime_and_relaxed_ones_do_not() {
        let mut c = config();
        c.agents = 2;
        c.populations = vec![
            Population {
                name: "greedy".into(),
                weight: 1,
                traits: vec![(1000, 1000), (0, 0)],
            },
            Population {
                name: "relaxed".into(),
                weight: 1,
                traits: vec![(0, 0), (1000, 1000)],
            },
        ];
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 5 * MINUTES_PER_DAY);
        assert!(lines.iter().any(|l| l.contains(" a0 overtime ")));
        assert!(!lines.iter().any(|l| l.contains(" a1 overtime ")));
    }

    #[test]
    fn agents_walk_to_work_and_home() {
        let mut c = config();
        c.agents = 1;
        let mut w = World::new(1, c);
        let lines = log_lines(&mut w, 3 * MINUTES_PER_DAY);
        let field = config().businesses[0].pos;
        assert!(lines
            .iter()
            .any(|l| l.contains(" travel ") && l.ends_with(&field.to_string())));
        let home = w.housing().config(w.home(0).expect("has a home")).pos;
        assert!(lines
            .iter()
            .any(|l| l.contains(" travel ") && l.ends_with(&home.to_string())));
        // Work only ever starts at the workplace.
        let mut w = World::new(1, {
            let mut c = config();
            c.agents = 1;
            c
        });
        w.run_until(3 * MINUTES_PER_DAY, &mut |e| {
            if let LogEntry::Started { action, .. } = e {
                if action.kind == ActionKind::Work {
                    assert_eq!(action.target, field);
                }
            }
        });
    }

    #[test]
    fn interrupted_travel_stops_on_the_way() {
        let a = Pos::new(0, 0);
        let mut w = World::new(1, {
            let mut c = config();
            c.agents = 1;
            c
        });
        w.pos[0] = a;
        w.action[0] = Action {
            kind: ActionKind::Travel,
            started: 0,
            ends: 40,
            start_fatigue: 0,
            recovery_per_hour: 0,
            target: Pos::new(10, 10),
        };
        w.settle(0, 9, &mut |_| {});
        assert_eq!(w.pos(0), Pos::new(4, 0));
    }

    #[test]
    fn homeless_sleep_recovers_slower() {
        let mut c = config();
        c.agents = 2;
        let mut w = World::new(1, c);
        w.home[1] = 0;
        w.now = 23 * 60;
        w.fatigue[0] = LazyNeed::new(400, w.now);
        w.fatigue[1] = LazyNeed::new(400, w.now);
        w.hunger[0] = LazyNeed::new(0, w.now);
        w.hunger[1] = LazyNeed::new(0, w.now);
        let homeless = w.decide(0, w.now);
        w.pos[1] = w.housing.config(0).pos;
        let housed = w.decide(1, w.now);
        assert_eq!(
            (homeless.kind, housed.kind),
            (ActionKind::Sleep, ActionKind::Sleep)
        );
        assert_eq!(housed.recovery_per_hour, 80);
        assert_eq!(homeless.recovery_per_hour, 40);
        assert!(homeless.ends > housed.ends, "{homeless:?} {housed:?}");
    }

    #[test]
    fn who_cannot_pay_rent_is_evicted() {
        let mut c = config();
        c.agents = 2;
        let mut w = World::new(1, c);
        w.run_until(2 * MINUTES_PER_DAY - 1, &mut |_| {});
        let h = w.home(0).expect("housed on day 0");
        w.money[0] = 0;
        let mut evicted = false;
        w.run_until(2 * MINUTES_PER_DAY, &mut |e| {
            evicted |= matches!(e, LogEntry::Home { agent: 0, change: HomeChange::Evicted, house, .. } if house == h);
        });
        assert!(evicted);
    }

    #[test]
    fn tenants_match_homes_and_beds() {
        let mut w = World::new(8, config());
        for _ in 0..200 {
            w.run_days(1);
            for h in 0..w.housing().len() {
                let listed = (0..w.agent_count())
                    .filter(|&a| w.home(a) == Some(h))
                    .count() as u32;
                assert_eq!(w.housing().tenants(h), listed);
                assert!(listed <= w.housing().config(h).beds);
            }
        }
    }

    #[test]
    fn land_is_dearer_near_houses() {
        let w = World::new(1, config());
        let house = w.housing().config(0).pos;
        let far = Pos::new(15, 0);
        assert_eq!(w.land_price(far), 100);
        assert!(w.land_price(house) > w.land_price(Pos::new(house.x + 2, house.y)));
        assert!(w.land_price(house) > 100);
    }
}
