//! Deterministic simulation core.
//!
//! Rules: integers only, randomness only through [`rng`], iteration only in
//! entity-ID order. Same seed + same inputs = same [`world::World::state_hash`].

pub mod behaviour;
pub mod economy;
pub mod formula;
pub mod hash;
pub mod housing;
pub mod life;
pub mod map;
pub mod needs;
pub mod queue;
pub mod rng;
pub mod world;

pub use behaviour::{
    variable_names, ActionRule, Allowed, Behaviour, Candidate, Explanation, FoundingRule,
    HousingRule, Population, BASE_VARS, MAX_TRAITS,
};
pub use economy::{BusinessConfig, BusinessId, Economy, GoodId, MarketConfig, RecipeInput};
pub use formula::{Formula, FormulaError};
pub use hash::{StateHash, StateHasher};
pub use housing::{HouseConfig, Housing, HousingConfig};
pub use life::{LifeConfig, Stage, DAYS_PER_YEAR};
pub use map::{Building, HouseId, Map, MapConfig, Pos};
pub use needs::{LazyNeed, NEED_MAX};
pub use world::{
    Action, ActionKind, FirmChange, JobChange, LifeChange, LogEntry, Minute, Stats, World,
    WorldConfig, MINUTES_PER_DAY, MINUTES_PER_HOUR,
};
