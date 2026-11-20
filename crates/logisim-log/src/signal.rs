//! A recorded signal: what it is ([`SignalInfo`]) and its value history ([`Signal`]).
//!
//! History is run-length encoded as `(value, duration)` segments, optionally bounded to the most
//! recent `max_size` segments; dropping old segments advances the signal's start time.

use std::collections::VecDeque;

use logisim_core::{BitWidth, Value};

use crate::radix::Radix;

/// Opaque id of where a signal's value comes from (the model's `fetch` callback resolves it,
/// e.g. to a component index in the circuit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub usize);

/// Description of a loggable signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInfo {
    pub source: SourceId,
    /// Path-like display name, e.g. `sub/Q[3..0]`.
    pub name: String,
    pub width: BitWidth,
    pub radix: Radix,
    /// True for input pins (shown before the other signals).
    pub is_input: bool,
}

impl SignalInfo {
    pub fn new(source: SourceId, name: &str, width: u8) -> Self {
        SignalInfo {
            source,
            name: name.to_string(),
            width: BitWidth::new(width),
            radix: Radix::Binary,
            is_input: false,
        }
    }
    pub fn input(mut self, is_input: bool) -> Self {
        self.is_input = is_input;
        self
    }
    pub fn radix(mut self, r: Radix) -> Self {
        self.radix = r;
        self
    }
    /// Display name with the bit range appended for buses (`name[7..0]`).
    pub fn display_name(&self) -> String {
        if self.width.get() > 1 {
            format!("{}[{}..0]", self.name, self.width.get() - 1)
        } else {
            self.name.clone()
        }
    }
    pub fn format(&self, v: &Value) -> String {
        self.radix.format(v)
    }
}

/// The history of one signal.
#[derive(Debug, Clone)]
pub struct Signal {
    pub info: SignalInfo,
    time_start: u64,
    /// `(value, duration)` segments, oldest first.
    segments: VecDeque<(Value, u64)>,
    /// Most recent value recorded (to merge equal neighbours).
    last: Option<Value>,
    /// Maximum number of segments, or 0 for unlimited.
    max_size: usize,
}

impl Signal {
    /// A signal that starts at `time_start` with `initial` lasting `duration`.
    pub fn new(
        info: SignalInfo,
        initial: Value,
        duration: u64,
        time_start: u64,
        max_size: usize,
    ) -> Self {
        let mut s = Signal {
            info,
            time_start,
            segments: VecDeque::new(),
            last: None,
            max_size,
        };
        s.extend(Some(initial), duration);
        s
    }

    /// Time of the first retained sample if old data was dropped, otherwise 0.
    pub fn omitted_data_time(&self) -> u64 {
        if self.segments.len() == self.max_size {
            self.time_start
        } else {
            0
        }
    }
    pub fn start_time(&self) -> u64 {
        self.time_start
    }
    pub fn end_time(&self) -> u64 {
        self.time_start + self.segments.iter().map(|(_, d)| d).sum::<u64>()
    }
    /// Number of stored segments.
    pub fn len(&self) -> usize {
        self.segments.len()
    }
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
    /// The stored `(value, duration)` segments, oldest first.
    pub fn segments(&self) -> impl Iterator<Item = (Value, u64)> + '_ {
        self.segments.iter().copied()
    }

    /// Hold the current value for `duration` more time units (before any value was recorded the
    /// start time moves instead).
    pub fn extend_duration(&mut self, duration: u64) {
        match (self.last, self.segments.back_mut()) {
            (Some(_), Some(seg)) => seg.1 += duration,
            _ => self.time_start += duration,
        }
    }

    /// Record `v` for `duration` (`None` just extends the current value). Equal consecutive
    /// values are merged into one segment.
    pub fn extend(&mut self, v: Option<Value>, duration: u64) {
        let Some(v) = v else {
            return self.extend_duration(duration);
        };
        if self.last == Some(v) {
            if let Some(seg) = self.segments.back_mut() {
                seg.1 += duration;
                return;
            }
        }
        self.last = Some(v);
        if self.max_size > 0 && self.segments.len() >= self.max_size {
            if let Some((_, d)) = self.segments.pop_front() {
                self.time_start += d;
            }
        }
        self.segments.push_back((v, duration));
    }

    /// Replace the most recent `duration` of history with `v` (used by coarse clocked logging,
    /// where each step supersedes the previous one within the same clock period).
    pub fn replace_recent(&mut self, v: Option<Value>, duration: u64) {
        let (Some(v), Some(_)) = (v, self.last) else {
            return self.extend(v, duration);
        };
        let Some(&(_, last_dur)) = self.segments.back() else {
            return self.extend(Some(v), duration);
        };
        let n = self.segments.len();
        if last_dur == duration {
            self.segments[n - 1].0 = v;
            self.last = Some(v);
            if n > 1 && self.segments[n - 2].0 == v {
                self.segments[n - 2].1 += duration;
                self.segments.pop_back();
            }
        } else if last_dur > duration {
            self.segments[n - 1].1 -= duration;
            self.extend(Some(v), duration);
        } else if n == 1 && last_dur + self.time_start >= duration {
            self.time_start -= duration - last_dur;
            self.segments[0] = (v, duration);
            self.last = Some(v);
        } else {
            let mut remaining = duration;
            while remaining > 0 {
                let Some(&(_, d)) = self.segments.back() else {
                    break;
                };
                if d <= remaining {
                    remaining -= d;
                    self.segments.pop_back();
                } else {
                    self.segments.back_mut().unwrap().1 = d - remaining;
                    remaining = 0;
                }
            }
            if self.segments.is_empty() {
                self.time_start = self.time_start.saturating_sub(remaining);
            }
            self.last = None;
            self.extend(Some(v), duration);
        }
    }

    /// Change the maximum number of stored segments (0 = unlimited); excess old data is dropped.
    pub fn resize(&mut self, new_max: usize) {
        if new_max > 0 {
            while self.segments.len() > new_max {
                if let Some((_, d)) = self.segments.pop_front() {
                    self.time_start += d;
                }
            }
        }
        self.max_size = new_max;
    }

    /// Forget all history and start over with `v` lasting `duration`, from time 0.
    pub fn reset(&mut self, v: Value, duration: u64) {
        self.segments.clear();
        self.last = None;
        self.time_start = 0;
        self.extend(Some(v), duration);
    }

    /// Value at time `t`, widened with zeros to the signal's current width (`None` outside the
    /// recorded range).
    pub fn value_at(&self, t: u64) -> Option<Value> {
        self.segment_at(t).map(|(v, _)| v)
    }

    /// `(value, time remaining in its segment)` at time `t`.
    pub fn segment_at(&self, t: u64) -> Option<(Value, u64)> {
        if t < self.time_start {
            return None;
        }
        let mut start = self.time_start;
        for &(v, d) in &self.segments {
            if t < start + d {
                return Some((
                    v.extend_width(self.info.width.get(), Value::FALSE),
                    start + d - t,
                ));
            }
            start += d;
        }
        None
    }

    pub fn formatted_value_at(&self, t: u64) -> String {
        self.value_at(t)
            .map_or_else(|| "-".to_string(), |v| self.info.format(&v))
    }
    pub fn formatted_max_value(&self) -> String {
        self.info.format(&Value::known(self.info.width, u64::MAX))
    }
    pub fn formatted_min_value(&self) -> String {
        self.info.format(&Value::known(self.info.width, 0))
    }
    pub fn name(&self) -> String {
        self.info.display_name()
    }
    pub fn width(&self) -> BitWidth {
        self.info.width
    }

    /// A cursor positioned at the start of the recorded data.
    pub fn cursor(&self) -> Cursor {
        Cursor::at(self, self.time_start)
    }
}

/// A read position in a [`Signal`]; re-derives its position from time, so it stays valid when
/// old history is dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    /// Current time.
    pub time: u64,
    /// Time left in the current segment (0 when past the data).
    pub duration: u64,
    /// Value at the cursor (`None` when there is no data at `time`).
    pub value: Option<Value>,
}

impl Cursor {
    pub fn at(s: &Signal, t: u64) -> Cursor {
        match s.segment_at(t) {
            Some((v, d)) => Cursor {
                time: t,
                duration: d,
                value: Some(v),
            },
            None => Cursor {
                time: t,
                duration: 0,
                value: None,
            },
        }
    }
    pub fn formatted_value(&self, s: &Signal) -> String {
        self.value
            .map_or_else(|| "-".to_string(), |v| s.info.format(&v))
    }
    /// Move forward by `dt`; false when the data runs out.
    pub fn advance(&mut self, s: &Signal, dt: u64) -> bool {
        if self.value.is_none() {
            return false;
        }
        *self = Cursor::at(s, self.time + dt);
        self.value.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(w: u8) -> SignalInfo {
        SignalInfo::new(SourceId(0), "x", w)
    }
    fn v(x: u64) -> Value {
        Value::known(BitWidth::new(4), x)
    }
    fn segs(s: &Signal) -> Vec<(u64, u64)> {
        s.segments()
            .map(|(val, d)| (val.to_u64().unwrap(), d))
            .collect()
    }

    #[test]
    fn extends_and_merges_equal_values() {
        let mut s = Signal::new(info(4), v(1), 10, 0, 0);
        s.extend(Some(v(1)), 5);
        s.extend(Some(v(2)), 3);
        s.extend(None, 4);
        assert_eq!(segs(&s), vec![(1, 15), (2, 7)]);
        assert_eq!(s.end_time(), 22);
        assert_eq!(s.value_at(14).unwrap(), v(1));
        assert_eq!(s.value_at(15).unwrap(), v(2));
        assert_eq!(s.value_at(22), None);
        assert_eq!(s.segment_at(20), Some((v(2), 2)));
    }

    #[test]
    fn bounded_history_drops_oldest() {
        let mut s = Signal::new(info(4), v(0), 1, 0, 3);
        for i in 1..6 {
            s.extend(Some(v(i)), 1);
        }
        assert_eq!(segs(&s), vec![(3, 1), (4, 1), (5, 1)]);
        assert_eq!(s.start_time(), 3);
        assert_eq!(s.omitted_data_time(), 3);
        assert_eq!(s.value_at(2), None);
        assert_eq!(s.end_time(), 6);
        s.resize(2);
        assert_eq!(segs(&s), vec![(4, 1), (5, 1)]);
        assert_eq!(s.start_time(), 4);
        s.resize(0);
        s.extend(Some(v(9)), 1);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn replace_recent_cases() {
        // same duration: value replaced, merged with a matching predecessor
        let mut s = Signal::new(info(4), v(1), 4, 0, 0);
        s.extend(Some(v(2)), 4);
        s.replace_recent(Some(v(1)), 4);
        assert_eq!(segs(&s), vec![(1, 8)]);
        // shorter replacement splits the last segment
        let mut s = Signal::new(info(4), v(1), 4, 0, 0);
        s.extend(Some(v(2)), 4);
        s.replace_recent(Some(v(3)), 1);
        assert_eq!(segs(&s), vec![(1, 4), (2, 3), (3, 1)]);
        // longer replacement eats earlier history
        let mut s = Signal::new(info(4), v(1), 4, 0, 0);
        s.extend(Some(v(2)), 2);
        s.extend(Some(v(3)), 2);
        s.replace_recent(Some(v(4)), 3);
        assert_eq!(segs(&s), vec![(1, 4), (2, 1), (4, 3)]);
    }

    #[test]
    fn reset_resize_and_cursor() {
        let mut s = Signal::new(info(4), v(1), 2, 0, 0);
        s.extend(Some(v(2)), 3);
        let mut c = s.cursor();
        assert_eq!((c.time, c.duration, c.value), (0, 2, Some(v(1))));
        assert!(c.advance(&s, 2));
        assert_eq!(c.value, Some(v(2)));
        assert_eq!(c.duration, 3);
        assert!(!c.advance(&s, 3));
        assert_eq!(c.formatted_value(&s), "-");
        s.reset(v(7), 5);
        assert_eq!(segs(&s), vec![(7, 5)]);
        assert_eq!(s.start_time(), 0);
        // a value narrower than the (changed) signal width is zero-extended
        s.info.width = BitWidth::new(8);
        assert_eq!(s.value_at(0).unwrap().width().get(), 8);
        assert_eq!(s.formatted_value_at(0), "0000 0111");
    }

    #[test]
    fn info_names() {
        assert_eq!(info(1).display_name(), "x");
        assert_eq!(info(8).display_name(), "x[7..0]");
        assert_eq!(
            Signal::new(info(4), v(0), 1, 0, 0).formatted_max_value(),
            "1111"
        );
    }
}
