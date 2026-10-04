//! The handler chain. Each request is a `Job` passed down the chain's
//! `before` hooks in order, to the operation, then back through the `after`
//! hooks in reverse.

use std::pin::Pin;
use std::sync::Arc;

use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::Value;

use crate::context::Context;
use crate::error::{Error, FrameworkError, ModelledError, Result};
use crate::operation::{Input, OperationInfo, Output, ServiceInfo};
use crate::protocol::Protocol;
use crate::validate::Rejection;

/// What a hook gives back: its result now, or a future to wait for. A hook
/// that needs no waiting returns `Ready` and costs no allocation; one that
/// does returns `Hook::wait(async move { .. })`.
pub enum Hook<'a, T> {
    Ready(T),
    Wait(Pin<Box<dyn Future<Output = T> + Send + 'a>>),
}

impl<'a, T> Hook<'a, T> {
    pub fn wait(future: impl Future<Output = T> + Send + 'a) -> Self {
        Self::Wait(Box::pin(future))
    }

    /// The result, waiting for it if it is not ready.
    pub async fn resolve(self) -> T {
        match self {
            Self::Ready(value) => value,
            Self::Wait(future) => future.await,
        }
    }
}

/// A step in every request. Both hooks have defaults, so a handler writes
/// only the one it needs. A hook that awaits something returns
/// [`Hook::wait`]; for writing hooks as `async fn`, implement
/// [`AsyncHandler`] instead.
pub trait Handler: Send + Sync + 'static {
    /// Runs in chain order before the operation. An error stops the chain:
    /// the operation does not run, and it becomes the request's outcome.
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        let _ = job;
        Hook::Ready(Ok(()))
    }

    /// Runs in reverse chain order after the operation, or after a `before`
    /// failed, for every handler whose `before` succeeded.
    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        let _ = job;
        Hook::Ready(())
    }

    /// A protocol handler routes requests and writes responses too.
    fn as_protocol(&self) -> Option<&dyn Protocol> {
        None
    }

    /// Runs once, at `build`: where a handler reads the model's traits into
    /// its own per-operation table, rather than parsing them on every
    /// request. [`Prepare::operations`] yields the operations the handler
    /// runs for (all of them, unless it is scoped), [`Prepare::service`] the
    /// whole service. An error fails the build, so a missing or malformed
    /// trait is found before the first request.
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}

/// The service as a handler sees it at `build`: the whole of it, and the
/// operations the handler's scope includes.
#[derive(Clone, Copy)]
pub struct Prepare<'a> {
    service: &'static ServiceInfo,
    scope: Option<&'a (dyn Fn(&OperationInfo) -> bool + 'a)>,
}

impl<'a> Prepare<'a> {
    pub(crate) fn new(service: &'static ServiceInfo) -> Self {
        Self {
            service,
            scope: None,
        }
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    /// Whether the handler runs for `operation`.
    pub fn applies(&self, operation: &OperationInfo) -> bool {
        self.scope.is_none_or(|scope| scope(operation))
    }

    /// The operations the handler runs for.
    pub fn operations(&self) -> impl Iterator<Item = &'static OperationInfo> + '_ {
        self.service
            .operations
            .iter()
            .copied()
            .filter(|op| self.applies(op))
    }

    /// The same service, narrowed to the operations `scope` accepts too: for
    /// a handler that wraps another and runs it for fewer operations.
    pub fn within<'b>(&self, scope: &'b (dyn Fn(&OperationInfo) -> bool + 'b)) -> Prepare<'b> {
        Prepare {
            service: self.service,
            scope: Some(scope),
        }
    }
}

/// A handler written with `async fn` hooks: every type implementing it is a
/// [`Handler`]. Each call allocates its future, which a handler that never
/// waits avoids by implementing `Handler` directly.
#[async_trait::async_trait]
pub trait AsyncHandler: Send + Sync + 'static {
    async fn before(&self, job: &mut Job) -> Result<()> {
        let _ = job;
        Ok(())
    }

    async fn after(&self, job: &mut Job) {
        let _ = job;
    }

    /// As [`Handler::prepare`].
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}

impl<T: AsyncHandler> Handler for T {
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        AsyncHandler::prepare(self, prepare)
    }

    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Wait(AsyncHandler::before(self, job))
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        Hook::Wait(AsyncHandler::after(self, job))
    }
}

/// What an operation produced, or how the request failed.
pub enum Outcome {
    Output(Box<dyn Output>),
    Failed(Error),
    /// A handler answered with a response of its own ([`Job::respond`]):
    /// the operation did not run, and the protocol does not rewrite it.
    Answered(Response),
}

/// How an outcome is answered, decided once for the protocol, the service
/// log and metrics alike.
pub enum Resolution<'a> {
    Success(&'a dyn Output),
    /// A modelled error the operation declares.
    Modelled(&'a dyn ModelledError),
    /// A request that broke the model: answered as the validation error.
    Rejected(&'a Rejection),
    /// Answered by the framework itself (an unknown operation).
    Framework(FrameworkError),
    /// Anything else: logged, and answered with no detail.
    Internal(&'a Error),
    /// A handler answered with its own response, sent as it is.
    Answered {
        status: u16,
    },
}

impl Resolution<'_> {
    /// The response status this resolution gets.
    pub fn status(&self, operation: Option<&OperationInfo>) -> u16 {
        match self {
            Self::Success(_) => operation.map_or(200, |op| op.status),
            Self::Modelled(error) => error.info().status,
            Self::Rejected(_) => 400,
            Self::Framework(error) => error.status(),
            Self::Internal(_) => 500,
            Self::Answered { status } => *status,
        }
    }

    /// `success`, `error` (the caller's) or `fault` (the service's).
    pub fn class(&self, operation: Option<&OperationInfo>) -> &'static str {
        match self.status(operation) {
            0..400 => "success",
            400..500 => "error",
            _ => "fault",
        }
    }

    /// The error's wire name, when the request failed.
    pub fn error_name(&self) -> Option<&'static str> {
        match self {
            Self::Success(_) | Self::Answered { .. } => None,
            Self::Modelled(error) => Some(error.info().name),
            Self::Rejected(_) => Some("ValidationError"),
            Self::Framework(error) => Some(error.name()),
            Self::Internal(_) => Some("InternalFailure"),
        }
    }
}

pub(crate) type Decode = fn(Value) -> std::result::Result<Box<dyn Input>, Rejection>;
pub(crate) type DecodeJson = fn(&[u8]) -> std::result::Result<Box<dyn Input>, Rejection>;

pub struct Job {
    pub(crate) service: &'static ServiceInfo,
    pub(crate) context: Arc<Context>,
    pub(crate) request: Arc<Parts>,
    pub(crate) body: Bytes,
    pub(crate) labels: Vec<(&'static str, String)>,
    pub(crate) decode: Option<Decode>,
    pub(crate) decode_json: Option<DecodeJson>,
    pub(crate) decode_bound: Option<crate::operations::DecodeBound>,
    pub(crate) input: Option<Box<dyn Input>>,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) response_headers: HeaderMap,
    pub(crate) wants_snapshot: bool,
    /// The input as JSON, kept for `after` hooks when one asked for it.
    pub(crate) input_snapshot: Option<Value>,
}

impl Job {
    /// The operation serving the request; `None` when no protocol claimed it.
    pub fn operation(&self) -> Option<&'static OperationInfo> {
        self.context.operation()
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    pub fn context(&self) -> &Arc<Context> {
        &self.context
    }

    pub fn request(&self) -> &Parts {
        &self.request
    }

    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// URI labels the routing protocol matched, percent-decoded.
    pub fn labels(&self) -> &[(&'static str, String)] {
        &self.labels
    }

    /// The validated input, once the protocol handler has read it.
    pub fn input(&self) -> Option<&dyn Input> {
        self.input.as_deref()
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// Replaces a failure, as an exception translator does. Ignored when the
    /// operation succeeded.
    pub fn replace_error(&mut self, error: Error) {
        if matches!(self.outcome, Some(Outcome::Failed(_))) {
            self.outcome = Some(Outcome::Failed(error));
        }
    }

    /// Answers the request with `response` as it is: the remaining `before`
    /// hooks, decoding and the operation are skipped, `after` hooks run (this
    /// handler's included) and see [`Resolution::Answered`], and the protocol
    /// does not rewrite it. For a response from elsewhere: relaying a request
    /// forwarded to another region, or a cached answer. Headers from
    /// [`Job::response_headers_mut`] are still added, and `x-request-id`
    /// when the response has none.
    pub fn respond(&mut self, response: Response) {
        self.outcome = Some(Outcome::Answered(response));
    }

    /// Headers added to the response, whatever its outcome.
    pub fn response_headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.response_headers
    }

    /// Reads a JSON value as the operation's input, and validates it. For
    /// protocol handlers.
    pub fn decode(&self, value: Value) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode {
            Some(decode) => decode(value),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Reads JSON bytes straight into the operation's input, in one pass, and
    /// validates it: for an input that is the whole body. For protocol
    /// handlers.
    pub fn decode_json(&self, bytes: &[u8]) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_json {
            Some(decode) => decode(bytes),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Reads the operation's input from its bound members (already checked
    /// against their kinds) and its JSON body in one pass, and validates it.
    /// For protocol handlers.
    pub fn decode_bound<'a>(
        &self,
        body: &'a [u8],
        bound: Vec<(&'static str, crate::protocol::BoundValue<'a>)>,
    ) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_bound {
            Some(decode) => decode(body, bound),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    pub fn set_input(&mut self, input: Box<dyn Input>) {
        self.input = Some(input);
    }

    /// Keeps a JSON copy of the input for `after` hooks: the operation
    /// consumes the input itself. Call it from `before`.
    pub fn snapshot_input(&mut self) {
        self.wants_snapshot = true;
    }

    /// The input as JSON, if a handler asked for a snapshot.
    pub fn input_snapshot(&self) -> Option<&Value> {
        self.input_snapshot.as_ref()
    }

    /// How the outcome is answered. `None` before there is one.
    pub fn resolution(&self) -> Option<Resolution<'_>> {
        Some(match self.outcome.as_ref()? {
            Outcome::Output(output) => Resolution::Success(output.as_ref()),
            Outcome::Failed(error) => self.resolve_error(error),
            Outcome::Answered(response) => Resolution::Answered {
                status: response.status().as_u16(),
            },
        })
    }

    fn resolve_error<'a>(&self, error: &'a Error) -> Resolution<'a> {
        if let Some(rejection) = error.downcast_ref::<Rejection>() {
            return Resolution::Rejected(rejection);
        }
        if let Some(framework) = error.downcast_ref::<FrameworkError>() {
            return match framework {
                FrameworkError::UnknownOperation => Resolution::Framework(framework.clone()),
                FrameworkError::InternalFailure => Resolution::Internal(error),
            };
        }
        match (self.service.classify)(error.get_ref()) {
            Some(modelled)
                if self
                    .operation()
                    .is_some_and(|op| op.declares(modelled.info().name)) =>
            {
                Resolution::Modelled(modelled)
            }
            _ => Resolution::Internal(error),
        }
    }
}
