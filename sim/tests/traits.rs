//! Iteration 3 acceptance: populations with different character traits work
//! measurably different hours and end up with a different wealth distribution.
//!
//! The town-wide Gini is printed but not checked: since the map (iteration
//! 3b), location, rent and unemployment make identical agents unequal too.

use timeloop_core::{Population, World, WorldConfig, MINUTES_PER_DAY};

const DAYS: u64 = 2 * 365;
/// The first year is skipped while prices and staff settle.
const WARMUP: u64 = 365;

fn population(name: &str, greed: (u32, u32), work_life: (u32, u32)) -> Population {
    Population {
        name: name.into(),
        weight: 1,
        traits: vec![greed, work_life],
    }
}

fn world_with(populations: Vec<Population>) -> WorldConfig {
    let mut c = timeloop_content::default_world();
    assert_eq!(c.traits, ["greed", "work_life"]);
    c.populations = populations;
    // A fixed town: newcomers would blur the populations' shares.
    c.growth = None;
    c
}

/// Per population: (work minutes per agent and day after warm-up, mean money
/// at the end), and the money Gini averaged over the days after warm-up.
fn run(seed: u64, config: WorldConfig) -> (Vec<(u64, i64)>, u32) {
    let groups = config.populations.len();
    let mut world = World::new(seed, config);
    world.run_until(WARMUP * MINUTES_PER_DAY, &mut |_| {});
    let start: Vec<u64> = (0..world.agent_count())
        .map(|a| world.work_minutes(a))
        .collect();
    let mut gini = 0u64;
    for day in WARMUP + 1..=DAYS {
        world.run_until(day * MINUTES_PER_DAY, &mut |_| {});
        gini += world.stats().money_gini_permille as u64;
    }
    let gini = (gini / (DAYS - WARMUP)) as u32;

    let mut minutes = vec![0u64; groups];
    let mut money = vec![0i64; groups];
    let mut count = vec![0u64; groups];
    for a in 0..world.agent_count() {
        let p = world.population(a) as usize;
        minutes[p] += world.work_minutes(a) - start[a as usize];
        money[p] += world.money(a);
        count[p] += 1;
    }
    let per_group = (0..groups)
        .map(|p| {
            let per_day = minutes[p] / count[p] / (DAYS - WARMUP);
            (per_day, money[p] / count[p] as i64)
        })
        .collect();
    (per_group, gini)
}

#[test]
fn traits_change_work_hours_and_wealth() {
    for seed in [1, 2, 3] {
        let (groups, gini_mixed) = run(
            seed,
            world_with(vec![
                population("diligent", (700, 1000), (0, 300)),
                population("easygoing", (0, 300), (700, 1000)),
            ]),
        );
        let ((work_d, money_d), (work_e, money_e)) = (groups[0], groups[1]);
        let (_, gini_alike) = run(
            seed,
            world_with(vec![population("alike", (500, 500), (500, 500))]),
        );
        println!(
            "seed {seed}: diligent {work_d} min/day, money {money_d}; \
             easygoing {work_e} min/day, money {money_e}; gini mixed {gini_mixed}, alike {gini_alike}"
        );

        // At least an hour a day more.
        assert!(
            work_d >= work_e + 60,
            "seed {seed}: diligent {work_d} min vs easygoing {work_e} min"
        );
        // Comfort spending, pensions and inheritance even out savings, so
        // the gap is smaller than the gap in work.
        assert!(
            money_d > money_e * 5 / 4,
            "seed {seed}: diligent money {money_d} vs easygoing {money_e}"
        );
    }
}
