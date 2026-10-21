//! Asynchronous convenience for decoded invocation extensions.

use axum::response::Response;

use crate::{DecodedJob, Hook, Interceptor, OperationJob, Prepare, Result};

/// Convenience for hooks that await work. Each hook allocates its future;
/// synchronous extensions can implement [`Interceptor`] directly.
#[async_trait::async_trait]
pub trait AsyncInterceptor: Send + Sync + 'static {
    async fn before(&self, job: &mut DecodedJob<'_>) -> Result<()> {
        let _ = job;
        Ok(())
    }

    async fn after(&self, job: &mut OperationJob<'_>) {
        let _ = job;
    }

    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        let _ = (job, response);
    }
    fn stream_complete(
        &self,
        job: &OperationJob<'_>,
        response: &Response,
        completion: &crate::StreamCompletion,
    ) {
        let _ = (job, response, completion);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}

impl<T: AsyncInterceptor> Interceptor for T {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        Hook::Wait(AsyncInterceptor::before(self, job))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        Hook::Wait(AsyncInterceptor::after(self, job))
    }

    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        AsyncInterceptor::complete(self, job, response);
    }
    fn stream_complete(
        &self,
        job: &OperationJob<'_>,
        response: &Response,
        completion: &crate::StreamCompletion,
    ) {
        AsyncInterceptor::stream_complete(self, job, response, completion);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        AsyncInterceptor::prepare(self, prepare)
    }
}
