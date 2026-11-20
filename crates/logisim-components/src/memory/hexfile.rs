//! Memory image files (Java `gui.hex.HexFile`, without the dialogs): loading and saving the
//! contents of a RAM or ROM ([`MemContents`], e.g. `ram.contents_mut()`).
//!
//! Supported formats (all Logisim-compatible):
//! * `v2.0 raw` — hex words, optionally `count*word` (decimal count) for runs, `#` comments.
//! * `v3.0 hex words plain|addressed` — one hex group per word; addressed lines start with a
//!   *word* address (`addr: w w w`).
//! * `v3.0 hex bytes plain|addressed [big-endian|little-endian]` — `xxd`-style byte dumps that are
//!   re-chunked into words of any width; addressed lines use *byte* addresses.
//! * Raw binary files, big- or little-endian ([`load_binary`], [`save_binary`]).
//!
//! In text files the first non-blank line is the header naming the format; anything after `#`
//! is a comment, `0x` prefixes are ignored, and in addressed lines two consecutive whitespace
//! characters end the data (so `xxd`'s ASCII column is skipped). Not ported: the
//! escaped-ASCII format and the file dialogs.
//!
//! Loading replaces the whole memory (unlisted locations become 0). Words that do not fit in the
//! memory's width, or data beyond its size, are errors that name the line; trailing all-zero
//! padding is ignored.

use super::contents::MemContents;

/// Byte order for formats that split bytes into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Endian {
    /// First byte in the file fills the most significant bits (Logisim's default).
    #[default]
    Big,
    /// First byte in the file fills the least significant bits.
    Little,
}

/// A text or binary memory file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexFormat {
    /// `v2.0 raw`
    Raw,
    WordsPlain,
    WordsAddressed,
    BytesPlain(Endian),
    BytesAddressed(Endian),
}

impl HexFormat {
    /// The header line written at the top of a saved file.
    pub fn header(self) -> String {
        let e = |e: Endian| match e {
            Endian::Big => "big-endian",
            Endian::Little => "little-endian",
        };
        match self {
            HexFormat::Raw => "v2.0 raw".into(),
            HexFormat::WordsPlain => "v3.0 hex words plain".into(),
            HexFormat::WordsAddressed => "v3.0 hex words addressed".into(),
            HexFormat::BytesPlain(x) => format!("v3.0 hex bytes plain {}", e(x)),
            HexFormat::BytesAddressed(x) => format!("v3.0 hex bytes addressed {}", e(x)),
        }
    }
}

fn strip(line: &str) -> &str {
    line.split('#').next().unwrap_or("")
}

fn parse_hex(tok: &str) -> Result<u64, String> {
    let t = tok
        .strip_prefix("0x")
        .or_else(|| tok.strip_prefix("0X"))
        .unwrap_or(tok);
    u64::from_str_radix(t, 16).map_err(|_| format!("`{tok}` is not a hexadecimal number"))
}

/// Header → format. `addressed_hint` tells whether the body contains ':' (for headers that do
/// not say `plain`/`addressed`).
fn parse_header(line: &str, addressed_hint: bool) -> Result<HexFormat, String> {
    let words: Vec<String> = line.split_whitespace().map(|w| w.to_lowercase()).collect();
    let first = words.first().map(String::as_str).unwrap_or("");
    if first == "v2.0" {
        return if words.get(1).map(String::as_str) == Some("raw") {
            Ok(HexFormat::Raw)
        } else {
            Err("header `v2.0` must be followed by `raw`".into())
        };
    }
    if first != "v3.0" {
        return Err(format!(
            "unrecognized header `{line}`; expected `v2.0 raw` or `v3.0 hex ...`"
        ));
    }
    let (mut unit, mut style, mut endian) = (None, None, Endian::Big);
    for tag in &words[1..] {
        match tag.as_str() {
            "hex" => {}
            "words" | "bytes" => unit = Some(tag.clone()),
            "plain" | "addressed" => style = Some(tag.clone()),
            "big-endian" => endian = Endian::Big,
            "little-endian" => endian = Endian::Little,
            other => return Err(format!("header tag `{other}` not recognized")),
        }
    }
    let unit = unit.ok_or("header should say either `words` or `bytes`")?;
    let addressed = match style.as_deref() {
        Some("addressed") => true,
        Some(_) => false,
        None => addressed_hint,
    };
    Ok(match (unit.as_str(), addressed) {
        ("words", false) => HexFormat::WordsPlain,
        ("words", true) => HexFormat::WordsAddressed,
        (_, false) => HexFormat::BytesPlain(endian),
        (_, true) => HexFormat::BytesAddressed(endian),
    })
}

/// Reorganizes a byte stream into `width`-bit words.
fn bytes_to_words(bytes: &[u8], width: u8, endian: Endian) -> Vec<u64> {
    let w = width as usize;
    let nbits = bytes.len() * 8;
    let bit = |i: usize| -> u64 {
        match endian {
            Endian::Big => ((bytes[i / 8] >> (7 - i % 8)) & 1) as u64,
            Endian::Little => ((bytes[i / 8] >> (i % 8)) & 1) as u64,
        }
    };
    let count = nbits.div_ceil(w);
    (0..count)
        .map(|k| {
            let mut v = 0u64;
            for j in 0..w {
                let i = k * w + j;
                let b = if i < nbits { bit(i) } else { 0 };
                match endian {
                    Endian::Big => v = (v << 1) | b,
                    Endian::Little => v |= b << j,
                }
            }
            v
        })
        .collect()
}

fn words_to_bytes(words: &[u64], width: u8, endian: Endian) -> Vec<u8> {
    let w = width as usize;
    let nbits = words.len() * w;
    let mut bytes = vec![0u8; nbits.div_ceil(8)];
    for (k, &v) in words.iter().enumerate() {
        for j in 0..w {
            let i = k * w + j;
            let (bitval, pos) = match endian {
                Endian::Big => ((v >> (w - 1 - j)) & 1, 7 - i % 8),
                Endian::Little => ((v >> j) & 1, i % 8),
            };
            bytes[i / 8] |= (bitval as u8) << pos;
        }
    }
    bytes
}

/// Loads a word list at address 0, ignoring trailing zero padding that does not fit.
fn fill(c: &mut MemContents, words: &[u64]) -> Result<(), String> {
    let cap = c.last_offset() + 1;
    for (i, &v) in words.iter().enumerate() {
        if i as u64 >= cap {
            if v == 0 {
                continue;
            }
            return Err(format!("the file has more data than the memory's {cap} words"));
        }
        c.set(i as u64, v);
    }
    Ok(())
}

fn word_fits(c: &MemContents, v: u64, what: &str) -> Result<(), String> {
    if v & !(if c.width() >= 64 { u64::MAX } else { (1u64 << c.width()) - 1 }) != 0 {
        Err(format!("{what}: {v:x} does not fit in {} bits", c.width()))
    } else {
        Ok(())
    }
}

/// Loads a text memory file into `c` (replacing its contents). With `format = None` the format
/// comes from the header line.
pub fn load_text(c: &mut MemContents, text: &str, format: Option<HexFormat>) -> Result<(), String> {
    let mut lines = text.lines().enumerate();
    // header: first non-blank line
    let (hdr_no, hdr) = lines
        .by_ref()
        .find(|(_, l)| !strip(l).trim().is_empty())
        .ok_or("the file is empty: it needs a header line such as `v2.0 raw`")?;
    let hint = text.contains(':');
    let parsed = parse_header(strip(hdr).trim(), hint).map_err(|e| format!("line {}: {e}", hdr_no + 1))?;
    let fmt = format.unwrap_or(parsed);
    let body: Vec<(usize, &str)> = lines.collect();
    let mut tmp = MemContents::new(c.addr_bits(), c.width());
    let err = |no: usize, e: String| format!("line {}: {e}", no + 1);
    match fmt {
        HexFormat::Raw => {
            let mut addr = 0u64;
            let cap = tmp.last_offset() + 1;
            for &(no, line) in &body {
                for tok in strip(line).split_whitespace() {
                    let (count, val) = match tok.split_once('*') {
                        Some((n, v)) => (
                            n.parse::<u64>().map_err(|_| err(no, format!("bad repeat count in `{tok}`")))?,
                            v,
                        ),
                        None => (1, tok),
                    };
                    let v = parse_hex(val).map_err(|e| err(no, e))?;
                    word_fits(&tmp, v, tok).map_err(|e| err(no, e))?;
                    if v != 0 && addr + count > cap || addr > cap {
                        return Err(err(no, format!("the file has more data than the memory's {cap} words")));
                    }
                    tmp.fill(addr, count.min(cap.saturating_sub(addr)), v);
                    addr += count;
                }
            }
        }
        HexFormat::WordsPlain => {
            let mut words = vec![];
            for &(no, line) in &body {
                for tok in strip(line).split_whitespace() {
                    let v = parse_hex(tok).map_err(|e| err(no, e))?;
                    word_fits(&tmp, v, tok).map_err(|e| err(no, e))?;
                    words.push(v);
                }
            }
            fill(&mut tmp, &words)?;
        }
        HexFormat::WordsAddressed => {
            for &(no, line) in &body {
                let Some((addr, toks)) = split_addressed(strip(line)) else { continue };
                let mut a = parse_hex(addr).map_err(|e| err(no, e))?;
                #[allow(clippy::explicit_counter_loop)]
                for tok in toks {
                    let v = parse_hex(tok).map_err(|e| err(no, e))?;
                    word_fits(&tmp, v, tok).map_err(|e| err(no, e))?;
                    if a > tmp.last_offset() {
                        return Err(err(no, format!("address {a:x} is beyond the memory")));
                    }
                    tmp.set(a, v);
                    a += 1;
                }
            }
        }
        HexFormat::BytesPlain(endian) => {
            let mut bytes = vec![];
            for &(no, line) in &body {
                let digits: String = strip(line)
                    .split_whitespace()
                    .map(|t| t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t))
                    .collect();
                push_hex_bytes(&digits, &mut bytes).map_err(|e| err(no, e))?;
            }
            fill(&mut tmp, &bytes_to_words(&bytes, c.width(), endian))?;
        }
        HexFormat::BytesAddressed(endian) => {
            let mut bytes: Vec<u8> = vec![];
            for &(no, line) in &body {
                let Some((addr, toks)) = split_addressed(strip(line)) else { continue };
                let a = parse_hex(addr).map_err(|e| err(no, e))? as usize;
                let digits: String = toks
                    .iter()
                    .map(|t| t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t))
                    .collect();
                let mut chunk = vec![];
                push_hex_bytes(&digits, &mut chunk).map_err(|e| err(no, e))?;
                let cap_bytes = ((tmp.last_offset() + 1) as usize * c.width() as usize).div_ceil(8);
                if a + chunk.len() > cap_bytes.max(bytes.len()) + 1 && chunk.iter().any(|&b| b != 0) && a + chunk.len() > cap_bytes {
                    return Err(err(no, "the file has more data than the memory".into()));
                }
                if bytes.len() < a + chunk.len() {
                    bytes.resize(a + chunk.len(), 0);
                }
                bytes[a..a + chunk.len()].copy_from_slice(&chunk);
            }
            fill(&mut tmp, &bytes_to_words(&bytes, c.width(), endian))?;
        }
    }
    c.clear();
    for a in 0..=tmp.last_offset().min(tmp.used_len().saturating_sub(1)) {
        if tmp.used_len() > 0 {
            c.set(a, tmp.get(a));
        }
    }
    Ok(())
}

/// `addr[:] data...`; stops at two consecutive whitespace characters (xxd's ASCII column).
fn split_addressed(line: &str) -> Option<(&str, Vec<&str>)> {
    let line = line.trim_end();
    if line.trim().is_empty() {
        return None;
    }
    let line = line.trim_start();
    let end = line.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(line.len());
    let (addr, rest) = line.split_at(end);
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let rest = rest.trim_start_matches([' ', '\t']);
    // data ends at a double whitespace
    let data = match rest.find("  ") {
        Some(i) => &rest[..i],
        None => rest,
    };
    Some((addr, data.split_whitespace().collect()))
}

fn push_hex_bytes(digits: &str, out: &mut Vec<u8>) -> Result<(), String> {
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{digits}` contains characters that are not hex digits"));
    }
    if !digits.len().is_multiple_of(2) {
        return Err(format!("`{digits}` has an odd number of hex digits; bytes need two each"));
    }
    for i in (0..digits.len()).step_by(2) {
        out.push(u8::from_str_radix(&digits[i..i + 2], 16).unwrap());
    }
    Ok(())
}

/// Loads a raw binary file (no header) in the given byte order.
pub fn load_binary(c: &mut MemContents, data: &[u8], endian: Endian) -> Result<(), String> {
    let mut tmp = MemContents::new(c.addr_bits(), c.width());
    fill(&mut tmp, &bytes_to_words(data, c.width(), endian))?;
    c.clear();
    for a in 0..tmp.used_len() {
        c.set(a, tmp.get(a));
    }
    Ok(())
}

/// Saves `c` as raw binary (up to the last non-zero word).
pub fn save_binary(c: &MemContents, endian: Endian) -> Vec<u8> {
    words_to_bytes(&c.get_range(0, c.used_len() as usize), c.width(), endian)
}

fn hex_digits(width: u8) -> usize {
    (width as usize).div_ceil(4)
}

/// Renders `c` in `format`, header line included. Addressed formats skip all-zero lines
/// (gaps read back as zero); other formats run up to the last non-zero word.
pub fn save_text(c: &MemContents, format: HexFormat) -> String {
    let used = c.used_len();
    let words = c.get_range(0, used as usize);
    let mut out = format.header();
    out.push('\n');
    let digits = hex_digits(c.width());
    match format {
        HexFormat::Raw => {
            let mut toks = vec![];
            let mut i = 0;
            while i < words.len() {
                let mut run = 1;
                while i + run < words.len() && words[i + run] == words[i] {
                    run += 1;
                }
                toks.push(if run >= 4 {
                    format!("{run}*{:x}", words[i])
                } else {
                    (0..run).map(|_| format!("{:x}", words[i])).collect::<Vec<_>>().join(" ")
                });
                i += run;
            }
            for line in toks.chunks(8) {
                out.push_str(&line.join(" "));
                out.push('\n');
            }
        }
        HexFormat::WordsPlain => {
            for line in words.chunks(8) {
                let l: Vec<String> = line.iter().map(|w| format!("{w:0digits$x}")).collect();
                out.push_str(&l.join(" "));
                out.push('\n');
            }
        }
        HexFormat::WordsAddressed => {
            for (n, line) in words.chunks(8).enumerate() {
                if line.iter().all(|&w| w == 0) {
                    continue;
                }
                let l: Vec<String> = line.iter().map(|w| format!("{w:0digits$x}")).collect();
                out.push_str(&format!("{:x}: {}\n", n * 8, l.join(" ")));
            }
        }
        HexFormat::BytesPlain(e) => {
            for line in words_to_bytes(&words, c.width(), e).chunks(16) {
                let l: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
                out.push_str(&l.join(" "));
                out.push('\n');
            }
        }
        HexFormat::BytesAddressed(e) => {
            for (n, line) in words_to_bytes(&words, c.width(), e).chunks(16).enumerate() {
                if line.iter().all(|&b| b == 0) {
                    continue;
                }
                let l: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
                out.push_str(&format!("{:x}: {}\n", n * 16, l.join(" ")));
            }
        }
    }
    out
}

/// Reads a memory image from `path`: text with a header, or — with `binary` set — raw bytes.
pub fn load_file(c: &mut MemContents, path: &std::path::Path, binary: Option<Endian>) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match binary {
        Some(e) => load_binary(c, &data, e),
        None => {
            let text = String::from_utf8(data).map_err(|_| "the file is not UTF-8 text; load it as binary instead".to_string())?;
            load_text(c, &text, None)
        }
    }
}

/// Writes `c` to `path` in `format`.
pub fn save_file(c: &MemContents, path: &std::path::Path, format: HexFormat) -> Result<(), String> {
    std::fs::write(path, save_text(c, format)).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(addr: u8, width: u8, words: &[u64]) -> MemContents {
        MemContents::from_values(addr, width, words)
    }

    #[test]
    fn raw_with_runs_and_comments() {
        let mut c = MemContents::new(4, 8);
        load_text(&mut c, "v2.0 raw\n# comment\n1 2 3 # more\n3*ff 10\n", None).unwrap();
        assert_eq!(c.get_range(0, 7), vec![1, 2, 3, 0xff, 0xff, 0xff, 0x10]);
        assert_eq!(c.get(7), 0);
    }

    #[test]
    fn words_plain_and_addressed() {
        let mut c = MemContents::new(4, 12);
        load_text(&mut c, "v3.0 hex words plain\n0x001 002 0xabc\n", None).unwrap();
        assert_eq!(c.get_range(0, 3), vec![1, 2, 0xabc]);
        load_text(&mut c, "v3.0 hex words addressed\n0x0003: 004 005  ignored text\n8: fff\n", None).unwrap();
        assert_eq!(c.get(0), 0); // loading replaces everything
        assert_eq!(c.get_range(3, 3), vec![4, 5, 0]);
        assert_eq!(c.get(8), 0xfff);
    }

    #[test]
    fn bytes_endianness() {
        let mut c = MemContents::new(4, 16);
        load_text(&mut c, "v3.0 hex bytes plain big-endian\n12 34 56 78\n", None).unwrap();
        assert_eq!(c.get_range(0, 2), vec![0x1234, 0x5678]);
        load_text(&mut c, "v3.0 hex bytes plain little-endian\n12 34 56 78\n", None).unwrap();
        assert_eq!(c.get_range(0, 2), vec![0x3412, 0x7856]);
        // odd word width: 9-bit big-endian
        let mut c = MemContents::new(4, 9);
        load_text(&mut c, "v3.0 hex bytes plain\nff 80\n", None).unwrap();
        assert_eq!(c.get(0), 0x1ff);
    }

    #[test]
    fn bytes_addressed_xxd_style() {
        let mut c = MemContents::new(5, 8);
        let text = "v3.0 hex bytes addressed\n00000000: 4865 6c6c 6f21 0a    Hello!.\n00000010: 7a\n";
        load_text(&mut c, text, None).unwrap();
        assert_eq!(c.get_range(0, 7), b"Hello!\n".iter().map(|&b| b as u64).collect::<Vec<_>>());
        assert_eq!(c.get(0x10), 0x7a);
        assert_eq!(c.get(8), 0);
    }

    #[test]
    fn save_then_load_round_trips_every_format() {
        let words: Vec<u64> = (0..40).map(|i| if i % 7 == 3 { 0 } else { (i * 37 + 5) & 0x3ff }).collect();
        let formats = [
            HexFormat::Raw,
            HexFormat::WordsPlain,
            HexFormat::WordsAddressed,
            HexFormat::BytesPlain(Endian::Big),
            HexFormat::BytesPlain(Endian::Little),
            HexFormat::BytesAddressed(Endian::Big),
            HexFormat::BytesAddressed(Endian::Little),
        ];
        for width in [10u8, 8, 16, 3, 1] {
            let mask = (1u64 << width) - 1;
            let src = mem(6, width, &words.iter().map(|w| w & mask).collect::<Vec<_>>());
            for f in formats {
                let text = save_text(&src, f);
                let mut dst = MemContents::new(6, width);
                load_text(&mut dst, &text, None).unwrap_or_else(|e| panic!("{f:?} w{width}: {e}\n{text}"));
                assert_eq!(dst.get_range(0, 64), src.get_range(0, 64), "{f:?} width {width}");
            }
            for e in [Endian::Big, Endian::Little] {
                let mut dst = MemContents::new(6, width);
                load_binary(&mut dst, &save_binary(&src, e), e).unwrap();
                assert_eq!(dst.get_range(0, 64), src.get_range(0, 64), "binary {e:?} width {width}");
            }
        }
    }

    #[test]
    fn errors_are_specific() {
        let mut c = MemContents::new(2, 8);
        let e = load_text(&mut c, "v2.0 raw\n1 2\n100\n", None).unwrap_err();
        assert!(e.contains("line 3") && e.contains("does not fit in 8 bits"), "{e}");
        let e = load_text(&mut c, "v2.0 raw\n1 2 3 4 5\n", None).unwrap_err();
        assert!(e.contains("more data"), "{e}");
        assert!(load_text(&mut c, "", None).is_err());
        assert!(load_text(&mut c, "hello\n1 2", None).unwrap_err().contains("unrecognized header"));
        assert!(load_text(&mut c, "v2.0 raw\nzz\n", None).unwrap_err().contains("not a hexadecimal"));
        assert!(load_text(&mut c, "v3.0 hex bytes plain\nabc\n", None).unwrap_err().contains("odd number"));
        // failed loads leave the memory untouched
        c.set(0, 9);
        let _ = load_text(&mut c, "v2.0 raw\n1 2 3 4 5\n", None);
        assert_eq!(c.get(0), 9);
    }

    #[test]
    fn works_on_ram_and_rom_contents() {
        use super::super::{Ram, RamOptions, Rom, RomOptions};
        let mut ram = Ram::new(RamOptions::default().addr_bits(4).data_bits(8));
        load_text(ram.contents_mut(), "v2.0 raw\n5 6 7\n", None).unwrap();
        assert_eq!(ram.contents().get(2), 7);
        let rom = Rom::new(RomOptions::default().addr_bits(4).data_bits(8));
        let mut rom = rom;
        load_text(rom.contents_mut(), "v3.0 hex words plain\n1 2\n", None).unwrap();
        assert!(save_text(rom.contents(), HexFormat::Raw).starts_with("v2.0 raw\n1 2"));
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("rcirc-hex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("img.hex");
        let src = mem(5, 12, &[1, 2, 0xabc, 0, 0, 0, 0, 0xfff]);
        save_file(&src, &p, HexFormat::WordsAddressed).unwrap();
        let mut dst = MemContents::new(5, 12);
        load_file(&mut dst, &p, None).unwrap();
        assert_eq!(dst.get_range(0, 8), src.get_range(0, 8));
        std::fs::remove_dir_all(&dir).ok();
    }
}
