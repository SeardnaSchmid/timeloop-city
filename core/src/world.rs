//! Event-driven world. State lives in dense arrays (index = agent ID).
//!
//! An agent only computes when its `Wake` event fires: it settles the action
//! it was doing, picks the next one and schedules its next wake-up. Between
//! wake-ups nothing is ticked; needs grow lazily. Once a day, `DayStart` runs
//! the market: rent and homes, restocking, prices, wages, firms closing,
//! layoffs, one firm founded, hiring and dividends.

use std::fmt;

use crate::assoc::{avar, AssocKind, AssociationConfig, RuleKind, ASSOC_VARS, WEEK_DAYS};
use crate::behaviour::{var, Allowed, Behaviour, Candidate, Explanation, Population, MAX_VARS};
use crate::economy::{BusinessConfig, BusinessId, Economy, GoodId, MarketConfig};
use crate::hash::{StateHash, StateHasher};
use crate::housing::{HouseConfig, Housing, HousingConfig};
use crate::life::{self, LifeConfig, Stage, DAYS_PER_YEAR};
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
/// In a founded firm that is the CEO; the town's businesses hire one. Without
/// it a business with no cash can never produce again.
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
    /// Ageing, death and new people; `None`: everyone works forever.
    pub life: Option<LifeConfig>,
    /// Guilds and the town council, in ID order.
    pub associations: Vec<AssociationConfig>,

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
    /// Left to run a firm of their own.
    Left,
    /// The firm closed.
    Closed,
    Retired,
    Died,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeChange {
    /// Died at this age (years), leaving money and firm to this many children.
    Died { age: u32, heirs: u32 },
    /// A descendant of `parent` takes the free place.
    Born { parent: u32 },
    /// A newcomer to town takes the free place.
    Arrived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmChange {
    Founded { at_pos: Pos },
    Closed,
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
    Firm {
        at: Minute,
        agent: u32,
        business: BusinessId,
        change: FirmChange,
    },
    Life {
        at: Minute,
        agent: u32,
        change: LifeChange,
    },
    /// An association set a rule to a new value.
    Rule {
        at: Minute,
        assoc: u32,
        rule: RuleKind,
        value: i64,
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
                    JobChange::Left => "left",
                    JobChange::Closed => "lost the job, closed:",
                    JobChange::Retired => "retired from",
                    JobChange::Died => "died while working at",
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
            LogEntry::Firm {
                at,
                agent,
                business,
                change,
            } => match change {
                FirmChange::Founded { at_pos } => {
                    write!(f, "{} a{agent} founded b{business} at {at_pos}", clock(at))
                }
                FirmChange::Closed => write!(f, "{} a{agent} closed b{business}", clock(at)),
            },
            LogEntry::Life { at, agent, change } => match change {
                LifeChange::Died { age, heirs } => {
                    write!(f, "{} a{agent} died aged {age}, {heirs} heirs", clock(at))
                }
                LifeChange::Born { parent } => {
                    write!(f, "{} a{agent} is a child of a{parent}", clock(at))
                }
                LifeChange::Arrived => write!(f, "{} a{agent} moved to town", clock(at)),
            },
            LogEntry::Rule {
                at,
                assoc,
                rule,
                value,
            } => write!(
                f,
                "{} association {assoc} sets {} = {value}",
                clock(at),
                rule.name()
            ),
        }
    }
}

/// Totals for one moment, for statistics output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub minute: Minute,
    pub money_agents: i64,
    pub money_businesses: i64,
    /// All money in the world: agents, businesses and houses. Constant.
    pub money_total: i64,
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
    /// Open firms founded by agents.
    pub firms: u32,
    pub firms_founded_total: u64,
    pub firms_closed_total: u64,
    /// Money held by firm owners (agents), summed.
    pub money_owners: i64,
    pub deaths_total: u64,
    pub births_total: u64,
    pub arrivals_total: u64,
    /// Mean age in years.
    pub mean_age: u32,
    pub in_education: u32,
    pub retired: u32,
    /// What the cheapest meal costs a customer (tax included), in percent
    /// of the food's starting price.
    pub price_index: i64,
    /// Treasury per association, in association order.
    pub assoc_cash: Vec<i64>,
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
    /// Firm the agent owns, or `NO_EMPLOYER`.
    owns: Vec<BusinessId>,
    firms_founded: u64,
    firms_closed: u64,
    /// Day of birth; negative for people older than the world.
    birth_day: Vec<i64>,
    /// Lifespan in days.
    lifespan: Vec<u64>,
    /// How many people have had this agent ID before.
    generation: Vec<u32>,
    /// Parent (agent ID and its generation then), or `NO_PARENT`.
    parent: Vec<(u32, u32)>,
    deaths: u64,
    births: u64,
    arrivals: u64,
    /// Wage share collected for those in education or retired, paid out daily.
    pension_fund: i64,
    /// Treasury per association.
    assoc_cash: Vec<i64>,
    /// Current value of every rule, per association in rule order.
    rule_values: Vec<Vec<i64>>,
}

const NO_PARENT: (u32, u32) = (u32::MAX, u32::MAX);

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
        let mut birth_day = vec![0i64; n];
        let mut lifespan = vec![u64::MAX; n];
        if let Some(l) = &config.life {
            for a in 0..n {
                let y = DAYS_PER_YEAR;
                let lo = l.start_age_min as u64 * y;
                let age = life::roll(
                    seed,
                    a as u32,
                    0,
                    0,
                    lo,
                    (l.start_age_max as u64 * y).max(lo),
                );
                birth_day[a] = -(age as i64);
                lifespan[a] = life::lifespan(l, seed, a as u32, 0, age);
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
            owns: vec![NO_EMPLOYER; n],
            firms_founded: 0,
            firms_closed: 0,
            birth_day,
            lifespan,
            generation: vec![0; n],
            parent: vec![NO_PARENT; n],
            deaths: 0,
            births: 0,
            arrivals: 0,
            pension_fund: 0,
            assoc_cash: vec![0; config.associations.len()],
            rule_values: (config.associations.iter())
                .map(|a| vec![0; a.rules.len()])
                .collect(),
            config,
        }
    }

    /// Age in days (0 without a life course).
    pub fn age_days(&self, agent: u32) -> u64 {
        (self.day() as i64 - self.birth_day[agent as usize]).max(0) as u64
    }

    pub fn stage(&self, agent: u32) -> Stage {
        self.config
            .life
            .as_ref()
            .map_or(Stage::Working, |l| l.stage(self.age_days(agent)))
    }

    /// The living parent, if this agent descends from one.
    pub fn parent(&self, agent: u32) -> Option<u32> {
        let (p, g) = self.parent[agent as usize];
        (p != u32::MAX && self.generation[p as usize] == g).then_some(p)
    }

    /// People who have lived under this agent ID before the current one.
    pub fn generation(&self, agent: u32) -> u32 {
        self.generation[agent as usize]
    }

    fn children(&self, agent: u32) -> Vec<u32> {
        (0..self.config.agents)
            .filter(|&a| self.parent(a) == Some(agent))
            .collect()
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

    /// The firm `agent` owns and runs as CEO.
    pub fn owned_firm(&self, agent: u32) -> Option<BusinessId> {
        Some(self.owns[agent as usize]).filter(|&b| b != NO_EMPLOYER)
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
        let target = self.safety_target();
        if target <= 0 {
            return 0;
        }
        let covered = self.money[agent as usize].max(0).min(target) * NEED_MAX as i64 / target;
        NEED_MAX - covered as u32
    }

    /// Savings at which an agent feels safe.
    fn safety_target(&self) -> i64 {
        let b = &self.config.behaviour;
        b.safety_reserve_days
            * b.meals_per_day
            * self.with_tax(self.config.food, self.food_list_price())
    }

    /// Money held by agents and businesses together. Constant over time.
    pub fn money_supply(&self) -> i64 {
        self.money.iter().sum::<i64>()
            + self.economy.total_cash()
            + self.housing.total_cash()
            + self.pension_fund
            + self.assoc_cash.iter().sum::<i64>()
    }

    pub fn stats(&self) -> Stats {
        let e = &self.economy;
        let employed = self.employer.iter().filter(|&&b| b != NO_EMPLOYER).count() as u32;
        Stats {
            minute: self.now,
            money_agents: self.money.iter().sum(),
            money_businesses: e.total_cash(),
            money_total: self.money_supply(),
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
            firms: self.owns.iter().filter(|&&b| b != NO_EMPLOYER).count() as u32,
            firms_founded_total: self.firms_founded,
            firms_closed_total: self.firms_closed,
            deaths_total: self.deaths,
            births_total: self.births,
            arrivals_total: self.arrivals,
            mean_age: if self.config.life.is_some() && self.config.agents > 0 {
                ((0..self.config.agents)
                    .map(|a| self.age_days(a))
                    .sum::<u64>()
                    / self.config.agents as u64
                    / DAYS_PER_YEAR) as u32
            } else {
                0
            },
            in_education: (0..self.config.agents)
                .filter(|&a| self.stage(a) == Stage::Education)
                .count() as u32,
            retired: (0..self.config.agents)
                .filter(|&a| self.stage(a) == Stage::Retired)
                .count() as u32,
            price_index: self.price_index(),
            assoc_cash: self.assoc_cash.clone(),
            money_owners: (0..self.config.agents as usize)
                .filter(|&i| self.owns[i] != NO_EMPLOYER)
                .map(|i| self.money[i])
                .sum(),
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
        self.life_course(at, log);
        self.collect_rent(at, log);
        self.housing.adjust_rents();
        self.choose_homes(at, log);
        if self.day().is_multiple_of(WEEK_DAYS) {
            self.associations_meet(at, log);
        }
        self.economy.restock_inputs(hours);
        self.economy.adjust_prices();
        let job_seekers = self.employer.contains(&NO_EMPLOYER);
        self.economy.adjust_wages(hours, job_seekers);
        self.economy.update_broke_days(hours);
        self.close_firms(at, log);

        for b in 0..self.economy.len() {
            if !self.economy.is_open(b) {
                continue;
            }
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

        self.found_firm(at, log);

        // Core crews first, so every link of the chain can produce.
        for b in 0..self.economy.len() {
            if !self.economy.is_open(b) {
                continue;
            }
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
        e.is_open(b)
            && workers < e.config(b).slots
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
    /// savings, then the highest ID. Never the owner.
    fn layoff_candidate(&self, b: BusinessId) -> u32 {
        (0..self.config.agents)
            .filter(|&a| self.employer[a as usize] == b && self.owns[a as usize] != b)
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
            .filter(|&a| {
                self.employer[a as usize] == NO_EMPLOYER && self.stage(a) == Stage::Working
            })
            .min_by_key(|&a| (self.money[a as usize], a))
    }

    /// Pays every business's surplus cash out: a firm's to its owner after
    /// the profit tax; the tax and the town businesses' surplus in equal
    /// shares to agents whose savings are below the safety reserve
    /// (everyone, if nobody is). That town welfare keeps money from piling
    /// up where nobody spends it, which would kill demand.
    fn pay_dividends(&mut self) {
        let hours = self.config.work_hours();
        // Comfort spending comes first and joins the rent in the town's payout.
        let target = self.safety_target();
        let permille = self.config.behaviour.comfort_spend_permille;
        let mut comfort = 0;
        for m in &mut self.money {
            let spend = (*m - target).max(0) * permille / 1000;
            *m -= spend;
            comfort += spend;
        }
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
        let tax = (self.config.behaviour.founding.as_ref()).map_or(0, |f| f.profit_tax_percent);
        for b in 0..self.economy.len() {
            let mut surplus = self.economy.take_surplus(b, hours);
            if let Some(owner) = self.economy.owner(b) {
                let dividend = surplus * (100 - tax) / 100;
                self.money[owner as usize] += dividend;
                surplus -= dividend;
            }
            let share = surplus / n;
            recipients.iter().for_each(|&i| self.money[i] += share);
            self.economy.add_cash(b, surplus - share * n);
        }
        // Houses have no costs yet: all rent flows back the same way.
        let mut rent = comfort;
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

    /// What founding a firm on the blueprint `t` looks like today: lowest
    /// price and unit cost among the open producers, shortage, their count
    /// and days since the newest of them opened.
    fn market_vars(&self, t: BusinessId) -> [i64; 5] {
        let e = &self.economy;
        let good = e.config(t).output;
        let producers: Vec<BusinessId> = e.producers(good).collect();
        let lowest = |f: &dyn Fn(BusinessId) -> i64| producers.iter().map(|&b| f(b)).min();
        [
            lowest(&|b| e.price(b)).unwrap_or(e.config(t).start_price),
            lowest(&|b| e.unit_cost(b)).unwrap_or(0),
            e.shortage(good) as i64,
            producers.len() as i64,
            producers
                .iter()
                .map(|&b| self.day().saturating_sub(e.opened_day(b)) as i64)
                .min()
                .unwrap_or(i64::MAX),
        ]
    }

    /// Blueprints for founded firms: the first business of the world file
    /// making each good.
    fn blueprints(&self) -> Vec<BusinessId> {
        let b = &self.config.businesses;
        (0..b.len())
            .filter(|&i| b[..i].iter().all(|o| o.output != b[i].output))
            .map(|i| i as BusinessId)
            .collect()
    }

    /// The best founding of the day, if any is allowed: (agent, blueprint).
    /// Ties go to the lower agent ID, then the lower blueprint.
    pub fn founding_choice(&self) -> Option<(u32, BusinessId, i64)> {
        let rule = self.config.behaviour.founding.as_ref()?;
        let open = self.owns.iter().filter(|&&b| b != NO_EMPLOYER).count() as u32;
        if open >= rule.max_firms {
            return None;
        }
        let kinds: Vec<(BusinessId, [i64; 5])> = (self.blueprints().into_iter())
            .map(|t| (t, self.market_vars(t)))
            .collect();
        let mut best: Option<(u32, BusinessId, i64)> = None;
        for agent in 0..self.config.agents {
            if self.owns[agent as usize] != NO_EMPLOYER || self.stage(agent) != Stage::Working {
                continue;
            }
            let mut v = self.vars(agent, self.now);
            for &(t, m) in &kinds {
                v[var::MARKET_PRICE..=var::MARKET_NEWEST_DAYS].copy_from_slice(&m);
                let fee = self.founding_fee(self.economy.config(t).output);
                v[var::MARKET_FEE] = fee;
                if v[var::MONEY] < rule.capital + fee
                    || rule.when.as_ref().is_some_and(|f| f.eval(&v) <= 0)
                {
                    continue;
                }
                let score = rule.score.eval(&v);
                if best.is_none_or(|(_, _, s)| score > s) {
                    best = Some((agent, t, score));
                }
            }
        }
        best
    }

    /// At most one new firm a day, next to the founder's home.
    fn found_firm(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let Some((agent, t, _)) = self.founding_choice() else {
            return;
        };
        let i = agent as usize;
        let near = self
            .home(agent)
            .map_or(self.pos[i], |h| self.housing.config(h).pos);
        let Some(pos) = self.map.free_parcel_near(near) else {
            return;
        };
        let capital = self
            .config
            .behaviour
            .founding
            .as_ref()
            .map_or(0, |f| f.capital);
        let [price, ..] = self.market_vars(t);
        let blueprint = &self.config.businesses[t as usize];
        let config = BusinessConfig {
            name: format!("{} (a{agent})", blueprint.name),
            pos,
            start_cash: capital,
            start_price: price,
            start_wage: self.economy.wage(t),
            start_stock: 0,
            ..blueprint.clone()
        };
        self.money[i] -= capital;
        let good = blueprint.output;
        for a in 0..self.assoc_cash.len() {
            let fee = self.rule(a, RuleKind::FoundingFee, good);
            self.money[i] -= fee;
            self.assoc_cash[a] += fee;
        }
        let b = self
            .economy
            .open_business(config, agent, at / MINUTES_PER_DAY);
        self.map.build(pos, Building::Business(b));
        self.owns[i] = b;
        self.firms_founded += 1;
        log(LogEntry::Firm {
            at,
            agent,
            business: b,
            change: FirmChange::Founded { at_pos: pos },
        });
        if self.employer[i] != NO_EMPLOYER {
            self.leave_job(agent, self.employer[i], JobChange::Left, at, log);
        }
        self.hire(agent, b, at, log);
    }

    /// Every open business reads the close rule (a firm through its CEO);
    /// a closing one lets everyone go and hands its cash to the owner. A
    /// town business that closes stays closed; founders can take its place.
    fn close_firms(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let Some(rule) = self.config.behaviour.founding.as_ref() else {
            return;
        };
        let closing: Vec<BusinessId> = (0..self.economy.len())
            .filter(|&b| {
                if !self.economy.is_open(b) {
                    return false;
                }
                let v = match self.economy.owner(b) {
                    Some(owner) => self.vars(owner, self.now),
                    None => {
                        let mut v = [0; MAX_VARS];
                        self.firm_vars(b, self.now, &mut v);
                        v
                    }
                };
                rule.close_when.eval(&v) > 0
            })
            .collect();
        for b in closing {
            let owner = self.economy.owner(b);
            let cash = self.close_business(b, at, log);
            match owner {
                Some(o) => self.money[o as usize] += cash,
                // Town cash goes to the needy with today's rent payout.
                None => self.housing.add_cash(0, cash),
            }
        }
    }

    /// Lets everyone go, clears the parcel and returns the cash.
    fn close_business(&mut self, b: BusinessId, at: Minute, log: &mut impl FnMut(LogEntry)) -> i64 {
        for a in 0..self.config.agents {
            if self.employer[a as usize] == b {
                self.leave_job(a, b, JobChange::Closed, at, log);
            }
        }
        let owner = self.economy.owner(b);
        let cash = self.economy.close(b);
        self.map.clear(self.economy.config(b).pos);
        if let Some(o) = owner {
            self.owns[o as usize] = NO_EMPLOYER;
        }
        self.firms_closed += 1;
        log(LogEntry::Firm {
            at,
            agent: owner.unwrap_or(u32::MAX),
            business: b,
            change: FirmChange::Closed,
        });
        cash
    }

    /// Daily: workers who reach retirement leave their job; whoever reached
    /// the end of their life dies and someone new takes the place; the
    /// pension fund is shared out among those in education or retired.
    fn life_course(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        if self.config.life.is_none() {
            return;
        }
        for agent in 0..self.config.agents {
            let i = agent as usize;
            if self.age_days(agent) >= self.lifespan[i] {
                self.die(agent, at, log);
                self.enter(agent, at, log);
            } else if self.employer[i] != NO_EMPLOYER && self.stage(agent) != Stage::Working {
                self.leave_job(agent, self.employer[i], JobChange::Retired, at, log);
            }
        }
        let receivers: Vec<usize> = (0..self.config.agents)
            .filter(|&a| self.stage(a) != Stage::Working)
            .map(|a| a as usize)
            .collect();
        if !receivers.is_empty() {
            let share = self.pension_fund / receivers.len() as i64;
            for &i in &receivers {
                self.money[i] += share;
            }
            self.pension_fund -= share * receivers.len() as i64;
        }
    }

    /// Money goes, after the inheritance tax, to the children in equal shares, the firm to the first
    /// child without one (who runs it if of working age); without children
    /// both go to the town (the firm closes, its cash joins the welfare).
    fn die(&mut self, agent: u32, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let i = agent as usize;
        if self.employer[i] != NO_EMPLOYER {
            self.leave_job(agent, self.employer[i], JobChange::Died, at, log);
        }
        if self.home[i] != NO_HOME {
            self.set_home(agent, NO_HOME, HomeChange::MovedOut, at, log);
        }
        let heirs = self.children(agent);
        let mut money = std::mem::take(&mut self.money[i]);
        let tax_percent = self
            .config
            .life
            .as_ref()
            .map_or(0, |l| l.inheritance_tax_percent);
        let tax = money.max(0) * tax_percent as i64 / 100;
        self.housing.add_cash(0, tax);
        money -= tax;
        if heirs.is_empty() {
            self.housing.add_cash(0, money);
        } else {
            let share = money / heirs.len() as i64;
            for &h in &heirs {
                self.money[h as usize] += share;
            }
            self.money[heirs[0] as usize] += money - share * heirs.len() as i64;
        }
        let b = std::mem::replace(&mut self.owns[i], NO_EMPLOYER);
        if b != NO_EMPLOYER {
            match heirs
                .iter()
                .copied()
                .find(|&h| self.owns[h as usize] == NO_EMPLOYER)
            {
                Some(h) => {
                    self.economy.set_owner(b, h);
                    self.owns[h as usize] = b;
                    if self.stage(h) == Stage::Working {
                        let job = self.employer[h as usize];
                        if job != NO_EMPLOYER {
                            self.leave_job(h, job, JobChange::Left, at, log);
                        }
                        self.hire(h, b, at, log);
                    }
                }
                None => {
                    let cash = self.close_business(b, at, log);
                    self.housing.add_cash(0, cash);
                }
            }
        }
        self.deaths += 1;
        log(LogEntry::Life {
            at,
            agent,
            change: LifeChange::Died {
                age: (self.age_days(agent) / DAYS_PER_YEAR) as u32,
                heirs: heirs.len() as u32,
            },
        });
    }

    /// A new person in a free slot: a child of a living agent of parent age
    /// (inheriting its population and traits close to its own) or a
    /// newcomer of the slot's population. Starts at `entry_age` with no money.
    fn enter(&mut self, agent: u32, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let Some(l) = self.config.life.clone() else {
            return;
        };
        let i = agent as usize;
        self.generation[i] += 1;
        let gen = self.generation[i];
        let seed = self.seed;
        let y = DAYS_PER_YEAR;
        let parents: Vec<u32> = (0..self.config.agents)
            .filter(|&a| {
                let years = self.age_days(a) / y;
                a != agent && (l.parent_age_min as u64..=l.parent_age_max as u64).contains(&years)
            })
            .collect();
        let born =
            !parents.is_empty() && life::roll(seed, agent, gen, 3, 0, 99) < l.birth_percent as u64;
        let k = self.config.traits.len();
        let parent = if born {
            let p = parents[life::roll(seed, agent, gen, 4, 0, parents.len() as u64 - 1) as usize];
            self.population[i] = self.population[p as usize];
            let pop = &self.config.populations[self.population[i] as usize];
            for t in 0..k {
                // Close to the parent: +-100, kept in the population's range.
                let pv = self.traits[p as usize * k + t] as i64;
                let d = life::roll(seed, agent, gen, 16 + t as u64, 0, 200) as i64 - 100;
                let (lo, hi) = pop.traits.get(t).copied().unwrap_or((0, NEED_MAX));
                self.traits[i * k + t] = (pv + d).clamp(lo as i64, hi as i64) as u32;
            }
            self.parent[i] = (p, self.generation[p as usize]);
            self.pos[i] = self
                .home(p)
                .map_or(self.pos[p as usize], |h| self.housing.config(h).pos);
            self.births += 1;
            Some(p)
        } else {
            let pop = self.config.populations[self.population[i] as usize].clone();
            for (t, &(lo, hi)) in pop.traits.iter().enumerate() {
                self.traits[i * k + t] =
                    life::roll(seed, agent, gen, 16 + t as u64, lo as u64, hi as u64) as u32;
            }
            self.parent[i] = NO_PARENT;
            self.pos[i] = self.config.map.center();
            self.arrivals += 1;
            None
        };
        let now = self.now;
        let entry = l.entry_age as u64 * y;
        self.birth_day[i] = self.day() as i64 - entry as i64;
        self.lifespan[i] = life::lifespan(&l, seed, agent, gen, entry);
        self.hunger[i] = LazyNeed::new(0, now);
        self.fatigue[i] = LazyNeed::new(0, now);
        self.action[i] = Action {
            started: now,
            ends: now,
            target: self.pos[i],
            ..Action::IDLE
        };
        self.work_minutes[i] = 0;
        self.work_at_day_start[i] = 0;
        self.work_recent[i] = 0;
        log(LogEntry::Life {
            at,
            agent,
            change: match parent {
                Some(parent) => LifeChange::Born { parent },
                None => LifeChange::Arrived,
            },
        });
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
            let price = self.economy.sell_one(seller);
            let food = self.config.food;
            self.money[i] -= price;
            for a in 0..self.assoc_cash.len() {
                let tax = price * self.rule(a, RuleKind::SalesTaxPercent, food) / 100;
                self.money[i] -= tax;
                self.assoc_cash[a] += tax;
            }
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
                let pension = self
                    .config
                    .life
                    .as_ref()
                    .map_or(0, |l| paid * l.pension_percent as i64 / 100);
                self.pension_fund += pension;
                self.money[i] += paid - pension;
                if paid < owed && self.economy.workers(b) > CORE_CREW && self.owns[i] != b {
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
    /// What the cheapest food in stock costs a customer, tax included.
    fn food_price(&self) -> Option<i64> {
        self.economy
            .cheapest_seller(self.config.food)
            .map(|b| self.with_tax(self.config.food, self.economy.price(b)))
    }

    /// A shop price plus the sales taxes on `good`.
    fn with_tax(&self, good: GoodId, price: i64) -> i64 {
        let tax: i64 = (0..self.assoc_cash.len())
            .map(|a| price * self.rule(a, RuleKind::SalesTaxPercent, good) / 100)
            .sum();
        price + tax
    }

    /// Current value of association `a`'s rule of `kind` for `good`; 0 if
    /// it has none or does not cover the good.
    fn rule(&self, a: usize, kind: RuleKind, good: GoodId) -> i64 {
        let c = &self.config.associations[a];
        match c.rule_index(kind) {
            Some(r) if c.covers(good) => self.rule_values[a][r],
            _ => 0,
        }
    }

    /// Fees for founding a firm making `good`, summed over associations.
    fn founding_fee(&self, good: GoodId) -> i64 {
        (0..self.assoc_cash.len())
            .map(|a| self.rule(a, RuleKind::FoundingFee, good))
            .sum()
    }

    fn price_index(&self) -> i64 {
        let food = self.config.food;
        let start = (self.config.businesses.iter())
            .find(|b| b.output == food)
            .map_or(0, |b| b.start_price);
        if start <= 0 {
            return 0;
        }
        self.with_tax(food, self.food_list_price()) * 100 / start
    }

    /// Current value of every rule of association `a`, in rule order.
    pub fn association_rule_values(&self, a: u32) -> &[i64] {
        &self.rule_values[a as usize]
    }

    /// Members of association `a`: businesses making its goods, or people.
    pub fn association_members(&self, a: u32) -> u32 {
        let c = &self.config.associations[a as usize];
        match &c.kind {
            AssocKind::Council => self.config.agents,
            AssocKind::Guild { .. } => (0..self.economy.len())
                .filter(|&b| self.economy.is_open(b) && c.covers(self.economy.config(b).output))
                .count() as u32,
        }
    }

    /// Weekly: dues come in, treasuries go out (a guild's to its member
    /// businesses, the council's to the welfare), then every rule is decided
    /// anew and price bounds follow.
    fn associations_meet(&mut self, at: Minute, log: &mut impl FnMut(LogEntry)) {
        let n = self.config.associations.len();
        for b in 0..self.economy.len() {
            let revenue = self.economy.take_revenue(b);
            if !self.economy.is_open(b) {
                continue;
            }
            let good = self.economy.config(b).output;
            for a in 0..n {
                let due = revenue * self.rule(a, RuleKind::DuesPercent, good) / 100;
                self.assoc_cash[a] += self.economy.pay(b, due);
            }
        }
        for a in 0..n {
            let cash = std::mem::take(&mut self.assoc_cash[a]);
            let c = &self.config.associations[a];
            match &c.kind {
                AssocKind::Council => self.housing.add_cash(0, cash),
                AssocKind::Guild { .. } => {
                    let members: Vec<BusinessId> = (0..self.economy.len())
                        .filter(|&b| {
                            self.economy.is_open(b) && c.covers(self.economy.config(b).output)
                        })
                        .collect();
                    if members.is_empty() {
                        self.assoc_cash[a] = cash;
                        continue;
                    }
                    let share = cash / members.len() as i64;
                    for &b in &members {
                        self.economy.add_cash(b, share);
                    }
                    self.assoc_cash[a] = cash - share * members.len() as i64;
                }
            }
        }
        for a in 0..n {
            let c = &self.config.associations[a];
            let good = match &c.kind {
                AssocKind::Guild { goods } => goods.first().copied(),
                AssocKind::Council => Some(self.config.food),
            };
            let mut v = [0i64; ASSOC_VARS.len()];
            if let Some(t) = good.and_then(|g| self.blueprint_of(g)) {
                v[avar::MARKET..avar::MARKET + 4].copy_from_slice(&self.market_vars(t)[..4]);
            }
            v[avar::CASH] = self.assoc_cash[a];
            v[avar::MEMBERS] = self.association_members(a as u32) as i64;
            v[avar::DAY] = self.day() as i64;
            for (r, rule) in c.rules.iter().enumerate() {
                let value = rule.value.eval(&v).max(0);
                if value != self.rule_values[a][r] {
                    self.rule_values[a][r] = value;
                    log(LogEntry::Rule {
                        at,
                        assoc: a as u32,
                        rule: rule.kind,
                        value,
                    });
                }
            }
        }
        for g in 0..self.config.goods.len() as GoodId {
            let (mut lo, mut hi) = (0, i64::MAX);
            for (a, c) in self.config.associations.iter().enumerate() {
                if !c.covers(g) {
                    continue;
                }
                if let Some(r) = c.rule_index(RuleKind::MinPrice) {
                    lo = lo.max(self.rule_values[a][r]);
                }
                if let Some(r) = c.rule_index(RuleKind::MaxPrice) {
                    hi = hi.min(self.rule_values[a][r]);
                }
            }
            self.economy.set_price_bounds(g, lo, hi);
        }
    }

    fn blueprint_of(&self, good: GoodId) -> Option<BusinessId> {
        (self.config.businesses.iter())
            .position(|b| b.output == good)
            .map(|b| b as BusinessId)
    }

    fn can_buy_food(&self, i: usize) -> bool {
        self.food_price().is_some_and(|p| self.money[i] >= p)
    }

    /// Lowest shop price of food among open makers, in stock or not. 0 if
    /// nobody makes food.
    fn food_list_price(&self) -> i64 {
        (self.economy.producers(self.config.food))
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
        v[var::FOOD_PRICE] = self.with_tax(c.food, self.food_list_price());
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
        if let Some(f) = &c.behaviour.founding {
            v[var::FIRM_CAPITAL] = f.capital;
        }
        if self.owns[i] != NO_EMPLOYER {
            self.firm_vars(self.owns[i], now, &mut v);
        }
        if self.config.life.is_some() {
            v[var::AGE] = (self.age_days(agent) / DAYS_PER_YEAR) as i64;
            let stage = self.stage(agent);
            v[var::IN_EDUCATION] = (stage == Stage::Education) as i64;
            v[var::RETIRED] = (stage == Stage::Retired) as i64;
        }
        for (t, &value) in self.traits(agent).iter().enumerate() {
            v[var::TRAITS + t] = value as i64;
        }
        v
    }

    fn firm_vars(&self, b: BusinessId, now: Minute, v: &mut [i64; MAX_VARS]) {
        let e = &self.economy;
        v[var::OWNS_FIRM] = 1;
        v[var::FIRM_CASH] = e.cash(b);
        v[var::FIRM_BROKE_DAYS] = e.broke_days(b) as i64;
        v[var::FIRM_AGE_DAYS] = (now / MINUTES_PER_DAY).saturating_sub(e.opened_day(b)) as i64;
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
        h.write_u32_slice(&self.owns);
        h.write_u64(self.firms_founded);
        h.write_u64(self.firms_closed);
        h.write_i64_slice(&self.birth_day);
        self.lifespan.iter().for_each(|&l| h.write_u64(l));
        h.write_u32_slice(&self.generation);
        for &(p, g) in &self.parent {
            h.write_u32(p);
            h.write_u32(g);
        }
        h.write_u64(self.deaths);
        h.write_u64(self.births);
        h.write_u64(self.arrivals);
        h.write_i64(self.pension_fund);
        h.write_i64_slice(&self.assoc_cash);
        self.rule_values.iter().for_each(|v| h.write_i64_slice(v));
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
            life: None,
            associations: Vec::new(),
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
            comfort_spend_permille: 0,
            meals_per_day: 3,
            housing: HousingRule {
                when: Some(compile(HOUSING.0)),
                score: compile(HOUSING.1),
                homeless_score: compile(HOUSING.2),
            },
            founding: None,
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

#[cfg(test)]
mod firm_tests {
    use super::tests::config;
    use super::*;
    use crate::behaviour::FoundingRule;
    use crate::formula::Formula;

    fn founding(when: &str, close: &str, tax: i64) -> WorldConfig {
        let mut c = config();
        let names = crate::behaviour::variable_names(&c.traits);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let f = |s: &str| Formula::compile(s, &names).unwrap();
        c.behaviour.founding = Some(FoundingRule {
            when: Some(f(when)),
            score: f("market.price - market.cost"),
            close_when: f(close),
            capital: 100,
            profit_tax_percent: tax,
            max_firms: 2,
        });
        c
    }

    fn log(w: &mut World, until: Minute) -> Vec<LogEntry> {
        let mut out = Vec::new();
        w.run_until(until, &mut |e| out.push(e));
        out
    }

    #[test]
    fn founder_pays_capital_runs_the_firm_and_money_stays() {
        // Only bread, and only from day 2 on.
        let mut w = World::new(
            1,
            founding(
                "market.firms == 1 && hour == 0 && market.price > 30",
                "0",
                0,
            ),
        );
        let supply = w.money_supply();
        let entries = log(&mut w, 3 * MINUTES_PER_DAY);
        let founded: Vec<_> = entries
            .iter()
            .filter_map(|e| match *e {
                LogEntry::Firm {
                    agent,
                    business,
                    change: FirmChange::Founded { at_pos },
                    ..
                } => Some((agent, business, at_pos)),
                _ => None,
            })
            .collect();
        assert!(!founded.is_empty(), "nobody founded a firm");
        let (agent, b, pos) = founded[0];
        let e = w.economy();
        assert_eq!(e.owner(b), Some(agent));
        assert_eq!(e.config(b).output, w.config().food);
        assert_eq!(w.owned_firm(agent), Some(b));
        assert_eq!(w.employer(agent), Some(b), "the founder works as CEO");
        assert_eq!(w.map().at(pos), Some(Building::Business(b)));
        assert_eq!(w.money_supply(), supply);
        assert!(founded.len() <= 2, "max_firms caps founding");
    }

    #[test]
    fn ceo_is_never_laid_off_and_gets_the_dividend() {
        let mut w = World::new(3, founding("market.firms == 1", "0", 0));
        let supply = w.money_supply();
        let mut ceos = Vec::new();
        for _ in 0..200 {
            let until = w.now() + MINUTES_PER_DAY;
            let entries = log(&mut w, until);
            for e in entries {
                if let LogEntry::Job {
                    agent,
                    business,
                    change: JobChange::LaidOff | JobChange::QuitUnpaid,
                    ..
                } = e
                {
                    assert_ne!(
                        w.economy().owner(business),
                        Some(agent),
                        "CEO left its own firm"
                    );
                }
            }
            for a in 0..w.agent_count() {
                if let Some(b) = w.owned_firm(a) {
                    assert_eq!(w.employer(a), Some(b));
                    ceos.push(a);
                }
            }
            assert_eq!(w.money_supply(), supply);
        }
        assert!(!ceos.is_empty());
    }

    #[test]
    fn a_closing_firm_frees_its_parcel_and_pays_out_its_cash() {
        let mut w = World::new(
            1,
            founding(
                "market.firms == 1 && firm.age_days == 0",
                "firm.age_days >= 3",
                0,
            ),
        );
        let supply = w.money_supply();
        let entries = log(&mut w, 10 * MINUTES_PER_DAY);
        let closed: Vec<_> = entries
            .iter()
            .filter_map(|e| match *e {
                LogEntry::Firm {
                    agent,
                    business,
                    change: FirmChange::Closed,
                    ..
                } => Some((agent, business)),
                _ => None,
            })
            .collect();
        // Town businesses are always older than three days too.
        assert!(
            closed.iter().any(|&(_, b)| b < 3),
            "town businesses follow the rule"
        );
        for &(agent, b) in &closed {
            let e = w.economy();
            if e.is_open(b) {
                continue; // reused by a later founding
            }
            assert_eq!(e.cash(b), 0);
            assert_eq!(e.workers(b), 0);
            assert_ne!(w.map().at(e.config(b).pos), Some(Building::Business(b)));
            if agent != u32::MAX {
                assert_ne!(w.owned_firm(agent), Some(b));
            }
        }
        assert_eq!(w.money_supply(), supply);
    }

    #[test]
    fn profit_tax_splits_the_surplus() {
        let mut w = World::new(1, founding("market.firms == 1", "0", 25));
        w.run_days(2);
        let (owner, b) = (0..w.agent_count())
            .find_map(|a| w.owned_firm(a).map(|b| (a, b)))
            .expect("a firm was founded");
        w.economy.add_cash(b, 10_000);
        let hours = w.config.work_hours();
        let surplus = w.economy.cash(b) - 3 * w.economy.reserve(b, hours) / 2;
        // Rich, so no welfare share; comfort spending is off in this config.
        w.money[owner as usize] += 100_000;
        let before = w.money(owner);
        let supply = w.money_supply();
        w.pay_dividends();
        assert_eq!(w.money(owner) - before, surplus * 75 / 100);
        assert_eq!(w.money_supply(), supply);
    }
}

#[cfg(test)]
mod life_tests {
    use super::tests::config;
    use super::*;
    use crate::economy::tests::chain;

    fn life_config() -> WorldConfig {
        let mut c = config();
        c.life = Some(LifeConfig {
            start_age_min: 16,
            start_age_max: 70,
            entry_age: 16,
            education_end_age: 18,
            retirement_age: 65,
            lifespan_min: 70,
            lifespan_max: 90,
            birth_percent: 100,
            parent_age_min: 20,
            parent_age_max: 50,
            pension_percent: 20,
            inheritance_tax_percent: 50,
        });
        c
    }

    #[test]
    fn only_working_age_people_have_jobs() {
        let mut w = World::new(1, life_config());
        let supply = w.money_supply();
        for _ in 0..60 {
            w.run_days(1);
            for a in 0..w.agent_count() {
                if w.stage(a) != Stage::Working {
                    assert_eq!(
                        w.employer(a),
                        None,
                        "a{a} works at age {}",
                        w.age_days(a) / 365
                    );
                }
            }
            assert_eq!(w.money_supply(), supply, "pension fund keeps money");
        }
        let s = w.stats();
        assert!(s.retired > 0 && s.retired < 25, "{} retired", s.retired);
    }

    #[test]
    fn children_inherit_money_after_tax_and_the_firm() {
        let mut w = World::new(1, life_config());
        w.run_days(1);
        let (parent, child) = (3, 4);
        w.parent[child as usize] = (parent, w.generation[parent as usize]);
        w.money[parent as usize] = 1000;
        let b = w.economy.open_business(chain()[2].clone(), parent, 1);
        w.owns[parent as usize] = b;
        let before = w.money(child);
        let supply = w.money_supply();
        let now = w.now;
        w.die(parent, now, &mut |_| {});
        assert_eq!(w.money(child) - before, 500);
        assert_eq!(w.economy.owner(b), Some(child));
        assert_eq!(w.owned_firm(child), Some(b));
        assert_eq!(w.parent(child), Some(parent), "slot not refilled yet");
        w.enter(parent, now, &mut |_| {});
        assert_eq!(w.parent(child), None, "the parent is gone");
        assert_eq!(w.money(parent), 0);
        assert_eq!(w.age_days(parent), 16 * DAYS_PER_YEAR);
        assert_eq!(w.generation(parent), 1);
        assert_eq!(w.money_supply(), supply);
    }

    #[test]
    fn without_children_estate_and_firm_go_to_the_town() {
        let mut w = World::new(2, life_config());
        w.run_days(1);
        let a = 5;
        let b = w.economy.open_business(chain()[2].clone(), a, 1);
        w.economy.add_cash(b, 300);
        w.owns[a as usize] = b;
        let supply = w.money_supply();
        let now = w.now;
        w.die(a, now, &mut |_| {});
        assert!(!w.economy.is_open(b));
        assert_eq!(w.money(a), 0);
        assert_eq!(w.money_supply(), supply);
    }

    #[test]
    fn the_dead_are_replaced_by_children_or_newcomers() {
        let mut w = World::new(3, life_config());
        let mut died = 0;
        for _ in 0..(10 * DAYS_PER_YEAR) {
            let until = w.now + MINUTES_PER_DAY;
            w.run_until(until, &mut |e| {
                if let LogEntry::Life {
                    change: LifeChange::Died { .. },
                    ..
                } = e
                {
                    died += 1;
                }
            });
        }
        let s = w.stats();
        assert!(died > 0);
        assert_eq!(s.deaths_total, died);
        assert_eq!(s.births_total + s.arrivals_total, died);
        assert!(s.births_total > 0, "birth_percent is 100");
        let children = (0..w.agent_count())
            .filter(|&a| w.parent(a).is_some())
            .count();
        assert!(children > 0);
    }

    #[test]
    fn without_a_life_course_nobody_ages() {
        let mut w = World::new(1, config());
        w.run_days(400);
        assert_eq!(w.stats().deaths_total, 0);
        assert!((0..w.agent_count()).all(|a| w.stage(a) == Stage::Working));
    }
}

#[cfg(test)]
mod assoc_tests {
    use super::tests::config;
    use super::*;
    use crate::assoc::Rule;
    use crate::behaviour::FoundingRule;
    use crate::economy::tests::BREAD;
    use crate::formula::Formula;

    fn rule(kind: RuleKind, src: &str) -> Rule {
        let value = Formula::compile(src, &ASSOC_VARS).unwrap();
        Rule { kind, value }
    }

    fn with(assocs: Vec<AssociationConfig>) -> WorldConfig {
        let mut c = config();
        c.associations = assocs;
        c
    }

    fn guild(rules: Vec<Rule>) -> AssociationConfig {
        AssociationConfig {
            name: "bakers".into(),
            kind: AssocKind::Guild { goods: vec![BREAD] },
            rules,
        }
    }

    fn council(rules: Vec<Rule>) -> AssociationConfig {
        AssociationConfig {
            name: "council".into(),
            kind: AssocKind::Council,
            rules,
        }
    }

    #[test]
    fn rules_are_decided_weekly_and_logged() {
        let mut w = World::new(
            1,
            with(vec![guild(vec![rule(RuleKind::MinPrice, "day + 1")])]),
        );
        let mut logged = Vec::new();
        w.run_until(15 * MINUTES_PER_DAY, &mut |e| {
            if let LogEntry::Rule { value, .. } = e {
                logged.push(value);
            }
        });
        assert_eq!(logged, [1, 8, 15]);
        assert_eq!(w.association_rule_values(0), [15]);
        assert_eq!(w.economy().price_bounds(BREAD), (15, i64::MAX));
    }

    #[test]
    fn a_minimum_price_holds_for_every_bakery() {
        let mut w = World::new(1, with(vec![guild(vec![rule(RuleKind::MinPrice, "90")])]));
        w.run_days(3);
        let e = w.economy();
        for b in e.producers(BREAD) {
            assert!(e.price(b) >= 90);
        }
        assert!(w.stats().price_index >= 90 * 100 / 35);
    }

    #[test]
    fn a_maximum_price_caps_the_council_wide() {
        let mut w = World::new(1, with(vec![council(vec![rule(RuleKind::MaxPrice, "4")])]));
        w.run_days(2);
        let e = w.economy();
        assert!((0..e.len()).all(|b| e.price(b) <= 4));
    }

    #[test]
    fn sales_tax_goes_to_the_treasury_and_money_stays() {
        let mut w = World::new(
            2,
            with(vec![council(vec![rule(RuleKind::SalesTaxPercent, "10")])]),
        );
        let supply = w.money_supply();
        w.run_until(MINUTES_PER_DAY * 6 + 600, &mut |_| {});
        assert!(w.stats().assoc_cash[0] > 0, "meals were taxed");
        assert_eq!(w.money_supply(), supply);
        // The council pays its treasury into welfare at the weekly meeting.
        w.run_until(7 * MINUTES_PER_DAY, &mut |_| {});
        assert_eq!(w.stats().assoc_cash[0], 0);
        assert_eq!(w.money_supply(), supply);
    }

    #[test]
    fn customers_pay_price_plus_tax() {
        let mut w = World::new(
            2,
            with(vec![council(vec![rule(RuleKind::SalesTaxPercent, "20")])]),
        );
        w.run_days(1);
        let price = w.food_list_price();
        assert_eq!(w.with_tax(BREAD, price), price + price / 5);
        let v = w.vars(0, w.now);
        assert_eq!(
            v[var::FOOD_PRICE],
            price + price / 5,
            "food_price is what eating costs"
        );
    }

    #[test]
    fn dues_on_revenue_go_to_the_council_and_into_welfare() {
        let mut w = World::new(
            3,
            with(vec![council(vec![rule(RuleKind::DuesPercent, "50")])]),
        );
        w.run_until(7 * MINUTES_PER_DAY - 1, &mut |_| {});
        let mut e = w.economy.clone();
        let expected: i64 = (0..e.len())
            .map(|b| {
                let due = e.take_revenue(b) * 50 / 100;
                due.min(e.cash(b)).max(0)
            })
            .sum();
        assert!(expected > 0, "businesses sold something");
        let (welfare, supply) = (w.housing.total_cash(), w.money_supply());
        let now = w.now;
        w.associations_meet(now, &mut |_| {});
        assert_eq!(w.housing.total_cash() - welfare, expected);
        assert_eq!(w.money_supply(), supply);
        let mut e = w.economy.clone();
        assert!(
            (0..e.len()).all(|b| e.take_revenue(b) == 0),
            "revenue counts anew"
        );
    }

    #[test]
    fn a_guild_shares_its_treasury_among_member_businesses() {
        let mut w = World::new(3, with(vec![guild(vec![])]));
        w.run_days(1);
        w.assoc_cash[0] = 101;
        w.money[0] -= 101; // keep the books balanced
        let bakeries: Vec<_> = w.economy.producers(BREAD).collect();
        let before: Vec<i64> = bakeries.iter().map(|&b| w.economy.cash(b)).collect();
        let now = w.now;
        w.associations_meet(now, &mut |_| {});
        let n = bakeries.len() as i64;
        for (k, &b) in bakeries.iter().enumerate() {
            assert_eq!(w.economy.cash(b) - before[k], 101 / n);
        }
        assert_eq!(w.assoc_cash[0], 101 % n);
    }

    #[test]
    fn founders_pay_the_guild_fee() {
        let mut c = with(vec![guild(vec![rule(RuleKind::FoundingFee, "50")])]);
        let names = crate::behaviour::variable_names(&c.traits);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let f = |s: &str| Formula::compile(s, &names).unwrap();
        c.behaviour.founding = Some(FoundingRule {
            when: Some(f(
                "market.firms == 1 && market.price > 30 && market.fee == 50",
            )),
            score: f("1"),
            close_when: f("0"),
            capital: 100,
            profit_tax_percent: 0,
            max_firms: 1,
        });
        let mut w = World::new(1, c);
        let supply = w.money_supply();
        let mut founder = None;
        w.run_until(3 * MINUTES_PER_DAY, &mut |e| {
            if let LogEntry::Firm {
                agent,
                change: FirmChange::Founded { .. },
                ..
            } = e
            {
                founder = Some(agent);
            }
        });
        assert!(founder.is_some(), "a bakery was founded");
        // Fee paid on day 1 or 2; the guild meets on day 7.
        assert_eq!(w.stats().assoc_cash[0], 50);
        assert_eq!(w.money_supply(), supply);
    }
}
