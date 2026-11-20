//! Deterministic simulation core.
//!
//! Rules: integers only, randomness only through [`rng`], iteration only in
//! entity-ID order. Same seed + same inputs = same [`world::World::state_hash`].

pub mod assoc;
pub mod behaviour;
pub mod career;
pub mod economy;
pub mod flows;
pub mod formula;
pub mod hash;
pub mod housing;
pub mod life;
pub mod map;
pub mod needs;
pub mod queue;
pub mod rng;
pub mod world;

pub use assoc::{AssocKind, AssociationConfig, Rule, RuleKind, ASSOC_VARS, WEEK_DAYS};
pub use behaviour::{
    variable_names, ActionRule, Allowed, Behaviour, BuildingRule, Candidate, Explanation,
    FoundingRule, HousingRule, Population, BASE_VARS, MAX_TRAITS,
};
pub use career::CareerConfig;
pub use economy::{BusinessConfig, BusinessId, Economy, GoodId, MarketConfig, RecipeInput};
pub use flows::{CloseReason, Flows, CLOSE_REASONS};
pub use formula::{Formula, FormulaError};
pub use hash::{StateHash, StateHasher};
pub use housing::{HouseConfig, Housing, HousingConfig};
pub use life::{GrowthConfig, LifeConfig, Stage, DAYS_PER_YEAR};
pub use map::{Building, HouseId, Map, MapConfig, Pos};
pub use needs::{LazyNeed, NEED_MAX};
pub use world::{
    Action, ActionKind, Diagnosis, FirmChange, JobChange, LifeChange, LogEntry, Minute, Stats,
    Work, World, WorldConfig, BRANCH_BLOCKS, MINUTES_PER_DAY, MINUTES_PER_HOUR,
};
