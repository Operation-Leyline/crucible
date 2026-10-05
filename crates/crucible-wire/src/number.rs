//! Exact arbitrary-precision model numbers serialized as JSON numeric tokens.
use bigdecimal::num_bigint::{Sign, ToBigInt};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

pub use bigdecimal::BigDecimal as DecimalValue;
pub use bigdecimal::num_bigint::BigInt as IntegerValue;

const MAX_BIG_INTEGER_DIGITS: i128 = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BigInteger(pub IntegerValue);
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BigDecimal(pub DecimalValue);

impl BigInteger {
    pub fn into_inner(self) -> IntegerValue {
        self.0
    }
    pub fn as_decimal(&self) -> Cow<'_, DecimalValue> {
        Cow::Owned(self.0.clone().into())
    }
}
impl BigDecimal {
    pub fn into_inner(self) -> DecimalValue {
        self.0
    }
    pub fn as_decimal(&self) -> Cow<'_, DecimalValue> {
        Cow::Borrowed(&self.0)
    }
}
impl Deref for BigInteger {
    type Target = IntegerValue;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Deref for BigDecimal {
    type Target = DecimalValue;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<IntegerValue> for BigInteger {
    fn from(value: IntegerValue) -> Self {
        Self(value)
    }
}
impl From<DecimalValue> for BigDecimal {
    fn from(value: DecimalValue) -> Self {
        Self(value)
    }
}
impl FromStr for BigInteger {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value: DecimalValue = text
            .parse()
            .map_err(|error: bigdecimal::ParseBigDecimalError| error.to_string())?;
        let (coefficient, scale) = value.as_bigint_and_scale();
        if coefficient.sign() == Sign::NoSign {
            return Ok(Self(IntegerValue::from(0)));
        }
        // Both integrality checks and integer conversion can materialize powers of ten.
        // Reject exponent expansion before either; zero needs no rescaling at all.
        let digits = i128::from(value.digits()) - i128::from(scale);
        if digits <= 0 {
            return Err("fractional big integer".into());
        }
        if digits > MAX_BIG_INTEGER_DIGITS {
            return Err("big integer has too many digits".into());
        }
        if !value.is_integer() {
            return Err("fractional big integer".into());
        }
        value
            .to_bigint()
            .map(Self)
            .ok_or_else(|| "invalid big integer".into())
    }
}
impl FromStr for BigDecimal {
    type Err = bigdecimal::ParseBigDecimalError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}
macro_rules! numeric_serde {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let number: serde_json::Number = self
                    .to_string()
                    .parse()
                    .map_err(serde::ser::Error::custom)?;
                number.serialize(serializer)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                serde_json::Number::deserialize(deserializer)?
                    .to_string()
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}
numeric_serde!(BigInteger);
numeric_serde!(BigDecimal);
macro_rules! integers {
    ($($kind:ty),*) => { $(
        impl From<$kind> for BigInteger { fn from(value: $kind) -> Self { Self(value.into()) } }
        impl From<$kind> for BigDecimal { fn from(value: $kind) -> Self { Self(value.into()) } }
    )* };
}
integers!(i8, i16, i32, i64, i128, u8, u16, u32, u64, u128);

#[cfg(test)]
mod tests {
    use super::BigInteger;

    #[test]
    fn resulting_integer_digit_limit_applies_to_both_exponent_directions() {
        let expected = format!("1{}", "0".repeat(9_999));
        assert_eq!(
            "1e9999".parse::<BigInteger>().unwrap().to_string(),
            expected
        );
        assert_eq!(
            "-1e9999".parse::<BigInteger>().unwrap().to_string(),
            format!("-{expected}")
        );
        assert_eq!(
            "1e10000".parse::<BigInteger>().unwrap_err(),
            "big integer has too many digits"
        );
        let coefficient = format!("1{}", "0".repeat(10_000));
        assert_eq!(
            format!("{coefficient}e-1")
                .parse::<BigInteger>()
                .unwrap()
                .to_string(),
            expected
        );
        assert_eq!(
            coefficient.parse::<BigInteger>().unwrap_err(),
            "big integer has too many digits"
        );
        assert_eq!(
            format!("{coefficient}0e-1")
                .parse::<BigInteger>()
                .unwrap_err(),
            "big integer has too many digits"
        );
    }

    #[test]
    fn fractional_big_integers_remain_invalid() {
        for text in ["1.5", "-1.5", "1e-1", "99e-2"] {
            assert_eq!(
                text.parse::<BigInteger>().unwrap_err(),
                "fractional big integer",
                "{text}"
            );
        }
        assert_eq!("100e-2".parse::<BigInteger>().unwrap(), BigInteger::from(1));
        assert_eq!(
            "-15.00".parse::<BigInteger>().unwrap(),
            BigInteger::from(-15)
        );
    }

    #[test]
    fn huge_exponents_are_rejected_before_power_of_ten_allocations() {
        for text in ["1e1000000000", "-1e1000000000", "1e9223372036854775808"] {
            assert_eq!(
                text.parse::<BigInteger>().unwrap_err(),
                "big integer has too many digits",
                "{text}"
            );
            assert!(serde_json::from_str::<BigInteger>(text).is_err());
        }
        for text in ["1e-1000000000", "-1e-1000000000", "1e-9223372036854775807"] {
            assert_eq!(
                text.parse::<BigInteger>().unwrap_err(),
                "fractional big integer",
                "{text}"
            );
        }
    }

    #[test]
    fn zero_does_not_require_exponent_expansion() {
        for text in [
            "0",
            "-0.0",
            "0e1000000000",
            "0e-1000000000",
            "0e9223372036854775808",
            "0e-9223372036854775807",
        ] {
            let value = text.parse::<BigInteger>().unwrap();
            assert_eq!(value, BigInteger::from(0), "{text}");
            assert_eq!(value.to_string(), "0");
        }
    }
}
