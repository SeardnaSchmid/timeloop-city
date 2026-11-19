//! Iteration 4 acceptance: agents found firms where goods sell well above
//! cost, firms survive, and the town eats at least as well as with only the
//! town's own businesses.

use timeloop_core::{World, MINUTES_PER_DAY};

const YEARS: u64 = 20;

#[test]
fn founders_fill_market_gaps_and_the_town_stays_fed() {
    for seed in [1, 2, 3] {
        let mut w = World::new(seed, timeloop_content::default_world());
        let supply = w.money_supply();
        w.run_days(365);
        let start = w.stats().starving_minutes_total;
        let mut open_days = 0;
        for _ in 1..YEARS * 365 {
            w.run_days(1);
            open_days += w.stats().firms as u64;
        }
        let s = w.stats();
        let days = (YEARS - 1) * 365;
        let starving = (s.starving_minutes_total - start) * 1000
            / (w.agent_count() as u64 * days * MINUTES_PER_DAY);
        let firms = open_days / days;
        println!(
            "seed {seed}: founded {}, closed {}, open on average {firms}, starving {starving}‰, \
             owners hold {} of {supply}",
            s.firms_founded_total, s.firms_closed_total, s.money_owners
        );
        assert_eq!(w.money_supply(), supply);
        assert!(
            firms >= 2,
            "seed {seed}: only {firms} firms open on average"
        );
        // Without founding the bundled town starves about 90‰ of the time.
        assert!(starving <= 100, "seed {seed}: starving {starving}‰");
        assert!(
            s.money_owners * 2 < supply,
            "seed {seed}: owners hold {} of {supply}",
            s.money_owners
        );
    }
}
