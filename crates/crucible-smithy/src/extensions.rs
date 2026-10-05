//! Typed client behavior descriptions; emitters never interpret Smithy trait documents.
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthScheme {
    Bearer,
    Basic,
    ApiKey {
        name: String,
        location: ApiKeyLocation,
        scheme: Option<String>,
    },
    SigV4 {
        signing_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiKeyLocation {
    Header,
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EndpointHostPrefix {
    pub parts: Vec<HostPrefixPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HostPrefixPart {
    Literal { value: String },
    Label { member: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiter {
    pub min_delay: u64,
    pub max_delay: u64,
    pub acceptors: Vec<WaiterAcceptor>,
    pub documentation: Option<String>,
    pub deprecated: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiterAcceptor {
    pub state: WaiterState,
    pub matcher: WaiterMatcher,
    pub message: Option<WaiterPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WaiterState {
    Success,
    Failure,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WaiterMatcher {
    Success {
        expected: bool,
    },
    ErrorType {
        name: String,
    },
    Output {
        path: WaiterPath,
        comparator: WaiterComparator,
        expected: String,
    },
    InputOutput {
        path: WaiterPath,
        comparator: WaiterComparator,
        expected: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WaiterComparator {
    StringEquals,
    BooleanEquals,
    AllStringEquals,
    AnyStringEquals,
}

/// A portable compiled JMESPath expression over model member names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiterPath {
    pub expression: crate::jmespath_ir::JmesExpression,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PathStep {
    Member { name: String },
    Index { index: i64 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EndpointRuleSet {
    pub version: String,
    pub parameters: BTreeMap<String, EndpointParameter>,
    pub rules: Vec<EndpointRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointParameter {
    pub kind: EndpointParameterKind,
    pub built_in: Option<String>,
    pub default: Option<Value>,
    pub required: bool,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EndpointParameterKind {
    String,
    Boolean,
    StringArray,
}

/// Sources in descending Smithy precedence. Select the first present value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EndpointParameterSource {
    Static { value: Value },
    InputMember { member: String },
    InputPath { path: WaiterPath },
    Client { name: String },
    BuiltIn { name: String },
    Default { value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EndpointRule {
    pub conditions: Vec<EndpointCondition>,
    pub result: EndpointResult,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EndpointCondition {
    pub function: EndpointFunction,
    pub arguments: Vec<EndpointExpression>,
    pub assign: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EndpointResult {
    Endpoint {
        url: EndpointExpression,
        headers: BTreeMap<String, Vec<EndpointExpression>>,
        properties: BTreeMap<String, EndpointExpression>,
    },
    Error {
        message: EndpointExpression,
    },
    Tree {
        rules: Vec<EndpointRule>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EndpointExpression {
    String {
        parts: Vec<EndpointTemplatePart>,
    },
    Boolean {
        value: bool,
    },
    Integer {
        value: i64,
    },
    Array {
        values: Vec<EndpointExpression>,
    },
    Object {
        values: BTreeMap<String, EndpointExpression>,
    },
    Reference {
        name: String,
    },
    Call {
        function: EndpointFunction,
        arguments: Vec<EndpointExpression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EndpointTemplatePart {
    Literal { value: String },
    Reference { name: String, path: Vec<PathStep> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EndpointFunction {
    IsSet,
    Not,
    BooleanEquals,
    StringEquals,
    GetAttr,
    Substring,
    IsValidHostLabel,
    ParseUrl,
    UriEncode,
}

/// The representation of an explicitly bound whole HTTP body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PayloadKind {
    Blob,
    String,
    Document,
    EventStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HeaderEncoding {
    Base64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Checksum {
    /// smithy.api#httpChecksumRequired: Content-MD5 over the complete body.
    Md5,
    AwsFlexible {
        request_required: bool,
        request_algorithm_member: Option<String>,
        request_algorithms: Vec<ChecksumAlgorithm>,
        validation_mode_member: Option<String>,
        response_algorithms: Vec<ChecksumAlgorithm>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChecksumAlgorithm {
    Crc32,
    Crc32c,
    Crc64nvme,
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Xxhash64,
    Xxhash3,
    Xxhash128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventBinding {
    Header,
    Payload,
}

/// The member of an operation input or output that carries an event stream
/// (`@streaming` union), and where the structure's other members travel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventStreamBinding {
    pub member: String,
    /// The `@streaming` union.
    pub target: crate::ShapeId,
    /// Protocols without HTTP bindings send the structure's other members as
    /// the first message of the stream: an `initial-request` or
    /// `initial-response` event whose payload is those members' document.
    /// With HTTP bindings they travel in labels, query parameters and headers.
    pub initial_message: bool,
}
