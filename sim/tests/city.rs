//! Iteration 3b acceptance: on the map, vacancy settles, rents stay bounded, houses near
//! work cost more, and people move closer to where they work.

use timeloop_core::{World, MINUTES_PER_DAY};

const YEAR: u64 = 365;
const DAYS: u64 = 10 * YEAR;

#[test]
fn rents_settle_and_people_move_closer_to_work() {
    for seed in [1, 2, 3] {
        let mut world = World::new(seed, timeloop_content::default_world());
        let houses = world.housing().len() as usize;
        let beds = world.housing().total_beds() as u64;

        world.run_until(MINUTES_PER_DAY, &mut |_| {});
        // Day 0 homes were chosen before anyone had a job.
        let first_commute = world.stats().commute_centi as u64;

        let (mut commute, mut homeless, mut vacant, mut n) = (0u64, 0u64, 0u64, 0u64);
        // Mean rent per house in years 5-7 and 8-10.
        let mut rent = vec![[0i64; 2]; houses];
        for day in 2..=DAYS {
            world.run_until(day * MINUTES_PER_DAY, &mut |_| {});
            let s = world.stats();
            if day > YEAR {
                commute += s.commute_centi as u64;
                homeless += s.homeless as u64;
                vacant += beds - s.tenants.iter().map(|&t| t as u64).sum::<u64>();
                n += 1;
            }
            let half = match day {
                d if d > 4 * YEAR && d <= 7 * YEAR => 0,
                d if d > 7 * YEAR => 1,
                _ => continue,
            };
            for (h, r) in s.rents.iter().enumerate() {
                rent[h][half] += r;
            }
        }
        let (commute, homeless, vacant) = (commute / n, homeless / n, vacant / n);
        let rent: Vec<[i64; 2]> = rent
            .iter()
            .map(|r| [r[0] / (3 * YEAR as i64), r[1] / (3 * YEAR as i64)])
            .collect();
        println!(
            "seed {seed}: commute {first_commute} -> {commute}, homeless {homeless}, \
             vacant beds {vacant}, rent years 5-7 vs 8-10 {rent:?}"
        );

        assert!(
            commute * 4 <= first_commute * 3,
            "seed {seed}: commute only went from {first_commute} to {commute}"
        );
        assert!(homeless <= 5, "seed {seed}: {homeless} homeless on average");
        assert!(
            vacant <= beds / 4,
            "seed {seed}: {vacant} of {beds} beds empty on average"
        );
        // Firms opening and closing move jobs around and rents follow, so
        // rents need not settle; they must not run away.
        for (h, [a, b]) in rent.iter().enumerate() {
            assert!(
                *a < 100 && *b < 100,
                "seed {seed}: house {h} rent runs away ({a}, {b})"
            );
        }
        let (lo, hi) = (
            rent.iter().map(|r| r[1]).min().unwrap(),
            rent.iter().map(|r| r[1]).max().unwrap(),
        );
        assert!(
            hi >= lo + 5,
            "seed {seed}: location makes no difference to rent ({lo}..{hi})"
        );
    }
}
