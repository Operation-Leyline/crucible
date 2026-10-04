//! Metrics plumbing: a record per request, handed to a recorder the service
//! supplies. Place the handler early in the chain, so its `after` runs for
//! requests a later handler refuses.

use std::time::Duration;

use crate::handler::{Handler, Job};
use crate::operation::OperationInfo;

#[derive(Debug, Clone)]
pub struct RequestRecord<'a> {
    pub request_id: &'a str,
    /// `None` for a request no operation serves.
    pub operation: Option<&'static OperationInfo>,
    pub status: u16,
    /// `success`, `error` (the caller's) or `fault` (the service's).
    pub class: &'static str,
    pub error: Option<&'static str>,
    pub latency: Duration,
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

#[async_trait::async_trait]
impl<R: Recorder> Handler for Metrics<R> {
    async fn after(&self, job: &mut Job) {
        let Some(resolution) = job.resolution() else {
            return;
        };
        let operation = job.operation();
        self.recorder.record(&RequestRecord {
            request_id: job.context().request_id(),
            operation,
            status: resolution.status(operation),
            class: resolution.class(operation),
            error: resolution.error_name(),
            latency: job.context().received().elapsed(),
        });
    }
}
