//! Game content as TOML. Produces ready-made configurations for the core.
//!
//! The file format is described by a JSON schema generated from the structs
//! below (`content/schema/world.schema.json`), so editors can autocomplete and
//! validate world files. Doc comments here become the schema's descriptions.

use std::{fmt, fs, path::Path};

use schemars::JsonSchema;
use serde::Deserialize;
use timeloop_core::{BusinessConfig, MarketConfig, RecipeInput, WorldConfig, NEED_MAX};

/// The bundled default world.
pub const DEFAULT_WORLD: &str = include_str!("../data/world.toml");

/// A world definition: population, goods, businesses and behaviour tuning.
/// All numbers are integers; needs use a 0-1000 scale.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorldFile {
    world: WorldSection,
    goods: GoodsSection,
    /// Businesses in ID order. Order matters: ties go to the lower ID.
    business: Vec<BusinessSection>,
    market: MarketSection,
    needs: NeedsSection,
    meal: MealSection,
    work: WorkSection,
    wander: WanderSection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorldSection {
    /// Number of agents.
    agents: u32,
    /// Money each agent starts with.
    start_money: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GoodsSection {
    /// All goods, by name.
    names: Vec<String>,
    /// The good agents buy and eat.
    food: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BusinessSection {
    name: String,
    /// Good this business produces and sells.
    output: String,
    /// Input consumed per unit of output. Omit for raw producers (fields, mines).
    input: Option<InputSection>,
    /// Output per worker-hour in thousandths of a unit (1500 = 1.5 units).
    output_milli_per_worker_hour: u32,
    /// Maximum number of workers.
    slots: u32,
    /// Starting cash.
    cash: i64,
    /// Starting price per unit of output.
    price: i64,
    /// Starting wage per worker-hour.
    wage: i64,
    /// Output units in stock at the start.
    #[serde(default)]
    stock: u32,
    /// Output stock the business aims for. Below it the price rises, above twice it falls.
    output_target: u32,
    /// Input stock the business restocks to every morning.
    #[serde(default)]
    input_target: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InputSection {
    good: String,
    amount: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MarketSection {
    /// Daily price change when stock is off target.
    price_step: i64,
    price_min: i64,
    /// Daily wage change when cash is off target.
    wage_step: i64,
    wage_min: i64,
    /// Days of full payroll kept in cash. Below it wages fall, above twice it they rise.
    reserve_days: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NeedsSection {
    hunger: HungerSection,
    fatigue: FatigueSection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HungerSection {
    per_hour: u32,
    /// Agents eat from this hunger on, if they can buy food.
    eat_at: u32,
    /// Interrupts the current action to go eat.
    critical: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FatigueSection {
    per_hour: u32,
    /// Sleep at any hour from here on; at night agents already sleep from 200.
    sleep_at: u32,
    recovery_per_hour: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MealSection {
    /// Hunger removed by one meal.
    relief: u32,
    minutes: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkSection {
    start_hour: u32,
    end_hour: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WanderSection {
    min_minutes: u32,
    max_minutes: u32,
}

#[derive(Debug)]
pub enum ContentError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Invalid(String),
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read file: {e}"),
            Self::Parse(e) => write!(f, "invalid TOML: {e}"),
            Self::Invalid(m) => write!(f, "invalid value: {m}"),
        }
    }
}

impl std::error::Error for ContentError {}

fn check(ok: bool, msg: impl Into<String>) -> Result<(), ContentError> {
    if ok {
        Ok(())
    } else {
        Err(ContentError::Invalid(msg.into()))
    }
}

/// JSON schema of the world file format, pretty-printed.
pub fn world_schema() -> String {
    let schema = schemars::schema_for!(WorldFile);
    serde_json::to_string_pretty(&schema).expect("schema serializes") + "\n"
}

pub fn parse_world(text: &str) -> Result<WorldConfig, ContentError> {
    let f: WorldFile = toml::from_str(text).map_err(ContentError::Parse)?;
    let (h, fa) = (&f.needs.hunger, &f.needs.fatigue);

    let good_id = |name: &str, field: &str| {
        f.goods
            .names
            .iter()
            .position(|g| g == name)
            .map(|i| i as u32)
            .ok_or_else(|| ContentError::Invalid(format!("{field}: unknown good \"{name}\"")))
    };
    let food = good_id(&f.goods.food, "goods.food")?;

    let mut businesses = Vec::new();
    for b in &f.business {
        let at = |field: &str| format!("business \"{}\".{field}", b.name);
        let input = match &b.input {
            Some(i) => {
                check(i.amount > 0, format!("{} must be > 0", at("input.amount")))?;
                Some(RecipeInput {
                    good: good_id(&i.good, &at("input.good"))?,
                    amount: i.amount,
                })
            }
            None => None,
        };
        check(b.slots > 0, format!("{} must be > 0", at("slots")))?;
        check(b.price > 0, format!("{} must be > 0", at("price")))?;
        businesses.push(BusinessConfig {
            name: b.name.clone(),
            output: good_id(&b.output, &at("output"))?,
            input,
            output_milli_per_worker_hour: b.output_milli_per_worker_hour,
            slots: b.slots,
            start_cash: b.cash,
            start_price: b.price,
            start_wage: b.wage,
            start_stock: b.stock,
            output_target: b.output_target,
            input_target: b.input_target,
        });
    }
    for b in &businesses {
        if let Some(input) = b.input {
            check(
                businesses.iter().any(|s| s.output == input.good),
                format!("business \"{}\": nobody produces its input", b.name),
            )?;
        }
    }
    check(
        businesses.iter().any(|b| b.output == food),
        "no business produces goods.food",
    )?;

    let m = &f.market;
    check(m.price_min > 0, "market.price_min must be > 0")?;
    check(m.wage_min >= 0, "market.wage_min must be >= 0")?;
    check(m.reserve_days >= 0, "market.reserve_days must be >= 0")?;

    check(h.eat_at <= NEED_MAX, "needs.hunger.eat_at must be <= 1000")?;
    check(
        h.critical <= NEED_MAX,
        "needs.hunger.critical must be <= 1000",
    )?;
    check(
        h.eat_at <= h.critical,
        "needs.hunger.eat_at must be <= critical",
    )?;
    check(
        fa.sleep_at <= NEED_MAX,
        "needs.fatigue.sleep_at must be <= 1000",
    )?;
    check(
        fa.recovery_per_hour > 0,
        "needs.fatigue.recovery_per_hour must be > 0",
    )?;
    check(f.meal.minutes > 0, "meal.minutes must be > 0")?;
    check(
        f.work.start_hour < f.work.end_hour && f.work.end_hour <= 24,
        "work hours must satisfy start_hour < end_hour <= 24",
    )?;
    check(
        0 < f.wander.min_minutes && f.wander.min_minutes <= f.wander.max_minutes,
        "wander must satisfy 0 < min_minutes <= max_minutes",
    )?;

    Ok(WorldConfig {
        agents: f.world.agents,
        start_money: f.world.start_money,
        goods: f.goods.names.clone(),
        food,
        businesses,
        market: MarketConfig {
            price_step: m.price_step,
            price_min: m.price_min,
            wage_step: m.wage_step,
            wage_min: m.wage_min,
            reserve_days: m.reserve_days,
        },
        hunger_per_hour: h.per_hour,
        hunger_eat_at: h.eat_at,
        hunger_critical: h.critical,
        fatigue_per_hour: fa.per_hour,
        fatigue_sleep_at: fa.sleep_at,
        sleep_recovery_per_hour: fa.recovery_per_hour,
        meal_relief: f.meal.relief,
        meal_minutes: f.meal.minutes,
        work_start_hour: f.work.start_hour,
        work_end_hour: f.work.end_hour,
        wander_min_minutes: f.wander.min_minutes,
        wander_max_minutes: f.wander.max_minutes,
    })
}

pub fn load_world(path: &Path) -> Result<WorldConfig, ContentError> {
    parse_world(&fs::read_to_string(path).map_err(ContentError::Io)?)
}

pub fn default_world() -> WorldConfig {
    parse_world(DEFAULT_WORLD).expect("bundled world.toml is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_world_parses() {
        let w = default_world();
        assert_eq!(w.agents, 50);
        assert_eq!(w.goods[w.food as usize], "bread");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = DEFAULT_WORLD.replace("agents = 50", "agents = 50\nagnets = 3");
        assert!(matches!(parse_world(&text), Err(ContentError::Parse(_))));
    }

    #[test]
    fn inconsistent_values_are_rejected() {
        for (from, to) in [
            ("critical = 800", "critical = 200"),
            ("end_hour = 18", "end_hour = 6"),
            ("recovery_per_hour = 80", "recovery_per_hour = 0"),
            ("min_minutes = 30", "min_minutes = 0"),
            ("food = \"bread\"", "food = \"cake\""),
            ("output = \"flour\"", "output = \"flower\""),
        ] {
            let text = DEFAULT_WORLD.replace(from, to);
            assert_ne!(text, DEFAULT_WORLD, "{from} not found");
            assert!(
                matches!(parse_world(&text), Err(ContentError::Invalid(_))),
                "{from} -> {to} was accepted"
            );
        }
    }

    #[test]
    fn missing_input_producer_is_rejected() {
        // Without the mill, the bakery's flour comes from nowhere.
        let text = DEFAULT_WORLD.replace("output = \"flour\"", "output = \"grain\"");
        assert!(matches!(parse_world(&text), Err(ContentError::Invalid(_))));
    }

    /// The checked-in schema must match the structs. Regenerate with
    /// `UPDATE_GOLDEN=1 cargo test -p timeloop-content`.
    #[test]
    fn schema_is_up_to_date() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/world.schema.json");
        let actual = world_schema();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &actual).unwrap();
            return;
        }
        let stored = fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            stored == actual,
            "{} is stale; run with UPDATE_GOLDEN=1",
            path.display()
        );
    }
}
