//! Shared serde machinery for forward-compatible client enums and unions.
use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, Visitor};
use serde::ser::{Error as _, SerializeMap};
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;

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
            let name = map
                .next_key::<String>()?
                .ok_or_else(|| A::Error::invalid_length(0, &self))?;
            let value = map.next_value_seed(MemberSeed::<T>(name, PhantomData))?;
            if map.next_key::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(2, &self));
            }
            Ok(value)
        }
    }
    deserializer.deserialize_map(UnionVisitor(PhantomData))
}
