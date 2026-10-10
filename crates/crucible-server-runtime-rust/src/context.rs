//! The request context: what an implementation knows about the request
//! besides its input. Implementations read it as `self.context()`.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;

use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap, HeaderValue};

use crate::OperationMetadata;
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
    operation: OnceLock<&'static OperationInfo>,
    operation_metadata: OnceLock<OperationMetadata>,
    api_version: OnceLock<crate::versioning::ApiVersion>,
    received: Instant,
    deadline: Mutex<Option<Instant>>,
    /// The request head, held once: the job reads it from here.
    pub(crate) request: Parts,
    attributes: Mutex<Extensions>,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}

impl Context {
    pub(crate) fn new(
        request_id: HeaderValue,
        operation: Option<&'static OperationInfo>,
        request: Parts,
    ) -> Self {
        let span = tracing::info_span!(
            "crucible.request",
            request_id = request_id.to_str().unwrap_or_default(),
            operation = operation.map_or("unknown", |op| op.name)
        );
        Self {
            request_id,
            operation: operation.map_or_else(OnceLock::new, OnceLock::from),
            operation_metadata: OnceLock::new(),
            api_version: OnceLock::new(),
            received: Instant::now(),
            deadline: Mutex::new(None),
            request,
            attributes: Mutex::new(Extensions::new()),
            span,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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
        let span = self.span.clone();
        let dispatch = self.dispatch.clone();
        CURRENT
            .scope(self, future)
            .instrument(span)
            .with_subscriber(dispatch)
            .await
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
        self.operation.get().copied()
    }

    /// Effective protocol and authentication settings after operation identification.
    /// Authentication schemes are metadata, not proof that credentials were verified.
    pub fn operation_metadata(&self) -> Option<&OperationMetadata> {
        self.operation_metadata.get()
    }

    pub(crate) fn select_metadata(&self, metadata: OperationMetadata) {
        self.operation_metadata
            .set(metadata)
            .expect("metadata selected once");
    }

    pub(crate) fn select_api_version(&self, version: crate::versioning::ApiVersion) {
        let _ = self.api_version.set(version);
    }

    /// The API version the request is served at, for a service whose model
    /// has migrations: a behaviour change branches on it
    /// (`self.context().api_version() < Some(V2026_11_01)`).
    pub fn api_version(&self) -> Option<crate::versioning::ApiVersion> {
        self.api_version.get().copied()
    }

    pub(crate) fn select_operation(&self, operation: &'static OperationInfo) {
        self.operation
            .set(operation)
            .expect("operation selected once");
        self.span.record("operation", operation.name);
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

    /// A value an extension stored for this request, such as the caller.
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
