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
    type Input: DeserializeOwned + Serialize + Validate + Send + Sync + 'static;
    type Output: Serialize + Send + Sync + 'static;
}

#[derive(Debug)]
pub struct ServiceInfo {
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
    Header(&'static str),
    Body,
}

/// How a bound member's text reads as JSON. Body members are `Json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar {
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    EpochSeconds,
    HttpDate,
    List(&'static Scalar),
    Json,
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

/// An operation's output, seen by handlers without knowing its type.
pub trait Output: Any + Send + Sync {
    fn to_json(&self) -> Value;
    /// Serializes into `out`. Every serializable output writes straight to
    /// bytes; this default, for other implementors, goes through `to_json`.
    fn write_json(&self, out: &mut Vec<u8>) -> serde_json::Result<()> {
        serde_json::to_writer(out, &self.to_json())
    }
    /// Serializes into `out` without the members `members` binds to
    /// headers, returning those as (header, text). The default goes through
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
