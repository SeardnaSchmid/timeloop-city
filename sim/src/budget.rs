//! Work counters per run and the hard budgets CI holds them to.
//!
//! The counters are deterministic (same seed, same counts on every machine),
//! so unlike wall-clock time they can gate a build. Each budget is the value
//! measured for the CI run (`--seed 1 --days 3650`) plus headroom; raise one
//! only when the extra work is intended.

use std::io::{self, Write};

use timeloop_core::Work;

/// Totals over a run, with the agent-days and the final town size.
#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub days: u64,
    /// Sum of the agent count at the end of every day.
    pub agent_days: u64,
    pub agents: u32,
    pub firms: u32,
    pub work: Work,
    /// Wall-clock seconds of the run: machine-dependent, shown, never gated.
    pub seconds: f64,
}

impl Totals {
    fn per_agent_day(&self, n: u64) -> f64 {
        n as f64 / self.agent_days.max(1) as f64
    }

    /// `(name, measured, limit)` for every budget.
    pub fn checks(&self) -> Vec<(&'static str, f64, f64)> {
        let w = &self.work;
        // Measured 2026-11-20 (100 agents, 10 years): 15.73 events and
        // 15.72 decisions per agent-day, 5 options per decision, at most 11
        // houses per home choice, 133 formula evals per agent-day.
        vec![
            ("events per agent-day", self.per_agent_day(w.events), 18.0),
            (
                "decisions per agent-day",
                self.per_agent_day(w.decisions),
                18.0,
            ),
            ("options per decision (max)", w.max_options as f64, 10.0),
            (
                "home options per choice (max)",
                w.max_home_options as f64,
                16.0,
            ),
            (
                "formula evals per agent-day",
                self.per_agent_day(w.formula_evals),
                160.0,
            ),
        ]
    }

    pub fn csv_header() -> &'static str {
        "day,agents,firms,events,decisions,options,max_options,home_choices,home_options,max_home_options,formula_evals"
    }

    /// One CSV row with cumulative counters.
    pub fn csv_row(&self) -> String {
        let w = &self.work;
        format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            self.days,
            self.agents,
            self.firms,
            w.events,
            w.decisions,
            w.options,
            w.max_options,
            w.home_choices,
            w.home_options,
            w.max_home_options,
            w.formula_evals
        )
    }

    /// Writes the same as [`Self::report`] as Markdown tables (for the CI
    /// job summary); returns whether every budget holds.
    pub fn markdown(&self, out: &mut impl Write) -> io::Result<bool> {
        let w = &self.work;
        let days_per_s = self.days as f64 / self.seconds.max(1e-9);
        let rows = [
            ("wall time", format!("{:.2} s", self.seconds)),
            ("sim days per second", format!("{days_per_s:.0}")),
            (
                "seconds per 66-year life",
                format!("{:.1}", (66 * 365) as f64 / days_per_s),
            ),
            ("sim days", self.days.to_string()),
            ("agents at the end", self.agents.to_string()),
            ("firms at the end", self.firms.to_string()),
            ("events", w.events.to_string()),
            ("decisions", w.decisions.to_string()),
            ("options scored", w.options.to_string()),
            ("home choices", w.home_choices.to_string()),
            ("formula evals", w.formula_evals.to_string()),
        ];
        writeln!(out, "| measure | value |\n|---|---:|")?;
        for (name, value) in rows {
            writeln!(out, "| {name} | {value} |")?;
        }
        writeln!(
            out,
            "\n| budget | measured | limit | |\n|---|---:|---:|---|"
        )?;
        let mut ok = true;
        for (name, value, limit) in self.checks() {
            let pass = value <= limit;
            ok &= pass;
            let mark = if pass { "ok" } else { "**OVER**" };
            writeln!(out, "| {name} | {value:.2} | {limit} | {mark} |")?;
        }
        writeln!(out)?;
        Ok(ok)
    }

    /// Writes the budget table; returns whether every budget holds.
    pub fn report(&self, out: &mut impl Write) -> io::Result<bool> {
        let w = &self.work;
        writeln!(
            out,
            "work: {} days, {} agents, {} firms at the end, {} agent-days",
            self.days, self.agents, self.firms, self.agent_days
        )?;
        writeln!(
            out,
            "work: {} events, {} decisions, {} options, {} home choices, {} formula evals",
            w.events, w.decisions, w.options, w.home_choices, w.formula_evals
        )?;
        writeln!(
            out,
            "work: {:.2} options per decision, {:.2} home options per choice",
            w.options as f64 / w.decisions.max(1) as f64,
            w.home_options as f64 / w.home_choices.max(1) as f64
        )?;
        let days_per_s = self.days as f64 / self.seconds.max(1e-9);
        writeln!(
            out,
            "time: {:.2} s, {:.0} days/s, {:.1} s per 66-year life",
            self.seconds,
            days_per_s,
            (66 * 365) as f64 / days_per_s
        )?;
        let mut ok = true;
        for (name, value, limit) in self.checks() {
            let pass = value <= limit;
            ok &= pass;
            let mark = if pass { "ok  " } else { "OVER" };
            writeln!(out, "budget {mark} {name:<30} {value:>9.2} <= {limit}")?;
        }
        Ok(ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_budget_fails() {
        let mut t = Totals {
            days: 1,
            agent_days: 10,
            ..Totals::default()
        };
        t.work.decisions = 100;
        assert!(t.report(&mut Vec::new()).unwrap());
        t.work.decisions = 1000;
        let mut out = Vec::new();
        assert!(!t.report(&mut out).unwrap());
        assert!(String::from_utf8(out).unwrap().contains("OVER"));
    }
}
