//! Iteration 3 acceptance: populations with different character traits work
//! measurably different hours and end up with a different wealth distribution.

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
    c
}

/// Per population: (work minutes per agent and day after warm-up, mean money at the end).
fn run(seed: u64, config: WorldConfig) -> (Vec<(u64, i64)>, u32) {
    let groups = config.populations.len();
    let mut world = World::new(seed, config);
    world.run_until(WARMUP * MINUTES_PER_DAY, &mut |_| {});
    let start: Vec<u64> = (0..world.agent_count())
        .map(|a| world.work_minutes(a))
        .collect();
    world.run_until(DAYS * MINUTES_PER_DAY, &mut |_| {});

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
    (per_group, world.stats().money_gini_permille)
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
        assert!(
            money_d > money_e * 3 / 2,
            "seed {seed}: diligent money {money_d} vs easygoing {money_e}"
        );
        assert!(
            gini_mixed > gini_alike + 50,
            "seed {seed}: gini mixed {gini_mixed} vs alike {gini_alike}"
        );
    }
}
