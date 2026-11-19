//! Game content as TOML. Produces ready-made configurations for the core.

use std::{fmt, fs, path::Path};

use serde::Deserialize;
use timeloop_core::{WorldConfig, NEED_MAX};

/// The bundled default world.
pub const DEFAULT_WORLD: &str = include_str!("../data/world.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldFile {
    world: WorldSection,
    needs: NeedsSection,
    meal: MealSection,
    work: WorkSection,
    wander: WanderSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldSection {
    agents: u32,
    start_money: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NeedsSection {
    hunger: HungerSection,
    fatigue: FatigueSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HungerSection {
    per_hour: u32,
    eat_at: u32,
    critical: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FatigueSection {
    per_hour: u32,
    sleep_at: u32,
    recovery_per_hour: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MealSection {
    price: i64,
    relief: u32,
    minutes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkSection {
    hourly_wage: i64,
    start_hour: u32,
    end_hour: u32,
}

#[derive(Debug, Deserialize)]
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

fn check(ok: bool, msg: &str) -> Result<(), ContentError> {
    if ok {
        Ok(())
    } else {
        Err(ContentError::Invalid(msg.into()))
    }
}

pub fn parse_world(text: &str) -> Result<WorldConfig, ContentError> {
    let f: WorldFile = toml::from_str(text).map_err(ContentError::Parse)?;
    let (h, fa) = (&f.needs.hunger, &f.needs.fatigue);

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
    check(f.meal.price > 0, "meal.price must be > 0")?;
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
        hunger_per_hour: h.per_hour,
        hunger_eat_at: h.eat_at,
        hunger_critical: h.critical,
        fatigue_per_hour: fa.per_hour,
        fatigue_sleep_at: fa.sleep_at,
        sleep_recovery_per_hour: fa.recovery_per_hour,
        meal_price: f.meal.price,
        meal_relief: f.meal.relief,
        meal_minutes: f.meal.minutes,
        hourly_wage: f.work.hourly_wage,
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

    #[test]
    fn default_world_parses() {
        assert_eq!(default_world().agents, 50);
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
        ] {
            let text = DEFAULT_WORLD.replace(from, to);
            assert!(
                matches!(parse_world(&text), Err(ContentError::Invalid(_))),
                "{from} -> {to} was accepted"
            );
        }
    }
}
