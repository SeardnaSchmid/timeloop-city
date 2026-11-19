//! Headless simulator: runs without graphics and prints the state hash per game day.
//!
//! The output is deliberately stable and line-based so CI can compare two runs
//! (or two platforms) with `diff`.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::time::SystemTime;
use std::{path::PathBuf, process::ExitCode};

mod live;
mod serve;
mod viewer;

use timeloop_core::{
    Allowed, Explanation, Minute, StateHash, Stats, World, WorldConfig, MINUTES_PER_DAY,
};

const USAGE: &str = "\
Usage: sim [OPTIONS]
       sim explain AGENT TIME [OPTIONS]
       sim serve [--port N] [OPTIONS]

  --seed N       world seed (default 1)
  --days N       number of game days (default 1)
  --agents N     override the number of agents from the world file
  --world FILE   custom world.toml instead of the bundled one
  --watch        reload the decision rules from --world when the file changes
  --stats FILE   write one CSV row of economy statistics per day
  --years N      number of game years of 365 days (instead of --days)
  --html FILE    write a map viewer (open it in a browser) of the whole run
  --html-days N  let the viewer cover only the last N days
  --html-step M  minutes between viewer frames (default: about 4000 frames,
                 at least 10 minutes; daily or longer steps show noon)
  --log          print every event before each day's hash
  --quiet        print only the final hash

serve runs the engine live (default 80 years, port 7878) and serves a page
to steer and watch it at http://127.0.0.1:PORT.

explain runs to TIME (minutes, or DAY:HH:MM such as 3:08:00) and shows how
AGENT scores every action at that moment.";

struct Args {
    seed: u64,
    days: u64,
    agents: Option<u32>,
    world: Option<PathBuf>,
    watch: bool,
    stats: Option<PathBuf>,
    html: Option<PathBuf>,
    html_days: Option<u64>,
    html_step: Option<Minute>,
    log: bool,
    quiet: bool,
    explain: Option<(u32, Minute)>,
    serve: bool,
    port: u16,
    /// Whether --days or --years was given.
    days_given: bool,
}

fn parse_time(s: &str) -> Result<Minute, String> {
    let bad = || format!("bad time \"{s}\": use minutes or DAY:HH:MM");
    let parts: Vec<&str> = s.split(':').collect();
    let n = |p: &str| p.parse::<Minute>().map_err(|_| bad());
    match parts[..] {
        [m] => n(m),
        [d, h, m] if n(h)? < 24 && n(m)? < 60 => Ok(n(d)? * MINUTES_PER_DAY + n(h)? * 60 + n(m)?),
        _ => Err(bad()),
    }
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        seed: 1,
        days: 1,
        agents: None,
        world: None,
        watch: false,
        stats: None,
        html: None,
        html_days: None,
        html_step: None,
        log: false,
        quiet: false,
        explain: None,
        serve: false,
        port: 7878,
        days_given: false,
    };
    let mut it = it.by_ref().peekable();
    if it.peek().map(String::as_str) == Some("serve") {
        it.next();
        a.serve = true;
    } else if it.peek().map(String::as_str) == Some("explain") {
        it.next();
        let agent = it.next().ok_or("explain needs AGENT and TIME")?;
        let time = it.next().ok_or("explain needs AGENT and TIME")?;
        a.explain = Some((parse(&agent, "AGENT")?, parse_time(&time)?));
    }
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--seed" => a.seed = parse(&value("--seed")?, "--seed")?,
            "--days" => {
                a.days = parse(&value("--days")?, "--days")?;
                a.days_given = true;
            }
            "--port" => a.port = parse(&value("--port")?, "--port")?,
            "--agents" => a.agents = Some(parse(&value("--agents")?, "--agents")?),
            "--world" => a.world = Some(value("--world")?.into()),
            "--watch" => a.watch = true,
            "--stats" => a.stats = Some(value("--stats")?.into()),
            "--html" => a.html = Some(value("--html")?.into()),
            "--html-days" => a.html_days = Some(parse(&value("--html-days")?, "--html-days")?),
            "--html-step" => a.html_step = Some(parse(&value("--html-step")?, "--html-step")?),
            "--years" => {
                a.days = 365 * parse::<u64>(&value("--years")?, "--years")?;
                a.days_given = true;
            }
            "--log" => a.log = true,
            "--quiet" => a.quiet = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    if a.log && a.quiet {
        return Err("--log and --quiet exclude each other".into());
    }
    if a.watch && a.world.is_none() {
        return Err("--watch needs --world".into());
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

    if args.serve {
        let days = if args.days_given { args.days } else { 80 * 365 };
        let live = live::Live::new(args.seed, config, days * MINUTES_PER_DAY);
        return match serve::serve(live, args.port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("cannot serve on port {}: {e}", args.port);
                ExitCode::from(1)
            }
        };
    }

    if let Some((agent, at)) = args.explain {
        if agent >= config.agents {
            eprintln!("agent {agent} does not exist ({} agents)", config.agents);
            return ExitCode::from(2);
        }
        let mut world = World::new(args.seed, config);
        world.run_until(at, &mut |_| {});
        print!("{}", format_explanation(&world, &world.explain(agent)));
        return ExitCode::SUCCESS;
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
    let mut recorder = args.html.as_ref().map(|_| {
        let days = args.html_days.unwrap_or(args.days).min(args.days);
        let start = (args.days - days) * MINUTES_PER_DAY;
        viewer::Recorder::new(start, days * MINUTES_PER_DAY, args.html_step)
    });
    let mut meals_before = 0;
    let mut work_before = 0;
    let mut starving_before = 0;
    let mut watched = args
        .world
        .as_ref()
        .filter(|_| args.watch)
        .map(|p| (p, modified(p)));
    for day in 1..=args.days {
        if let Some((path, seen)) = &mut watched {
            let now = modified(path);
            if now != *seen {
                *seen = now;
                match timeloop_content::load_world(path) {
                    Ok(c) => {
                        reload(&mut world, c);
                        eprintln!("day {day}: reloaded rules from {}", path.display());
                    }
                    Err(e) => eprintln!("day {day}: {}: {e} (keeping old rules)", path.display()),
                }
            }
        }
        // Stop at every viewer frame due today, then at the end of the day.
        let end = day * MINUTES_PER_DAY;
        loop {
            let frame = recorder
                .as_ref()
                .and_then(|r| r.next_frame())
                .filter(|&t| t <= end);
            let until = frame.unwrap_or(end);
            let mut result = Ok(());
            world.run_until(until, &mut |entry| {
                if args.log && result.is_ok() {
                    result = writeln!(out, "{entry}");
                }
            });
            result?;
            match (frame, &mut recorder) {
                (Some(_), Some(r)) => r.record(&world),
                _ => break,
            }
        }
        if let Some(f) = &mut stats {
            let s = world.stats();
            let work_hours = (s.work_minutes_total - work_before) / 60;
            let agent_minutes = (world.agent_count() as u64 * MINUTES_PER_DAY).max(1);
            let starving = (s.starving_minutes_total - starving_before) * 1000 / agent_minutes;
            let meals = s.meals_total - meals_before;
            writeln!(
                f,
                "{}",
                csv_row(
                    day,
                    &s,
                    world.config().businesses.len(),
                    meals,
                    work_hours,
                    starving
                )
            )?;
            meals_before = s.meals_total;
            work_before = s.work_minutes_total;
            starving_before = s.starving_minutes_total;
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
    if let (Some(r), Some(path)) = (&recorder, &args.html) {
        r.write(&world, path)?;
        eprintln!("map viewer written to {}", path.display());
    }
    out.flush()
}

fn csv_header(world: &World) -> String {
    let c = world.config();
    let mut cols = vec![
        "day".to_string(),
        "money_agents".into(),
        "money_businesses".into(),
        "money_total".into(),
    ];
    cols.extend(c.businesses.iter().map(|b| format!("price_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("wage_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("cash_{}", b.name)));
    cols.extend(c.businesses.iter().map(|b| format!("workers_{}", b.name)));
    cols.extend(c.goods.iter().map(|g| format!("stock_{g}")));
    cols.extend(
        [
            "employed",
            "unemployed",
            "hungry",
            "meals",
            "work_hours",
            "gini_money",
            "starving_permille",
        ]
        .map(String::from),
    );
    cols.extend(
        c.houses
            .iter()
            .map(|h| format!("rent_{}", h.name.replace(' ', "_"))),
    );
    cols.extend(
        c.houses
            .iter()
            .map(|h| format!("tenants_{}", h.name.replace(' ', "_"))),
    );
    cols.extend(
        [
            "homeless",
            "commute_centi",
            "firms",
            "firms_founded",
            "firms_closed",
            "money_owners",
            "deaths",
            "births",
            "newcomers",
            "mean_age",
            "in_education",
            "retired",
            "price_index",
            "mean_level_tenths",
            "population",
            "immigrants",
            "beds",
            "houses_built",
            "comforts",
        ]
        .map(String::from),
    );
    cols.extend(
        c.associations
            .iter()
            .map(|a| format!("cash_{}", a.name.replace(' ', "_"))),
    );
    cols.join(",")
}

/// One CSV row; `meals`, `work_hours` and `starving_permille` are for this day.
/// Per-business columns cover the town's businesses; founded firms are summed up.
fn csv_row(
    day: u64,
    s: &Stats,
    town: usize,
    meals: u64,
    work_hours: u64,
    starving_permille: u64,
) -> String {
    let mut cols = vec![
        day.to_string(),
        s.money_agents.to_string(),
        s.money_businesses.to_string(),
        s.money_total.to_string(),
    ];
    cols.extend(s.prices[..town].iter().map(i64::to_string));
    cols.extend(s.wages[..town].iter().map(i64::to_string));
    cols.extend(s.cash[..town].iter().map(i64::to_string));
    cols.extend(s.workers[..town].iter().map(u32::to_string));
    cols.extend(s.stock.iter().map(u32::to_string));
    cols.extend([s.employed, s.unemployed, s.hungry].map(|v| v.to_string()));
    cols.push(meals.to_string());
    cols.push(work_hours.to_string());
    cols.push(s.money_gini_permille.to_string());
    cols.push(starving_permille.to_string());
    // Per-house columns cover the town's houses; built ones are in the totals.
    let houses = s.rents.len() - s.houses_built_total as usize;
    cols.extend(s.rents[..houses].iter().map(i64::to_string));
    cols.extend(s.tenants[..houses].iter().map(u32::to_string));
    cols.push(s.homeless.to_string());
    cols.push(s.commute_centi.to_string());
    cols.push(s.firms.to_string());
    cols.push(s.firms_founded_total.to_string());
    cols.push(s.firms_closed_total.to_string());
    cols.push(s.money_owners.to_string());
    cols.extend([s.deaths_total, s.births_total, s.arrivals_total].map(|v| v.to_string()));
    cols.extend([s.mean_age, s.in_education, s.retired].map(|v| v.to_string()));
    cols.push(s.price_index.to_string());
    cols.push(s.mean_level_tenths.to_string());
    cols.extend(
        [
            (s.employed + s.unemployed) as u64,
            s.immigrants_total,
            s.beds as u64,
            s.houses_built_total,
            s.comforts_total,
        ]
        .map(|v| v.to_string()),
    );
    cols.extend(s.assoc_cash.iter().map(i64::to_string));
    cols.join(",")
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Takes the new decision rules; population and economy stay as they are.
fn reload(world: &mut World, new: WorldConfig) {
    if new.traits != world.config().traits {
        eprintln!("trait names changed; restart to apply them (keeping old rules)");
        return;
    }
    world.set_behaviour(new.behaviour);
}

fn clock(at: Minute) -> String {
    let m = at % MINUTES_PER_DAY;
    format!("d{:04} {:02}:{:02}", at / MINUTES_PER_DAY, m / 60, m % 60)
}

fn format_explanation(world: &World, e: &Explanation) -> String {
    let mut out = format!("a{} at {}\n", e.agent, clock(e.minute));
    let action = world.action(e.agent);
    out += &format!(
        "  doing     {} until {}\n",
        action.kind.name(),
        clock(action.ends)
    );
    out += "\nvariables\n";
    for (name, value) in &e.vars {
        out += &format!("  {name:<16} {value:>8}\n");
    }
    out += "\ncandidates\n";
    let rules = &world.config().behaviour.actions;
    for (c, rule) in e.candidates.iter().zip(rules) {
        let mark = if c.kind == e.winner { "->" } else { "  " };
        let status = match c.allowed {
            Allowed::Yes => "allowed".to_string(),
            Allowed::NotPossible => "not possible now".to_string(),
            Allowed::RuleSaysNo => format!(
                "blocked by when = {}",
                rule.when.as_ref().map_or("", |w| w.source())
            ),
        };
        out += &format!(
            "{mark} {:<9} {:>7}  {status}\n             score = {}\n",
            c.kind.name(),
            c.score,
            rule.score.source()
        );
    }
    out += &format!("\nwinner: {}\n", e.winner.name());
    out
}
