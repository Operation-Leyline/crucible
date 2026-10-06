//! Transport policies for a selected operation, before decoding.
use crate::{Hook, OperationJob, Prepare, Result};
use axum::response::Response;

/// Admission, forwarding, and policy based on operation metadata and the
/// transport request. These hooks always run after identification and before
/// decoding; their return hooks unwind before protocol encoding.
pub trait OperationAwareMiddleware: Send + Sync + 'static {
    fn before<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, Result<()>> {
        let _ = job;
        Hook::Ready(Ok(()))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        let _ = job;
        Hook::Ready(())
    }

    /// Observes the final native middleware response, before body delivery.
    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        let _ = (job, response);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}
