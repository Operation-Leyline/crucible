//! Composable wire codecs for native collections, without public wrapper types.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub trait Codec<T> {
    fn serialize<S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error>;
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error>;
}

pub struct Adapter<C>(PhantomData<C>);
impl<C> Adapter<C> {
    pub fn serialize<T, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        C: Codec<T>,
    {
        C::serialize(value, serializer)
    }
    pub fn deserialize<'de, T, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error>
    where
        C: Codec<T>,
    {
        C::deserialize(deserializer)
    }
}

pub struct Ref<'a, T, C>(pub &'a T, pub PhantomData<C>);
impl<T, C: Codec<T>> Serialize for Ref<'_, T, C> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        C::serialize(self.0, serializer)
    }
}
struct Value<T, C>(T, PhantomData<C>);
impl<'de, T, C: Codec<T>> Deserialize<'de> for Value<T, C> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        C::deserialize(deserializer).map(|value| Self(value, PhantomData))
    }
}

pub struct Identity;
impl<T: Serialize + DeserializeOwned> Codec<T> for Identity {
    fn serialize<S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error> {
        T::deserialize(deserializer)
    }
}

/// An optional server document retains an explicitly supplied JSON null.
pub struct OptionalDocument;
impl Codec<Option<serde_json::Value>> for OptionalDocument {
    fn serialize<S: Serializer>(
        value: &Option<serde_json::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<serde_json::Value>, D::Error> {
        serde_json::Value::deserialize(deserializer).map(Some)
    }
}

/// A document used as a dense collection element cannot itself be null.
pub struct Document;
impl Codec<serde_json::Value> for Document {
    fn serialize<S: Serializer>(
        value: &serde_json::Value,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if value.is_null() {
            return Err(serde::ser::Error::custom(
                "dense collection elements cannot be null",
            ));
        }
        value.serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<serde_json::Value, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Err(serde::de::Error::custom(
                "dense collection elements cannot be null",
            ));
        }
        Ok(value)
    }
}

pub struct EpochSeconds;
pub struct HttpDate;
macro_rules! timestamp {
    ($codec:ident, $module:ident) => {
        impl Codec<crate::time::Timestamp> for $codec {
            fn serialize<S: Serializer>(
                value: &crate::time::Timestamp,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                crate::time::$module::serialize(value, serializer)
            }
            fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<crate::time::Timestamp, D::Error> {
                crate::time::$module::deserialize(deserializer)
            }
        }
    };
}
timestamp!(EpochSeconds, epoch_seconds);
timestamp!(HttpDate, http_date);

pub struct Optional<C>(PhantomData<C>);
impl<T, C: Codec<T>> Codec<Option<T>> for Optional<C> {
    fn serialize<S: Serializer>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error> {
        value
            .as_ref()
            .map(|value| Ref::<T, C>(value, PhantomData))
            .serialize(serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
        Option::<Value<T, C>>::deserialize(deserializer).map(|value| value.map(|value| value.0))
    }
}
pub struct Field<C>(PhantomData<C>);
impl<T, C: Codec<T>> Codec<crate::field::Field<T>> for Field<C> {
    fn serialize<S: Serializer>(
        value: &crate::field::Field<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            crate::field::Field::Value(value) => C::serialize(value, serializer),
            _ => serializer.serialize_none(),
        }
    }
    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<crate::field::Field<T>, D::Error> {
        Optional::<C>::deserialize(deserializer)
            .map(|value| value.map_or(crate::field::Field::Null, crate::field::Field::Value))
    }
}
/// A protocol that omits null structure members reads an explicit null as
/// absence. Documents keep their nulls and never use this codec.
pub struct NullAbsent<C>(PhantomData<C>);
impl<T, C: Codec<T>> Codec<crate::field::Field<T>> for NullAbsent<C> {
    fn serialize<S: Serializer>(
        value: &crate::field::Field<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Field::<C>::serialize(value, serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<crate::field::Field<T>, D::Error> {
        Field::<C>::deserialize(deserializer).map(|value| match value {
            crate::field::Field::Null => crate::field::Field::Absent,
            value => value,
        })
    }
}

/// Floating-point values whose protocol spells non-finite numbers as the
/// strings `NaN`, `Infinity` and `-Infinity`.
pub struct NonFinite;

/// A single-precision float in protocols that accept only finite numbers.
pub struct FiniteFloat;
impl Codec<f32> for FiniteFloat {
    fn serialize<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }

    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
        if !deserializer.is_human_readable() {
            return <NonFinite as Codec<f32>>::deserialize(deserializer);
        }
        // Serde's f32 visitor narrows unchecked, including finite f64 values
        // that become infinity. Check the destination before handing it out.
        let value = f64::deserialize(deserializer)? as f32;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(serde::de::Error::custom("number out of range"))
        }
    }
}

macro_rules! non_finite {
    ($float:ty, $serialize:ident, $deserialize:ident) => {
        impl Codec<$float> for NonFinite {
            fn serialize<S: Serializer>(value: &$float, serializer: S) -> Result<S::Ok, S::Error> {
                if !serializer.is_human_readable() {
                    return serializer.$serialize(*value);
                }
                if value.is_nan() {
                    serializer.serialize_str("NaN")
                } else if value.is_infinite() {
                    serializer.serialize_str(if *value > 0.0 {
                        "Infinity"
                    } else {
                        "-Infinity"
                    })
                } else {
                    serializer.$serialize(*value)
                }
            }
            fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<$float, D::Error> {
                if !deserializer.is_human_readable() {
                    struct FloatVisitor;
                    impl<'de> serde::de::Visitor<'de> for FloatVisitor {
                        type Value = $float;

                        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            f.write_str("a floating-point number")
                        }

                        fn visit_f32<E: serde::de::Error>(self, value: f32) -> Result<$float, E> {
                            Ok(<$float>::from(value))
                        }

                        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<$float, E> {
                            let narrowed = value as $float;
                            if value.is_finite() && !narrowed.is_finite() {
                                Err(serde::de::Error::custom("number out of range"))
                            } else {
                                Ok(narrowed)
                            }
                        }

                        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<$float, E> {
                            Ok(value as $float)
                        }

                        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<$float, E> {
                            Ok(value as $float)
                        }
                    }
                    // Keep fixed-width binary encodings symmetric with serialization;
                    // formats like CBOR can still hand the visitor a wider float.
                    return deserializer.$deserialize(FloatVisitor);
                }
                match serde_json::Value::deserialize(deserializer)? {
                    serde_json::Value::Number(number) => number
                        .as_f64()
                        .map(|value| value as $float)
                        .filter(|value| value.is_finite())
                        .ok_or_else(|| serde::de::Error::custom("number out of range")),
                    serde_json::Value::String(text) => match text.as_str() {
                        "NaN" => Ok(<$float>::NAN),
                        "Infinity" => Ok(<$float>::INFINITY),
                        "-Infinity" => Ok(<$float>::NEG_INFINITY),
                        _ => Err(serde::de::Error::custom(
                            "a float is a number, NaN, Infinity or -Infinity",
                        )),
                    },
                    _ => Err(serde::de::Error::custom("expected a number")),
                }
            }
        }
    };
}
non_finite!(f32, serialize_f32, deserialize_f32);
non_finite!(f64, serialize_f64, deserialize_f64);

pub struct Boxed<C>(PhantomData<C>);
impl<T, C: Codec<T>> Codec<Box<T>> for Boxed<C> {
    fn serialize<S: Serializer>(value: &Box<T>, serializer: S) -> Result<S::Ok, S::Error> {
        C::serialize(value, serializer)
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Box<T>, D::Error> {
        C::deserialize(deserializer).map(Box::new)
    }
}

pub struct List<C, const UNIQUE: bool>(PhantomData<C>);
impl<T, C: Codec<T>, const UNIQUE: bool> Codec<Vec<T>> for List<C, UNIQUE> {
    fn serialize<S: Serializer>(value: &Vec<T>, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seen = BTreeSet::new();
        let mut sequence = serializer.serialize_seq(Some(value.len()))?;
        for item in value {
            let item = Ref::<T, C>(item, PhantomData);
            if UNIQUE && !seen.insert(key(&item).map_err(serde::ser::Error::custom)?) {
                return Err(serde::ser::Error::custom("items must be unique"));
            }
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<T>, D::Error> {
        struct Items<T, C, const UNIQUE: bool>(PhantomData<(T, C)>);
        impl<'de, T, C: Codec<T>, const UNIQUE: bool> Visitor<'de> for Items<T, C, UNIQUE> {
            type Value = Vec<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
                let mut output = Vec::new();
                let mut seen = BTreeSet::new();
                while let Some(Value(value, _)) = input.next_element::<Value<T, C>>()? {
                    if UNIQUE
                        && !seen.insert(
                            key(&Ref::<T, C>(&value, PhantomData))
                                .map_err(serde::de::Error::custom)?,
                        )
                    {
                        return Err(serde::de::Error::custom("items must be unique"));
                    }
                    output.push(value);
                }
                Ok(output)
            }
        }
        deserializer.deserialize_seq(Items::<T, C, UNIQUE>(PhantomData))
    }
}

pub struct Map<C>(PhantomData<C>);
impl<K: Serialize + DeserializeOwned + Ord, T, C: Codec<T>> Codec<BTreeMap<K, T>> for Map<C> {
    fn serialize<S: Serializer>(value: &BTreeMap<K, T>, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(key, &Ref::<T, C>(value, PhantomData))?;
        }
        map.end()
    }
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<K, T>, D::Error> {
        struct Entries<K, T, C>(PhantomData<(K, T, C)>);
        impl<'de, K: DeserializeOwned + Ord, T, C: Codec<T>> Visitor<'de> for Entries<K, T, C> {
            type Value = BTreeMap<K, T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
                let mut output = BTreeMap::new();
                while let Some((key, Value(value, _))) = input.next_entry::<K, Value<T, C>>()? {
                    output.insert(key, value);
                }
                Ok(output)
            }
        }
        deserializer.deserialize_map(Entries::<K, T, C>(PhantomData))
    }
}

/// Structural equality ignores object order and numeric spelling.
pub(crate) fn key(value: &impl Serialize) -> Result<String, serde_json::Error> {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Number(number) => {
                if let Ok(decimal) = number.to_string().parse::<crate::number::DecimalValue>() {
                    *number = decimal
                        .normalized()
                        .to_string()
                        .parse()
                        .expect("decimal is JSON numeric syntax");
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(normalize),
            serde_json::Value::Object(items) => {
                // serde_json's preserve_order feature can be enabled by another
                // workspace dependency; set equality must remain order independent.
                let sorted: BTreeMap<_, _> = std::mem::take(items).into_iter().collect();
                *items = sorted
                    .into_iter()
                    .map(|(key, mut value)| {
                        normalize(&mut value);
                        (key, value)
                    })
                    .collect();
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value)?;
    normalize(&mut value);
    serde_json::to_string(&value)
}

#[cfg(test)]
mod tests {
    use super::{Adapter, Document, Identity, List, Map};
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn binary_f32_uses_its_native_deserializer() {
        struct BinaryFloat;
        impl<'de> serde::Deserializer<'de> for BinaryFloat {
            type Error = serde_json::Error;

            fn is_human_readable(&self) -> bool {
                false
            }

            fn deserialize_f32<V: serde::de::Visitor<'de>>(
                self,
                visitor: V,
            ) -> Result<V::Value, Self::Error> {
                visitor.visit_f32(1.5)
            }

            fn deserialize_any<V: serde::de::Visitor<'de>>(
                self,
                _: V,
            ) -> Result<V::Value, Self::Error> {
                Err(serde::de::Error::custom("expected a 32-bit float"))
            }

            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f64 char str string bytes byte_buf
                option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
                identifier ignored_any
            }
        }
        assert_eq!(
            Adapter::<super::NonFinite>::deserialize::<f32, _>(BinaryFloat).unwrap(),
            1.5
        );
        assert_eq!(
            Adapter::<super::FiniteFloat>::deserialize::<f32, _>(BinaryFloat).unwrap(),
            1.5
        );
    }

    #[test]
    fn finite_only_json_body_rejects_f32_overflow() {
        #[derive(serde::Deserialize)]
        struct Input {
            #[serde(with = "Adapter::<super::FiniteFloat>")]
            value: f32,
        }
        for body in [
            r#"{"value":1e39}"#,
            r#"{"value":-1e39}"#,
            r#"{"value":"Infinity"}"#,
        ] {
            assert!(serde_json::from_str::<Input>(body).is_err(), "{body}");
        }
        assert_eq!(
            serde_json::from_str::<Input>(r#"{"value":0.1}"#)
                .unwrap()
                .value,
            0.1
        );
    }

    #[test]
    fn finite_numbers_cannot_overflow_f32_in_json_or_cbor() {
        use super::NonFinite;
        for text in ["1e39", "-1e39", "1e400", "-1e400"] {
            let mut input = serde_json::Deserializer::from_str(text);
            assert!(
                Adapter::<NonFinite>::deserialize::<f32, _>(&mut input).is_err(),
                "{text}"
            );
        }
        for value in [1e39, -1e39] {
            let mut bytes = Vec::new();
            crate::cbor::write(&value, &mut bytes).unwrap();
            #[derive(serde::Deserialize)]
            struct Single(#[serde(with = "Adapter::<NonFinite>")] f32);
            assert!(crate::cbor::read::<Single>(&bytes).is_err());
            let mut bytes = Vec::new();
            crate::cbor::write(&f64::INFINITY, &mut bytes).unwrap();
            assert_eq!(
                crate::cbor::read::<Single>(&bytes).unwrap().0,
                f32::INFINITY
            );
        }
        for value in [f32::MAX, f32::MIN, 0.1] {
            let text = value.to_string();
            let mut input = serde_json::Deserializer::from_str(&text);
            assert_eq!(
                Adapter::<NonFinite>::deserialize::<f32, _>(&mut input).unwrap(),
                value
            );
        }
        let mut input = serde_json::Deserializer::from_str("1e39");
        assert_eq!(
            Adapter::<NonFinite>::deserialize::<f64, _>(&mut input).unwrap(),
            1e39
        );
    }

    #[test]
    fn non_finite_floats_are_strings_and_finite_floats_numbers() {
        use super::{Field, NonFinite, NullAbsent};
        for (value, wire) in [
            (f64::NAN, "\"NaN\""),
            (f64::INFINITY, "\"Infinity\""),
            (f64::NEG_INFINITY, "\"-Infinity\""),
            (1.5, "1.5"),
        ] {
            let mut out = Vec::new();
            Adapter::<NonFinite>::serialize(&value, &mut serde_json::Serializer::new(&mut out))
                .unwrap();
            assert_eq!(std::str::from_utf8(&out).unwrap(), wire);
            let mut input = serde_json::Deserializer::from_str(wire);
            let read: f64 = Adapter::<NonFinite>::deserialize(&mut input).unwrap();
            assert!(read == value || (read.is_nan() && value.is_nan()));
        }
        let mut input = serde_json::Deserializer::from_str("\"nan\"");
        assert!(Adapter::<NonFinite>::deserialize::<f32, _>(&mut input).is_err());
        let mut input = serde_json::Deserializer::from_str("null");
        let value: crate::field::Field<String> =
            Adapter::<NullAbsent<Identity>>::deserialize(&mut input).unwrap();
        assert!(value.is_absent());
        let mut input = serde_json::Deserializer::from_str("null");
        let value: crate::field::Field<String> =
            Adapter::<Field<Identity>>::deserialize(&mut input).unwrap();
        assert!(value.is_null());
    }

    #[test]
    fn unique_items_ignore_object_order_and_exact_numeric_spelling() {
        let mut input = serde_json::Deserializer::from_str(
            r#"[{"x":12345678901234567890.1,"y":true},{"y":true,"x":12345678901234567890.10}]"#,
        );
        assert!(Adapter::<List<Identity, true>>::deserialize::<Vec<Value>, _>(&mut input).is_err());
    }

    #[test]
    fn dense_document_maps_reject_outer_null_and_keep_nested_null() {
        let mut input = serde_json::Deserializer::from_str(r#"{"x":null}"#);
        assert!(
            Adapter::<Map<Document>>::deserialize::<BTreeMap<String, Value>, _>(&mut input)
                .is_err()
        );
        let mut input = serde_json::Deserializer::from_str(r#"{"x":{"nil":null}}"#);
        let value = Adapter::<Map<Document>>::deserialize::<BTreeMap<String, Value>, _>(&mut input)
            .unwrap();
        assert!(value["x"]["nil"].is_null());
    }
}
