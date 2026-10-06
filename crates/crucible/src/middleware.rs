//! Transport extensions. Before runs before identification; after sees encoded bytes.
use crate::{Hook, Job, Prepare, Result};
use axum::response::Response;

pub trait Middleware: Send + Sync + 'static {
    fn before<'a>(&'a self, request: &'a mut Job) -> Hook<'a, Result<()>> {
        let _ = request;
        Hook::Ready(Ok(()))
    }
    /// Runs after serialization, in reverse order for successful before entries.
    fn after<'a>(&'a self, request: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        let _ = (request, response);
        Hook::Ready(())
    }
    /// Response ready, after native transformations; not response-body delivery.
    fn complete(&self, request: &Job, response: &Response) {
        let _ = (request, response);
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}
