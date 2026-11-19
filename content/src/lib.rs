//! Spielinhalte als TOML. Liefert fertige Konfigurationen für den Core.

use std::{fmt, fs, path::Path};

use serde::Deserialize;
use timeloop_core::WorldConfig;

/// Die mitgelieferte Standardwelt.
pub const DEFAULT_WORLD: &str = include_str!("../data/world.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldFile {
    world: WorldSection,
    needs: NeedsSection,
    economy: EconomySection,
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
    hunger_per_hour: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EconomySection {
    meal_price: i64,
    daily_wage: i64,
    work_chance_permille: u32,
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
            Self::Io(e) => write!(f, "Datei nicht lesbar: {e}"),
            Self::Parse(e) => write!(f, "TOML ungültig: {e}"),
            Self::Invalid(m) => write!(f, "Wert ungültig: {m}"),
        }
    }
}

impl std::error::Error for ContentError {}

pub fn parse_world(text: &str) -> Result<WorldConfig, ContentError> {
    let f: WorldFile = toml::from_str(text).map_err(ContentError::Parse)?;
    if f.economy.work_chance_permille > 1000 {
        return Err(ContentError::Invalid(
            "economy.work_chance_permille muss 0–1000 sein".into(),
        ));
    }
    if f.economy.meal_price <= 0 {
        return Err(ContentError::Invalid(
            "economy.meal_price muss > 0 sein".into(),
        ));
    }
    Ok(WorldConfig {
        agents: f.world.agents,
        start_money: f.world.start_money,
        hunger_per_hour: f.needs.hunger_per_hour,
        meal_price: f.economy.meal_price,
        daily_wage: f.economy.daily_wage,
        work_chance_permille: f.economy.work_chance_permille,
    })
}

pub fn load_world(path: &Path) -> Result<WorldConfig, ContentError> {
    parse_world(&fs::read_to_string(path).map_err(ContentError::Io)?)
}

pub fn default_world() -> WorldConfig {
    parse_world(DEFAULT_WORLD).expect("mitgelieferte world.toml ist gültig")
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
    fn out_of_range_is_rejected() {
        let text = DEFAULT_WORLD.replace("= 700", "= 1200");
        assert!(matches!(parse_world(&text), Err(ContentError::Invalid(_))));
    }
}
