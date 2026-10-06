//! Smithy RPC v2 CBOR document encoding and model codecs.
use bigdecimal::num_bigint::{BigInt, Sign};
use ciborium::Value;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::codec::Codec;
use crate::number::{BigDecimal, BigInteger};

pub fn write<T: Serialize + ?Sized>(value: &T, out: &mut Vec<u8>) -> Result<(), String> {
    ciborium::ser::into_writer(value, out).map_err(|error| error.to_string())
}

/// Exactly one document: trailing data must not become a second, hidden request.
pub fn read<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let mut cursor = std::io::Cursor::new(bytes);
    let value = ciborium::de::from_reader(&mut cursor).map_err(|error| error.to_string())?;
    if cursor.position() != bytes.len() as u64 {
        return Err("bytes follow the CBOR document".to_owned());
    }
    Ok(value)
}

/// Only envelope strings needed to classify a response; model fields stay in CBOR.
pub fn error_envelope(bytes: &[u8]) -> Option<serde_json::Value> {
    let Value::Map(fields) = read(bytes).ok()? else {
        return None;
    };
    let mut envelope = serde_json::Map::new();
    for (key, value) in fields {
        if let (Value::Text(key), Value::Text(value)) = (key, value)
            && matches!(key.as_str(), "__type" | "message")
        {
            envelope.insert(key, serde_json::Value::String(value));
        }
    }
    Some(serde_json::Value::Object(envelope))
}

/// Error discriminator added outside the modeled fields, retaining native CBOR values.
pub fn write_typed<T: Serialize + ?Sized>(
    value: &T,
    type_name: &'static str,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let mut value = Value::serialized(value).map_err(|error| error.to_string())?;
    let Value::Map(fields) = &mut value else {
        return Err("a modeled error must be a structure".to_owned());
    };
    fields.retain(|(key, _)| key != &Value::Text("__type".to_owned()));
    fields.push((
        Value::Text("__type".to_owned()),
        Value::Text(type_name.to_owned()),
    ));
    write(&value, out)
}

/// Event streams carry a shared JSON view between framing and typed receivers.
/// Restore the model type before writing CBOR so native wire values survive.
pub fn event_encode<T: Serialize + DeserializeOwned>(
    value: &serde_json::Value,
) -> Result<bytes::Bytes, String> {
    let value: T = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    write(&value, &mut bytes)?;
    Ok(bytes.into())
}

pub fn event_decode<T: Serialize + DeserializeOwned>(
    bytes: &[u8],
) -> Result<serde_json::Value, String> {
    let value: T = read(bytes)?;
    serde_json::to_value(&value).map_err(|error| error.to_string())
}

pub struct Timestamp;
impl Codec<crate::time::Timestamp> for Timestamp {
    fn serialize<S: Serializer>(
        value: &crate::time::Timestamp,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return crate::time::epoch_seconds::serialize(value, serializer);
        }
        let seconds = value.timestamp() as f64 + f64::from(value.timestamp_subsec_nanos()) / 1e9;
        ciborium::tag::Required::<_, 1>(seconds).serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<crate::time::Timestamp, D::Error> {
        if deserializer.is_human_readable() {
            return crate::time::epoch_seconds::deserialize(deserializer);
        }
        let ciborium::tag::Required::<f64, 1>(seconds) = Deserialize::deserialize(deserializer)?;
        crate::time::epoch_seconds::parse(&seconds.to_string())
            .ok_or_else(|| serde::de::Error::custom("timestamp out of range"))
    }
}

fn integer_value(value: &BigInt) -> Value {
    let negative = value.sign() == Sign::Minus;
    let magnitude = if negative {
        -value - BigInt::from(1)
    } else {
        value.clone()
    };
    let (_, bytes) = magnitude.to_bytes_be();
    Value::Tag(if negative { 3 } else { 2 }, Box::new(Value::Bytes(bytes)))
}

fn integer(value: Value) -> Result<BigInt, String> {
    match value {
        Value::Integer(value) => Ok(BigInt::from(i128::from(value))),
        Value::Tag(tag @ (2 | 3), bytes) => {
            let Value::Bytes(bytes) = *bytes else {
                return Err("bignum requires bytes".to_owned());
            };
            if bytes.len() > 4200 {
                return Err("big integer has too many digits".to_owned());
            }
            let value = BigInt::from_bytes_be(Sign::Plus, &bytes);
            Ok(if tag == 3 {
                -value - BigInt::from(1)
            } else {
                value
            })
        }
        _ => Err("expected a CBOR integer or bignum".to_owned()),
    }
}

pub struct Integer;
impl Codec<BigInteger> for Integer {
    fn serialize<S: Serializer>(value: &BigInteger, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return value.serialize(serializer);
        }
        integer_value(&value.0).serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInteger, D::Error> {
        if deserializer.is_human_readable() {
            return BigInteger::deserialize(deserializer);
        }
        integer(Value::deserialize(deserializer)?)
            .map(BigInteger)
            .map_err(serde::de::Error::custom)
    }
}

pub struct Decimal;
impl Codec<BigDecimal> for Decimal {
    fn serialize<S: Serializer>(value: &BigDecimal, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return value.serialize(serializer);
        }
        let (coefficient, scale) = value.0.as_bigint_and_scale();
        let exponent = scale
            .checked_neg()
            .ok_or_else(|| serde::ser::Error::custom("decimal exponent out of range"))?;
        Value::Tag(
            4,
            Box::new(Value::Array(vec![
                Value::Integer(exponent.into()),
                integer_value(&coefficient),
            ])),
        )
        .serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigDecimal, D::Error> {
        if deserializer.is_human_readable() {
            return BigDecimal::deserialize(deserializer);
        }
        let Value::Tag(4, value) = Value::deserialize(deserializer)? else {
            return Err(serde::de::Error::custom("decimal requires tag 4"));
        };
        let Value::Array(mut values) = *value else {
            return Err(serde::de::Error::custom(
                "decimal requires exponent and coefficient",
            ));
        };
        if values.len() != 2 {
            return Err(serde::de::Error::custom("decimal requires two items"));
        }
        let coefficient =
            integer(values.pop().expect("two items")).map_err(serde::de::Error::custom)?;
        let Value::Integer(exponent) = values.pop().expect("two items") else {
            return Err(serde::de::Error::custom(
                "decimal exponent requires an integer",
            ));
        };
        let exponent: i64 = exponent.try_into().map_err(serde::de::Error::custom)?;
        let scale = exponent
            .checked_neg()
            .ok_or_else(|| serde::de::Error::custom("decimal exponent out of range"))?;
        Ok(BigDecimal(bigdecimal::BigDecimal::new(coefficient, scale)))
    }
}

/// RPC v2 JSON arbitrary-precision values travel as strings, never lossy numbers.
pub struct StringNumber;
macro_rules! string_number {
    ($ty:ty, $integer:expr) => {
        impl Codec<$ty> for StringNumber {
            fn serialize<S: Serializer>(value: &$ty, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(value)
            }
            fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<$ty, D::Error> {
                let text = String::deserialize(deserializer)?;
                if text.parse::<serde_json::Number>().is_err()
                    || ($integer && text.contains(['.', 'e', 'E']))
                {
                    return Err(serde::de::Error::custom("invalid RPC v2 number grammar"));
                }
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}
string_number!(BigInteger, true);
string_number!(BigDecimal, false);
