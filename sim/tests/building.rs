//! Iteration 5c acceptance: people found kinds of business the town never
//! had (beer, wood, furniture) and buy their goods, build houses, and the
//! town grows when work is left over, while it stays fed and no money
//! appears or vanishes. Six seeds of 30 years.

use timeloop_core::{World, MINUTES_PER_DAY};

const YEARS: u64 = 30;
const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

#[test]
#[ignore = "long run; CI runs it with --include-ignored"]
fn the_town_builds_grows_and_buys_new_goods() {
    // Calibrated for 50 people; the default world now starts with more.
    let mut start = timeloop_content::default_world();
    start.agents = 50;
    let people = start.agents;
    let first: Vec<_> = start.businesses.iter().map(|b| b.output).collect();
    let (mut starving, mut houses, mut grew, mut comforts, mut new_kinds) = (0, 0, 0, 0, 0);
    for seed in SEEDS {
        let mut w = World::new(seed, start.clone());
        let supply = w.money_supply();
        let mut kinds = std::collections::BTreeSet::new();
        for _ in 0..YEARS {
            w.run_days(365);
            let e = w.economy();
            for b in (0..e.len()).filter(|&b| e.is_open(b)) {
                let good = e.config(b).output as usize;
                if !first.contains(&(good as u32)) {
                    kinds.insert(good);
                }
            }
            assert_eq!(w.money_supply(), supply, "seed {seed}");
        }
        let s = w.stats();
        let st = s.starving_minutes_total * 1000
            / (w.agent_count() as u64 * YEARS * 365 * MINUTES_PER_DAY);
        println!(
            "seed {seed}: {} people, {} houses built, {} comforts, new kinds {kinds:?}, starving {st}‰",
            w.agent_count(),
            s.houses_built_total,
            s.comforts_total
        );
        starving += st;
        houses += s.houses_built_total;
        grew += (w.agent_count() > people) as u32;
        comforts += (s.comforts_total > 0) as u32;
        new_kinds += kinds.len();
    }
    let n = SEEDS.len() as u64;
    assert!(grew as u64 == n, "only {grew} towns grew");
    assert!(
        comforts as u64 == n,
        "comfort goods sold in only {comforts} towns"
    );
    assert!(
        new_kinds >= 2 * SEEDS.len(),
        "only {new_kinds} new kinds of business"
    );
    assert!(houses >= n, "only {houses} houses built");
    assert!(starving / n <= 30, "starving {}‰ on average", starving / n);
}
