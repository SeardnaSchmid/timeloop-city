//! Game content as TOML. Produces ready-made configurations for the core.
//!
//! The file format is described by a JSON schema generated from the structs
//! below (`content/schema/world.schema.json`), so editors can autocomplete and
//! validate world files. Doc comments here become the schema's descriptions.

use std::{collections::BTreeMap, fmt, fs, path::Path};

use schemars::JsonSchema;
use serde::Deserialize;
use timeloop_core::{
    variable_names, ActionKind, ActionRule, Behaviour, BusinessConfig, Formula, MarketConfig,
    Population, RecipeInput, WorldConfig, MAX_TRAITS, NEED_MAX,
};

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
    traits: TraitsSection,
    /// Agent groups; agents are split over them in ID order, by weight.
    population: Vec<PopulationSection>,
    needs: NeedsSection,
    meal: MealSection,
    work: WorkSection,
    wander: WanderSection,
    /// Decision rules per action: eat, sleep, work, overtime, wander. An
    /// action without a rule never happens; wander is the fallback.
    action: BTreeMap<String, ActionSection>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TraitsSection {
    /// Character traits (0-1000). Formulas read them as `trait.<name>`.
    names: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PopulationSection {
    name: String,
    /// Share of the agents, relative to the other populations.
    weight: u32,
    /// `[min, max]` per trait; agents roll evenly in between. Missing traits are 500.
    #[serde(default)]
    traits: BTreeMap<String, [u32; 2]>,
}

/// Formulas are integer expressions over the decision variables:
/// `need.hunger`, `need.fatigue`, `need.safety` (0-1000), `money`, `wage`,
/// `food_price`, `can_eat`, `employed`, `hour`, `night`, `work_time`,
/// `overtime_time` (0 or 1) and `trait.<name>`. Operators: `+ - * / %`,
/// comparisons, `&& || !`, `min max clamp if`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ActionSection {
    /// Extra precondition; the action is allowed when this is > 0.
    when: Option<String>,
    /// The allowed action with the highest score is chosen.
    score: String,
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
    safety: SafetySection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HungerSection {
    per_hour: u32,
    /// Interrupts the current action to go eat.
    critical: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FatigueSection {
    per_hour: u32,
    recovery_per_hour: u32,
}

/// Safety is 1000 without savings and 0 once savings cover the reserve.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SafetySection {
    /// Days of meals the reserve covers.
    reserve_days: i64,
    meals_per_day: i64,
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
    /// Overtime runs from end_hour to here.
    overtime_end_hour: u32,
    /// Work happens in blocks of at most this long; agents decide again after each.
    block_minutes: u64,
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

    check(
        h.critical <= NEED_MAX,
        "needs.hunger.critical must be <= 1000",
    )?;
    check(
        fa.recovery_per_hour > 0,
        "needs.fatigue.recovery_per_hour must be > 0",
    )?;
    check(f.meal.minutes > 0, "meal.minutes must be > 0")?;
    check(
        f.work.start_hour < f.work.end_hour
            && f.work.end_hour <= f.work.overtime_end_hour
            && f.work.overtime_end_hour <= 24,
        "work hours must satisfy start_hour < end_hour <= overtime_end_hour <= 24",
    )?;
    check(f.work.block_minutes > 0, "work.block_minutes must be > 0")?;
    let s = &f.needs.safety;
    check(
        s.reserve_days >= 0 && s.meals_per_day >= 0,
        "needs.safety values must be >= 0",
    )?;

    let traits = f.traits.names.clone();
    check(
        traits.len() <= MAX_TRAITS,
        format!("traits.names: at most {MAX_TRAITS} traits"),
    )?;
    for (i, t) in traits.iter().enumerate() {
        check(
            !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            format!("traits.names: \"{t}\" must be letters, digits or _"),
        )?;
        check(
            !traits[..i].contains(t),
            format!("traits.names: \"{t}\" twice"),
        )?;
    }
    check(
        !f.population.is_empty(),
        "at least one [[population]] needed",
    )?;
    check(
        f.population.iter().any(|p| p.weight > 0),
        "population: at least one weight must be > 0",
    )?;
    let mut populations = Vec::new();
    for p in &f.population {
        for name in p.traits.keys() {
            check(
                traits.contains(name),
                format!("population \"{}\": unknown trait \"{name}\"", p.name),
            )?;
        }
        let ranges = traits
            .iter()
            .map(|t| {
                let [lo, hi] = p.traits.get(t).copied().unwrap_or([500, 500]);
                check(
                    lo <= hi && hi <= NEED_MAX,
                    format!(
                        "population \"{}\".traits.{t}: need min <= max <= 1000",
                        p.name
                    ),
                )
                .map(|()| (lo, hi))
            })
            .collect::<Result<_, _>>()?;
        populations.push(Population {
            name: p.name.clone(),
            weight: p.weight,
            traits: ranges,
        });
    }
    let behaviour = parse_behaviour(&f.action, &traits, s.reserve_days, s.meals_per_day)?;
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
        traits,
        populations,
        behaviour,
        hunger_per_hour: h.per_hour,
        hunger_critical: h.critical,
        fatigue_per_hour: fa.per_hour,
        sleep_recovery_per_hour: fa.recovery_per_hour,
        meal_relief: f.meal.relief,
        meal_minutes: f.meal.minutes,
        work_start_hour: f.work.start_hour,
        work_end_hour: f.work.end_hour,
        overtime_end_hour: f.work.overtime_end_hour,
        work_block_minutes: f.work.block_minutes,
        wander_min_minutes: f.wander.min_minutes,
        wander_max_minutes: f.wander.max_minutes,
    })
}

fn parse_behaviour(
    actions: &BTreeMap<String, ActionSection>,
    traits: &[String],
    reserve_days: i64,
    meals_per_day: i64,
) -> Result<Behaviour, ContentError> {
    let names = variable_names(traits);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let compile = |src: &str, at: String| {
        Formula::compile(src, &names).map_err(|e| ContentError::Invalid(format!("{at}: {e}")))
    };
    for name in actions.keys() {
        check(
            ActionKind::from_name(name).is_some(),
            format!("action.{name}: unknown action"),
        )?;
    }
    let mut rules = Vec::new();
    for kind in ActionKind::CHOOSABLE {
        let Some(a) = actions.get(kind.name()) else {
            continue;
        };
        let at = |field: &str| format!("action.{}.{field}", kind.name());
        rules.push(ActionRule {
            kind,
            when: a
                .when
                .as_deref()
                .map(|w| compile(w, at("when")))
                .transpose()?,
            score: compile(&a.score, at("score"))?,
        });
    }
    Ok(Behaviour {
        actions: rules,
        safety_reserve_days: reserve_days,
        meals_per_day,
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
            ("critical = 800", "critical = 2000"),
            ("end_hour = 18", "end_hour = 6"),
            (
                "names = [\"greed\", \"work_life\"]",
                "names = [\"greed\", \"greed\"]",
            ),
            ("greed = [", "gred = ["),
            ("score = \"4 * need.hunger\"", "score = \"4 * need.hungry\""),
            ("score = \"4 * need.hunger\"", "score = \"4 *\""),
            ("[action.eat]", "[action.dance]"),
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
