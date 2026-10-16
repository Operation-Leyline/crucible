//! What generated code tells the framework about a service and its
//! operations. All of it is `'static` data built at compile time, so routing,
//! binding and logging read it without allocation.

use std::any::Any;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::ModelledError;
use crate::validate::{Rejection, Validate};

/// One operation, as the generated marker type `GetGraph` describes it.
pub trait Operation: Send + Sync + 'static {
    const INFO: &'static OperationInfo;
    /// Whether the operation's effective authentication schemes include bearer auth.
    /// Regenerate clients with this runtime; older generated markers omit this flag.
    /// Handwritten operations must opt in explicitly when bearer auth is supported.
    const AUTH_BEARER: bool = false;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = None;
    /// The release a generated client names in its `Crucible-Version`
    /// header: the `.cru` service's `version` (docs/api-versions-clients.md).
    const API_VERSION: Option<&'static str> = None;
    /// How the operation pages (`@paginated`), for server extensions that
    /// read the token from the decoded input.
    const PAGINATION: Option<Pagination> = None;
    /// The input member that carries the operation's idempotency token
    /// (`@idempotencyToken`), for server extensions that read it from the
    /// decoded input. Generated for server bindings.
    const IDEMPOTENCY_TOKEN: Option<PaginationMember> = None;
    /// Reads a successful output back from its JSON form
    /// ([`Output::to_json`]), so a server extension can answer a retry with
    /// the output an earlier attempt produced, encoded afresh for the
    /// retry. Generated for server operations that have an idempotency token;
    /// `None` means the operation's output cannot be replayed.
    const OUTPUT_FROM_JSON: Option<OutputFromJson<Self::Output>> = None;
    /// What the operation requires of its caller (`requires`, `filters`,
    /// `requires none`, `requires pending`), for the server's `Permissions`
    /// interceptor. Generated for server bindings; `Undeclared` otherwise.
    const REQUIREMENTS: crate::permissions::Requirements =
        crate::permissions::Requirements::Undeclared;
    /// Evaluates the bindings and `when` conditions of
    /// [`Operation::REQUIREMENTS`] against the decoded input: one
    /// [`Check`](crate::permissions::Check) per `Check` or `Filter` clause
    /// that applies, in clause order. Generated, so it reads fields rather
    /// than JSON; `None` for an operation with no such clause.
    const CHECKS: Option<crate::permissions::Checks<Self::Input>> = None;
    /// Whether the model explicitly makes the operation public (no
    /// authentication: `@auth([])` on the operation, a `.cru` public
    /// operation), so it carries no requirements and there is nothing to
    /// authorize. Never inferred from an empty list of schemes, which also
    /// describes a service authenticated outside the model.
    const PUBLIC: bool = false;
    const HAS_INPUT: bool = true;
    const HAS_OUTPUT: bool = true;
    type Input: DeserializeOwned + Serialize + Validate + Send + Sync + 'static;
    type Output: Serialize + Send + Sync + 'static;
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ServiceInfo {
    /// External MCP tools, present only when service generation enabled MCP.
    pub mcp_tools: Option<&'static [crate::mcp::ToolDescriptor]>,
    /// The service's shape id, `namespace#Name`.
    pub id: &'static str,
    pub version: &'static str,
    pub operations: &'static [&'static OperationInfo],
    pub errors: &'static [&'static ErrorInfo],
    /// Finds the modelled error inside a boxed error, by downcasting to each
    /// of the service's error types.
    pub classify: Classify,
    /// The validation error a rejected request becomes, when the generator
    /// could fill it; otherwise the service supplies one to the builder.
    pub reject: Option<RejectFn>,
    /// The error an internal failure is answered with, carrying only a fixed
    /// message; `None` answers the protocol's own `InternalFailure`.
    pub internal: Option<InternalFn>,
    /// The error a request no operation serves is answered with: the
    /// validation error with reason `unknownOperation`, when the model has
    /// that standard reason; `None` answers the protocol's own
    /// `UnknownOperation`.
    pub unknown: Option<InternalFn>,
    /// Every permission the model declares, primaries only
    /// ([`ServiceInfo::with_permissions`]): what a stored authorization
    /// decision is read back against, and where a modelled
    /// `NotFoundException` finds its resource.
    pub permissions: &'static [&'static crate::permissions::PermissionInfo],
    /// The shape id of the error the service answers a hidden or missing
    /// resource with, when the model designates its own (`notFound` in the
    /// `.cru` service, [`ServiceInfo::with_not_found`]); `None` is the
    /// framework's `crucible.errors#NotFoundException`. The server never
    /// builds it: the service's `not_found_factory` does, and every
    /// response that carries it is sent with `Cache-Control: no-store`.
    pub not_found: Option<&'static str>,
}

pub type Classify =
    for<'a> fn(&'a (dyn std::error::Error + 'static)) -> Option<&'a dyn ModelledError>;
pub type RejectFn = fn(&Rejection) -> Box<dyn ModelledError>;
pub type InternalFn = fn() -> Box<dyn ModelledError>;

impl ServiceInfo {
    pub fn operation(&self, name: &str) -> Option<&'static OperationInfo> {
        self.operations.iter().copied().find(|op| op.name == name)
    }

    /// The same service with its permission registry: generated as
    /// `service_info(..).with_permissions(&[..])`.
    pub const fn with_permissions(
        mut self,
        permissions: &'static [&'static crate::permissions::PermissionInfo],
    ) -> Self {
        self.permissions = permissions;
        self
    }

    /// The same service with its own not-found error, by shape id:
    /// generated as `.with_not_found("pigeon.api#ResourceNotFoundException")`
    /// for a model that designates one.
    pub const fn with_not_found(mut self, shape_id: &'static str) -> Self {
        self.not_found = Some(shape_id);
        self
    }

    /// The permission `value` (or an alias of it) on the resource named
    /// `resource`, from the registry.
    pub fn permission(
        &self,
        resource: &str,
        value: &str,
    ) -> Option<&'static crate::permissions::PermissionInfo> {
        self.permissions
            .iter()
            .copied()
            .find(|permission| permission.resource.name == resource && permission.accepts(value))
    }

    /// The resource named `name`, from the registry.
    pub fn resource(&self, name: &str) -> Option<&'static crate::permissions::ResourceInfo> {
        self.permissions
            .iter()
            .map(|permission| permission.resource)
            .find(|resource| resource.name == name)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct OperationInfo {
    /// `internal` in the model; caller authorization remains application policy.
    pub internal: bool,
    /// The operation's shape id, `namespace#Name`.
    pub id: &'static str,
    pub name: &'static str,
    /// The resource that binds the operation, if one does.
    pub resource: Option<&'static str>,
    pub method: &'static str,
    pub path: &'static [Segment],
    /// Literal query parameters required by the modeled URI.
    pub literal_query: &'static [(&'static str, Option<&'static str>)],
    /// The status of a successful response.
    pub status: u16,
    pub input: &'static [MemberInfo],
    pub output: &'static [MemberInfo],
    /// The errors the operation can return, by shape name.
    pub errors: &'static [&'static str],
    pub readonly: bool,
    pub idempotent: bool,
    pub idempotency_token: Option<&'static str>,
    /// The model's own traits on the operation: shape id and JSON value.
    pub traits: &'static [(&'static str, &'static str)],
    /// Who calls a sender-signed operation (`@sender`, decision 81), a
    /// third party that signs each request; `None` for an operation the
    /// service's clients call. Generated as `.with_sender("Stripe")`.
    pub sender: Option<&'static str>,
}

impl OperationInfo {
    /// The same operation, called and signed by `sender` (decision 81).
    pub const fn with_sender(mut self, sender: &'static str) -> Self {
        self.sender = Some(sender);
        self
    }

    /// A trait's JSON value, as written in the model. It is parsed on each
    /// call: a handler reads its traits once, in `Handler::prepare`.
    pub fn trait_value(&self, id: &str) -> Option<Value> {
        trait_value(self.traits, id)
    }

    /// A trait's value as `T`: `Ok(None)` when the operation does not have
    /// it, an error when its value is not a `T`.
    pub fn trait_as<T: DeserializeOwned>(&self, id: &str) -> serde_json::Result<Option<T>> {
        self.traits
            .iter()
            .find(|(trait_id, _)| *trait_id == id)
            .map(|(_, json)| serde_json::from_str(json))
            .transpose()
    }

    pub fn has_trait(&self, id: &str) -> bool {
        self.traits.iter().any(|(trait_id, _)| *trait_id == id)
    }

    pub fn declares(&self, error: &str) -> bool {
        self.errors.contains(&error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Segment {
    GreedyLabel(&'static str),
    Literal(&'static str),
    Label(&'static str),
}

/// A member of an input, output or error structure, and where it travels.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MemberInfo {
    pub name: &'static str,
    pub location: Location,
    pub scalar: Scalar,
    /// Static, recursive logging policy resolved from the model.
    pub redaction: &'static Redaction,
    /// The model's own traits on the member: shape id and JSON value.
    pub traits: &'static [(&'static str, &'static str)],
}

/// Logging metadata, including recursive shapes. References are static so
/// recursive models need no per-request schema construction.
pub enum Redaction {
    Public,
    Sensitive,
    Structure(&'static [(&'static str, &'static Redaction)]),
    List(&'static Redaction),
    Map(&'static Redaction),
}

impl std::fmt::Debug for Redaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Following static references here would recurse forever for a
        // recursive model, even though redacting a finite value terminates.
        f.write_str(match self {
            Self::Public => "Public",
            Self::Sensitive => "Sensitive",
            Self::Structure(_) => "Structure(..)",
            Self::List(_) => "List(..)",
            Self::Map(_) => "Map(..)",
        })
    }
}

impl Redaction {
    /// Removes sensitive content in place. False means the whole value is
    /// sensitive and its parent must omit it.
    pub fn redact(&self, value: &mut Value) -> bool {
        match self {
            Self::Sensitive => return false,
            Self::Public => {}
            Self::Structure(members) => {
                if let Value::Object(object) = value {
                    object.retain(|name, value| {
                        members
                            .iter()
                            .find(|(member, _)| name == member)
                            .is_some_and(|(_, policy)| policy.redact(value))
                    });
                }
            }
            Self::List(policy) => {
                if let Value::Array(items) = value {
                    for item in items {
                        if !policy.redact(item) {
                            *item = Value::Null;
                        }
                    }
                }
            }
            Self::Map(policy) => {
                if let Value::Object(object) = value {
                    object.retain(|_, value| policy.redact(value));
                }
            }
        }
        true
    }
}

impl MemberInfo {
    /// A trait's JSON value, as written in the model.
    pub fn trait_value(&self, id: &str) -> Option<Value> {
        trait_value(self.traits, id)
    }

    pub fn has_trait(&self, id: &str) -> bool {
        self.traits.iter().any(|(trait_id, _)| *trait_id == id)
    }
}

fn trait_value(traits: &[(&str, &str)], id: &str) -> Option<Value> {
    traits
        .iter()
        .find(|(trait_id, _)| *trait_id == id)
        .and_then(|(_, json)| serde_json::from_str(json).ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Location {
    Label,
    Query(&'static str),
    Header(&'static str),
    Body,
    /// The whole HTTP body, raw (`@httpPayload` on a blob), with the media
    /// type a sender gives it. A structure with a payload member has no
    /// other body members.
    Payload(&'static str),
    /// The whole HTTP body as UTF-8 text (`@httpPayload` on a string or enum).
    TextPayload(&'static str),
    /// The whole HTTP body as one JSON value (`@httpPayload` on a structure,
    /// union or document). Model serialization decides omission; an explicitly
    /// serialized null is the JSON value `null`, which reads back as the
    /// member's null, as it would in a JSON body.
    DocumentPayload(&'static str),
}

impl Location {
    /// The payload member of `members`, if it has one, and its media type.
    pub fn payload(members: &[MemberInfo]) -> Option<(&MemberInfo, &'static str)> {
        members.iter().find_map(|member| match member.location {
            Self::Payload(media_type)
            | Self::TextPayload(media_type)
            | Self::DocumentPayload(media_type) => Some((member, media_type)),
            _ => None,
        })
    }
}

/// How a bound member's text reads as JSON. Body members are `Json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scalar {
    BigInteger,
    BigDecimal,
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    EpochSeconds,
    HttpDate,
    /// A timestamp whose bound text uses a different format than its JSON
    /// value: a REST protocol's HTTP-date header for an epoch-seconds member.
    Timestamp {
        text: TimestampFormat,
        json: TimestampFormat,
    },
    /// A `@mediaType` string bound to a header: base64 of its UTF-8 text.
    Base64,
    List(&'static Scalar),
    Json,
}

impl Scalar {
    /// HTTP-date text, whose own commas a header list does not quote.
    pub fn http_date(self) -> bool {
        matches!(
            self,
            Self::HttpDate
                | Self::Timestamp {
                    text: TimestampFormat::HttpDate,
                    ..
                }
        )
    }
}

/// A timestamp's wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampFormat {
    DateTime,
    EpochSeconds,
    HttpDate,
}

/// The input members of a `@paginated` operation that say where a listing
/// resumes and how much of it to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    pub input_token: PaginationMember,
    pub page_size: Option<PaginationMember>,
    /// Whether the input token is a map rather than a string (the IR allows
    /// either): a server extension that seals string tokens refuses such an
    /// operation instead of reading its token as absent.
    pub input_token_map: bool,
    /// The largest length the model allows the input token member (`@length`
    /// max, in characters), if it bounds one: a server extension that issues
    /// tokens checks at build that the longest it can issue fits.
    pub input_token_max_length: Option<u64>,
}

/// A top-level input member, named as the model names it (a validation
/// path) and as its key in the input's JSON form ([`Input::to_json`]): a
/// pagination member, or an operation's idempotency token
/// ([`Operation::IDEMPOTENCY_TOKEN`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaginationMember {
    pub name: &'static str,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ErrorInfo {
    pub name: &'static str,
    pub status: u16,
    pub fault: Fault,
    /// The model explicitly marks this error as retryable.
    pub retryable: bool,
    pub throttling: bool,
    pub members: &'static [MemberInfo],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fault {
    /// The caller's mistake: a 4xx.
    Client,
    /// The service's failure: a 5xx.
    Server,
}

/// No value: the input of an operation that takes none, or a union member
/// that carries nothing. `{}` on the wire.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Unit {}

impl Validate for Unit {
    fn validate(&self, _: &mut crate::validate::Validator) {}
}

/// A validated input, seen by handlers without knowing its type.
pub trait Input: Any + Send + Sync {
    fn to_json(&self) -> Value;
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

impl<T: Serialize + Any + Send + Sync> Input for T {
    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// Reads an output of type `O` from its JSON form: [`Operation::OUTPUT_FROM_JSON`].
pub type OutputFromJson<O> = fn(Value) -> serde_json::Result<O>;

/// An operation's output, seen by handlers without knowing its type.
pub trait Output: Any + Send + Sync {
    fn to_json(&self) -> Value;
    /// Serializes into `out`. Every serializable output writes straight to
    /// bytes; this default, for other implementors, goes through `to_json`.
    fn write_json(&self, out: &mut Vec<u8>) -> serde_json::Result<()> {
        serde_json::to_writer(out, &self.to_json())
    }
    /// Serializes into `out` without the members `members` binds to
    /// headers, returning those as (header, text); with a payload member,
    /// `out` gets that member's bytes, raw. The default goes through
    /// `to_json`, as `write_json`'s does.
    fn write_json_split(
        &self,
        members: &[MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<Vec<(&'static str, String)>> {
        crate::split::write(&self.to_json(), members, out)
    }
    fn as_any(&self) -> &dyn Any;
}

impl<T: Serialize + Any + Send + Sync> Output for T {
    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    fn write_json(&self, out: &mut Vec<u8>) -> serde_json::Result<()> {
        serde_json::to_writer(out, self)
    }

    fn write_json_split(
        &self,
        members: &[MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<Vec<(&'static str, String)>> {
        crate::split::write(self, members, out)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Effective authentication options in modeled priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthScheme {
    Bearer,
}
