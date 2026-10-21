use crate::error::{Error, FrameworkError, ModelledError};
use crate::operation::{OperationInfo, Output};
use crate::validate::Rejection;

/// How an outcome is answered, decided once for the protocol, the service
/// log and metrics alike.
pub enum Resolution<'a> {
    Success(&'a dyn Output),
    Streaming,
    /// A modelled error the operation declares.
    Modelled(&'a dyn ModelledError),
    /// A request that broke the model: answered as the validation error.
    Rejected(&'a Rejection),
    /// Answered by the framework itself (an unknown operation).
    Framework(FrameworkError),
    /// Anything else: logged, and answered with no detail.
    Internal(&'a Error),
    /// A handler answered with its own response, sent as it is.
    Answered {
        status: u16,
    },
}

impl Resolution<'_> {
    /// The response status this resolution gets.
    pub fn status(&self, operation: Option<&OperationInfo>) -> u16 {
        match self {
            Self::Success(_) | Self::Streaming => operation.map_or(200, |op| op.status),
            Self::Modelled(error) => error.info().status,
            Self::Rejected(_) => 400,
            Self::Framework(error) => error.status(),
            Self::Internal(_) => 500,
            Self::Answered { status } => *status,
        }
    }

    /// `success`, `error` (the caller's) or `fault` (the service's).
    pub fn class(&self, operation: Option<&OperationInfo>) -> &'static str {
        match self.status(operation) {
            0..400 => "success",
            400..500 => "error",
            _ => "fault",
        }
    }

    /// The error's wire name, when the request failed.
    pub fn error_name(&self) -> Option<&'static str> {
        match self {
            Self::Success(_) | Self::Streaming | Self::Answered { .. } => None,
            Self::Modelled(error) => Some(error.info().name),
            Self::Rejected(_) => Some("ValidationError"),
            Self::Framework(error) => Some(error.name()),
            Self::Internal(_) => Some("InternalFailure"),
        }
    }
}
