//! Sparse memory contents (Java `MemContents`, minus listeners and hex-file I/O).

use std::collections::HashMap;

const PAGE_BITS: u32 = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_MASK: u64 = (PAGE_SIZE as u64) - 1;

/// `2^addr_bits` words of `width` bits each, stored in 4096-word pages allocated on first write
/// of a non-zero value. Unwritten words read as 0.
#[derive(Debug, Clone)]
pub struct MemContents {
    addr_bits: u8,
    width: u8,
    pages: HashMap<u64, Box<[u64; PAGE_SIZE]>>,
}

impl MemContents {
    /// # Panics
    /// If `addr_bits` is not in 1..=32 or `width` not in 1..=64.
    pub fn new(addr_bits: u8, width: u8) -> Self {
        assert!((1..=32).contains(&addr_bits), "address width must be 1..=32");
        assert!((1..=64).contains(&width), "data width must be 1..=64");
        MemContents {
            addr_bits,
            width,
            pages: HashMap::new(),
        }
    }

    /// Contents initialised from `values` starting at address 0 (extra values are dropped).
    pub fn from_values(addr_bits: u8, width: u8, values: &[u64]) -> Self {
        let mut c = Self::new(addr_bits, width);
        c.set_range(0, values);
        c
    }

    pub fn addr_bits(&self) -> u8 {
        self.addr_bits
    }
    pub fn width(&self) -> u8 {
        self.width
    }
    /// Highest valid address.
    pub fn last_offset(&self) -> u64 {
        (1u64 << self.addr_bits) - 1
    }
    fn mask(&self) -> u64 {
        if self.width >= 64 {
            u64::MAX
        } else {
            (1u64 << self.width) - 1
        }
    }

    /// Word at `addr` (0 if never written or out of range).
    pub fn get(&self, addr: u64) -> u64 {
        if addr > self.last_offset() {
            return 0;
        }
        self.pages
            .get(&(addr >> PAGE_BITS))
            .map_or(0, |p| p[(addr & PAGE_MASK) as usize] & self.mask())
    }

    /// Stores `value` (masked to the data width). Out-of-range addresses are ignored.
    pub fn set(&mut self, addr: u64, value: u64) {
        if addr > self.last_offset() {
            return;
        }
        let v = value & self.mask();
        let page = addr >> PAGE_BITS;
        match self.pages.get_mut(&page) {
            Some(p) => p[(addr & PAGE_MASK) as usize] = v,
            None if v != 0 => {
                let mut p = Box::new([0u64; PAGE_SIZE]);
                p[(addr & PAGE_MASK) as usize] = v;
                self.pages.insert(page, p);
            }
            None => {}
        }
    }

    /// Stores consecutive words starting at `start`.
    pub fn set_range(&mut self, start: u64, values: &[u64]) {
        for (i, v) in values.iter().enumerate() {
            self.set(start + i as u64, *v);
        }
    }

    /// `len` consecutive words starting at `start`.
    pub fn get_range(&self, start: u64, len: usize) -> Vec<u64> {
        (0..len as u64).map(|i| self.get(start + i)).collect()
    }

    /// Sets `len` words starting at `start` to `value`.
    pub fn fill(&mut self, start: u64, len: u64, value: u64) {
        let end = start.saturating_add(len).min(self.last_offset() + 1);
        if value & self.mask() == 0 {
            // zero-fill: drop whole pages when possible
            let mut a = start;
            while a < end {
                let page = a >> PAGE_BITS;
                let page_end = ((page + 1) << PAGE_BITS).min(end);
                if a & PAGE_MASK == 0 && page_end - a == PAGE_SIZE as u64 {
                    self.pages.remove(&page);
                } else {
                    for x in a..page_end {
                        self.set(x, 0);
                    }
                }
                a = page_end;
            }
        } else {
            for a in start..end {
                self.set(a, value);
            }
        }
    }

    /// Sets every word to 0.
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn is_clear(&self) -> bool {
        self.pages.values().all(|p| p.iter().all(|&w| w & self.mask() == 0))
    }

    /// Changes the geometry, keeping words that remain in range (masked to the new width).
    pub fn set_dimensions(&mut self, addr_bits: u8, width: u8) {
        assert!((1..=32).contains(&addr_bits) && (1..=64).contains(&width));
        let old_mask = self.mask();
        self.addr_bits = addr_bits;
        self.width = width;
        let new_mask = self.mask();
        let last = self.last_offset();
        self.pages.retain(|&page, p| {
            let base = page << PAGE_BITS;
            if base > last {
                return false;
            }
            for (i, w) in p.iter_mut().enumerate() {
                *w = if base + i as u64 <= last {
                    *w & old_mask & new_mask
                } else {
                    0
                };
            }
            true
        });
    }
}

impl MemContents {
    /// Number of words up to and including the last non-zero one (0 if all clear).
    pub fn used_len(&self) -> u64 {
        let mut best = 0;
        for (&page, p) in &self.pages {
            if let Some(i) = p.iter().rposition(|&w| w & self.mask() != 0) {
                best = best.max((page << PAGE_BITS) + i as u64 + 1);
            }
        }
        best
    }

    /// Text form for saving: hexadecimal words separated by spaces, with `value*count` for runs,
    /// ending at the last non-zero word (e.g. `1f 2a 0*10 ff`). Empty when all zero.
    pub fn to_text(&self) -> String {
        let len = self.used_len();
        let mut out: Vec<String> = vec![];
        let (mut run_val, mut run_len) = (0u64, 0u64);
        let flush = |out: &mut Vec<String>, v: u64, n: u64| match n {
            0 => {}
            1 => out.push(format!("{v:x}")),
            _ => out.push(format!("{v:x}*{n}")),
        };
        let mut a = 0;
        while a < len {
            // skip absent pages in one step
            if a & PAGE_MASK == 0 && !self.pages.contains_key(&(a >> PAGE_BITS)) {
                let n = (PAGE_SIZE as u64).min(len - a);
                if run_len > 0 && run_val == 0 {
                    run_len += n;
                } else {
                    flush(&mut out, run_val, run_len);
                    (run_val, run_len) = (0, n);
                }
                a += n;
                continue;
            }
            let v = self.get(a);
            if run_len > 0 && v == run_val {
                run_len += 1;
            } else {
                flush(&mut out, run_val, run_len);
                (run_val, run_len) = (v, 1);
            }
            a += 1;
        }
        flush(&mut out, run_val, run_len);
        out.join(" ")
    }

    /// Loads text produced by [`to_text`](Self::to_text) at address 0.
    pub fn load_text(&mut self, text: &str) -> Result<(), String> {
        let mut addr = 0u64;
        for tok in text.split_whitespace() {
            let (v, n) = match tok.split_once('*') {
                Some((v, n)) => (
                    v,
                    n.parse::<u64>().map_err(|_| format!("bad repeat count in `{tok}`"))?,
                ),
                None => (tok, 1),
            };
            let v = u64::from_str_radix(v, 16).map_err(|_| format!("`{v}` is not a hexadecimal word"))?;
            if v & !self.mask() != 0 {
                return Err(format!("`{tok}` does not fit in {} bits", self.width));
            }
            if addr.saturating_add(n) > self.last_offset() + 1 {
                return Err("contents are larger than the memory".into());
            }
            self.fill(addr, n, v);
            addr += n;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_round_trip_with_runs() {
        let mut m = MemContents::new(16, 16);
        m.set(0, 0x1f);
        m.set(1, 0x1f);
        m.set(2, 0x1f);
        m.set(10, 0xabcd);
        m.set(9000, 7);
        let t = m.to_text();
        assert!(t.starts_with("1f*3 0*7 abcd 0*"), "{t}");
        let mut n = MemContents::new(16, 16);
        n.load_text(&t).unwrap();
        assert_eq!(n.get(10), 0xabcd);
        assert_eq!(n.get(9000), 7);
        assert_eq!(n.get(8999), 0);
        assert_eq!(n.to_text(), t);
        assert_eq!(MemContents::new(4, 4).to_text(), "");
    }

    #[test]
    fn text_load_rejects_bad_input() {
        let mut m = MemContents::new(2, 4);
        assert!(m.load_text("zz").is_err());
        assert!(m.load_text("1f").is_err());
        assert!(m.load_text("1*5").is_err());
    }

    use super::*;

    #[test]
    fn unwritten_reads_zero_and_set_masks_to_width() {
        let mut m = MemContents::new(10, 4);
        assert_eq!(m.get(5), 0);
        m.set(5, 0x1F);
        assert_eq!(m.get(5), 0xF);
        m.set(5, 0);
        assert_eq!(m.get(5), 0);
        assert!(m.is_clear());
    }

    #[test]
    fn out_of_range_is_ignored() {
        let mut m = MemContents::new(4, 8);
        m.set(16, 7);
        assert_eq!(m.get(16), 0);
        assert!(m.is_clear());
    }

    #[test]
    fn large_sparse_memory_across_pages() {
        let mut m = MemContents::new(24, 32);
        m.set(0xFF_FFFF, 0xDEAD_BEEF);
        m.set(0x1000, 1);
        assert_eq!(m.get(0xFF_FFFF), 0xDEAD_BEEF);
        assert_eq!(m.get(0x1000), 1);
        assert_eq!(m.get(0x0FFF), 0);
        m.clear();
        assert!(m.is_clear());
    }

    #[test]
    fn ranges_and_fill() {
        let mut m = MemContents::from_values(13, 8, &[1, 2, 3]);
        assert_eq!(m.get_range(0, 4), [1, 2, 3, 0]);
        m.fill(4090, 20, 0xAA);
        assert_eq!(m.get(4089), 0);
        assert_eq!(m.get(4090), 0xAA);
        assert_eq!(m.get(4109), 0xAA);
        assert_eq!(m.get(4110), 0);
        m.fill(0, 8192, 0);
        assert!(m.is_clear());
    }

    #[test]
    fn dimension_change_truncates() {
        let mut m = MemContents::from_values(4, 8, &[0xFF, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0x1AB]);
        m.set_dimensions(3, 4);
        assert_eq!(m.get(0), 0xF);
        assert_eq!(m.get(7), 8);
        assert_eq!(m.get(8), 0);
        m.set_dimensions(5, 8);
        assert_eq!(m.get(8), 0, "removed words stay gone");
        assert_eq!(m.get(0), 0xF);
    }
}
