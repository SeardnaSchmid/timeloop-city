//! The town as a grid of parcels. Every building stands on one parcel;
//! travel time is the Manhattan distance times a fixed rate.

use crate::economy::BusinessId;
use crate::hash::StateHasher;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Pos {
    pub x: u16,
    pub y: u16,
}

impl Pos {
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    /// Manhattan distance in parcels.
    pub fn distance(self, other: Pos) -> u32 {
        self.x.abs_diff(other.x) as u32 + self.y.abs_diff(other.y) as u32
    }

    /// Where you are after `steps` parcels on the way to `to`: along x
    /// first, then along y. Stops at `to`.
    pub fn toward(self, to: Pos, steps: u32) -> Pos {
        let dx = self.x.abs_diff(to.x) as u32;
        let along_x = steps.min(dx) as u16;
        let along_y = (steps - steps.min(dx)).min(self.y.abs_diff(to.y) as u32) as u16;
        let step = |from: u16, to: u16, n: u16| if to >= from { from + n } else { from - n };
        Pos::new(step(self.x, to.x, along_x), step(self.y, to.y, along_y))
    }

    pub(crate) fn hash_into(self, h: &mut StateHasher) {
        h.write_u32((self.x as u32) << 16 | self.y as u32);
    }
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}

pub type HouseId = u32;

/// What stands on a parcel. Its owner is whoever owns the building; empty
/// parcels belong to the town.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Building {
    Business(BusinessId),
    House(HouseId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapConfig {
    pub width: u16,
    pub height: u16,
    pub travel_minutes_per_parcel: u64,
    /// Land price of a parcel far from any house.
    pub land_price_base: i64,
    /// Houses closer than this raise the land price around them.
    pub land_price_radius: u32,
}

impl MapConfig {
    pub fn center(&self) -> Pos {
        Pos::new(self.width / 2, self.height / 2)
    }

    pub fn contains(&self, p: Pos) -> bool {
        p.x < self.width && p.y < self.height
    }
}

/// Which building stands where. Fixed for now; building and demolishing
/// come with firms.
#[derive(Clone, Debug)]
pub struct Map {
    config: MapConfig,
    parcels: Vec<Option<Building>>,
}

impl Map {
    /// Panics if a building is off the map or two share a parcel; content
    /// validation rejects both before.
    pub fn new(config: MapConfig, buildings: impl IntoIterator<Item = (Pos, Building)>) -> Self {
        let mut parcels = vec![None; config.width as usize * config.height as usize];
        for (p, b) in buildings {
            assert!(config.contains(p), "{p} is off the map");
            let i = p.y as usize * config.width as usize + p.x as usize;
            assert!(parcels[i].is_none(), "two buildings at {p}");
            parcels[i] = Some(b);
        }
        Self { config, parcels }
    }

    pub fn config(&self) -> &MapConfig {
        &self.config
    }

    pub fn at(&self, p: Pos) -> Option<Building> {
        self.parcels[p.y as usize * self.config.width as usize + p.x as usize]
    }

    pub fn travel_minutes(&self, from: Pos, to: Pos) -> u64 {
        from.distance(to) as u64 * self.config.travel_minutes_per_parcel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_manhattan() {
        assert_eq!(Pos::new(1, 2).distance(Pos::new(4, 0)), 5);
        assert_eq!(Pos::new(3, 3).distance(Pos::new(3, 3)), 0);
    }

    #[test]
    fn toward_walks_x_then_y_and_stops() {
        let (a, b) = (Pos::new(5, 1), Pos::new(2, 4));
        assert_eq!(a.toward(b, 0), a);
        assert_eq!(a.toward(b, 2), Pos::new(3, 1));
        assert_eq!(a.toward(b, 4), Pos::new(2, 2));
        assert_eq!(a.toward(b, 99), b);
        assert_eq!(b.toward(a, 3), Pos::new(5, 4));
    }

    #[test]
    fn map_knows_its_buildings() {
        let c = MapConfig {
            width: 4,
            height: 3,
            travel_minutes_per_parcel: 5,
            land_price_base: 100,
            land_price_radius: 3,
        };
        let m = Map::new(
            c,
            [
                (Pos::new(0, 0), Building::Business(1)),
                (Pos::new(3, 2), Building::House(0)),
            ],
        );
        assert_eq!(m.at(Pos::new(3, 2)), Some(Building::House(0)));
        assert_eq!(m.at(Pos::new(1, 1)), None);
        assert_eq!(m.travel_minutes(Pos::new(0, 0), Pos::new(3, 2)), 25);
        assert_eq!(m.config().center(), Pos::new(2, 1));
    }
}
