//! Exact decimal arithmetic for sample numbers: a model bound such as
//! `0.29999999999999999999` or `9223372036854775808` is kept as written,
//! where `f64` or `i64` would round it onto a value the server's exact
//! comparison rejects.

use std::cmp::Ordering;

use serde_json::Number;

/// `mantissa / 10^scale`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decimal {
    mantissa: i128,
    scale: u32,
}

impl Decimal {
    pub(crate) const fn integer(value: i128) -> Self {
        Self {
            mantissa: value,
            scale: 0,
        }
    }

    pub(crate) const fn new(mantissa: i128, scale: u32) -> Self {
        Self { mantissa, scale }
    }

    /// A JSON number's exact value, if its digits fit an `i128` mantissa.
    pub(crate) fn parse(number: &Number) -> Option<Self> {
        let text = number.to_string();
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
            None => (text.as_str(), 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits: i128 = format!("{whole}{fraction}").parse().ok()?;
        let scale = i32::try_from(fraction.len()).ok()?.checked_sub(exponent)?;
        if scale >= 0 {
            Some(Self::new(digits, scale.unsigned_abs()))
        } else {
            let factor = 10i128.checked_pow(scale.unsigned_abs())?;
            Some(Self::integer(digits.checked_mul(factor)?))
        }
    }

    /// Both values at one scale, if neither overflows.
    fn aligned(self, other: Self) -> Option<(i128, i128)> {
        let scale = self.scale.max(other.scale);
        let lift = |d: Self| {
            10i128
                .checked_pow(scale - d.scale)
                .and_then(|f| d.mantissa.checked_mul(f))
        };
        Some((lift(self)?, lift(other)?))
    }

    pub(crate) fn compare(self, other: Self) -> Option<Ordering> {
        self.aligned(other).map(|(a, b)| a.cmp(&b))
    }

    pub(crate) fn add(self, other: Self) -> Option<Self> {
        let (a, b) = self.aligned(other)?;
        Some(Self::new(a.checked_add(b)?, self.scale.max(other.scale)))
    }

    pub(crate) fn sub(self, other: Self) -> Option<Self> {
        self.add(Self::new(other.mantissa.checked_neg()?, other.scale))
    }

    /// The least integer at or above the value.
    pub(crate) fn ceil(self) -> Option<i128> {
        let factor = 10i128.checked_pow(self.scale)?;
        Some(self.mantissa.div_euclid(factor) + i128::from(self.mantissa.rem_euclid(factor) != 0))
    }

    /// The greatest integer at or below the value.
    pub(crate) fn floor(self) -> Option<i128> {
        Some(self.mantissa.div_euclid(10i128.checked_pow(self.scale)?))
    }

    /// The value as a JSON number, written exactly.
    pub(crate) fn to_number(self) -> Number {
        let sign = if self.mantissa < 0 { "-" } else { "" };
        let digits = self.mantissa.unsigned_abs().to_string();
        let scale = self.scale as usize;
        let text = if scale == 0 {
            digits
        } else {
            let digits = format!("{digits:0>width$}", width = scale + 1);
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            let fraction = fraction.trim_end_matches('0');
            if fraction.is_empty() {
                whole.to_owned()
            } else {
                format!("{whole}.{fraction}")
            }
        };
        format!("{sign}{text}")
            .parse()
            .expect("decimal text is a JSON number")
    }
}

#[cfg(test)]
mod tests {
    use super::Decimal;
    use std::cmp::Ordering;

    fn number(text: &str) -> serde_json::Number {
        text.parse().unwrap()
    }

    #[test]
    fn bounds_keep_their_digits() {
        let bound = Decimal::parse(&number("0.29999999999999999999")).unwrap();
        assert_eq!(bound.to_number().to_string(), "0.29999999999999999999");
        assert_eq!(
            bound.compare(Decimal::new(3, 1)),
            Some(Ordering::Less),
            "below 0.3"
        );
        let big = Decimal::parse(&number("9223372036854775808")).unwrap();
        assert_eq!(
            big.add(Decimal::integer(1))
                .unwrap()
                .to_number()
                .to_string(),
            "9223372036854775809"
        );
        assert_eq!(Decimal::parse(&number("1.5e2")).unwrap().floor(), Some(150));
        assert_eq!(Decimal::parse(&number("-1.5")).unwrap().ceil(), Some(-1));
        assert_eq!(Decimal::parse(&number("-1.5")).unwrap().floor(), Some(-2));
        assert_eq!(Decimal::new(-5, 2).to_number().to_string(), "-0.05");
    }
}
