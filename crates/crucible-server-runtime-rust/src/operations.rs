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
) -> std::result::Result<Box<dyn Input>, Rejection>;

/// Reads an operation's output back from its JSON form, for a replay.
pub(crate) type OutputFromJson = fn(Value) -> Result<Box<dyn Output>>;

pub(crate) struct Entry {
    pub info: &'static OperationInfo,
    pub metadata: OperationMetadata,
    pub decode: Decode,
    pub decode_json: DecodeJson,
    pub decode_bound: DecodeBound,
    pub invoke: Invoke,
    /// Reads the current version's output from JSON, when the operation's
    /// output can be replayed (`Operation::OUTPUT_FROM_JSON`).
    pub output_from_json: Option<OutputFromJson>,
    /// This operation's body limit when the builder chose one; `None` keeps
    /// the service-wide limit. Settled at build, not looked up per request.
    pub body_limit: Option<usize>,
    /// At an older API version: the decoded input read as the current
    /// version's, and the outcome read back as this version's.
    pub upgrade: Option<crate::versioning::Upgrade>,
    pub downgrade: Option<crate::versioning::Downgrade>,
    pub downgrade_error: Option<crate::versioning::DowngradeError>,
    /// The older release whose errors a legacy handler returns, beside the
    /// current model's.
    pub errors: Option<&'static crate::operation::ServiceInfo>,
}

/// An older API version's operations: its routes and decoders, each served
/// by the current implementation through its conversions.
pub(crate) struct OlderVersion {
    pub version: crate::versioning::ApiVersion,
    /// The `Sunset` header its responses carry.
    pub sunset: Option<axum::http::HeaderValue>,
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

    /// The metadata `operation` is served with: the entry whose own
    /// description it is, at whichever version; else, for a description no
    /// entry carries, the current version's entry by name and id, else the
    /// newest older version's. An id is shared across versions, but an
    /// older version's entry keeps its own facts (its authentication
    /// schemes), so a description finds its own entry first.
    pub(crate) fn served_metadata(&self, operation: &OperationInfo) -> Option<&OperationMetadata> {
        let tables = std::iter::once(self).chain(
            self.versions
                .older
                .iter()
                .rev()
                .map(|older| &older.operations),
        );
        let exact = tables
            .filter_map(|operations| operations.entries.get(operation.name))
            .find(|entry| std::ptr::eq(entry.info, operation));
        if let Some(entry) = exact {
            return Some(&entry.metadata);
        }
        fn own<'a>(
            operations: &'a Operations,
            operation: &OperationInfo,
        ) -> Option<&'a OperationMetadata> {
            operations
                .entries
                .get(operation.name)
                .filter(|entry| entry.info.id == operation.id)
                .map(|entry| &entry.metadata)
        }
        own(self, operation).or_else(|| {
            self.versions
                .older
                .iter()
                .rev()
                .find_map(|older| own(&older.operations, operation))
        })
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

/// An input that does not read as its type.
pub(crate) fn unreadable(error: serde_json::Error) -> Rejection {
    unreadable_message(&error.to_string())
}

/// An input that does not read as its type, from the reader's message. A
/// required member that is missing (a payload sent as an empty body, say)
/// is named in the path: the name is the model's. A member a strict
/// structure does not define is the client's text, of any length, so its
/// path is `[key]`, as MCP names an undeclared argument.
pub(crate) fn unreadable_message(message: &str) -> Rejection {
    if message.starts_with("unknown field") {
        return Rejection::malformed("[key]", "is not a member of this structure");
    }
    if let Some(member) = message
        .strip_prefix("missing field `")
        .and_then(|rest| rest.split('`').next())
    {
        return Rejection::constraint(member, "is required");
    }
    Rejection::malformed("", crucible_wire::text::without_values(message))
}

pub(crate) fn decode<O: Operation>(value: Value) -> std::result::Result<Box<dyn Input>, Rejection> {
    let input: O::Input = crucible_wire::object::from_value(value).map_err(unreadable)?;
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
    let input: O::Input = crucible_wire::object::from_slice(bytes).map_err(unreadable)?;
    Validator::check(&input)?;
    Ok(Box::new(input))
}

/// The input from its bound members and its JSON body, in one pass.
pub(crate) fn decode_bound<'a, O: Operation>(
    body: &'a [u8],
    bound: Vec<(&'static str, BoundValue<'a>)>,
) -> std::result::Result<Box<dyn Input>, Rejection> {
    let input: O::Input =
        crucible_wire::bound::read_http(body, bound, O::INFO.input).map_err(unreadable)?;
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

/// `O`'s output read from its JSON form, wrapped as the operation's own
/// output is, so the protocol and an older version's downgrade treat a
/// replayed output as the operation's.
pub(crate) fn output_from_json<O: Operation>(value: Value) -> Result<Box<dyn Output>> {
    let read = O::OUTPUT_FROM_JSON.ok_or_else(|| {
        crate::Error::internal(format!("{}: the output cannot be replayed", O::INFO.name))
    })?;
    let output = read(value).map_err(|error| {
        crate::Error::internal(format!(
            "{}: a stored output does not read as the operation's output: {error}",
            O::INFO.name
        ))
    })?;
    Ok(Box::new(OperationOutput::<O>(output)))
}

pub(crate) struct OperationOutput<O: Operation>(pub(crate) O::Output);
impl<O: Operation> Output for OperationOutput<O> {
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
    fn as_any(&self) -> &dyn std::any::Any {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::unreadable;
    use crate::validate::ViolationKind;

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        name: String,
    }

    #[test]
    fn an_unknown_member_is_refused_without_naming_it() {
        let secret = "hunter2`, expected `name".repeat(100);
        let value = serde_json::json!({ "name": "a", secret.as_str(): 1 });
        let rejection = unreadable(serde_json::from_value::<Strict>(value).unwrap_err());
        assert_eq!(rejection.violations().len(), 1);
        let violation = &rejection.violations()[0];
        assert_eq!(violation.path, "[key]");
        assert_eq!(violation.message, "is not a member of this structure");
        assert_eq!(violation.kind, ViolationKind::Malformed);
    }

    #[test]
    fn a_missing_member_is_named() {
        let rejection = unreadable(serde_json::from_str::<Strict>("{}").unwrap_err());
        let violation = &rejection.violations()[0];
        assert_eq!(violation.path, "name");
        assert_eq!(violation.message, "is required");
        assert_eq!(violation.kind, ViolationKind::Constraint);
    }
}
