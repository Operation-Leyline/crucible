//! Extensions that run after a protocol has decoded and validated the input.

use axum::response::Response;

use crate::{DecodedJob, Hook, OperationJob, Prepare, Result};

/// A step around a decoded invocation. `before` runs in registration order;
/// `after` unwinds successful `before` hooks in reverse, before encoding.
/// A decoding failure never enters this chain.
pub trait Interceptor: Send + Sync + 'static {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        let _ = job;
        Hook::Ready(Ok(()))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        let _ = job;
        Hook::Ready(())
    }

    /// Observes the response after native transport middleware finishes. Outer
    /// Tower adapters and body delivery follow; this does not observe body completion.
    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        let _ = (job, response);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}
