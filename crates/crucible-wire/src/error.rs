//! Errors. An implementation returns `crucible_server_runtime_rust::Result<T>` and may fail with
//! anything: a modelled error, or any other error through `?`. The framework
//! decides afterwards what reaches the caller (see `docs/framework.md`).

use std::fmt;

use serde_json::Value;

use crate::operation::ErrorInfo;
use crate::permissions::{PermissionInfo, ResourceInfo};

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
///
/// Non-exhaustive: the framework may answer new errors, so a `match` outside
/// this crate needs a wildcard arm. [`name`](Self::name),
/// [`status`](Self::status), [`message`](Self::message) and
/// [`shape_id`](Self::shape_id) cover every variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameworkError {
    /// Credentials were absent or refused by an authentication extension.
    Unauthenticated,
    /// An authenticated principal cannot perform the requested action.
    Forbidden,
    /// The request conflicts with one the service already accepted: an
    /// idempotency token reused for a different request, or for one still
    /// running. The message says which, and what the caller can do.
    Conflict {
        message: &'static str,
    },
    /// The resource does not exist, or the caller may not know that it
    /// does: the modelled `crucible.errors#NotFoundException` (404), whose
    /// members are the resource's fixed message ([`ResourceInfo::not_found`])
    /// and `resourceType`, its authorization name. An authorization that
    /// hides a resource and a handler reporting genuine absence both answer
    /// it, so the two are indistinguishable. It never names a permission or
    /// an identifier.
    NotFound {
        resource: &'static ResourceInfo,
    },
    /// An authenticated caller can see the resource but lacks `permission`
    /// on it: a 403 `ForbiddenException`, like `Forbidden`, whose message is
    /// the permission's [`PermissionInfo::denied`], naming its value and
    /// resource type, unless the permission is internal: then the message
    /// names neither. Unlike `Forbidden` it sends no `insufficient_scope`
    /// challenge: no token would carry a grant.
    PermissionDenied {
        permission: &'static PermissionInfo,
    },
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
            Self::Conflict { .. } => "ConflictException",
            Self::NotFound { .. } => "NotFoundException",
            Self::PermissionDenied { .. } => "ForbiddenException",
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
            Self::Conflict { .. } => 409,
            Self::NotFound { .. } => 404,
            Self::PermissionDenied { .. } => 403,
            Self::UnknownOperation => 404,
            Self::MethodNotAllowed { .. } => 405,
            Self::UnsupportedMediaType => 415,
            Self::InternalFailure => 500,
        }
    }

    /// The absolute shape id of the error, as the RPC v2 protocols name it
    /// in `__type`: Smithy's `smithy.framework` errors, and the modelled
    /// `crucible.errors#NotFoundException` for [`Self::NotFound`].
    pub fn shape_id(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "smithy.framework#UnauthenticatedException",
            Self::Forbidden | Self::PermissionDenied { .. } => {
                "smithy.framework#ForbiddenException"
            }
            Self::Conflict { .. } => "smithy.framework#ConflictException",
            Self::NotFound { .. } => "crucible.errors#NotFoundException",
            Self::UnknownOperation => "smithy.framework#UnknownOperationException",
            Self::MethodNotAllowed { .. } => "smithy.framework#MethodNotAllowedException",
            Self::UnsupportedMediaType => "smithy.framework#UnsupportedMediaTypeException",
            Self::InternalFailure => "smithy.framework#InternalFailureException",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "authentication is required",
            Self::Forbidden => "access is forbidden",
            Self::Conflict { message } => message,
            Self::NotFound { resource } => resource.not_found,
            Self::PermissionDenied { permission } if permission.is_public() => permission.denied,
            Self::PermissionDenied { .. } => "This principal may not perform this operation.",
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
    fn not_found_and_permission_denied_name_only_what_they_may() {
        use crate::permissions::Tier;

        static INBOX: ResourceInfo = ResourceInfo {
            name: "inbox",
            identifiers: &["workspaceId", "inboxId"],
            not_found: "No such inbox.",
        };
        static SEND: PermissionInfo = PermissionInfo {
            resource: &INBOX,
            symbol: "mailSend",
            value: "mail:send",
            aliases: &[],
            tier: Tier::Public,
            denied: "This principal does not hold mail:send on that inbox.",
        };
        static WATCH: PermissionInfo = PermissionInfo {
            resource: &INBOX,
            symbol: "watch",
            value: "inbox:watch",
            aliases: &[],
            tier: Tier::Internal,
            denied: "This principal does not hold inbox:watch on that inbox.",
        };
        let missing = FrameworkError::NotFound { resource: &INBOX };
        assert_eq!(missing.status(), 404);
        assert_eq!(missing.name(), "NotFoundException");
        assert_eq!(missing.message(), "No such inbox.");

        let denied = FrameworkError::PermissionDenied { permission: &SEND };
        assert_eq!(denied.status(), 403);
        assert_eq!(denied.name(), "ForbiddenException");
        assert_eq!(
            denied.message(),
            "This principal does not hold mail:send on that inbox."
        );
        // An internal permission's own message is never used.
        let internal = FrameworkError::PermissionDenied { permission: &WATCH };
        assert_eq!(
            internal.message(),
            "This principal may not perform this operation."
        );
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

    #[test]
    fn rpc_v2_shape_ids_are_absolute_and_name_the_error() {
        for error in [
            FrameworkError::Unauthenticated,
            FrameworkError::Forbidden,
            FrameworkError::Conflict { message: "taken" },
            FrameworkError::UnknownOperation,
            FrameworkError::MethodNotAllowed {
                allowed: vec!["GET"],
            },
            FrameworkError::UnsupportedMediaType,
            FrameworkError::InternalFailure,
        ] {
            let id = error.shape_id();
            assert!(id.starts_with("smithy.framework#"), "{id}");
            assert!(
                id.trim_start_matches("smithy.framework#")
                    .starts_with(error.name()),
                "{id} for {}",
                error.name()
            );
        }
    }
}
