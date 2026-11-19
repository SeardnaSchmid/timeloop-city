//! Deterministic simulation core.
//!
//! Rules: integers only, randomness only through [`rng`], iteration only in
//! entity-ID order. Same seed + same inputs = same [`world::World::state_hash`].

pub mod hash;
pub mod rng;
pub mod world;

pub use hash::{StateHash, StateHasher};
pub use world::{Minute, World, WorldConfig, MINUTES_PER_DAY};
