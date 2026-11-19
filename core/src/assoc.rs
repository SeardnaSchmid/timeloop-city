//! Associations: guilds and the town council. They have members, a
//! treasury and rules; once a week they collect dues, pay out their
//! treasury and decide their rules anew.
//!
//! Rules do not add logic of their own, they change what actions below cost
//! or allow: a minimum or maximum price for sellers, a fee for founding a
//! firm, dues on sales revenue, a tax on every meal bought.
//!
//! A guild covers some goods; its members are the open businesses making
//! them. The council covers every good; its members are all people.

use crate::economy::GoodId;
use crate::formula::Formula;

/// Associations meet and decide every this many days.
pub const WEEK_DAYS: u64 = 7;

/// Variables rule formulas can read. `market.*` describe the
/// association's first good (the food for the council), as founders see it.
pub const ASSOC_VARS: [&str; 7] = [
    "market.price",
    "market.cost",
    "market.shortage",
    "market.firms",
    "assoc.cash",
    "assoc.members",
    "day",
];

pub(crate) mod avar {
    pub const MARKET: usize = 0;
    pub const CASH: usize = 4;
    pub const MEMBERS: usize = 5;
    pub const DAY: usize = 6;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    /// Sellers of the covered goods may not ask less.
    MinPrice,
    /// Sellers of the covered goods may not ask more.
    MaxPrice,
    /// Paid to the association by whoever founds a firm making a covered good.
    FoundingFee,
    /// Percent of members' sales revenue, paid weekly.
    DuesPercent,
    /// Percent on top of the price of every meal of a covered good.
    SalesTaxPercent,
}

impl RuleKind {
    pub const ALL: [RuleKind; 5] = [
        RuleKind::MinPrice,
        RuleKind::MaxPrice,
        RuleKind::FoundingFee,
        RuleKind::DuesPercent,
        RuleKind::SalesTaxPercent,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RuleKind::MinPrice => "min_price",
            RuleKind::MaxPrice => "max_price",
            RuleKind::FoundingFee => "founding_fee",
            RuleKind::DuesPercent => "dues_percent",
            RuleKind::SalesTaxPercent => "sales_tax_percent",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub kind: RuleKind,
    /// Evaluated weekly; negative values count as 0.
    pub value: Formula,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssocKind {
    /// Covers these goods; members are the businesses making them. Its
    /// treasury goes back to them in equal shares every week.
    Guild { goods: Vec<GoodId> },
    /// Covers every good; members are all people. Its treasury joins the
    /// town's welfare every week.
    Council,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssociationConfig {
    pub name: String,
    pub kind: AssocKind,
    /// At most one rule per kind.
    pub rules: Vec<Rule>,
}

impl AssociationConfig {
    pub fn covers(&self, good: GoodId) -> bool {
        match &self.kind {
            AssocKind::Guild { goods } => goods.contains(&good),
            AssocKind::Council => true,
        }
    }

    pub fn rule_index(&self, kind: RuleKind) -> Option<usize> {
        self.rules.iter().position(|r| r.kind == kind)
    }
}
