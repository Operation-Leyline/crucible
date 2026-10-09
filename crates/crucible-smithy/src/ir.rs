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
pub use crate::doc_notes::{Deprecation, DocNotes};
pub use crate::extensions::*;
pub use crate::jmespath_ir::{JmesComparator, JmesEntry, JmesExpression, JmesFunction};
pub use crate::protocol_tests::{
    HttpMessageExpectation, HttpProtocolTest, HttpTestCase, MalformedBodyAssertion,
    MalformedRequestDefinition, MalformedResponseBody, MalformedResponseDefinition, TestAppliesTo,
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
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    pub endpoint_discovery: Option<EndpointDiscovery>,
    pub recursive_groups: Vec<RecursiveGroup>,
    pub xml_namespace: Option<XmlNamespace>,
    pub xml_shapes: BTreeMap<ShapeId, XmlShape>,
    pub protocol: Protocol,
    /// Declared supported protocols, sorted by trait ID; bindings use `protocol`.
    pub protocols: Vec<Protocol>,
    /// Errors declared on the service, independently of operation inheritance.
    pub errors: Vec<ShapeId>,
    pub protocol_settings: ProtocolSettings,
    pub requirements: Vec<crate::FeatureUse>,
    pub id: ShapeId,
    pub version: String,
    pub title: Option<String>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// Auth schemes the service declares, in declaration order.
    pub auth: Vec<ShapeId>,
    /// `@crucible.auth#scopes` on the service: the default for operations that
    /// declare none, and the scope a 401 challenge advertises. Model order.
    pub scopes: Vec<String>,
    pub auth_schemes: BTreeMap<ShapeId, AuthScheme>,
    pub endpoint_rules: Option<EndpointRuleSet>,
    pub protocol_tests: Vec<HttpProtocolTest>,
    /// Sorted by name, so output does not depend on model file order.
    pub operations: Vec<Operation>,
    /// Resources bound to this service, including nested resources. Populated
    /// by `Model::lower_infrastructure` and `Model::lower_documentation`.
    /// Only infrastructure lowering populates design-time references/bindings.
    pub resources: BTreeMap<ShapeId, Resource>,
    /// Direct service bindings, sorted by shape ID.
    pub resource_roots: Vec<ShapeId>,
    /// Design-time resource references, keyed by the referencing shape.
    pub resource_references: BTreeMap<ShapeId, Vec<ResourceReference>>,
    /// Every shape an operation reaches, prelude shapes included. Infrastructure
    /// lowering additionally includes resource identifier/property targets.
    pub shapes: BTreeMap<ShapeId, Shape>,
    pub traits: Traits,
    /// Service-local shape renames (Smithy `rename`); [`Service::name_of`]
    /// reads them. Public so other front ends can build a service.
    pub renames: BTreeMap<ShapeId, String>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
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
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
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
    /// Every scope the service declares, service default and operations, sorted
    /// and unique: what an authorization server or discovery document advertises.
    pub fn all_scopes(&self) -> Vec<&str> {
        self.scopes
            .iter()
            .chain(self.operations.iter().flat_map(|op| &op.scopes))
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Emitters must check their own capabilities even for externally lowered IR.
    pub fn require_capabilities(
        &self,
        capabilities: &crate::Capabilities,
    ) -> Result<(), crate::Diagnostics> {
        capabilities.check(&self.requirements)
    }
    /// Keeps only project metadata in every raw `traits` map, dropping
    /// Smithy's and Crucible's own traits, which the IR already carries as
    /// typed fields (decision 43). The `.cru` front end emits IR this way.
    pub fn project_metadata_only(&mut self) {
        let standard = |id: &ShapeId| {
            id.is_prelude()
                || id.namespace().starts_with("aws.")
                || id.namespace() == "crucible.protocols"
                || id.namespace() == "smithy.test"
        };
        let strip = |traits: &mut Traits| traits.retain(|id, _| !standard(id));
        strip(&mut self.traits);
        for operation in &mut self.operations {
            strip(&mut operation.traits);
        }
        for shape in self.shapes.values_mut() {
            match shape {
                Shape::Unit { .. } => {}
                Shape::Simple(s) => strip(&mut s.traits),
                Shape::Enum(s) => {
                    strip(&mut s.traits);
                    for value in &mut s.values {
                        strip(&mut value.traits);
                    }
                }
                Shape::IntEnum(s) => {
                    strip(&mut s.traits);
                    for value in &mut s.values {
                        strip(&mut value.traits);
                    }
                }
                Shape::List(s) => {
                    strip(&mut s.traits);
                    strip(&mut s.member.traits);
                }
                Shape::Map(s) => {
                    strip(&mut s.traits);
                    strip(&mut s.key.traits);
                    strip(&mut s.value.traits);
                }
                Shape::Structure(s) | Shape::Union(s) => {
                    strip(&mut s.traits);
                    for member in &mut s.members {
                        strip(&mut member.traits);
                    }
                }
            }
        }
    }

    /// Whether `scheme`, one of the service's auth scheme ids, is bearer auth.
    pub fn is_bearer(&self, scheme: &ShapeId) -> bool {
        matches!(self.auth_schemes.get(scheme), Some(AuthScheme::Bearer))
    }

    /// Effective sensitivity: a member or its target can forbid logging.
    pub fn sensitive(&self, member: &Member) -> bool {
        member.sensitive || self.shape(&member.target).is_some_and(Shape::sensitive)
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
            Some(self.wire_example(member, value, None))
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
    Unit {
        internal: bool,
    },
    Simple(Simple),
    Enum(Enum),
    IntEnum(IntEnum),
    List(List),
    Map(Map),
    Structure(Structure),
    Union(Structure),
}

impl Shape {
    pub fn internal(&self) -> bool {
        match self {
            Self::Unit { internal } => *internal,
            Self::Simple(s) => s.internal,
            Self::Enum(s) => s.internal,
            Self::IntEnum(s) => s.internal,
            Self::List(s) => s.internal,
            Self::Map(s) => s.internal,
            Self::Structure(s) | Self::Union(s) => s.internal,
        }
    }

    /// Documentation-only traits; `Unit` carries none.
    pub fn notes(&self) -> Option<&DocNotes> {
        match self {
            Self::Unit { .. } => None,
            Self::Simple(s) => Some(&s.notes),
            Self::Enum(s) => Some(&s.notes),
            Self::IntEnum(s) => Some(&s.notes),
            Self::List(s) => Some(&s.notes),
            Self::Map(s) => Some(&s.notes),
            Self::Structure(s) | Self::Union(s) => Some(&s.notes),
        }
    }

    pub fn members(&self) -> Vec<&Member> {
        match self {
            Self::Structure(s) | Self::Union(s) => s.members.iter().collect(),
            Self::List(s) => vec![&s.member],
            Self::Map(s) => vec![&s.key, &s.value],
            _ => vec![],
        }
    }

    pub fn sensitive(&self) -> bool {
        match self {
            Self::Unit { .. } => false,
            Self::Simple(s) => s.sensitive,
            Self::Enum(s) => s.sensitive,
            Self::IntEnum(s) => s.sensitive,
            Self::List(s) => s.sensitive,
            Self::Map(s) => s.sensitive,
            Self::Structure(s) | Self::Union(s) => s.sensitive,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Simple {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub streaming: bool,
    pub requires_length: bool,
    pub kind: SimpleKind,
    pub constraints: Constraints,
    pub timestamp_format: Option<TimestampFormat>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
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
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub values: Vec<EnumValue>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// The member name, for a language symbol.
    pub name: String,
    /// What goes on the wire.
    pub value: String,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// The value's own traits, such as project metadata on one enum value.
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct IntEnum {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub values: Vec<IntEnumValue>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct IntEnumValue {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    pub name: String,
    pub value: i32,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// The value's own traits, such as project metadata on one enum value.
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct List {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub sparse: bool,
    pub is_set: bool,
    pub member: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Map {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub sparse: bool,
    pub key: Member,
    pub value: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone)]
pub struct Structure {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub streaming: bool,
    /// Refuses members it does not define (`crucible.protocols#strict`, or a
    /// `strict` structure or input). Server side only.
    pub strict: bool,
    /// An operation's own input structure (Smithy `@input`, or an anonymous
    /// `input` body). Generators use it to type members as the model says.
    pub input: bool,
    pub members: Vec<Member>,
    /// Present on a shape marked `@error`.
    pub error: Option<ErrorInfo>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

impl Structure {
    pub fn sensitive(&self) -> bool {
        self.sensitive
    }

    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// `@sensitive` on the member itself; see [`Service::sensitive`].
    pub sensitive: bool,
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
    /// The input member a client fills with a fresh idempotency token.
    pub idempotency_token: bool,
    /// An explicit infrastructure mutability for the property this member
    /// carries; `None` leaves it to the resource lifecycle.
    pub mutability: Option<Mutability>,
    /// The target's constraints with the member's own laid over them.
    pub constraints: Constraints,
    /// The member's own `@timestampFormat`; see [`Service::timestamp_format`].
    pub timestamp_format: Option<TimestampFormat>,
    /// Effective timestamp format for this member's top-level wire location.
    /// Nested document encoding uses the document format instead.
    pub wire_timestamp_format: TimestampFormat,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
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

/// How provisioning may change a resource property, in CloudFormation's
/// vocabulary, which the CDK and Terraform generators map onto their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    /// Set at create, changed by update, read back.
    Full,
    /// Set at create only, never read back.
    Create,
    /// Set at create only, read back: changing it replaces the resource.
    CreateAndRead,
    /// Read only: the service assigns it.
    Read,
    /// Written at create and update, never read back (a secret, say).
    Write,
}

impl Mutability {
    /// The `aws.cloudformation#cfnMutability` spelling.
    pub fn from_cfn(value: &str) -> Option<Self> {
        Some(match value {
            "full" => Self::Full,
            "create" => Self::Create,
            "create-and-read" => Self::CreateAndRead,
            "read" => Self::Read,
            "write" => Self::Write,
            _ => return None,
        })
    }

    /// Whether changing the property after creation replaces the resource.
    pub fn create_only(self) -> bool {
        matches!(self, Self::Create | Self::CreateAndRead)
    }

    /// Whether reads never return the property.
    pub fn write_only(self) -> bool {
        matches!(self, Self::Create | Self::Write)
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
    pub protocol_headers: BTreeMap<String, String>,
    /// Selected protocol's body discriminator; REST JSON uses its header instead.
    pub body_discriminator: Option<String>,
    pub discriminator_header: Option<String>,
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
    /// Fixed request headers required by the selected protocol.
    pub protocol_headers: BTreeMap<String, String>,
    pub response_protocol_headers: BTreeMap<String, String>,
    /// Visibility settled from Smithy @internal during lowering.
    pub internal: bool,
    /// The sole output member is the restXml root rather than its child.
    pub unwrapped_xml_output: bool,
    pub unsigned_payload: bool,
    pub discovered_endpoint: Option<DiscoveredEndpoint>,
    pub checksum: Option<Checksum>,
    pub id: ShapeId,
    /// Effective authentication schemes, after service and operation overrides.
    pub auth: Vec<ShapeId>,
    /// OAuth scopes a caller must hold, all of them: the operation's own
    /// `@crucible.auth#scopes`, else the service default. Empty when public.
    pub scopes: Vec<String>,
    pub optional_auth: bool,
    pub endpoint: Option<EndpointHostPrefix>,
    pub endpoint_parameters: BTreeMap<String, Vec<EndpointParameterSource>>,
    pub waiters: BTreeMap<String, Waiter>,
    /// The resource that binds the operation; `None` when the service does.
    pub resource: Option<ShapeId>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
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
    /// `@examples`, validated against the operation's shapes, in model order.
    pub examples: Vec<Example>,
    pub request: Bindings,
    pub response: Bindings,
    pub traits: Traits,
}

/// One `@examples` entry. `input`, `output` and `error.content` are model
/// values: members are named as in the model and timestamps are as written.
/// [`Service::wire_example`] renders a member's value as a document carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    pub title: String,
    pub docs: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<ExampleError>,
    /// Constraint violations (length, range, pattern, uniqueness, required
    /// members) were permitted in this example.
    pub allow_constraint_errors: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExampleError {
    pub shape: ShapeId,
    pub content: Value,
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
