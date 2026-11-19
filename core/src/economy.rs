//! Businesses, goods and the daily market.
//!
//! The money supply is closed: money only moves between agents and business
//! cash, it is never created or destroyed. Goods enter the world through
//! recipes without inputs (the field) and leave it when eaten.

use crate::hash::StateHasher;
use crate::map::Pos;

pub type GoodId = u32;
pub type BusinessId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecipeInput {
    pub good: GoodId,
    pub amount: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusinessConfig {
    pub name: String,
    /// Parcel the business stands on; its workers work here.
    pub pos: Pos,
    pub output: GoodId,
    pub input: Option<RecipeInput>,
    /// Output per worker-hour, in thousandths of a unit.
    pub output_milli_per_worker_hour: u32,
    pub slots: u32,
    pub start_cash: i64,
    pub start_price: i64,
    pub start_wage: i64,
    /// Output units in stock when the world begins.
    pub start_stock: u32,
    /// Output stock the business aims to hold; drives its price.
    pub output_target: u32,
    /// Input stock it restocks to every morning.
    pub input_target: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketConfig {
    pub price_step: i64,
    pub price_min: i64,
    pub wage_step: i64,
    pub wage_min: i64,
    /// Days of full payroll a business wants in cash before it raises wages.
    pub reserve_days: i64,
}

#[derive(Clone, Debug)]
pub struct Economy {
    goods: usize,
    config: Vec<BusinessConfig>,
    market: MarketConfig,
    cash: Vec<i64>,
    price: Vec<i64>,
    wage: Vec<i64>,
    /// Flat `business * goods + good`.
    stock: Vec<u32>,
    progress_milli: Vec<u64>,
    workers: Vec<u32>,
    /// Units sold since the last daily adjustment; tells real demand from empty shelves.
    sold_today: Vec<u32>,
    /// Price paid per unit of input at the last restock; part of the cost floor.
    last_input_price: Vec<i64>,
}

impl Economy {
    pub fn new(goods: usize, config: Vec<BusinessConfig>, market: MarketConfig) -> Self {
        let n = config.len();
        let mut stock = vec![0; n * goods];
        for (b, c) in config.iter().enumerate() {
            stock[b * goods + c.output as usize] = c.start_stock;
        }
        Self {
            goods,
            cash: config.iter().map(|b| b.start_cash).collect(),
            price: config.iter().map(|b| b.start_price).collect(),
            wage: config.iter().map(|b| b.start_wage).collect(),
            stock,
            progress_milli: vec![0; n],
            workers: vec![0; n],
            sold_today: vec![0; n],
            last_input_price: vec![0; n],
            config,
            market,
        }
    }

    pub fn len(&self) -> u32 {
        self.config.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.config.is_empty()
    }

    pub fn config(&self, b: BusinessId) -> &BusinessConfig {
        &self.config[b as usize]
    }

    pub fn cash(&self, b: BusinessId) -> i64 {
        self.cash[b as usize]
    }

    pub fn price(&self, b: BusinessId) -> i64 {
        self.price[b as usize]
    }

    pub fn wage(&self, b: BusinessId) -> i64 {
        self.wage[b as usize]
    }

    pub fn workers(&self, b: BusinessId) -> u32 {
        self.workers[b as usize]
    }

    pub fn stock(&self, b: BusinessId, good: GoodId) -> u32 {
        self.stock[b as usize * self.goods + good as usize]
    }

    fn stock_mut(&mut self, b: BusinessId, good: GoodId) -> &mut u32 {
        &mut self.stock[b as usize * self.goods + good as usize]
    }

    pub fn total_cash(&self) -> i64 {
        self.cash.iter().sum()
    }

    pub fn total_stock(&self, good: GoodId) -> u32 {
        (0..self.len()).map(|b| self.stock(b, good)).sum()
    }

    /// Cheapest business selling `good` that has it in stock; ties go to the lower ID.
    pub fn cheapest_seller(&self, good: GoodId) -> Option<BusinessId> {
        (0..self.len())
            .filter(|&b| self.config(b).output == good && self.stock(b, good) > 0)
            .min_by_key(|&b| (self.price(b), b))
    }

    /// Sells one unit to a customer and returns the price paid.
    pub fn sell_one(&mut self, b: BusinessId) -> i64 {
        let good = self.config(b).output;
        let s = self.stock_mut(b, good);
        assert!(*s > 0, "sold from empty stock");
        *s -= 1;
        let price = self.price(b);
        self.cash[b as usize] += price;
        self.sold_today[b as usize] += 1;
        price
    }

    pub fn add_worker(&mut self, b: BusinessId) {
        self.workers[b as usize] += 1;
    }

    pub fn remove_worker(&mut self, b: BusinessId) {
        self.workers[b as usize] -= 1;
    }

    /// Turns `minutes` of one worker's labour into output, limited by input stock.
    pub fn produce(&mut self, b: BusinessId, minutes: u64) -> u32 {
        let c = self.config(b).clone();
        let i = b as usize;
        let p = self.progress_milli[i] + minutes * c.output_milli_per_worker_hour as u64 / 60;
        let possible = (p / 1000) as u32;
        let units = match c.input {
            Some(input) => possible.min(self.stock(b, input.good) / input.amount),
            None => possible,
        };
        // Labour that found no input is lost.
        self.progress_milli[i] = if units < possible {
            0
        } else {
            p - units as u64 * 1000
        };
        if let Some(input) = c.input {
            *self.stock_mut(b, input.good) -= units * input.amount;
        }
        *self.stock_mut(b, c.output) += units;
        units
    }

    /// Pays up to `owed` from the business's cash and returns what was paid.
    pub fn pay(&mut self, b: BusinessId, owed: i64) -> i64 {
        let paid = owed.min(self.cash[b as usize]).max(0);
        self.cash[b as usize] -= paid;
        paid
    }

    /// What one unit of output costs to make: input at the last price paid
    /// plus labour at the current wage, rounded up.
    pub fn unit_cost(&self, b: BusinessId) -> i64 {
        let c = self.config(b);
        let input = c
            .input
            .map_or(0, |i| self.last_input_price[b as usize] * i.amount as i64);
        let rate = c.output_milli_per_worker_hour.max(1) as i64;
        let labour = (self.wage(b).max(0) * 1000 + rate - 1) / rate;
        input + labour
    }

    pub fn payroll_per_day(&self, b: BusinessId, workers: u32, work_hours: i64) -> i64 {
        self.wage(b) * work_hours * workers as i64
    }

    /// Input units the current workforce can process in `days` working days.
    fn processing_capacity(&self, b: BusinessId, work_hours: i64, days: u64) -> u32 {
        let c = self.config(b);
        let Some(input) = c.input else { return 0 };
        let workers = self.workers(b).max(1) as u64;
        let units =
            workers * work_hours as u64 * c.output_milli_per_worker_hour as u64 * days / 1000;
        (units * input.amount as u64).min(u32::MAX as u64) as u32
    }

    /// Morning trade between businesses: each restocks its input, cheapest
    /// supplier first. Input comes before payroll (workers without input
    /// produce nothing), but a business buys at most two days of processing.
    /// If payroll is then short, the daily layoffs shrink the workforce.
    pub fn restock_inputs(&mut self, work_hours: i64) {
        for buyer in 0..self.len() {
            let Some(input) = self.config(buyer).input else {
                continue;
            };
            let mut want = self
                .config(buyer)
                .input_target
                .min(self.processing_capacity(buyer, work_hours, 2))
                .saturating_sub(self.stock(buyer, input.good));
            while want > 0 {
                let Some(seller) = self.cheapest_seller(input.good) else {
                    break;
                };
                let price = self.price(seller);
                let affordable = (self.cash(buyer) / price).clamp(0, u32::MAX as i64) as u32;
                let qty = want.min(self.stock(seller, input.good)).min(affordable);
                if qty == 0 {
                    break;
                }
                *self.stock_mut(seller, input.good) -= qty;
                *self.stock_mut(buyer, input.good) += qty;
                self.cash[buyer as usize] -= price * qty as i64;
                self.cash[seller as usize] += price * qty as i64;
                self.sold_today[seller as usize] += qty;
                self.last_input_price[buyer as usize] = price;
                want -= qty;
            }
        }
    }

    /// Fixed-step price adjustment, once a day.
    ///
    /// Price rises only when goods actually sold and stock ran low (excess
    /// demand); it falls on a glut or when nothing sold at all. Empty shelves
    /// with no buyers leave it unchanged. It never drops below unit cost.
    pub fn adjust_prices(&mut self) {
        let m = self.market.clone();
        for b in 0..self.len() {
            let i = b as usize;
            let c = &self.config[i];
            let stock = self.stock(b, c.output);
            let sold = std::mem::take(&mut self.sold_today[i]);
            if stock > 2 * c.output_target || (sold == 0 && stock > 0) {
                self.price[i] -= m.price_step;
            } else if stock < c.output_target && sold > 0 {
                self.price[i] += m.price_step;
            }
            self.price[i] = self.price[i].max(m.price_min).max(self.unit_cost(b));
        }
    }

    /// Cash a business wants to keep: `reserve_days` of payroll for its crew (at least one).
    pub fn reserve(&self, b: BusinessId, work_hours: i64) -> i64 {
        self.market.reserve_days * self.payroll_per_day(b, self.workers(b).max(1), work_hours)
    }

    /// Fixed-step wage adjustment, once a day: down when cash is below half
    /// the reserve, up when the business has work to give but nobody is looking
    /// for a job.
    pub fn adjust_wages(&mut self, work_hours: i64, job_seekers: bool) {
        let m = self.market.clone();
        for b in 0..self.len() {
            let i = b as usize;
            let c = &self.config[i];
            let short_handed =
                self.workers[i] < c.slots && self.stock(b, c.output) < c.output_target;
            if 2 * self.cash[i] < self.reserve(b, work_hours) {
                self.wage[i] = (self.wage[i] - m.wage_step).max(m.wage_min);
            } else if short_handed && !job_seekers {
                self.wage[i] += m.wage_step;
            }
        }
    }

    /// Takes everything above 1.5 times the reserve out of the business, for payout.
    pub fn take_surplus(&mut self, b: BusinessId, work_hours: i64) -> i64 {
        let surplus = (self.cash(b) - 3 * self.reserve(b, work_hours) / 2).max(0);
        self.cash[b as usize] -= surplus;
        surplus
    }

    /// Adds cash from outside the business (investment, payout remainder).
    pub fn add_cash(&mut self, b: BusinessId, amount: i64) {
        self.cash[b as usize] += amount;
    }

    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_i64_slice(&self.cash);
        h.write_i64_slice(&self.price);
        h.write_i64_slice(&self.wage);
        h.write_u32_slice(&self.stock);
        h.write_u32_slice(&self.workers);
        h.write_u32_slice(&self.sold_today);
        h.write_i64_slice(&self.last_input_price);
        self.progress_milli.iter().for_each(|&p| h.write_u64(p));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const GRAIN: GoodId = 0;
    pub(crate) const FLOUR: GoodId = 1;
    pub(crate) const BREAD: GoodId = 2;

    pub(crate) fn chain() -> Vec<BusinessConfig> {
        let b = |name: &str, output, input, rate, slots, price| BusinessConfig {
            name: name.into(),
            pos: Pos::default(),
            output,
            input,
            output_milli_per_worker_hour: rate,
            slots,
            start_cash: 300,
            start_price: price,
            start_wage: 2,
            start_stock: 0,
            output_target: 60,
            input_target: 60,
        };
        let one = |good| Some(RecipeInput { good, amount: 1 });
        vec![
            b("field", GRAIN, None, 1500, 12, 1),
            b("mill", FLOUR, one(GRAIN), 3000, 6, 1),
            b("bakery", BREAD, one(FLOUR), 3000, 6, 2),
        ]
    }

    pub(crate) fn market() -> MarketConfig {
        MarketConfig {
            price_step: 1,
            price_min: 1,
            wage_step: 1,
            wage_min: 1,
            reserve_days: 3,
        }
    }

    #[test]
    fn production_is_limited_by_input() {
        let mut e = Economy::new(3, chain(), market());
        *e.stock_mut(1, GRAIN) = 2;
        assert_eq!(e.produce(1, 60), 2);
        assert_eq!(e.stock(1, GRAIN), 0);
        assert_eq!(e.stock(1, FLOUR), 2);
    }

    #[test]
    fn fractional_output_accumulates() {
        let mut e = Economy::new(3, chain(), market());
        // 1.5 grain per hour: 40 minutes = 1000 milli.
        assert_eq!(e.produce(0, 20), 0);
        assert_eq!(e.produce(0, 20), 1);
        assert_eq!(e.produce(0, 60), 1);
        assert_eq!(e.produce(0, 60), 2);
    }

    #[test]
    fn restocking_moves_goods_and_money_but_keeps_totals() {
        let mut e = Economy::new(3, chain(), market());
        *e.stock_mut(0, GRAIN) = 100;
        let cash = e.total_cash();
        e.restock_inputs(10);
        assert_eq!(e.stock(1, GRAIN), 60);
        assert_eq!(e.stock(0, GRAIN), 40);
        assert_eq!(e.cash(0), 360);
        assert_eq!(e.total_cash(), cash);
    }

    #[test]
    fn scarce_output_raises_price_glut_lowers_it() {
        let mut e = Economy::new(3, chain(), market());
        // Empty shelves, no buyers: unchanged.
        e.adjust_prices();
        assert_eq!(e.price(2), 2);
        // Sold out with buyers: up.
        *e.stock_mut(2, BREAD) = 1;
        e.sell_one(2);
        e.adjust_prices();
        assert_eq!(e.price(2), 3);
        // Nothing sold despite stock: down.
        *e.stock_mut(2, BREAD) = 10;
        e.adjust_prices();
        assert_eq!(e.price(2), 2);
        *e.stock_mut(2, BREAD) = 500;
        e.adjust_prices();
        e.adjust_prices();
        assert_eq!(e.price(2), 1);
    }

    #[test]
    fn restocking_is_capped_at_two_days_of_processing() {
        let mut c = chain();
        c[1].input_target = 10_000;
        let mut e = Economy::new(3, c, market());
        *e.stock_mut(0, GRAIN) = 10_000;
        e.add_worker(1);
        e.restock_inputs(10);
        // One worker, 3 flour/h, 10 h, 2 days = 60 grain.
        assert_eq!(e.stock(1, GRAIN), 60);
    }

    #[test]
    fn price_never_drops_below_unit_cost() {
        let mut e = Economy::new(3, chain(), market());
        *e.stock_mut(1, FLOUR) = 1000;
        e.restock_inputs(10); // bakery buys flour at the mill's price, 1
        *e.stock_mut(2, BREAD) = 1000;
        for _ in 0..10 {
            e.adjust_prices();
        }
        // Flour 1 + labour ceil(wage 1 x 1000 / 3000) = 1 -> 2.
        assert_eq!(e.unit_cost(2), 2);
        assert_eq!(e.price(2), 2);
    }

    #[test]
    fn cheapest_seller_needs_stock() {
        let mut e = Economy::new(3, chain(), market());
        assert_eq!(e.cheapest_seller(BREAD), None);
        *e.stock_mut(2, BREAD) = 1;
        assert_eq!(e.cheapest_seller(BREAD), Some(2));
        assert_eq!(e.sell_one(2), 2);
        assert_eq!(e.cheapest_seller(BREAD), None);
    }
}
