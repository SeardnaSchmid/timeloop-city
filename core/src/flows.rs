//! Money flows and why firms close: running totals for `periods.csv`.
//! Watching only; nothing reads them back, so they stay out of the hash.

/// Why a business closed. The close rule says *that* a firm closes; the
/// reason is read off its state at that moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// The owner died without heirs.
    OwnerDead,
    /// Its input ran out: the input shelf is empty.
    NoInput,
    /// Goods on the shelf that nobody bought.
    NoDemand,
    /// Neither: it simply earned less than it paid out.
    Broke,
}

pub const CLOSE_REASONS: [&str; 4] = ["owner_dead", "no_input", "no_demand", "broke"];

impl CloseReason {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Money moved since the world began, by kind. All amounts are positive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flows {
    /// Wages paid by businesses, before the pension contribution.
    pub wages: i64,
    /// Pension contributions taken off wages.
    pub pension_in: i64,
    /// Pensions paid to those in education or retired.
    pub pensions: i64,
    /// Rent paid by tenants.
    pub rent: i64,
    /// Firm surplus paid to owners, after the profit tax.
    pub dividends: i64,
    /// Rent income paid to house owners, after the profit tax.
    pub house_income: i64,
    /// Profit tax on dividends and house income.
    pub tax_profit: i64,
    /// Sales tax on goods bought.
    pub tax_sales: i64,
    /// Inheritance tax.
    pub tax_inheritance: i64,
    /// Dues businesses paid to associations.
    pub dues: i64,
    /// Founding fees to associations.
    pub fees: i64,
    /// Comfort spending that goes to the town (no comfort goods).
    pub comfort_to_town: i64,
    /// Town welfare paid to agents: meal top-ups and equal shares.
    pub welfare: i64,
    /// Poor-relief levy on savings above the safety reserve.
    pub levy: i64,
    /// Town cash used to top up its own businesses.
    pub town_backing: i64,
    /// Estates passed to heirs (after tax).
    pub inheritance: i64,
    /// Estates without heirs that went to the town.
    pub escheat: i64,
    /// Founding capital agents put into new firms.
    pub capital: i64,
    /// Founding capital town businesses put into branches.
    pub branch_capital: i64,
    /// Houses agents paid for.
    pub building: i64,
    /// Cash a closing business handed back (to owner or town).
    pub closing_cash: i64,
    /// Businesses closed, by [`CloseReason`].
    pub closed: [u64; 4],
}

impl Flows {
    /// Column names and values, in a fixed order, for CSV.
    pub fn columns(&self) -> Vec<(&'static str, i64)> {
        let mut v = vec![
            ("wages", self.wages),
            ("pension_in", self.pension_in),
            ("pensions", self.pensions),
            ("rent", self.rent),
            ("dividends", self.dividends),
            ("house_income", self.house_income),
            ("tax_profit", self.tax_profit),
            ("tax_sales", self.tax_sales),
            ("tax_inheritance", self.tax_inheritance),
            ("dues", self.dues),
            ("fees", self.fees),
            ("comfort_to_town", self.comfort_to_town),
            ("welfare", self.welfare),
            ("levy", self.levy),
            ("town_backing", self.town_backing),
            ("inheritance", self.inheritance),
            ("escheat", self.escheat),
            ("capital", self.capital),
            ("branch_capital", self.branch_capital),
            ("building", self.building),
            ("closing_cash", self.closing_cash),
        ];
        for (i, r) in CLOSE_REASONS.iter().enumerate() {
            v.push((CLOSED_COLUMNS[i], self.closed[i] as i64));
            debug_assert!(CLOSED_COLUMNS[i].ends_with(r));
        }
        v
    }

    /// `self - earlier`, column by column: the flows of one period.
    pub fn since(&self, earlier: &Flows) -> Vec<(&'static str, i64)> {
        self.columns()
            .into_iter()
            .zip(earlier.columns())
            .map(|((n, a), (_, b))| (n, a - b))
            .collect()
    }
}

const CLOSED_COLUMNS: [&str; 4] = [
    "closed_owner_dead",
    "closed_no_input",
    "closed_no_demand",
    "closed_broke",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_subtracts_per_column() {
        let a = Flows {
            wages: 10,
            closed: [1, 0, 0, 2],
            ..Flows::default()
        };
        let b = Flows {
            wages: 25,
            closed: [1, 1, 0, 5],
            ..a
        };
        let d = b.since(&a);
        assert_eq!(d[0], ("wages", 15));
        assert_eq!(d.iter().find(|c| c.0 == "closed_broke").unwrap().1, 3);
        assert_eq!(d.iter().find(|c| c.0 == "closed_no_input").unwrap().1, 1);
    }
}
