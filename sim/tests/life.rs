//! Iteration 4b acceptance: over a whole life of 80 years people age, retire
//! and die, children and newcomers take their places, the age structure
//! stays sound and no money appears or vanishes.

use timeloop_core::{Stage, World, DAYS_PER_YEAR};

#[test]
#[ignore = "long run; CI runs it with --include-ignored"]
fn eighty_years_of_generations() {
    let mut w = World::new(1, timeloop_content::default_world());
    let supply = w.money_supply();
    let life = w
        .config()
        .life
        .clone()
        .expect("the bundled town has a life course");
    for year in 1..=80 {
        w.run_days(DAYS_PER_YEAR);
        assert_eq!(w.money_supply(), supply, "year {year}");
        for a in 0..w.agent_count() {
            let age = w.age_days(a) / DAYS_PER_YEAR;
            assert!(age < life.lifespan_max as u64 + 5, "a{a} is {age}");
            if w.stage(a) != Stage::Working {
                assert_eq!(w.employer(a), None, "a{a} works at {age}");
            }
        }
    }
    let s = w.stats();
    println!(
        "deaths {}, births {}, newcomers {}, mean age {}, in education {}, retired {}",
        s.deaths_total, s.births_total, s.arrivals_total, s.mean_age, s.in_education, s.retired
    );
    // 50 people living about 80 years: everyone alive at the start has died.
    assert!(s.deaths_total >= 50, "only {} deaths", s.deaths_total);
    assert_eq!(s.births_total + s.arrivals_total, s.deaths_total);
    assert!(
        s.births_total > s.arrivals_total,
        "most places go to children"
    );
    assert!((30..=60).contains(&s.mean_age), "mean age {}", s.mean_age);
    let people = w.agent_count();
    assert!(
        s.retired * 100 < 35 * people,
        "{} of {people} retired",
        s.retired
    );
}
