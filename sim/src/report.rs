//! `--report`: a short table of how the town is doing and why it does not
//! grow, one line per period, so a long run reads at a glance.

use std::io::{self, Write};

use timeloop_core::{Diagnosis, World, BRANCH_BLOCKS, MINUTES_PER_DAY};

pub struct Report {
    period: u64,
    last: Snapshot,
}

#[derive(Clone, Default)]
struct Snapshot {
    meals: u64,
    starving: u64,
    diagnosis: Diagnosis,
}

impl Snapshot {
    fn of(w: &World) -> Self {
        let s = w.stats();
        Self {
            meals: s.meals_total,
            starving: s.starving_minutes_total,
            diagnosis: w.diagnosis().clone(),
        }
    }
}

const HEADER: &str = "  day   pop emp%  wage meal  m/d starv‰ broke% empty%  rough  beds  firms branch thouse | branch blocked";

impl Report {
    /// One line per year, or per month for runs under two years.
    pub fn new(days: u64) -> Self {
        Self {
            period: if days >= 730 { 365 } else { 30 },
            last: Snapshot::default(),
        }
    }

    pub fn header(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "{HEADER}")
    }

    /// Prints a line when `day` ends a period (or is the last day).
    pub fn day(&mut self, w: &World, day: u64, last: bool, out: &mut impl Write) -> io::Result<()> {
        if !day.is_multiple_of(self.period) && !last {
            return Ok(());
        }
        let days = (day - 1) % self.period + 1;
        let s = w.stats();
        let now = Snapshot::of(w);
        let pop = w.agent_count() as u64;
        let e = w.economy();
        let food = w.config().food;
        let (mut paid, mut workers) = (0i64, 0i64);
        for b in 0..e.len() {
            paid += s.wages[b as usize] * s.workers[b as usize] as i64;
            workers += s.workers[b as usize] as i64;
        }
        let wage = paid / workers.max(1);
        let meal = (0..e.len())
            .filter(|&b| e.is_open(b) && e.config(b).output == food)
            .map(|b| e.price(b))
            .min()
            .unwrap_or(0);
        let starving = now.starving - self.last.starving;
        let (d, l) = (&now.diagnosis, &self.last.diagnosis);
        let pct = |x: u64| x * 100 / starving.max(1);
        let blocked: Vec<u64> = (0..BRANCH_BLOCKS.len())
            .map(|i| d.branch_blocked[i] - l.branch_blocked[i])
            .collect();
        let total: u64 = blocked.iter().sum::<u64>().max(1);
        let mut reasons: Vec<(u64, &str)> = blocked
            .iter()
            .zip(BRANCH_BLOCKS)
            .filter(|(&n, r)| n > 0 && *r != "closed")
            .map(|(&n, r)| (n, r))
            .collect();
        reasons.sort_by(|a, b| b.0.cmp(&a.0));
        let reasons: Vec<String> = reasons
            .iter()
            .take(3)
            .map(|(n, r)| format!("{r} {}%", n * 100 / total))
            .collect();
        writeln!(
            out,
            "{day:>5} {pop:>5} {:>4} {wage:>5} {meal:>4} {:>4.1} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6} {:>6} | {}",
            s.employed as u64 * 100 / pop.max(1),
            (now.meals - self.last.meals) as f64 / (pop * days).max(1) as f64,
            starving * 1000 / (pop * days * MINUTES_PER_DAY).max(1),
            pct(d.starving_broke_minutes - l.starving_broke_minutes),
            pct(d.starving_no_stock_minutes - l.starving_no_stock_minutes),
            s.homeless,
            s.beds,
            s.firms,
            d.branches,
            d.town_houses,
            reasons.join(", ")
        )?;
        self.last = now;
        Ok(())
    }
}
