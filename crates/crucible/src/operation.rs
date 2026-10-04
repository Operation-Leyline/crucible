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
    /// A trait's JSON value, as written in the model.
    pub fn trait_value(&self, id: &str) -> Option<Value> {
        self.traits
            .iter()
            .find(|(trait_id, _)| *trait_id == id)
            .and_then(|(_, json)| serde_json::from_str(json).ok())
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
    /// `@sensitive`: never written to a log.
    pub sensitive: bool,
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
    fn as_any(&self) -> &dyn Any;
}

impl<T: Serialize + Any + Send + Sync> Output for T {
    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
