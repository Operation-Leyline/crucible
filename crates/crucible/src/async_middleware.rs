use crate::{Hook, Job, Middleware, Prepare, Result};
use axum::response::Response;

#[async_trait::async_trait]
pub trait AsyncMiddleware: Send + Sync + 'static {
    async fn before(&self, request: &mut Job) -> Result<()> {
        let _ = request;
        Ok(())
    }
    async fn after(&self, request: &mut Job, response: &mut Response) {
        let _ = (request, response);
    }
    fn complete(&self, request: &Job, response: &Response) {
        let _ = (request, response);
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}
impl<T: AsyncMiddleware> Middleware for T {
    fn before<'a>(&'a self, request: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Wait(AsyncMiddleware::before(self, request))
    }
    fn after<'a>(&'a self, request: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        Hook::Wait(AsyncMiddleware::after(self, request, response))
    }
    fn complete(&self, request: &Job, response: &Response) {
        AsyncMiddleware::complete(self, request, response);
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        AsyncMiddleware::prepare(self, prepare)
    }
}
