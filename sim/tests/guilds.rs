//! Iteration 5 acceptance: a guild rule (a minimum price for bread) visibly
//! raises the price index, and associations never create or destroy money.

use timeloop_core::{RuleKind, World, WorldConfig};

const DAYS: u64 = 3650;
const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

/// Mean price index over years 2-10, over all seeds.
fn mean_price_index(config: &WorldConfig) -> i64 {
    let mut sum = 0;
    for seed in SEEDS {
        let mut w = World::new(seed, config.clone());
        let supply = w.money_supply();
        w.run_days(365);
        for _ in 365..DAYS {
            w.run_days(1);
            sum += w.stats().price_index;
        }
        assert_eq!(w.money_supply(), supply, "seed {seed}");
    }
    sum / (SEEDS.len() as i64 * (DAYS - 365) as i64)
}

#[test]
#[ignore = "long run; CI runs it with --include-ignored"]
fn a_guild_minimum_price_raises_the_price_index() {
    let with_rule = timeloop_content::default_world();
    let guild = (with_rule.associations.iter())
        .position(|a| a.rule_index(RuleKind::MinPrice).is_some())
        .expect("the bundled town has a guild with a minimum price");
    let mut without = with_rule.clone();
    without.associations[guild]
        .rules
        .retain(|r| r.kind != RuleKind::MinPrice);

    let (a, b) = (mean_price_index(&with_rule), mean_price_index(&without));
    println!("price index with minimum price {a}, without {b}");
    assert!(a * 100 >= b * 105, "only {a} with the rule vs {b} without");
}
