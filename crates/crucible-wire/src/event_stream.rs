//! Smithy event streams over the AWS event stream framing
//! (`application/vnd.amazon.eventstream`): the binary message format, an
//! incremental decoder, and the encoding of a `@streaming` union's events as
//! messages, driven by `'static` descriptions generated from the model.
//!
//! A message is a prelude (total length, headers length, CRC32 of those
//! eight bytes), typed headers, a payload, and a CRC32 of everything before
//! it. A Smithy event is a message with `:message-type` `event` and
//! `:event-type` naming the union member; a modeled error is `exception`
//! with `:exception-type`; an unmodeled one is `error` with `:error-code`
//! and `:error-message`.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
pub use bytes::Bytes;
use bytes::BytesMut;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::operation::TimestampFormat;
use crate::stream::ChunkStream;

/// The media type of an event stream body.
pub const CONTENT_TYPE: &str = "application/vnd.amazon.eventstream";
/// The largest headers section a message may carry.
pub const MAX_HEADERS: usize = 128 * 1024;
/// The largest payload a message may carry.
pub const MAX_PAYLOAD: usize = 24 * 1024 * 1024;
const PRELUDE: usize = 12;
const TRAILER: usize = 4;

static TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

/// CRC-32 (IEEE 802.3, as zlib computes it): both of a message's checksums.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc = TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// A header's typed value, by its wire type number (0 through 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderValue {
    /// Types 0 (true) and 1 (false): no value bytes.
    Bool(bool),
    Byte(i8),
    Short(i16),
    Integer(i32),
    Long(i64),
    Bytes(Bytes),
    String(String),
    /// Milliseconds since the Unix epoch.
    Timestamp(i64),
    Uuid([u8; 16]),
}

impl HeaderValue {
    fn type_number(&self) -> u8 {
        match self {
            Self::Bool(true) => 0,
            Self::Bool(false) => 1,
            Self::Byte(_) => 2,
            Self::Short(_) => 3,
            Self::Integer(_) => 4,
            Self::Long(_) => 5,
            Self::Bytes(_) => 6,
            Self::String(_) => 7,
            Self::Timestamp(_) => 8,
            Self::Uuid(_) => 9,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    /// Any integer header, widened.
    pub fn as_integer(&self) -> Option<i64> {
        match *self {
            Self::Byte(value) => Some(i64::from(value)),
            Self::Short(value) => Some(i64::from(value)),
            Self::Integer(value) => Some(i64::from(value)),
            Self::Long(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: HeaderValue,
}

/// One event stream message. Header order is kept, as it is part of the
/// encoded bytes a signature covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub headers: Vec<Header>,
    pub payload: Bytes,
}

/// A message that cannot be framed or read: a length out of range, a bad
/// checksum, a truncated or unknown header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError(pub String);

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid event stream message: {}", self.0)
    }
}

impl std::error::Error for FrameError {}

fn frame_error<T>(message: impl Into<String>) -> Result<T, FrameError> {
    Err(FrameError(message.into()))
}

impl Message {
    pub fn new(payload: impl Into<Bytes>) -> Self {
        Self {
            headers: Vec::new(),
            payload: payload.into(),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: HeaderValue) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value,
        });
        self
    }

    /// The first header with this name.
    pub fn header(&self, name: &str) -> Option<&HeaderValue> {
        self.headers
            .iter()
            .find(|header| header.name == name)
            .map(|header| &header.value)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        self.header(name).and_then(HeaderValue::as_str)
    }

    pub fn encode(&self) -> Result<Bytes, FrameError> {
        let mut headers = Vec::new();
        encode_headers(&self.headers, &mut headers)?;
        if headers.len() > MAX_HEADERS {
            return frame_error("headers exceed 128 KiB");
        }
        if self.payload.len() > MAX_PAYLOAD {
            return frame_error("payload exceeds 24 MiB");
        }
        let total = PRELUDE + headers.len() + self.payload.len() + TRAILER;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&(total as u32).to_be_bytes());
        out.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        out.extend_from_slice(&crc32(&out).to_be_bytes());
        out.extend_from_slice(&headers);
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&crc32(&out).to_be_bytes());
        Ok(out.into())
    }

    /// Exactly one complete message.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        let mut decoder = MessageDecoder::new();
        decoder.push(bytes);
        let message = decoder
            .next_message()?
            .ok_or_else(|| FrameError("truncated message".into()))?;
        if !decoder.is_empty() {
            return frame_error("bytes follow the message");
        }
        Ok(message)
    }
}

/// Headers as they are encoded inside a message: what a chunk signature
/// hashes for its `:date` header.
pub fn encode_headers(headers: &[Header], out: &mut Vec<u8>) -> Result<(), FrameError> {
    let mut names = std::collections::HashSet::new();
    for header in headers {
        if !names.insert(header.name.as_str()) {
            return frame_error("duplicate header name");
        }
        let name = header.name.as_bytes();
        if name.is_empty() || name.len() > 255 {
            return frame_error("header names are 1 to 255 bytes");
        }
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        out.push(header.value.type_number());
        match &header.value {
            HeaderValue::Bool(_) => {}
            HeaderValue::Byte(value) => out.push(*value as u8),
            HeaderValue::Short(value) => out.extend_from_slice(&value.to_be_bytes()),
            HeaderValue::Integer(value) => out.extend_from_slice(&value.to_be_bytes()),
            HeaderValue::Long(value) | HeaderValue::Timestamp(value) => {
                out.extend_from_slice(&value.to_be_bytes())
            }
            HeaderValue::Bytes(bytes) => variable(out, bytes)?,
            HeaderValue::String(text) => variable(out, text.as_bytes())?,
            HeaderValue::Uuid(bytes) => out.extend_from_slice(bytes),
        }
    }
    Ok(())
}

fn variable(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), FrameError> {
    let Ok(length) = u16::try_from(bytes.len()) else {
        return frame_error("header values are at most 65535 bytes");
    };
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn decode_headers(mut bytes: &[u8]) -> Result<Vec<Header>, FrameError> {
    fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8], FrameError> {
        if bytes.len() < count {
            return frame_error("a header runs past the headers section");
        }
        let (head, rest) = bytes.split_at(count);
        *bytes = rest;
        Ok(head)
    }
    let mut headers = Vec::new();
    let mut names = std::collections::HashSet::new();
    while !bytes.is_empty() {
        let length = take(&mut bytes, 1)?[0] as usize;
        if length == 0 {
            return frame_error("a header name is empty");
        }
        let name = std::str::from_utf8(take(&mut bytes, length)?)
            .map_err(|_| FrameError("a header name is not UTF-8".into()))?;
        if !names.insert(name) {
            return frame_error("duplicate header name");
        }
        let name = name.to_owned();
        let kind = take(&mut bytes, 1)?[0];
        let value = match kind {
            0 => HeaderValue::Bool(true),
            1 => HeaderValue::Bool(false),
            2 => HeaderValue::Byte(take(&mut bytes, 1)?[0] as i8),
            3 => HeaderValue::Short(i16::from_be_bytes(
                take(&mut bytes, 2)?.try_into().expect("two bytes"),
            )),
            4 => HeaderValue::Integer(i32::from_be_bytes(
                take(&mut bytes, 4)?.try_into().expect("four bytes"),
            )),
            5 | 8 => {
                let value = i64::from_be_bytes(take(&mut bytes, 8)?.try_into().expect("eight"));
                if kind == 5 {
                    HeaderValue::Long(value)
                } else {
                    HeaderValue::Timestamp(value)
                }
            }
            6 | 7 => {
                let length =
                    u16::from_be_bytes(take(&mut bytes, 2)?.try_into().expect("two")) as usize;
                let value = take(&mut bytes, length)?;
                if kind == 6 {
                    HeaderValue::Bytes(Bytes::copy_from_slice(value))
                } else {
                    HeaderValue::String(
                        std::str::from_utf8(value)
                            .map_err(|_| FrameError("a string header is not UTF-8".into()))?
                            .to_owned(),
                    )
                }
            }
            9 => HeaderValue::Uuid(take(&mut bytes, 16)?.try_into().expect("sixteen")),
            other => return frame_error(format!("unknown header value type {other}")),
        };
        headers.push(Header { name, value });
    }
    Ok(headers)
}

/// Reassembles messages from arbitrary chunks of a byte stream. Checksums
/// are verified before a message is returned, and the prelude's as soon as
/// it arrives, so a corrupt length never makes the decoder wait for data.
#[derive(Debug)]
pub struct MessageDecoder {
    buffer: BytesMut,
    validate_limits: bool,
}
impl Default for MessageDecoder {
    fn default() -> Self {
        Self {
            buffer: BytesMut::new(),
            validate_limits: true,
        }
    }
}

impl MessageDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Client receivers validate framing and checksums, but not service-only size limits.
    pub fn for_response() -> Self {
        Self {
            validate_limits: false,
            ..Self::default()
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// No partial message is waiting for more bytes.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// The next complete message, or `None` until more bytes arrive.
    pub fn next_message(&mut self) -> Result<Option<Message>, FrameError> {
        if self.buffer.len() < PRELUDE {
            return Ok(None);
        }
        let word =
            |at: usize| u32::from_be_bytes(self.buffer[at..at + 4].try_into().expect("four bytes"));
        let (total, headers, prelude) = (word(0) as usize, word(4) as usize, word(8));
        let actual = crc32(&self.buffer[..8]);
        if actual != prelude {
            return frame_error(format!(
                "prelude checksum {prelude:#010x} does not match {actual:#010x}"
            ));
        }
        if self.validate_limits && headers > MAX_HEADERS {
            return frame_error("headers exceed 128 KiB");
        }
        if total < PRELUDE + TRAILER + headers {
            return frame_error("the message is shorter than its headers");
        }
        if self.validate_limits && total - PRELUDE - TRAILER - headers > MAX_PAYLOAD {
            return frame_error("payload exceeds 24 MiB");
        }
        if self.buffer.len() < total {
            return Ok(None);
        }
        let frame = self.buffer.split_to(total).freeze();
        let expected = u32::from_be_bytes(frame[total - 4..].try_into().expect("four bytes"));
        let actual = crc32(&frame[..total - 4]);
        if actual != expected {
            return frame_error(format!(
                "message checksum {expected:#010x} does not match {actual:#010x}"
            ));
        }
        let payload = frame.slice(PRELUDE + headers..total - TRAILER);
        let headers = decode_headers(&frame[PRELUDE..PRELUDE + headers])?;
        Ok(Some(Message { headers, payload }))
    }
}

/// An event stream union, as generated server and client markers describe it.
#[derive(Debug)]
#[doc(hidden)]
pub struct EventStreamInfo {
    /// The protocol sends the structure's other members as an
    /// `initial-request` / `initial-response` event (protocols without HTTP
    /// bindings) instead of in HTTP bindings.
    pub initial_message: bool,
    /// The `:content-type` of a JSON document payload and of the initial message.
    pub document_type: &'static str,
    pub events: &'static [EventInfo],
}

/// One union member: an event, or an `@error` structure the stream may end with.
#[derive(Debug)]
#[doc(hidden)]
pub struct EventInfo {
    /// The union member's name: its `:event-type` or `:exception-type`.
    pub name: &'static str,
    /// The union member's key in the union's JSON form (its `@jsonName`).
    pub json: &'static str,
    pub error: bool,
    /// Members bound to headers or to the payload. The others form a JSON
    /// document payload, unless one member is bound to the payload.
    pub members: &'static [EventMember],
    /// Native document codecs, when the event's document is CBOR.
    pub encode_document: Option<DocumentEncoder>,
    pub decode_document: Option<DocumentDecoder>,
}

pub type DocumentEncoder = fn(&Value) -> Result<Bytes, String>;
pub type DocumentDecoder = fn(&[u8]) -> Result<Value, String>;

#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct EventMember {
    /// The member's name: a header member's header name.
    pub name: &'static str,
    /// The member's key in the event structure's JSON form.
    pub json: &'static str,
    pub binding: EventBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum EventBinding {
    Header(EventHeaderKind),
    Payload(EventPayloadKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum EventHeaderKind {
    Boolean,
    Byte,
    Short,
    Integer,
    Long,
    Blob,
    String,
    /// The member's JSON value is in this format; the header holds milliseconds.
    Timestamp(TimestampFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum EventPayloadKind {
    Blob,
    String,
    Document,
}

impl EventStreamInfo {
    fn event(&self, name: &str) -> Option<&'static EventInfo> {
        self.events.iter().find(|event| event.name == name)
    }

    fn event_by_json(&self, key: &str) -> Option<&'static EventInfo> {
        self.events.iter().find(|event| event.json == key)
    }

    /// The message carrying an event, given the union's JSON form
    /// `{"member": {...}}`.
    pub fn encode(&self, event: &Value) -> Result<Message, String> {
        let (name, value) = match event {
            Value::Object(object) if object.len() == 1 => object.iter().next().expect("one"),
            _ => return Err("an event is a union with exactly one member".into()),
        };
        let info = self
            .event_by_json(name)
            .ok_or_else(|| format!("`{name}` is not an event of this stream"))?;
        let name = info.name;
        let mut fields = match value {
            Value::Object(object) => object.clone(),
            Value::Null => Map::new(),
            _ => return Err(format!("event `{name}` is not a structure")),
        };
        let mut message = if info.error {
            Message::default()
                .with_header(":message-type", HeaderValue::String("exception".into()))
                .with_header(":exception-type", HeaderValue::String(name.to_owned()))
        } else {
            Message::default()
                .with_header(":message-type", HeaderValue::String("event".into()))
                .with_header(":event-type", HeaderValue::String(name.to_owned()))
        };
        let mut payload = None;
        let mut headers = Vec::new();
        for member in info.members {
            let value = fields.remove(member.json).unwrap_or(Value::Null);
            match member.binding {
                EventBinding::Header(kind) => {
                    if !value.is_null() {
                        let value = header_value(kind, &value).ok_or_else(|| {
                            format!("`{name}.{}` is not a valid header", member.name)
                        })?;
                        headers.push(Header {
                            name: member.name.to_owned(),
                            value,
                        });
                    }
                }
                EventBinding::Payload(kind) => payload = Some((kind, value)),
            }
        }
        let (content_type, bytes) = match payload {
            Some((EventPayloadKind::Blob, value)) => (
                "application/octet-stream",
                match value {
                    Value::Null => Bytes::new(),
                    Value::String(text) => STANDARD
                        .decode(text)
                        .map_err(|_| format!("`{name}` payload is not a blob"))?
                        .into(),
                    _ => return Err(format!("`{name}` payload is not a blob")),
                },
            ),
            Some((EventPayloadKind::String, value)) => (
                "text/plain",
                match value {
                    Value::Null => Bytes::new(),
                    Value::String(text) => text.into(),
                    _ => return Err(format!("`{name}` payload is not a string")),
                },
            ),
            Some((EventPayloadKind::Document, Value::Null)) => (self.document_type, Bytes::new()),
            Some((EventPayloadKind::Document, value)) => (
                self.document_type,
                match info.encode_document {
                    Some(encode) => encode(&value)?,
                    None => serde_json::to_vec(&value)
                        .map_err(|error| error.to_string())?
                        .into(),
                },
            ),
            None => (
                self.document_type,
                match info.encode_document {
                    Some(encode) => encode(&Value::Object(fields))?,
                    None => serde_json::to_vec(&Value::Object(fields))
                        .map_err(|error| error.to_string())?
                        .into(),
                },
            ),
        };
        message = message.with_header(":content-type", HeaderValue::String(content_type.into()));
        message.headers.extend(headers);
        message.payload = bytes;
        Ok(message)
    }

    /// The `initial-request` message: the structure's other members, as
    /// the protocol's JSON document.
    pub fn initial_request(&self, document: Bytes) -> Message {
        Message::new(document)
            .with_header(":message-type", HeaderValue::String("event".into()))
            .with_header(":event-type", HeaderValue::String("initial-request".into()))
            .with_header(
                ":content-type",
                HeaderValue::String(self.document_type.into()),
            )
    }

    /// What a received message is.
    pub fn decode(&self, message: &Message) -> Result<Decoded, String> {
        match message.string(":message-type") {
            Some("event") => {
                let name = message
                    .string(":event-type")
                    .ok_or("an event message has no :event-type")?;
                if matches!(name, "initial-response" | "initial-request") {
                    return Ok(Decoded::Initial(message.payload.clone()));
                }
                match self.event(name).filter(|event| !event.error) {
                    Some(info) => Ok(Decoded::Event(self.fields(info, message)?)),
                    // A newer service's event: the union's unknown member.
                    None => Ok(Decoded::Event(Value::Object(Map::from_iter([(
                        name.to_owned(),
                        unknown_payload(&message.payload),
                    )])))),
                }
            }
            Some("exception") => {
                let name = message
                    .string(":exception-type")
                    .ok_or("an exception message has no :exception-type")?;
                match self.event(name).filter(|event| event.error) {
                    Some(info) => Ok(Decoded::Exception(self.fields(info, message)?)),
                    None => {
                        let body: Value =
                            serde_json::from_slice(&message.payload).unwrap_or(Value::Null);
                        let text = body
                            .get("message")
                            .or_else(|| body.get("Message"))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        Ok(Decoded::Error {
                            code: name.to_owned(),
                            message: text.to_owned(),
                        })
                    }
                }
            }
            Some("error") => Ok(Decoded::Error {
                code: message.string(":error-code").unwrap_or_default().to_owned(),
                message: message
                    .string(":error-message")
                    .unwrap_or_default()
                    .to_owned(),
            }),
            Some(other) => Err(format!("unknown :message-type `{other}`")),
            None => Err("a message has no :message-type".into()),
        }
    }

    /// `{"member": {...}}` from the payload and the bound headers.
    fn fields(&self, info: &EventInfo, message: &Message) -> Result<Value, String> {
        let name = info.name;
        let payload = info.members.iter().find_map(|member| match member.binding {
            EventBinding::Payload(kind) => Some((member.json, kind)),
            EventBinding::Header(_) => None,
        });
        let mut fields = match payload {
            None if message.payload.is_empty() => Map::new(),
            None => match match info.decode_document {
                Some(decode) => decode(&message.payload),
                None => serde_json::from_slice(&message.payload).map_err(|error| error.to_string()),
            } {
                Ok(Value::Object(object)) => object,
                _ => return Err(format!("event `{name}` payload is not a JSON object")),
            },
            Some((member, kind)) => {
                let mut fields = Map::new();
                let value = match kind {
                    EventPayloadKind::Blob => Value::String(STANDARD.encode(&message.payload)),
                    EventPayloadKind::String => Value::String(
                        std::str::from_utf8(&message.payload)
                            .map_err(|_| format!("event `{name}` payload is not UTF-8"))?
                            .to_owned(),
                    ),
                    EventPayloadKind::Document if message.payload.is_empty() => Value::Null,
                    EventPayloadKind::Document => match info.decode_document {
                        Some(decode) => decode(&message.payload)?,
                        None => serde_json::from_slice(&message.payload)
                            .map_err(|_| format!("event `{name}` payload is not JSON"))?,
                    },
                };
                if !value.is_null() {
                    fields.insert(member.to_owned(), value);
                }
                fields
            }
        };
        for member in info.members {
            let EventBinding::Header(kind) = member.binding else {
                continue;
            };
            if let Some(value) = message.header(member.name) {
                let value = json_value(kind, value).ok_or_else(|| {
                    format!("header `{}` of `{name}` has the wrong type", member.name)
                })?;
                fields.insert(member.json.to_owned(), value);
            }
        }
        Ok(Value::Object(Map::from_iter([(
            info.json.to_owned(),
            Value::Object(fields),
        )])))
    }
}

/// A received message, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// An event, as the union's JSON form.
    Event(Value),
    /// An `initial-response` (or `initial-request`) event's payload.
    Initial(Bytes),
    /// A modeled error, as the union's JSON form. The stream ends.
    Exception(Value),
    /// An unmodeled error. The stream ends.
    Error { code: String, message: String },
}

fn unknown_payload(payload: &Bytes) -> Value {
    serde_json::from_slice(payload).unwrap_or_else(|_| Value::String(STANDARD.encode(payload)))
}

fn header_value(kind: EventHeaderKind, value: &Value) -> Option<HeaderValue> {
    Some(match kind {
        EventHeaderKind::Boolean => HeaderValue::Bool(value.as_bool()?),
        EventHeaderKind::Byte => HeaderValue::Byte(i8::try_from(value.as_i64()?).ok()?),
        EventHeaderKind::Short => HeaderValue::Short(i16::try_from(value.as_i64()?).ok()?),
        EventHeaderKind::Integer => HeaderValue::Integer(i32::try_from(value.as_i64()?).ok()?),
        EventHeaderKind::Long => HeaderValue::Long(value.as_i64()?),
        EventHeaderKind::Blob => HeaderValue::Bytes(STANDARD.decode(value.as_str()?).ok()?.into()),
        EventHeaderKind::String => HeaderValue::String(value.as_str()?.to_owned()),
        EventHeaderKind::Timestamp(_) => {
            HeaderValue::Timestamp(crate::time::from_json(value)?.timestamp_millis())
        }
    })
}

fn json_value(kind: EventHeaderKind, value: &HeaderValue) -> Option<Value> {
    Some(match (kind, value) {
        (EventHeaderKind::Boolean, HeaderValue::Bool(value)) => Value::Bool(*value),
        (EventHeaderKind::Byte, value) => i8::try_from(value.as_integer()?).ok()?.into(),
        (EventHeaderKind::Short, value) => i16::try_from(value.as_integer()?).ok()?.into(),
        (EventHeaderKind::Integer, value) => i32::try_from(value.as_integer()?).ok()?.into(),
        (EventHeaderKind::Long, value) => value.as_integer()?.into(),
        (EventHeaderKind::Blob, HeaderValue::Bytes(bytes)) => Value::String(STANDARD.encode(bytes)),
        (EventHeaderKind::String, HeaderValue::String(text)) => Value::String(text.clone()),
        (EventHeaderKind::Timestamp(format), HeaderValue::Timestamp(millis)) => {
            crate::time::to_json(&chrono::DateTime::from_timestamp_millis(*millis)?, format).ok()?
        }
        _ => return None,
    })
}

/// Why an event stream ended early.
#[derive(Debug)]
pub enum EventStreamError<T> {
    /// A modeled error: the union's `@error` member. The stream has ended.
    Exception(T),
    /// An error the model does not name (`:message-type error`, or an
    /// exception type this client does not know). The stream has ended.
    Error { code: String, message: String },
    /// Bytes that are not a valid message, or an event that does not match
    /// the model.
    Decode(String),
    /// The connection failed, the call deadline expired, the call was
    /// cancelled, or the caller's own event source failed.
    Io(io::Error),
}

impl<T: fmt::Debug> fmt::Display for EventStreamError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exception(error) => write!(f, "event stream exception: {error:?}"),
            Self::Error { code, message } => write!(f, "event stream error {code}: {message}"),
            Self::Decode(message) => write!(f, "unreadable event stream: {message}"),
            Self::Io(error) => write!(f, "event stream: {error}"),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for EventStreamError<T> {}

impl<T> EventStreamError<T> {
    fn map<U>(self, convert: impl FnOnce(T) -> Result<U, String>) -> EventStreamError<U> {
        match self {
            Self::Exception(value) => match convert(value) {
                Ok(value) => EventStreamError::Exception(value),
                Err(message) => EventStreamError::Decode(message),
            },
            Self::Error { code, message } => EventStreamError::Error { code, message },
            Self::Decode(message) => EventStreamError::Decode(message),
            Self::Io(error) => EventStreamError::Io(error),
        }
    }
}

/// Events on their way out, as the union's JSON form: what a client sends.
pub type Outgoing = Pin<Box<dyn Stream<Item = io::Result<Value>> + Send + 'static>>;
/// Events received, as the union's JSON form: what a client reads.
pub type Incoming = Pin<Box<dyn Stream<Item = Result<Value, EventStreamError<Value>>> + Send>>;
type Source<T> = Pin<Box<dyn Stream<Item = Result<T, EventStreamError<T>>> + Send + 'static>>;

/// A one-shot stream of events: the value of an event stream member. To
/// send, build it from a `Stream` of events; it is read as the transport
/// sends, so a slow connection slows the source. A received one is read
/// once with [`EventStream::open`]. Clones share the stream.
pub struct EventStream<T> {
    source: Arc<Mutex<Option<Source<T>>>>,
}

impl<T: Send + 'static> EventStream<T> {
    pub fn new<S>(events: S) -> Self
    where
        S: Stream<Item = T> + Send + 'static,
    {
        Self::from_source(Box::pin(events.map(Ok)))
    }

    /// Events from a fallible source: an error ends the stream, and the
    /// call that sends it fails.
    pub fn try_new<S, E>(events: S) -> Self
    where
        S: Stream<Item = Result<T, E>> + Send + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::from_source(Box::pin(events.map(|event| {
            event.map_err(|error| {
                EventStreamError::Io(match error.into().downcast::<io::Error>() {
                    Ok(error) => *error,
                    Err(error) => io::Error::other(error),
                })
            })
        })))
    }

    pub fn from_events<I>(events: I) -> Self
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: Send + 'static,
    {
        Self::new(futures_util::stream::iter(events))
    }

    pub fn empty() -> Self {
        Self::from_source(Box::pin(futures_util::stream::empty()))
    }

    fn from_source(source: Source<T>) -> Self {
        Self {
            source: Arc::new(Mutex::new(Some(source))),
        }
    }

    /// Takes the events. Dropping the receiver releases the connection.
    pub fn open(&self) -> io::Result<EventReceiver<T>> {
        let source = self
            .source
            .lock()
            .map_err(|_| io::Error::other("event stream lock poisoned"))?
            .take()
            .ok_or_else(|| io::Error::other("event stream already opened"))?;
        Ok(EventReceiver {
            source: Some(source),
        })
    }

    /// Releases an unopened stream, and with it a received stream's connection.
    pub fn close(&self) {
        if let Ok(mut source) = self.source.lock() {
            source.take();
        }
    }

    /// For the transport: the events to send, as JSON.
    #[doc(hidden)]
    pub fn outgoing(&self) -> io::Result<Outgoing>
    where
        T: Serialize,
    {
        let receiver = self.open()?;
        Ok(Box::pin(receiver.map(|event| match event {
            Ok(event) => serde_json::to_value(&event).map_err(io::Error::other),
            Err(EventStreamError::Io(error)) => Err(error),
            Err(_) => Err(io::Error::other("the event source failed")),
        })))
    }

    /// For the transport: received events, typed.
    #[doc(hidden)]
    pub fn incoming(events: Incoming) -> Self
    where
        T: DeserializeOwned,
    {
        let typed = |value: Value| serde_json::from_value::<T>(value).map_err(|e| e.to_string());
        Self::from_source(Box::pin(events.map(move |event| match event {
            Ok(value) => typed(value).map_err(EventStreamError::Decode),
            Err(error) => Err(error.map(typed)),
        })))
    }

    /// Server input events are checked as they arrive, before delivery to the operation.
    #[doc(hidden)]
    pub fn incoming_validated(events: Incoming) -> Self
    where
        T: DeserializeOwned + crate::validate::Validate,
    {
        let typed = |value: Value| {
            let value: T = serde_json::from_value(value).map_err(|e| e.to_string())?;
            crate::validate::Validator::check(&value).map_err(|error| error.summary())?;
            Ok(value)
        };
        Self::from_source(Box::pin(events.map(move |event| match event {
            Ok(value) => typed(value).map_err(EventStreamError::Decode),
            Err(error) => Err(error.map(typed)),
        })))
    }
}

impl<T: Send + 'static> Default for EventStream<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> Clone for EventStream<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}

impl<T> fmt::Debug for EventStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

impl<T> PartialEq for EventStream<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
    }
}

/// The stream travels outside the JSON document; a structure holding one
/// serializes it as nothing.
impl<T> Serialize for EventStream<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

impl<'de, T: Send + 'static> Deserialize<'de> for EventStream<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <()>::deserialize(deserializer)?;
        Ok(Self::empty())
    }
}

/// An opened event stream: events in order, then `None`. After an error the
/// stream has ended.
pub struct EventReceiver<T> {
    source: Option<Source<T>>,
}

impl<T> EventReceiver<T> {
    pub async fn recv(&mut self) -> Result<Option<T>, EventStreamError<T>> {
        futures_util::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx))
            .await
            .transpose()
    }
}

impl<T> Stream for EventReceiver<T> {
    type Item = Result<T, EventStreamError<T>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Some(source) = self.source.as_mut() else {
            return Poll::Ready(None);
        };
        match source.as_mut().poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(event))) => Poll::Ready(Some(Ok(event))),
            Poll::Ready(item) => {
                // An error or the end: nothing follows, and the connection goes.
                self.source = None;
                Poll::Ready(item)
            }
        }
    }
}

impl<T> fmt::Debug for EventReceiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventReceiver")
            .field("open", &self.source.is_some())
            .finish()
    }
}

/// Incremental event reader shared by clients and servers.
pub struct Reader {
    info: &'static EventStreamInfo,
    chunks: ChunkStream,
    decoder: MessageDecoder,
    ended: bool,
}

impl Reader {
    pub fn new(info: &'static EventStreamInfo, chunks: ChunkStream) -> Self {
        Self {
            info,
            chunks,
            decoder: MessageDecoder::for_response(),
            ended: false,
        }
    }

    /// Server receivers also enforce the service-side event frame size limits.
    pub fn for_request(info: &'static EventStreamInfo, chunks: ChunkStream) -> Self {
        Self {
            decoder: MessageDecoder::new(),
            ..Self::new(info, chunks)
        }
    }

    pub async fn next_message(&mut self) -> Option<Result<Message, EventStreamError<Value>>> {
        loop {
            if self.ended {
                return None;
            }
            match self.decoder.next_message() {
                Ok(Some(message)) => {
                    return Some(Ok(message));
                }
                Ok(None) => {}
                Err(error) => {
                    self.ended = true;
                    return Some(Err(EventStreamError::Decode(error.to_string())));
                }
            }
            match self.chunks.next().await {
                Some(Ok(chunk)) => self.decoder.push(&chunk),
                Some(Err(error)) => {
                    self.ended = true;
                    return Some(Err(EventStreamError::Io(error)));
                }
                None => {
                    self.ended = true;
                    if !self.decoder.is_empty() {
                        return Some(Err(EventStreamError::Decode(
                            "the body ended inside a message".into(),
                        )));
                    }
                    return None;
                }
            }
        }
    }

    pub async fn next(&mut self) -> Option<Result<Decoded, EventStreamError<Value>>> {
        let message = self.next_message().await?;
        let decoded = match message {
            Ok(message) => self.info.decode(&message).map_err(EventStreamError::Decode),
            Err(error) => Err(error),
        };
        if decoded.is_err() {
            self.ended = true;
        }
        Some(decoded)
    }

    /// Server inputs accept event messages only; an initial document belongs at the beginning.
    pub fn into_request_incoming(self) -> Incoming {
        Box::pin(futures_util::stream::unfold(
            Some(self),
            |state| async move {
                let mut reader = state?;
                let result = match reader.next().await? {
                    Ok(Decoded::Event(event)) => Ok(event),
                    Ok(_) => Err(EventStreamError::Decode(
                        "request streams accept event messages only".into(),
                    )),
                    Err(error) => Err(error),
                };
                let more = result.is_ok().then_some(reader);
                Some((result, more))
            },
        ))
    }

    /// The events, starting with `pending` when the transport already read
    /// one while looking for an initial response. A modeled exception or
    /// error ends the stream.
    pub fn into_incoming(
        self,
        pending: Option<Result<Decoded, EventStreamError<Value>>>,
    ) -> Incoming {
        Box::pin(futures_util::stream::unfold(
            Some((self, pending)),
            |state| async move {
                let (mut reader, mut pending) = state?;
                loop {
                    let next = match pending.take() {
                        Some(next) => next,
                        None => reader.next().await?,
                    };
                    let error = match next {
                        Ok(Decoded::Event(event)) => {
                            return Some((Ok(event), Some((reader, None))));
                        }
                        // A late or repeated initial message carries no event.
                        Ok(Decoded::Initial(_)) => continue,
                        Ok(Decoded::Exception(error)) => EventStreamError::Exception(error),
                        Ok(Decoded::Error { code, message }) => {
                            EventStreamError::Error { code, message }
                        }
                        Err(error) => error,
                    };
                    return Some((Err(error), None));
                }
            },
        ))
    }
}
