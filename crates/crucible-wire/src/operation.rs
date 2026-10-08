//! What generated code tells the framework about a service and its
//! operations. All of it is `'static` data built at compile time, so routing,
//! binding and logging read it without allocation.

pub use crate::cors_info::CorsInfo;
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
    const OPTIONAL_AUTH: bool = false;
    const UNSIGNED_PAYLOAD: bool = false;
    /// The modeled wire protocol. Server defaults select the corresponding implementation.
    const PROTOCOL: Protocol = Protocol::CrucibleJson;
    /// The operation's modeled HTTP checksum behavior, for clients.
    const CHECKSUM: Option<Checksum> = None;
    const HAS_INPUT: bool = true;
    const HAS_OUTPUT: bool = true;
    fn output_status(_output: &Self::Output) -> Option<u16> {
        None
    }
    fn input_stream(_input: &Self::Input) -> Option<crate::stream::ByteStream> {
        None
    }
    fn output_stream(_output: &Self::Output) -> Option<crate::stream::ByteStream> {
        None
    }
    fn set_input_stream(_input: &mut Self::Input, _stream: crate::stream::ByteStream) {}
    fn set_output_stream(_output: &mut Self::Output, _stream: crate::stream::ByteStream) {}
    /// The event stream the input sends, when it has one.
    const INPUT_EVENTS: Option<&'static crate::event_stream::EventStreamInfo> = None;
    /// The event stream the output receives, when it has one.
    const OUTPUT_EVENTS: Option<&'static crate::event_stream::EventStreamInfo> = None;
    fn input_events(
        _input: &Self::Input,
    ) -> Option<std::io::Result<crate::event_stream::Outgoing>> {
        None
    }
    fn set_output_events(_output: &mut Self::Output, _events: crate::event_stream::Incoming) {}
    fn set_input_events(_input: &mut Self::Input, _events: crate::event_stream::Incoming) {}
    fn output_events(
        _output: &Self::Output,
    ) -> Option<std::io::Result<crate::event_stream::Outgoing>> {
        None
    }
    type Input: DeserializeOwned + Serialize + Validate + Send + Sync + 'static;
    type Output: Serialize + Send + Sync + 'static;
}

#[derive(Debug)]
pub struct ServiceInfo {
    pub cors: Option<&'static CorsInfo>,
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
}

pub type Classify =
    for<'a> fn(&'a (dyn std::error::Error + 'static)) -> Option<&'a dyn ModelledError>;
pub type RejectFn = fn(&Rejection) -> Box<dyn ModelledError>;
pub type InternalFn = fn() -> Box<dyn ModelledError>;

impl ServiceInfo {
    pub fn operation(&self, name: &str) -> Option<&'static OperationInfo> {
        self.operations.iter().copied().find(|op| op.name == name)
    }
}

#[derive(Debug)]
pub struct OperationInfo {
    /// Smithy @internal; caller authorization remains application policy.
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
}

impl OperationInfo {
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
pub enum Segment {
    GreedyLabel(&'static str),
    Literal(&'static str),
    Label(&'static str),
}

/// A member of an input, output or error structure, and where it travels.
#[derive(Debug, Clone, Copy)]
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
pub enum Location {
    Label,
    Query(&'static str),
    QueryParams,
    PrefixHeaders(&'static str),
    ResponseCode,
    Header(&'static str),
    Body,
    /// The whole HTTP body, raw (`@httpPayload` on a blob), with the media
    /// type a sender gives it. A structure with a payload member has no
    /// other body members.
    Payload(&'static str),
    /// The whole HTTP body as UTF-8 text (`@httpPayload` on a string or enum).
    TextPayload(&'static str),
    /// The whole HTTP body as one JSON value (`@httpPayload` on a structure,
    /// union or document). An absent or null value sends no body.
    DocumentPayload(&'static str),
    StreamingPayload {
        media_type: &'static str,
        requires_length: bool,
        minimum: Option<u64>,
        maximum: Option<u64>,
    },
    /// An event stream (`@streaming` union): the whole body, as
    /// `application/vnd.amazon.eventstream` messages. It is never part of a
    /// JSON document; the operation's `INPUT_EVENTS` / `OUTPUT_EVENTS`
    /// describe its events.
    EventStream,
}

impl Location {
    /// The payload member of `members`, if it has one, and its media type.
    pub fn payload(members: &[MemberInfo]) -> Option<(&MemberInfo, &'static str)> {
        members.iter().find_map(|member| match member.location {
            Self::Payload(media_type)
            | Self::TextPayload(media_type)
            | Self::DocumentPayload(media_type)
            | Self::StreamingPayload { media_type, .. } => Some((member, media_type)),
            _ => None,
        })
    }
}

/// How a bound member's text reads as JSON. Body members are `Json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The protocol a generated client or server speaks, chosen once from the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `crucible.protocols#json`: HTTP bindings, `__type` or
    /// `x-crucible-error` naming errors.
    CrucibleJson,
    /// `aws.protocols#restJson1`: HTTP bindings, `X-Amzn-Errortype`.
    AwsRestJson1,
    /// `aws.protocols#awsJson1_0`: every member in a JSON body POSTed to `/`
    /// with `X-Amz-Target: {service}.{operation}`.
    AwsJson10 {
        service: &'static str,
    },
    /// `aws.protocols#awsJson1_1`, as `AwsJson10` with its own content type.
    AwsJson11 {
        service: &'static str,
    },
    SmithyRpcV2Json,
    SmithyRpcV2Cbor,
}

impl Protocol {
    /// Whether errors and messages follow the AWS JSON conventions.
    pub fn aws(self) -> bool {
        matches!(
            self,
            Self::AwsRestJson1 | Self::AwsJson10 { .. } | Self::AwsJson11 { .. }
        )
    }

    pub fn rpc_v2(self) -> bool {
        matches!(self, Self::SmithyRpcV2Json | Self::SmithyRpcV2Cbor)
    }
}

/// An operation's modeled HTTP checksum behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checksum {
    /// `@httpChecksumRequired`: `Content-MD5` over the complete body.
    Md5,
    /// `@aws.protocols#httpChecksum`. Member names are wire names.
    Flexible {
        request_required: bool,
        request_algorithm_member: Option<&'static str>,
        validation_mode_member: Option<&'static str>,
        response_algorithms: &'static [ChecksumAlgorithm],
    },
}

/// A checksum algorithm a model may name. A client computes only those
/// `supported` reports; the others are recognized so a model can name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChecksumAlgorithm {
    Crc32,
    Crc32c,
    Crc64Nvme,
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Xxhash64,
    Xxhash3,
    Xxhash128,
}

impl ChecksumAlgorithm {
    pub const ALL: &'static [Self] = &[
        Self::Crc32,
        Self::Crc32c,
        Self::Crc64Nvme,
        Self::Md5,
        Self::Sha1,
        Self::Sha256,
        Self::Sha512,
        Self::Xxhash64,
        Self::Xxhash3,
        Self::Xxhash128,
    ];

    /// The model's spelling: `CRC32`, `SHA256`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Crc32 => "CRC32",
            Self::Crc32c => "CRC32C",
            Self::Crc64Nvme => "CRC64NVME",
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
            Self::Xxhash64 => "XXHASH64",
            Self::Xxhash3 => "XXHASH3",
            Self::Xxhash128 => "XXHASH128",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|value| value.name() == name)
    }

    /// The `x-amz-checksum-*` header that carries this checksum.
    pub fn header(self) -> &'static str {
        match self {
            Self::Crc32 => "x-amz-checksum-crc32",
            Self::Crc32c => "x-amz-checksum-crc32c",
            Self::Crc64Nvme => "x-amz-checksum-crc64nvme",
            Self::Md5 => "x-amz-checksum-md5",
            Self::Sha1 => "x-amz-checksum-sha1",
            Self::Sha256 => "x-amz-checksum-sha256",
            Self::Sha512 => "x-amz-checksum-sha512",
            Self::Xxhash64 => "x-amz-checksum-xxhash64",
            Self::Xxhash3 => "x-amz-checksum-xxhash3",
            Self::Xxhash128 => "x-amz-checksum-xxhash128",
        }
    }
}

impl std::fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug)]
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

/// HTTP bindings removed from a serialized response body.
#[derive(Debug, Default)]
pub struct ResponseBindings {
    pub headers: Vec<(String, String)>,
    pub status: Option<u16>,
}

/// An operation's output, seen by handlers without knowing its type.
pub trait Output: Any + Send + Sync {
    fn events(&self) -> Option<std::io::Result<crate::event_stream::Outgoing>> {
        None
    }
    fn write_cbor(&self, out: &mut Vec<u8>) -> Result<(), String> {
        crate::cbor::write(&self.to_json(), out)
    }
    fn response_status(&self) -> Option<u16> {
        None
    }
    fn stream(&self) -> Option<crate::stream::ByteStream> {
        None
    }
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
    fn write_http(
        &self,
        members: &[MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<ResponseBindings> {
        crate::split::write_http(&self.to_json(), members, None, out)
    }
    fn as_any(&self) -> &dyn Any;
}

impl<T: Serialize + Any + Send + Sync> Output for T {
    fn write_cbor(&self, out: &mut Vec<u8>) -> Result<(), String> {
        crate::cbor::write(self, out)
    }
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

    fn write_http(
        &self,
        members: &[MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<ResponseBindings> {
        crate::split::write_http(self, members, None, out)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Invalid modeled response codes become the same 500 that serialization emits.
pub fn response_status(value: i64) -> u16 {
    u16::try_from(value)
        .ok()
        .filter(|value| (100..=599).contains(value))
        .unwrap_or(500)
}

/// Effective authentication options in modeled priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    Basic,
    ApiKey {
        name: &'static str,
        query: bool,
        scheme: Option<&'static str>,
    },
    SigV4 {
        signing_name: &'static str,
    },
}
