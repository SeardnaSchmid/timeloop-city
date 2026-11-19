//! Houses rent out beds by the day. A house is run like a firm: rent goes
//! into its cash, the rent follows demand in fixed steps. The houses of the
//! world file belong to the town; agents build more, which take some days
//! before anyone can move in.

/// Owner of a town house.
pub const TOWN: u32 = u32::MAX;

use crate::hash::StateHasher;
use crate::map::{HouseId, Pos};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HouseConfig {
    pub name: String,
    pub pos: Pos,
    pub beds: u32,
    pub start_rent: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HousingConfig {
    /// Daily rent change.
    pub rent_step: i64,
    pub rent_min: i64,
    /// Sleep recovery without a home, in percent of the normal rate.
    pub homeless_recovery_percent: u32,
}

#[derive(Clone, Debug)]
pub struct Housing {
    config: Vec<HouseConfig>,
    market: HousingConfig,
    rent: Vec<i64>,
    tenants: Vec<u32>,
    cash: Vec<i64>,
    /// Agents who wanted in today but found the house full.
    turned_away: Vec<u32>,
    /// Turned away over all houses at the last rent adjustment.
    wanted: u32,
    /// Agent who owns the house, or `TOWN`.
    owner: Vec<u32>,
    /// Under construction: nobody can move in yet.
    building: Vec<bool>,
}

impl Housing {
    pub fn new(config: Vec<HouseConfig>, market: HousingConfig) -> Self {
        let n = config.len();
        Self {
            rent: config.iter().map(|h| h.start_rent).collect(),
            tenants: vec![0; n],
            cash: vec![0; n],
            turned_away: vec![0; n],
            wanted: 0,
            owner: vec![TOWN; n],
            building: vec![false; n],
            config,
            market,
        }
    }

    /// Starts a new house owned by `owner`; nobody moves in until [`Housing::finish`].
    pub(crate) fn start_building(&mut self, config: HouseConfig, owner: u32) -> HouseId {
        self.rent.push(config.start_rent);
        self.tenants.push(0);
        self.cash.push(0);
        self.turned_away.push(0);
        self.owner.push(owner);
        self.building.push(true);
        self.config.push(config);
        self.len() - 1
    }

    pub(crate) fn finish(&mut self, h: HouseId) {
        self.building[h as usize] = false;
    }

    pub fn under_construction(&self, h: HouseId) -> bool {
        self.building[h as usize]
    }

    pub fn owner(&self, h: HouseId) -> Option<u32> {
        Some(self.owner[h as usize]).filter(|&o| o != TOWN)
    }

    pub(crate) fn set_owner(&mut self, h: HouseId, owner: u32) {
        self.owner[h as usize] = owner;
    }

    /// People turned away by full houses at the last rent adjustment.
    pub fn wanted(&self) -> u32 {
        self.wanted
    }

    pub fn len(&self) -> u32 {
        self.config.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.config.is_empty()
    }

    pub fn config(&self, h: HouseId) -> &HouseConfig {
        &self.config[h as usize]
    }

    pub fn market(&self) -> &HousingConfig {
        &self.market
    }

    pub fn rent(&self, h: HouseId) -> i64 {
        self.rent[h as usize]
    }

    pub fn tenants(&self, h: HouseId) -> u32 {
        self.tenants[h as usize]
    }

    pub fn free_beds(&self, h: HouseId) -> u32 {
        if self.building[h as usize] {
            return 0;
        }
        self.config[h as usize].beds - self.tenants[h as usize]
    }

    pub fn cash(&self, h: HouseId) -> i64 {
        self.cash[h as usize]
    }

    pub fn total_cash(&self) -> i64 {
        self.cash.iter().sum()
    }

    pub fn total_beds(&self) -> u32 {
        self.config.iter().map(|h| h.beds).sum()
    }

    pub(crate) fn move_in(&mut self, h: HouseId) {
        assert!(self.free_beds(h) > 0, "house {h} is full");
        self.tenants[h as usize] += 1;
    }

    pub(crate) fn move_out(&mut self, h: HouseId) {
        self.tenants[h as usize] -= 1;
    }

    pub(crate) fn turn_away(&mut self, h: HouseId) {
        self.turned_away[h as usize] += 1;
    }

    pub(crate) fn receive_rent(&mut self, h: HouseId) -> i64 {
        let rent = self.rent[h as usize];
        self.cash[h as usize] += rent;
        rent
    }

    /// Full and people turned away: up a step. Beds free and nobody turned
    /// away: down a step. Resets the turned-away counts.
    pub(crate) fn adjust_rents(&mut self) {
        let m = &self.market;
        self.wanted = self.turned_away.iter().sum();
        for h in 0..self.config.len() {
            if self.building[h] {
                continue;
            }
            let full = self.tenants[h] == self.config[h].beds;
            if full && self.turned_away[h] > 0 {
                self.rent[h] += m.rent_step;
            } else if !full && self.turned_away[h] == 0 {
                self.rent[h] = (self.rent[h] - m.rent_step).max(m.rent_min);
            }
            self.turned_away[h] = 0;
        }
    }

    pub(crate) fn add_cash(&mut self, h: HouseId, amount: i64) {
        self.cash[h as usize] += amount;
    }

    /// Takes all cash out, for payout.
    pub(crate) fn take_cash(&mut self, h: HouseId) -> i64 {
        std::mem::take(&mut self.cash[h as usize])
    }

    pub(crate) fn hash_into(&self, h: &mut StateHasher) {
        h.write_i64_slice(&self.rent);
        h.write_u32_slice(&self.tenants);
        h.write_i64_slice(&self.cash);
        h.write_u32_slice(&self.turned_away);
        h.write_u32(self.wanted);
        h.write_u32_slice(&self.owner);
        self.building
            .iter()
            .for_each(|&b| h.write_bytes(&[b as u8]));
        for c in &self.config {
            c.pos.hash_into(h);
            h.write_u32(c.beds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn housing() -> Housing {
        let house = |beds| HouseConfig {
            name: String::new(),
            pos: Pos::default(),
            beds,
            start_rent: 10,
        };
        Housing::new(
            vec![house(2), house(2)],
            HousingConfig {
                rent_step: 1,
                rent_min: 9,
                homeless_recovery_percent: 50,
            },
        )
    }

    #[test]
    fn rent_follows_demand() {
        let mut h = housing();
        h.move_in(0);
        h.move_in(0);
        h.turn_away(0);
        h.adjust_rents();
        assert_eq!((h.rent(0), h.rent(1)), (11, 9));
        h.adjust_rents();
        // Full but nobody waiting: holds. Empty: floor.
        assert_eq!((h.rent(0), h.rent(1)), (11, 9));
    }

    #[test]
    fn rent_goes_into_cash() {
        let mut h = housing();
        assert_eq!(h.receive_rent(1), 10);
        assert_eq!(h.total_cash(), 10);
        assert_eq!(h.take_cash(1), 10);
        assert_eq!(h.total_cash(), 0);
    }

    #[test]
    #[should_panic(expected = "full")]
    fn no_more_tenants_than_beds() {
        let mut h = housing();
        for _ in 0..3 {
            h.move_in(1);
        }
    }
}
