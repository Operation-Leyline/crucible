//! One service, lowered: what every emitter reads.
//!
//! Smithy's rules are settled here, once, so no emitter re-derives them:
//! which members are optional, the effective constraints on a member, where
//! each member travels in a request or response, the errors an operation can
//! return, and its pagination. Traits the IR does not model are kept raw in
//! each `traits` map, so a project's own traits reach the emitters intact.

use std::collections::BTreeMap;
use std::fmt;

pub use crate::cors::Cors;
pub use crate::discovery::{DiscoveredEndpoint, EndpointDiscovery};
pub use crate::extensions::*;
pub use crate::jmespath_ir::{JmesComparator, JmesEntry, JmesExpression, JmesFunction};
pub use crate::protocol_tests::{
    HttpMessageExpectation, HttpProtocolTest, HttpTestCase, TestAppliesTo,
};
pub use crate::recursion::RecursiveGroup;
pub use crate::xml::{XmlMember, XmlNamespace, XmlShape};
pub use crate::{Protocol, ProtocolSettings};
pub use serde_json::{Number, Value};

use crate::shape_id::ShapeId;

pub type Traits = BTreeMap<ShapeId, Value>;

#[derive(Debug, Clone)]
pub struct Service {
    pub cors: Option<Cors>,
    pub endpoint_discovery: Option<EndpointDiscovery>,
    pub recursive_groups: Vec<RecursiveGroup>,
    pub xml_namespace: Option<XmlNamespace>,
    pub xml_shapes: BTreeMap<ShapeId, XmlShape>,
    pub protocol: Protocol,
    pub protocol_settings: ProtocolSettings,
    pub requirements: Vec<crate::FeatureUse>,
    pub id: ShapeId,
    pub version: String,
    pub title: Option<String>,
    pub docs: Option<String>,
    /// Auth schemes the service declares, in declaration order.
    pub auth: Vec<ShapeId>,
    pub auth_schemes: BTreeMap<ShapeId, AuthScheme>,
    pub endpoint_rules: Option<EndpointRuleSet>,
    pub protocol_tests: Vec<HttpProtocolTest>,
    /// Sorted by name, so output does not depend on model file order.
    pub operations: Vec<Operation>,
    /// Resources bound to this service, including nested resources. Populated
    /// only by `Model::lower_infrastructure`, as are roots and references.
    pub resources: BTreeMap<ShapeId, Resource>,
    /// Direct service bindings, sorted by shape ID.
    pub resource_roots: Vec<ShapeId>,
    /// Design-time resource references, keyed by the referencing shape.
    pub resource_references: BTreeMap<ShapeId, Vec<ResourceReference>>,
    /// Every shape an operation reaches, prelude shapes included. Infrastructure
    /// lowering additionally includes resource identifier/property targets.
    pub shapes: BTreeMap<ShapeId, Shape>,
    pub traits: Traits,
    pub(crate) renames: BTreeMap<ShapeId, String>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub id: ShapeId,
    pub identifiers: BTreeMap<String, ShapeId>,
    pub properties: BTreeMap<String, ShapeId>,
    pub resources: Vec<ShapeId>,
    pub lifecycle: ResourceLifecycle,
    pub operations: Vec<ShapeId>,
    pub collection_operations: Vec<ShapeId>,
    /// Bindings are per resource: an operation may belong to several resources.
    pub bindings: BTreeMap<ShapeId, ResourceOperationBindings>,
    pub traits: Traits,
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceLifecycle {
    pub create: Option<ShapeId>,
    pub put: Option<ShapeId>,
    pub read: Option<ShapeId>,
    pub update: Option<ShapeId>,
    pub delete: Option<ShapeId>,
    pub list: Option<ShapeId>,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceOperationBindings {
    pub input: ResourceMemberBindings,
    pub output: ResourceMemberBindings,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceMemberBindings {
    /// Resource identifier name to member path, starting at the input/output.
    pub identifiers: BTreeMap<String, Vec<String>>,
    /// Resource property name to member path, starting at the input/output.
    pub properties: BTreeMap<String, Vec<String>>,
    /// Collection member name to bindings relative to each list element.
    /// These are repeated resource instances, not scalar member paths.
    pub collections: BTreeMap<String, ResourceMemberBindings>,
}

#[derive(Debug, Clone)]
pub struct ResourceReference {
    pub resource: ShapeId,
    pub service: Option<ShapeId>,
    /// Identifier names to member paths; an empty path denotes the string itself.
    pub identifiers: BTreeMap<String, Vec<String>>,
    pub rel: Option<String>,
    /// External references are valid Smithy and may not resolve in this model.
    pub resolved: bool,
}

impl Service {
    /// Emitters must check their own capabilities even for externally lowered IR.
    pub fn require_capabilities(
        &self,
        capabilities: &crate::Capabilities,
    ) -> Result<(), crate::Diagnostics> {
        capabilities.check(&self.requirements)
    }
    /// Effective sensitivity: a member or its target can forbid logging.
    pub fn sensitive(&self, member: &Member) -> bool {
        member
            .traits
            .contains_key(&ShapeId::new("smithy.api", "sensitive"))
            || self.shape(&member.target).is_some_and(Shape::sensitive)
    }

    pub fn shape(&self, id: &ShapeId) -> Option<&Shape> {
        self.shapes.get(id)
    }

    /// The name a shape goes by in this service: its `rename`, else its own.
    pub fn name_of<'a>(&'a self, id: &'a ShapeId) -> &'a str {
        self.renames.get(id).map_or(id.name(), String::as_str)
    }

    /// Timestamp encoding inside a document; HTTP annotations have no effect here.
    pub fn document_timestamp_format(&self, member: &Member) -> TimestampFormat {
        member
            .timestamp_format
            .or_else(|| match self.shape(&member.target) {
                Some(Shape::Simple(simple)) => simple.timestamp_format,
                _ => None,
            })
            .unwrap_or_else(|| self.protocol.timestamp_format(false))
    }

    pub fn document_wire_default(&self, member: &Member) -> Option<Value> {
        let value = member.client_default()?;
        if matches!(
            self.shape(&member.target),
            Some(Shape::Simple(Simple {
                kind: SimpleKind::Timestamp,
                ..
            }))
        ) {
            crate::timestamp_default::wire_value(value, self.document_timestamp_format(member))
        } else {
            Some(value.clone())
        }
    }

    /// Timestamp format resolved during lowering, including only effective HTTP bindings.
    pub fn timestamp_format(&self, member: &Member) -> TimestampFormat {
        member.wire_timestamp_format
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
    IntEnum(IntEnum),
    List(List),
    Map(Map),
    Structure(Structure),
    Union(Structure),
}

impl Shape {
    pub fn sensitive(&self) -> bool {
        let traits = match self {
            Self::Unit => return false,
            Self::Simple(s) => &s.traits,
            Self::Enum(s) => &s.traits,
            Self::IntEnum(s) => &s.traits,
            Self::List(s) => &s.traits,
            Self::Map(s) => &s.traits,
            Self::Structure(s) | Self::Union(s) => &s.traits,
        };
        traits.contains_key(&ShapeId::new("smithy.api", "sensitive"))
    }
}

#[derive(Debug, Clone)]
pub struct Simple {
    pub streaming: bool,
    pub requires_length: bool,
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
    BigInteger,
    BigDecimal,
    Timestamp,
    Document,
    /// Bytes: base64 in a JSON body, raw as an `@httpPayload`.
    Blob,
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
pub struct IntEnum {
    pub values: Vec<IntEnumValue>,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct IntEnumValue {
    pub name: String,
    pub value: i32,
    pub docs: Option<String>,
}

#[derive(Debug, Clone)]
pub struct List {
    pub sparse: bool,
    pub is_set: bool,
    pub member: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub sparse: bool,
    pub key: Member,
    pub value: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Structure {
    pub streaming: bool,
    pub members: Vec<Member>,
    /// Present on a shape marked `@error`.
    pub error: Option<ErrorInfo>,
    pub docs: Option<String>,
    pub traits: Traits,
}

impl Structure {
    pub fn sensitive(&self) -> bool {
        self.traits
            .contains_key(&ShapeId::new("smithy.api", "sensitive"))
    }

    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub event_binding: Option<EventBinding>,
    pub name: String,
    /// JSON property name; HTTP and public API names remain independent.
    pub wire_name: String,
    pub target: ShapeId,
    pub required: bool,
    pub default: Option<Value>,
    /// The default represented in the member's resolved wire format.
    pub wire_default: Option<Value>,
    /// `@clientOptional`, or a member of an `@input` structure: a client may
    /// omit it even when the server requires it.
    pub client_optional: bool,
    /// The target's constraints with the member's own laid over them.
    pub constraints: Constraints,
    /// The member's own `@timestampFormat`; see [`Service::timestamp_format`].
    pub timestamp_format: Option<TimestampFormat>,
    /// Effective timestamp format for this member's top-level wire location.
    /// Nested document encoding uses the document format instead.
    pub wire_timestamp_format: TimestampFormat,
    pub docs: Option<String>,
    pub traits: Traits,
}

impl Member {
    pub fn client_wire_default(&self) -> Option<&Value> {
        if self.client_optional {
            None
        } else {
            self.wire_default.as_ref()
        }
    }

    /// Defaults do not apply to non-authoritative readers of client-optional members.
    pub fn client_default(&self) -> Option<&Value> {
        if self.client_optional {
            None
        } else {
            self.default.as_ref()
        }
    }

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

impl Constraints {
    /// Translate the ECMA-262 pattern for Rust's Unicode scalar regex engine.
    /// The original pattern remains available for other targets and diagnostics.
    pub fn rust_pattern(&self) -> Result<Option<String>, String> {
        self.pattern
            .as_deref()
            .map(crate::pattern::rust)
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub timestamp_formats: BTreeMap<String, TimestampFormat>,
    pub wire_code: String,
    pub payload: Option<Payload>,
    pub header_encodings: BTreeMap<String, HeaderEncoding>,
    pub fault: Fault,
    pub status: u16,
    pub retryable: Option<Retryable>,
    /// Members sent as response headers; the rest form the body.
    pub headers: Vec<Binding>,
    pub prefix_headers: Vec<Binding>,
    pub response_code: Option<String>,
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
    /// The sole output member is the restXml root rather than its child.
    pub unwrapped_xml_output: bool,
    pub unsigned_payload: bool,
    pub discovered_endpoint: Option<DiscoveredEndpoint>,
    pub checksum: Option<Checksum>,
    pub id: ShapeId,
    /// Effective authentication schemes, after service and operation overrides.
    pub auth: Vec<ShapeId>,
    pub optional_auth: bool,
    pub endpoint: Option<EndpointHostPrefix>,
    pub endpoint_parameters: BTreeMap<String, Vec<EndpointParameterSource>>,
    pub waiters: BTreeMap<String, Waiter>,
    /// The resource that binds the operation; `None` when the service does.
    pub resource: Option<ShapeId>,
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
    pub timestamp_formats: BTreeMap<String, TimestampFormat>,
    pub header_encodings: BTreeMap<String, HeaderEncoding>,
    pub labels: Vec<String>,
    pub query: Vec<Binding>,
    pub headers: Vec<Binding>,
    pub query_params: Option<String>,
    pub prefix_headers: Vec<Binding>,
    pub response_code: Option<String>,
    /// Members serialized into the JSON body, in member order. Empty when
    /// there is a payload. An event stream member is never a body member;
    /// with `event_stream.initial_message` these members form the initial
    /// event's document instead of the HTTP body.
    pub body: Vec<String>,
    /// The member carrying an event stream, when the structure has one.
    pub event_stream: Option<EventStreamBinding>,
    /// The member that is the whole body, raw (`@httpPayload` on a blob).
    pub payload: Option<Payload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub kind: PayloadKind,
    pub streaming: bool,
    pub requires_length: bool,
    pub member: String,
    /// The target's `@mediaType`, else `application/octet-stream`: the
    /// `Content-Type` a sender gives the body.
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub member: String,
    /// The query parameter or header name.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
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
    /// Labels whose values preserve slash separators when percent-encoded.
    pub greedy_labels: Vec<String>,
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
                Segment::Label(name) => write!(
                    f,
                    "/{{{name}{}}}",
                    if self.greedy_labels.contains(name) {
                        "+"
                    } else {
                        ""
                    }
                )?,
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
