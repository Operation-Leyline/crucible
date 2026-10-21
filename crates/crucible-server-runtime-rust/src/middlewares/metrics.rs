//! Metrics plumbing: a record per request, handed to a recorder the service
//! supplies. Place the handler early in the chain, so its completion hook
//! observes requests a later handler refuses. Records include serialization,
//! before outer tower layers or response-body delivery.

use axum::response::Response;
use std::time::Duration;

use crate::operation::OperationInfo;
use crate::{Job, Middleware, StreamCompletion, StreamEnd};

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestRecord<'a> {
    pub request_id: &'a str,
    /// `None` for a request no operation serves.
    pub operation: Option<&'static OperationInfo>,
    pub status: u16,
    /// `success`, `error` (the caller's), `fault` (the service's), or `cancelled`.
    pub class: &'static str,
    pub error: Option<&'static str>,
    pub latency: Duration,
    /// Present at the termination of a streaming response. Counts describe
    /// production by Crucible, not acknowledgment by the remote client.
    pub stream: Option<&'a StreamCompletion>,
}

pub trait Recorder: Send + Sync + 'static {
    fn record(&self, record: &RequestRecord<'_>);
}

pub struct Metrics<R> {
    recorder: R,
}

impl<R: Recorder> Metrics<R> {
    pub fn new(recorder: R) -> Self {
        Self { recorder }
    }
}

impl<R: Recorder> Middleware for Metrics<R> {
    fn complete(&self, job: &Job, response: &Response) {
        if job.is_streaming_response() {
            return;
        }
        let operation = job.operation();
        let status = response.status().as_u16();
        let error = if response
            .extensions()
            .get::<crate::protocol::SerializationFailure>()
            .is_some()
        {
            Some("InternalFailure")
        } else {
            job.resolution()
                .and_then(|resolution| resolution.error_name())
        };
        self.recorder.record(&RequestRecord {
            request_id: job.context().request_id(),
            operation,
            status,
            class: match status {
                0..400 => "success",
                400..500 => "error",
                _ => "fault",
            },
            error,
            latency: job.context().received().elapsed(),
            stream: None,
        });
    }

    fn stream_complete(&self, job: &Job, response: &Response, completion: &StreamCompletion) {
        let resolution = job.resolution();
        self.recorder.record(&RequestRecord {
            request_id: job.context().request_id(),
            operation: job.operation(),
            status: response.status().as_u16(),
            class: match completion.end {
                StreamEnd::Completed => "success",
                StreamEnd::Cancelled => "cancelled",
                StreamEnd::Failed
                    if resolution.as_ref().is_none_or(|r| r.error_name().is_none()) =>
                {
                    if response.status().is_client_error() {
                        "error"
                    } else {
                        "fault"
                    }
                }
                StreamEnd::Failed
                    if resolution
                        .as_ref()
                        .is_some_and(|r| r.status(job.operation()) < 500) =>
                {
                    "error"
                }
                StreamEnd::Failed => "fault",
            },
            error: resolution.and_then(|r| r.error_name()),
            latency: job.context().received().elapsed(),
            stream: Some(completion),
        });
    }
}
