//! Writing a structure as a JSON body with its header members taken out, in
//! one pass: the body goes straight to bytes, and each header member becomes
//! header text on the way.

use serde::ser::{self, Impossible, Serialize, SerializeStruct, Serializer};
use serde_json::Value;

use crate::operation::{Location, MemberInfo};

/// Writes `value` (a structure) into `out` as a JSON object without the
/// members `members` binds to headers, and returns those as (header, text).
pub fn write<T: Serialize + ?Sized>(
    value: &T,
    members: &[MemberInfo],
    out: &mut Vec<u8>,
) -> serde_json::Result<Vec<(&'static str, String)>> {
    write_typed(value, members, None, out)
}

/// [`write`], with `"__type": type_name` appended when one is given: how an
/// error body names its error.
pub fn write_typed<T: Serialize + ?Sized>(
    value: &T,
    members: &[MemberInfo],
    type_name: Option<&'static str>,
    out: &mut Vec<u8>,
) -> serde_json::Result<Vec<(&'static str, String)>> {
    let mut taken = Vec::new();
    value.serialize(Split {
        members,
        take: Take::Headers,
        type_name,
        out,
        taken: &mut taken,
    })?;
    Ok(taken
        .into_iter()
        .filter_map(|(member, value)| match member.location {
            Location::Header(header) => Some((header, text(&value))),
            _ => None,
        })
        .collect())
}

/// Writes `value` (an input) into `out` as a JSON object of its body
/// members only, and returns every bound member (label, query parameter or
/// header) with its value: what a client needs to build the request.
pub fn write_bound<T: Serialize + ?Sized>(
    value: &T,
    members: &'static [MemberInfo],
    out: &mut Vec<u8>,
) -> serde_json::Result<Vec<(&'static MemberInfo, Value)>> {
    let mut taken = Vec::new();
    value.serialize(Split {
        members,
        take: Take::Bound,
        type_name: None,
        out,
        taken: &mut taken,
    })?;
    Ok(taken
        .into_iter()
        .map(|(member, value)| {
            let member = members
                .iter()
                .find(|m| m.name == member.name)
                .expect("taken members are this structure's");
            (member, value)
        })
        .collect())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    Headers,
    Bound,
}

struct Split<'s> {
    members: &'s [MemberInfo],
    take: Take,
    type_name: Option<&'static str>,
    out: &'s mut Vec<u8>,
    taken: &'s mut Vec<(MemberInfo, Value)>,
}

struct Fields<'s> {
    split: Split<'s>,
    first: bool,
}

impl SerializeStruct for Fields<'_> {
    type Ok = ();
    type Error = serde_json::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> serde_json::Result<()> {
        let take = self.split.take;
        let taken = self.split.members.iter().find(|member| {
            member.name == key
                && matches!(
                    (take, member.location),
                    (_, Location::Header(_)) | (Take::Bound, Location::Label | Location::Query(_))
                )
        });
        if let Some(member) = taken {
            let value = serde_json::to_value(value)?;
            let empty = matches!(&value, Value::Array(items) if items.is_empty());
            if !value.is_null() && !empty {
                self.split.taken.push((*member, value));
            }
            return Ok(());
        }
        if !self.first {
            self.split.out.push(b',');
        }
        self.first = false;
        serde_json::to_writer(&mut *self.split.out, key)?;
        self.split.out.push(b':');
        serde_json::to_writer(&mut *self.split.out, value)
    }

    fn end(self) -> serde_json::Result<()> {
        if let Some(name) = self.split.type_name {
            if !self.first {
                self.split.out.push(b',');
            }
            self.split.out.extend_from_slice(b"\"__type\":");
            serde_json::to_writer(&mut *self.split.out, name)?;
        }
        self.split.out.push(b'}');
        Ok(())
    }
}

/// A map with string keys (a JSON object value), written like a structure.
struct Entries<'s> {
    fields: Fields<'s>,
    key: Option<String>,
}

impl ser::SerializeMap for Entries<'_> {
    type Ok = ();
    type Error = serde_json::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> serde_json::Result<()> {
        match serde_json::to_value(key)? {
            Value::String(key) => {
                self.key = Some(key);
                Ok(())
            }
            _ => Err(ser::Error::custom("a member name is text")),
        }
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> serde_json::Result<()> {
        let key = self.key.take().unwrap_or_default();
        // Header members are matched by name; a key that is no member's is
        // written to the body like any other.
        match self.fields.split.members.iter().find(|m| m.name == key) {
            Some(member) => SerializeStruct::serialize_field(&mut self.fields, member.name, value),
            None => {
                if !self.fields.first {
                    self.fields.split.out.push(b',');
                }
                self.fields.first = false;
                serde_json::to_writer(&mut *self.fields.split.out, &key)?;
                self.fields.split.out.push(b':');
                serde_json::to_writer(&mut *self.fields.split.out, value)
            }
        }
    }

    fn end(self) -> serde_json::Result<()> {
        SerializeStruct::end(self.fields)
    }
}

/// A header member's value as header text: a list is comma-separated.
pub fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => crate::text::join_header(items.iter().map(text)),
        other => other.to_string(),
    }
}

fn not_a_structure() -> serde_json::Error {
    ser::Error::custom("an output with header members is a structure")
}

impl<'s> Serializer for Split<'s> {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Impossible<(), serde_json::Error>;
    type SerializeTuple = Impossible<(), serde_json::Error>;
    type SerializeTupleStruct = Impossible<(), serde_json::Error>;
    type SerializeTupleVariant = Impossible<(), serde_json::Error>;
    type SerializeMap = Entries<'s>;
    type SerializeStruct = Fields<'s>;
    type SerializeStructVariant = Impossible<(), serde_json::Error>;

    fn serialize_struct(self, _: &'static str, _: usize) -> serde_json::Result<Fields<'s>> {
        self.out.push(b'{');
        Ok(Fields {
            split: self,
            first: true,
        })
    }

    fn serialize_unit(self) -> serde_json::Result<()> {
        self.out.extend_from_slice(b"{}");
        Ok(())
    }

    fn serialize_bool(self, _: bool) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_i8(self, _: i8) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_i16(self, _: i16) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_i32(self, _: i32) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_i64(self, _: i64) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_u8(self, _: u8) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_u16(self, _: u16) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_u32(self, _: u32) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_u64(self, _: u64) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_f32(self, _: f32) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_f64(self, _: f64) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_char(self, _: char) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_str(self, _: &str) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_bytes(self, _: &[u8]) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_none(self) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_unit_struct(self, _: &'static str) -> serde_json::Result<()> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> serde_json::Result<()> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> serde_json::Result<()> {
        Err(not_a_structure())
    }
    fn serialize_seq(self, _: Option<usize>) -> serde_json::Result<Self::SerializeSeq> {
        Err(not_a_structure())
    }
    fn serialize_tuple(self, _: usize) -> serde_json::Result<Self::SerializeTuple> {
        Err(not_a_structure())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeTupleStruct> {
        Err(not_a_structure())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeTupleVariant> {
        Err(not_a_structure())
    }
    fn serialize_map(self, _: Option<usize>) -> serde_json::Result<Self::SerializeMap> {
        self.out.push(b'{');
        Ok(Entries {
            fields: Fields {
                split: self,
                first: true,
            },
            key: None,
        })
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeStructVariant> {
        Err(not_a_structure())
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;
    use crate::operation::Scalar;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Output {
        city: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        forecast: Option<u32>,
        request_id: Option<&'static str>,
        tags: Vec<&'static str>,
    }

    const MEMBERS: &[MemberInfo] = &[
        MemberInfo {
            name: "city",
            location: Location::Body,
            scalar: Scalar::Json,
            sensitive: false,
            traits: &[],
        },
        MemberInfo {
            name: "requestId",
            location: Location::Header("x-request-id"),
            scalar: Scalar::String,
            sensitive: false,
            traits: &[],
        },
        MemberInfo {
            name: "tags",
            location: Location::Header("x-tags"),
            scalar: Scalar::List(&Scalar::String),
            sensitive: false,
            traits: &[],
        },
    ];

    #[test]
    fn header_members_leave_the_body_as_header_text() {
        let output = Output {
            city: "Oslo",
            forecast: None,
            request_id: Some("r-1"),
            tags: vec!["a", "b,c"],
        };
        let mut body = Vec::new();
        let headers = write(&output, MEMBERS, &mut body).unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), r#"{"city":"Oslo"}"#);
        assert_eq!(
            headers,
            [
                ("x-request-id", "r-1".to_owned()),
                ("x-tags", "a, \"b,c\"".to_owned())
            ]
        );
    }

    #[test]
    fn an_absent_or_empty_header_member_sends_no_header() {
        let output = Output {
            city: "Oslo",
            forecast: Some(3),
            request_id: None,
            tags: vec![],
        };
        let mut body = Vec::new();
        let headers = write(&output, MEMBERS, &mut body).unwrap();
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"city":"Oslo","forecast":3}"#
        );
        assert!(headers.is_empty(), "{headers:?}");
    }
}
