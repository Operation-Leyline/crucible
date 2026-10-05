//! Writing a structure as a JSON body with its header members taken out, in
//! one pass: the body goes straight to bytes, and each header member becomes
//! header text on the way. A structure with an `@httpPayload` member writes
//! that member's bytes as the body instead, raw.

use serde::ser::{self, Impossible, Serialize, SerializeStruct, Serializer};
use serde_json::Value;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::operation::{Location, MemberInfo, ResponseBindings};

/// Writes `value` (a structure) into `out` as a JSON object without the
/// members `members` binds to headers, and returns those as (header, text).
/// With a payload member, `out` gets that member's bytes instead.
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
    if members.iter().any(|m| {
        matches!(
            m.location,
            Location::PrefixHeaders(_) | Location::ResponseCode
        )
    }) {
        return Err(<serde_json::Error as ser::Error>::custom(
            "dynamic response bindings require write_http",
        ));
    }
    let mut taken = Vec::new();
    value.serialize(Split {
        members,
        take: Take::Headers,
        type_name,
        payload: Location::payload(members).is_some(),
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

/// Serializes a response once, collecting dynamic headers and its status.
pub fn write_http<T: Serialize + ?Sized>(
    value: &T,
    members: &[MemberInfo],
    type_name: Option<&'static str>,
    out: &mut Vec<u8>,
) -> serde_json::Result<ResponseBindings> {
    let mut taken = Vec::new();
    value.serialize(Split {
        members,
        take: Take::Headers,
        type_name,
        payload: Location::payload(members).is_some(),
        out,
        taken: &mut taken,
    })?;
    let mut result = ResponseBindings::default();
    for (member, value) in taken {
        match member.location {
            Location::Header(name) => result.headers.push((name.to_owned(), text(&value))),
            Location::PrefixHeaders(prefix) => {
                let values = value.as_object().ok_or_else(|| {
                    <serde_json::Error as ser::Error>::custom("prefix headers must be a map")
                })?;
                for (key, value) in values {
                    let name = format!("{prefix}{key}").to_ascii_lowercase();
                    if members.iter().any(|m| matches!(m.location, Location::Header(explicit) if explicit.eq_ignore_ascii_case(&name))) { continue; }
                    let value = value.as_str().ok_or_else(|| {
                        <serde_json::Error as ser::Error>::custom(
                            "prefix header values must be strings",
                        )
                    })?;
                    result.headers.push((name, value.to_owned()));
                }
            }
            Location::ResponseCode => {
                result.status = Some(
                    value
                        .as_u64()
                        .filter(|v| (100..=599).contains(v))
                        .ok_or_else(|| {
                            <serde_json::Error as ser::Error>::custom(
                                "response status must be an integer from 100 through 599",
                            )
                        })? as u16,
                )
            }
            _ => {}
        }
    }
    Ok(result)
}

/// Writes `value` (an input) into `out` as a JSON object of its body
/// members only, and returns every bound member (label, query parameter or
/// header) with its value: what a client needs to build the request. With a
/// payload member, `out` gets that member's bytes instead.
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
        payload: Location::payload(members).is_some(),
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
    /// The structure has a payload member: `out` is its bytes, not JSON.
    payload: bool,
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
        if self.split.payload {
            let member = self.split.members.iter().find(|member| member.name == key);
            if member.is_some_and(|member| {
                matches!(
                    member.location,
                    Location::Payload(_) | Location::StreamingPayload { .. }
                )
            }) {
                return value.serialize(Raw {
                    out: &mut *self.split.out,
                });
            }
        }
        let take = self.split.take;
        let taken = self.split.members.iter().find(|member| {
            member.name == key
                && matches!(
                    (take, member.location),
                    (
                        _,
                        Location::Header(_) | Location::PrefixHeaders(_) | Location::ResponseCode
                    ) | (
                        Take::Bound,
                        Location::Label | Location::Query(_) | Location::QueryParams
                    )
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
        if self.split.payload {
            return Err(beside_payload());
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
        if self.split.payload {
            return Ok(());
        }
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
            None if self.fields.split.payload => Err(beside_payload()),
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

fn beside_payload() -> serde_json::Error {
    ser::Error::custom("a structure with a payload member has no body members")
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
        if !self.payload {
            self.out.push(b'{');
        }
        Ok(Fields {
            split: self,
            first: true,
        })
    }

    fn serialize_unit(self) -> serde_json::Result<()> {
        if !self.payload {
            self.out.extend_from_slice(b"{}");
        }
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
        if !self.payload {
            self.out.push(b'{');
        }
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

/// Writes a payload member's bytes, raw. A `Blob` gives its bytes to a
/// serializer that is not human-readable; a JSON value of one (the default
/// `Output::write_json_split`) is base64 text, decoded here.
struct Raw<'s> {
    out: &'s mut Vec<u8>,
}

fn not_a_blob() -> serde_json::Error {
    ser::Error::custom("a payload member is a blob")
}

impl Serializer for Raw<'_> {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Impossible<(), serde_json::Error>;
    type SerializeTuple = Impossible<(), serde_json::Error>;
    type SerializeTupleStruct = Impossible<(), serde_json::Error>;
    type SerializeTupleVariant = Impossible<(), serde_json::Error>;
    type SerializeMap = Impossible<(), serde_json::Error>;
    type SerializeStruct = Impossible<(), serde_json::Error>;
    type SerializeStructVariant = Impossible<(), serde_json::Error>;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_bytes(self, bytes: &[u8]) -> serde_json::Result<()> {
        self.out.extend_from_slice(bytes);
        Ok(())
    }
    fn serialize_str(self, text: &str) -> serde_json::Result<()> {
        STANDARD
            .decode_vec(text, self.out)
            .map_err(|_| ser::Error::custom("a payload's JSON value is base64"))
    }
    fn serialize_none(self) -> serde_json::Result<()> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> serde_json::Result<()> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> serde_json::Result<()> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> serde_json::Result<()> {
        value.serialize(self)
    }

    fn serialize_bool(self, _: bool) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_i8(self, _: i8) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_i16(self, _: i16) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_i32(self, _: i32) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_i64(self, _: i64) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_u8(self, _: u8) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_u16(self, _: u16) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_u32(self, _: u32) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_u64(self, _: u64) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_f32(self, _: f32) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_f64(self, _: f64) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_char(self, _: char) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_unit_struct(self, _: &'static str) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> serde_json::Result<()> {
        Err(not_a_blob())
    }
    fn serialize_seq(self, _: Option<usize>) -> serde_json::Result<Self::SerializeSeq> {
        Err(not_a_blob())
    }
    fn serialize_tuple(self, _: usize) -> serde_json::Result<Self::SerializeTuple> {
        Err(not_a_blob())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeTupleStruct> {
        Err(not_a_blob())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeTupleVariant> {
        Err(not_a_blob())
    }
    fn serialize_map(self, _: Option<usize>) -> serde_json::Result<Self::SerializeMap> {
        Err(not_a_blob())
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeStruct> {
        Err(not_a_blob())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> serde_json::Result<Self::SerializeStructVariant> {
        Err(not_a_blob())
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
            redaction: &crate::operation::Redaction::Public,
            traits: &[],
        },
        MemberInfo {
            name: "requestId",
            location: Location::Header("x-request-id"),
            scalar: Scalar::String,
            redaction: &crate::operation::Redaction::Public,
            traits: &[],
        },
        MemberInfo {
            name: "tags",
            location: Location::Header("x-tags"),
            scalar: Scalar::List(&Scalar::String),
            redaction: &crate::operation::Redaction::Public,
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

    #[derive(Serialize)]
    struct Upload {
        caption: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        photo: Option<crate::blob::Blob>,
    }

    const UPLOAD: &[MemberInfo] = &[
        MemberInfo {
            name: "caption",
            location: Location::Header("x-caption"),
            scalar: Scalar::String,
            redaction: &crate::operation::Redaction::Public,
            traits: &[],
        },
        MemberInfo {
            name: "photo",
            location: Location::Payload("image/jpeg"),
            scalar: Scalar::Json,
            redaction: &crate::operation::Redaction::Public,
            traits: &[],
        },
    ];

    #[test]
    fn a_payload_member_is_the_body_raw() {
        let bytes: &'static [u8] = b"\xff\xd8{\"not\":\"json\"}";
        let upload = Upload {
            caption: Some("sea"),
            photo: Some(bytes.into()),
        };
        let mut body = Vec::new();
        let headers = write(&upload, UPLOAD, &mut body).unwrap();
        assert_eq!(body, bytes);
        assert_eq!(headers, [("x-caption", "sea".to_owned())]);

        // Through a JSON value, the default `Output` path, the bytes are the same.
        let mut body = Vec::new();
        write(&serde_json::to_value(&upload).unwrap(), UPLOAD, &mut body).unwrap();
        assert_eq!(body, bytes);

        let mut body = Vec::new();
        let bound = write_bound(&upload, UPLOAD, &mut body).unwrap();
        assert_eq!(body, bytes);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].0.name, "caption");
    }

    #[test]
    fn an_absent_payload_writes_no_body() {
        let upload = Upload {
            caption: None,
            photo: None,
        };
        let mut body = Vec::new();
        assert!(write(&upload, UPLOAD, &mut body).unwrap().is_empty());
        assert!(body.is_empty());
    }
}
