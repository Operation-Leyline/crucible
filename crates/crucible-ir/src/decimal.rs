//! Exact values of model numbers. A bound such as `0.29999999999999999999`
//! or `9223372036854775808` is held as written: `f64` would round it onto a
//! neighbour and `i128` stops at 39 digits. The value is the server's own
//! `DecimalValue` type (`bigdecimal`), so a check made here compares a
//! `bigInteger` or `bigDecimal` the way the generated server does.

use std::cmp::Ordering;
use std::str::FromStr;

use bigdecimal::num_bigint::{BigInt, Sign};
use bigdecimal::{BigDecimal, RoundingMode, ToPrimitive};
use serde_json::Number;

use crate::ir::Bounds;

/// The most digit positions arithmetic or plain text may span. Past it a
/// result is refused rather than written out: `1e999999999 + 1` would
/// materialize a billion digits. Comparison has no such limit.
const MAX_SPAN: i64 = 1_000;

/// An exact decimal value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decimal(BigDecimal);

impl Decimal {
    pub fn integer(value: i128) -> Self {
        Self(BigDecimal::from(value))
    }

    /// `mantissa / 10^scale`.
    pub fn new(mantissa: i128, scale: u32) -> Self {
        Self(BigDecimal::new(BigInt::from(mantissa), i64::from(scale)))
    }

    /// A JSON number's exact value, exponent notation included.
    pub fn parse(number: &Number) -> Option<Self> {
        Self::from_text(&number.to_string())
    }

    /// The exact value of number text such as `-2.5E3` or `1e-10`.
    pub fn from_text(text: &str) -> Option<Self> {
        BigDecimal::from_str(text).ok().map(Self)
    }

    pub fn is_zero(&self) -> bool {
        self.0.sign() == Sign::NoSign
    }

    pub fn is_negative(&self) -> bool {
        self.0.sign() == Sign::Minus
    }

    /// The digit positions from the most significant integer digit (at
    /// least the units) to the last fractional one (at least none).
    fn span(&self) -> (i64, i64) {
        let (_, scale) = self.0.as_bigint_and_scale();
        let digits = i64::try_from(self.0.digits()).unwrap_or(i64::MAX);
        (digits.saturating_sub(scale).max(1), scale.max(0))
    }

    fn fits_span(&self, other: &Self) -> bool {
        let ((a_int, a_frac), (b_int, b_frac)) = (self.span(), other.span());
        a_int
            .max(b_int)
            .saturating_add(a_frac.max(b_frac))
            .saturating_add(1)
            <= MAX_SPAN
    }

    /// `self + other`, unless the sum would span more than a thousand digits.
    pub fn add(&self, other: &Self) -> Option<Self> {
        self.fits_span(other).then(|| Self(&self.0 + &other.0))
    }

    /// `self - other`, unless the difference would span more than a
    /// thousand digits.
    pub fn sub(&self, other: &Self) -> Option<Self> {
        self.fits_span(other).then(|| Self(&self.0 - &other.0))
    }

    /// The value rounded to an integer, up or down, if that fits an `i128`.
    fn rounded(&self, up: bool) -> Option<i128> {
        let (_, scale) = self.0.as_bigint_and_scale();
        let digits = i64::try_from(self.0.digits()).unwrap_or(i64::MAX);
        let integer_digits = digits.saturating_sub(scale);
        // i128 holds at most 39 digits; more cannot fit, and checking first
        // keeps `1e999999999` from being expanded.
        if integer_digits > 39 {
            return None;
        }
        // A value below one in magnitude rounds to -1, 0 or 1; rounding it
        // directly would scale by its (possibly huge) exponent.
        if integer_digits <= 0 {
            return Some(match (self.0.sign(), up) {
                (Sign::Plus, true) => 1,
                (Sign::Minus, false) => -1,
                _ => 0,
            });
        }
        let mode = if up {
            RoundingMode::Ceiling
        } else {
            RoundingMode::Floor
        };
        self.0.with_scale_round(0, mode).to_i128()
    }

    /// The least integer at or above the value, if it fits an `i128`.
    pub fn ceil(&self) -> Option<i128> {
        self.rounded(true)
    }

    /// The greatest integer at or below the value, if it fits an `i128`.
    pub fn floor(&self) -> Option<i128> {
        self.rounded(false)
    }

    /// The value as a JSON number, written exactly: plain digits, or an
    /// exponent when plain digits would span more than a thousand.
    pub fn to_number(&self) -> Number {
        let (digits, scale) = self.0.as_bigint_and_scale();
        let sign = if self.is_negative() { "-" } else { "" };
        let mut digits = digits.magnitude().to_string();
        let mut scale = scale;
        // Trailing zeros carry no value: `1.50` is `1.5`, `100e-2` is `1`.
        while scale > 0 && digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
            scale -= 1;
        }
        let (int, frac) = self.span();
        let text = if self.is_zero() {
            "0".to_owned()
        } else if int.saturating_add(frac) > MAX_SPAN {
            format!("{digits}e{}", scale.saturating_neg())
        } else if scale <= 0 {
            format!("{digits}{}", "0".repeat(scale.unsigned_abs() as usize))
        } else {
            let scale = scale as usize;
            let digits = format!("{digits:0>width$}", width = scale + 1);
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            format!("{whole}.{fraction}")
        };
        format!("{sign}{text}")
            .parse()
            .expect("decimal text is a JSON number")
    }
}

/// `a` against `b`, exactly, if both are numbers.
pub fn compare(a: &Number, b: &Number) -> Option<Ordering> {
    Some(Decimal::parse(a)?.cmp(&Decimal::parse(b)?))
}

impl Bounds<Number> {
    /// The bounds a `byte`, `short`, `integer` or `long` is held to: the
    /// least integer at or above `min` and the greatest at or below `max`,
    /// which accept exactly the integers the written bounds do. A bound past
    /// `i128` saturates there, beyond every 64-bit value.
    pub fn integer_bounds(&self) -> (Option<i128>, Option<i128>) {
        let rounded = |bound: &Option<Number>, round: fn(&Decimal) -> Option<i128>| {
            let bound = Decimal::parse(bound.as_ref()?)?;
            Some(round(&bound).unwrap_or(if bound.is_negative() {
                i128::MIN
            } else {
                i128::MAX
            }))
        };
        (
            rounded(&self.min, Decimal::ceil),
            rounded(&self.max, Decimal::floor),
        )
    }

    /// The bounds a `float` or `double` is held to: each the nearest `f64`,
    /// infinite past its range. The server compares in IEEE arithmetic, a
    /// `float` after narrowing both sides to `f32`.
    pub fn float_bounds(&self) -> (Option<f64>, Option<f64>) {
        let float = |bound: &Option<Number>| bound.as_ref()?.to_string().parse::<f64>().ok();
        (float(&self.min), float(&self.max))
    }
}

#[cfg(test)]
mod tests {
    use super::{Decimal, compare};
    use std::cmp::Ordering;

    fn number(text: &str) -> serde_json::Number {
        text.parse().unwrap()
    }

    fn decimal(text: &str) -> Decimal {
        Decimal::from_text(text).unwrap()
    }

    #[test]
    fn compares_past_f64_and_i128() {
        for (a, b, order) in [
            ("0.29999999999999999999", "0.3", Ordering::Less),
            ("0.30000000000000000001", "0.3", Ordering::Greater),
            ("0.3", "0.30", Ordering::Equal),
            ("3e-1", "0.3", Ordering::Equal),
            ("2.5E3", "2500", Ordering::Equal),
            ("1e-10", "0.0000000001", Ordering::Equal),
            ("1e-10", "0.00000000010000000000000000001", Ordering::Less),
            ("-0.29999999999999999999", "-0.3", Ordering::Greater),
            ("9007199254740993", "9007199254740992", Ordering::Greater),
            ("9007199254740992.5", "9007199254740993", Ordering::Less),
            (
                "1000000000000000000000000000000000000000001",
                "1e42",
                Ordering::Greater,
            ),
            (
                "-1000000000000000000000000000000000000000001",
                "-1e42",
                Ordering::Less,
            ),
            ("1e999999999", "1e999999998", Ordering::Greater),
            ("1e-999999999", "0", Ordering::Greater),
            ("-0", "0", Ordering::Equal),
        ] {
            assert_eq!(compare(&number(a), &number(b)), Some(order), "{a} vs {b}");
            assert_eq!(
                compare(&number(b), &number(a)),
                Some(order.reverse()),
                "{b} vs {a}"
            );
        }
    }

    #[test]
    fn arithmetic_keeps_every_digit() {
        let bound = decimal("0.29999999999999999999");
        assert_eq!(bound.to_number().to_string(), "0.29999999999999999999");
        assert!(bound < Decimal::new(3, 1), "below 0.3");
        let big = decimal("9223372036854775808");
        assert_eq!(
            big.add(&Decimal::integer(1))
                .unwrap()
                .to_number()
                .to_string(),
            "9223372036854775809"
        );
        let huge = decimal("170141183460469231731687303715884105728");
        assert_eq!(
            huge.add(&Decimal::integer(1))
                .unwrap()
                .to_number()
                .to_string(),
            "170141183460469231731687303715884105729"
        );
        assert_eq!(huge.ceil(), None, "past i128");
        assert_eq!(decimal("1.5e2").floor(), Some(150));
        assert_eq!(decimal("-1.5").ceil(), Some(-1));
        assert_eq!(decimal("-1.5").floor(), Some(-2));
        assert_eq!(decimal("1e-999999999").ceil(), Some(1));
        assert_eq!(decimal("1e-999999999").floor(), Some(0));
        assert_eq!(decimal("-1e-999999999").floor(), Some(-1));
        assert_eq!(decimal("1e999999999").floor(), None);
        assert_eq!(decimal("1e999999999").add(&Decimal::integer(1)), None);
        // Written with its exponent, not a billion digits.
        let huge = decimal("1e999999999");
        assert!(huge.to_number().to_string().len() < 16);
        assert_eq!(Decimal::parse(&huge.to_number()), Some(huge));
        assert_eq!(Decimal::new(-5, 2).to_number().to_string(), "-0.05");
        assert_eq!(decimal("1.50").to_number().to_string(), "1.5");
        assert_eq!(decimal("2.5E3").to_number().to_string(), "2500");
        assert_eq!(decimal("0.000").to_number().to_string(), "0");
        assert!(decimal("-0.0").is_zero());
    }
}
