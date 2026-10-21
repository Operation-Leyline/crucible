use crate::Error;
use crate::operation::Output;
use axum::response::Response;

/// What an operation produced, or how the request failed.
pub enum Outcome {
    Output(Box<dyn Output>),
    /// The handler opened a response sequence; success is decided at its end.
    Stream(crate::StreamingOutput),
    Failed(Error),
    /// A handler answered with a response of its own ([`crate::Job::respond`]):
    /// the operation did not run, and the protocol does not rewrite it.
    Answered(Response),
}
