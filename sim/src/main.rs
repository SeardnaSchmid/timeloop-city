//! Headless simulator: runs without graphics and prints the state hash per game day.
//!
//! The output is deliberately stable and line-based so CI can compare two runs
//! (or two platforms) with `diff`.

use std::{path::PathBuf, process::ExitCode};

use timeloop_core::{StateHash, World};

const USAGE: &str = "\
Usage: sim [--seed N] [--days N] [--world FILE] [--quiet]

  --seed N       world seed (default 1)
  --days N       number of game days (default 1)
  --world FILE   custom world.toml instead of the bundled one
  --quiet        print only the final hash";

struct Args {
    seed: u64,
    days: u64,
    world: Option<PathBuf>,
    quiet: bool,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        seed: 1,
        days: 1,
        world: None,
        quiet: false,
    };
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--seed" => {
                a.seed = value("--seed")?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?
            }
            "--days" => {
                a.days = value("--days")?
                    .parse()
                    .map_err(|e| format!("--days: {e}"))?
            }
            "--world" => a.world = Some(value("--world")?.into()),
            "--quiet" => a.quiet = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("{msg}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(if msg.is_empty() { 0 } else { 2 });
        }
    };

    let config = match &args.world {
        Some(path) => match timeloop_content::load_world(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return ExitCode::from(1);
            }
        },
        None => timeloop_content::default_world(),
    };

    let mut world = World::new(args.seed, config);
    for _ in 0..args.days {
        world.step_day();
        if !args.quiet {
            println!("day {:>6} {:016x}", world.day(), world.state_hash());
        }
    }
    if args.quiet {
        println!("{:016x}", world.state_hash());
    }
    ExitCode::SUCCESS
}
