//! Reading an input from its bound members (labels, query parameters,
//! headers) and its JSON body in one pass: a deserializer that presents both
//! as one map, so the typed input is built straight from the request with no
//! JSON value in between. An `@httpPayload` member is bound too: the body's
//! bytes, handed to the `Blob` without a copy.

use std::borrow::Cow;
use std::marker::PhantomData;

use bytes::Bytes;
use serde::de::value::{BorrowedStrDeserializer, CowStrDeserializer};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, IgnoredAny, IntoDeserializer, MapAccess,
    SeqAccess, Visitor,
};

use crate::operation::Scalar;
use crate::{blob, text, time};

/// A bound member's value, already checked against its kind, so a bad one
/// is a violation naming the parameter or header before deserializing starts.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundValue<'a> {
    Text(Cow<'a, str>),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<BoundValue<'a>>),
    /// A payload member: the whole body.
    Bytes(Bytes),
}

impl<'a> BoundValue<'a> {
    /// A bound member's text as its kind, or why it is not one: what a
    /// server reads from a label, query parameter or header, and a client
    /// from a response header.
    pub fn parse(text: Cow<'a, str>, scalar: Scalar) -> Result<Self, String> {
        match scalar {
            Scalar::String | Scalar::Json => Ok(Self::Text(text)),
            Scalar::Integer => text
                .parse::<i64>()
                .map(Self::Integer)
                .map_err(|_| format!("must be an integer (`{text}` is not)")),
            Scalar::Float | Scalar::EpochSeconds => text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Float)
                .ok_or_else(|| format!("must be a number (`{text}` is not)")),
            Scalar::Boolean => match &*text {
                "true" => Ok(Self::Boolean(true)),
                "false" => Ok(Self::Boolean(false)),
                _ => Err("must be true or false".to_owned()),
            },
            Scalar::DateTime => match time::parse_date_time(&text) {
                Some(_) => Ok(Self::Text(text)),
                None => Err("must be an RFC 3339 timestamp".to_owned()),
            },
            Scalar::HttpDate => match time::http_date::parse(&text) {
                Some(_) => Ok(Self::Text(text)),
                None => Err("must be an HTTP date".to_owned()),
            },
            Scalar::List(item) => text::split_header(&text)
                .into_iter()
                .map(|item_text| Self::parse(Cow::Owned(item_text), *item))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
        }
    }
}

/// `T` from `body` (a JSON object, or empty for `{}`) with `bound` laid over
/// it. A body member that is also bound is ignored: the bound value wins.
pub fn read<'a, T: DeserializeOwned>(
    body: &'a [u8],
    bound: Vec<(&'static str, BoundValue<'a>)>,
) -> Result<T, serde_json::Error> {
    let names: Vec<&'static str> = bound.iter().map(|(name, _)| *name).collect();
    if body.iter().all(u8::is_ascii_whitespace) {
        return T::deserialize(BoundOnly { bound });
    }
    let mut json = serde_json::Deserializer::from_slice(body);
    let value = json.deserialize_map(Bridge {
        bound,
        names,
        target: PhantomData::<T>,
    })?;
    json.end()?;
    Ok(value)
}

/// Hands serde_json's map access to the target type, chained after the
/// bound members.
struct Bridge<'b, T> {
    bound: Vec<(&'static str, BoundValue<'b>)>,
    names: Vec<&'static str>,
    target: PhantomData<T>,
}

impl<'de, T: DeserializeOwned> Visitor<'de> for Bridge<'de, T> {
    type Value = T;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, json: A) -> Result<T, A::Error> {
        T::deserialize(Merged {
            access: Chain {
                bound: self.bound.into_iter(),
                names: self.names,
                value: None,
                json: Some(json),
            },
        })
    }
}

/// A deserializer over a ready map access: what the target type sees.
struct Merged<A> {
    access: A,
}

impl<'de, A: MapAccess<'de>> Deserializer<'de> for Merged<A> {
    type Error = A::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, A::Error> {
        visitor.visit_map(self.access)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

/// The bound members alone, for a request with no body.
struct BoundOnly<'b> {
    bound: Vec<(&'static str, BoundValue<'b>)>,
}

impl<'de> Deserializer<'de> for BoundOnly<'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(Chain::<'de, NoBody> {
            bound: self.bound.into_iter(),
            names: Vec::new(),
            value: None,
            json: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

/// The empty body's map access, so `Chain` has one shape.
struct NoBody;

impl<'de> MapAccess<'de> for NoBody {
    type Error = serde_json::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        _: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, _: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("no value without a key"))
    }
}

/// Bound members first, then the body's members, skipping any the bound
/// members already gave.
struct Chain<'b, A> {
    bound: std::vec::IntoIter<(&'static str, BoundValue<'b>)>,
    names: Vec<&'static str>,
    /// The bound value whose key was just handed out.
    value: Option<BoundValue<'b>>,
    json: Option<A>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Chain<'de, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        if let Some((name, value)) = self.bound.next() {
            self.value = Some(value);
            return seed
                .deserialize(BorrowedStrDeserializer::new(name))
                .map(Some);
        }
        let Some(json) = self.json.as_mut() else {
            return Ok(None);
        };
        loop {
            let Some(Key(key)) = json.next_key::<Key<'de>>()? else {
                return Ok(None);
            };
            if self.names.iter().any(|name| *name == key) {
                json.next_value::<IgnoredAny>()?;
                continue;
            }
            return seed.deserialize(CowStrDeserializer::new(key)).map(Some);
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        match (self.value.take(), self.json.as_mut()) {
            (Some(value), _) => seed.deserialize(Bound::new(value)),
            (None, Some(json)) => json.next_value_seed(seed),
            (None, None) => Err(de::Error::custom("no value without a key")),
        }
    }
}

/// A map key, borrowed from the body when it has no escapes.
struct Key<'de>(Cow<'de, str>);

impl<'de> de::Deserialize<'de> for Key<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = Key<'de>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a member name")
            }

            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Borrowed(v)))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(v.to_owned())))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(KeyVisitor)
    }
}

/// One bound value, deserialized as whatever the target member is.
struct Bound<'b, E> {
    value: BoundValue<'b>,
    error: PhantomData<E>,
}

impl<'b, E> Bound<'b, E> {
    fn new(value: BoundValue<'b>) -> Self {
        Self {
            value,
            error: PhantomData,
        }
    }
}

impl<'de, E: de::Error> Deserializer<'de> for Bound<'de, E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            BoundValue::Text(Cow::Borrowed(text)) => visitor.visit_borrowed_str(text),
            BoundValue::Text(Cow::Owned(text)) => visitor.visit_string(text),
            BoundValue::Integer(n) => visitor.visit_i64(n),
            BoundValue::Float(n) => visitor.visit_f64(n),
            BoundValue::Boolean(b) => visitor.visit_bool(b),
            BoundValue::List(items) => visitor.visit_seq(Items {
                items: items.into_iter(),
                error: PhantomData,
            }),
            BoundValue::Bytes(bytes) => visitor.visit_bytes(&bytes),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, E> {
        match self.value {
            BoundValue::Bytes(bytes) if name == blob::TOKEN => {
                blob::hand_off(bytes, || visitor.visit_unit())
            }
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        visitor.visit_some(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        match self.value {
            BoundValue::Text(Cow::Borrowed(text)) => IntoDeserializer::<E>::into_deserializer(text)
                .deserialize_enum(name, variants, visitor),
            BoundValue::Text(Cow::Owned(text)) => IntoDeserializer::<E>::into_deserializer(text)
                .deserialize_enum(name, variants, visitor),
            _ => Err(de::Error::custom("an enum value is text")),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct Items<'b, E> {
    items: std::vec::IntoIter<BoundValue<'b>>,
    error: PhantomData<E>,
}

impl<'de, E: de::Error> SeqAccess<'de> for Items<'de, E> {
    type Error = E;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, E> {
        self.items
            .next()
            .map(|item| seed.deserialize(Bound::new(item)))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    enum Sky {
        Clear,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Input {
        city_id: String,
        max_results: Option<i32>,
        tags: Option<Vec<String>>,
        sky: Option<Sky>,
        name: Option<String>,
        nested: Option<Nested>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Nested {
        x: f64,
    }

    #[test]
    fn bound_members_and_body_read_as_one_object() {
        let bound = vec![
            ("cityId", BoundValue::Text(Cow::Borrowed("c-1"))),
            ("maxResults", BoundValue::Integer(5)),
            (
                "tags",
                BoundValue::List(vec![
                    BoundValue::Text("a".into()),
                    BoundValue::Text("b".into()),
                ]),
            ),
            ("sky", BoundValue::Text("clear".into())),
        ];
        let input: Input = read(
            br#"{"name":"Oslo","cityId":"ignored","nested":{"x":1.5}}"#,
            bound,
        )
        .unwrap();
        assert_eq!(
            input,
            Input {
                city_id: "c-1".into(),
                max_results: Some(5),
                tags: Some(vec!["a".into(), "b".into()]),
                sky: Some(Sky::Clear),
                name: Some("Oslo".into()),
                nested: Some(Nested { x: 1.5 }),
            }
        );
    }

    #[test]
    fn an_empty_body_reads_the_bound_members_alone() {
        let input: Input = read(b"", vec![("cityId", BoundValue::Text("c-2".into()))]).unwrap();
        assert_eq!(input.city_id, "c-2");
        assert_eq!(input.name, None);
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Upload {
        caption: Option<String>,
        photo: crate::blob::Blob,
        thumbnail: Option<crate::blob::Blob>,
    }

    #[test]
    fn a_payload_member_takes_the_body_without_copying_it() {
        let body = Bytes::from_static(b"\xff\x00{\"a\":1}");
        let upload: Upload = read(
            b"",
            vec![
                ("caption", BoundValue::Text("sea".into())),
                ("photo", BoundValue::Bytes(body.clone())),
            ],
        )
        .unwrap();
        assert_eq!(upload.photo.as_bytes(), &body[..]);
        assert_eq!(
            upload.photo.as_ptr(),
            body.as_ptr(),
            "shares the body's buffer"
        );
        assert_eq!(upload.caption.as_deref(), Some("sea"));
        assert_eq!(upload.thumbnail, None);
    }

    #[test]
    fn a_missing_payload_is_a_missing_member() {
        let error = read::<Upload>(b"", vec![]).unwrap_err();
        assert!(
            error.to_string().contains("missing field `photo`"),
            "{error}"
        );
    }

    #[test]
    fn a_body_that_is_not_an_object_fails() {
        assert!(read::<Input>(b"[1]", vec![("cityId", BoundValue::Text("c".into()))]).is_err());
    }
}
