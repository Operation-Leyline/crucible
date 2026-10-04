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
protocol-specific: bindings (label, query, header, body) are data in
`OperationInfo`, read by whichever protocol serves the request.

## The request context

`self.context()` inside an implementation, or `crucible::Context::current()`
anywhere on the request's task, returns the request context. It holds the
request id, the operation's `OperationInfo`, the deadline, request headers,
and typed attributes that handlers set (an authenticated principal, say).
It reads like a field of the implementing type, but `&self` is shared by concurrent
requests, so it is a task-local the orchestrator sets around the call.
**Decision:** a task spawned inside an implementation does not inherit it;
`context.scope(future)` carries it across.

## The orchestrator and the handler chain

The service builder takes handlers in order. Each request:

1. **Route.** The protocols registered in the chain are asked in order who
   claims the request. The first claim fixes the operation, so every handler
   knows `job.operation()` from the start. No protocol claims it: the
   operation is unknown, and the chain still runs, so it is logged.
2. **Before**, in chain order. A handler sees the `Job`: operation, context,
   the raw request, and (after the protocol handler) the typed input as a
   `&dyn Input` (JSON view, downcast).
3. **The implementation**, if every `before` passed.
4. **After**, in reverse order. A handler sees the outcome (output, modelled
   error or internal failure) and the response being built.

A failing `before` stops the chain going forward. The `after` of every
handler that already ran still runs, in reverse, so logging and exception
translation see failures too. The protocol that routed the request writes
the response last, after every `after`.

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

Crucible::builder(api::operations(Arc::new(service)))
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

## Extension points

Everything around an operation is a `Handler`, and the built-in ones are
ordinary handlers with nothing private. On top of the trait:

- **Scoping** (`HandlerExt`): `.only(&["GetCity"])`, `.except(..)`,
  `.when(|op| ..)` over `OperationInfo`, and `.when_trait("acme#audited")` for
  a trait in the model. The handler is skipped elsewhere.
- **Closures**: `before_fn(|job| ..)` and `after_fn(|job| ..)` for synchronous
  checks. Anything that awaits implements `Handler`.
- **Typed per-operation handlers**: `OperationHandler<GetCity>` sees
  `&GetCityInput` and `Option<&GetCityOutput>`; `for_operation(h)` puts it in
  the chain.
- **Bundles**: a library ships `Vec<Box<dyn Handler>>`; `Builder::handlers`
  appends it in order.
- **Protocols**: implement `Protocol` (route, write) and `Handler` (read the
  input) for a new wire format.
- **Sinks and recorders**: `LogSink` takes service-log records, `Recorder`
  takes metrics records.
- **Exception rules**: `ExceptionTranslator::translate::<E>` by type, or
  `translate_with` over any error.
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
  HTTP bindings, a JSON body, errors with `"__type"`.
- `InvokeProtocol` serves direct invocations: an operation called by name
  with its whole input as JSON, answered `{"output": ..}` or
  `{"error": {"__type": .., ..}}`. Lambda's direct invokes arrive this way,
  and `Crucible::invoke(name, input)` calls it in process. The request is
  marked by an extension only the framework sets, so no HTTP client can
  forge one.

## Errors

An implementation returns `crucible::Result<T>`, whose error is
`crucible::Error`, a boxed error. `?` works on anything. After the
implementation runs, the error is classified:

- **A modelled error the operation declares**: answered as itself.
- **A modelled error it does not declare**: a bug. It becomes an internal
  failure and is logged.
- **Anything else**: an internal failure.

The `ExceptionTranslator` handler maps internal failures to modelled errors
with registered translations (`|e: &ThrottledByDependency| ThrottlingException
{..}`). What nothing translates is answered as the service's internal error
(its one 500 server error, filled by the generator) or the protocol's own
`InternalFailure`. Either way the response has a fixed message, never the
error's text, which is logged with the request id. **Decision:** typed
per-operation error enums would check at compile time which errors an
operation returns. They are traded away for "return anything, translate
centrally". The check moves to runtime, where an undeclared error is caught
and logged as a bug.

A request that breaks the model becomes the service's validation error. The
generator fills it when it can: `message`, and a `fieldList` of `path`/`name`
plus `message`. Otherwise the service gives the builder
`on_rejection(|rejection| ..)`, and `Violation::kind` tells a malformed
request from a broken constraint.

## The service log

`ServiceLog` writes one structured record per request to its `LogSink`. The
default sink is `tracing`, target `crucible::service_log`. A record holds the
request id, operation, status, outcome (success, error, fault), latency,
error type, and chosen fields. **Default: errors only.** Per operation, a
`LogPolicy` chooses whether successes are logged and which input, output and
error members appear. `@sensitive` members are never written, even when
chosen. An internal failure's cause is in the record, because the log is the
service's own.

## Metrics

The `Metrics` handler builds a `RequestRecord` per request (operation,
latency, status, class, error) and hands it to a `Recorder` the service
supplies. Place it early in the chain so its `after` runs for requests a
later handler refuses. There are no built-in metric names yet: this is
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

`crucible_build::client("<links>")` generates a client from the same model
package, into `crucible_client::include_client!()`. The model and error code
is the server's, generated against `crucible-client`, which re-exports the
wire types (`crucible-wire`) under the same paths the framework does. A
server and a client built from one model share their types, validation and
member bindings; neither can drift from the other.

The generated file holds `model`, `errors`, the operation markers, and per
operation an error enum (`GetCityError`) of the errors it declares, plus
`Client` with one async method per operation and `*_pages` for
`@paginated` ones.

`Transport::send` builds a request from the operation's `OperationInfo`:
`split::write_bound` writes the body members in one pass and hands back
the labels (percent-encoded into the path), query parameters (repeated for
lists) and headers. A response is read back the way the server reads a
request, with `bound::read` laying response-header members over the body. An
error response decodes by its body's `__type`, else by status when exactly
one declared error has it; anything else is `Error::Unknown` with the status,
type name, message and request id.

Retries follow one rule: throttling errors always, other 5xx and transport
failures only when the operation is safe to repeat (`@readonly`,
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
