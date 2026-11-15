//! Presence of a model member, independently of its value's nullability.
use std::ops::Deref;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A serde field adapter that makes a required document's presence mandatory.
/// Derive treats `deserialize_with` fields as required even when the underlying
/// type can deserialize a missing value as JSON null.
pub fn required<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    T::deserialize(deserializer)
}

/// An omitted member, an explicit JSON null, or a modeled value.
///
/// Generated structures omit `Absent` during serialization. `Null` is always
/// present on the wire, including for optional document members.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<T> Field<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    /// Whether this field is explicit null, clearing a clearable input member.
    pub fn is_clear(&self) -> bool {
        self.is_null()
    }
    /// Borrow the modeled value, if present.
    pub fn as_value(&self) -> Option<&T> {
        self.value()
    }
    /// Transform a value while preserving omission and explicit null.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Field<U> {
        match self {
            Self::Absent => Field::Absent,
            Self::Null => Field::Null,
            Self::Value(value) => Field::Value(f(value)),
        }
    }
    /// Absent or null: what a protocol that omits null members leaves out.
    pub fn is_unset(&self) -> bool {
        !matches!(self, Self::Value(_))
    }
    pub fn value(&self) -> Option<&T> {
        if let Self::Value(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn into_value(self) -> Option<T> {
        if let Self::Value(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_deref(&self) -> Option<&T::Target>
    where
        T: Deref,
    {
        self.value().map(Deref::deref)
    }
}

impl<T> From<T> for Field<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}
impl<T> From<Option<T>> for Field<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Absent, Self::Value)
    }
}
impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Absent | Self::Null => serializer.serialize_none(),
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Self::Null, Self::Value))
    }
}

#[cfg(test)]
mod tests {
    use super::Field;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Model {
        #[serde(default, skip_serializing_if = "Field::is_absent")]
        document: Field<Value>,
        #[serde(default, skip_serializing_if = "Field::is_absent")]
        values: Field<Vec<String>>,
    }

    #[test]
    fn clearing_helpers_distinguish_presence_and_borrow_the_value() {
        let absent = Field::<String>::Absent;
        let clear = Field::<String>::Null;
        let value = Field::Value("set".to_owned());
        assert!(!absent.is_clear());
        assert!(clear.is_clear());
        assert!(!value.is_clear());
        assert_eq!(absent.as_value(), None);
        assert_eq!(clear.as_value(), None);
        assert_eq!(value.as_value().map(String::as_str), Some("set"));
        assert!(!Field::Value(Value::Null).is_clear());
    }

    #[test]
    fn map_transforms_only_a_present_value() {
        let calls = std::cell::Cell::new(0);
        for (input, expected) in [
            (Field::Absent, Field::Absent),
            (Field::Null, Field::Null),
            (Field::Value(3), Field::Value("3".to_owned())),
        ] {
            assert_eq!(
                input.map(|value: i32| {
                    calls.set(calls.get() + 1);
                    value.to_string()
                }),
                expected
            );
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn missing_null_and_value_are_independent() {
        for wire in [
            json!({}),
            json!({"document":null,"values":null}),
            json!({"document":{},"values":[]}),
        ] {
            let model: Model = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&model).unwrap(), wire);
        }
        let missing: Model = serde_json::from_str("{}").unwrap();
        let null: Model = serde_json::from_str(r#"{"document":null}"#).unwrap();
        assert_eq!(missing.document, Field::Absent);
        assert_eq!(null.document, Field::Null);
        assert_eq!(
            serde_json::to_value(Field::Value(Value::Null)).unwrap(),
            Value::Null
        );
    }
}
