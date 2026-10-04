//! The orchestrator: routes a request, runs the handler chain around the
//! operation, and has the routing protocol write the response.

use std::fmt;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderValue, Request};
use axum::response::Response;

use crate::context::Context;
use crate::error::{Error, FrameworkError, ModelledError};
use crate::handler::{Handler, Job, Outcome};
use crate::handlers::{ExceptionTranslator, ServiceLog};
use crate::operations::{Invoke, Operations};
use crate::protocol::{Answers, Invocation, InvokeProtocol, JsonProtocol, Route};
use crate::validate::Rejection;

type Reject = Arc<dyn Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync>;

/// Bodies above this are refused unread, unless the builder says otherwise.
pub const DEFAULT_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// A service: its operations, its handler chain, and the protocols in it.
#[derive(Clone)]
pub struct Crucible {
    inner: Arc<Inner>,
}

struct Inner {
    operations: Operations,
    handlers: Vec<Box<dyn Handler>>,
    /// Indexes into `handlers` of the protocol handlers, in chain order.
    protocols: Vec<usize>,
    reject: Reject,
    body_limit: usize,
}

impl Crucible {
    pub fn builder(operations: Operations) -> Builder {
        Builder {
            operations,
            handlers: Vec::new(),
            reject: None,
            body_limit: DEFAULT_BODY_LIMIT,
        }
    }

    /// An axum router that hands every request to the service; HTTP layers
    /// (compression, CORS, timeouts) wrap it as usual.
    pub fn into_router(self) -> axum::Router {
        axum::Router::new().fallback(move |request: Request<Body>| {
            let service = self.clone();
            async move { service.handle(request).await }
        })
    }

    /// Calls an operation by name with its input as JSON, through the whole
    /// handler chain, without HTTP: for direct Lambda invokes and in-process
    /// callers. Needs an [`InvokeProtocol`] in the chain.
    pub async fn invoke(&self, operation: &str, input: serde_json::Value) -> serde_json::Value {
        let mut request = Request::post("/")
            .body(Body::from(input.to_string()))
            .expect("a POST to / is a valid request");
        request.extensions_mut().insert(Invocation {
            operation: operation.to_owned(),
        });
        let response = self.handle(request).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            serde_json::json!({ "error": { "__type": "InternalFailure", "message": "no protocol answered the invocation" } })
        })
    }

    pub async fn handle(&self, request: Request<Body>) -> Response {
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
        let context = Arc::new(Context::new(
            uuid::Uuid::new_v4().to_string(),
            entry.map(|entry| entry.info),
            parts.headers.clone(),
        ));

        let body = axum::body::to_bytes(body, inner.body_limit).await;
        let mut job = Job {
            service: inner.operations.service,
            context: context.clone(),
            request: parts,
            body: body.as_ref().cloned().unwrap_or_default(),
            labels: route.map(|Route { labels, .. }| labels).unwrap_or_default(),
            decode: entry.map(|entry| entry.decode),
            input: None,
            outcome: None,
            response_headers: Default::default(),
            wants_snapshot: false,
            input_snapshot: None,
        };
        if body.is_err() {
            let limit = inner.body_limit;
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
                let mut response = protocol.write(&job, &answers);
                for (name, value) in job.response_headers.drain() {
                    if let Some(name) = name {
                        response.headers_mut().insert(name, value);
                    }
                }
                // An operation that models its own `x-request-id` member keeps it.
                if !response.headers().contains_key("x-request-id") {
                    if let Ok(id) = HeaderValue::from_str(job.context.request_id()) {
                        response.headers_mut().insert("x-request-id", id);
                    }
                }
                response
            })
            .await
    }

    /// `before` in order, the operation, then `after` in reverse for every
    /// handler whose `before` succeeded.
    async fn run(&self, job: &mut Job, invoke: Option<Invoke>) {
        let handlers = &self.inner.handlers;
        let mut ran = 0;
        if job.outcome.is_none() {
            for handler in handlers {
                if let Err(error) = handler.before(job).await {
                    job.outcome = Some(Outcome::Failed(error));
                    break;
                }
                ran += 1;
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
            handler.after(job).await;
        }
    }
}

pub struct Builder {
    operations: Operations,
    handlers: Vec<Box<dyn Handler>>,
    reject: Option<Reject>,
    body_limit: usize,
}

impl Builder {
    /// Appends a handler to the chain. Order is execution order for
    /// `before`, and the reverse for `after`.
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

    /// How a rejected request becomes the service's validation error. Needed
    /// when the generator could not fill that error itself.
    pub fn on_rejection(
        mut self,
        reject: impl Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync + 'static,
    ) -> Self {
        self.reject = Some(Arc::new(reject));
        self
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    pub fn build(self) -> Result<Crucible, BuildError> {
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
        Ok(Crucible {
            inner: Arc::new(Inner {
                operations: self.operations,
                handlers: self.handlers,
                protocols,
                reject,
                body_limit: self.body_limit,
            }),
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
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoProtocol => "the handler chain has no protocol handler",
            Self::NoRejection => {
                "the validation error has required members crucible cannot fill: call on_rejection"
            }
        })
    }
}

impl std::error::Error for BuildError {}
