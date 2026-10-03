//! One service, lowered: what every emitter reads.
//!
//! Smithy's rules are settled here, once, so no emitter re-derives them:
//! which members are optional, the effective constraints on a member, where
//! each member travels in a request or response, the errors an operation can
//! return, and its pagination. Traits the IR does not model are kept raw in
//! each `traits` map, so a project's own traits reach the emitters intact.

use std::collections::BTreeMap;
use std::fmt;

pub use serde_json::{Number, Value};

use crate::shape_id::ShapeId;

pub type Traits = BTreeMap<ShapeId, Value>;

#[derive(Debug, Clone)]
pub struct Service {
    pub id: ShapeId,
    pub version: String,
    pub title: Option<String>,
    pub docs: Option<String>,
    /// Auth schemes the service declares, in declaration order.
    pub auth: Vec<ShapeId>,
    /// Sorted by name, so output does not depend on model file order.
    pub operations: Vec<Operation>,
    /// Every shape an operation reaches, prelude shapes included.
    pub shapes: BTreeMap<ShapeId, Shape>,
    pub traits: Traits,
    pub(crate) renames: BTreeMap<ShapeId, String>,
}

impl Service {
    pub fn shape(&self, id: &ShapeId) -> Option<&Shape> {
        self.shapes.get(id)
    }

    /// The name a shape goes by in this service: its `rename`, else its own.
    pub fn name_of<'a>(&'a self, id: &'a ShapeId) -> &'a str {
        self.renames.get(id).map_or(id.name(), String::as_str)
    }

    /// How a timestamp member is written where it travels: the member's own
    /// format, else its target's, else the protocol default for the location.
    pub fn timestamp_format(&self, member: &Member, location: Location) -> TimestampFormat {
        let target = match self.shapes.get(&member.target) {
            Some(Shape::Simple(simple)) => simple.timestamp_format,
            _ => None,
        };
        member
            .timestamp_format
            .or(target)
            .unwrap_or(match location {
                Location::Body => TimestampFormat::EpochSeconds,
                Location::Header => TimestampFormat::HttpDate,
                Location::Label | Location::Query => TimestampFormat::DateTime,
            })
    }
}

#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "built once per shape and only read; boxing would only add indirection for emitters"
)]
pub enum Shape {
    /// `smithy.api#Unit`: no value, as an operation's input or output or as
    /// a union member that carries nothing.
    Unit,
    Simple(Simple),
    Enum(Enum),
    List(List),
    Map(Map),
    Structure(Structure),
    Union(Structure),
}

#[derive(Debug, Clone)]
pub struct Simple {
    pub kind: SimpleKind,
    pub constraints: Constraints,
    pub timestamp_format: Option<TimestampFormat>,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleKind {
    Boolean,
    String,
    Byte,
    Short,
    Integer,
    Long,
    Float,
    Double,
    Timestamp,
    Document,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub values: Vec<EnumValue>,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    /// The member name, for a language symbol.
    pub name: String,
    /// What goes on the wire.
    pub value: String,
    pub docs: Option<String>,
}

#[derive(Debug, Clone)]
pub struct List {
    pub member: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub key: Member,
    pub value: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Structure {
    pub members: Vec<Member>,
    /// Present on a shape marked `@error`.
    pub error: Option<ErrorInfo>,
    pub docs: Option<String>,
    pub traits: Traits,
}

impl Structure {
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub target: ShapeId,
    pub required: bool,
    pub default: Option<Value>,
    /// `@clientOptional`, or a member of an `@input` structure: a client may
    /// omit it even when the server requires it.
    pub client_optional: bool,
    /// The target's constraints with the member's own laid over them.
    pub constraints: Constraints,
    /// The member's own `@timestampFormat`; see [`Service::timestamp_format`].
    pub timestamp_format: Option<TimestampFormat>,
    pub docs: Option<String>,
    pub traits: Traits,
}

impl Member {
    /// Smithy 2.0 nullability as the server sees it.
    pub fn optional_on_server(&self) -> bool {
        !self.required && self.default.is_none()
    }

    pub fn optional_on_client(&self) -> bool {
        self.client_optional || self.optional_on_server()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Constraints {
    pub length: Option<Bounds<u64>>,
    pub range: Option<Bounds<Number>>,
    pub pattern: Option<String>,
    pub unique_items: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub fault: Fault,
    pub status: u16,
    pub retryable: Option<Retryable>,
    /// Members sent as response headers; the rest form the body.
    pub headers: Vec<Binding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retryable {
    pub throttling: bool,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub id: ShapeId,
    pub docs: Option<String>,
    pub method: String,
    pub uri: Uri,
    /// The status of a successful response.
    pub status: u16,
    pub input: Option<ShapeId>,
    pub output: Option<ShapeId>,
    /// The operation's own errors, then the service's.
    pub errors: Vec<ShapeId>,
    pub readonly: bool,
    pub idempotent: bool,
    /// The input member a client fills with a fresh token when it is absent.
    pub idempotency_token: Option<String>,
    pub paginated: Option<Paginated>,
    pub request: Bindings,
    pub response: Bindings,
    pub traits: Traits,
}

impl Operation {
    pub fn name(&self) -> &str {
        self.id.name()
    }
}

/// Where each member of an input or output structure travels.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    pub labels: Vec<String>,
    pub query: Vec<Binding>,
    pub headers: Vec<Binding>,
    /// Members serialized into the JSON body, in member order.
    pub body: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub member: String,
    /// The query parameter or header name.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Label,
    Query,
    Header,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampFormat {
    DateTime,
    HttpDate,
    EpochSeconds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paginated {
    pub input_token: String,
    pub output_token: String,
    pub items: Option<String>,
    pub page_size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uri {
    pub segments: Vec<Segment>,
    /// Literal query parameters in the pattern, such as `?x-id=Get`.
    pub query: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Label(String),
}

impl Uri {
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Label(name) => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            f.write_str("/")?;
        }
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => write!(f, "/{text}")?,
                Segment::Label(name) => write!(f, "/{{{name}}}")?,
            }
        }
        for (index, (key, value)) in self.query.iter().enumerate() {
            f.write_str(if index == 0 { "?" } else { "&" })?;
            match value {
                Some(value) => write!(f, "{key}={value}")?,
                None => f.write_str(key)?,
            }
        }
        Ok(())
    }
}
