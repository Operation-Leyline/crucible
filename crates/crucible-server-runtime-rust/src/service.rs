//! The orchestrator: routes a request, runs the phased pipeline around the
//! operation, and has the routing protocol write the response.

use std::convert::Infallible;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request};
use axum::response::Response;
use futures_util::FutureExt;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

use crate::context::{Context, RequestMeta};
use crate::error::{Error, FrameworkError, ModelledError};
use crate::middlewares::ServiceLog;
use crate::operation::{Operation, OperationInfo};
use crate::operations::Operations;
use crate::permissions::Requirements;
use crate::protocol::{Answers, Invocation, InvokeProtocol, JsonProtocol, Protocol, ProtocolMatch};
use crate::validate::Rejection;
use crate::versioning::{self, Unversioned};
use crate::{
    DecodedJob, ExceptionTranslator, Interceptor, Job, Middleware, OperationAwareMiddleware,
    OperationJob, Outcome, Prepare,
};

/// The invocation answer when no protocol answered with JSON.
pub(crate) const INVOKE_FAILURE: &str =
    r#"{"error":{"__type":"InternalFailure","message":"no protocol answered the invocation"}}"#;

type Reject = Arc<dyn Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync>;

/// The service as a tower service: what a [`Builder::tower_layer`] wraps.
pub type Endpoint = BoxCloneSyncService<Request<Body>, Response, Infallible>;

type Wrap = Box<dyn FnOnce(Endpoint) -> Endpoint + Send + Sync>;

/// Picks a request's id from the request: an inbound header a trusted edge
/// sets, say. `None` falls back to the host's id, then a fresh one.
pub type RequestIdSource = fn(&axum::http::request::Parts) -> Option<HeaderValue>;

/// The longest request id taken from outside; a longer one is replaced.
const MAX_REQUEST_ID: usize = 128;

fn panic_error(payload: Box<dyn std::any::Any + Send>) -> Error {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload");
    Error::internal(format!("request panicked: {message}"))
}

fn recover<T>(call: impl FnOnce() -> T) -> crate::Result<T> {
    catch_unwind(AssertUnwindSafe(call)).map_err(panic_error)
}

async fn recover_async<T>(future: impl Future<Output = T>) -> crate::Result<T> {
    AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .map_err(panic_error)
}

/// A random UUID (v4) as a header value, formatted once.
fn request_id() -> HeaderValue {
    let mut text = [0u8; uuid::fmt::Hyphenated::LENGTH];
    let text = uuid::Uuid::new_v4().hyphenated().encode_lower(&mut text);
    HeaderValue::from_str(text).expect("a UUID is visible ASCII")
}

/// Buffered bodies above this are refused during reading, unless the builder says otherwise.
pub const DEFAULT_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// A service: its operations, its phased pipeline, and the protocols in it.
#[derive(Clone)]
pub struct Crucible {
    inner: Arc<Inner>,
    /// The layered service, when the builder added layers; every entry point
    /// goes through it. Without layers requests go straight to the chain.
    endpoint: Option<Endpoint>,
}

struct Inner {
    operations: Operations,
    middleware: Vec<Box<dyn Middleware>>,
    operation_middleware: Vec<Box<dyn OperationAwareMiddleware>>,
    interceptors: Vec<Box<dyn Interceptor>>,
    protocols: Vec<Box<dyn Protocol>>,
    fallback: usize,
    translator: ExceptionTranslator,
    reject: Reject,
    body_limit: usize,
    request_timeout: Option<Duration>,
    request_id: Option<RequestIdSource>,
    unversioned: Unversioned,
    /// Whether the current release has a sender-signed operation (decision
    /// 81), settled at build: only then can a request without a release be
    /// one, served at the current release.
    signed: bool,
}

impl Crucible {
    pub fn builder(operations: Operations) -> Builder {
        Builder {
            operations,
            middleware: Vec::new(),
            operation_middleware: Vec::new(),
            interceptors: Vec::new(),
            protocols: Vec::new(),
            translator: ExceptionTranslator::new(),
            layers: Vec::new(),
            reject: None,
            body_limit: DEFAULT_BODY_LIMIT,
            body_limits: Vec::new(),
            request_timeout: None,
            request_id: None,
            unversioned: Unversioned::default(),
        }
    }

    /// An axum router that hands every request to the service, through its
    /// layers. Layers added here instead apply on this host only; prefer
    /// [`Builder::tower_layer`], which every host runs.
    pub fn into_router(self) -> axum::Router {
        axum::Router::new().fallback_service(self)
    }

    /// Calls an operation by name with its input as JSON bytes, through the
    /// whole phased pipeline, without HTTP, and answers `{"output": ..}` or
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

    /// Answers one request: through the layers, then the phased pipeline.
    pub async fn handle(&self, request: Request<Body>) -> Response {
        match &self.endpoint {
            Some(endpoint) => match endpoint.clone().oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            },
            None => self.dispatch(request).await,
        }
    }

    async fn dispatch(&self, request: Request<Body>) -> Response {
        let inner = &self.inner;
        let (parts, body) = request.into_parts();
        let meta = parts.extensions.get::<RequestMeta>();
        let id = inner
            .request_id
            .and_then(|source| source(&parts))
            .or_else(|| meta.and_then(|meta| meta.request_id.clone()))
            .filter(|id| !id.is_empty() && id.len() <= MAX_REQUEST_ID && id.to_str().is_ok())
            .unwrap_or_else(request_id);
        let host_deadline = meta.and_then(|meta| meta.deadline);
        let context = Arc::new(Context::new(id, None, parts));
        // The deadline the framework enforces: the host's, less the time the
        // after phases and the response need, or the service's own budget,
        // whichever comes first. The context holds it, so an extension or
        // an implementation budgets against the moment it is stopped.
        let deadline = host_deadline
            .map(|deadline| {
                deadline
                    .checked_sub(crate::DEADLINE_RESERVE)
                    .unwrap_or(deadline)
            })
            .into_iter()
            // A budget too long for an `Instant` is no budget.
            .chain(
                inner
                    .request_timeout
                    .and_then(|timeout| context.received().checked_add(timeout)),
            )
            .min();
        if let Some(deadline) = deadline {
            context.set_deadline(deadline);
        }
        let mut job = Job {
            service: inner.operations.service,
            context: context.clone(),
            body: Bytes::new(),
            raw_body: Some(body),
            body_limit: inner.body_limit,
            body_read: crate::job::BodyRead::Pending,
            labels: Vec::new(),
            decode: None,
            decode_json: None,
            decode_bound: None,
            output_from_json: None,
            input: None,
            outcome: None,
            downgrade: None,
            downgrade_error: None,
            errors: None,
            error_revision: 0,
            response_headers: Default::default(),
            wants_snapshot: false,
            input_snapshot: None,
            designated_not_found: false,
        };
        context
            .scope(async move {
                let mut entered = Entered::default();
                // An early transport failure still uses its owner's error envelope.
                let mut selected = inner.fallback;
                // Keep the job and entered phases outside the unwind boundary so
                // a panic follows the same return path as an internal error.
                if let Err(error) = recover_async(async {
                    selected = inner
                        .protocols
                        .iter()
                        .position(|p| p.owns(job.request()))
                        .unwrap_or(inner.fallback);
                    let execute = self.execute(&mut job, &mut entered, &mut selected);
                    // Past the deadline the phase running is dropped, as a
                    // panic would end it: the phases entered still unwind.
                    let expired = match deadline {
                        None => {
                            execute.await;
                            false
                        }
                        Some(deadline) => {
                            tokio::select! {
                                biased;
                                () = execute => false,
                                () = tokio::time::sleep_until(deadline.into()) => true,
                            }
                        }
                    };
                    if expired {
                        let elapsed = job.context().received().elapsed();
                        job.fail(Error::new(crate::DeadlineExceeded::new(elapsed)));
                    }
                    // Decoded input is still present when an interceptor refuses or answers.
                    if entered.decoded && job.wants_snapshot && job.input_snapshot.is_none() {
                        job.input_snapshot = job.input.as_ref().map(|input| input.to_json());
                    }
                })
                .await
                {
                    job.fail(error);
                }
                let mut translated = None;
                self.translate(&mut job, &mut translated);
                for interceptor in inner.interceptors[..entered.interceptors].iter().rev() {
                    if let Err(error) = recover_async(async {
                        interceptor
                            .after(&mut OperationJob::new(&mut job))
                            .resolve()
                            .await;
                    })
                    .await
                    {
                        job.fail(error);
                    }
                    self.translate(&mut job, &mut translated);
                }
                for middleware in inner.operation_middleware[..entered.operation].iter().rev() {
                    if let Err(error) = recover_async(async {
                        middleware
                            .after(&mut OperationJob::new(&mut job))
                            .resolve()
                            .await;
                    })
                    .await
                    {
                        job.fail(error);
                    }
                    self.translate(&mut job, &mut translated);
                }
                self.translate(&mut job, &mut translated);
                let answers = Answers {
                    reject: &*inner.reject,
                    internal: inner.operations.service.internal,
                    unknown: inner.operations.service.unknown,
                };
                // An older version's client gets its version's output or
                // error. The conversion may call the service's hooks, so a
                // panic in one fails the request as any handler's would.
                if let Some(downgrade) = job.downgrade.clone()
                    && let Some(Outcome::Output(_)) = &job.outcome
                {
                    let Some(Outcome::Output(output)) = job.outcome.take() else {
                        unreachable!("checked above");
                    };
                    let mut converted = None;
                    let recovered = recover_async(async {
                        converted = Some(downgrade(output).await);
                    })
                    .await;
                    job.outcome = Some(match (recovered, converted) {
                        (Ok(()), Some(Ok(output))) => Outcome::Output(output),
                        (Ok(()), Some(Err(error))) | (Err(error), _) => Outcome::Failed(error),
                        (Ok(()), None) => Outcome::Failed(Error::internal(
                            "the output's conversion to its API version did not finish",
                        )),
                    });
                }
                // A hook's failure meets the exception rules, as a handler's
                // does; the after phases have already seen the current
                // version's outcome.
                self.translate(&mut job, &mut translated);
                crate::permissions::answer_not_found(&mut job);
                job.designated_not_found = matches!(
                    job.resolution(),
                    Some(crate::Resolution::Modelled(error))
                        if job.service.not_found == Some(error.shape_id())
                );
                job.outcome = match job.outcome.take() {
                    Some(Outcome::Failed(error)) => {
                        Some(Outcome::Failed(match &job.downgrade_error {
                            Some(downgrade) => downgrade(error),
                            None => error,
                        }))
                    }
                    other => other,
                };
                let mut response = match job.outcome.take() {
                    Some(Outcome::Answered(response)) => response,
                    outcome => {
                        job.outcome = outcome;
                        self.write(&mut job, selected, &answers)
                    }
                };
                Self::response_headers(&mut job, &mut response);
                for middleware in inner.middleware[..entered.transport].iter().rev() {
                    if let Err(error) = recover_async(async {
                        middleware.after(&mut job, &mut response).resolve().await;
                    })
                    .await
                    {
                        job.fail(error);
                        response = self.write(&mut job, selected, &answers);
                        Self::response_headers(&mut job, &mut response);
                    }
                }
                // HEAD runs the normal pipeline, but never sends a response body,
                // including one supplied or transformed by transport middleware.
                if job.request().method == "HEAD"
                    && job.request().extensions.get::<Invocation>().is_none()
                {
                    // Preserve the representation's length before discarding its bytes.
                    if !response.headers().contains_key("content-length")
                        && !matches!(response.status().as_u16(), 100..=199 | 204 | 304)
                        && let Some(length) = response.body().size_hint().exact()
                    {
                        response
                            .headers_mut()
                            .insert("content-length", HeaderValue::from(length));
                    }
                    *response.body_mut() = Body::empty();
                }
                if entered.operation > 0 || entered.interceptors > 0 {
                    for interceptor in inner.interceptors[..entered.interceptors].iter().rev() {
                        if let Err(error) = recover(|| {
                            interceptor.complete(&OperationJob::new(&mut job), &response);
                        }) {
                            job.fail(error);
                            response = self.write(&mut job, selected, &answers);
                            Self::response_headers(&mut job, &mut response);
                        }
                    }
                    for middleware in inner.operation_middleware[..entered.operation].iter().rev() {
                        if let Err(error) = recover(|| {
                            middleware.complete(&OperationJob::new(&mut job), &response);
                        }) {
                            job.fail(error);
                            response = self.write(&mut job, selected, &answers);
                            Self::response_headers(&mut job, &mut response);
                        }
                    }
                }
                for middleware in inner.middleware[..entered.transport].iter().rev() {
                    if let Err(error) = recover(|| middleware.complete(&job, &response)) {
                        job.fail(error);
                        response = self.write(&mut job, selected, &answers);
                        Self::response_headers(&mut job, &mut response);
                    }
                }
                // A failing completion hook can replace the response with an error.
                if job.request().method == "HEAD"
                    && job.request().extensions.get::<Invocation>().is_none()
                {
                    *response.body_mut() = Body::empty();
                }
                if let Err(error) =
                    recover(|| inner.protocols[selected].decorate_response(&mut response))
                {
                    job.fail(error);
                    response = self.write(&mut job, selected, &answers);
                    Self::response_headers(&mut job, &mut response);
                    if job.request().method == "HEAD" {
                        *response.body_mut() = Body::empty();
                    }
                }
                response
            })
            .await
    }

    fn response_headers(job: &mut Job, response: &mut Response) {
        response.headers_mut().extend(job.response_headers.drain());
        // Authentication is a typed refusal across protocols, including modeled factory substitutions.
        let refusal = match job.resolution() {
            Some(crate::Resolution::Framework(error)) => Some(error),
            _ => job.context().attribute::<FrameworkError>(),
        };
        if let Some(error @ (FrameworkError::Unauthenticated | FrameworkError::Forbidden)) = refusal
            && response.status().as_u16() == error.status()
            && job
                .context()
                .operation_metadata()
                .is_some_and(|metadata| metadata.bearer_auth)
        {
            response
                .headers_mut()
                .entry(axum::http::header::WWW_AUTHENTICATE)
                .or_insert(HeaderValue::from_static(
                    if error == FrameworkError::Forbidden {
                        "Bearer error=\"insufficient_scope\""
                    } else {
                        "Bearer"
                    },
                ));
        }
        // The service's own not-found error is answered as the framework's
        // NotFound is: never cached, whether a hidden resource's (built by
        // the not-found factory) or a genuinely missing one's (the handler's),
        // so the two cannot be told apart, and visibility changes with
        // grants.
        let designated = job.designated_not_found
            || matches!(
                job.resolution(),
                Some(crate::Resolution::Modelled(error))
                    if job.service().not_found == Some(error.shape_id())
            );
        if designated {
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
        }
        if !response.headers().contains_key("x-request-id") {
            response
                .headers_mut()
                .insert("x-request-id", job.context.request_id_header().clone());
        }
    }

    fn write(&self, job: &mut Job, selected: usize, answers: &Answers<'_>) -> Response {
        let protocol = &self.inner.protocols[selected];
        match recover(|| protocol.write(job, answers)) {
            Ok(response) => response,
            Err(error) => {
                job.fail(error);
                // Encoding an output may panic while encoding an internal error
                // still works. A broken error writer must not escape recovery.
                recover(|| protocol.write(job, answers)).unwrap_or_else(|_| {
                    let fallback = Answers {
                        internal: None,
                        unknown: None,
                        reject: answers.reject,
                    };
                    if job.request().extensions.get::<Invocation>().is_some() {
                        InvokeProtocol.write(job, &fallback)
                    } else {
                        JsonProtocol.write(job, &fallback)
                    }
                })
            }
        }
    }

    /// Settles the request's API version and returns the operations that
    /// serve it: the current table, or an older version's. A `.cru`
    /// service without migrations has one release and refuses any other; a
    /// service that names no release skips this.
    fn version<'a>(&'a self, job: &mut Job) -> crate::Result<&'a Operations> {
        let operations = &self.inner.operations;
        let versions = &operations.versions;
        let Some(current) = versions.current else {
            return Ok(operations);
        };
        if self
            .inner
            .protocols
            .iter()
            .any(|protocol| protocol.current_version_only(job.request()))
        {
            job.context.select_api_version(current);
            job.response_headers
                .insert(versioning::VERSION_HEADER, current.header());
            return Ok(operations);
        }
        // `None` is the current version.
        let pick = |name: &str| -> crate::Result<Option<&'a crate::operations::OlderVersion>> {
            if name == current.name() {
                return Ok(None);
            }
            versions
                .older
                .iter()
                .find(|older| older.version.name() == name)
                .map(Some)
                .ok_or_else(|| versioning::unsupported(&versions.supported()).into())
        };
        let header = job
            .request()
            .headers
            .get(versioning::VERSION_HEADER)
            .map(|value| value.to_str().unwrap_or_default().trim());
        let older = match header {
            Some(name) => pick(name)?,
            // A third party's request names no release of this API and is
            // written for the one that declares its operation, the current
            // (decision 81), whatever the builder chose for clients.
            None if self.inner.signed
                && self.inner.protocols.iter().any(|protocol| {
                    protocol
                        .route(job.request(), operations)
                        .and_then(|route| operations.entry(route.operation))
                        .is_some_and(|entry| entry.info.sender.is_some())
                }) =>
            {
                None
            }
            None => match &self.inner.unversioned {
                // No version, and none chosen: the oldest supported.
                Unversioned::Oldest => versions.older.first(),
                Unversioned::Required => {
                    return Err(versioning::missing(&versions.supported()).into());
                }
                Unversioned::Resolve(resolver) => match resolver(job.request()) {
                    Some(name) => pick(&name)?,
                    None => versions.older.first(),
                },
            },
        };
        let (version, table) = match older {
            Some(older) => {
                if let Some(sunset) = &older.sunset {
                    job.response_headers.insert("sunset", sunset.clone());
                }
                (older.version, &older.operations)
            }
            None => (current, operations),
        };
        job.context.select_api_version(version);
        job.response_headers
            .insert(versioning::VERSION_HEADER, version.header());
        // A cache keeps one response per version.
        job.response_headers.append(
            axum::http::header::VARY,
            HeaderValue::from_static(versioning::VERSION_HEADER),
        );
        Ok(table)
    }

    fn translate(&self, job: &mut Job, translated: &mut Option<u64>) {
        if matches!(job.outcome, Some(Outcome::Failed(_)))
            && *translated != Some(job.error_revision)
        {
            if let Err(error) = recover(|| self.inner.translator.apply(job)) {
                job.fail(error);
            }
            *translated = Some(job.error_revision);
        }
    }

    async fn execute(&self, job: &mut Job, entered: &mut Entered, selected: &mut usize) {
        if let Err(error) = self.forward(job, entered, selected).await {
            job.fail(error);
        }
    }

    async fn forward(
        &self,
        job: &mut Job,
        entered: &mut Entered,
        selected: &mut usize,
    ) -> crate::Result<()> {
        let inner = &self.inner;
        if let Some(protocol) = inner
            .protocols
            .get(*selected)
            .filter(|p| p.owns(job.request()))
        {
            protocol.admit(job).resolve().await?;
            if job.outcome.is_some() {
                return Ok(());
            }
        }
        for middleware in &inner.middleware {
            middleware.before(job).resolve().await?;
            entered.transport += 1;
            if job.outcome.is_some() {
                return Ok(());
            }
        }
        job.input = None;
        *selected = inner.fallback;
        let operations = match self.version(job) {
            Ok(operations) => operations,
            Err(error) => {
                // Refused in the envelope of the protocol the request is for.
                if let Some(owner) = inner.protocols.iter().position(|p| p.owns(job.request())) {
                    *selected = owner;
                }
                return Err(error);
            }
        };
        let mut route = None;
        for (index, protocol) in inner.protocols.iter().enumerate() {
            match protocol.identify(job, operations).resolve().await {
                Ok(ProtocolMatch::Declined) => {}
                Ok(ProtocolMatch::Claimed(found)) => {
                    *selected = index;
                    route = found;
                    break;
                }
                Err(error) => {
                    *selected = index;
                    return Err(error);
                }
            }
        }
        if job.outcome.is_some() {
            return Ok(());
        }
        let route = route.ok_or(FrameworkError::UnknownOperation)?;
        let entry = operations
            .entry(route.operation)
            .ok_or_else(|| Error::internal("protocol selected an unregistered operation"))?;
        job.downgrade = entry.downgrade.clone();
        job.downgrade_error = entry.downgrade_error.clone();
        job.errors = entry.errors;
        job.context.select_operation(entry.info);
        job.context.select_metadata(entry.metadata);
        job.labels = route.labels;
        job.decode = Some(entry.decode);
        job.decode_json = Some(entry.decode_json);
        job.decode_bound = Some(entry.decode_bound);
        job.output_from_json = entry.output_from_json;
        let limit = entry.body_limit.unwrap_or(inner.body_limit);
        let buffered = job.select_body_limit(limit);
        for middleware in &inner.operation_middleware {
            middleware
                .before(&mut OperationJob::new(job))
                .resolve()
                .await?;
            entered.operation += 1;
            if job.outcome.is_some() {
                return Ok(());
            }
        }
        buffered?;
        if let Some(rejection) = route.rejection {
            return Err(rejection.into());
        }
        // Earlier transport extensions cannot supply an unvalidated input.
        job.input = None;
        job.read_body().await?;
        inner.protocols[*selected].decode(job).resolve().await?;
        if job.input.is_none() {
            return Err(Error::internal("protocol did not decode an input"));
        }
        // An older version's input, read as the current one: interceptors
        // and the implementation see only current types.
        if let Some(upgrade) = &entry.upgrade {
            let input = job.input.take().expect("decoded input checked");
            // Logged as the client sent it, which the operation's
            // redactions describe; kept even before anything asks, since an
            // interceptor may ask after the input became the current one's.
            job.input_snapshot = Some(input.to_json());
            job.input = Some(upgrade(input).await?);
        }
        entered.decoded = true;
        for interceptor in &inner.interceptors {
            interceptor
                .before(&mut DecodedJob::new(job))
                .resolve()
                .await?;
            entered.interceptors += 1;
            if job.outcome.is_some() {
                return Ok(());
            }
        }
        if job.wants_snapshot && entry.upgrade.is_none() {
            job.input_snapshot = job.input.as_ref().map(|input| input.to_json());
        }
        let input = job.input.take().expect("decoded input checked");
        job.outcome = Some(match (entry.invoke)(input).await {
            Ok(output) => Outcome::Output(output),
            Err(error) => Outcome::Failed(error),
        });
        if matches!(job.outcome, Some(Outcome::Failed(_))) {
            job.error_revision += 1;
        }
        Ok(())
    }
}

/// The service as a tower service, for mounting in an application's own
/// router or wrapping in layers on one host only: every call is
/// [`Crucible::handle`].
impl Service<Request<Body>> for Crucible {
    type Response = Response;
    type Error = Infallible;
    type Future = std::pin::Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Infallible>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let service = self.clone();
        Box::pin(async move { Ok(service.handle(request).await) })
    }
}

#[derive(Default)]
struct Entered {
    transport: usize,
    operation: usize,
    interceptors: usize,
    decoded: bool,
}

pub struct Builder {
    operations: Operations,
    middleware: Vec<Box<dyn Middleware>>,
    operation_middleware: Vec<Box<dyn OperationAwareMiddleware>>,
    interceptors: Vec<Box<dyn Interceptor>>,
    protocols: Vec<Box<dyn Protocol>>,
    translator: ExceptionTranslator,
    layers: Vec<Wrap>,
    reject: Option<Reject>,
    body_limit: usize,
    body_limits: Vec<BodyLimit>,
    request_timeout: Option<Duration>,
    request_id: Option<RequestIdSource>,
    unversioned: Unversioned,
}

type BodyLimit = Box<dyn Fn(&OperationInfo) -> Option<usize> + Send + Sync>;

impl Builder {
    /// Transport before hooks run before protocol identification, after hooks after encoding.
    pub fn middleware(mut self, middleware: impl Middleware) -> Self {
        self.middleware.push(Box::new(middleware));
        self
    }
    pub fn middlewares(
        mut self,
        middleware: impl IntoIterator<Item = Box<dyn Middleware>>,
    ) -> Self {
        self.middleware.extend(middleware);
        self
    }
    /// Policies with guaranteed operation metadata, before input decoding.
    pub fn operation_middleware(mut self, middleware: impl OperationAwareMiddleware) -> Self {
        self.operation_middleware.push(Box::new(middleware));
        self
    }
    pub fn operation_middlewares(
        mut self,
        middleware: impl IntoIterator<Item = Box<dyn OperationAwareMiddleware>>,
    ) -> Self {
        self.operation_middleware.extend(middleware);
        self
    }
    /// Decoded invocation extensions. Registration order within this stage is execution order.
    pub fn interceptor(mut self, interceptor: impl Interceptor) -> Self {
        self.interceptors.push(Box::new(interceptor));
        self
    }
    pub fn interceptors(
        mut self,
        interceptors: impl IntoIterator<Item = Box<dyn Interceptor>>,
    ) -> Self {
        self.interceptors.extend(interceptors);
        self
    }
    /// Protocols are alternatives: only the first claiming protocol decodes/writes.
    pub fn protocol(mut self, protocol: impl Protocol) -> Self {
        self.protocols.push(Box::new(protocol));
        self
    }
    /// Serve the tools carried by generated service metadata on the configured path.
    /// Service generation must enable `.mcp()` in its build script.
    pub fn mcp(mut self, config: crate::mcp::McpConfig) -> Self {
        let protocol = crate::mcp::McpProtocol::new(config, self.operations.service.mcp_tools);
        self.protocols.insert(0, Box::new(protocol));
        self
    }
    /// Serve the embedded local-development API explorer. Generate its static
    /// description with `crucible_build::model(..).server().explorer()`.
    #[cfg(feature = "explorer")]
    pub fn explorer(mut self, explorer: crate::explorer::Explorer) -> Self {
        self.protocols.insert(0, Box::new(explorer));
        self
    }
    pub fn protocols(mut self, protocols: impl IntoIterator<Item = Box<dyn Protocol>>) -> Self {
        self.protocols.extend(protocols);
        self
    }
    /// Orchestrator error rules cover failures in every entered phase.
    pub fn exception_translator(mut self, translator: ExceptionTranslator) -> Self {
        self.translator = translator;
        self
    }
    pub fn defaults(self) -> Self {
        use crate::ProtocolExt;
        // Older API versions' operations too: one a later version removed
        // is still served at the versions that had it.
        let mut names: Vec<&'static str> = self
            .operations
            .entries
            .values()
            .chain(
                self.operations
                    .versions
                    .older
                    .iter()
                    .flat_map(|older| older.operations.entries.values()),
            )
            .map(|entry| entry.info.name)
            .collect();
        names.dedup();
        self.middleware(ServiceLog::errors_only())
            .protocol(JsonProtocol.only(&names))
            .protocol(InvokeProtocol)
    }

    /// Wraps the whole service in a tower layer, outside the phased pipeline:
    /// for work on the response as sent, such as compression or counting
    /// the bytes that leave. Like extensions, the first added is outermost.
    /// Every host runs the layers: HTTP, Lambda HTTP events and direct
    /// invokes alike, so swapping hosts keeps them. A layer's errors must be
    /// answered inside it (tower's `HandleErrorLayer`); the response body may
    /// be any HTTP body.
    pub fn tower_layer<L, S, B>(mut self, layer: L) -> Self
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

    /// How a request that names no API version is served, for a `.cru`
    /// model: at the oldest supported version (the default), refused
    /// (`Unversioned::Required`), or at the version a resolver chooses.
    pub fn api_version(mut self, unversioned: Unversioned) -> Self {
        self.unversioned = unversioned;
        self
    }

    /// The largest request body any operation reads. Admission extensions run
    /// before buffering; the selected protocol rejects larger buffered bodies
    /// as a validation error. Streaming bodies enforce the limit while read.
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

    /// How long a request may take, from its arrival until its outcome is
    /// settled. Past it, the framework stops the request and fails it with
    /// [`DeadlineExceeded`](crate::DeadlineExceeded). Set it below what
    /// waits in front of the service: a CDN's origin timeout (CloudFront's
    /// is 30 s by default), or a gateway's. Otherwise the caller has gone
    /// before the service answers. A host's own deadline, such as a Lambda
    /// invocation's, applies with or without it; the earlier wins.
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    pub fn build(mut self) -> Result<Crucible, BuildError> {
        let builtins = std::cell::RefCell::new(crate::prepare::Builtins::default());
        let prepare = Prepare::new(&self.operations, &builtins);
        for middleware in &mut self.middleware {
            middleware.prepare(&prepare).map_err(BuildError::Prepare)?;
        }
        for middleware in &mut self.operation_middleware {
            middleware.prepare(&prepare).map_err(BuildError::Prepare)?;
        }
        for interceptor in &mut self.interceptors {
            interceptor.prepare(&prepare).map_err(BuildError::Prepare)?;
        }
        for protocol in &mut self.protocols {
            protocol.prepare(&prepare).map_err(BuildError::Prepare)?;
        }
        // Only `Permissions` builds a designated not-found error: without it
        // a hidden or missing resource would have no answer of that shape.
        if let Some(shape) = self.operations.service.not_found
            && !builtins.borrow().order.contains(&"permissions")
        {
            return Err(BuildError::Prepare(format!(
                "the model answers a missing resource with `{shape}` (`notFound` in the service), which only Permissions builds: install Permissions with a not_found_factory"
            )));
        }
        // Only `Permissions` enforces declared requirements: a served
        // operation that declares them outside it would run unchecked. Each
        // API version's entry is checked on its own, since a scope decides
        // on the description a request at that version carries.
        let unenforced: Vec<String> = {
            let builtins = builtins.borrow();
            prepare
                .served()
                .filter(|served| {
                    matches!(served.metadata.requirements, Requirements::List(_))
                        && !builtins.authorized.contains(&served.key())
                })
                .map(|served| served.label())
                .collect()
        };
        if !unenforced.is_empty() {
            return Err(BuildError::Prepare(format!(
                "no Permissions covers {}, so their declared requirements would go unchecked: install Permissions (the `permissions` feature) for them, or Permissions::shadow() to record without refusing",
                unenforced.join(", ")
            )));
        }
        if self.protocols.is_empty() {
            return Err(BuildError::NoProtocol);
        }
        let reject = match (self.reject, self.operations.service.reject) {
            (Some(reject), _) => reject,
            (None, Some(generated)) => Arc::new(generated) as Reject,
            (None, None) => return Err(BuildError::NoRejection),
        };
        let body_limit = |info| {
            self.body_limits
                .iter()
                .rev()
                .find_map(|limits| limits(info))
        };
        for entry in self.operations.entries.values_mut() {
            entry.body_limit = body_limit(entry.info);
        }
        for older in &mut self.operations.versions.older {
            for entry in older.operations.entries.values_mut() {
                entry.body_limit = body_limit(entry.info);
            }
        }
        let fallback = self
            .protocols
            .iter()
            .position(|p| p.fallback())
            .unwrap_or(0);
        let signed = self
            .operations
            .entries
            .values()
            .any(|entry| entry.info.sender.is_some());
        let core = Crucible {
            inner: Arc::new(Inner {
                operations: self.operations,
                middleware: self.middleware,
                operation_middleware: self.operation_middleware,
                interceptors: self.interceptors,
                protocols: self.protocols,
                fallback,
                translator: self.translator,
                reject,
                body_limit: self.body_limit,
                request_timeout: self.request_timeout,
                request_id: self.request_id,
                unversioned: self.unversioned,
                signed,
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
#[non_exhaustive]
pub enum BuildError {
    /// No protocol was registered, so nothing can identify requests.
    NoProtocol,
    /// The service's validation error needs members crucible cannot fill:
    /// give the builder `on_rejection`.
    NoRejection,
    /// An extension's `prepare` refused the service, saying why.
    Prepare(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoProtocol => "no protocol is registered",
            Self::NoRejection => {
                "the validation error has required members crucible cannot fill: call on_rejection"
            }
            Self::Prepare(reason) => reason,
        })
    }
}

impl std::error::Error for BuildError {}
