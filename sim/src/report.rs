//! `--report`: a short table of how the town is doing and why it does not
//! grow, one line per period, so a long run reads at a glance.
//!
//! `--snapshots DIR` cuts the run at years 1, 2, 3, 5, 10, 15, ... and
//! writes, per cut, a row of `periods.csv`, the map as text
//! (`map_yNN.txt`) and every agent (`agents_yNN.csv`).

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use timeloop_core::{Building, Diagnosis, Flows, Pos, World, BRANCH_BLOCKS, MINUTES_PER_DAY};

const YEAR: u64 = 365;

pub struct Report {
    cuts: Vec<u64>,
    last_day: u64,
    last: Snapshot,
    dir: Option<PathBuf>,
}

#[derive(Clone, Default)]
struct Snapshot {
    meals: u64,
    starving: u64,
    diagnosis: Diagnosis,
    flows: Flows,
}

impl Snapshot {
    fn of(w: &World) -> Self {
        let s = w.stats();
        Self {
            meals: s.meals_total,
            starving: s.starving_minutes_total,
            diagnosis: w.diagnosis().clone(),
            flows: *w.flows(),
        }
    }
}

/// One period's numbers, for the table and `periods.csv`.
struct Row {
    cols: Vec<(String, String)>,
    blocked: String,
}

const TABLE: [&str; 15] = [
    "day",
    "pop",
    "emp%",
    "wage",
    "meal",
    "m/d",
    "starv‰",
    "broke%",
    "empty%",
    "rough",
    "beds",
    "firms",
    "branch",
    "thouse",
    "money p10/50/90",
];

impl Report {
    /// One line per year, or per month for runs under two years.
    pub fn new(days: u64) -> Self {
        let period = if days >= 2 * YEAR { YEAR } else { 30 };
        Self::with_cuts((1..=days / period).map(|k| k * period).collect(), None)
    }

    /// Lines at years 1, 2, 3, 5 and then every 5 years; files into `dir`.
    pub fn snapshots(days: u64, dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut years = vec![1, 2, 3];
        years.extend((1..).map(|k| 5 * k).take_while(|&y| y * YEAR <= days));
        let cuts = years.into_iter().map(|y| y * YEAR).filter(|&d| d <= days);
        Ok(Self::with_cuts(cuts.collect(), Some(dir.to_path_buf())))
    }

    fn with_cuts(cuts: Vec<u64>, dir: Option<PathBuf>) -> Self {
        Self {
            cuts,
            last_day: 0,
            last: Snapshot::default(),
            dir,
        }
    }

    pub fn header(&self, out: &mut impl Write) -> io::Result<()> {
        let head: Vec<String> = TABLE.iter().map(|h| format!("{h:>6}")).collect();
        writeln!(out, "{} | branch blocked", head.join(" "))
    }

    /// Prints a line when `day` is a cut (or the last day), and writes the
    /// snapshot files if there is a directory.
    pub fn day(&mut self, w: &World, day: u64, last: bool, out: &mut impl Write) -> io::Result<()> {
        if !self.cuts.contains(&day) && !last {
            return Ok(());
        }
        let row = self.row(w, day);
        let line: Vec<String> = row.cols[..TABLE.len()]
            .iter()
            .map(|(_, v)| format!("{v:>6}"))
            .collect();
        writeln!(out, "{} | {}", line.join(" "), row.blocked)?;
        if let Some(dir) = &self.dir {
            let tag = format!("y{:02}", day.div_ceil(YEAR));
            write_period(&dir.join("periods.csv"), &row, self.last_day == 0)?;
            write_map(w, &dir.join(format!("map_{tag}.txt")))?;
            write_agents(w, &dir.join(format!("agents_{tag}.csv")))?;
        }
        self.last = Snapshot::of(w);
        self.last_day = day;
        Ok(())
    }

    fn row(&self, w: &World, day: u64) -> Row {
        let days = day - self.last_day;
        let s = w.stats();
        let now = Snapshot::of(w);
        let pop = w.agent_count() as u64;
        let e = w.economy();
        let food = w.config().food;
        let (mut paid, mut workers) = (0i64, 0i64);
        for b in 0..e.len() {
            paid += s.wages[b as usize] * s.workers[b as usize] as i64;
            workers += s.workers[b as usize] as i64;
        }
        let meal = (0..e.len())
            .filter(|&b| e.is_open(b) && e.config(b).output == food)
            .map(|b| e.price(b))
            .min()
            .unwrap_or(0);
        let starving = now.starving - self.last.starving;
        let (d, l) = (&now.diagnosis, &self.last.diagnosis);
        let pct = |x: u64| x * 100 / starving.max(1);
        let blocked: Vec<u64> = (0..BRANCH_BLOCKS.len())
            .map(|i| d.branch_blocked[i] - l.branch_blocked[i])
            .collect();
        let total: u64 = blocked.iter().sum::<u64>().max(1);
        let mut reasons: Vec<(u64, &str)> = blocked
            .iter()
            .zip(BRANCH_BLOCKS)
            .filter(|(&n, r)| n > 0 && *r != "closed")
            .map(|(&n, r)| (n, r))
            .collect();
        reasons.sort_by(|a, b| b.0.cmp(&a.0));
        let reasons: Vec<String> = reasons
            .iter()
            .take(3)
            .map(|(n, r)| format!("{r} {}%", n * 100 / total))
            .collect();
        let mut money: Vec<i64> = (0..w.agent_count()).map(|a| w.money(a)).collect();
        money.sort_unstable();
        let q = |p: usize| money.get(money.len() * p / 100).copied().unwrap_or(0);
        let goods = &w.config().goods;
        let price = |g: usize| {
            (0..e.len())
                .filter(|&b| e.is_open(b) && e.config(b).output as usize == g)
                .map(|b| e.price(b))
                .min()
                .map_or("-".into(), |p| p.to_string())
        };
        let makers = |g: usize| {
            (0..e.len())
                .filter(|&b| e.is_open(b) && e.config(b).output as usize == g)
                .count()
        };
        let cols: Vec<(&str, String)> = vec![
            ("day", day.to_string()),
            ("pop", pop.to_string()),
            ("emp%", (s.employed as u64 * 100 / pop.max(1)).to_string()),
            ("wage", (paid / workers.max(1)).to_string()),
            ("meal", meal.to_string()),
            (
                "m/d",
                format!(
                    "{:.1}",
                    (now.meals - self.last.meals) as f64 / (pop * days).max(1) as f64
                ),
            ),
            (
                "starv‰",
                (starving * 1000 / (pop * days * MINUTES_PER_DAY).max(1)).to_string(),
            ),
            (
                "broke%",
                pct(d.starving_broke_minutes - l.starving_broke_minutes).to_string(),
            ),
            (
                "empty%",
                pct(d.starving_no_stock_minutes - l.starving_no_stock_minutes).to_string(),
            ),
            ("rough", s.homeless.to_string()),
            ("beds", s.beds.to_string()),
            ("firms", s.firms.to_string()),
            ("branch", d.branches.to_string()),
            ("thouse", d.town_houses.to_string()),
            ("money", format!("{}/{}/{}", q(10), q(50), q(90))),
        ];
        let extra: Vec<(&str, String)> = vec![
            ("mean_age", s.mean_age.to_string()),
            ("education", s.in_education.to_string()),
            ("retired", s.retired.to_string()),
            ("deaths", s.deaths_total.to_string()),
            ("births", s.births_total.to_string()),
            ("arrivals", s.arrivals_total.to_string()),
            ("founded", s.firms_founded_total.to_string()),
            ("closed", s.firms_closed_total.to_string()),
            ("houses_built", s.houses_built_total.to_string()),
            ("comforts", s.comforts_total.to_string()),
            ("level_tenths", s.mean_level_tenths.to_string()),
            ("price_index", s.price_index.to_string()),
            (
                "purchasing_power",
                format!("{:.2}", w.purchasing_power_centi() as f64 / 100.0),
            ),
        ];
        let mut cols: Vec<(String, String)> = (cols.into_iter().chain(extra))
            .map(|(n, v)| (n.to_string(), v))
            .collect();
        // Money moved during the period, by kind; closures by reason.
        for (n, v) in now.flows.since(&self.last.flows) {
            cols.push((n.to_string(), v.to_string()));
        }
        for (g, name) in goods.iter().enumerate() {
            cols.push((
                format!("{name}_makers/price"),
                format!("{}/{}", makers(g), price(g)),
            ));
        }
        Row {
            cols,
            blocked: reasons.join(", "),
        }
    }
}

fn write_period(path: &Path, row: &Row, fresh: bool) -> io::Result<()> {
    let mut f = if fresh {
        let mut f = File::create(path)?;
        let head: Vec<&str> = row.cols.iter().map(|(n, _)| n.as_str()).collect();
        writeln!(f, "{},branch_blocked", head.join(","))?;
        f
    } else {
        File::options().append(true).open(path)?
    };
    let vals: Vec<&str> = row.cols.iter().map(|(_, v)| v.as_str()).collect();
    writeln!(f, "{},\"{}\"", vals.join(","), row.blocked)
}

/// One character per parcel: a letter per good for businesses (upper case:
/// the town's, lower case: an agent's), `H`/`h` for houses, `.` empty.
fn write_map(w: &World, path: &Path) -> io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    let goods = &w.config().goods;
    let letter = |g: usize| (b'a' + g as u8) as char;
    let legend: Vec<String> = goods
        .iter()
        .enumerate()
        .map(|(g, n)| format!("{}={n}", letter(g)))
        .collect();
    writeln!(
        f,
        "day {}: {}, H=house (h: agent-owned), upper case: town, lower: agent",
        w.day(),
        legend.join(" ")
    )?;
    let m = w.map().config();
    let (e, h) = (w.economy(), w.housing());
    for y in 0..m.height {
        let line: String = (0..m.width)
            .map(|x| match w.map().at(Pos::new(x, y)) {
                None => '.',
                Some(Building::House(id)) => {
                    if h.owner(id).is_some() {
                        'h'
                    } else {
                        'H'
                    }
                }
                Some(Building::Business(b)) => {
                    let c = letter(e.config(b).output as usize);
                    if e.owner(b).is_some() {
                        c
                    } else {
                        c.to_ascii_uppercase()
                    }
                }
            })
            .collect();
        writeln!(f, "{line}")?;
    }
    Ok(())
}

fn write_agents(w: &World, path: &Path) -> io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    let goods = &w.config().goods;
    let e = w.economy();
    writeln!(
        f,
        "agent,age,stage,job,level,owns,money,home,rent,hunger,fatigue,safety,comfort,action"
    )?;
    for a in 0..w.agent_count() {
        let job = w.employer(a).map(|b| e.config(b).output);
        let owns = w
            .owned_firm(a)
            .map(|b| goods[e.config(b).output as usize].as_str());
        let home = w.home(a);
        writeln!(
            f,
            "{a},{},{:?},{},{},{},{},{},{},{},{},{},{},{}",
            w.age_days(a) / YEAR,
            w.stage(a),
            job.map_or("-", |g| goods[g as usize].as_str()),
            job.map_or(0, |g| w.level(a, g)),
            owns.unwrap_or("-"),
            w.money(a),
            home.map_or("-".into(), |h| h.to_string()),
            home.map_or(0, |h| w.housing().rent(h)),
            w.hunger(a),
            w.fatigue(a),
            w.safety(a),
            w.comfort(a),
            w.action(a).kind.name(),
        )?;
    }
    Ok(())
}
