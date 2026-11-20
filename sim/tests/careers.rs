//! Iteration 5b acceptance: workers gain levels, the experienced save more,
//! prices settle instead of swinging, and the town eats at least as well
//! as in iteration 5.

use timeloop_core::{World, MINUTES_PER_DAY};

const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

#[test]
fn experience_pays_and_prices_settle() {
    let (mut swings, mut days, mut starving) = (0i64, 0i64, 0u64);
    let (mut money_low, mut money_high) = (0i64, 0i64);
    for seed in SEEDS {
        // Calibrated for 50 people; the default world now starts with more.
        let mut config = timeloop_content::default_world();
        config.agents = 50;
        let mut w = World::new(seed, config);
        w.run_days(365);
        let start = w.stats().starving_minutes_total;
        let mut last: Vec<i64> = w.stats().prices;
        for _ in 365..3650 {
            w.run_days(1);
            let prices = w.stats().prices;
            // Daily change of the town businesses' prices, in thousandths.
            for (a, b) in last.iter().zip(&prices).take(3) {
                swings += (b - a).abs() * 1000 / (*a).max(1);
                days += 1;
            }
            last = prices;
        }
        let s = w.stats();
        starving += (s.starving_minutes_total - start) * 1000
            / (s.employed + s.unemployed) as u64
            / (3285 * MINUTES_PER_DAY);

        // Workers by level in their job: the upper half earns and saves more.
        let mut workers: Vec<(u32, i64, i64)> = (0..w.agent_count())
            .filter_map(|a| {
                let b = w.employer(a)?;
                let level = w.level(a, w.economy().config(b).output);
                Some((level, w.economy().wage(b), w.money(a)))
            })
            .collect();
        workers.sort();
        let half = workers.len() / 2;
        let mean = |v: &[(u32, i64, i64)], f: fn(&(u32, i64, i64)) -> i64| {
            v.iter().map(f).sum::<i64>() / v.len().max(1) as i64
        };
        let (low, high) = workers.split_at(half);
        println!(
            "seed {seed}: levels {}..{}, money low half {} high half {}, mean level {:.1}",
            workers.first().map_or(0, |w| w.0),
            workers.last().map_or(0, |w| w.0),
            mean(low, |w| w.2),
            mean(high, |w| w.2),
            s.mean_level_tenths as f64 / 10.0
        );
        assert!(
            s.mean_level_tenths >= 50,
            "seed {seed}: workers stay beginners"
        );
        money_low += mean(low, |w| w.2);
        money_high += mean(high, |w| w.2);
    }
    let swing = swings / days;
    let starving = starving / SEEDS.len() as u64;
    println!(
        "daily price change {swing}‰, starving {starving}‰, savings {money_low} vs {money_high}"
    );
    // Single snapshots are noisy (a pensioner's savings, a comfort spree);
    // over all seeds the experienced half has clearly more, even though
    // poor relief (iteration 5c) levies savings above the safety reserve.
    assert!(
        money_high * 100 >= money_low * 115,
        "experience does not pay"
    );
    // Fixed ±1 steps made prices swing by about 9 % a day on average.
    assert!(swing <= 50, "prices swing {swing}‰ a day");
    // Iteration 5: about 90‰ over six seeds.
    assert!(starving <= 90, "starving {starving}‰");
}
