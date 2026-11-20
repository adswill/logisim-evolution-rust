//! Boolean expressions: construction, evaluation, simplification and printing.

use std::collections::BTreeSet;
use std::fmt;

/// A Boolean expression over named one-bit variables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Const(bool),
    Var(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    Xnor(Box<Expr>, Box<Expr>),
}

/// Operator kinds, ordered like the original's `Op` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Xnor,
    Or,
    Xor,
    And,
    Not,
}

/// Precedence levels shared by the parser and the printer (higher binds tighter).
pub mod prec {
    pub const NOT: i32 = 14;
    pub const IMPLICIT_AND: i32 = 13;
    pub const TIMES: i32 = 13;
    pub const OPLUS: i32 = 12;
    pub const PLUS: i32 = 11;
    pub const OTIMES: i32 = 10;
    pub const LOGIC: i32 = 9;
    pub const BITAND: i32 = 8;
    pub const BITOR: i32 = 6;
    pub const AND: i32 = 5;
    pub const OR: i32 = 4;
    pub const PYTHON_AND: i32 = 3;
    pub const PYTHON_XOR: i32 = 2;
    pub const PYTHON_OR: i32 = 1;
    pub const EQ: i32 = 0;
}

/// How expressions are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Notation {
    /// `a⋅b + ~c`
    #[default]
    Mathematical,
    /// `a ∧ b ∨ ¬c`
    Logic,
    /// `a ∧ b ∨ ~c`, with `≢` for XOR
    AltLogic,
    /// `a && b || !c`
    ProgBools,
    /// `a & b | ~c`
    ProgBits,
    /// `a \cdot b + \overline{c}` (output only)
    Latex,
}

impl Notation {
    pub const ALL: [Notation; 6] = [
        Notation::Mathematical,
        Notation::Logic,
        Notation::AltLogic,
        Notation::ProgBools,
        Notation::ProgBits,
        Notation::Latex,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Notation::Mathematical => "Mathematical (a⋅b + ~c)",
            Notation::Logic => "Logic (a ∧ b ∨ ¬c)",
            Notation::AltLogic => "Alternative logic (a ∧ b ∨ ~c)",
            Notation::ProgBools => "Programming, booleans (a && b || !c)",
            Notation::ProgBits => "Programming, bits (a & b | ~c)",
            Notation::Latex => "LaTeX",
        }
    }

    /// Precedence per operator: `[Xnor, Or, Xor, And, Not]`.
    fn levels(self) -> [i32; 5] {
        use prec::*;
        match self {
            Notation::Logic | Notation::AltLogic => [LOGIC, LOGIC, LOGIC, LOGIC, NOT],
            Notation::ProgBools => [LOGIC, OR, LOGIC, AND, NOT],
            // `^` is read at the same level as the mathematical ⊕, so print it that way too.
            Notation::ProgBits => [LOGIC, BITOR, OPLUS, BITAND, NOT],
            Notation::Latex | Notation::Mathematical => [OTIMES, PLUS, OPLUS, TIMES, NOT],
        }
    }

    fn symbols(self) -> [&'static str; 5] {
        match self {
            Notation::Logic => ["≡", "∨", "⊻", "∧", "¬"],
            Notation::AltLogic => ["≡", "∨", "≢", "∧", "~"],
            Notation::ProgBools => ["==", "||", "!=", "&&", "!"],
            Notation::ProgBits => ["^~", "|", "^", "&", "~"],
            Notation::Latex => [" \\oplus ", "+", " \\oplus ", " \\cdot ", "\\overline{"],
            Notation::Mathematical => ["⊙", "+", "⊕", "⋅", "~"],
        }
    }

    fn idx(op: Op) -> usize {
        match op {
            Op::Xnor => 0,
            Op::Or => 1,
            Op::Xor => 2,
            Op::And => 3,
            Op::Not => 4,
        }
    }

    pub fn precedence(self, op: Op) -> i32 {
        self.levels()[Self::idx(op)]
    }
    fn symbol(self, op: Op) -> &'static str {
        self.symbols()[Self::idx(op)]
    }
}

impl Expr {
    pub fn var(name: &str) -> Expr {
        Expr::Var(name.to_string())
    }
    #[allow(clippy::should_implement_trait)]
    pub fn not(e: Expr) -> Expr {
        Expr::Not(Box::new(e))
    }
    pub fn and(a: Expr, b: Expr) -> Expr {
        Expr::And(Box::new(a), Box::new(b))
    }
    pub fn or(a: Expr, b: Expr) -> Expr {
        Expr::Or(Box::new(a), Box::new(b))
    }
    pub fn xor(a: Expr, b: Expr) -> Expr {
        Expr::Xor(Box::new(a), Box::new(b))
    }
    pub fn xnor(a: Expr, b: Expr) -> Expr {
        Expr::Xnor(Box::new(a), Box::new(b))
    }

    /// Left-fold `items` with `f`; `None` for an empty list.
    pub fn fold(items: Vec<Expr>, f: fn(Expr, Expr) -> Expr) -> Option<Expr> {
        items.into_iter().reduce(f)
    }

    pub fn op(&self) -> Option<Op> {
        match self {
            Expr::Const(_) | Expr::Var(_) => None,
            Expr::Not(_) => Some(Op::Not),
            Expr::And(..) => Some(Op::And),
            Expr::Or(..) => Some(Op::Or),
            Expr::Xor(..) => Some(Op::Xor),
            Expr::Xnor(..) => Some(Op::Xnor),
        }
    }

    /// Value under `assign`, which gives each variable's value.
    pub fn eval(&self, assign: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Expr::Const(c) => *c,
            Expr::Var(n) => assign(n),
            Expr::Not(a) => !a.eval(assign),
            Expr::And(a, b) => a.eval(assign) && b.eval(assign),
            Expr::Or(a, b) => a.eval(assign) || b.eval(assign),
            Expr::Xor(a, b) => a.eval(assign) != b.eval(assign),
            Expr::Xnor(a, b) => a.eval(assign) == b.eval(assign),
        }
    }

    /// The variable names used, sorted.
    pub fn variables(&self) -> BTreeSet<String> {
        let mut s = BTreeSet::new();
        self.collect_vars(&mut s);
        s
    }
    fn collect_vars(&self, out: &mut BTreeSet<String>) {
        match self {
            Expr::Const(_) => {}
            Expr::Var(n) => {
                out.insert(n.clone());
            }
            Expr::Not(a) => a.collect_vars(out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Xor(a, b) | Expr::Xnor(a, b) => {
                a.collect_vars(out);
                b.collect_vars(out);
            }
        }
    }

    /// True if the variable occurs in the expression.
    pub fn uses(&self, name: &str) -> bool {
        match self {
            Expr::Const(_) => false,
            Expr::Var(n) => n == name,
            Expr::Not(a) => a.uses(name),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Xor(a, b) | Expr::Xnor(a, b) => {
                a.uses(name) || b.uses(name)
            }
        }
    }

    /// Number of operators and operands (a size measure).
    pub fn size(&self) -> usize {
        match self {
            Expr::Const(_) | Expr::Var(_) => 1,
            Expr::Not(a) => 1 + a.size(),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Xor(a, b) | Expr::Xnor(a, b) => {
                1 + a.size() + b.size()
            }
        }
    }

    /// Rename a variable throughout.
    pub fn rename(&self, old: &str, new: &str) -> Expr {
        self.map_vars(&|n| {
            if n == old {
                Expr::var(new)
            } else {
                Expr::var(n)
            }
        })
    }

    /// Replace every variable by `f(name)`.
    pub fn map_vars(&self, f: &dyn Fn(&str) -> Expr) -> Expr {
        match self {
            Expr::Const(c) => Expr::Const(*c),
            Expr::Var(n) => f(n),
            Expr::Not(a) => Expr::not(a.map_vars(f)),
            Expr::And(a, b) => Expr::and(a.map_vars(f), b.map_vars(f)),
            Expr::Or(a, b) => Expr::or(a.map_vars(f), b.map_vars(f)),
            Expr::Xor(a, b) => Expr::xor(a.map_vars(f), b.map_vars(f)),
            Expr::Xnor(a, b) => Expr::xnor(a.map_vars(f), b.map_vars(f)),
        }
    }

    /// Fold constants away and cancel double negations; the result computes the same function.
    pub fn simplify(&self) -> Expr {
        use Expr::*;
        match self {
            Const(_) | Var(_) => self.clone(),
            Not(a) => match a.simplify() {
                Const(c) => Const(!c),
                Not(inner) => *inner,
                other => Expr::not(other),
            },
            And(a, b) => match (a.simplify(), b.simplify()) {
                (Const(false), _) | (_, Const(false)) => Const(false),
                (Const(true), x) | (x, Const(true)) => x,
                (x, y) => Expr::and(x, y),
            },
            Or(a, b) => match (a.simplify(), b.simplify()) {
                (Const(true), _) | (_, Const(true)) => Const(true),
                (Const(false), x) | (x, Const(false)) => x,
                (x, y) => Expr::or(x, y),
            },
            Xor(a, b) => match (a.simplify(), b.simplify()) {
                (Const(x), Const(y)) => Const(x != y),
                (Const(false), x) | (x, Const(false)) => x,
                (Const(true), x) | (x, Const(true)) => Expr::not(x).simplify(),
                (x, y) => Expr::xor(x, y),
            },
            Xnor(a, b) => match (a.simplify(), b.simplify()) {
                (Const(x), Const(y)) => Const(x == y),
                (Const(true), x) | (x, Const(true)) => x,
                (Const(false), x) | (x, Const(false)) => Expr::not(x).simplify(),
                (x, y) => Expr::xnor(x, y),
            },
        }
    }

    /// True when this is a sum of products (OR of ANDs of literals).
    pub fn is_sop(&self) -> bool {
        fn lit(e: &Expr) -> bool {
            match e {
                Expr::Var(_) | Expr::Const(_) => true,
                Expr::Not(a) => matches!(**a, Expr::Var(_)),
                _ => false,
            }
        }
        fn product(e: &Expr) -> bool {
            match e {
                Expr::And(a, b) => product(a) && product(b),
                _ => lit(e),
            }
        }
        match self {
            Expr::Or(a, b) => a.is_sop() && b.is_sop(),
            _ => product(self),
        }
    }

    /// True when this is a product of sums (AND of ORs of literals).
    pub fn is_pos(&self) -> bool {
        fn lit(e: &Expr) -> bool {
            match e {
                Expr::Var(_) | Expr::Const(_) => true,
                Expr::Not(a) => matches!(**a, Expr::Var(_)),
                _ => false,
            }
        }
        fn sum(e: &Expr) -> bool {
            match e {
                Expr::Or(a, b) => sum(a) && sum(b),
                _ => lit(e),
            }
        }
        match self {
            Expr::And(a, b) => a.is_pos() && b.is_pos(),
            _ => sum(self),
        }
    }

    /// Write the expression in the given notation.
    pub fn to_text(&self, n: Notation) -> String {
        let mut s = String::new();
        self.write(n, &mut s);
        s
    }

    fn write(&self, n: Notation, out: &mut String) {
        match self {
            Expr::Const(c) => out.push(if *c { '1' } else { '0' }),
            Expr::Var(name) => {
                if n == Notation::Latex {
                    out.push_str(&latex_name(name));
                } else {
                    out.push_str(name);
                }
            }
            Expr::Not(a) => {
                out.push_str(n.symbol(Op::Not));
                if n == Notation::Latex {
                    a.write(n, out);
                    out.push('}');
                } else if matches!(**a, Expr::Var(_) | Expr::Const(_) | Expr::Not(_)) {
                    a.write(n, out);
                } else {
                    out.push('(');
                    a.write(n, out);
                    out.push(')');
                }
            }
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Xor(a, b) | Expr::Xnor(a, b) => {
                let op = self.op().unwrap();
                let sym = n.symbol(op);
                let spaced = !matches!(op, Op::And) || n != Notation::Mathematical;
                let pad = if spaced && !sym.starts_with(' ') {
                    " "
                } else {
                    ""
                };
                // operands: same operator chains without parentheses (all of these associate)
                for (i, side) in [a, b].into_iter().enumerate() {
                    if i == 1 {
                        out.push_str(pad);
                        out.push_str(sym);
                        out.push_str(pad);
                    }
                    let child_lvl = side.op().map(|o| n.precedence(o));
                    let own = n.precedence(op);
                    let paren = match child_lvl {
                        None => false,
                        Some(l) if side.op() == Some(Op::Not) => l < own,
                        Some(l) => {
                            l < own
                                || (l == own
                                    && !(side.op() == Some(op) && associative(op) && i == 0))
                        }
                    };
                    if paren {
                        out.push('(');
                        side.write(n, out);
                        out.push(')');
                    } else {
                        side.write(n, out);
                    }
                }
            }
        }
    }
}

fn associative(op: Op) -> bool {
    matches!(op, Op::And | Op::Or | Op::Xor)
}

fn latex_name(name: &str) -> String {
    // `A[2]` becomes `A_{2}`; other characters that LaTeX treats specially are escaped.
    let mut out = String::new();
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' => out.push_str("_{"),
            ']' => out.push('}'),
            '_' | '$' | '&' | '%' | '#' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        let _ = chars.peek();
    }
    out
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text(Notation::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Expr::*;

    fn a() -> Expr {
        Expr::var("a")
    }
    fn b() -> Expr {
        Expr::var("b")
    }
    fn c() -> Expr {
        Expr::var("c")
    }

    fn truth(e: &Expr, names: &[&str]) -> Vec<bool> {
        (0..1u32 << names.len())
            .map(|row| {
                e.eval(&|n| {
                    let i = names.iter().position(|x| *x == n).unwrap();
                    row >> (names.len() - 1 - i) & 1 == 1
                })
            })
            .collect()
    }

    #[test]
    fn evaluates_every_operator() {
        let t = |e: Expr| truth(&e, &["a", "b"]);
        assert_eq!(t(Expr::and(a(), b())), [false, false, false, true]);
        assert_eq!(t(Expr::or(a(), b())), [false, true, true, true]);
        assert_eq!(t(Expr::xor(a(), b())), [false, true, true, false]);
        assert_eq!(t(Expr::xnor(a(), b())), [true, false, false, true]);
        assert_eq!(t(Expr::not(a())), [true, true, false, false]);
    }

    #[test]
    fn variables_and_size() {
        let e = Expr::or(Expr::and(a(), Expr::not(b())), c());
        assert_eq!(
            e.variables().into_iter().collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert!(e.uses("b") && !e.uses("d"));
        assert_eq!(e.size(), 6);
        assert_eq!(
            e.rename("b", "z").to_text(Notation::Mathematical),
            "a⋅~z + c"
        );
    }

    #[test]
    fn prints_in_every_notation() {
        let e = Expr::or(Expr::and(a(), Expr::not(b())), Expr::xor(b(), c()));
        let want = [
            (Notation::Mathematical, "a⋅~b + b ⊕ c"),
            (Notation::Logic, "(a ∧ ¬b) ∨ (b ⊻ c)"),
            (Notation::AltLogic, "(a ∧ ~b) ∨ (b ≢ c)"),
            (Notation::ProgBools, "a && !b || b != c"),
            (Notation::ProgBits, "a & ~b | b ^ c"),
            (Notation::Latex, "a \\cdot \\overline{b} + b \\oplus c"),
        ];
        for (n, text) in want {
            assert_eq!(e.to_text(n), text, "{n:?}");
        }
    }

    #[test]
    fn parentheses_follow_precedence() {
        let m = Notation::Mathematical;
        assert_eq!(Expr::and(Expr::or(a(), b()), c()).to_text(m), "(a + b)⋅c");
        assert_eq!(Expr::and(a(), Expr::and(b(), c())).to_text(m), "a⋅(b⋅c)");
        assert_eq!(Expr::and(Expr::and(a(), b()), c()).to_text(m), "a⋅b⋅c");
        assert_eq!(Expr::not(Expr::or(a(), b())).to_text(m), "~(a + b)");
        assert_eq!(Expr::not(Expr::not(a())).to_text(m), "~~a");
        assert_eq!(
            Expr::xnor(Expr::xnor(a(), b()), c()).to_text(m),
            "(a ⊙ b) ⊙ c"
        );
        assert_eq!(
            Expr::and(Expr::or(a(), b()), Expr::or(a(), c())).to_text(Notation::Logic),
            "(a ∨ b) ∧ (a ∨ c)"
        );
    }

    #[test]
    fn simplify_folds_constants() {
        let t = Const(true);
        let f = Const(false);
        assert_eq!(Expr::and(a(), f.clone()).simplify(), f);
        assert_eq!(Expr::and(a(), t.clone()).simplify(), a());
        assert_eq!(Expr::or(a(), t.clone()).simplify(), t);
        assert_eq!(Expr::or(f.clone(), b()).simplify(), b());
        assert_eq!(Expr::xor(a(), t.clone()).simplify(), Expr::not(a()));
        assert_eq!(Expr::xnor(a(), f.clone()).simplify(), Expr::not(a()));
        assert_eq!(Expr::xnor(a(), t.clone()).simplify(), a());
        assert_eq!(Expr::not(Expr::not(a())).simplify(), a());
        assert_eq!(Expr::not(Expr::and(t.clone(), f)).simplify(), t);
    }

    #[test]
    fn simplify_keeps_the_function() {
        let names = ["a", "b"];
        let consts = [Const(false), Const(true), a(), b()];
        for x in &consts {
            for y in &consts {
                for e in [
                    Expr::and(x.clone(), y.clone()),
                    Expr::or(x.clone(), y.clone()),
                    Expr::xor(x.clone(), y.clone()),
                    Expr::xnor(x.clone(), y.clone()),
                    Expr::not(Expr::xor(x.clone(), y.clone())),
                ] {
                    assert_eq!(truth(&e, &names), truth(&e.simplify(), &names), "{e:?}");
                }
            }
        }
    }

    #[test]
    fn recognises_normal_forms() {
        let sop = Expr::or(Expr::and(a(), Expr::not(b())), c());
        assert!(sop.is_sop() && !sop.is_pos());
        let pos = Expr::and(Expr::or(a(), b()), Expr::or(Expr::not(a()), c()));
        assert!(pos.is_pos() && !pos.is_sop());
        assert!(a().is_sop() && a().is_pos());
        assert!(!Expr::xor(a(), b()).is_sop());
        assert!(!Expr::not(Expr::and(a(), b())).is_sop());
    }

    #[test]
    fn latex_names_are_escaped() {
        assert_eq!(Expr::var("D[2]").to_text(Notation::Latex), "D_{2}");
        assert_eq!(Expr::var("my_x").to_text(Notation::Latex), "my\\_x");
    }
}
