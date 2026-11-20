//! Iteration 2 acceptance: the bundled bread town runs ten years without
//! famine, runaway stock (in any one business) or money appearing from
//! nowhere. Since people age and retire (iteration 4b) single seeds can go
//! through a hungry spell, so hunger is judged on the mean over six seeds,
//! with a cap for each.

use timeloop_core::{World, MINUTES_PER_DAY};

const DAYS: u64 = 3650;
const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

#[test]
#[ignore = "long run; CI runs it with --include-ignored"]
fn ten_years_stay_fed_and_balanced() {
    let mut total_starving = 0;
    for seed in SEEDS {
        let mut world = World::new(seed, timeloop_content::default_world());
        let agents = world.agent_count() as u64;
        let supply = world.money_supply();
        let (mut meals_before, mut meals, mut employed) = (0u64, 0u64, 0u64);
        let mut starving_start = 0;
        let mut max_stock = 0;

        for day in 1..=DAYS {
            world.run_until(day * MINUTES_PER_DAY, &mut |_| {});
            let s = world.stats();
            assert_eq!(world.money_supply(), supply, "seed {seed} day {day}");
            // Skip the first year while prices and staff settle.
            if day == 365 {
                starving_start = s.starving_minutes_total;
            }
            if day > 365 {
                meals += s.meals_total - meals_before;
                employed += s.employed as u64;
                let e = world.economy();
                let stock = (0..e.len()).map(|b| e.stock(b, e.config(b).output)).max();
                max_stock = max_stock.max(stock.unwrap_or(0));
            }
            meals_before = s.meals_total;
        }

        let days = DAYS - 365;
        let (meals, employed) = (meals / days, employed / days);
        let s = world.stats();
        // Share of all agent time spent at critical hunger, in thousandths.
        let starving =
            (s.starving_minutes_total - starving_start) * 1000 / (agents * days * MINUTES_PER_DAY);
        total_starving += starving;
        println!(
            "seed {seed}: starving {starving}‰, meals/day {meals}, employed {employed}, \
             max stock {max_stock}, homeless {}, commute {}",
            s.homeless, s.commute_centi
        );
        assert!(
            starving <= 200,
            "seed {seed}: agents starve {starving}‰ of the time"
        );
        assert!(
            meals >= agents * 3 * 8 / 10,
            "seed {seed}: only {meals} meals a day"
        );
        // About a fifth of the people are in education or retired.
        assert!(
            employed >= agents * 6 / 10,
            "seed {seed}: only {employed} employed"
        );
        assert!(
            max_stock < 1000,
            "seed {seed}: stock ran away to {max_stock}"
        );
    }
    let mean = total_starving / SEEDS.len() as u64;
    println!("mean starving {mean}‰");
    assert!(mean <= 100, "agents starve {mean}‰ of the time on average");
}
