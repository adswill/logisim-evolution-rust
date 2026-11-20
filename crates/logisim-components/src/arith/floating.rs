//! Floating-point parts (port of `std.arith.floating`).
//!
//! Operands are IEEE-754 values of 8 (Logisim "minifloat": 1-4-3, bias 7), 16, 32 or 64 bits. As in
//! the Java original every operation is computed in `f64` and rounded to the output format, and
//! each part has an `err` output that is high when the result is NaN.
//!
//! Port order for the calculation parts built on [`FpCalc`]: value outputs, then `err`
//! (-20,20), then inputs. Port 0 is always the primary output at (0,0). Inputs/outputs that are
//! not wired up are unknown, so they read as NaN and raise `err`.

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::{delay, raw, sext, Sign};
use crate::registry::PartDesc;

/// Supported floating-point widths.
pub const FP_WIDTHS: [u8; 4] = [8, 16, 32, 64];

fn check_width(w: u8) -> BitWidth {
    assert!(FP_WIDTHS.contains(&w), "floating-point width must be 8, 16, 32 or 64");
    BitWidth::new(w)
}

// ---- format conversion ------------------------------------------------------------------

/// `(exponent bits, mantissa bits)` of a supported width.
fn layout(width: u8) -> Option<(u32, u32)> {
    match width {
        8 => Some((4, 3)),
        16 => Some((5, 10)),
        32 => Some((8, 23)),
        64 => Some((11, 52)),
        _ => None,
    }
}

/// Decode a small IEEE-style float (`exp_bits` + `man_bits`, sign on top) to `f32`.
fn small_to_f32(bits: u64, exp_bits: u32, man_bits: u32) -> f32 {
    let bias = (1i32 << (exp_bits - 1)) - 1;
    let max_exp = (1u64 << exp_bits) - 1;
    let sign = (bits >> (exp_bits + man_bits)) & 1;
    let exp = (bits >> man_bits) & max_exp;
    let man = bits & ((1u64 << man_bits) - 1);
    let neg = if sign == 1 { -1.0f32 } else { 1.0 };
    if exp == 0 {
        neg * (man as f64 * 2f64.powi(1 - bias - man_bits as i32)) as f32
    } else if exp == max_exp {
        if man == 0 {
            neg * f32::INFINITY
        } else {
            f32::from_bits((sign as u32) << 31 | 0x7f80_0000 | (man as u32) << (23 - man_bits))
        }
    } else {
        f32::from_bits((sign as u32) << 31 | ((exp as i32 - bias + 127) as u32) << 23 | (man as u32) << (23 - man_bits))
    }
}

/// Encode an `f32` as a small IEEE-style float, rounding to nearest even.
fn f32_to_small(f: f32, exp_bits: u32, man_bits: u32) -> u64 {
    let bias = (1i32 << (exp_bits - 1)) - 1;
    let max_exp = (1u64 << exp_bits) - 1;
    let bits = f.to_bits();
    let sign = u64::from(bits >> 31) << (exp_bits + man_bits);
    let shift = 23 - man_bits;
    if f.is_nan() {
        let frac = bits & 0x7f_ffff;
        let man = u64::from(frac >> shift) | u64::from(frac & ((1 << shift) - 1) != 0);
        return sign | max_exp << man_bits | man;
    }
    if f.is_infinite() {
        return sign | max_exp << man_bits;
    }
    if f == 0.0 {
        return sign;
    }
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    if (bits >> 23) & 0xff == 0 {
        return sign; // f32 subnormal: far below the smallest small-float subnormal
    }
    let m = u64::from(bits & 0x7f_ffff | 1 << 23);
    let e_min = 1 - bias;
    let (s, subnormal) = if e < e_min { (shift as i32 + (e_min - e), true) } else { (shift as i32, false) };
    if s > 25 {
        return sign; // rounds to zero
    }
    let s = s as u32;
    let mut q = m >> s;
    let round = (m >> (s - 1)) & 1;
    let sticky = m & ((1 << (s - 1)) - 1) != 0;
    if round == 1 && (sticky || q & 1 == 1) {
        q += 1;
    }
    if subnormal {
        return sign | q;
    }
    if e + bias >= max_exp as i32 {
        return sign | max_exp << man_bits;
    }
    sign | ((((e + bias) as u64) << man_bits) + (q - (1 << man_bits)))
}

/// Decode a floating-point `Value` to `f64` (NaN if undefined or of an unsupported width).
pub fn fp_to_f64(v: &Value) -> f64 {
    let w = v.width().get();
    match (layout(w), v.is_fully_defined()) {
        (Some((e, m)), true) => match w {
            8 | 16 => f64::from(small_to_f32(raw(v), e, m)),
            32 => f64::from(f32::from_bits(raw(v) as u32)),
            _ => f64::from_bits(raw(v)),
        },
        _ => f64::NAN,
    }
}

/// Encode `x` in the given floating-point width (`Value::ERROR` for unsupported widths).
pub fn f64_to_fp(width: BitWidth, x: f64) -> Value {
    match width.get() {
        w @ (8 | 16) => {
            let (e, m) = layout(w).unwrap();
            Value::known(width, f32_to_small(x as f32, e, m))
        }
        32 => {
            let f = x as f32;
            Value::known(width, u64::from(if f.is_nan() { 0x7fc0_0000 } else { f.to_bits() }))
        }
        64 => Value::known(width, if x.is_nan() { 0x7ff8_0000_0000_0000 } else { x.to_bits() }),
        _ => Value::ERROR,
    }
}

fn flag(b: bool) -> Value {
    if b {
        Value::TRUE
    } else {
        Value::FALSE
    }
}

// ---- Java Math semantics that differ from Rust's ------------------------------------------

fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

fn java_pow(base: f64, exp: f64) -> f64 {
    if exp.is_nan() || (base.abs() == 1.0 && exp.is_infinite()) {
        f64::NAN
    } else {
        base.powf(exp)
    }
}

/// `Math.round`: ties toward positive infinity, saturating, NaN -> 0.
fn java_round(x: f64) -> i64 {
    let f = x.floor();
    (if x - f >= 0.5 { f + 1.0 } else { f }) as i64
}

/// `Math.IEEEremainder`.
fn ieee_remainder(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() || x.is_infinite() || y == 0.0 {
        return f64::NAN;
    }
    if y.is_infinite() || x == 0.0 {
        return x;
    }
    let ay = y.abs();
    let two = 2.0 * ay;
    let t = if two.is_finite() { x.abs() % two } else { x.abs() };
    let odd = t >= ay;
    let r = if odd { t - ay } else { t };
    let res = if r > ay - r || (r == ay - r && odd) { r - ay } else { r };
    if res == 0.0 { 0.0f64.copysign(x) } else if x < 0.0 { -res } else { res }
}

// ---- options ------------------------------------------------------------------------------

/// How a float is rounded to an integer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoundMode {
    Ceil,
    Floor,
    /// Round half up (default).
    #[default]
    Round,
    /// Round half to even.
    Rint,
    Truncate,
}

fn round_f64(mode: RoundMode, a: f64) -> f64 {
    match mode {
        RoundMode::Ceil => a.ceil(),
        RoundMode::Floor => a.floor(),
        RoundMode::Round => {
            if a.abs() < 4503599627370496.0 {
                (java_round(a) as f64).copysign(a)
            } else {
                a
            }
        }
        RoundMode::Rint => a.round_ties_even(),
        RoundMode::Truncate => a.trunc(),
    }
}

fn round_to_i64(mode: RoundMode, a: f64) -> i64 {
    match mode {
        RoundMode::Ceil => a.ceil() as i64,
        RoundMode::Floor => a.floor() as i64,
        RoundMode::Round => java_round(a),
        RoundMode::Rint => a.round_ties_even() as i64,
        RoundMode::Truncate => a as i64,
    }
}

/// Which exponential a [`FpCalc::exponentiator`] computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpMode {
    /// `base ^ exponent` with two inputs (default).
    #[default]
    Arbitrary,
    /// `e ^ x`.
    Exp,
    /// `e ^ x - 1`.
    ExpMinusOne,
}

/// Which logarithm a [`FpCalc::logarithm`] computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogMode {
    /// `log_base(x)` with two inputs (default).
    #[default]
    ArbitraryBase,
    /// `ln(x + 1)`.
    NaturalPlusOne,
    Natural,
    Base10,
}

/// Which trigonometric family a [`FpCalc::trigonometry`] computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrigMode {
    /// sin, tan, cos (default).
    #[default]
    Trig,
    /// asin, atan, acos.
    Inverse,
    /// sinh, tanh, cosh.
    Hyperbolic,
}

/// Whether the multiplier also adds a third input (fused multiply-add).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FpMulMode {
    #[default]
    Multiply,
    FusedMultiplyAdd,
}

// ---- generic calculation part -----------------------------------------------------------

type Pos = (i32, i32);

/// What an [`FpCalc`] computes (the variants with a mode mirror Logisim's mode attribute).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcKind {
    Adder,
    /// `A - B`.
    Subtractor,
    Multiplier(FpMulMode),
    /// Quotient and IEEE remainder.
    Divider,
    Negator,
    Absolute,
    SquareRoot,
    Exponentiator(ExpMode),
    Logarithm(LogMode),
    MinMax,
    Round(RoundMode),
    Trigonometry(TrigMode),
}

const MUL_MODES: [(&str, FpMulMode); 2] = [("multiply", FpMulMode::Multiply), ("fused_multiply_add", FpMulMode::FusedMultiplyAdd)];
const EXP_MODES: [(&str, ExpMode); 3] =
    [("arbitrary", ExpMode::Arbitrary), ("exp", ExpMode::Exp), ("exp_minus_one", ExpMode::ExpMinusOne)];
const LOG_MODES: [(&str, LogMode); 4] = [
    ("arbitrary_base", LogMode::ArbitraryBase),
    ("natural_plus_one", LogMode::NaturalPlusOne),
    ("natural", LogMode::Natural),
    ("base10", LogMode::Base10),
];
const ROUND_MODES: [(&str, RoundMode); 5] = [
    ("ceil", RoundMode::Ceil),
    ("floor", RoundMode::Floor),
    ("round", RoundMode::Round),
    ("rint", RoundMode::Rint),
    ("truncate", RoundMode::Truncate),
];
const TRIG_MODES: [(&str, TrigMode); 3] =
    [("trig", TrigMode::Trig), ("inverse", TrigMode::Inverse), ("hyperbolic", TrigMode::Hyperbolic)];

impl CalcKind {
    fn name(self) -> &'static str {
        match self {
            CalcKind::Adder => "FP Adder",
            CalcKind::Subtractor => "FP Subtractor",
            CalcKind::Multiplier(_) => "FP Multiplier",
            CalcKind::Divider => "FP Divider",
            CalcKind::Negator => "FP Negator",
            CalcKind::Absolute => "FP Absolute Value",
            CalcKind::SquareRoot => "FP Square Root",
            CalcKind::Exponentiator(_) => "FP Exponentiator",
            CalcKind::Logarithm(_) => "FP Logarithm",
            CalcKind::MinMax => "FP Min/Max",
            CalcKind::Round(_) => "FP Round",
            CalcKind::Trigonometry(_) => "FP Trigonometry",
        }
    }
    fn outs(self) -> &'static [Pos] {
        match self {
            CalcKind::Divider => &[(0, 0), (-10, 20)],
            CalcKind::MinMax => &[(0, -10), (0, 10)],
            CalcKind::Trigonometry(_) => &[(0, -10), (0, 0), (0, 10)],
            _ => &[(0, 0)],
        }
    }
    fn ins(self) -> &'static [Pos] {
        const ONE: &[Pos] = &[(-40, 0)];
        const TWO: &[Pos] = &[(-40, -10), (-40, 10)];
        match self {
            CalcKind::Multiplier(FpMulMode::FusedMultiplyAdd) => &[(-40, -10), (-40, 10), (-20, -20)],
            CalcKind::Adder
            | CalcKind::Subtractor
            | CalcKind::Multiplier(_)
            | CalcKind::Divider
            | CalcKind::MinMax
            | CalcKind::Exponentiator(ExpMode::Arbitrary)
            | CalcKind::Logarithm(LogMode::ArbitraryBase) => TWO,
            _ => ONE,
        }
    }
    fn eval(self, v: &[f64]) -> Vec<f64> {
        match self {
            CalcKind::Adder => vec![v[0] + v[1]],
            CalcKind::Subtractor => vec![v[0] - v[1]],
            CalcKind::Multiplier(FpMulMode::Multiply) => vec![v[0] * v[1]],
            CalcKind::Multiplier(FpMulMode::FusedMultiplyAdd) => vec![v[0].mul_add(v[1], v[2])],
            CalcKind::Divider => vec![v[0] / v[1], ieee_remainder(v[0], v[1])],
            CalcKind::Negator => vec![-v[0]],
            CalcKind::Absolute => vec![v[0].abs()],
            CalcKind::SquareRoot => vec![v[0].sqrt()],
            CalcKind::Exponentiator(ExpMode::Arbitrary) => vec![java_pow(v[0], v[1])],
            CalcKind::Exponentiator(ExpMode::Exp) => vec![v[0].exp()],
            CalcKind::Exponentiator(ExpMode::ExpMinusOne) => vec![v[0].exp_m1()],
            CalcKind::Logarithm(LogMode::ArbitraryBase) => vec![v[0].ln() / v[1].ln()],
            CalcKind::Logarithm(LogMode::NaturalPlusOne) => vec![v[0].ln_1p()],
            CalcKind::Logarithm(LogMode::Natural) => vec![v[0].ln()],
            CalcKind::Logarithm(LogMode::Base10) => vec![v[0].log10()],
            CalcKind::MinMax => vec![java_min(v[0], v[1]), java_max(v[0], v[1])],
            CalcKind::Round(m) => vec![round_f64(m, v[0])],
            CalcKind::Trigonometry(TrigMode::Trig) => vec![v[0].sin(), v[0].tan(), v[0].cos()],
            CalcKind::Trigonometry(TrigMode::Inverse) => vec![v[0].asin(), v[0].atan(), v[0].acos()],
            CalcKind::Trigonometry(TrigMode::Hyperbolic) => vec![v[0].sinh(), v[0].tanh(), v[0].cosh()],
        }
    }
    /// The `mode` option name, if this kind has one.
    fn mode_name(self) -> Option<&'static str> {
        Some(match self {
            CalcKind::Multiplier(m) => OptionMap::name_of(m, &MUL_MODES),
            CalcKind::Exponentiator(m) => OptionMap::name_of(m, &EXP_MODES),
            CalcKind::Logarithm(m) => OptionMap::name_of(m, &LOG_MODES),
            CalcKind::Round(m) => OptionMap::name_of(m, &ROUND_MODES),
            CalcKind::Trigonometry(m) => OptionMap::name_of(m, &TRIG_MODES),
            _ => return None,
        })
    }
    fn with_mode(self, m: &OptionMap) -> Result<Self, String> {
        Ok(match self {
            CalcKind::Multiplier(d) => CalcKind::Multiplier(m.choice("mode", &MUL_MODES)?.unwrap_or(d)),
            CalcKind::Exponentiator(d) => CalcKind::Exponentiator(m.choice("mode", &EXP_MODES)?.unwrap_or(d)),
            CalcKind::Logarithm(d) => CalcKind::Logarithm(m.choice("mode", &LOG_MODES)?.unwrap_or(d)),
            CalcKind::Round(d) => CalcKind::Round(m.choice("mode", &ROUND_MODES)?.unwrap_or(d)),
            CalcKind::Trigonometry(d) => CalcKind::Trigonometry(m.choice("mode", &TRIG_MODES)?.unwrap_or(d)),
            other => other,
        })
    }
}

/// Reads an optional floating-point width option.
fn fp_width_opt(m: &OptionMap, key: &str) -> Result<Option<BitWidth>, String> {
    match m.int(key, 1, 64)? {
        None => Ok(None),
        Some(w) if FP_WIDTHS.contains(&(w as u8)) => Ok(Some(BitWidth::new(w as u8))),
        Some(w) => Err(format!("option '{key}' = {w} must be one of {FP_WIDTHS:?}")),
    }
}

/// A floating-point part computing one or more `f64` results from its inputs.
///
/// Ports: the value outputs (first at (0,0)), `err` at (-20,20), then the inputs.
/// Default width 32; modes (where present) default to Logisim's defaults.
#[derive(Debug, Clone)]
pub struct FpCalc {
    kind: CalcKind,
    width: BitWidth,
}

impl FpCalc {
    pub fn new(kind: CalcKind, width: u8) -> Self {
        FpCalc { kind, width: check_width(width) }
    }
    /// Number of value outputs; `err` is the port after them.
    pub fn output_count(&self) -> usize {
        self.kind.outs().len()
    }
    pub fn kind(&self) -> CalcKind {
        self.kind
    }

    pub fn adder(w: u8) -> Self {
        Self::new(CalcKind::Adder, w)
    }
    /// Ports: 0 out, 1 err, 2 A, 3 B. Computes `A - B`.
    pub fn subtractor(w: u8) -> Self {
        Self::new(CalcKind::Subtractor, w)
    }
    /// The fused mode adds a third input C at (-20,-20): ports 0 out, 1 err, 2 A, 3 B, 4 C.
    pub fn multiplier(w: u8, mode: FpMulMode) -> Self {
        Self::new(CalcKind::Multiplier(mode), w)
    }
    /// Ports: 0 quotient, 1 IEEE remainder (-10,20), 2 err, 3 A, 4 B.
    pub fn divider(w: u8) -> Self {
        Self::new(CalcKind::Divider, w)
    }
    pub fn negator(w: u8) -> Self {
        Self::new(CalcKind::Negator, w)
    }
    pub fn absolute(w: u8) -> Self {
        Self::new(CalcKind::Absolute, w)
    }
    pub fn square_root(w: u8) -> Self {
        Self::new(CalcKind::SquareRoot, w)
    }
    /// Arbitrary mode ports: 0 out, 1 err, 2 base (-40,-10), 3 exponent (-40,10); other modes: 2 input.
    pub fn exponentiator(w: u8, mode: ExpMode) -> Self {
        Self::new(CalcKind::Exponentiator(mode), w)
    }
    /// Arbitrary-base ports: 0 out, 1 err, 2 value (-40,-10), 3 base (-40,10); other modes: 2 input.
    pub fn logarithm(w: u8, mode: LogMode) -> Self {
        Self::new(CalcKind::Logarithm(mode), w)
    }
    /// Ports: 0 min (0,-10), 1 max (0,10), 2 err, 3 A, 4 B.
    pub fn min_max(w: u8) -> Self {
        Self::new(CalcKind::MinMax, w)
    }
    pub fn round(w: u8, mode: RoundMode) -> Self {
        Self::new(CalcKind::Round(mode), w)
    }
    /// Ports: 0 sin (0,-10), 1 tan (0,0), 2 cos (0,10), 3 err, 4 input (-40,0). Inverse/hyperbolic
    /// modes compute asin/atan/acos or sinh/tanh/cosh on the same ports.
    pub fn trigonometry(w: u8, mode: TrigMode) -> Self {
        Self::new(CalcKind::Trigonometry(mode), w)
    }
}

impl Component for FpCalc {
    fn name(&self) -> &str {
        self.kind.name()
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        let mut specs = vec![super::fp_width_spec("width")];
        let mode = match self.kind {
            CalcKind::Multiplier(_) => Some((&["multiply", "fused_multiply_add"][..], "multiply")),
            CalcKind::Exponentiator(_) => Some((&["arbitrary", "exp", "exp_minus_one"][..], "arbitrary")),
            CalcKind::Logarithm(_) => {
                Some((&["arbitrary_base", "natural_plus_one", "natural", "base10"][..], "arbitrary_base"))
            }
            CalcKind::Round(_) => Some((&["ceil", "floor", "round", "rint", "truncate"][..], "round")),
            CalcKind::Trigonometry(_) => Some((&["trig", "inverse", "hyperbolic"][..], "trig")),
            _ => None,
        };
        if let Some((names, default)) = mode {
            specs.push(OptionSpec::choice("mode", "Mode", names, default));
        }
        specs
    }
    fn options(&self) -> OptionMap {
        let m = OptionMap::new().with("width", self.width.get());
        match self.kind.mode_name() {
            Some(mode) => m.with("mode", mode),
            None => m,
        }
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(if self.kind.mode_name().is_some() { &["width", "mode"] } else { &["width"] })?;
        let kind = self.kind.with_mode(m)?;
        let width = fp_width_opt(m, "width")?.unwrap_or(self.width);
        (self.kind, self.width) = (kind, width);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let mut ps: Vec<Port> =
            self.kind.outs().iter().map(|&(x, y)| Port::new(x, y, PortKind::Output, self.width)).collect();
        ps.push(Port::new(-20, 20, PortKind::Output, BitWidth::ONE));
        ps.extend(self.kind.ins().iter().map(|&(x, y)| Port::new(x, y, PortKind::Input, self.width)));
        ps
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let n = self.output_count();
        let args: Vec<f64> = (0..self.kind.ins().len()).map(|i| fp_to_f64(&ctx.port_value(n + 1 + i))).collect();
        let results = self.kind.eval(&args);
        let d = delay(self.width.get() as usize + 2);
        for (i, r) in results.iter().enumerate() {
            ctx.set_port(i, f64_to_fp(self.width, *r), d);
        }
        ctx.set_port(n, flag(results.iter().any(|r| r.is_nan())), d);
    }
}

// ---- other parts --------------------------------------------------------------------------

/// A constant floating-point value. Port 0 out (0,0). Defaults: 32 bits, 0.0.
#[derive(Debug, Clone)]
pub struct FpConstant {
    width: BitWidth,
    value: f64,
}

impl Default for FpConstant {
    fn default() -> Self {
        FpConstant::new(32, 0.0)
    }
}

impl FpConstant {
    pub fn new(width: u8, value: f64) -> Self {
        FpConstant { width: check_width(width), value }
    }
}

impl Component for FpConstant {
    fn name(&self) -> &str {
        "FP Constant"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::fp_width_spec("width"), OptionSpec::string("value", "Value", "0")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get()).with("value", self.value.to_string())
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "value"])?;
        let value = match m.string("value")? {
            None => self.value,
            Some(t) => t.trim().parse::<f64>().map_err(|_| format!("option 'value' = '{t}' is not a number"))?,
        };
        self.width = fp_width_opt(m, "width")?.unwrap_or(self.width);
        self.value = value;
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, f64_to_fp(self.width, self.value), 1);
    }
}

/// Compares two floats. Ports: 0 `=` (0,0); 1 `>` (0,-10); 2 `<` (0,10); 3 err (-20,20) (either
/// input NaN); 4 A (-40,-10); 5 B (-40,10). Default 32 bits.
#[derive(Debug, Clone)]
pub struct FpComparator {
    width: BitWidth,
}

impl Default for FpComparator {
    fn default() -> Self {
        FpComparator::new(32)
    }
}

impl FpComparator {
    pub fn new(width: u8) -> Self {
        FpComparator { width: check_width(width) }
    }
}

impl Component for FpComparator {
    fn name(&self) -> &str {
        "FP Comparator"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::fp_width_spec("width")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        self.width = fp_width_opt(m, "width")?.unwrap_or(self.width);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let one = BitWidth::ONE;
        vec![
            Port::new(0, 0, PortKind::Output, one),
            Port::new(0, -10, PortKind::Output, one),
            Port::new(0, 10, PortKind::Output, one),
            Port::new(-20, 20, PortKind::Output, one),
            Port::new(-40, -10, PortKind::Input, self.width),
            Port::new(-40, 10, PortKind::Input, self.width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (a, b) = (fp_to_f64(&ctx.port_value(4)), fp_to_f64(&ctx.port_value(5)));
        let d = delay(self.width.get() as usize + 2);
        ctx.set_port(0, flag(a == b), d);
        ctx.set_port(1, flag(a > b), d);
        ctx.set_port(2, flag(a < b), d);
        ctx.set_port(3, flag(a.is_nan() || b.is_nan()), d);
    }
}

/// Classifies a float. Ports (outputs 1 bit each): 0 normal (0,0); 1 negative (0,-30); 2 zero
/// (0,-20); 3 subnormal (0,-10); 4 infinite (0,10); 5 NaN (0,30); 6 input (-40,0). Default 32 bits.
#[derive(Debug, Clone)]
pub struct FpClassifier {
    width: BitWidth,
}

impl Default for FpClassifier {
    fn default() -> Self {
        FpClassifier::new(32)
    }
}

impl FpClassifier {
    pub fn new(width: u8) -> Self {
        FpClassifier { width: check_width(width) }
    }
}

impl Component for FpClassifier {
    fn name(&self) -> &str {
        "FP Classifier"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::fp_width_spec("width")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        self.width = fp_width_opt(m, "width")?.unwrap_or(self.width);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let one = BitWidth::ONE;
        let mut ps: Vec<Port> =
            [(0, 0), (0, -30), (0, -20), (0, -10), (0, 10), (0, 30)].iter().map(|&(x, y)| Port::new(x, y, PortKind::Output, one)).collect();
        ps.push(Port::new(-40, 0, PortKind::Input, self.width));
        ps
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let a = ctx.port_value(6);
        let bits = raw(&a);
        let w = self.width.get();
        let (e, m) = layout(w).unwrap();
        let exp = (bits >> m) & ((1 << e) - 1);
        let frac = bits & ((1 << m) - 1);
        let x = fp_to_f64(&a);
        let d = delay(w as usize + 2);
        ctx.set_port(0, flag(exp > 0 && exp < (1 << e) - 1), d);
        ctx.set_port(1, flag((bits >> (w - 1)) & 1 == 1), d);
        ctx.set_port(2, flag(x == 0.0), d);
        ctx.set_port(3, flag(exp == 0 && frac != 0), d);
        ctx.set_port(4, flag(x.is_infinite()), d);
        ctx.set_port(5, flag(x.is_nan()), d);
    }
}

/// Converts between floating-point widths. Ports: 0 out (0,0); 1 err (-20,20); 2 in (-40,0).
/// Default 32 -> 32.
#[derive(Debug, Clone)]
pub struct FpToFp {
    from: BitWidth,
    to: BitWidth,
}

impl Default for FpToFp {
    fn default() -> Self {
        FpToFp::new(32, 32)
    }
}

impl FpToFp {
    pub fn new(from: u8, to: u8) -> Self {
        FpToFp { from: check_width(from), to: check_width(to) }
    }
}

impl Component for FpToFp {
    fn name(&self) -> &str {
        "FP to FP"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::fp_width_spec("from_width"), super::fp_width_spec("to_width")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("from_width", self.from.get()).with("to_width", self.to.get())
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["from_width", "to_width"])?;
        let (from, to) = (fp_width_opt(m, "from_width")?, fp_width_opt(m, "to_width")?);
        self.from = from.unwrap_or(self.from);
        self.to = to.unwrap_or(self.to);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Output, self.to),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, 0, PortKind::Input, self.from),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let a = fp_to_f64(&ctx.port_value(2));
        let d = delay(self.to.get() as usize + 2);
        ctx.set_port(0, f64_to_fp(self.to, a), d);
        ctx.set_port(1, flag(a.is_nan()), d);
    }
}

/// Floating point to integer. Ports: 0 integer out (0,0); 1 err (-20,20); 2 float in (-40,0).
/// Defaults: 8-bit integer, 32-bit float, round half up.
#[derive(Debug, Clone)]
pub struct FpToInt {
    int_width: BitWidth,
    fp_width: BitWidth,
    mode: RoundMode,
}

impl Default for FpToInt {
    fn default() -> Self {
        FpToInt::new(8, 32, RoundMode::Round)
    }
}

impl FpToInt {
    pub fn new(int_width: u8, fp_width: u8, mode: RoundMode) -> Self {
        assert!((1..=64).contains(&int_width), "integer width must be 1..=64");
        FpToInt { int_width: BitWidth::new(int_width), fp_width: check_width(fp_width), mode }
    }
}

impl Component for FpToInt {
    fn name(&self) -> &str {
        "FP to Int"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::int("int_width", "Integer bits", 1, 64, 8), super::fp_width_spec("fp_width"), OptionSpec::choice("mode", "Rounding", &["ceil", "floor", "round", "rint", "truncate"], "round")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("int_width", self.int_width.get())
            .with("fp_width", self.fp_width.get())
            .with("mode", OptionMap::name_of(self.mode, &ROUND_MODES))
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["int_width", "fp_width", "mode"])?;
        let mode = m.choice("mode", &ROUND_MODES)?.unwrap_or(self.mode);
        let int_width = m.int("int_width", 1, 64)?.map_or(self.int_width, |w| BitWidth::new(w as u8));
        self.fp_width = fp_width_opt(m, "fp_width")?.unwrap_or(self.fp_width);
        (self.mode, self.int_width) = (mode, int_width);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Output, self.int_width),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, 0, PortKind::Input, self.fp_width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let a = fp_to_f64(&ctx.port_value(2));
        let d = delay(self.int_width.get() as usize + 2);
        ctx.set_port(0, Value::known(self.int_width, round_to_i64(self.mode, a) as u64), d);
        ctx.set_port(1, flag(a.is_nan()), d);
    }
}

/// Integer to floating point. Ports: 0 float out (0,0); 1 err (-20,20); 2 integer in (-40,0).
/// Defaults: 8-bit signed integer, 32-bit float.
#[derive(Debug, Clone)]
pub struct IntToFp {
    int_width: BitWidth,
    fp_width: BitWidth,
    sign: Sign,
}

impl Default for IntToFp {
    fn default() -> Self {
        IntToFp::new(8, 32, Sign::Signed)
    }
}

impl IntToFp {
    pub fn new(int_width: u8, fp_width: u8, sign: Sign) -> Self {
        assert!((1..=64).contains(&int_width), "integer width must be 1..=64");
        IntToFp { int_width: BitWidth::new(int_width), fp_width: check_width(fp_width), sign }
    }
}

impl Component for IntToFp {
    fn name(&self) -> &str {
        "Int to FP"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::int("int_width", "Integer bits", 1, 64, 8), super::fp_width_spec("fp_width"), super::sign_spec("signed")]
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("int_width", self.int_width.get())
            .with("fp_width", self.fp_width.get())
            .with("sign", OptionMap::name_of(self.sign, &super::SIGNS))
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["int_width", "fp_width", "sign"])?;
        let sign = m.choice("sign", &super::SIGNS)?.unwrap_or(self.sign);
        let int_width = m.int("int_width", 1, 64)?.map_or(self.int_width, |w| BitWidth::new(w as u8));
        self.fp_width = fp_width_opt(m, "fp_width")?.unwrap_or(self.fp_width);
        (self.sign, self.int_width) = (sign, int_width);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Output, self.fp_width),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, 0, PortKind::Input, self.int_width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let a = ctx.port_value(2);
        let x = if !a.is_fully_defined() {
            f64::NAN
        } else if self.sign == Sign::Unsigned {
            raw(&a) as f64
        } else {
            sext(raw(&a), self.int_width.get()) as f64
        };
        let d = delay(self.int_width.get() as usize + 2);
        ctx.set_port(0, f64_to_fp(self.fp_width, x), d);
        ctx.set_port(1, flag(x.is_nan()), d);
    }
}

/// The floating-point parts, with Logisim's default options, for the library menu.
pub fn parts() -> Vec<PartDesc> {
    let p = |name, make: fn() -> Box<dyn Component>| PartDesc { name, make };
    vec![
        p("FP Constant", || Box::new(FpConstant::default())),
        p("FP Adder", || Box::new(FpCalc::adder(32))),
        p("FP Subtractor", || Box::new(FpCalc::subtractor(32))),
        p("FP Multiplier", || Box::new(FpCalc::multiplier(32, FpMulMode::default()))),
        p("FP Divider", || Box::new(FpCalc::divider(32))),
        p("FP Negator", || Box::new(FpCalc::negator(32))),
        p("FP Exponentiator", || Box::new(FpCalc::exponentiator(32, ExpMode::default()))),
        p("FP Logarithm", || Box::new(FpCalc::logarithm(32, LogMode::default()))),
        p("FP Square Root", || Box::new(FpCalc::square_root(32))),
        p("FP Absolute Value", || Box::new(FpCalc::absolute(32))),
        p("FP Comparator", || Box::new(FpComparator::default())),
        p("FP Min/Max", || Box::new(FpCalc::min_max(32))),
        p("FP Round", || Box::new(FpCalc::round(32, RoundMode::default()))),
        p("FP Trigonometry", || Box::new(FpCalc::trigonometry(32, TrigMode::default()))),
        p("FP Classifier", || Box::new(FpClassifier::default())),
        p("FP to FP", || Box::new(FpToFp::default())),
        p("FP to Int", || Box::new(FpToInt::default())),
        p("Int to FP", || Box::new(IntToFp::default())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::testutil::{eval, k};

    fn fp(w: u8, x: f64) -> Value {
        f64_to_fp(BitWidth::new(w), x)
    }
    fn f(v: &Value) -> f64 {
        fp_to_f64(v)
    }

    #[test]
    fn format_conversions() {
        assert_eq!(fp(16, 1.0), k(16, 0x3C00));
        assert_eq!(fp(16, 1.5), k(16, 0x3E00));
        assert_eq!(fp(16, -2.0), k(16, 0xC000));
        assert_eq!(fp(16, 65504.0), k(16, 0x7BFF));
        assert_eq!(fp(16, 65520.0), k(16, 0x7C00)); // rounds up to infinity
        assert_eq!(fp(16, 1e-8), k(16, 0)); // underflow
        assert_eq!(fp(16, 2f64.powi(-24)), k(16, 1)); // smallest subnormal
        assert_eq!(fp(16, 1.0 + 2f64.powi(-11)), k(16, 0x3C00)); // tie rounds to even
        assert_eq!(fp(16, 1.0 + 3.0 * 2f64.powi(-11)), k(16, 0x3C02));
        assert_eq!(f(&k(16, 0x0001)), 2f64.powi(-24));
        assert_eq!(f(&k(16, 0x7C00)), f64::INFINITY);
        assert!(f(&k(16, 0x7E00)).is_nan());
        assert_eq!(fp(8, 1.0), k(8, 0x38));
        assert_eq!(fp(8, 240.0), k(8, 0x77));
        assert_eq!(fp(8, 256.0), k(8, 0x78)); // infinity
        assert_eq!(f(&k(8, 0x38)), 1.0);
        assert_eq!(f(&k(8, 0x01)), 2f64.powi(-9));
        assert_eq!(fp(32, 1.0), k(32, 0x3F80_0000));
        assert_eq!(fp(64, -1.0), k(64, 0xBFF0_0000_0000_0000));
        assert_eq!(fp(32, f64::NAN), k(32, 0x7FC0_0000));
        // every fp8 and fp16 pattern survives a round trip (NaNs stay NaN)
        for w in [8u8, 16] {
            for bits in 0..(1u64 << w) {
                let v = k(w, bits);
                let x = f(&v);
                let back = fp(w, x);
                if x.is_nan() {
                    assert!(f(&back).is_nan(), "w={w} bits={bits:#x}");
                } else {
                    assert_eq!(back, v, "w={w} bits={bits:#x}");
                }
            }
        }
        assert!(f(&Value::create(32, 0, 1, 0)).is_nan());
    }

    fn run(c: FpCalc, ins: &[f64], outs: usize) -> Vec<f64> {
        let w = c.width.get();
        let n = c.output_count();
        let inputs: Vec<(usize, Value)> = ins.iter().enumerate().map(|(i, x)| (n + 1 + i, fp(w, *x))).collect();
        let ports: Vec<usize> = (0..outs).collect();
        eval(c, &inputs, &ports).iter().map(f).collect()
    }
    fn err_of(c: FpCalc, ins: &[f64]) -> Value {
        let w = c.width.get();
        let n = c.output_count();
        let inputs: Vec<(usize, Value)> = ins.iter().enumerate().map(|(i, x)| (n + 1 + i, fp(w, *x))).collect();
        eval(c, &inputs, &[n])[0]
    }

    #[test]
    fn arithmetic() {
        assert_eq!(run(FpCalc::adder(32), &[1.5, 2.25], 1), vec![3.75]);
        assert_eq!(run(FpCalc::subtractor(32), &[1.5, 2.25], 1), vec![-0.75]);
        assert_eq!(run(FpCalc::multiplier(32, FpMulMode::Multiply), &[1.5, 2.0], 1), vec![3.0]);
        assert_eq!(run(FpCalc::multiplier(64, FpMulMode::FusedMultiplyAdd), &[2.0, 3.0, 1.0], 1), vec![7.0]);
        assert_eq!(run(FpCalc::divider(32), &[7.0, 2.0], 2), vec![3.5, -1.0]); // IEEE remainder: 7/2 = 3.5 ties to even (4)
        assert_eq!(run(FpCalc::negator(16), &[1.5], 1), vec![-1.5]);
        assert_eq!(run(FpCalc::absolute(16), &[-1.5], 1), vec![1.5]);
        assert_eq!(run(FpCalc::square_root(64), &[2.25], 1), vec![1.5]);
        assert_eq!(run(FpCalc::min_max(32), &[1.0, -3.0], 2), vec![-3.0, 1.0]);
        assert_eq!(run(FpCalc::exponentiator(32, ExpMode::Arbitrary), &[2.0, 10.0], 1), vec![1024.0]);
        assert_eq!(run(FpCalc::exponentiator(64, ExpMode::Exp), &[0.0], 1), vec![1.0]);
        assert_eq!(run(FpCalc::logarithm(32, LogMode::ArbitraryBase), &[8.0, 2.0], 1), vec![3.0]);
        assert_eq!(run(FpCalc::logarithm(32, LogMode::Base10), &[1000.0], 1)[0].round(), 3.0);
        assert_eq!(run(FpCalc::trigonometry(64, TrigMode::Trig), &[0.0], 3), vec![0.0, 0.0, 1.0]);
        assert_eq!(run(FpCalc::trigonometry(64, TrigMode::Hyperbolic), &[0.0], 3), vec![0.0, 0.0, 1.0]);
    }

    #[test]
    fn rounding_modes() {
        let r = |m, x| run(FpCalc::round(32, m), &[x], 1)[0];
        assert_eq!(r(RoundMode::Ceil, 1.2), 2.0);
        assert_eq!(r(RoundMode::Floor, -1.2), -2.0);
        assert_eq!(r(RoundMode::Round, 2.5), 3.0);
        assert_eq!(r(RoundMode::Round, -2.5), -2.0 - 0.0); // Java: Math.round(-2.5) = -2
        assert_eq!(r(RoundMode::Rint, 2.5), 2.0);
        assert_eq!(r(RoundMode::Truncate, -1.9), -1.0);
    }

    #[test]
    fn error_flag_on_nan() {
        assert_eq!(err_of(FpCalc::divider(32), &[0.0, 0.0]), Value::TRUE);
        assert_eq!(err_of(FpCalc::adder(32), &[1.0, 2.0]), Value::FALSE);
        assert_eq!(err_of(FpCalc::square_root(32), &[-1.0]), Value::TRUE);
        assert_eq!(err_of(FpCalc::divider(32), &[1.0, 0.0]), Value::TRUE); // 1/0 = inf but the remainder is NaN
        assert_eq!(err_of(FpCalc::adder(32), &[f64::INFINITY, 1.0]), Value::FALSE); // infinity is not NaN
        // an unconnected input is NaN
        let o = eval(FpCalc::adder(32), &[(2, fp(32, 1.0))], &[1]);
        assert_eq!(o[0], Value::TRUE);
    }

    #[test]
    fn ieee_remainder_matches_java() {
        assert_eq!(ieee_remainder(5.0, 3.0), -1.0);
        assert_eq!(ieee_remainder(4.0, 3.0), 1.0);
        assert_eq!(ieee_remainder(2.5, 1.0), 0.5);
        assert_eq!(ieee_remainder(-7.0, 2.0), 1.0);
        assert!(ieee_remainder(1.0, 0.0).is_nan());
        assert_eq!(ieee_remainder(3.0, f64::INFINITY), 3.0);
    }

    #[test]
    fn comparator_constant_classifier() {
        let cmp = |a, b| {
            eval(FpComparator::default(), &[(4, fp(32, a)), (5, fp(32, b))], &[0, 1, 2, 3])
        };
        let (t, fl) = (Value::TRUE, Value::FALSE);
        assert_eq!(cmp(1.0, 2.0), vec![fl, fl, t, fl]);
        assert_eq!(cmp(2.0, 2.0), vec![t, fl, fl, fl]);
        assert_eq!(cmp(f64::NAN, 2.0), vec![fl, fl, fl, t]);
        let o = eval(FpConstant::new(16, 1.5), &[], &[0]);
        assert_eq!(o[0], k(16, 0x3E00));

        let class = |w, x| eval(FpClassifier::new(w), &[(6, fp(w, x))], &[0, 1, 2, 3, 4, 5]);
        assert_eq!(class(32, 1.0), vec![t, fl, fl, fl, fl, fl]);
        assert_eq!(class(32, -0.0), vec![fl, t, t, fl, fl, fl]);
        assert_eq!(class(32, f64::NEG_INFINITY), vec![fl, t, fl, fl, t, fl]);
        assert_eq!(class(32, f64::NAN), vec![fl, fl, fl, fl, fl, t]);
        assert_eq!(eval(FpClassifier::new(16), &[(6, k(16, 1))], &[3])[0], t); // subnormal
    }

    #[test]
    fn conversions() {
        let o = eval(FpToFp::new(32, 16), &[(2, fp(32, 1.5))], &[0, 1]);
        assert_eq!(o, vec![k(16, 0x3E00), Value::FALSE]);
        let o = eval(FpToInt::new(8, 32, RoundMode::Truncate), &[(2, fp(32, -3.7))], &[0]);
        assert_eq!(o[0], k(8, 0xFD));
        let o = eval(FpToInt::default(), &[(2, fp(32, 2.5))], &[0]);
        assert_eq!(o[0], k(8, 3));
        let o = eval(FpToInt::default(), &[(2, fp(32, f64::NAN))], &[0, 1]);
        assert_eq!(o, vec![k(8, 0), Value::TRUE]);
        let o = eval(IntToFp::default(), &[(2, k(8, 0xFE))], &[0]);
        assert_eq!(f(&o[0]), -2.0);
        let o = eval(IntToFp::new(8, 32, Sign::Unsigned), &[(2, k(8, 0xFE))], &[0]);
        assert_eq!(f(&o[0]), 254.0);
        let o = eval(IntToFp::default(), &[], &[1]);
        assert_eq!(o[0], Value::TRUE);
    }
}
