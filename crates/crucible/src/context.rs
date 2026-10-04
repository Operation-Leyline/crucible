//! The request context: what an implementation knows about the request
//! besides its input. Implementations read it as `self.context()`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap, HeaderValue};

use crate::operation::OperationInfo;

tokio::task_local! {
    static CURRENT: Arc<Context>;
}

/// What a host knows about a request before the service sees it, put in
/// the request's extensions: the id the platform gave it, and when the
/// platform stops waiting. The Lambda host sets both from the invocation.
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub request_id: Option<HeaderValue>,
    pub deadline: Option<Instant>,
}

#[derive(Debug)]
pub struct Context {
    /// Visible ASCII by construction, so `request_id` reads it as text.
    request_id: HeaderValue,
    operation: Option<&'static OperationInfo>,
    received: Instant,
    deadline: Mutex<Option<Instant>>,
    /// Shared with the job, not copied: one request head per request.
    request: Arc<Parts>,
    attributes: Mutex<Extensions>,
}

impl Context {
    pub(crate) fn new(
        request_id: HeaderValue,
        operation: Option<&'static OperationInfo>,
        request: Arc<Parts>,
    ) -> Self {
        Self {
            request_id,
            operation,
            received: Instant::now(),
            deadline: Mutex::new(None),
            request,
            attributes: Mutex::new(Extensions::new()),
        }
    }

    /// The context of the request this task is serving.
    ///
    /// # Panics
    /// Outside a request: on a task the framework did not start, or on a task
    /// spawned without [`Context::scope`].
    pub fn current() -> Arc<Context> {
        Self::try_current()
            .expect("no request context: not on a request's task (see Context::scope)")
    }

    pub fn try_current() -> Option<Arc<Context>> {
        CURRENT.try_with(Arc::clone).ok()
    }

    /// Runs `future` with this context current: for a task an implementation
    /// spawns, which does not inherit it.
    pub async fn scope<F: Future>(self: Arc<Self>, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }

    /// The id this request goes by in logs and in the `x-request-id` header.
    pub fn request_id(&self) -> &str {
        self.request_id.to_str().unwrap_or_default()
    }

    pub(crate) fn request_id_header(&self) -> &HeaderValue {
        &self.request_id
    }

    /// The operation serving the request; `None` when no operation does.
    pub fn operation(&self) -> Option<&'static OperationInfo> {
        self.operation
    }

    pub fn received(&self) -> Instant {
        self.received
    }

    pub fn deadline(&self) -> Option<Instant> {
        *self.deadline.lock().expect("deadline lock")
    }

    pub fn set_deadline(&self, deadline: Instant) {
        *self.deadline.lock().expect("deadline lock") = Some(deadline);
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.request.headers
    }

    /// A value a handler stored for this request, such as the caller.
    pub fn attribute<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.attributes
            .lock()
            .expect("attributes lock")
            .get::<T>()
            .cloned()
    }

    pub fn set_attribute<T: Clone + Send + Sync + 'static>(&self, value: T) {
        self.attributes
            .lock()
            .expect("attributes lock")
            .insert(value);
    }
}

/// `self.context()` for every type: the request context, read like a field
/// of the type implementing an operation.
pub trait WithContext {
    fn context(&self) -> Arc<Context> {
        Context::current()
    }
}

impl<T: ?Sized> WithContext for T {}
