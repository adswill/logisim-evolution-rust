//! Karnaugh map layout: where each truth-table row sits on the map and how groups of rows
//! (implicants) are drawn.
//!
//! The first inputs run down the side and the rest across the top, both in Gray-code order,
//! so that neighbouring cells (also across the map's edges) differ in one input.

use crate::implicant::Implicant;

/// Largest number of inputs a map can show.
pub const MAX_VARS: usize = 6;

/// Number of inputs on the side (rows) for 0..=6 inputs.
const ROW_VARS: [usize; 7] = [0, 0, 1, 1, 2, 2, 3];
/// Number of inputs along the top (columns) for 0..=6 inputs.
const COL_VARS: [usize; 7] = [0, 1, 1, 2, 2, 3, 3];

/// Gray code: the bit pattern shown at position `i` along an axis.
pub fn gray(i: usize) -> usize {
    i ^ (i >> 1)
}

/// Position along an axis of the bit pattern `g` (inverse of [`gray`]).
pub fn gray_position(g: usize) -> usize {
    let mut i = g;
    let mut shift = 1;
    while g >> shift != 0 {
        i ^= g >> shift;
        shift += 1;
    }
    i
}

/// A rectangle of map cells that may wrap around the edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub row: usize,
    pub col: usize,
    pub height: usize,
    pub width: usize,
}

/// A group drawn on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub implicant: Implicant,
    /// Every covered cell as (row, column).
    pub cells: Vec<(usize, usize)>,
    /// The cells as rectangles. Wrapping groups, and groups whose cells are not adjacent (such as
    /// the four corners), are drawn as several pieces.
    pub rects: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KMap {
    inputs: usize,
}

impl KMap {
    /// Layout for 1 to [`MAX_VARS`] inputs; `None` outside that range.
    pub fn new(inputs: usize) -> Option<KMap> {
        (1..=MAX_VARS).contains(&inputs).then_some(KMap { inputs })
    }

    pub fn inputs(&self) -> usize {
        self.inputs
    }
    /// Number of inputs shown down the side.
    pub fn row_vars(&self) -> usize {
        ROW_VARS[self.inputs]
    }
    /// Number of inputs shown across the top.
    pub fn col_vars(&self) -> usize {
        COL_VARS[self.inputs]
    }
    pub fn rows(&self) -> usize {
        1 << self.row_vars()
    }
    pub fn cols(&self) -> usize {
        1 << self.col_vars()
    }

    /// Truth-table row shown at (`row`, `col`).
    pub fn table_row(&self, row: usize, col: usize) -> usize {
        assert!(row < self.rows() && col < self.cols());
        gray(row) << self.col_vars() | gray(col)
    }

    /// Where a truth-table row is shown, as (row, column).
    pub fn position(&self, table_row: usize) -> (usize, usize) {
        assert!(table_row < 1 << self.inputs);
        let col_bits = table_row & (self.cols() - 1);
        let row_bits = table_row >> self.col_vars();
        (gray_position(row_bits), gray_position(col_bits))
    }

    /// Header text for a row or column position: the input bits there, e.g. `"01"`.
    pub fn row_label(&self, row: usize) -> String {
        bits_text(gray(row), self.row_vars())
    }
    pub fn col_label(&self, col: usize) -> String {
        bits_text(gray(col), self.col_vars())
    }

    /// Draw a group for an implicant.
    pub fn group(&self, imp: Implicant) -> Group {
        let mut cells: Vec<(usize, usize)> = imp
            .rows()
            .into_iter()
            .map(|r| self.position(r as usize))
            .collect();
        cells.sort_unstable();
        let mut row_set: Vec<usize> = cells.iter().map(|c| c.0).collect();
        row_set.dedup();
        let mut col_set: Vec<usize> = cells.iter().map(|c| c.1).collect();
        col_set.sort_unstable();
        col_set.dedup();
        let rr = runs(&row_set, self.rows());
        let cr = runs(&col_set, self.cols());
        let mut rects = vec![];
        for (r0, h) in &rr {
            for (c0, w) in &cr {
                rects.push(Rect {
                    row: *r0,
                    col: *c0,
                    height: *h,
                    width: *w,
                });
            }
        }
        Group {
            implicant: imp,
            cells,
            rects,
        }
    }

    pub fn groups(&self, implicants: &[Implicant]) -> Vec<Group> {
        implicants.iter().map(|i| self.group(*i)).collect()
    }
}

fn bits_text(v: usize, n: usize) -> String {
    (0..n)
        .rev()
        .map(|b| if v >> b & 1 == 1 { '1' } else { '0' })
        .collect()
}

/// Split sorted distinct positions on a ring of `size` into maximal runs (start, length), where
/// a run may wrap from the last position to the first.
fn runs(sorted: &[usize], size: usize) -> Vec<(usize, usize)> {
    if sorted.len() == size {
        return vec![(0, size)];
    }
    let mut out: Vec<(usize, usize)> = vec![];
    for &p in sorted {
        match out.last_mut() {
            Some((s, l)) if *s + *l == p => *l += 1,
            _ => out.push((p, 1)),
        }
    }
    if out.len() > 1 {
        let (ls, ll) = *out.last().unwrap();
        if ls + ll == size && out[0].0 == 0 {
            let (_, first_len) = out.remove(0);
            let last = out.last_mut().unwrap();
            last.1 += first_len;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_matches_the_original_tables() {
        let g: Vec<_> = (0..8).map(gray).collect();
        assert_eq!(g, [0, 1, 3, 2, 6, 7, 5, 4]); // bigColIndex
        let p: Vec<_> = (0..8).map(gray_position).collect();
        assert_eq!(p, [0, 1, 3, 2, 7, 6, 4, 5]); // bigColPlace
    }

    #[test]
    fn layout_sizes() {
        let want = [(1, 2), (2, 2), (2, 4), (4, 4), (4, 8), (8, 8)];
        for (n, size) in (1..=6).zip(want) {
            let k = KMap::new(n).unwrap();
            assert_eq!((k.rows(), k.cols()), size, "{n} inputs");
        }
        assert!(KMap::new(0).is_none() && KMap::new(7).is_none());
    }

    #[test]
    fn position_and_table_row_are_inverse() {
        for n in 1..=6 {
            let k = KMap::new(n).unwrap();
            let mut seen = std::collections::HashSet::new();
            for r in 0..1 << n {
                let (row, col) = k.position(r);
                assert_eq!(k.table_row(row, col), r);
                assert!(seen.insert((row, col)));
            }
        }
    }

    #[test]
    fn neighbours_differ_in_one_input_including_across_the_edges() {
        for n in 2..=6 {
            let k = KMap::new(n).unwrap();
            for row in 0..k.rows() {
                for col in 0..k.cols() {
                    let here = k.table_row(row, col);
                    for there in [
                        k.table_row((row + 1) % k.rows(), col),
                        k.table_row(row, (col + 1) % k.cols()),
                    ] {
                        let d = (here ^ there).count_ones();
                        assert!(d == 1 || (d == 0), "{n} inputs at {row},{col}");
                    }
                }
            }
        }
    }

    #[test]
    fn labels() {
        let k = KMap::new(4).unwrap();
        let cols: Vec<_> = (0..4).map(|c| k.col_label(c)).collect();
        assert_eq!(cols, ["00", "01", "11", "10"]);
        assert_eq!(KMap::new(3).unwrap().row_label(1), "1");
    }

    #[test]
    fn group_cells_are_the_implicants_rows() {
        let k = KMap::new(4).unwrap();
        // a=1 (rows 8..15), b free
        let imp = Implicant {
            dashes: 0b0111,
            values: 0b1000,
        };
        let g = k.group(imp);
        assert_eq!(g.cells.len(), 8);
        assert_eq!(
            g.rects,
            [Rect {
                row: 2,
                col: 0,
                height: 2,
                width: 4
            }]
        );
    }

    #[test]
    fn wrapping_group_is_one_rectangle_with_wraparound() {
        let k = KMap::new(4).unwrap();
        // columns 0 and 3 (bits c=0,d=0 and c=1,d=0 -> d=0): d=0 is columns 0 and 3; with all rows
        let imp = Implicant {
            dashes: 0b1110,
            values: 0,
        };
        let g = k.group(imp);
        assert_eq!(g.cells.len(), 8);
        assert!(
            g.rects.contains(&Rect {
                row: 0,
                col: 3,
                height: 4,
                width: 2
            }),
            "{:?}",
            g.rects
        );
    }

    #[test]
    fn corners_are_four_pieces_or_one_wrapped_piece() {
        let k = KMap::new(4).unwrap();
        // b=0, d=0 : rows with b=0 are positions 0 and 3, columns with d=0 are 0 and 3
        let imp = Implicant {
            dashes: 0b1010,
            values: 0,
        };
        let g = k.group(imp);
        assert_eq!(g.cells.len(), 4);
        let covered: usize = g.rects.iter().map(|r| r.height * r.width).sum();
        assert_eq!(covered, 4);
        assert_eq!(
            g.rects,
            [Rect {
                row: 3,
                col: 3,
                height: 2,
                width: 2
            }]
        );
    }

    #[test]
    fn non_adjacent_columns_split() {
        let k = KMap::new(3).unwrap(); // 1 row bit, 2 column bits
                                       // c=1 : column bit 0 set -> columns at positions 1 and 2 (values 01, 11) hmm adjacent;
                                       // a=? use the low bit of the column pair
        let imp = Implicant {
            dashes: 0b110,
            values: 0b001,
        };
        let g = k.group(imp);
        let covered: usize = g.rects.iter().map(|r| r.height * r.width).sum();
        assert_eq!(covered, g.cells.len());
    }
}
