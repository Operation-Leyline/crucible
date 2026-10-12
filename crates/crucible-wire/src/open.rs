//! Shared serde machinery for forward-compatible client enums and unions.
use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, Visitor};
use serde::ser::{Error as _, SerializeMap};
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;

/// String enum values, including values added by a newer service.
pub trait Enum: Sized {
    fn from_string(value: String) -> Self;
    fn as_str(&self) -> &str;
}

pub fn deserialize_enum<'de, T: Enum, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    String::deserialize(deserializer).map(T::from_string)
}

pub fn serialize_enum<T: Enum, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.as_str())
}

/// A future union arm, retained as its original member name and JSON value.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownUnion {
    pub name: String,
    pub value: Value,
}

pub trait Union: Sized {
    /// The protocol tolerates a `__type` key beside the member, as AWS JSON
    /// protocols do; a union that models a `__type` member never sets it.
    const IGNORE_TYPE: bool = false;
    fn deserialize_member<'de, D: Deserializer<'de>>(
        name: String,
        value: D,
    ) -> Result<Self, D::Error>;
    fn serialize_member<S: SerializeMap>(&self, map: &mut S) -> Result<(), S::Error>;
}

pub fn serialize_unknown<S: SerializeMap>(
    value: &UnknownUnion,
    known: &[&str],
    map: &mut S,
) -> Result<(), S::Error> {
    if known.contains(&value.name.as_str()) {
        return Err(S::Error::custom(
            "unknown union arm uses a declared member name",
        ));
    }
    map.serialize_entry(&value.name, &value.value)
}

pub fn serialize_union<T: Union, S: Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1))?;
    value.serialize_member(&mut map)?;
    map.end()
}

pub fn deserialize_union<'de, T: Union, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    struct UnionVisitor<T>(PhantomData<T>);
    struct MemberSeed<T>(String, PhantomData<T>);
    impl<'de, T: Union> DeserializeSeed<'de> for MemberSeed<T> {
        type Value = T;
        fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
            T::deserialize_member(self.0, deserializer)
        }
    }
    impl<'de, T: Union> Visitor<'de> for UnionVisitor<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a union object with exactly one member")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
            let mut value = None;
            while let Some(name) = map.next_key::<String>()? {
                if T::IGNORE_TYPE && name == "__type" {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
                if value.is_some() {
                    return Err(A::Error::invalid_length(2, &self));
                }
                value = Some(map.next_value_seed(MemberSeed::<T>(name, PhantomData))?);
            }
            value.ok_or_else(|| A::Error::invalid_length(0, &self))
        }
    }
    deserializer.deserialize_map(UnionVisitor(PhantomData))
}

/// Static value mapping emitted for integer enums, including open client values.
pub trait IntEnum: Sized {
    fn as_i32(&self) -> i32;
    fn from_i32(value: i32) -> Option<Self>;
}
pub fn serialize_int_enum<T: IntEnum, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_i32(value.as_i32())
}
pub fn deserialize_int_enum<'de, T: IntEnum, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = <i32 as serde::Deserialize>::deserialize(deserializer)?;
    T::from_i32(value).ok_or_else(|| serde::de::Error::custom("unknown integer enum value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Shape {
        Text(String),
    }
    struct Aws(Shape);
    impl Union for Shape {
        fn deserialize_member<'de, D: Deserializer<'de>>(
            name: String,
            value: D,
        ) -> Result<Self, D::Error> {
            if name != "text" {
                return Err(D::Error::custom("unmodeled member"));
            }
            String::deserialize(value).map(Self::Text)
        }
        fn serialize_member<S: SerializeMap>(&self, _: &mut S) -> Result<(), S::Error> {
            unreachable!()
        }
    }
    impl Union for Aws {
        const IGNORE_TYPE: bool = true;
        fn deserialize_member<'de, D: Deserializer<'de>>(
            name: String,
            value: D,
        ) -> Result<Self, D::Error> {
            Shape::deserialize_member(name, value).map(Self)
        }
        fn serialize_member<S: SerializeMap>(&self, _: &mut S) -> Result<(), S::Error> {
            unreachable!()
        }
    }

    #[test]
    fn a_type_key_is_ignored_only_where_the_protocol_allows_it() {
        for wire in [
            r#"{"__type":"x","text":"a"}"#,
            r#"{"text":"a","__type":"x"}"#,
        ] {
            let mut input = serde_json::Deserializer::from_str(wire);
            let value: Aws = deserialize_union(&mut input).unwrap();
            assert_eq!(value.0, Shape::Text("a".into()));
            let mut input = serde_json::Deserializer::from_str(wire);
            assert!(deserialize_union::<Shape, _>(&mut input).is_err());
        }
        let mut input = serde_json::Deserializer::from_str(r#"{"__type":"x"}"#);
        assert!(deserialize_union::<Aws, _>(&mut input).is_err());
    }
}
