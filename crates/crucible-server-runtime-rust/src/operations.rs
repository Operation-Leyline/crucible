//! The operations a service implements, behind one type-erased table the
//! orchestrator calls. Generated code fills it: `operations(service)` for one
//! type implementing every operation, or `Operations::builder` for several.

use std::collections::BTreeMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::OperationMetadata;
use crate::error::Result;
use crate::generated_operations_builder::OperationsBuilder;
use crate::operation::{Input, Operation, OperationInfo, Output, ServiceInfo};
use crate::protocol::BoundValue;
use crate::validate::{Rejection, Validator};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub(crate) type Invoke =
    Arc<dyn Fn(Box<dyn Input>) -> BoxFuture<Result<Box<dyn Output>>> + Send + Sync>;
type Decode = fn(Value) -> std::result::Result<Box<dyn Input>, Rejection>;
type DecodeJson = fn(&[u8]) -> std::result::Result<Box<dyn Input>, Rejection>;
pub(crate) type DecodeBound = for<'a> fn(
    &'a [u8],
    Vec<(&'static str, BoundValue<'a>)>,
    Option<crucible_wire::stream::ByteStream>,
) -> std::result::Result<Box<dyn Input>, Rejection>;

pub(crate) type DecodeEvents = for<'a> fn(
    &'a [u8],
    Vec<(&'static str, BoundValue<'a>)>,
    crucible_wire::event_stream::Incoming,
) -> std::result::Result<Box<dyn Input>, Rejection>;

pub(crate) struct Entry {
    pub info: &'static OperationInfo,
    pub metadata: OperationMetadata,
    pub decode: Decode,
    pub decode_json: DecodeJson,
    pub decode_cbor: DecodeJson,
    pub decode_bound: DecodeBound,
    pub decode_events: DecodeEvents,
    pub invoke: Invoke,
    /// This operation's body limit when the builder chose one; `None` keeps
    /// the service-wide limit. Settled at build, not looked up per request.
    pub body_limit: Option<usize>,
    /// At an older API version: the decoded input read as the current
    /// version's, and the outcome read back as this version's.
    pub upgrade: Option<crate::versioning::Upgrade>,
    pub downgrade: Option<crate::versioning::Downgrade>,
    pub downgrade_error: Option<crate::versioning::DowngradeError>,
}

/// An older API version's operations: its routes and decoders, each served
/// by the current implementation through its conversions.
pub(crate) struct OlderVersion {
    pub version: crate::versioning::ApiVersion,
    pub sunset: Option<&'static str>,
    pub operations: Operations,
}

/// The API versions a service serves, when its model has migrations.
#[derive(Default)]
pub(crate) struct Versions {
    pub current: Option<crate::versioning::ApiVersion>,
    /// Oldest first.
    pub older: Vec<OlderVersion>,
}

impl Versions {
    /// Every version served, oldest first.
    pub fn supported(&self) -> Vec<crate::versioning::ApiVersion> {
        self.older
            .iter()
            .map(|older| older.version)
            .chain(self.current)
            .collect()
    }
}

pub struct Operations {
    pub(crate) service: &'static ServiceInfo,
    pub(crate) entries: BTreeMap<&'static str, Entry>,
    /// Operations by path length (the index), built once, so routing a
    /// request compares only the paths that could match.
    pub(crate) routes: Vec<Vec<&'static OperationInfo>>,
    pub(crate) greedy_routes: Vec<&'static OperationInfo>,
    pub(crate) versions: Versions,
}

impl Operations {
    #[doc(hidden)]
    pub fn builder(service: &'static ServiceInfo) -> OperationsBuilder {
        OperationsBuilder {
            operations: Operations {
                service,
                entries: BTreeMap::new(),
                routes: Vec::new(),
                greedy_routes: Vec::new(),
                versions: Versions::default(),
            },
            older: Vec::new(),
            versioned: Vec::new(),
            hooks: Default::default(),
            legacy: Default::default(),
        }
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    /// The effective contract of a registered operation, available during preparation.
    pub fn metadata(&self, operation: &str) -> Option<&OperationMetadata> {
        self.entries.get(operation).map(|entry| &entry.metadata)
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    pub fn greedy_routes(&self) -> &[&'static OperationInfo] {
        &self.greedy_routes
    }

    /// The operations whose path has `segments` segments.
    pub fn routes(&self, segments: usize) -> &[&'static OperationInfo] {
        self.routes.get(segments).map_or(&[], Vec::as_slice)
    }
}

impl fmt::Debug for Operations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operations")
            .field("service", &self.service.id)
            .field("operations", &self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// An input that does not read as its type. A member a strict structure
/// does not define is named in the path, and so is a required member that
/// is missing (a payload sent as an empty body, say).
pub(crate) fn unreadable(error: serde_json::Error) -> Rejection {
    let message = error.to_string();
    let named = |prefix: &str| {
        message
            .strip_prefix(prefix)
            .and_then(|rest| rest.split('`').next())
            .map(str::to_owned)
    };
    if let Some(member) = named("unknown field `") {
        return Rejection::malformed(member, "is not a member of this structure");
    }
    if let Some(member) = named("missing field `") {
        return Rejection::constraint(member, "is required");
    }
    Rejection::malformed("", message)
}

pub(crate) fn decode<O: Operation>(value: Value) -> std::result::Result<Box<dyn Input>, Rejection> {
    let input: O::Input = serde_json::from_value(value).map_err(unreadable)?;
    Validator::check(&input)?;
    Ok(Box::new(input))
}

/// The input straight from JSON bytes, in one pass: for an input with no
/// member bound outside the body. An empty or JSON-whitespace-only body is `{}`.
pub(crate) fn decode_json<O: Operation>(
    bytes: &[u8],
) -> std::result::Result<Box<dyn Input>, Rejection> {
    let bytes = if crucible_wire::text::empty_json_body(bytes) {
        b"{}".as_slice()
    } else {
        bytes
    };
    let input: O::Input = serde_json::from_slice(bytes).map_err(unreadable)?;
    Validator::check(&input)?;
    Ok(Box::new(input))
}

pub(crate) fn decode_cbor<O: Operation>(
    bytes: &[u8],
) -> std::result::Result<Box<dyn Input>, Rejection> {
    if !O::HAS_INPUT && !bytes.is_empty() {
        return Err(Rejection::malformed(
            "",
            "an operation without input requires an empty body",
        ));
    }
    let bytes = if bytes.is_empty() {
        b"\xA0".as_slice()
    } else {
        bytes
    };
    let input: O::Input =
        crucible_wire::cbor::read(bytes).map_err(|message| Rejection::malformed("", message))?;
    Validator::check(&input)?;
    Ok(Box::new(input))
}

/// The input from its bound members and its JSON body, in one pass.
pub(crate) fn decode_bound<'a, O: Operation>(
    body: &'a [u8],
    bound: Vec<(&'static str, BoundValue<'a>)>,
    stream: Option<crucible_wire::stream::ByteStream>,
) -> std::result::Result<Box<dyn Input>, Rejection> {
    let mut input: O::Input =
        crucible_wire::bound::read_http(body, bound, O::INFO.input).map_err(unreadable)?;
    if let Some(stream) = stream {
        O::set_input_stream(&mut input, stream);
    }
    Validator::check(&input)?;
    Ok(Box::new(input))
}

pub(crate) fn decode_events<O: Operation>(
    body: &[u8],
    bound: Vec<(&'static str, BoundValue<'_>)>,
    events: crucible_wire::event_stream::Incoming,
) -> std::result::Result<Box<dyn Input>, Rejection> {
    let mut input: O::Input = if O::PROTOCOL == crucible_wire::operation::Protocol::SmithyRpcV2Cbor
    {
        let body = if body.is_empty() {
            b"\xA0".as_slice()
        } else {
            body
        };
        crucible_wire::cbor::read(body).map_err(|e| Rejection::malformed("", e))?
    } else {
        crucible_wire::bound::read_http(body, bound, O::INFO.input).map_err(unreadable)?
    };
    O::set_input_events(&mut input, events);
    Validator::check(&input)?;
    Ok(Box::new(input))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingOperations(pub Vec<&'static str>);

impl fmt::Display for MissingOperations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no implementation for: {}", self.0.join(", "))
    }
}

impl std::error::Error for MissingOperations {}

pub(crate) struct StreamingOutput<O: Operation>(pub(crate) O::Output);
impl<O: Operation> Output for StreamingOutput<O> {
    fn write_cbor(&self, out: &mut Vec<u8>) -> Result<(), String> {
        crucible_wire::cbor::write(&self.0, out)
    }
    fn response_status(&self) -> Option<u16> {
        O::output_status(&self.0)
    }
    fn to_json(&self) -> Value {
        serde_json::to_value(&self.0).unwrap_or(Value::Null)
    }
    fn write_json(&self, out: &mut Vec<u8>) -> serde_json::Result<()> {
        serde_json::to_writer(out, &self.0)
    }
    fn write_json_split(
        &self,
        members: &[crate::operation::MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<Vec<(&'static str, String)>> {
        crucible_wire::split::write(&self.0, members, out)
    }
    fn write_http(
        &self,
        members: &[crate::operation::MemberInfo],
        out: &mut Vec<u8>,
    ) -> serde_json::Result<crate::operation::ResponseBindings> {
        crucible_wire::split::write_http(&self.0, members, None, out)
    }
    fn events(&self) -> Option<std::io::Result<crucible_wire::event_stream::Outgoing>> {
        O::output_events(&self.0)
    }
    fn stream(&self) -> Option<crucible_wire::stream::ByteStream> {
        O::output_stream(&self.0)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        &self.0
    }
}
