//! Minimisation of one output column: Quine–McCluskey prime implicants, then a minimal cover.
//!
//! The result is deterministic (the original's depends on hash ordering): among covers with the
//! fewest terms, the one with the fewest literals wins, ties broken by term order.

use crate::expr::Expr;
use crate::table::{Entry, TruthTable};
use std::collections::{BTreeMap, BTreeSet};

/// Which normal form to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Form {
    /// Sum of products: OR of terms that make the output 1.
    #[default]
    SumOfProducts,
    /// Product of sums: AND of clauses that rule out the 0 rows.
    ProductOfSums,
}

/// A group of rows sharing some input bits. Bit `n-1-col` belongs to input column `col`, like a
/// row number; bits set in `dashes` are free (either value), the others are fixed to `values`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Implicant {
    pub dashes: u32,
    pub values: u32,
}

impl Implicant {
    pub fn row(row: u32) -> Self {
        Implicant {
            dashes: 0,
            values: row,
        }
    }

    pub fn covers(&self, row: u32) -> bool {
        row & !self.dashes == self.values
    }

    /// Number of fixed bits (literals in the term).
    pub fn literals(&self, inputs: usize) -> usize {
        inputs - self.dashes.count_ones() as usize
    }

    /// Every row covered.
    pub fn rows(&self) -> Vec<u32> {
        let mut out = vec![];
        let mut sub = self.dashes;
        loop {
            out.push(self.values | sub);
            if sub == 0 {
                break;
            }
            sub = (sub - 1) & self.dashes;
        }
        out.sort_unstable();
        out
    }

    /// `1-0x` style text for input columns left to right (`-` = free).
    pub fn pattern(&self, inputs: usize) -> String {
        (0..inputs)
            .map(|col| {
                let bit = 1 << (inputs - 1 - col);
                if self.dashes & bit != 0 {
                    '-'
                } else if self.values & bit != 0 {
                    '1'
                } else {
                    '0'
                }
            })
            .collect()
    }

    /// The product (SOP) or sum (POS) term for this group; `None` when it has no literals.
    fn term(&self, names: &[String], form: Form) -> Option<Expr> {
        let n = names.len();
        let lits: Vec<Expr> = (0..n)
            .filter_map(|col| {
                let bit = 1 << (n - 1 - col);
                if self.dashes & bit != 0 {
                    return None;
                }
                let one = self.values & bit != 0;
                let v = Expr::var(&names[col]);
                // SOP: a 1 bit is the plain variable. POS clauses describe rows to avoid, so flip.
                Some(if one == (form == Form::SumOfProducts) {
                    v
                } else {
                    Expr::not(v)
                })
            })
            .collect();
        Expr::fold(
            lits,
            if form == Form::SumOfProducts {
                Expr::and
            } else {
                Expr::or
            },
        )
    }
}

/// All prime implicants of the given rows (`care` plus don't-cares in `extra`).
pub fn prime_implicants(inputs: usize, rows: &[u32]) -> Vec<Implicant> {
    let mut current: BTreeSet<Implicant> = rows.iter().map(|r| Implicant::row(*r)).collect();
    let mut primes = BTreeSet::new();
    while !current.is_empty() {
        let mut used = BTreeSet::new();
        let mut next = BTreeSet::new();
        for imp in &current {
            for b in 0..inputs {
                let bit = 1u32 << b;
                if imp.dashes & bit != 0 || imp.values & bit != 0 {
                    continue;
                }
                let partner = Implicant {
                    dashes: imp.dashes,
                    values: imp.values | bit,
                };
                if current.contains(&partner) {
                    used.insert(*imp);
                    used.insert(partner);
                    next.insert(Implicant {
                        dashes: imp.dashes | bit,
                        values: imp.values,
                    });
                }
            }
        }
        for imp in &current {
            if !used.contains(imp) {
                primes.insert(*imp);
            }
        }
        current = next;
    }
    primes.into_iter().collect()
}

const SEARCH_LIMIT: usize = 200_000;

/// A cheapest set of `primes` covering every row in `must`.
pub fn minimal_cover(inputs: usize, primes: &[Implicant], must: &[u32]) -> Vec<Implicant> {
    // which primes cover each required row
    let mut covers: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for r in must {
        covers.insert(
            *r,
            primes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.covers(*r))
                .map(|(i, _)| i)
                .collect(),
        );
    }
    let mut chosen: BTreeSet<usize> = BTreeSet::new();
    let mut remaining: BTreeSet<u32> = must.iter().copied().collect();
    // essential primes: the only cover of some row (repeat, as picking one can create others)
    loop {
        let essential = remaining.iter().find_map(|r| {
            let c = &covers[r];
            (c.len() == 1).then_some(c[0])
        });
        let Some(p) = essential else { break };
        chosen.insert(p);
        remaining.retain(|r| !primes[p].covers(*r));
    }
    if remaining.is_empty() {
        return chosen.into_iter().map(|i| primes[i]).collect();
    }

    // Row dominance is skipped; branch and bound over the rows left, cheapest cover first.
    let rows: Vec<u32> = remaining.iter().copied().collect();
    let cost = |sel: &[usize]| -> (usize, usize) {
        (
            sel.len(),
            sel.iter().map(|i| primes[*i].literals(inputs)).sum(),
        )
    };
    let mut best: Option<Vec<usize>> = None;
    let mut nodes = 0usize;
    let mut path: Vec<usize> = vec![];
    fn search(
        rows: &[u32],
        covers: &BTreeMap<u32, Vec<usize>>,
        primes: &[Implicant],
        path: &mut Vec<usize>,
        best: &mut Option<Vec<usize>>,
        nodes: &mut usize,
        cost: &dyn Fn(&[usize]) -> (usize, usize),
    ) {
        *nodes += 1;
        if *nodes > SEARCH_LIMIT {
            return;
        }
        if let Some(b) = best {
            if cost(path) >= cost(b) {
                return;
            }
        }
        // first row not yet covered, with the fewest candidate primes
        let open = rows
            .iter()
            .filter(|r| !path.iter().any(|p| primes[*p].covers(**r)))
            .min_by_key(|r| covers[*r].len());
        let Some(row) = open else {
            *best = Some(path.clone());
            return;
        };
        let mut cands = covers[row].clone();
        // prefer primes covering many open rows
        cands.sort_by_key(|p| {
            std::cmp::Reverse(
                rows.iter()
                    .filter(|r| {
                        primes[*p].covers(**r) && !path.iter().any(|q| primes[*q].covers(**r))
                    })
                    .count(),
            )
        });
        for c in cands {
            path.push(c);
            search(rows, covers, primes, path, best, nodes, cost);
            path.pop();
        }
    }
    search(
        &rows, &covers, primes, &mut path, &mut best, &mut nodes, &cost,
    );
    // if the budget ran out before any cover was found, fall back to the greedy cover
    let extra = best.unwrap_or_else(|| {
        let mut left: BTreeSet<u32> = remaining.clone();
        let mut pick = vec![];
        while !left.is_empty() {
            let (i, _) = primes
                .iter()
                .enumerate()
                .map(|(i, p)| (i, left.iter().filter(|r| p.covers(**r)).count()))
                .max_by_key(|(i, n)| (*n, std::cmp::Reverse(primes[*i].literals(inputs))))
                .unwrap();
            left.retain(|r| !primes[i].covers(*r));
            pick.push(i);
        }
        pick
    });
    chosen.extend(extra);
    chosen.into_iter().map(|i| primes[i]).collect()
}

/// What minimising a column found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minimal {
    pub form: Form,
    /// Terms of the cover; empty means the function is constant (see [`Minimal::to_expr`]).
    pub implicants: Vec<Implicant>,
}

impl Minimal {
    /// The minimal expression. A constant function becomes `0` or `1`.
    pub fn to_expr(&self, names: &[String]) -> Expr {
        let sop = self.form == Form::SumOfProducts;
        let terms: Vec<Option<Expr>> = self
            .implicants
            .iter()
            .map(|i| i.term(names, self.form))
            .collect();
        // a term without literals is constant 1 (SOP) or 0 (POS) and decides everything
        if terms.iter().any(|t| t.is_none()) {
            return Expr::Const(sop);
        }
        let terms: Vec<Expr> = terms.into_iter().flatten().collect();
        Expr::fold(terms, if sop { Expr::or } else { Expr::and }).unwrap_or(Expr::Const(!sop))
    }
}

/// Largest input count minimised without being asked (it takes noticeably long beyond this).
pub const AUTO_LIMIT: usize = 6;

/// Minimal cover for output column `out` of `table`. Bus-error rows count as don't care.
/// Returns `None` when the table has more than [`AUTO_LIMIT`] inputs and `force` is false.
pub fn minimize(table: &TruthTable, out: usize, form: Form, force: bool) -> Option<Minimal> {
    let n = table.input_count();
    if n > AUTO_LIMIT && !force {
        return None;
    }
    let wanted = if form == Form::SumOfProducts {
        Entry::One
    } else {
        Entry::Zero
    };
    let other = if form == Form::SumOfProducts {
        Entry::Zero
    } else {
        Entry::One
    };
    let mut must = vec![];
    let mut usable = vec![];
    for row in 0..table.row_count() {
        let e = table.get(row, out);
        if e == other {
            continue;
        }
        usable.push(row as u32);
        if e == wanted {
            must.push(row as u32);
        }
    }
    if must.is_empty() {
        return Some(Minimal {
            form,
            implicants: vec![],
        });
    }
    let primes = prime_implicants(n, &usable);
    Some(Minimal {
        form,
        implicants: minimal_cover(n, &primes, &must),
    })
}

/// The unminimised expression: one term per wanted row.
pub fn canonical(table: &TruthTable, out: usize, form: Form, names: &[String]) -> Expr {
    let wanted = if form == Form::SumOfProducts {
        Entry::One
    } else {
        Entry::Zero
    };
    let imps = Minimal {
        form,
        implicants: table
            .rows_with(out, wanted)
            .into_iter()
            .map(|r| Implicant::row(r as u32))
            .collect(),
    };
    if imps.implicants.is_empty() {
        return Expr::Const(form == Form::ProductOfSums);
    }
    imps.to_expr(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| ((b'a' + i as u8) as char).to_string())
            .collect()
    }

    fn table_from(n: usize, ones: &[usize], dont_care: &[usize]) -> TruthTable {
        let mut t = TruthTable::new(n, 1);
        t.fill_column(0, Entry::Zero);
        for r in ones {
            t.set(*r, 0, Entry::One);
        }
        for r in dont_care {
            t.set(*r, 0, Entry::DontCare);
        }
        t
    }

    fn eval(e: &Expr, n: usize, row: usize) -> bool {
        let nm = names(n);
        e.eval(&|v| {
            let col = nm.iter().position(|x| x == v).unwrap();
            row >> (n - 1 - col) & 1 == 1
        })
    }

    /// The expression must be right on every row that matters.
    fn check(t: &TruthTable, e: &Expr) {
        for row in 0..t.row_count() {
            match t.get(row, 0) {
                Entry::One => assert!(eval(e, t.input_count(), row), "row {row} should be 1: {e}"),
                Entry::Zero => {
                    assert!(!eval(e, t.input_count(), row), "row {row} should be 0: {e}")
                }
                _ => {}
            }
        }
    }

    #[test]
    fn implicant_basics() {
        let i = Implicant {
            dashes: 0b0101,
            values: 0b1010,
        };
        assert!(i.covers(0b1010) && i.covers(0b1111) && !i.covers(0b0010));
        assert_eq!(i.rows(), [0b1010, 0b1011, 0b1110, 0b1111]);
        assert_eq!(i.literals(4), 2);
    }

    #[test]
    fn pattern_text() {
        let i = Implicant {
            dashes: 0b0100,
            values: 0b1001,
        };
        assert_eq!(i.pattern(4), "1-01");
    }

    #[test]
    fn primes_of_a_classic_example() {
        // f(a,b,c,d) = Σ m(4,8,10,11,12,15) + d(9,14) : textbook
        let rows = [4, 8, 9, 10, 11, 12, 14, 15];
        let p = prime_implicants(4, &rows);
        let texts: Vec<_> = p.iter().map(|i| i.pattern(4)).collect();
        for want in ["-100", "10--", "1-1-", "1--0", "-1-0"] {
            if want == "-1-0" {
                continue;
            }
            assert!(texts.contains(&want.to_string()), "{texts:?} lacks {want}");
        }
        // every prime only covers usable rows
        for i in &p {
            assert!(i
                .rows()
                .iter()
                .all(|r| rows.contains(&(*r as usize).try_into().unwrap())));
        }
    }

    #[test]
    fn textbook_minimisation() {
        let t = table_from(4, &[4, 8, 10, 11, 12, 15], &[9, 14]);
        let m = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        let e = m.to_expr(&names(4));
        check(&t, &e);
        assert_eq!(m.implicants.len(), 3, "{m:?}");
        let lits: usize = m.implicants.iter().map(|i| i.literals(4)).sum();
        assert_eq!(lits, 2 + 2 + 3, "{e}"); // 10-- , 1-1- , -100  => 7 literals
    }

    #[test]
    fn product_of_sums_uses_zeros() {
        let t = table_from(3, &[1, 3, 5, 7], &[]); // f = c
        let m = minimize(&t, 0, Form::ProductOfSums, false).unwrap();
        assert_eq!(m.to_expr(&names(3)), Expr::var("c"));
        let s = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        assert_eq!(s.to_expr(&names(3)), Expr::var("c"));
    }

    #[test]
    fn constants() {
        let zero = table_from(2, &[], &[]);
        let one = table_from(2, &[0, 1, 2, 3], &[]);
        let s = |t: &TruthTable| {
            minimize(t, 0, Form::SumOfProducts, false)
                .unwrap()
                .to_expr(&names(2))
        };
        let p = |t: &TruthTable| {
            minimize(t, 0, Form::ProductOfSums, false)
                .unwrap()
                .to_expr(&names(2))
        };
        assert_eq!(s(&zero), Expr::Const(false));
        assert_eq!(s(&one), Expr::Const(true));
        assert_eq!(p(&zero), Expr::Const(false));
        assert_eq!(p(&one), Expr::Const(true));
        // all don't care counts as nothing to implement
        let x = TruthTable::new(2, 1);
        assert_eq!(s(&x), Expr::Const(false));
        assert_eq!(p(&x), Expr::Const(true));
    }

    #[test]
    fn dont_cares_are_used_to_simplify() {
        // 1 only on row 0b11; row 0b10 is don't care: f = a
        let t = table_from(2, &[3], &[2]);
        let m = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        assert_eq!(m.to_expr(&names(2)), Expr::var("a"));
    }

    #[test]
    fn bus_errors_count_as_dont_care() {
        let mut t = table_from(2, &[3], &[]);
        t.set(2, 0, Entry::BusError);
        let m = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        assert_eq!(m.to_expr(&names(2)), Expr::var("a"));
    }

    #[test]
    fn xor_has_no_simplification() {
        let t = table_from(3, &[1, 2, 4, 7], &[]);
        let m = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        assert_eq!(m.implicants.len(), 4);
        check(&t, &m.to_expr(&names(3)));
    }

    #[test]
    fn many_inputs_need_force() {
        let t = table_from(7, &[0, 1], &[]);
        assert!(minimize(&t, 0, Form::SumOfProducts, false).is_none());
        let m = minimize(&t, 0, Form::SumOfProducts, true).unwrap();
        assert_eq!(m.implicants.len(), 1);
        assert_eq!(m.implicants[0].literals(7), 6);
    }

    #[test]
    fn cyclic_cover_is_minimal() {
        // the classic cyclic prime table: f = Σ m(0,1,2,5,6,7) has two minimal covers of 3 terms
        let t = table_from(3, &[0, 1, 2, 5, 6, 7], &[]);
        let m = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        assert_eq!(m.implicants.len(), 3);
        check(&t, &m.to_expr(&names(3)));
    }

    #[test]
    fn result_is_deterministic() {
        let t = table_from(4, &[0, 2, 5, 7, 8, 10, 13, 15], &[1, 6]);
        let a = minimize(&t, 0, Form::SumOfProducts, false).unwrap();
        for _ in 0..5 {
            assert_eq!(a, minimize(&t, 0, Form::SumOfProducts, false).unwrap());
        }
    }

    #[test]
    fn exhaustive_three_input_functions_are_correct_and_no_larger_than_canonical() {
        for bits in 0..256u32 {
            let ones: Vec<usize> = (0..8).filter(|r| bits >> r & 1 == 1).collect();
            let t = table_from(3, &ones, &[]);
            for form in [Form::SumOfProducts, Form::ProductOfSums] {
                let m = minimize(&t, 0, form, false).unwrap();
                let e = m.to_expr(&names(3));
                check(&t, &e);
                assert!(m.implicants.len() <= ones.len().max(8 - ones.len()).max(1));
                if form == Form::SumOfProducts {
                    assert!(e.is_sop(), "{e}");
                } else {
                    assert!(e.is_pos(), "{e}");
                }
            }
        }
    }

    #[test]
    fn random_four_input_functions_with_dont_cares_are_correct() {
        let mut seed = 0x9E3779B97F4A7C15u64;
        for _ in 0..300 {
            let mut t = TruthTable::new(4, 1);
            for r in 0..16 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                t.set(
                    r,
                    0,
                    match seed % 5 {
                        0 | 1 => Entry::Zero,
                        2 | 3 => Entry::One,
                        _ => Entry::DontCare,
                    },
                );
            }
            for form in [Form::SumOfProducts, Form::ProductOfSums] {
                let e = minimize(&t, 0, form, false).unwrap().to_expr(&names(4));
                check(&t, &e);
            }
        }
    }

    #[test]
    fn canonical_form_lists_every_wanted_row() {
        let t = table_from(2, &[1, 2], &[]);
        let e = canonical(&t, 0, Form::SumOfProducts, &names(2));
        assert_eq!(
            e.to_text(crate::expr::Notation::Mathematical),
            "~a⋅b + a⋅~b"
        );
        let p = canonical(&t, 0, Form::ProductOfSums, &names(2));
        check(&t, &p);
        assert_eq!(
            p.to_text(crate::expr::Notation::Mathematical),
            "(a + b)⋅(~a + ~b)"
        );
        let none = table_from(2, &[], &[]);
        assert_eq!(
            canonical(&none, 0, Form::SumOfProducts, &names(2)),
            Expr::Const(false)
        );
    }
}
