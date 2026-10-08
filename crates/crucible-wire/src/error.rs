//! Errors. An implementation returns `crucible_server_runtime_rust::Result<T>` and may fail with
//! anything: a modelled error, or any other error through `?`. The framework
//! decides afterwards what reaches the caller (see `docs/framework.md`).

use std::fmt;

use serde_json::Value;

use crate::operation::ErrorInfo;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Maximum depth of error source traversal to protect against cycles.
pub const MAX_SOURCE_DEPTH: usize = 64;

/// Any error. Like `anyhow::Error` it is not itself a `std::error::Error`,
/// so every error type converts into it with `?`.
pub struct Error {
    inner: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl Error {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(error),
        }
    }

    /// An internal failure with a message for the logs. Callers never see it.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(Internal(message.into()))
    }

    pub fn get_ref(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        &*self.inner
    }

    pub fn downcast_ref<E: std::error::Error + 'static>(&self) -> Option<&E> {
        self.inner.downcast_ref::<E>()
    }

    /// This error and its sources, for a log line. Traversal depth is bounded
    /// by [`MAX_SOURCE_DEPTH`] to protect against cycles in source chains.
    pub fn chain(&self) -> String {
        let mut text = self.inner.to_string();
        let mut source = self.inner.source();
        for _ in 0..MAX_SOURCE_DEPTH {
            let Some(next) = source else { break };
            text.push_str(": ");
            text.push_str(&next.to_string());
            source = next.source();
        }
        text
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self::new(error)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

#[derive(Debug)]
struct Internal(String);

impl fmt::Display for Internal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Internal {}

/// An error shape from the model. Generated for every `@error` structure.
pub trait ModelledError: std::error::Error + Send + Sync + 'static {
    fn info(&self) -> &'static ErrorInfo;
    /// Original Smithy shape id, independent of service-local Rust renames.
    fn shape_id(&self) -> &'static str {
        self.info().name
    }
    fn write_cbor(&self, out: &mut Vec<u8>) -> Result<(), String> {
        crate::cbor::write_typed(&self.to_json(), self.shape_id(), out)
    }
    fn response_status(&self) -> u16 {
        self.info().status
    }
    /// Every member, as on the wire; the protocol moves header members out.
    fn to_json(&self) -> Value;

    /// The body (members plus `"__type"`) straight into `out`, without the
    /// header members, which come back as (header, text). Generated errors
    /// write in one pass through [`write_error`]; this default goes through
    /// [`ModelledError::to_json`].
    fn write_json(&self, out: &mut Vec<u8>) -> serde_json::Result<Vec<(&'static str, String)>> {
        let info = self.info();
        crate::split::write_typed(&self.to_json(), info.members, Some(info.name), out)
    }
    fn write_http(
        &self,
        out: &mut Vec<u8>,
    ) -> serde_json::Result<crate::operation::ResponseBindings> {
        let info = self.info();
        crate::split::write_http(&self.to_json(), info.members, Some(info.name), out)
    }

    /// Serializes with the discriminator chosen by the serving protocol.
    /// Payload bytes are retained; protocols put their discriminator in a header.
    fn write_http_typed(
        &self,
        type_name: Option<&'static str>,
        out: &mut Vec<u8>,
    ) -> serde_json::Result<crate::operation::ResponseBindings> {
        crate::split::write_http(&self.to_json(), self.info().members, type_name, out)
    }
}

/// How a generated error implements [`ModelledError::write_json`]: one pass,
/// no JSON value in between.
#[doc(hidden)]
pub fn write_error<E: ModelledError + serde::Serialize>(
    error: &E,
    out: &mut Vec<u8>,
) -> serde_json::Result<Vec<(&'static str, String)>> {
    let info = error.info();
    crate::split::write_typed(error, info.members, Some(info.name), out)
}

/// Errors the framework answers itself, outside any model: a request no
/// operation serves, or an internal failure when the service models no
/// internal error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameworkError {
    /// Credentials were absent or refused by an authentication extension.
    Unauthenticated,
    /// An authenticated principal cannot perform the requested action.
    Forbidden,
    UnknownOperation,
    MethodNotAllowed {
        allowed: Vec<&'static str>,
    },
    UnsupportedMediaType,
    InternalFailure,
}

impl FrameworkError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "UnauthenticatedException",
            Self::Forbidden => "ForbiddenException",
            Self::UnknownOperation => "UnknownOperation",
            Self::MethodNotAllowed { .. } => "MethodNotAllowed",
            Self::UnsupportedMediaType => "UnsupportedMediaTypeException",
            Self::InternalFailure => "InternalFailure",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::Forbidden => 403,
            Self::UnknownOperation => 404,
            Self::MethodNotAllowed { .. } => 405,
            Self::UnsupportedMediaType => 415,
            Self::InternalFailure => 500,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "authentication is required",
            Self::Forbidden => "access is forbidden",
            Self::UnknownOperation => "no operation serves this request",
            Self::MethodNotAllowed { .. } => "the request method is not allowed for this target",
            Self::UnsupportedMediaType => "the request content-type is not supported",
            Self::InternalFailure => "the request failed; quote the request id when reporting it",
        }
    }
}

impl fmt::Display for FrameworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for FrameworkError {}

/// Serializes a generated error's body and dynamic HTTP bindings in one pass.
#[doc(hidden)]
pub fn write_error_http<E: ModelledError + serde::Serialize>(
    error: &E,
    out: &mut Vec<u8>,
) -> serde_json::Result<crate::operation::ResponseBindings> {
    let info = error.info();
    crate::split::write_http(error, info.members, Some(info.name), out)
}

/// A generated error with its serving protocol's discriminator, in one pass.
#[doc(hidden)]
pub fn write_error_http_typed<E: ModelledError + serde::Serialize>(
    error: &E,
    type_name: Option<&'static str>,
    out: &mut Vec<u8>,
) -> serde_json::Result<crate::operation::ResponseBindings> {
    crate::split::write_http(error, error.info().members, type_name, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct SimpleError(&'static str);

    impl fmt::Display for SimpleError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for SimpleError {}

    #[derive(Debug)]
    struct NestedError {
        msg: &'static str,
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    }

    impl fmt::Display for NestedError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.msg)
        }
    }

    impl std::error::Error for NestedError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|s| s as &(dyn std::error::Error + 'static))
        }
    }

    #[derive(Debug)]
    struct CyclicError;

    impl fmt::Display for CyclicError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("loop")
        }
    }

    impl std::error::Error for CyclicError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(self)
        }
    }

    #[derive(Debug)]
    struct CycleA;
    #[derive(Debug)]
    struct CycleB;
    static CYCLE_A: CycleA = CycleA;
    static CYCLE_B: CycleB = CycleB;

    impl fmt::Display for CycleA {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("node_a")
        }
    }

    impl std::error::Error for CycleA {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&CYCLE_B)
        }
    }

    impl fmt::Display for CycleB {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("node_b")
        }
    }

    impl std::error::Error for CycleB {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&CYCLE_A)
        }
    }

    #[test]
    fn chain_with_no_source() {
        let error = Error::internal("standalone");
        assert_eq!(error.chain(), "standalone");
    }

    #[test]
    fn chain_with_sources() {
        let leaf = SimpleError("leaf");
        let middle = NestedError {
            msg: "middle",
            source: Some(Box::new(leaf)),
        };
        let root = NestedError {
            msg: "root",
            source: Some(Box::new(middle)),
        };
        let error = Error::new(root);
        assert_eq!(error.chain(), "root: middle: leaf");
    }

    #[test]
    fn chain_terminates_on_self_referential_cycle() {
        let error = Error::new(CyclicError);
        let chain = error.chain();
        assert_eq!(chain.matches(": loop").count(), MAX_SOURCE_DEPTH);
        assert!(chain.starts_with("loop: loop"));
    }

    #[test]
    fn chain_terminates_on_mutual_cycle() {
        let error = Error::new(CycleA);
        let chain = error.chain();
        let total_sources = chain.matches(": node_a").count() + chain.matches(": node_b").count();
        assert_eq!(total_sources, MAX_SOURCE_DEPTH);
        assert!(chain.starts_with("node_a: node_b: node_a"));
    }

    #[test]
    fn chain_bounds_deep_error_hierarchy() {
        let mut current = NestedError {
            msg: "leaf",
            source: None,
        };
        for _ in 0..100 {
            current = NestedError {
                msg: "step",
                source: Some(Box::new(current)),
            };
        }
        let error = Error::new(current);
        let chain = error.chain();
        assert_eq!(chain.matches(": ").count(), MAX_SOURCE_DEPTH);
    }
}
