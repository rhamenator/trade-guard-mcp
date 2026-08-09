//! A minimal, dependency-free UTC-only timestamp type.
//!
//! Duplicated from `market-intelligence-mcp`'s
//! `market_intelligence_core::utc_timestamp` — every timestamp in this
//! system is UTC RFC 3339 (see
//! `market-system-contracts/schemas/2.0.0/common.schema.json`), so this
//! type deliberately does not model timezones, offsets other than
//! `Z`/`+00:00`, or leap seconds. That constraint is what makes it
//! practical to hand-roll correctly instead of depending on the `time`
//! crate. Kept as a per-repository duplicate rather than a shared
//! dependency because these are separate repositories/security boundaries
//! with no shared internal crate to depend on — small, well-tested
//! duplication over cross-repo coupling.
//!
//! The civil-date/day-count conversion is Howard Hinnant's public-domain
//! `days_from_civil`/`civil_from_days` algorithm
//! (<https://howardhinnant.github.io/date_algorithms.html>), which is exact
//! for the proleptic Gregorian calendar over a wide range of years using
//! only integer arithmetic — no lookup tables of era boundaries beyond the
//! algorithm's own 400-year cycle.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC instant: seconds since the Unix epoch plus a nanosecond remainder.
///
/// `nanos` is always in `0..1_000_000_000`; a negative instant is
/// represented by a negative `secs` with a non-negative `nanos` (i.e. it
/// rounds toward negative infinity, matching `div_euclid`/`rem_euclid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp {
    secs: i64,
    nanos: u32,
}

/// Every way `parse_rfc3339` can reject input. Deliberately granular so
/// tests can assert on the exact failure reason instead of just "it failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampParseError {
    TooShort,
    InvalidDigit,
    InvalidDateSeparator,
    InvalidTimeSeparator,
    MonthOutOfRange,
    DayOutOfRange,
    HourOutOfRange,
    MinuteOutOfRange,
    SecondOutOfRange,
    FractionInvalid,
    MissingUtcDesignator,
    UnsupportedOffset,
    TrailingGarbage,
    Overflow,
}

impl fmt::Display for TimestampParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::TooShort => "input shorter than the minimum RFC 3339 length",
            Self::InvalidDigit => "expected an ASCII digit",
            Self::InvalidDateSeparator => "expected '-' between date components",
            Self::InvalidTimeSeparator => "expected 'T'/'t' or ':' in the time portion",
            Self::MonthOutOfRange => "month is not in 1..=12",
            Self::DayOutOfRange => "day is out of range for the given year/month",
            Self::HourOutOfRange => "hour is not in 0..=23",
            Self::MinuteOutOfRange => "minute is not in 0..=59",
            Self::SecondOutOfRange => "second is not in 0..=59 (leap seconds are not supported)",
            Self::FractionInvalid => "fractional-second component is empty or longer than 9 digits",
            Self::MissingUtcDesignator => "missing 'Z' or '+00:00'/'-00:00' UTC designator",
            Self::UnsupportedOffset => "only the UTC offset (Z, +00:00, -00:00) is supported",
            Self::TrailingGarbage => "unexpected trailing characters after the UTC designator",
            Self::Overflow => "computed instant overflows i64 seconds since epoch",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TimestampParseError {}

fn is_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn last_day_of_month(y: i64, m: u32) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if m == 2 && is_leap_year(y) {
        29
    } else {
        DAYS[(m - 1) as usize]
    }
}

/// Days since 1970-01-01 for the given proleptic-Gregorian civil date.
/// Precondition: `m` in `1..=12`, `d` in `1..=last_day_of_month(y, m)`.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp as i64 + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil`: returns `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl UtcTimestamp {
    pub const UNIX_EPOCH: UtcTimestamp = UtcTimestamp { secs: 0, nanos: 0 };

    /// Constructs an instant from seconds-since-epoch and a nanosecond
    /// remainder, normalizing an out-of-range `nanos` into `secs`.
    pub fn from_unix(secs: i64, nanos: u32) -> Self {
        let extra_secs = (nanos / 1_000_000_000) as i64;
        UtcTimestamp {
            secs: secs.wrapping_add(extra_secs),
            nanos: nanos % 1_000_000_000,
        }
    }

    pub fn now() -> Self {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        UtcTimestamp {
            secs: dur.as_secs() as i64,
            nanos: dur.subsec_nanos(),
        }
    }

    pub fn unix_seconds(&self) -> i64 {
        self.secs
    }

    pub fn subsec_nanos(&self) -> u32 {
        self.nanos
    }

    pub fn checked_add_seconds(&self, secs: i64) -> Option<Self> {
        self.secs.checked_add(secs).map(|s| UtcTimestamp {
            secs: s,
            nanos: self.nanos,
        })
    }

    pub fn checked_add_days(&self, days: i64) -> Option<Self> {
        days.checked_mul(86_400)
            .and_then(|s| self.checked_add_seconds(s))
    }

    /// Saturating difference in whole seconds (`self - other`), clamped to
    /// `[i64::MIN, i64::MAX]` instead of panicking on overflow.
    pub fn saturating_diff_seconds(&self, other: &Self) -> i64 {
        self.secs.saturating_sub(other.secs)
    }

    /// Parses a UTC-only RFC 3339 timestamp: `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+00:00|-00:00)`.
    /// Rejects any non-zero UTC offset, leap seconds, and malformed input —
    /// it never panics, regardless of input.
    pub fn parse_rfc3339(s: &str) -> Result<Self, TimestampParseError> {
        let b = s.as_bytes();
        if b.len() < 20 {
            return Err(TimestampParseError::TooShort);
        }

        fn digit(byte: u8) -> Result<i64, TimestampParseError> {
            if byte.is_ascii_digit() {
                Ok((byte - b'0') as i64)
            } else {
                Err(TimestampParseError::InvalidDigit)
            }
        }
        fn two(b0: u8, b1: u8) -> Result<u32, TimestampParseError> {
            Ok((digit(b0)? * 10 + digit(b1)?) as u32)
        }

        let year = digit(b[0])? * 1000 + digit(b[1])? * 100 + digit(b[2])? * 10 + digit(b[3])?;
        if b[4] != b'-' {
            return Err(TimestampParseError::InvalidDateSeparator);
        }
        let month = two(b[5], b[6])?;
        if b[7] != b'-' {
            return Err(TimestampParseError::InvalidDateSeparator);
        }
        let day = two(b[8], b[9])?;
        match b[10] {
            b'T' | b't' => {}
            _ => return Err(TimestampParseError::InvalidTimeSeparator),
        }
        let hour = two(b[11], b[12])?;
        if b[13] != b':' {
            return Err(TimestampParseError::InvalidTimeSeparator);
        }
        let minute = two(b[14], b[15])?;
        if b[16] != b':' {
            return Err(TimestampParseError::InvalidTimeSeparator);
        }
        let second = two(b[17], b[18])?;

        if !(1..=12).contains(&month) {
            return Err(TimestampParseError::MonthOutOfRange);
        }
        if day < 1 || day > last_day_of_month(year, month) {
            return Err(TimestampParseError::DayOutOfRange);
        }
        if hour > 23 {
            return Err(TimestampParseError::HourOutOfRange);
        }
        if minute > 59 {
            return Err(TimestampParseError::MinuteOutOfRange);
        }
        if second > 59 {
            return Err(TimestampParseError::SecondOutOfRange);
        }

        let mut idx = 19usize;
        let mut nanos: u32 = 0;
        if idx < b.len() && b[idx] == b'.' {
            idx += 1;
            let frac_start = idx;
            while idx < b.len() && b[idx].is_ascii_digit() {
                idx += 1;
            }
            let frac_len = idx - frac_start;
            if frac_len == 0 || frac_len > 9 {
                return Err(TimestampParseError::FractionInvalid);
            }
            let mut value: u32 = 0;
            for &byte in &b[frac_start..idx] {
                value = value * 10 + (byte - b'0') as u32;
            }
            for _ in 0..(9 - frac_len) {
                value *= 10;
            }
            nanos = value;
        }

        if idx >= b.len() {
            return Err(TimestampParseError::MissingUtcDesignator);
        }
        match b[idx] {
            b'Z' | b'z' => {
                idx += 1;
            }
            b'+' | b'-' => {
                if b.len() < idx + 6 {
                    return Err(TimestampParseError::UnsupportedOffset);
                }
                let oh = two(b[idx + 1], b[idx + 2])?;
                if b[idx + 3] != b':' {
                    return Err(TimestampParseError::UnsupportedOffset);
                }
                let om = two(b[idx + 4], b[idx + 5])?;
                if oh != 0 || om != 0 {
                    return Err(TimestampParseError::UnsupportedOffset);
                }
                idx += 6;
            }
            _ => return Err(TimestampParseError::MissingUtcDesignator),
        }
        if idx != b.len() {
            return Err(TimestampParseError::TrailingGarbage);
        }

        let days = days_from_civil(year, month, day);
        let day_secs = hour as i64 * 3600 + minute as i64 * 60 + second as i64;
        let secs = days
            .checked_mul(86_400)
            .and_then(|d| d.checked_add(day_secs))
            .ok_or(TimestampParseError::Overflow)?;

        Ok(UtcTimestamp { secs, nanos })
    }

    /// Formats as `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, always UTC, trimming
    /// trailing zero digits from the fractional part (and omitting it
    /// entirely when `nanos` is zero).
    pub fn to_rfc3339(&self) -> String {
        let days = self.secs.div_euclid(86_400);
        let day_secs = self.secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let hour = day_secs / 3600;
        let minute = (day_secs % 3600) / 60;
        let second = day_secs % 60;
        if self.nanos == 0 {
            format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
        } else {
            let mut frac = format!("{:09}", self.nanos);
            while frac.ends_with('0') {
                frac.pop();
            }
            format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{frac}Z")
        }
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl serde::Serialize for UtcTimestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> serde::Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::string::String as serde::Deserialize>::deserialize(deserializer)?;
        UtcTimestamp::parse_rfc3339(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trips() {
        assert_eq!(
            UtcTimestamp::UNIX_EPOCH.to_rfc3339(),
            "1970-01-01T00:00:00Z"
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1970-01-01T00:00:00Z").unwrap(),
            UtcTimestamp::UNIX_EPOCH
        );
    }

    #[test]
    fn accepts_lowercase_designators() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1970-01-01t00:00:00z").unwrap(),
            UtcTimestamp::UNIX_EPOCH
        );
    }

    #[test]
    fn accepts_zero_offset_forms() {
        let expected = UtcTimestamp::UNIX_EPOCH;
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1970-01-01T00:00:00+00:00").unwrap(),
            expected
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1970-01-01T00:00:00-00:00").unwrap(),
            expected
        );
    }

    #[test]
    fn rejects_nonzero_offset() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1970-01-01T00:00:00+05:00"),
            Err(TimestampParseError::UnsupportedOffset)
        );
    }

    #[test]
    fn fractional_seconds_round_trip_and_are_trimmed() {
        let t = UtcTimestamp::parse_rfc3339("2026-07-19T14:30:00.500Z").unwrap();
        assert_eq!(t.subsec_nanos(), 500_000_000);
        assert_eq!(t.to_rfc3339(), "2026-07-19T14:30:00.5Z");
    }

    #[test]
    fn nine_digit_fraction_is_exact() {
        let t = UtcTimestamp::parse_rfc3339("2026-07-19T14:30:00.123456789Z").unwrap();
        assert_eq!(t.subsec_nanos(), 123_456_789);
    }

    #[test]
    fn leap_year_feb_29_is_valid() {
        assert!(UtcTimestamp::parse_rfc3339("2024-02-29T00:00:00Z").is_ok());
        assert!(UtcTimestamp::parse_rfc3339("2000-02-29T00:00:00Z").is_ok());
    }

    #[test]
    fn non_leap_century_year_rejects_feb_29() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("1900-02-29T00:00:00Z"),
            Err(TimestampParseError::DayOutOfRange)
        );
    }

    #[test]
    fn day_before_and_after_epoch_differ_by_exactly_one_day() {
        let epoch = UtcTimestamp::UNIX_EPOCH;
        let before = UtcTimestamp::parse_rfc3339("1969-12-31T00:00:00Z").unwrap();
        let after = UtcTimestamp::parse_rfc3339("1970-01-02T00:00:00Z").unwrap();
        assert_eq!(epoch.saturating_diff_seconds(&before), 86_400);
        assert_eq!(after.saturating_diff_seconds(&epoch), 86_400);
    }

    #[test]
    fn rejects_month_zero_and_thirteen() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-00-01T00:00:00Z"),
            Err(TimestampParseError::MonthOutOfRange)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-13-01T00:00:00Z"),
            Err(TimestampParseError::MonthOutOfRange)
        );
    }

    #[test]
    fn rejects_day_zero_and_february_thirty() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-01-00T00:00:00Z"),
            Err(TimestampParseError::DayOutOfRange)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-02-30T00:00:00Z"),
            Err(TimestampParseError::DayOutOfRange)
        );
    }

    #[test]
    fn rejects_hour_minute_second_out_of_range() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T24:00:00Z"),
            Err(TimestampParseError::HourOutOfRange)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T00:60:00Z"),
            Err(TimestampParseError::MinuteOutOfRange)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T00:00:60Z"),
            Err(TimestampParseError::SecondOutOfRange),
        );
    }

    #[test]
    fn rejects_missing_separators() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026/07-19T00:00:00Z"),
            Err(TimestampParseError::InvalidDateSeparator)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19 00:00:00Z"),
            Err(TimestampParseError::InvalidTimeSeparator)
        );
    }

    #[test]
    fn rejects_missing_designator_and_trailing_garbage() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T00:00:00"),
            Err(TimestampParseError::TooShort)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T00:00:00X"),
            Err(TimestampParseError::MissingUtcDesignator)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19T00:00:00Zgarbage"),
            Err(TimestampParseError::TrailingGarbage)
        );
    }

    #[test]
    fn rejects_empty_and_too_short_input() {
        assert_eq!(
            UtcTimestamp::parse_rfc3339(""),
            Err(TimestampParseError::TooShort)
        );
        assert_eq!(
            UtcTimestamp::parse_rfc3339("2026-07-19"),
            Err(TimestampParseError::TooShort)
        );
    }

    #[test]
    fn rejects_non_ascii_and_non_digit_bytes_without_panicking() {
        assert!(UtcTimestamp::parse_rfc3339("202✓-07-19T00:00:00Z").is_err());
        assert!(UtcTimestamp::parse_rfc3339("²⁰²⁶-07-19T00:00:00Z").is_err());
    }

    #[test]
    fn ordering_matches_chronological_order() {
        let a = UtcTimestamp::parse_rfc3339("2026-01-01T00:00:00Z").unwrap();
        let b = UtcTimestamp::parse_rfc3339("2026-01-01T00:00:01Z").unwrap();
        assert!(a < b);
        let c = UtcTimestamp::from_unix(a.unix_seconds(), 500);
        assert!(a <= c && c < b);
    }

    #[test]
    fn from_unix_normalizes_overflowing_nanos() {
        let t = UtcTimestamp::from_unix(0, 1_500_000_000);
        assert_eq!(t.unix_seconds(), 1);
        assert_eq!(t.subsec_nanos(), 500_000_000);
    }

    #[test]
    fn checked_add_days_matches_manual_parse() {
        let t = UtcTimestamp::parse_rfc3339("2026-01-01T00:00:00Z").unwrap();
        let expected = UtcTimestamp::parse_rfc3339("2026-01-02T00:00:00Z").unwrap();
        assert_eq!(t.checked_add_days(1).unwrap(), expected);
    }

    #[test]
    fn checked_add_seconds_none_on_overflow() {
        let t = UtcTimestamp::from_unix(i64::MAX, 0);
        assert!(t.checked_add_seconds(1).is_none());
    }

    #[test]
    fn serde_round_trips_through_json() {
        let t = UtcTimestamp::parse_rfc3339("2026-07-19T14:30:00.25Z").unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"2026-07-19T14:30:00.25Z\"");
        let back: UtcTimestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn serde_rejects_malformed_json_string() {
        let result: Result<UtcTimestamp, _> = serde_json::from_str("\"not-a-timestamp\"");
        assert!(result.is_err());
    }

    struct XorShift64(u64);
    impl XorShift64 {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    #[test]
    fn fuzz_smoke_parse_never_panics_on_arbitrary_bytes() {
        let mut rng = XorShift64(0x9E3779B97F4A7C15);
        for _ in 0..20_000 {
            let len = (rng.next() % 40) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| (rng.next() % 256) as u8).collect();
            let s = String::from_utf8_lossy(&bytes);
            let _ = UtcTimestamp::parse_rfc3339(&s);
        }
    }

    #[test]
    fn fuzz_smoke_mutated_valid_timestamps_never_panic() {
        let mut rng = XorShift64(0xD1B54A32D192ED03);
        let seed = "2026-07-19T14:30:00.123456789Z";
        for _ in 0..20_000 {
            let mut chars: Vec<char> = seed.chars().collect();
            let mutations = 1 + (rng.next() % 5) as usize;
            for _ in 0..mutations {
                let idx = (rng.next() as usize) % chars.len();
                chars[idx] = (b'!' + (rng.next() % 90) as u8) as char;
            }
            let mutated: String = chars.into_iter().collect();
            let _ = UtcTimestamp::parse_rfc3339(&mutated);
        }
    }

    #[test]
    fn fuzz_smoke_round_trip_is_stable_for_random_valid_instants() {
        let mut rng = XorShift64(0x2545F4914F6CDD1D);
        for _ in 0..5_000 {
            let secs = (rng.next() as i64) % 10_000_000_000;
            let nanos = (rng.next() % 1_000_000_000) as u32;
            let t = UtcTimestamp::from_unix(secs, nanos);
            let rendered = t.to_rfc3339();
            let parsed = UtcTimestamp::parse_rfc3339(&rendered)
                .unwrap_or_else(|e| panic!("failed to reparse {rendered:?}: {e}"));
            assert_eq!(parsed, t, "round trip mismatch for {rendered:?}");
        }
    }
}
