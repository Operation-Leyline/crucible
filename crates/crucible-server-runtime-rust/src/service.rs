//! The orchestrator: routes a request, runs the phased pipeline around the
//! operation, and has the routing protocol write the response.

use std::convert::Infallible;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request};
use axum::response::Response;
use futures_util::FutureExt;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

use crate::context::{Context, RequestMeta};
use crate::error::{Error, FrameworkError, ModelledError};
use crate::middlewares::{Cors, ServiceLog};
use crate::operation::{Operation, OperationInfo};
use crate::operations::Operations;
use crate::protocol::{Answers, Invocation, InvokeProtocol, JsonProtocol, Protocol, ProtocolMatch};
use crate::validate::Rejection;
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
    cors: Option<Cors>,
    operations: Operations,
    middleware: Vec<Box<dyn Middleware>>,
    operation_middleware: Vec<Box<dyn OperationAwareMiddleware>>,
    interceptors: Vec<Box<dyn Interceptor>>,
    protocols: Vec<Box<dyn Protocol>>,
    fallback: usize,
    translator: ExceptionTranslator,
    reject: Reject,
    body_limit: usize,
    request_id: Option<RequestIdSource>,
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
            request_id: None,
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
        let cors = self.inner.cors.as_ref();
        let http = request.extensions().get::<Invocation>().is_none();
        let mut response = match &self.endpoint {
            Some(endpoint) => match endpoint.clone().oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            },
            None => self.dispatch(request).await,
        };
        // Completion recovery and outer Tower rejections can replace an encoded response.
        if http && let Some(cors) = cors {
            cors.decorate(&mut response);
        }
        response
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
        let deadline = meta.and_then(|meta| meta.deadline);
        let context = Arc::new(Context::new(id, None, parts));
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
            body_stream: None,
            labels: Vec::new(),
            decode: None,
            decode_json: None,
            decode_cbor: None,
            decode_bound: None,
            decode_events: None,
            input: None,
            outcome: None,
            error_revision: 0,
            response_headers: Default::default(),
            wants_snapshot: false,
            input_snapshot: None,
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
                    self.execute(&mut job, &mut entered, &mut selected).await;
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
        let mut route = None;
        for (index, protocol) in inner.protocols.iter().enumerate() {
            match protocol.identify(job, &inner.operations).resolve().await {
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
        let entry = inner
            .operations
            .entry(route.operation)
            .ok_or_else(|| Error::internal("protocol selected an unregistered operation"))?;
        job.context.select_operation(entry.info);
        job.context.select_metadata(entry.metadata);
        job.labels = route.labels;
        job.decode = Some(entry.decode);
        job.decode_json = Some(entry.decode_json);
        job.decode_cbor = Some(entry.decode_cbor);
        job.decode_bound = Some(entry.decode_bound);
        job.decode_events = Some(entry.decode_events);
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
        if job.wants_snapshot {
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
    request_id: Option<RequestIdSource>,
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
        use crate::operation::Protocol as Wire;
        use crate::protocol::{
            AwsJsonProtocol, RestJsonProtocol, RpcV2CborProtocol, RpcV2JsonProtocol,
        };
        let mut formats = Vec::new();
        for entry in self.operations.entries.values() {
            if !formats.contains(&entry.metadata.protocol) {
                formats.push(entry.metadata.protocol);
            }
        }
        if formats.is_empty() {
            formats.push(Wire::CrucibleJson);
        }
        use crate::ProtocolExt;
        let mut builder = self.middleware(ServiceLog::errors_only());
        for format in formats {
            let names: Vec<_> = builder
                .operations
                .entries
                .values()
                .filter(|entry| entry.metadata.protocol == format)
                .map(|entry| entry.info.name)
                .collect();
            builder = match format {
                Wire::CrucibleJson => builder.protocol(JsonProtocol.only(&names)),
                Wire::AwsRestJson1 => builder.protocol(RestJsonProtocol.only(&names)),
                Wire::AwsJson10 { .. } => builder.protocol(AwsJsonProtocol::v10().only(&names)),
                Wire::AwsJson11 { .. } => builder.protocol(AwsJsonProtocol::v11().only(&names)),
                Wire::SmithyRpcV2Json => builder.protocol(RpcV2JsonProtocol.only(&names)),
                Wire::SmithyRpcV2Cbor => builder.protocol(RpcV2CborProtocol.only(&names)),
                // Generated bindings and this runtime share a version; no unknown wire is routable.
                _ => panic!("unsupported generated wire protocol"),
            };
        }
        builder.protocol(InvokeProtocol)
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

    pub fn build(mut self) -> Result<Crucible, BuildError> {
        let cors = self
            .operations
            .service
            .cors
            .map(Cors::new)
            .transpose()
            .map_err(BuildError::Prepare)?;
        if let Some(cors) = &cors {
            self.middleware.insert(0, Box::new(cors.clone()));
        }
        let prepare = Prepare::new(&self.operations);
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
        if self.protocols.is_empty() {
            return Err(BuildError::NoProtocol);
        }
        let reject = match (self.reject, self.operations.service.reject) {
            (Some(reject), _) => reject,
            (None, Some(generated)) => Arc::new(generated) as Reject,
            (None, None) => return Err(BuildError::NoRejection),
        };
        for entry in self.operations.entries.values_mut() {
            entry.body_limit = self
                .body_limits
                .iter()
                .rev()
                .find_map(|limits| limits(entry.info));
        }
        let fallback = self
            .protocols
            .iter()
            .position(|p| p.fallback())
            .unwrap_or(0);
        let core = Crucible {
            inner: Arc::new(Inner {
                cors,
                operations: self.operations,
                middleware: self.middleware,
                operation_middleware: self.operation_middleware,
                interceptors: self.interceptors,
                protocols: self.protocols,
                fallback,
                translator: self.translator,
                reject,
                body_limit: self.body_limit,
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
