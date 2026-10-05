//! Exact arbitrary-precision model numbers serialized as JSON numeric tokens.
use bigdecimal::num_bigint::ToBigInt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

pub use bigdecimal::BigDecimal as DecimalValue;
pub use bigdecimal::num_bigint::BigInt as IntegerValue;

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
