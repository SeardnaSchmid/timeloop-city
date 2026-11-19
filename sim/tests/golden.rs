//! Golden event logs: the simulation must reproduce a stored reference exactly.
//!
//! An intended behaviour change will break these tests. Review the diff, then
//! regenerate the references with:
//!
//! ```bash
//! UPDATE_GOLDEN=1 cargo test -p timeloop-sim --test golden
//! ```

use std::{fs, path::PathBuf};

use timeloop_core::{World, MINUTES_PER_DAY};

fn event_log(seed: u64, agents: u32, days: u64) -> String {
    let mut config = timeloop_content::default_world();
    config.agents = agents;
    let mut world = World::new(seed, config);
    let mut out = String::new();
    world.run_until(days * MINUTES_PER_DAY, &mut |e| {
        out.push_str(&e.to_string());
        out.push('\n');
    });
    out
}

fn check_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_GOLDEN=1)", path.display()))
        .replace("\r\n", "\n");
    if expected != actual {
        let first_diff = expected
            .lines()
            .zip(actual.lines())
            .position(|(e, a)| e != a)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{name} differs from line {}:\n  expected: {:?}\n  actual:   {:?}",
            first_diff + 1,
            expected.lines().nth(first_diff),
            actual.lines().nth(first_diff),
        );
    }
}

#[test]
fn one_agent_one_week() {
    check_golden("one_agent_7_days.log", &event_log(1, 1, 7));
}

#[test]
fn three_agents_share_a_queue() {
    check_golden("three_agents_2_days.log", &event_log(42, 3, 2));
}
