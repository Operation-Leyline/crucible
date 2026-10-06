//! A stage view with guaranteed decoded, validated input.

use std::sync::Arc;

use axum::http::HeaderMap;
use axum::response::Response;

use crate::operation::{Input, OperationInfo, ServiceInfo};
use crate::{Context, Job};

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

    pub fn response_headers_mut(&mut self) -> &mut HeaderMap {
        self.job.response_headers_mut()
    }
}
