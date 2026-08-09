//! A minimal, dependency-free decimal-safe fixed-point type for money,
//! price, quantity, and FX values.
//!
//! `market-system-contracts/schemas/2.0.0/common.schema.json`'s
//! `decimal-string` definition requires values to cross every service
//! boundary as a string matching `^-?(0|[1-9][0-9]*)(\.[0-9]+)?$` (no
//! leading zeros, no leading `+`, no bare `.5` or trailing `5.`, no
//! exponent notation, and never `-0`/`-0.0...0`) specifically so that
//! `NaN`/`Infinity`/binary-floating-point rounding can never appear at an
//! external boundary. This type is the internal representation that
//! parses and re-serializes exactly that grammar, backed by a scaled
//! `i128` integer instead of `f64` so arithmetic stays exact.
//!
//! Fixed at 8 fractional decimal digits (`SCALE_EXP = 8`), matching common
//! practice for equities/options/FX notional and most crypto quantities.
//! Values needing finer precision are rejected outright
//! (`DecimalParseError::TooManyFractionalDigits`) rather than silently
//! rounded — a real crypto adapter needing e.g. 18-decimal ERC-20
//! precision would need a wider scale; tracked as a known limitation of
//! this paper-only vertical slice, not a silent correctness bug.

use std::cmp::Ordering;
use std::fmt;

const SCALE_EXP: u32 = 8;
const SCALE: i128 = 100_000_000; // 10^SCALE_EXP

/// A decimal-safe fixed-point value, scaled by 10^8 internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal {
    raw: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalParseError {
    EmptyInput,
    LeadingZeroInIntegerPart,
    MissingIntegerDigit,
    InvalidDigit,
    EmptyFractionalPart,
    TrailingGarbage,
    NegativeZero,
    TooManyFractionalDigits,
    Overflow,
}

impl fmt::Display for DecimalParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::EmptyInput => "input is empty",
            Self::LeadingZeroInIntegerPart => "integer part has a leading zero (e.g. \"01\")",
            Self::MissingIntegerDigit => "missing integer-part digit",
            Self::InvalidDigit => "expected an ASCII digit",
            Self::EmptyFractionalPart => "fractional part after '.' is empty",
            Self::TrailingGarbage => "unexpected trailing characters",
            Self::NegativeZero => {
                "negative zero (\"-0\" or \"-0.0...0\") is not a valid decimal-string"
            }
            Self::TooManyFractionalDigits => {
                "more than 8 fractional digits (finer than this type's supported scale)"
            }
            Self::Overflow => "value overflows this type's internal i128 representation",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for DecimalParseError {}

impl Decimal {
    pub const ZERO: Decimal = Decimal { raw: 0 };

    /// Constructs a `Decimal` directly from a whole-unit `i64`, e.g.
    /// `Decimal::from_i64(100)` is exactly `100`.
    pub fn from_i64(whole: i64) -> Self {
        Decimal {
            raw: (whole as i128) * SCALE,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    pub fn is_negative(&self) -> bool {
        self.raw < 0
    }

    /// Parses a `decimal-string` per
    /// `common.schema.json#/$defs/decimal-string`. Never panics, regardless
    /// of input.
    pub fn parse(s: &str) -> Result<Decimal, DecimalParseError> {
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            return Err(DecimalParseError::EmptyInput);
        }

        let mut idx = 0usize;
        let negative = bytes[0] == b'-';
        if negative {
            idx += 1;
        }
        if idx >= bytes.len() || !bytes[idx].is_ascii_digit() {
            return Err(DecimalParseError::MissingIntegerDigit);
        }

        let int_start = idx;
        if bytes[idx] == b'0' {
            idx += 1;
            if idx < bytes.len() && bytes[idx].is_ascii_digit() {
                return Err(DecimalParseError::LeadingZeroInIntegerPart);
            }
        } else {
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
        }
        let int_part = &s[int_start..idx];

        let mut frac_part = "";
        if idx < bytes.len() && bytes[idx] == b'.' {
            idx += 1;
            let frac_start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            if idx == frac_start {
                return Err(DecimalParseError::EmptyFractionalPart);
            }
            frac_part = &s[frac_start..idx];
        }

        if idx != bytes.len() {
            // Whatever remains is neither more integer/fractional digits
            // (the scanning loops above already consumed all of those)
            // nor a second '.' — an exponent marker, stray character, or
            // repeated separator all land here.
            return Err(DecimalParseError::TrailingGarbage);
        }

        if frac_part.len() > SCALE_EXP as usize {
            return Err(DecimalParseError::TooManyFractionalDigits);
        }

        let is_all_zero_frac = frac_part.is_empty() || frac_part.bytes().all(|b| b == b'0');
        if negative && int_part == "0" && is_all_zero_frac {
            return Err(DecimalParseError::NegativeZero);
        }

        let int_value: i128 = int_part.parse().map_err(|_| DecimalParseError::Overflow)?;
        let mut frac_value: i128 = 0;
        for (i, b) in frac_part.bytes().enumerate() {
            if !b.is_ascii_digit() {
                return Err(DecimalParseError::InvalidDigit);
            }
            let digit = (b - b'0') as i128;
            let place = SCALE_EXP as usize - 1 - i;
            frac_value += digit * 10i128.pow(place as u32);
        }

        let magnitude = int_value
            .checked_mul(SCALE)
            .and_then(|v| v.checked_add(frac_value))
            .ok_or(DecimalParseError::Overflow)?;

        let raw = if negative {
            magnitude.checked_neg().ok_or(DecimalParseError::Overflow)?
        } else {
            magnitude
        };
        Ok(Decimal { raw })
    }

    /// Renders back to the canonical `decimal-string` grammar: trailing
    /// fractional zeros trimmed, no fractional part at all when the value
    /// is a whole number, `-0` never produced (`ZERO.raw == 0`, and
    /// `checked_neg`/subtraction that would land exactly on zero produces
    /// positive zero because `i128` has no signed-zero representation).
    pub fn to_decimal_string(&self) -> String {
        let negative = self.raw < 0;
        let abs = self.raw.unsigned_abs();
        let int_part = abs / (SCALE as u128);
        let frac_part = abs % (SCALE as u128);
        let sign = if negative { "-" } else { "" };
        if frac_part == 0 {
            format!("{sign}{int_part}")
        } else {
            let mut frac_str = format!("{frac_part:0width$}", width = SCALE_EXP as usize);
            while frac_str.ends_with('0') {
                frac_str.pop();
            }
            format!("{sign}{int_part}.{frac_str}")
        }
    }

    pub fn checked_add(&self, other: &Decimal) -> Option<Decimal> {
        self.raw.checked_add(other.raw).map(|raw| Decimal { raw })
    }

    pub fn checked_sub(&self, other: &Decimal) -> Option<Decimal> {
        self.raw.checked_sub(other.raw).map(|raw| Decimal { raw })
    }

    /// Multiplies two decimal-scaled values (e.g. `quantity * price`),
    /// rescaling back down by `SCALE` after the raw multiplication.
    pub fn checked_mul(&self, other: &Decimal) -> Option<Decimal> {
        self.raw.checked_mul(other.raw).map(|scaled_up| Decimal {
            raw: scaled_up / SCALE,
        })
    }

    pub fn abs(&self) -> Decimal {
        Decimal {
            raw: self.raw.abs(),
        }
    }

    /// Approximate division, used only for average-fill-price bookkeeping
    /// (`order::Order::apply_fill`, `account` position updates) where an
    /// `f64`-precision result is acceptable. Not exposed as `checked_div`
    /// deliberately: decimal division's rounding-mode policy matters for
    /// anything money-facing at an external boundary, and this crate has
    /// no such requirement yet — see the module doc comment. Returns
    /// `Decimal::ZERO` for division by zero rather than panicking.
    pub fn approx_div(&self, other: &Decimal) -> Decimal {
        if other.is_zero() {
            return Decimal::ZERO;
        }
        let num_f: f64 = self.to_decimal_string().parse().unwrap_or(0.0);
        let den_f: f64 = other.to_decimal_string().parse().unwrap_or(1.0);
        let quotient = num_f / den_f;
        if !quotient.is_finite() {
            return Decimal::ZERO;
        }
        // Round to this type's 8-digit scale before re-parsing, since
        // `Decimal::parse` rejects more than 8 fractional digits and an
        // `f64` division can produce long, non-terminating decimal noise.
        let rounded = format!("{:.8}", quotient.abs());
        let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
        let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
        let sign = if quotient.is_sign_negative() && quotient != 0.0 {
            "-"
        } else {
            ""
        };
        Decimal::parse(&format!("{sign}{trimmed}")).unwrap_or(Decimal::ZERO)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}

impl serde::Serialize for Decimal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_decimal_string())
    }
}

impl<'de> serde::Deserialize<'de> for Decimal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::string::String as serde::Deserialize>::deserialize(deserializer)?;
        Decimal::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips_simple_integers() {
        assert_eq!(Decimal::parse("0").unwrap().to_decimal_string(), "0");
        assert_eq!(Decimal::parse("100").unwrap().to_decimal_string(), "100");
        assert_eq!(Decimal::parse("-100").unwrap().to_decimal_string(), "-100");
    }

    #[test]
    fn parses_and_round_trips_fractions_trimming_trailing_zeros() {
        assert_eq!(Decimal::parse("1.50").unwrap().to_decimal_string(), "1.5");
        assert_eq!(
            Decimal::parse("0.00000001").unwrap().to_decimal_string(),
            "0.00000001"
        );
        assert_eq!(
            Decimal::parse("123.456").unwrap().to_decimal_string(),
            "123.456"
        );
    }

    #[test]
    fn rejects_leading_zero_in_integer_part() {
        assert_eq!(
            Decimal::parse("01"),
            Err(DecimalParseError::LeadingZeroInIntegerPart)
        );
        assert_eq!(
            Decimal::parse("00"),
            Err(DecimalParseError::LeadingZeroInIntegerPart)
        );
    }

    #[test]
    fn rejects_bare_dot_forms() {
        assert_eq!(
            Decimal::parse(".5"),
            Err(DecimalParseError::MissingIntegerDigit)
        );
        assert_eq!(
            Decimal::parse("5."),
            Err(DecimalParseError::EmptyFractionalPart)
        );
    }

    #[test]
    fn rejects_leading_plus_and_exponent_notation() {
        assert_eq!(
            Decimal::parse("+5"),
            Err(DecimalParseError::MissingIntegerDigit)
        );
        assert_eq!(
            Decimal::parse("5e10"),
            Err(DecimalParseError::TrailingGarbage)
        );
        assert_eq!(
            Decimal::parse("1.5e10"),
            Err(DecimalParseError::TrailingGarbage)
        );
    }

    #[test]
    fn rejects_negative_zero_in_every_form() {
        assert_eq!(Decimal::parse("-0"), Err(DecimalParseError::NegativeZero));
        assert_eq!(Decimal::parse("-0.0"), Err(DecimalParseError::NegativeZero));
        assert_eq!(
            Decimal::parse("-0.00000000"),
            Err(DecimalParseError::NegativeZero)
        );
    }

    #[test]
    fn negative_nonzero_fraction_is_accepted() {
        assert_eq!(
            Decimal::parse("-0.01").unwrap().to_decimal_string(),
            "-0.01"
        );
    }

    #[test]
    fn rejects_more_than_eight_fractional_digits() {
        assert_eq!(
            Decimal::parse("1.123456789"),
            Err(DecimalParseError::TooManyFractionalDigits)
        );
        assert!(Decimal::parse("1.12345678").is_ok());
    }

    #[test]
    fn rejects_empty_and_garbage_input() {
        assert_eq!(Decimal::parse(""), Err(DecimalParseError::EmptyInput));
        assert_eq!(
            Decimal::parse("abc"),
            Err(DecimalParseError::MissingIntegerDigit)
        );
        assert_eq!(
            Decimal::parse("1.2.3"),
            Err(DecimalParseError::TrailingGarbage)
        );
        assert_eq!(
            Decimal::parse("1 "),
            Err(DecimalParseError::TrailingGarbage)
        );
        assert_eq!(
            Decimal::parse("NaN"),
            Err(DecimalParseError::MissingIntegerDigit)
        );
        assert_eq!(
            Decimal::parse("Infinity"),
            Err(DecimalParseError::MissingIntegerDigit)
        );
    }

    #[test]
    fn checked_add_and_sub_are_exact() {
        let a = Decimal::parse("0.1").unwrap();
        let b = Decimal::parse("0.2").unwrap();
        // Exact fixed-point arithmetic never produces float's 0.30000000000000004.
        assert_eq!(a.checked_add(&b).unwrap().to_decimal_string(), "0.3");
        assert_eq!(b.checked_sub(&a).unwrap().to_decimal_string(), "0.1");
    }

    #[test]
    fn checked_mul_computes_quantity_times_price() {
        let quantity = Decimal::parse("10").unwrap();
        let price = Decimal::parse("19.99").unwrap();
        assert_eq!(
            quantity.checked_mul(&price).unwrap().to_decimal_string(),
            "199.9"
        );
    }

    #[test]
    fn checked_mul_none_on_overflow() {
        let huge = Decimal { raw: i128::MAX };
        assert!(huge.checked_mul(&Decimal::from_i64(2)).is_none());
    }

    #[test]
    fn checked_add_none_on_overflow() {
        let huge = Decimal { raw: i128::MAX };
        assert!(huge.checked_add(&Decimal::from_i64(1)).is_none());
    }

    #[test]
    fn abs_removes_sign() {
        assert_eq!(
            Decimal::parse("-5.5").unwrap().abs().to_decimal_string(),
            "5.5"
        );
        assert_eq!(
            Decimal::parse("5.5").unwrap().abs().to_decimal_string(),
            "5.5"
        );
        assert_eq!(Decimal::ZERO.abs(), Decimal::ZERO);
    }

    #[test]
    fn approx_div_computes_a_reasonable_quotient() {
        let a = Decimal::from_i64(1010);
        let b = Decimal::from_i64(10);
        assert_eq!(a.approx_div(&b).to_decimal_string(), "101");
    }

    #[test]
    fn approx_div_by_zero_is_zero_not_a_panic() {
        assert_eq!(
            Decimal::from_i64(5).approx_div(&Decimal::ZERO),
            Decimal::ZERO
        );
    }

    #[test]
    fn ordering_matches_numeric_order() {
        assert!(Decimal::parse("1.5").unwrap() < Decimal::parse("2").unwrap());
        assert!(Decimal::parse("-1").unwrap() < Decimal::ZERO);
        assert!(Decimal::ZERO < Decimal::parse("0.00000001").unwrap());
    }

    #[test]
    fn serde_round_trips_through_json() {
        let d = Decimal::parse("42.5").unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"42.5\"");
        let back: Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn serde_rejects_malformed_decimal_string() {
        let result: Result<Decimal, _> = serde_json::from_str("\"not-a-number\"");
        assert!(result.is_err());
        let result: Result<Decimal, _> = serde_json::from_str("\"01\"");
        assert!(result.is_err());
    }

    /// A tiny, dependency-free xorshift64 PRNG for deterministic,
    /// reproducible robustness testing — same convention as
    /// `market_intelligence_core::utc_timestamp`'s fuzz-smoke tests.
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
            let _ = Decimal::parse(&s);
        }
    }

    #[test]
    fn fuzz_smoke_valid_round_trip_always_reparses_to_the_same_value() {
        let mut rng = XorShift64(0x2545F4914F6CDD1D);
        for _ in 0..5_000 {
            let whole = (rng.next() % 1_000_000) as i64;
            let d = Decimal::from_i64(whole);
            let rendered = d.to_decimal_string();
            let reparsed = Decimal::parse(&rendered)
                .unwrap_or_else(|e| panic!("failed to reparse {rendered:?}: {e}"));
            assert_eq!(reparsed, d);
        }
    }
}
