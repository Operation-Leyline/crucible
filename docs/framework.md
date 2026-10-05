# The Crucible framework

Crucible is two products:

1. **The framework** (`crucible` crate): everything a service does that is not
   specific to its API. That covers the request pipeline (an orchestrator
   running a handler chain), the request context, protocols, validation, the
   error model, the service log, metrics plumbing and transports. A service
   depends on it like any library.
2. **The Smithy adaptor** (`crucible-smithy` + `crucible-codegen-rust`, run by
   `crucible-build`): reads a model and generates the API-specific code that
   plugs into the framework. That is the types, an interface per operation,
   and the operation registry.

```
leyline-model/      model package: Smithy sources + the JSON AST its smithy build produces
leyline-service/    service package
  Cargo.toml          [dependencies] crucible, leyline-model   [build-dependencies] crucible-build
  build.rs            crucible_build::service("leyline_model").generate()
  src/                impl GetGraphOperation for Leyline { ... }   Crucible::builder()...
```

## Model packages

A model package holds the Smithy files and the JSON AST its own smithy build
produces (with `flattenAndRemoveMixins`). The AST is checked in and CI keeps
it fresh, so building a service needs no JVM. It is a Cargo crate with
`links = "<name>"`. Its build script publishes the AST's path
(`crucible_build::publish_model`), and a service's build script reads it from
`DEP_<NAME>_MODEL`. "This service depends on leyline-model 1.4" is then an
ordinary, version-pinned Cargo dependency.

## Generated code

`crucible_build::service(..).generate()` writes one file into `OUT_DIR`; the service
includes it with `crucible::include_service!()`. Per operation `GetGraph` it
generates:

```rust
pub struct GetGraph;                         // identity: impl crucible::Operation
impl Operation for GetGraph {
    const INFO: &'static OperationInfo = &...; // name, resource, http binding, errors, traits
    type Input = GetGraphInput;
    type Output = GetGraphOutput;
}

pub trait GetGraphOperation: crucible::WithContext {
    fn get_graph(&self, input: GetGraphInput) -> impl Future<Output = crucible::Result<GetGraphOutput>> + Send;
}
```

The service implements the trait. Grouping is free: one type can implement
every operation, or one type per resource.

```rust
impl GetGraphOperation for Leyline {
    async fn get_graph(&self, input: GetGraphInput) -> crucible::Result<GetGraphOutput> {
        let caller = self.context().attribute::<Principal>();
        let graph = self.graphs.find(&input.graph_id).await?;          // any error: `?`
        graph.ok_or_else(|| ResourceNotFoundException::new("no such graph").into())
    }
}
```

Registration is checked at compile time: the generated
`operations(service: Arc<T>)` requires `T` to implement every operation
trait. For split implementations, `Operations::builder()` takes one operation
at a time and `build()` names the missing ones.

Types are generated with serde and `Validate`. Errors implement
`crucible::ModelledError` (wire name, status, fault). None of it is
protocol-specific: bindings (label, query, header, body, payload) are data
in `OperationInfo`, read by whichever protocol serves the request.

A Smithy `blob` is `crucible::Blob`, which holds `bytes::Bytes`: base64 in
a JSON body, its byte count checked by `@length`. An `@httpPayload` blob is
the request body itself, and its `Blob` shares the body's buffer rather than
copying it.

## Laying out a service

One type per file, named for what it holds, Java-package style:

```text
src/
  contract              the generated contract: pub mod contract { include_service!() }
  operations/
    mod.rs              the service type, and what operations share
    service/            operations the service itself binds
      get_me.rs
    graph/              one folder per resource, its Smithy name in snake_case
      mod.rs            what the graph operations share
      create_graph.rs   impl CreateGraphOperation for Service
  handlers/
    mod.rs              pub use authenticate::Authenticate; ...
    authenticate.rs     one handler per file, used as handlers::Authenticate
  interceptors/         the same, for a client's interceptors
```

Each operation file holds one `impl XOperation for Service`; there is no
dispatch module, and `contract::operations(..)` still refuses to build while
any operation lacks one. `crucible scaffold <model.json>` writes a stub for
every operation that has no file yet, in its resource's folder, and adds the
`mod` lines that reach it; it never overwrites a file. (`--out` is the
operations folder, `src/operations` by default; `--service-type` the
implementing type, `crate::operations::Service` by default.) crucible's own
built-ins follow the same rule: `handlers::ServiceLog` is
`handlers/service_log.rs`, `JsonProtocol` is `protocol/json_protocol.rs`.

## The request context

`self.context()` inside an implementation, or `crucible::Context::current()`
anywhere on the request's task, returns the request context. It holds the
request id, the operation's `OperationInfo`, the deadline, request headers,
and typed attributes that handlers set (an authenticated principal, say).
It reads like a field of the implementing type, but `&self` is shared by concurrent
requests, so it is a task-local the orchestrator sets around the call.
**Decision:** a task spawned inside an implementation does not inherit it;
`context.scope(future)` carries it across, together with the request tracing span
and subscriber. The span records `request_id` and `operation`, so application
events emitted through `tracing` carry the same request context.

The request id comes from `Builder::request_id(source)` when the service
names one (an inbound header behind a trusted edge), else from the host, else
it is a fresh UUID; an id that is empty or over 128 bytes is replaced. On
Lambda the host supplies an HTTP event's own request id, or the invocation's
id for a direct invoke, and the invocation deadline, so a handler can budget
work against the time the function has left. Hosts pass both through a
`RequestMeta` request extension.

## The orchestrator and the handler chain

The service builder takes handlers in order. Each request:

1. **Route.** The protocols registered in the chain are asked in order who
   claims the request. The first claim fixes the operation, so every handler
   knows `job.operation()` from the start. No protocol claims it: the
   operation is unknown, and the chain still runs, so it is logged.
2. **Before**, in chain order. A handler sees the `Job`: operation, context,
   the raw request, and (after the protocol handler) the typed input as a
   `&dyn Input` (JSON view, downcast). The body is read when the selected
   protocol is reached, so authentication, admission checks and relays can
   finish without waiting for a client to send its body. Other protocols'
   hooks are skipped.
3. **The implementation**, if every `before` passed.
4. **After**, in reverse order. A handler sees the outcome (output, modelled
   error or internal failure) and the response being built.

A failing `before` stops the chain going forward. The `after` of every
handler whose `before` succeeded still runs, in reverse, so logging and exception
translation see failures too. The protocol that routed the request writes
the response after every `after`. Then `Handler::complete(&Job, &Response)`
runs in reverse order for the same handlers, observing the serialized status.
Completion observers cannot change the response. An answered response has been
moved out of the job by then; read its status from the response. Tower layers
outside the chain and transport streaming happen later.
If reading the body fails, only handlers already entered are unwound;
handlers beyond the selected protocol have no cleanup or completion hooks.

```rust
pub trait Handler: Send + Sync + 'static {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, crucible::Result<()>> { Hook::Ready(Ok(())) }
    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> { Hook::Ready(()) }
}

// A hook that waits on something returns a future instead:
//     Hook::wait(async move { self.limiter.check(job.operation()).await })
// Or write hooks as `async fn` by implementing AsyncHandler, which is a
// Handler too, at the cost of an allocation per call.
#[crucible::async_trait]
pub trait AsyncHandler: Send + Sync + 'static {
    async fn before(&self, job: &mut Job) -> crucible::Result<()> { Ok(()) }
    async fn after(&self, job: &mut Job) {}
}

Crucible::builder(contract::operations(Arc::new(service)))
    .handler(ServiceLog::errors_only())
    .handler(ExceptionTranslator::new().translate(|e: &DdbError| ...))
    .handler(MyAuthentication)          // sets a Principal attribute
    .handler(MyThrottle.when_trait("acme#operationClass"))
    .handler(JsonProtocol)              // reads + validates the input
    .handler(InvokeProtocol)            // direct invocations
    .build()?
```

`Crucible::builder(ops).defaults()` appends the service log (errors only),
the exception translator and both protocols. Handlers placed before the
protocols see the raw request. Handlers placed after them see the typed
input.

`Job::body()` is empty until the body has been read. A handler placed before
the protocol that needs buffered bytes must first await
`job.read_body().await?`. Reading again reuses the buffered result or the
original rejection. For HTTP streaming payloads, this prepares the bounded
stream instead of buffering it. Existing middleware that inspected
`job.body()` before the protocol must add this call in an asynchronous hook.

## Extension points

Everything around an operation is a `Handler`, and the built-in ones are
ordinary handlers with nothing private. On top of the trait:

- **Scoping** (`HandlerExt`): `.only(&["GetCity"])`, `.except(..)`,
  `.when(|op| ..)` over `OperationInfo`, and `.when_trait("acme#audited")` for
  a trait in the model. The handler is skipped elsewhere. A scoped protocol
  can claim only operations allowed by its scope.
- **Closures**: `before_fn(|job| ..)` and `after_fn(|job| ..)` for synchronous
  checks. Anything that awaits implements `Handler`.
- **Typed per-operation handlers**: `OperationHandler<GetCity>` sees
  `&GetCityInput` and `Option<&GetCityOutput>`; `for_operation(h)` puts it in
  the chain.
- **Bundles**: a library ships `Vec<Box<dyn Handler>>`; `Builder::handlers`
  appends it in order.
- **Protocols**: implement `Protocol` (route, write) and `Handler` (read the
  input) for a new wire format. A router with overlapping candidates should
  override `Protocol::route_scoped` to filter candidates before ranking them.
  Its default checks only the selected route; `JsonProtocol` filters first.
- **Sinks and recorders**: `LogSink` takes service-log records, `Recorder`
  takes metrics records.
- **Exception rules**: `ExceptionTranslator::translate::<E>` by type, or
  `translate_with` over any error.
- **The model's own traits**: `OperationInfo::traits` and
  `MemberInfo::traits` carry every trait outside Smithy's namespaces, as
  written in the model. `Handler::prepare(&mut self, &Prepare)` runs once
  at `build`: a handler reads them there (`trait_as::<T>(id)`,
  `has_trait(id)`) into its own per-operation table, and an `Err` fails the
  build, so a missing or malformed trait never reaches a request.
  `prepare.operations()` yields only the operations the handler runs for, so
  a scoped handler (`.only(..)`) is not refused over operations it skips;
  `prepare.service()` is the whole service.
- **Answering a request**: `job.respond(response)` in a `before` hook sends
  `response` as it is. The remaining `before` hooks, decoding and the
  operation are skipped; `after` hooks (the answering handler's included) see
  `Resolution::Answered { status }`, so a service log records it and a meter
  can skip it. For a response from elsewhere, such as relaying a request
  forwarded to another region.
- **Layers**: `Builder::layer(l)` wraps the whole service in any tower
  layer, outside the handler chain, first added outermost. It is for work on
  the response as it leaves, such as compression or counting bytes after
  compression, which a handler (it sees the outcome, not the bytes) cannot
  do. Every host runs the layers, Lambda included; a service with none pays
  nothing for them.
- **The request context**: a handler stores typed values with
  `context.set_attribute(..)`; implementations read them with
  `self.context().attribute::<T>()`.

## Protocols

A protocol is a handler that also routes and writes. In `before` it reads the
input and validates it, refusing a broken request with every violation. After
the chain it writes the response. Several can be registered, and the first to
claim a request serves it.

- `JsonProtocol` implements `crucible.protocols#json` (`smithy/crucible.smithy`):
  HTTP bindings, a JSON body, errors with `"__type"`. An input payload is
  the body unparsed, whatever its content type; an output payload is
  written raw under the blob's media type, and an absent one sends no body.
- `InvokeProtocol` serves direct invocations: an operation called by name
  with its whole input as JSON, answered `{"output": ..}` or
  `{"error": {"__type": .., ..}}`. A payload blob is base64 there, like any
  blob in JSON. Lambda's direct invokes arrive this way,
  and `Crucible::invoke(name, input)` calls it in process. The request is
  marked by an extension only the framework sets, so no HTTP client can
  forge one. Operations with streaming input or output payloads are refused
  before their implementation runs; invoke envelopes cannot carry these
  streams. Call those operations through HTTP.

## Errors

An implementation returns `crucible::Result<T>`, whose error is
`crucible::Error`, a boxed error. `?` works on anything. After the
implementation runs, the error is classified:

- **A modelled error the operation declares**: answered as itself.
- **A modelled error it does not declare**: a bug. It becomes an internal
  failure.
- **Anything else**: an internal failure.

The `ExceptionTranslator` handler maps internal failures to modelled errors
with registered translations (`|e: &ThrottledByDependency| ThrottlingException
{..}`). What nothing translates is answered as the service's internal error
(its one 500 server error, filled by the generator) or the protocol's own
`InternalFailure`. Either way the response has a fixed message, never the
error's text. An applicable `ServiceLog` with failure logging enabled records
the cause with the request id, provided it ran before any handler whose
`before` hook failed (see [The service log](#the-service-log)). **Decision:** typed
per-operation error enums would check at compile time which errors an
operation returns. They are traded away for "return anything, translate
centrally". The check moves to runtime, where an undeclared error is caught
as a bug and logging follows the configured policy.

A request that breaks the model becomes the service's validation error. The
generator fills it when it can: `message`, a `fieldList` of `path`/`name`
plus `message`, and the standard `reason` enum (`cannotParse` for a request
that does not read as its input, `fieldValidationFailed` for a broken
constraint). When that enum also has `unknownOperation`, a request no
operation serves gets the validation error with that reason (400), as the
standard asks; without it, the protocol answers its own `UnknownOperation`
(404). The values match in any casing (`cannotParse`, `cannot_parse`), so a
model keeps its own enum convention. Otherwise the service gives the builder
`on_rejection(|rejection| ..)`, and `Violation::kind` tells a malformed
request from a broken constraint.

A required member that is missing (a payload sent as an empty body, say)
is a broken constraint naming the member.

A buffered request body over the limit is refused during reading, as the
validation error. Other body transport failures report a malformed request
without exposing the underlying I/O error.
`Builder::body_limit` sets the service-wide limit (10 MiB by default);
`operation_body_limit::<GetCity>(bytes)` overrides it for one operation and
`body_limits(|op| ..)` for any rule over `OperationInfo`, such as a trait in
the model, resolved once at `build`.

Inputs ignore members the model does not define, unless the structure is
strict: marked `@crucible.protocols#strict`, or every structure an input
reaches when the service generates with `strict_inputs()`. A strict
structure answers an unknown member with the validation error, its path
naming the member. Clients never generate strict, so a newer service can add
members without breaking them.

An error the generator fills (validation or internal) that has a string
`requestId` member gets the id of the request it answers, the same one as
the `x-request-id` header. A translation or `on_rejection` that builds such
an error itself reads it from `Context::try_current()`.

## The service log

`ServiceLog` emits one typed `LogRecord` per selected request through its
`LogSink`, after protocol serialization. It records the actual protocol
response status, including serialization failures. The record contains the
request id, operation, status, outcome, latency, error type, and optional
payload fields. By default it records failures only.

Place `ServiceLog` first in the handler chain, with failure logging enabled
for every operation whose failures should be recorded. A missing, disabled,
or scoped-out service log emits no failure record. A service log placed after
a handler whose `before` hook fails never runs its completion hook, so it
cannot record that failure's cause. Protocols provide no fallback error log.
The default `TracingSink` also requires an installed subscriber whose filters
enable the event.

Per-operation `LogPolicy` controls successes, failures, and input, output,
and modelled-error fields. `@sensitive` members and targets are removed
recursively through structures, unions, lists, maps, and recursive shapes.
A map with sensitive keys is omitted completely. Sensitive list elements
become null. The generated metadata drives redaction; wire payloads are
unchanged. Arbitrary application messages and internal error text are the
application's responsibility: model annotations cannot redact those strings.

The default `TracingSink` emits core fields individually under the target
`crucible::service_log`: `request_id`, `operation`, `status`, `outcome`,
`latency_ms`, `error_type`, and `cause`. Optional redacted payloads are JSON
strings. Server faults use ERROR; successes and expected client errors use
INFO. `LogPolicy::client_error_level(tracing::Level::WARN)` changes the client
error level. Protocols do not emit a duplicate internal-failure event.

Ordinary application and dependency diagnostics use `tracing` directly. A
request span supplies correlation to application events:

```rust
tracing::info!(city_id = %city.id, "city created");
```

Applications install their own subscriber. Crucible never installs one as a
side effect of building or hosting a service. For a standard stdout setup,
enable the optional `logging` feature and call this once from the executable:

```rust
crucible::logging::init(crucible::logging::Format::Json)?;
```

Use `Format::Pretty` for local output. The helper reads `RUST_LOG`, defaults
to INFO, and returns an error if a subscriber is already installed. Applications
with their own subscriber, exporters, or filtering can keep those unchanged.

Custom sinks implement `LogSink::write(&LogRecord<'_>)`. The call is synchronous;
a sink doing I/O should enqueue records to a bounded writer and choose its
backpressure policy. `serde_json::to_value(record)` provides the previous
camelCase JSON layout for consumers that need it. Serialization-failure
records carry an internal cause but the response retains a fixed message.
Completion describes the response produced by the protocol; it does not
claim successful delivery or include subsequent tower-layer transformations.

### Migrating existing services

Regenerate Rust services and clients with the matching Crucible runtime.
`MemberInfo::sensitive` is replaced by recursive `redaction` metadata, and
`OperationInfo` now includes `literal_query`. Update custom log sinks from
`&serde_json::Value` to `&LogRecord<'_>`. If constructing `LogPolicy` directly,
set `client_error_level`; its constructors default to INFO. Move custom
post-serialization observers to `Handler::complete` when appropriate.
Before-protocol middleware that inspects `Job::body()` must await
`Job::read_body()` first. Keep admission handlers that need only headers
before that read. Move streaming direct-invoke callers to HTTP. Test both
hosts and early rejection paths before upgrading a service.

Deploy generated code, middleware and runtime together. Roll back together using
the previous dependency revision. This change does not require stored-data
migration. Consumers should release and bump their pinned Crucible revision
deliberately; changing this repository does not deploy Leyline or Pigeon.

## Metrics

The `Metrics` handler builds a `RequestRecord` per request (operation,
latency, status, class, error) and hands it to a `Recorder` the service
supplies. Place it early in the chain so its completion hook runs for requests a
later handler refuses. It records in `complete`, using the serialized
response's status and including serialization in latency. A serialization
failure therefore records an internal failure even if the operation
succeeded. Tower layers and delivery of streaming bytes happen afterward.
There are no built-in metric names yet: this is
plumbing.

## Hosting

`crucible::host::run(service)` reads the environment at startup:

- **Inside Lambda** (`AWS_LAMBDA_RUNTIME_API` set): it takes events from the
  Lambda runtime. HTTP events (Function URLs, API Gateway REST and HTTP
  APIs, ALB) are answered in their source's response shape. Direct JSON
  invokes go to `InvokeProtocol`. One function can receive both.
- **Elsewhere** (ECS, a VM, a laptop): it serves HTTP on `CRUCIBLE_ADDR`,
  else `0.0.0.0:$PORT`, else `0.0.0.0:8080`. On SIGTERM or Ctrl-C it stops
  accepting and finishes the requests in flight.

The same binary runs in both places, so moving a service between them is a
deployment change. `host::serve(service, Host::..)` picks explicitly,
`host::http_on(service, listener)` takes a bound socket, and
`host::lambda_event(service, payload)` answers one event (for tests or a
runtime loop of your own). Layers given to `Builder::layer` run on every
host, so they move with the service; `into_router()` gives an
`axum::Router` for layers that belong to one HTTP deployment only.

## Clients

For package setup, calls, and runtime limits, see the [Rust client guide](clients/rust.md).
The [client index](clients.md) links every other language.

`crucible_build::client("<links>")` generates a client from the same model
package, into `crucible_client::include_client!()`. The client uses
`crucible-client`, which re-exports the shared `crucible-wire` runtime. The IR
settles member bindings and constraints for both sides. Client models use
`Field<T>` for client-optional members and retain unknown enum and union values;
server models keep server nullability and closed enums and unions. See the
[Rust model migration](clients/rust.md#migrating-client-models) when updating
existing consumers.

The generated file holds `model`, `errors`, the operation markers, and per
operation an error enum (`GetCityError`) of the errors it declares, plus
a service-named client (`WeatherClient`, for example) with one async method per
operation and `*_pages` for `@paginated` ones.

`Transport::send` builds a request from the operation's `OperationInfo`:
`split::write_bound` writes the body members in one pass and hands back
the labels (percent-encoded into the path), query parameters (repeated for
lists) and headers. A payload goes as its bytes, under its media type, and
a payload response is read raw, sharing the response's buffer. A response is read back the way the server reads a
request, with `bound::read` laying response-header members over the body. An
error response decodes by its body's `__type`, else by status when exactly
one declared error has it; anything else is `Error::Unknown` with the status,
type name, message and request id.

Retries follow the [shared client contract](clients.md#http-retry-contract):
modeled throttling errors are eligible for retry, while explicitly retryable
errors, HTTP 500/502/503/504, and transport failures require an operation safe to repeat (`@readonly`,
`@idempotent`, or an idempotency token, which the client fills once per call
so every attempt carries the same one). Delays are full-jitter exponential,
and a `Retry-After` header wins. The bearer token (fixed, or a provider asked
on every attempt), default headers and `Interceptor`s apply to every attempt,
in that order.

TLS is rustls with the ring provider, installed as the process default only
when the application has not chosen one.

## Performance

The goal is a framework whose abstraction costs little next to the
operation. Two benchmarks in `e2e/weather-service/benches` measure it, in
process, against the same read written straight on axum:

- **`allocations`**: heap allocations and bytes per request. It is
  deterministic, so it is the number to compare changes by on a busy
  machine.
- **`requests`**: criterion timings.

| request | allocations at first | now | bare axum |
|---|---|---|---|
| `get_city` (label, query, header out) | 89 | 37 | 40 |
| `list_cities` (10 items) | 200 | 50 | |
| `create_city` (a replay: the token names an existing city) | 62 | 27 | |
| `ping` | 20 | 12 | |
| rejected label (validation error) | 83 | 54 | |
| direct invoke, JSON bytes in and out | 116 | 28 | |
| Lambda direct invoke, from the raw event | | 33 | |

In time, `get_city` is about on a par with hand-written axum (3.3 µs against
3.6 µs on the same run), down from 7 µs.

What got it there:

- **Input in one pass.** An input that is all body is deserialized straight
  from the body bytes. One with bound members is deserialized from a single
  map that chains the bound members (checked against their kinds first, so
  violations still name the parameter or header) with the body.
- **Output in one pass.** An output, an error and an invoke answer are each
  serialized straight to bytes. Header members are taken out as the body is
  written.
- **Hooks that do not wait do not allocate.** `Handler` hooks return
  `Hook::Ready` or a future; only `AsyncHandler` and waiting hooks box one.
- **Lambda direct invokes from the raw event.** The input is a slice of the
  event text, read once.
- **Lazy validation paths.** A violation's path is built into text only
  when a violation happens.
- **Borrowed text.** Query parameters are looked up and decoded per bound
  member, borrowing when nothing needs decoding. Label names are the model's
  `'static` strings.
- **Smaller fixed costs.** The request id is formatted once and shared with
  the response header, the request head is shared between the job and the
  context, routes are indexed by path length, and generated header names are
  lowercase.
