//! Data-driven decisions: every action has a precondition and a score formula.
//!
//! On each wake-up the agent fills a table of variables (needs, money, clock,
//! traits), evaluates every action and takes the highest score among those
//! allowed. Ties go to a seed-based roll, never to memory order.

use crate::formula::Formula;
use crate::world::ActionKind;

/// Traits an agent can have at most.
pub const MAX_TRAITS: usize = 16;

/// Variables every formula can read, in table order. Traits follow as
/// `trait.<name>`.
/// The `house.*` variables describe the house being considered; in action
/// formulas that is the agent's current home (all 0 without one).
/// The `market.*` variables describe the good a founder considers making
/// (0 outside the founding rule); `firm.*` describe the firm the agent
/// owns (0 without one), except `firm.capital`, the cost of founding one.
pub const BASE_VARS: [&str; 26] = [
    "need.hunger",
    "need.fatigue",
    "need.safety",
    "money",
    "wage",
    "food_price",
    "can_eat",
    "employed",
    "hour",
    "night",
    "work_time",
    "overtime_time",
    "has_home",
    "house.rent",
    "house.distance",
    "house.current",
    "market.price",
    "market.cost",
    "market.shortage",
    "market.firms",
    "market.newest_days",
    "firm.capital",
    "owns_firm",
    "firm.cash",
    "firm.broke_days",
    "firm.age_days",
];

pub const MAX_VARS: usize = BASE_VARS.len() + MAX_TRAITS;

/// Indices into the variable table.
pub(crate) mod var {
    pub const HUNGER: usize = 0;
    pub const FATIGUE: usize = 1;
    pub const SAFETY: usize = 2;
    pub const MONEY: usize = 3;
    pub const WAGE: usize = 4;
    pub const FOOD_PRICE: usize = 5;
    pub const CAN_EAT: usize = 6;
    pub const EMPLOYED: usize = 7;
    pub const HOUR: usize = 8;
    pub const NIGHT: usize = 9;
    pub const WORK_TIME: usize = 10;
    pub const OVERTIME_TIME: usize = 11;
    pub const HAS_HOME: usize = 12;
    pub const HOUSE_RENT: usize = 13;
    pub const HOUSE_DISTANCE: usize = 14;
    pub const HOUSE_CURRENT: usize = 15;
    pub const MARKET_PRICE: usize = 16;
    #[allow(dead_code)]
    pub const MARKET_COST: usize = 17;
    #[allow(dead_code)]
    pub const MARKET_SHORTAGE: usize = 18;
    #[allow(dead_code)]
    pub const MARKET_FIRMS: usize = 19;
    pub const MARKET_NEWEST_DAYS: usize = 20;
    pub const FIRM_CAPITAL: usize = 21;
    pub const OWNS_FIRM: usize = 22;
    pub const FIRM_CASH: usize = 23;
    pub const FIRM_BROKE_DAYS: usize = 24;
    pub const FIRM_AGE_DAYS: usize = 25;
    pub const TRAITS: usize = 26;
}

/// All variable names for the given traits, in table order.
pub fn variable_names(traits: &[String]) -> Vec<String> {
    BASE_VARS
        .iter()
        .map(|s| s.to_string())
        .chain(traits.iter().map(|t| format!("trait.{t}")))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRule {
    pub kind: ActionKind,
    /// Extra precondition on top of the built-in one; allowed when > 0.
    pub when: Option<Formula>,
    pub score: Formula,
}

/// A group of agents sharing trait ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Population {
    pub name: String,
    /// Share of the agents, relative to the other populations.
    pub weight: u32,
    /// Inclusive `(min, max)` per trait, in trait order.
    pub traits: Vec<(u32, u32)>,
}

/// Everything that steers decisions and can be swapped while running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviour {
    /// Rules in evaluation order; an action without a rule never happens.
    pub actions: Vec<ActionRule>,
    /// Savings worth this many days of meals make an agent feel safe.
    pub safety_reserve_days: i64,
    pub meals_per_day: i64,
    /// Comfort, the level above safety: every day agents spend this many
    /// thousandths of their savings above the safety reserve on comforts.
    /// The town provides them, so the money flows back as welfare. Keeps
    /// wealth from piling up at firm owners until there are luxury goods.
    pub comfort_spend_permille: i64,
    /// Once a day every agent scores each house it could live in (one with
    /// a free bed, or its own) and staying homeless, and takes the best.
    pub housing: HousingRule,
    /// Agents founding and closing firms; `None`: only the town's businesses.
    pub founding: Option<FoundingRule>,
}

/// Once a day every agent without a firm scores founding one for each good
/// the town makes (the first business making it is the blueprint); the best
/// allowed founding of the day happens. The founder pays `capital` into the
/// firm and runs it as CEO: works there, is never laid off, takes what the
/// till can pay, and gets the surplus as dividend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundingRule {
    /// Founding is an option when this is > 0.
    pub when: Option<Formula>,
    pub score: Formula,
    /// Read daily for every open business; it closes when > 0. Its cash
    /// goes back to the owner, its stock is lost. For a firm the formula sees
    /// its CEO's variables; for the town's businesses only `firm.*`.
    pub close_when: Formula,
    pub capital: i64,
    /// Share of a firm's surplus the town takes for welfare; the owner gets the rest.
    pub profit_tax_percent: i64,
    /// Open founded firms at most.
    pub max_firms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HousingRule {
    /// The house is an option when this is > 0.
    pub when: Option<Formula>,
    pub score: Formula,
    /// Score of having no home.
    pub homeless_score: Formula,
}

/// One candidate action as seen by the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub kind: ActionKind,
    pub allowed: Allowed,
    pub score: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allowed {
    Yes,
    /// Built-in precondition failed (not employed, no food for sale, wrong hour).
    NotPossible,
    /// The rule's `when` formula said no.
    RuleSaysNo,
}

/// Why an agent picked what it picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explanation {
    pub agent: u32,
    pub minute: crate::world::Minute,
    pub vars: Vec<(String, i64)>,
    pub candidates: Vec<Candidate>,
    pub winner: ActionKind,
}
