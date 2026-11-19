//! Small integer formula language for behaviour data.
//!
//! Formulas are compiled once into postfix code and evaluated with a fixed
//! stack: no allocation, no floats, same result on every platform.
//!
//! Grammar, loosest binding first:
//!
//! ```text
//! a || b    a && b    == != < <= > >=    + -    * / %    -a !a
//! 42  need.hunger  min(a, b)  max(a, b)  clamp(x, lo, hi)  if(c, a, b)  (a)
//! ```
//!
//! Comparisons and logic yield 0 or 1; any non-zero value is true.
//! Arithmetic saturates; division and remainder by zero yield 0.

use std::fmt;

/// Deepest evaluation stack a formula may need.
const MAX_STACK: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Const(i64),
    Var(u16),
    Neg,
    Not,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Min,
    Max,
    Clamp,
    If,
}

impl Op {
    /// Values taken from the stack.
    fn arity(self) -> usize {
        match self {
            Op::Const(_) | Op::Var(_) => 0,
            Op::Neg | Op::Not => 1,
            Op::Clamp | Op::If => 3,
            _ => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formula {
    source: String,
    code: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaError {
    /// Byte offset in the source.
    pub at: usize,
    pub message: String,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at column {})", self.message, self.at + 1)
    }
}

impl std::error::Error for FormulaError {}

impl Formula {
    /// Compiles `source`; variable names resolve to their index in `vars`.
    pub fn compile(source: &str, vars: &[&str]) -> Result<Self, FormulaError> {
        let tokens = lex(source)?;
        let mut p = Parser {
            tokens: &tokens,
            pos: 0,
            vars,
            code: Vec::new(),
            end: source.len(),
        };
        p.expr(0)?;
        if let Some(t) = p.tokens.get(p.pos) {
            return Err(err(t.at, format!("unexpected {}", t.kind)));
        }
        let code = p.code;
        let mut depth = 0usize;
        for op in &code {
            depth = depth - op.arity() + 1;
            if depth > MAX_STACK {
                return Err(err(0, "formula is too deeply nested"));
            }
        }
        Ok(Self {
            source: source.to_string(),
            code,
        })
    }

    /// A formula that always yields `value`.
    pub fn constant(value: i64) -> Self {
        Self {
            source: value.to_string(),
            code: vec![Op::Const(value)],
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Evaluates with `vars[i]` as the value of variable `i`.
    pub fn eval(&self, vars: &[i64]) -> i64 {
        let mut stack = [0i64; MAX_STACK];
        let mut n = 0usize;
        for &op in &self.code {
            let k = op.arity();
            let base = n - k;
            let a = |i: usize| stack[base + i];
            let v = match op {
                Op::Const(c) => c,
                Op::Var(i) => vars[i as usize],
                Op::Neg => a(0).saturating_neg(),
                Op::Not => (a(0) == 0) as i64,
                Op::Add => a(0).saturating_add(a(1)),
                Op::Sub => a(0).saturating_sub(a(1)),
                Op::Mul => a(0).saturating_mul(a(1)),
                Op::Div => match a(1) {
                    0 => 0,
                    d => a(0).checked_div(d).unwrap_or(i64::MAX),
                },
                Op::Rem => a(0).checked_rem(a(1)).unwrap_or(0),
                Op::Lt => (a(0) < a(1)) as i64,
                Op::Le => (a(0) <= a(1)) as i64,
                Op::Gt => (a(0) > a(1)) as i64,
                Op::Ge => (a(0) >= a(1)) as i64,
                Op::Eq => (a(0) == a(1)) as i64,
                Op::Ne => (a(0) != a(1)) as i64,
                Op::And => (a(0) != 0 && a(1) != 0) as i64,
                Op::Or => (a(0) != 0 || a(1) != 0) as i64,
                Op::Min => a(0).min(a(1)),
                Op::Max => a(0).max(a(1)),
                Op::Clamp => a(0).max(a(1)).min(a(2)),
                Op::If => {
                    if a(0) != 0 {
                        a(1)
                    } else {
                        a(2)
                    }
                }
            };
            stack[base] = v;
            n = base + 1;
        }
        stack[0]
    }
}

fn err(at: usize, message: impl Into<String>) -> FormulaError {
    FormulaError {
        at,
        message: message.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Num(i64),
    Ident(String),
    Sym(&'static str),
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Num(n) => write!(f, "number {n}"),
            Kind::Ident(s) => write!(f, "\"{s}\""),
            Kind::Sym(s) => write!(f, "\"{s}\""),
        }
    }
}

struct Token {
    at: usize,
    kind: Kind,
}

const SYMBOLS: [&str; 19] = [
    "&&", "||", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "!", "(", ")", ",", "=",
    "&",
];

fn lex(src: &str) -> Result<Vec<Token>, FormulaError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            let text: String = src[start..i].chars().filter(|&c| c != '_').collect();
            let n = text
                .parse()
                .map_err(|_| err(start, format!("number too large: {text}")))?;
            out.push(Token {
                at: start,
                kind: Kind::Num(n),
            });
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push(Token {
                at: start,
                kind: Kind::Ident(src[start..i].to_string()),
            });
        } else {
            let sym = SYMBOLS
                .iter()
                .find(|s| src[i..].starts_with(**s))
                .ok_or_else(|| err(i, format!("unexpected character {:?}", c as char)))?;
            if *sym == "=" || *sym == "&" {
                return Err(err(i, format!("unknown operator \"{sym}\"")));
            }
            out.push(Token {
                at: i,
                kind: Kind::Sym(sym),
            });
            i += sym.len();
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    vars: &'a [&'a str],
    code: Vec<Op>,
    end: usize,
}

/// Binary operators by binding strength, loosest first.
const LEVELS: [&[(&str, Op)]; 5] = [
    &[("||", Op::Or)],
    &[("&&", Op::And)],
    &[
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("<", Op::Lt),
        (">", Op::Gt),
    ],
    &[("+", Op::Add), ("-", Op::Sub)],
    &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
];

impl Parser<'_> {
    fn peek_sym(&self) -> Option<&'static str> {
        match self.tokens.get(self.pos) {
            Some(Token {
                kind: Kind::Sym(s), ..
            }) => Some(s),
            _ => None,
        }
    }

    fn here(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.end, |t| t.at)
    }

    fn expect(&mut self, sym: &str) -> Result<(), FormulaError> {
        if self.peek_sym() == Some(sym) {
            self.pos += 1;
            Ok(())
        } else {
            Err(err(self.here(), format!("expected \"{sym}\"")))
        }
    }

    fn expr(&mut self, level: usize) -> Result<(), FormulaError> {
        if level == LEVELS.len() {
            return self.unary();
        }
        self.expr(level + 1)?;
        while let Some(op) = self
            .peek_sym()
            .and_then(|s| LEVELS[level].iter().find(|(t, _)| *t == s))
            .map(|&(_, op)| op)
        {
            self.pos += 1;
            self.expr(level + 1)?;
            self.code.push(op);
        }
        Ok(())
    }

    fn unary(&mut self) -> Result<(), FormulaError> {
        match self.peek_sym() {
            Some("-") => {
                self.pos += 1;
                self.unary()?;
                self.code.push(Op::Neg);
            }
            Some("!") => {
                self.pos += 1;
                self.unary()?;
                self.code.push(Op::Not);
            }
            _ => self.primary()?,
        }
        Ok(())
    }

    fn primary(&mut self) -> Result<(), FormulaError> {
        let Some(t) = self.tokens.get(self.pos) else {
            return Err(err(self.end, "formula ends too early"));
        };
        let at = t.at;
        self.pos += 1;
        match &t.kind {
            Kind::Num(n) => self.code.push(Op::Const(*n)),
            Kind::Sym("(") => {
                self.expr(0)?;
                self.expect(")")?;
            }
            Kind::Ident(name) if self.peek_sym() == Some("(") => {
                let (op, args) = match name.as_str() {
                    "min" => (Op::Min, 2),
                    "max" => (Op::Max, 2),
                    "clamp" => (Op::Clamp, 3),
                    "if" => (Op::If, 3),
                    _ => return Err(err(at, format!("unknown function \"{name}\""))),
                };
                self.pos += 1;
                for i in 0..args {
                    if i > 0 {
                        self.expect(",")?;
                    }
                    self.expr(0)?;
                }
                self.expect(")")?;
                self.code.push(op);
            }
            Kind::Ident(name) => {
                let i = self
                    .vars
                    .iter()
                    .position(|v| v == name)
                    .ok_or_else(|| err(at, format!("unknown variable \"{name}\"")))?;
                self.code.push(Op::Var(i as u16));
            }
            other => return Err(err(at, format!("unexpected {other}"))),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARS: [&str; 3] = ["need.hunger", "trait.greed", "money"];

    fn eval(src: &str) -> i64 {
        Formula::compile(src, &VARS).unwrap().eval(&[300, 800, -5])
    }

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(eval("1 + 2 * 3"), 7);
        assert_eq!(eval("(1 + 2) * 3"), 9);
        assert_eq!(eval("10 - 3 - 2"), 5);
        assert_eq!(eval("100 / 10 / 5"), 2);
        assert_eq!(eval("-2 * -3"), 6);
        assert_eq!(eval("7 % 4 + 1"), 4);
    }

    #[test]
    fn variables() {
        assert_eq!(eval("need.hunger * trait.greed / 1000"), 240);
        assert_eq!(eval("money"), -5);
    }

    #[test]
    fn logic_and_comparison() {
        assert_eq!(eval("need.hunger >= 300 && money < 0"), 1);
        assert_eq!(eval("need.hunger > 300 || !money"), 0);
        assert_eq!(eval("1 + 1 == 2"), 1);
        assert_eq!(eval("!0 != 0"), 1);
    }

    #[test]
    fn functions() {
        assert_eq!(eval("min(3, 5) + max(3, 5)"), 8);
        assert_eq!(eval("clamp(money, 0, 10)"), 0);
        assert_eq!(eval("clamp(need.hunger, 0, 10)"), 10);
        assert_eq!(eval("if(money < 0, 1, 2)"), 1);
    }

    #[test]
    fn edge_cases_do_not_panic() {
        assert_eq!(eval("5 / 0"), 0);
        assert_eq!(eval("5 % 0"), 0);
        assert_eq!(eval("9223372036854775807 + 1"), i64::MAX);
        assert_eq!(eval("-9223372036854775807 - 2"), i64::MIN);
        assert_eq!(eval("1_000 * 2"), 2000);
    }

    #[test]
    fn errors_point_at_the_problem() {
        let e = |src: &str| Formula::compile(src, &VARS).unwrap_err();
        assert_eq!(e("trait.gred + 1").at, 0);
        assert!(e("trait.gred").message.contains("trait.gred"));
        assert_eq!(e("1 +").at, 3);
        assert_eq!(e("(1 + 2").at, 6);
        assert_eq!(e("1 2").at, 2);
        assert!(e("a = 1").message.contains("unknown"));
        assert!(e("pow(1, 2)").message.contains("pow"));
        assert!(e("1 $ 2").message.contains('$'));
    }

    #[test]
    fn too_deep_is_rejected() {
        let deep = "(".repeat(40) + "1" + &" + 1)".repeat(40);
        assert_eq!(Formula::compile(&deep, &VARS).unwrap().eval(&[0; 3]), 41);
        let wide = (0..40).map(|_| "(1+").collect::<String>() + "1" + &")".repeat(40);
        assert!(Formula::compile(&wide, &VARS).is_err());
    }
}
