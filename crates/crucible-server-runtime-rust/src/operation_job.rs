use std::sync::Arc;

use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::Value;

use crate::{Context, Error, Job, OperationInfo, Outcome, Resolution, Result, ServiceInfo};

/// The transport request after a protocol identified its operation. Decoded
/// input is deliberately unavailable: use an interceptor for model values.
pub struct OperationJob<'a> {
    job: &'a mut Job,
}

impl<'a> OperationJob<'a> {
    pub fn is_streaming_response(&self) -> bool {
        self.job.is_streaming_response()
    }
    pub(crate) fn new(job: &'a mut Job) -> Self {
        assert!(
            job.operation().is_some(),
            "an operation job requires a selected operation"
        );
        Self { job }
    }

    pub fn operation(&self) -> &'static OperationInfo {
        self.job
            .operation()
            .expect("an operation job has a selected operation")
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.job.service()
    }

    pub fn metadata(&self) -> &crate::OperationMetadata {
        self.context()
            .operation_metadata()
            .expect("identified metadata")
    }

    pub fn context(&self) -> &Arc<Context> {
        self.job.context()
    }

    pub fn request(&self) -> &Parts {
        self.job.request()
    }

    pub fn body(&self) -> &Bytes {
        self.job.body()
    }

    /// Buffers a non-streaming body once or prepares its bounded stream.
    pub async fn read_body(&mut self) -> Result<()> {
        self.job.read_body().await
    }

    pub fn labels(&self) -> &[(&'static str, String)] {
        self.job.labels()
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.job.outcome()
    }

    pub fn resolution(&self) -> Option<Resolution<'_>> {
        self.job.resolution()
    }

    pub fn replace_error(&mut self, error: Error) {
        self.job.replace_error(error);
    }

    /// Fails the request, even one whose operation succeeded: for the
    /// runtime's own assertions after the operation (an open permission
    /// obligation).
    #[cfg(feature = "permissions")]
    pub(crate) fn fail(&mut self, error: Error) {
        self.job.fail(error);
    }

    /// Answers without decoding or entering interceptors and the operation.
    pub fn respond(&mut self, response: Response) {
        self.job.respond(response);
    }

    pub fn response_headers_mut(&mut self) -> &mut HeaderMap {
        self.job.response_headers_mut()
    }

    /// Requests a JSON snapshot once the input has been decoded. The
    /// operation consumes the typed input, so return hooks use this snapshot.
    pub fn snapshot_input(&mut self) {
        self.job.snapshot_input();
    }

    pub fn input_snapshot(&self) -> Option<&Value> {
        self.job.input_snapshot()
    }
}
