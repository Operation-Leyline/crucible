//! RequestTag: an async transport middleware.

use axum::http::HeaderValue;
use axum::response::Response;
use crucible_server_runtime_rust::{AsyncMiddleware, Job, Result};

/// Tags every request before the operation is known and echoes the tag on the
/// response, the way a service attaches a correlation id from an async lookup.
pub struct RequestTag;

#[derive(Clone)]
struct Tag(String);

#[crucible_server_runtime_rust::async_trait]
impl AsyncMiddleware for RequestTag {
    async fn before(&self, job: &mut Job) -> Result<()> {
        // Stands in for an await on a cache or a tracing backend.
        tokio::task::yield_now().await;
        job.context()
            .set_attribute(Tag(format!("tag-{}", job.context().request_id())));
        Ok(())
    }

    async fn after(&self, job: &mut Job, response: &mut Response) {
        if let Some(Tag(tag)) = job.context().attribute::<Tag>() {
            response.headers_mut().insert(
                "x-leyline-tag",
                HeaderValue::from_str(&tag).expect("request ids are header-safe"),
            );
        }
    }
}
