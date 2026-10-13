//! A stage view with guaranteed decoded, validated input.

use std::sync::Arc;

use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::Value;

use crate::operation::{Input, OperationInfo, ServiceInfo};
use crate::{Context, Error, Job, Outcome, Result};

/// The operation's validated input, before its implementation consumes it.
/// The orchestrator constructs this view only after decoding succeeds.
pub struct DecodedJob<'a> {
    job: &'a mut Job,
}

impl<'a> DecodedJob<'a> {
    pub(crate) fn new(job: &'a mut Job) -> Self {
        assert!(job.operation().is_some() && job.input().is_some());
        Self { job }
    }

    pub fn operation(&self) -> &'static OperationInfo {
        self.job.operation().expect("decoded operation")
    }

    /// URI labels selected by the protocol, percent-decoded.
    pub fn labels(&self) -> &[(&'static str, String)] {
        self.job.labels()
    }

    pub fn input(&self) -> &dyn Input {
        self.job.input().expect("decoded input")
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.job.service()
    }

    pub fn metadata(&self) -> &crate::OperationMetadata {
        self.context()
            .operation_metadata()
            .expect("decoded metadata")
    }

    pub fn context(&self) -> &Arc<Context> {
        self.job.context()
    }

    /// Retains a JSON copy for hooks after the operation consumes its input.
    pub fn snapshot_input(&mut self) {
        self.job.snapshot_input();
    }

    /// Ends the invocation with an already serialized response.
    /// Entered interceptors still run their `after` hooks.
    pub fn respond(&mut self, response: Response) {
        self.job.respond(response);
    }

    /// Ends the invocation as if the operation had returned `output`, read
    /// from its JSON form ([`Output::to_json`](crate::operation::Output::to_json)):
    /// the operation does not run, and the protocol encodes the output for
    /// this request as it would the operation's own, at the request's API
    /// version, with its own request id. For answering a retry with what an
    /// earlier attempt produced. Entered interceptors still run their `after`
    /// hooks, and see a success.
    ///
    /// An error when the operation's output cannot be replayed
    /// ([`OperationMetadata::replays_output`](crate::OperationMetadata::replays_output))
    /// or `output` does not read as it; the invocation is left as it was.
    pub fn replay_output(&mut self, output: Value) -> Result<()> {
        let read = self.job.output_from_json.ok_or_else(|| {
            Error::internal(format!(
                "{}: the output cannot be replayed",
                self.operation().name
            ))
        })?;
        self.job.outcome = Some(Outcome::Output(read(output)?));
        Ok(())
    }

    pub fn response_headers_mut(&mut self) -> &mut HeaderMap {
        self.job.response_headers_mut()
    }
}
