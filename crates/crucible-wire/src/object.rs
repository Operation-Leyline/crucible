//! Smithy encodes a structure as a JSON object, and only as one. serde's
//! derived struct visitor also takes a sequence of positional values, and
//! serde_json offers one for `[`: left alone, `[]` or `["Oslo", 1]` would read
//! as a structure, at any depth of a body or as a whole `@httpPayload`.
//! [`Objects`] wraps a deserializer so every structure it reaches is refused
//! unless it is a map. Every typed JSON read
//! of a model value goes through it, so the server and the Rust client agree.

use std::fmt;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess,
    Unexpected, VariantAccess, Visitor,
};

/// `T` from exactly one JSON value in `bytes`, structures only from objects.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let mut json = serde_json::Deserializer::from_slice(bytes);
    let value = T::deserialize(Objects(&mut json))?;
    json.end()?;
    Ok(value)
}

/// `T` from a JSON value, structures only from objects.
pub fn from_value<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, serde_json::Error> {
    T::deserialize(Objects(value))
}

/// A deserializer that reads a structure only from a map, at every depth.
pub struct Objects<D>(pub D);

/// Wraps the visitor of every method, so the values it is handed are wrapped
/// in turn.
macro_rules! forward {
    ($($method:ident),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
                self.0.$method(Wrap::value(visitor))
            }
        )*
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Objects<D> {
    type Error = D::Error;

    forward! {
        deserialize_any, deserialize_bool, deserialize_i8, deserialize_i16, deserialize_i32,
        deserialize_i64, deserialize_i128, deserialize_u8, deserialize_u16, deserialize_u32,
        deserialize_u64, deserialize_u128, deserialize_f32, deserialize_f64, deserialize_char,
        deserialize_str, deserialize_string, deserialize_bytes, deserialize_byte_buf,
        deserialize_option, deserialize_unit, deserialize_seq, deserialize_map,
        deserialize_identifier, deserialize_ignored_any,
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_unit_struct(name, Wrap::value(visitor))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_newtype_struct(name, Wrap::value(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_tuple(len, Wrap::value(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_tuple_struct(name, len, Wrap::value(visitor))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_struct(name, fields, Wrap::structure(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_enum(name, variants, Wrap::value(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

/// A visitor that wraps what it is handed; for a structure, it refuses a
/// sequence.
struct Wrap<V> {
    visitor: V,
    structure: bool,
}

impl<V> Wrap<V> {
    fn value(visitor: V) -> Self {
        Self {
            visitor,
            structure: false,
        }
    }

    fn structure(visitor: V) -> Self {
        Self {
            visitor,
            structure: true,
        }
    }
}

macro_rules! visit {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(
            fn $method<E: de::Error>(self, value: $ty) -> Result<V::Value, E> {
                self.visitor.$method(value)
            }
        )*
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }

    visit! {
        visit_bool(bool), visit_i8(i8), visit_i16(i16), visit_i32(i32), visit_i64(i64),
        visit_i128(i128), visit_u8(u8), visit_u16(u16), visit_u32(u32), visit_u64(u64),
        visit_u128(u128), visit_f32(f32), visit_f64(f64), visit_char(char), visit_str(&str),
        visit_borrowed_str(&'de str), visit_string(String), visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]), visit_byte_buf(Vec<u8>),
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.visitor.visit_some(Objects(deserializer))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<V::Value, D::Error> {
        self.visitor.visit_newtype_struct(Objects(deserializer))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        if self.structure {
            return Err(de::Error::invalid_type(Unexpected::Seq, &self));
        }
        self.visitor.visit_seq(Seq(seq))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_map(Map(map))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_enum(Enum(data))
    }
}

/// A seed whose deserializer is wrapped.
struct Seed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.0.deserialize(Objects(deserializer))
    }
}

struct Seq<A>(A);

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Seq<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.0.next_element_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct Map<A>(A);

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Map<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(Seed(seed))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, A::Error> {
        self.0.next_value_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct Enum<A>(A);

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Enum<A> {
    type Error = A::Error;
    type Variant = Variant<A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self::Variant), A::Error> {
        self.0
            .variant_seed(Seed(seed))
            .map(|(value, variant)| (value, Variant(variant)))
    }
}

struct Variant<A>(A);

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Variant<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.0.newtype_variant_seed(Seed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, Wrap::value(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, Wrap::structure(visitor))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    struct Point {
        x: f64,
        #[serde(default)]
        label: Option<String>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    enum Shape {
        #[serde(rename = "point")]
        Point(Point),
        #[serde(rename = "path")]
        Path(Vec<Point>),
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Body {
        #[serde(default)]
        point: Option<Point>,
        #[serde(default)]
        points: Vec<Point>,
        #[serde(default)]
        named: BTreeMap<String, Point>,
        #[serde(default)]
        shape: Option<Shape>,
        #[serde(default)]
        field: crate::field::Field<Point>,
        #[serde(default)]
        boxed: Option<Box<Point>>,
        #[serde(default)]
        document: Option<serde_json::Value>,
        #[serde(default)]
        tags: Vec<String>,
    }

    fn both(json: &str) -> [Result<Body, serde_json::Error>; 2] {
        [
            from_slice(json.as_bytes()),
            from_value(serde_json::from_str(json).unwrap()),
        ]
    }

    #[test]
    fn plain_serde_json_reads_a_structure_from_an_array() {
        // What this module exists for: without it, these are accepted.
        assert!(serde_json::from_str::<Body>(r#"{"point":[1]}"#).is_ok());
        assert!(serde_json::from_str::<Body>("[]").is_ok());
    }

    #[test]
    fn a_structure_is_only_an_object_at_every_depth() {
        for json in [
            "[]",
            r#"[null, []]"#,
            r#"{"point":[]}"#,
            r#"{"point":[1]}"#,
            r#"{"point":[1, "a"]}"#,
            r#"{"points":[{"x":1},[2]]}"#,
            r#"{"named":{"a":[1]}}"#,
            r#"{"shape":{"point":[1]}}"#,
            r#"{"shape":{"path":[[1]]}}"#,
            r#"{"field":[1]}"#,
            r#"{"boxed":[1]}"#,
        ] {
            for result in both(json) {
                let error = result.expect_err(json).to_string();
                assert!(error.contains("invalid type: sequence"), "{json}: {error}");
            }
        }
    }

    #[test]
    fn everything_else_reads_as_before() {
        let json = r#"{"point":{"x":1},"points":[{"x":2,"label":"b"}],"named":{"a":{"x":3}},
            "shape":{"path":[{"x":4}]},"field":null,"boxed":{"x":5},
            "document":[1,{"x":[2]}],"tags":["t"]}"#;
        for body in both(json) {
            let body = body.unwrap();
            assert_eq!(
                body.point,
                Some(Point {
                    x: 1.0,
                    label: None
                })
            );
            assert_eq!(body.points[0].label.as_deref(), Some("b"));
            assert_eq!(body.named["a"].x, 3.0);
            assert_eq!(
                body.shape,
                Some(Shape::Path(vec![Point {
                    x: 4.0,
                    label: None
                }]))
            );
            assert!(body.field.is_null());
            assert_eq!(body.boxed.map(|point| point.x), Some(5.0));
            // A document is any JSON value, arrays included.
            assert_eq!(body.document, Some(json!([1, {"x": [2]}])));
            assert_eq!(body.tags, ["t"]);
        }
        for body in both(r#"{"point":null,"shape":null}"#) {
            let body = body.unwrap();
            assert_eq!((body.point, body.shape), (None, None));
        }
        // Trailing data is still refused.
        assert!(from_slice::<Body>(b"{} {}").is_err());
    }
}
