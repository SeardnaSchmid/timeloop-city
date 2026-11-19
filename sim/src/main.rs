//! Headless simulator: runs without graphics and prints the state hash per game day.
//!
//! The output is deliberately stable and line-based so CI can compare two runs
//! (or two platforms) with `diff`.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::{path::PathBuf, process::ExitCode};

use timeloop_core::{StateHash, Stats, World, MINUTES_PER_DAY};

const USAGE: &str = "\
Usage: sim [--seed N] [--days N] [--agents N] [--world FILE] [--stats FILE] [--log] [--quiet]

  --seed N       world seed (default 1)
  --days N       number of game days (default 1)
  --agents N     override the number of agents from the world file
  --world FILE   custom world.toml instead of the bundled one
  --stats FILE   write one CSV row of economy statistics per day
  --log          print every event before each day's hash
  --quiet        print only the final hash";

struct Args {
    seed: u64,
    days: u64,
    agents: Option<u32>,
    world: Option<PathBuf>,
    stats: Option<PathBuf>,
    log: bool,
    quiet: bool,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        seed: 1,
        days: 1,
        agents: None,
        world: None,
        stats: None,
        log: false,
        quiet: false,
    };
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--seed" => a.seed = parse(&value("--seed")?, "--seed")?,
            "--days" => a.days = parse(&value("--days")?, "--days")?,
            "--agents" => a.agents = Some(parse(&value("--agents")?, "--agents")?),
            "--world" => a.world = Some(value("--world")?.into()),
            "--stats" => a.stats = Some(value("--stats")?.into()),
            "--log" => a.log = true,
            "--quiet" => a.quiet = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    if a.log && a.quiet {
        return Err("--log and --quiet exclude each other".into());
    }
    Ok(a)
}

fn parse<T: std::str::FromStr>(s: &str, name: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    s.parse().map_err(|e| format!("{name}: {e}"))
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

    let mut config = match &args.world {
        Some(path) => match timeloop_content::load_world(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return ExitCode::from(1);
            }
        },
        None => timeloop_content::default_world(),
    };
    if let Some(n) = args.agents {
        config.agents = n;
    }

    match run(&args, World::new(args.seed, config)) {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (e.g. `| head`) is not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &Args, mut world: World) -> io::Result<()> {
    let mut out = BufWriter::new(io::stdout().lock());
    let mut stats = match &args.stats {
        Some(path) => {
            let mut f = BufWriter::new(File::create(path)?);
            writeln!(f, "{}", csv_header(&world))?;
            Some(f)
        }
        None => None,
    };
    let mut meals_before = 0;
    for day in 1..=args.days {
        let mut result = Ok(());
        world.run_until(day * MINUTES_PER_DAY, &mut |entry| {
            if args.log && result.is_ok() {
                result = writeln!(out, "{entry}");
            }
        });
        result?;
        if let Some(f) = &mut stats {
            let s = world.stats();
            writeln!(f, "{}", csv_row(day, &s, s.meals_total - meals_before))?;
            meals_before = s.meals_total;
        }
        if !args.quiet {
            writeln!(out, "day {day:>6} {:016x}", world.state_hash())?;
        }
    }
    if args.quiet {
        writeln!(out, "{:016x}", world.state_hash())?;
    }
    if let Some(f) = &mut stats {
        f.flush()?;
    }
    out.flush()
}

fn csv_header(world: &World) -> String {
    let c = world.config();
    let mut cols = vec![
        "day".to_string(),
        "money_agents".into(),
        "money_businesses".into(),
    ];
    cols.extend(c.businesses.iter().map(|b| format!("price_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("wage_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("cash_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("workers_{}", b.name)));
    cols.extend(c.goods.iter().map(|g| format!("stock_{g}")));
    cols.extend(["employed", "unemployed", "hungry", "meals"].map(String::from));
    cols.join(",")
}

fn csv_row(day: u64, s: &Stats, meals: u64) -> String {
    let mut cols = vec![
        day.to_string(),
        s.money_agents.to_string(),
        s.money_businesses.to_string(),
    ];
    cols.extend(s.prices.iter().map(i64::to_string));
    cols.extend(s.wages.iter().map(i64::to_string));
    cols.extend(s.cash.iter().map(i64::to_string));
    cols.extend(s.workers.iter().map(u32::to_string));
    cols.extend(s.stock.iter().map(u32::to_string));
    cols.extend([s.employed, s.unemployed, s.hungry].map(|v| v.to_string()));
    cols.push(meals.to_string());
    cols.join(",")
}
