use crate::{Hook, OperationAwareMiddleware, OperationJob, Prepare, Result};
use axum::response::Response;

/// Convenience for asynchronous hooks; each call boxes its future.
#[async_trait::async_trait]
pub trait AsyncOperationAwareMiddleware: Send + Sync + 'static {
    async fn before(&self, job: &mut OperationJob<'_>) -> Result<()> {
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

impl<T: AsyncOperationAwareMiddleware> OperationAwareMiddleware for T {
    fn before<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, Result<()>> {
        Hook::Wait(AsyncOperationAwareMiddleware::before(self, job))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        Hook::Wait(AsyncOperationAwareMiddleware::after(self, job))
    }

    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        AsyncOperationAwareMiddleware::complete(self, job, response);
    }
    fn stream_complete(
        &self,
        job: &OperationJob<'_>,
        response: &Response,
        completion: &crate::StreamCompletion,
    ) {
        AsyncOperationAwareMiddleware::stream_complete(self, job, response, completion);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        AsyncOperationAwareMiddleware::prepare(self, prepare)
    }
}
