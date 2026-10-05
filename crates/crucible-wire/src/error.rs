//! Errors. An implementation returns `crucible::Result<T>` and may fail with
//! anything: a modelled error, or any other error through `?`. The framework
//! decides afterwards what reaches the caller (see `docs/framework.md`).

use std::fmt;

use serde_json::Value;

use crate::operation::ErrorInfo;

pub type Result<T, E = Error> = std::result::Result<T, E>;

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

    /// This error and its sources, for a log line.
    pub fn chain(&self) -> String {
        let mut text = self.inner.to_string();
        let mut source = self.inner.source();
        while let Some(next) = source {
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
}

/// How a generated error implements [`ModelledError::write_json`]: one pass,
/// no JSON value in between.
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
    UnknownOperation,
    InternalFailure,
}

impl FrameworkError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::UnknownOperation => "UnknownOperation",
            Self::InternalFailure => "InternalFailure",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::UnknownOperation => 404,
            Self::InternalFailure => 500,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::UnknownOperation => "no operation serves this request",
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
pub fn write_error_http<E: ModelledError + serde::Serialize>(
    error: &E,
    out: &mut Vec<u8>,
) -> serde_json::Result<crate::operation::ResponseBindings> {
    let info = error.info();
    crate::split::write_http(error, info.members, Some(info.name), out)
}
