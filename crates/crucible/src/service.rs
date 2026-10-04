//! The orchestrator: routes a request, runs the handler chain around the
//! operation, and has the routing protocol write the response.

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request};
use axum::response::Response;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

use crate::context::{Context, RequestMeta};
use crate::error::{Error, FrameworkError, ModelledError};
use crate::handler::{Handler, Job, Outcome, Prepare};
use crate::handlers::{ExceptionTranslator, ServiceLog};
use crate::operation::{Operation, OperationInfo};
use crate::operations::{Invoke, Operations};
use crate::protocol::{Answers, Invocation, InvokeProtocol, JsonProtocol, Route};
use crate::validate::Rejection;

/// The invocation answer when no protocol answered with JSON.
pub(crate) const INVOKE_FAILURE: &str =
    r#"{"error":{"__type":"InternalFailure","message":"no protocol answered the invocation"}}"#;

type Reject = Arc<dyn Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync>;

/// The service as a tower service: what a [`Builder::layer`] wraps.
pub type Endpoint = BoxCloneSyncService<Request<Body>, Response, Infallible>;

type Wrap = Box<dyn FnOnce(Endpoint) -> Endpoint + Send + Sync>;

/// Picks a request's id from the request: an inbound header a trusted edge
/// sets, say. `None` falls back to the host's id, then a fresh one.
pub type RequestIdSource = fn(&axum::http::request::Parts) -> Option<HeaderValue>;

/// The longest request id taken from outside; a longer one is replaced.
const MAX_REQUEST_ID: usize = 128;

/// A random UUID (v4) as a header value, formatted once.
fn request_id() -> HeaderValue {
    let mut text = [0u8; uuid::fmt::Hyphenated::LENGTH];
    let text = uuid::Uuid::new_v4().hyphenated().encode_lower(&mut text);
    HeaderValue::from_str(text).expect("a UUID is visible ASCII")
}

/// Bodies above this are refused unread, unless the builder says otherwise.
pub const DEFAULT_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// A service: its operations, its handler chain, and the protocols in it.
#[derive(Clone)]
pub struct Crucible {
    inner: Arc<Inner>,
    /// The layered service, when the builder added layers; every entry point
    /// goes through it. Without layers requests go straight to the chain.
    endpoint: Option<Endpoint>,
}

struct Inner {
    operations: Operations,
    handlers: Vec<Box<dyn Handler>>,
    /// Indexes into `handlers` of the protocol handlers, in chain order.
    protocols: Vec<usize>,
    reject: Reject,
    body_limit: usize,
    /// Per-operation overrides of `body_limit`, by operation shape id.
    body_limits: HashMap<&'static str, usize>,
    request_id: Option<RequestIdSource>,
}

impl Crucible {
    pub fn builder(operations: Operations) -> Builder {
        Builder {
            operations,
            handlers: Vec::new(),
            layers: Vec::new(),
            reject: None,
            body_limit: DEFAULT_BODY_LIMIT,
            body_limits: Vec::new(),
            request_id: None,
        }
    }

    /// An axum router that hands every request to the service, through its
    /// layers. Layers added here instead apply on this host only; prefer
    /// [`Builder::layer`], which every host runs.
    pub fn into_router(self) -> axum::Router {
        axum::Router::new().fallback(move |request: Request<Body>| {
            let service = self.clone();
            async move { service.handle(request).await }
        })
    }

    /// Calls an operation by name with its input as JSON bytes, through the
    /// whole handler chain, without HTTP, and answers `{"output": ..}` or
    /// `{"error": ..}` as bytes. The input is read straight from `input`, in
    /// one pass. For direct Lambda invokes and in-process callers; needs an
    /// [`InvokeProtocol`] in the chain.
    pub async fn invoke_json(&self, operation: &str, input: impl Into<Bytes>) -> Bytes {
        self.invoke_json_with(operation, input, RequestMeta::default())
            .await
    }

    /// [`Crucible::invoke_json`] with what the host knows about the call.
    pub async fn invoke_json_with(
        &self,
        operation: &str,
        input: impl Into<Bytes>,
        meta: RequestMeta,
    ) -> Bytes {
        let mut request = Request::post("/")
            .body(Body::from(input.into()))
            .expect("a POST to / is a valid request");
        request.extensions_mut().insert(Invocation {
            operation: operation.to_owned(),
        });
        request.extensions_mut().insert(meta);
        let response = self.handle(request).await;
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|_| Bytes::from_static(INVOKE_FAILURE.as_bytes()))
    }

    /// [`Crucible::invoke_json`] with JSON values, for tests and callers
    /// that already hold one.
    pub async fn invoke(&self, operation: &str, input: serde_json::Value) -> serde_json::Value {
        let input = serde_json::to_vec(&input).unwrap_or_default();
        let answer = self.invoke_json(operation, input).await;
        serde_json::from_slice(&answer).unwrap_or_else(|_| {
            serde_json::from_str(INVOKE_FAILURE).expect("the failure envelope is JSON")
        })
    }

    /// Answers one request: through the layers, then the handler chain.
    pub async fn handle(&self, request: Request<Body>) -> Response {
        match &self.endpoint {
            Some(endpoint) => match endpoint.clone().oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            },
            None => self.dispatch(request).await,
        }
    }

    /// The handler chain around the operation, under the layers.
    async fn dispatch(&self, request: Request<Body>) -> Response {
        let inner = &self.inner;
        let (parts, body) = request.into_parts();
        let routed = inner.protocols.iter().find_map(|&index| {
            let protocol = inner.handlers[index].as_protocol()?;
            protocol
                .route(&parts, &inner.operations)
                .map(|route| (index, route))
        });
        let (protocol, route) = match routed {
            Some((index, route)) => (index, Some(route)),
            None => {
                let owner = inner.protocols.iter().copied().find(|&index| {
                    inner.handlers[index]
                        .as_protocol()
                        .is_some_and(|protocol| protocol.owns(&parts))
                });
                (owner.unwrap_or(inner.protocols[0]), None)
            }
        };
        let entry = route
            .as_ref()
            .and_then(|route| inner.operations.entry(route.operation));
        let meta = parts.extensions.get::<RequestMeta>();
        let id = inner
            .request_id
            .and_then(|source| source(&parts))
            .or_else(|| meta.and_then(|meta| meta.request_id.clone()))
            .filter(|id| !id.is_empty() && id.len() <= MAX_REQUEST_ID && id.to_str().is_ok())
            .unwrap_or_else(request_id);
        let deadline = meta.and_then(|meta| meta.deadline);
        let parts = Arc::new(parts);
        let context = Arc::new(Context::new(
            id,
            entry.map(|entry| entry.info),
            parts.clone(),
        ));
        if let Some(deadline) = deadline {
            context.set_deadline(deadline);
        }

        let limit = entry
            .and_then(|entry| inner.body_limits.get(entry.info.id))
            .copied()
            .unwrap_or(inner.body_limit);
        let body = axum::body::to_bytes(body, limit).await;
        let mut job = Job {
            service: inner.operations.service,
            context: context.clone(),
            request: parts,
            body: body.as_ref().cloned().unwrap_or_default(),
            labels: route.map(|Route { labels, .. }| labels).unwrap_or_default(),
            decode: entry.map(|entry| entry.decode),
            decode_json: entry.map(|entry| entry.decode_json),
            decode_bound: entry.map(|entry| entry.decode_bound),
            input: None,
            outcome: None,
            response_headers: Default::default(),
            wants_snapshot: false,
            input_snapshot: None,
        };
        if body.is_err() {
            job.outcome = Some(Outcome::Failed(
                Rejection::malformed("", format!("the body is larger than {limit} bytes")).into(),
            ));
        }

        context
            .clone()
            .scope(async move {
                self.run(&mut job, entry.map(|entry| entry.invoke.clone()))
                    .await;
                let answers = Answers {
                    reject: &*inner.reject,
                    internal: inner.operations.service.internal,
                };
                let protocol = inner.handlers[protocol]
                    .as_protocol()
                    .expect("indexed as a protocol");
                let mut response = match job.outcome.take() {
                    Some(Outcome::Answered(response)) => response,
                    outcome => {
                        job.outcome = outcome;
                        protocol.write(&job, &answers)
                    }
                };
                for (name, value) in job.response_headers.drain() {
                    if let Some(name) = name {
                        response.headers_mut().insert(name, value);
                    }
                }
                // An operation that models its own `x-request-id` member keeps it.
                if !response.headers().contains_key("x-request-id") {
                    response
                        .headers_mut()
                        .insert("x-request-id", job.context.request_id_header().clone());
                }
                response
            })
            .await
    }

    /// `before` in order, the operation, then `after` in reverse for every
    /// handler whose `before` succeeded. A request refused before the chain
    /// starts (an oversized body) runs no `before` but every `after`, so the
    /// service log and metrics still see it.
    async fn run(&self, job: &mut Job, invoke: Option<Invoke>) {
        let handlers = &self.inner.handlers;
        let refused_early = job.outcome.is_some();
        let mut ran = if refused_early { handlers.len() } else { 0 };
        if !refused_early {
            for handler in handlers {
                if let Err(error) = handler.before(job).resolve().await {
                    job.outcome = Some(Outcome::Failed(error));
                    break;
                }
                ran += 1;
                // A handler that answered ends the `before` hooks; its own
                // `after` still runs.
                if job.outcome.is_some() {
                    break;
                }
            }
        }
        if job.outcome.is_none() {
            if job.wants_snapshot {
                job.input_snapshot = job.input.as_ref().map(|input| input.to_json());
            }
            job.outcome = Some(match (job.input.take(), invoke) {
                (Some(input), Some(invoke)) => match invoke(input).await {
                    Ok(output) => Outcome::Output(output),
                    Err(error) => Outcome::Failed(error),
                },
                (None, Some(_)) => Outcome::Failed(Error::internal(
                    "no protocol handler read the input: put one in the chain",
                )),
                (_, None) => Outcome::Failed(FrameworkError::UnknownOperation.into()),
            });
        }
        for handler in handlers[..ran].iter().rev() {
            handler.after(job).resolve().await;
        }
    }
}

pub struct Builder {
    operations: Operations,
    handlers: Vec<Box<dyn Handler>>,
    layers: Vec<Wrap>,
    reject: Option<Reject>,
    body_limit: usize,
    body_limits: Vec<BodyLimit>,
    request_id: Option<RequestIdSource>,
}

type BodyLimit = Box<dyn Fn(&OperationInfo) -> Option<usize> + Send + Sync>;

impl Builder {
    /// Appends a handler to the chain, inside every handler added before it:
    /// the first added is outermost. `before` hooks run in the order added,
    /// `after` hooks in reverse.
    pub fn handler(mut self, handler: impl Handler) -> Self {
        self.handlers.push(Box::new(handler));
        self
    }

    /// Appends a bundle of handlers, in order: a chain a library ships.
    pub fn handlers(mut self, handlers: impl IntoIterator<Item = Box<dyn Handler>>) -> Self {
        self.handlers.extend(handlers);
        self
    }

    /// Appends the default handlers: the service log (errors only), the
    /// exception translator (no translations: everything unmodelled is a 500)
    /// and the JSON protocol.
    pub fn defaults(self) -> Self {
        self.handler(ServiceLog::errors_only())
            .handler(ExceptionTranslator::new())
            .handler(JsonProtocol)
            .handler(InvokeProtocol)
    }

    /// Wraps the whole service in a tower layer, outside the handler chain:
    /// for work on the response as sent, such as compression or counting
    /// the bytes that leave. Like handlers, the first added is outermost.
    /// Every host runs the layers: HTTP, Lambda HTTP events and direct
    /// invokes alike, so swapping hosts keeps them. A layer's errors must be
    /// answered inside it (tower's `HandleErrorLayer`); the response body may
    /// be any HTTP body.
    pub fn layer<L, S, B>(mut self, layer: L) -> Self
    where
        L: Layer<Endpoint, Service = S> + Send + Sync + 'static,
        S: Service<Request<Body>, Response = axum::http::Response<B>, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
        B: HttpBody<Data = Bytes> + Send + 'static,
        B::Error: Into<axum::BoxError>,
    {
        self.layers.push(Box::new(move |inner: Endpoint| {
            BoxCloneSyncService::new(
                layer
                    .layer(inner)
                    .map_response(|response: axum::http::Response<B>| response.map(Body::new)),
            )
        }));
        self
    }

    /// How a rejected request becomes the service's validation error. Needed
    /// when the generator could not fill that error itself.
    pub fn on_rejection(
        mut self,
        reject: impl Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync + 'static,
    ) -> Self {
        self.reject = Some(Arc::new(reject));
        self
    }

    /// Where a request's id comes from, before the host's id (the Lambda
    /// invocation's) and a fresh one. Ids empty or over 128 bytes are
    /// replaced. Trust an inbound header only behind an edge that sets it.
    pub fn request_id(mut self, source: RequestIdSource) -> Self {
        self.request_id = Some(source);
        self
    }

    /// The largest request body any operation reads; larger ones are refused
    /// unread as a validation error.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    /// A body limit for one operation, overriding [`Builder::body_limit`].
    pub fn operation_body_limit<O: Operation>(self, bytes: usize) -> Self {
        let id = O::INFO.id;
        self.body_limits(move |op| (op.id == id).then_some(bytes))
    }

    /// Body limits chosen per operation, for example from a trait in the
    /// model: `limits(op)` is asked once per operation at `build`, and
    /// `None` keeps the service-wide limit. Later calls win.
    pub fn body_limits(
        mut self,
        limits: impl Fn(&OperationInfo) -> Option<usize> + Send + Sync + 'static,
    ) -> Self {
        self.body_limits.push(Box::new(limits));
        self
    }

    pub fn build(mut self) -> Result<Crucible, BuildError> {
        let prepare = Prepare::new(self.operations.service);
        for handler in &mut self.handlers {
            handler.prepare(&prepare).map_err(BuildError::Prepare)?;
        }
        let protocols: Vec<usize> = self
            .handlers
            .iter()
            .enumerate()
            .filter(|(_, handler)| handler.as_protocol().is_some())
            .map(|(index, _)| index)
            .collect();
        if protocols.is_empty() {
            return Err(BuildError::NoProtocol);
        }
        let reject = match (self.reject, self.operations.service.reject) {
            (Some(reject), _) => reject,
            (None, Some(generated)) => Arc::new(generated) as Reject,
            (None, None) => return Err(BuildError::NoRejection),
        };
        let body_limits = self
            .operations
            .service
            .operations
            .iter()
            .filter_map(|op| {
                let limit = self
                    .body_limits
                    .iter()
                    .rev()
                    .find_map(|limits| limits(op))?;
                Some((op.id, limit))
            })
            .collect();
        let core = Crucible {
            inner: Arc::new(Inner {
                operations: self.operations,
                handlers: self.handlers,
                protocols,
                reject,
                body_limit: self.body_limit,
                body_limits,
                request_id: self.request_id,
            }),
            endpoint: None,
        };
        if self.layers.is_empty() {
            return Ok(core);
        }
        let chain = core.clone();
        let mut endpoint: Endpoint =
            BoxCloneSyncService::new(tower::service_fn(move |request: Request<Body>| {
                let chain = chain.clone();
                async move { Ok::<_, Infallible>(chain.dispatch(request).await) }
            }));
        // The last added wraps the chain first, so the first added is outermost.
        for wrap in self.layers.into_iter().rev() {
            endpoint = wrap(endpoint);
        }
        Ok(Crucible {
            endpoint: Some(endpoint),
            ..core
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// No handler in the chain is a protocol, so nothing can route.
    NoProtocol,
    /// The service's validation error needs members crucible cannot fill:
    /// give the builder `on_rejection`.
    NoRejection,
    /// A handler's `prepare` refused the service, saying why.
    Prepare(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoProtocol => "the handler chain has no protocol handler",
            Self::NoRejection => {
                "the validation error has required members crucible cannot fill: call on_rejection"
            }
            Self::Prepare(reason) => reason,
        })
    }
}

impl std::error::Error for BuildError {}
