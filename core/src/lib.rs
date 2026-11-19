//! Deterministischer Simulationskern.
//!
//! Regeln: nur Ganzzahlen, Zufall nur über [`rng`], Iteration nur nach
//! Entity-ID. Gleicher Seed + gleiche Eingaben = gleicher [`world::World::state_hash`].

pub mod hash;
pub mod rng;
pub mod world;

pub use hash::{StateHash, StateHasher};
pub use world::{Minute, World, WorldConfig, MINUTES_PER_DAY};
