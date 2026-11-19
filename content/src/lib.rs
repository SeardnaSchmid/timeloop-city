//! Game content as TOML. Produces ready-made configurations for the core.
//!
//! The file format is described by a JSON schema generated from the structs
//! below (`content/schema/world.schema.json`), so editors can autocomplete and
//! validate world files. Doc comments here become the schema's descriptions.

use std::{collections::BTreeMap, fmt, fs, path::Path};

use schemars::JsonSchema;
use serde::Deserialize;
use timeloop_core::{
    variable_names, ActionKind, ActionRule, AssocKind, AssociationConfig, Behaviour,
    BusinessConfig, CareerConfig, Formula, FoundingRule, GoodId, HouseConfig, HousingConfig,
    HousingRule, LifeConfig, MapConfig, MarketConfig, Population, Pos, RecipeInput, Rule, RuleKind,
    WorldConfig, ASSOC_VARS, MAX_TRAITS, NEED_MAX,
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
    map: MapSection,
    /// Houses in ID order. Each rents out beds by the day.
    house: Vec<HouseSection>,
    housing: HousingSection,
    /// Agents founding firms. Without this section only the businesses
    /// above exist.
    founding: Option<FoundingSection>,
    traits: TraitsSection,
    /// Agent groups; agents are split over them in ID order, by weight.
    population: Vec<PopulationSection>,
    /// Ageing, education, retirement, death and new people. Without it
    /// nobody ages.
    life: Option<LifeSection>,
    /// Guilds and the town council, in ID order.
    #[serde(default)]
    association: Vec<AssociationSection>,
    /// Professions with levels. Without it everyone works alike.
    career: Option<CareerSection>,
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
struct MapSection {
    /// Parcels across; x runs 0..width.
    width: u16,
    /// Parcels down; y runs 0..height.
    height: u16,
    /// Walking time per parcel (Manhattan distance).
    travel_minutes_per_parcel: u64,
    /// Land price of a parcel far from any house.
    land_price_base: i64,
    /// Houses closer than this raise the land price around them by their rent.
    land_price_radius: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HouseSection {
    name: String,
    x: u16,
    y: u16,
    beds: u32,
    /// Starting rent per bed and day.
    rent: i64,
}

/// Once a day every agent scores each house it could move into (or its own)
/// against staying homeless and takes the best. Formulas see the action
/// variables plus `house.rent`, `house.distance` (to work, or the town centre
/// when unemployed) and `house.current` (1 for its own home).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HousingSection {
    /// Daily rent change: up when full and someone was turned away, down when beds stay free.
    rent_step: i64,
    rent_min: i64,
    /// Sleep recovery without a home, in percent of the normal rate.
    homeless_recovery_percent: u32,
    /// The house is an option when this is > 0.
    when: Option<String>,
    score: String,
    /// Score of having no home.
    homeless_score: String,
}

/// Once a day every agent without a firm scores founding one for each good
/// (the first business above making it is the blueprint); the best allowed
/// founding happens, at most one a day. The founder pays `capital` into the
/// firm and runs it as CEO; the firm's surplus goes to the founder.
/// Formulas see the action variables plus `market.price` and `market.cost`
/// (lowest price and unit cost of the good's makers), `market.shortage`
/// (0-1000, how often it ran short lately), `market.firms` (its makers) and
/// `firm.capital`. `close` is read daily for every CEO with `firm.cash`,
/// `firm.broke_days` (days in a row without a day's payroll) and `firm.age_days`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FoundingSection {
    capital: i64,
    /// Percent of a firm's surplus the town takes for welfare; the owner gets the rest.
    profit_tax_percent: i64,
    /// Open founded firms at most.
    max_firms: u32,
    /// Founding is an option when this is > 0.
    when: Option<String>,
    score: String,
    /// The firm closes when this is > 0.
    close: String,
}

/// Ages are in years. The number of agents stays fixed: whoever dies is
/// replaced the same day by a descendant of a living agent or a newcomer,
/// entering at `entry_age` with no money. Money and firm go to the children
/// (the firm to one, who runs it), without children to the town.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LifeSection {
    /// Ages of the first inhabitants, spread evenly.
    start_age: [u32; 2],
    entry_age: u32,
    /// No job before this age.
    education_end_age: u32,
    /// No job from this age on.
    retirement_age: u32,
    /// Rolled once per person, evenly in this range.
    lifespan: [u32; 2],
    /// Chance in percent that a free place goes to a child of an agent aged
    /// `parent_age` rather than to a newcomer.
    birth_percent: u32,
    parent_age: [u32; 2],
    /// Percent of every wage paid into the pension fund, shared out daily
    /// to everyone in education or retired.
    pension_percent: u32,
    /// Percent of an estate the town takes for welfare before the children share the rest.
    inheritance_tax_percent: u32,
}

/// A guild (with `goods`; its members are the businesses making them) or the
/// town council (without `goods`; covers every good, members are all
/// people). Once a week it collects dues, pays out its treasury (a guild to
/// its member businesses, the council into welfare) and decides its rules.
/// Rule formulas see `market.price`, `market.cost`, `market.shortage`,
/// `market.firms` (of its first good; the food for the council),
/// `assoc.cash`, `assoc.members` and `day`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AssociationSection {
    name: String,
    goods: Option<Vec<String>>,
    /// Rule name -> formula: `min_price`, `max_price` (for sellers of its
    /// goods), `founding_fee` (paid to it when a firm making its goods is
    /// founded), `dues_percent` (of members' sales revenue), `sales_tax_percent`
    /// (on top of every meal of its goods).
    #[serde(default)]
    rules: BTreeMap<String, String>,
}

/// Every kind of business is a profession (by the good it makes). Work
/// gives experience in it; each level needs `level_xp_growth_percent` more
/// than the one before. Levels raise output and wage per hour. Hiring takes
/// the most experienced job seeker first.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CareerSection {
    xp_per_hour: u32,
    /// Experience from level 0 to 1.
    level_xp_base: u32,
    level_xp_growth_percent: u32,
    /// Extra output per level, in thousandths (20 = +2 %).
    output_per_level_permille: u32,
    /// Extra wage per level, in thousandths.
    wage_per_level_permille: u32,
    max_level: u32,
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
    /// Parcel the business stands on.
    x: u16,
    y: u16,
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
    /// Largest daily price change in percent. Prices follow demand
    /// against the sales that would bring stock to its target.
    price_change_percent: i64,
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
    comfort: Option<ComfortSection>,
}

/// Above safety: savings beyond the safety reserve are partly spent on
/// comforts each day. The town provides them; the money returns as welfare.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ComfortSection {
    /// Thousandths of the savings above the reserve spent per day.
    spend_permille: i64,
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
    /// Longest sleep in minutes; agents get up then even if not rested.
    max_sleep_minutes: u64,
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
            pos: Pos::new(b.x, b.y),
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
    let mc = &f.map;
    check(
        mc.width > 0 && mc.height > 0,
        "map width and height must be > 0",
    )?;
    let map = MapConfig {
        width: mc.width,
        height: mc.height,
        travel_minutes_per_parcel: mc.travel_minutes_per_parcel,
        land_price_base: mc.land_price_base,
        land_price_radius: mc.land_price_radius,
    };
    let houses: Vec<HouseConfig> = f
        .house
        .iter()
        .map(|h| HouseConfig {
            name: h.name.clone(),
            pos: Pos::new(h.x, h.y),
            beds: h.beds,
            start_rent: h.rent,
        })
        .collect();
    let mut placed: Vec<(Pos, String)> = Vec::new();
    let sites = (businesses
        .iter()
        .map(|b| (b.pos, format!("business \"{}\"", b.name))))
    .chain(
        houses
            .iter()
            .map(|h| (h.pos, format!("house \"{}\"", h.name))),
    );
    for (pos, what) in sites {
        check(map.contains(pos), format!("{what}: {pos} is off the map"))?;
        if let Some((_, other)) = placed.iter().find(|(p, _)| *p == pos) {
            return Err(ContentError::Invalid(format!(
                "{what} and {other} both stand on {pos}"
            )));
        }
        placed.push((pos, what));
    }
    for h in &f.house {
        check(h.beds > 0, format!("house \"{}\".beds must be > 0", h.name))?;
    }
    let hs = &f.housing;
    check(hs.rent_min >= 0, "housing.rent_min must be >= 0")?;
    check(
        (1..=100).contains(&hs.homeless_recovery_percent),
        "housing.homeless_recovery_percent must be 1..=100",
    )?;
    let housing = HousingConfig {
        rent_step: hs.rent_step,
        rent_min: hs.rent_min,
        homeless_recovery_percent: hs.homeless_recovery_percent,
    };
    let behaviour = parse_behaviour(&f, &traits)?;
    check(
        0 < f.wander.min_minutes && f.wander.min_minutes <= f.wander.max_minutes,
        "wander must satisfy 0 < min_minutes <= max_minutes",
    )?;

    let life = match &f.life {
        None => None,
        Some(l) => {
            let ordered = |[a, b]: [u32; 2]| a <= b;
            check(ordered(l.start_age), "life.start_age must be [min, max]")?;
            check(
                ordered(l.lifespan) && l.lifespan[0] > 0,
                "life.lifespan must be [min, max] > 0",
            )?;
            check(ordered(l.parent_age), "life.parent_age must be [min, max]")?;
            check(
                l.entry_age <= l.education_end_age && l.education_end_age <= l.retirement_age,
                "life: entry_age <= education_end_age <= retirement_age",
            )?;
            check(
                l.entry_age < l.lifespan[0],
                "life.entry_age must be below the lifespan",
            )?;
            check(l.birth_percent <= 100, "life.birth_percent must be 0..=100")?;
            check(
                l.pension_percent <= 100,
                "life.pension_percent must be 0..=100",
            )?;
            check(
                l.inheritance_tax_percent <= 100,
                "life.inheritance_tax_percent must be 0..=100",
            )?;
            Some(LifeConfig {
                start_age_min: l.start_age[0],
                start_age_max: l.start_age[1],
                entry_age: l.entry_age,
                education_end_age: l.education_end_age,
                retirement_age: l.retirement_age,
                lifespan_min: l.lifespan[0],
                lifespan_max: l.lifespan[1],
                birth_percent: l.birth_percent,
                parent_age_min: l.parent_age[0],
                parent_age_max: l.parent_age[1],
                pension_percent: l.pension_percent,
                inheritance_tax_percent: l.inheritance_tax_percent,
            })
        }
    };

    let assoc_names: Vec<&str> = ASSOC_VARS.to_vec();
    let mut associations = Vec::new();
    for a in &f.association {
        let kind = match &a.goods {
            None => AssocKind::Council,
            Some(goods) => {
                let mut ids = Vec::new();
                for g in goods {
                    let id = f.goods.names.iter().position(|n| n == g).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "association \"{}\": unknown good {g}",
                            a.name
                        ))
                    })?;
                    ids.push(id as GoodId);
                }
                check(
                    !ids.is_empty(),
                    format!("association \"{}\": no goods", a.name),
                )?;
                AssocKind::Guild { goods: ids }
            }
        };
        let mut rules = Vec::new();
        for (name, src) in &a.rules {
            let at = format!("association \"{}\".rules.{name}", a.name);
            let kind = RuleKind::from_name(name)
                .ok_or_else(|| ContentError::Invalid(format!("{at}: unknown rule")))?;
            let value = Formula::compile(src, &assoc_names)
                .map_err(|e| ContentError::Invalid(format!("{at}: {e}")))?;
            rules.push(Rule { kind, value });
        }
        associations.push(AssociationConfig {
            name: a.name.clone(),
            kind,
            rules,
        });
    }

    Ok(WorldConfig {
        agents: f.world.agents,
        start_money: f.world.start_money,
        goods: f.goods.names.clone(),
        food,
        businesses,
        map,
        houses,
        housing,
        market: MarketConfig {
            price_change_percent: m.price_change_percent,
            price_min: m.price_min,
            wage_step: m.wage_step,
            wage_min: m.wage_min,
            reserve_days: m.reserve_days,
        },
        traits,
        populations,
        behaviour,
        life,
        associations,
        career: f.career.as_ref().map(|c| CareerConfig {
            xp_per_hour: c.xp_per_hour,
            level_xp_base: c.level_xp_base,
            level_xp_growth_percent: c.level_xp_growth_percent,
            output_per_level_permille: c.output_per_level_permille,
            wage_per_level_permille: c.wage_per_level_permille,
            max_level: c.max_level,
        }),
        hunger_per_hour: h.per_hour,
        hunger_critical: h.critical,
        fatigue_per_hour: fa.per_hour,
        sleep_recovery_per_hour: fa.recovery_per_hour,
        max_sleep_minutes: fa.max_sleep_minutes,
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

fn parse_behaviour(f: &WorldFile, traits: &[String]) -> Result<Behaviour, ContentError> {
    let actions = &f.action;
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
    let h = &f.housing;
    Ok(Behaviour {
        actions: rules,
        safety_reserve_days: f.needs.safety.reserve_days,
        meals_per_day: f.needs.safety.meals_per_day,
        comfort_spend_permille: match &f.needs.comfort {
            None => 0,
            Some(c) => {
                check(
                    (0..=1000).contains(&c.spend_permille),
                    "needs.comfort.spend_permille must be 0..=1000",
                )?;
                c.spend_permille
            }
        },
        housing: HousingRule {
            when: h
                .when
                .as_deref()
                .map(|w| compile(w, "housing.when".into()))
                .transpose()?,
            score: compile(&h.score, "housing.score".into())?,
            homeless_score: compile(&h.homeless_score, "housing.homeless_score".into())?,
        },
        founding: match &f.founding {
            None => None,
            Some(fs) => {
                check(fs.capital > 0, "founding.capital must be > 0")?;
                check(
                    (0..=100).contains(&fs.profit_tax_percent),
                    "founding.profit_tax_percent must be 0..=100",
                )?;
                Some(FoundingRule {
                    when: fs
                        .when
                        .as_deref()
                        .map(|w| compile(w, "founding.when".into()))
                        .transpose()?,
                    score: compile(&fs.score, "founding.score".into())?,
                    close_when: compile(&fs.close, "founding.close".into())?,
                    capital: fs.capital,
                    profit_tax_percent: fs.profit_tax_percent,
                    max_firms: fs.max_firms,
                })
            }
        },
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
            ("width = 24", "width = 4"),
            ("beds = 12", "beds = 0"),
            (
                "homeless_recovery_percent = 50",
                "homeless_recovery_percent = 0",
            ),
            ("homeless_score = \"0\"", "homeless_score = \"house.size\""),
            ("recovery_per_hour = 80", "recovery_per_hour = 0"),
            ("min_minutes = 30", "min_minutes = 0"),
            ("food = \"bread\"", "food = \"cake\""),
            ("output = \"flour\"", "output = \"flower\""),
            ("capital = 300", "capital = 0"),
            ("retirement_age = 65", "retirement_age = 10"),
            ("dues_percent = \"5\"", "dues_pct = \"5\""),
            ("goods = [\"bread\"]", "goods = [\"cake\"]"),
            (
                "sales_tax_percent = \"5\"",
                "sales_tax_percent = \"need.hunger\"",
            ),
            ("lifespan = [70, 90]", "lifespan = [90, 70]"),
            ("close = \"", "close = \"firm.bank + "),
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
