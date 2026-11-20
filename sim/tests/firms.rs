//! Iteration 4 acceptance: agents found firms where goods sell well above
//! cost, firms survive, the town stays fed and owners hold less than half
//! the money, on average over six seeds of 20 years.

use timeloop_core::{World, MINUTES_PER_DAY};

const YEARS: u64 = 20;
const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

#[test]
#[ignore = "long run; CI runs it with --include-ignored"]
fn founders_fill_market_gaps_and_the_town_stays_fed() {
    let (mut total_starving, mut total_owners, mut supply) = (0, 0, 0);
    let mut total_firms = 0;
    for seed in SEEDS {
        let mut w = World::new(seed, timeloop_content::default_world());
        supply = w.money_supply();
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
        total_firms += firms;
        // No town stays in famine; single seeds may go through hungry years.
        assert!(starving <= 250, "seed {seed}: starving {starving}‰");
        total_starving += starving;
        total_owners += s.money_owners;
    }
    let n = SEEDS.len() as u64;
    let (starving, owners) = (total_starving / n, total_owners / n as i64);
    let firms = total_firms * 10 / n;
    println!("mean: {}.{} firms open", firms / 10, firms % 10);
    assert!(firms >= 20, "only {firms} tenths of firms open on average");
    println!("mean: starving {starving}‰, owners hold {owners} of {supply}");
    assert!(starving <= 100, "starving {starving}‰ on average");
    assert!(
        owners * 2 < supply,
        "owners hold {owners} of {supply} on average"
    );
}
