# The Crucible framework

The documented service-author API and generated-code version boundary are
described in [Rust runtime stability](stability.md).

Crucible is two products:

1. **The framework** (`crucible-server-runtime-rust` crate): everything a service does that is not
   specific to its API. That covers the request pipeline (an orchestrator
   running a phased pipeline), the request context, protocols, validation, the
   error model, the service log, metrics plumbing and transports. A service
   depends on it like any library.
2. **The Smithy adaptor** (`crucible-smithy` + `crucible-codegen-rust`, run by
   `crucible-build`): reads a model and generates the API-specific code that
   plugs into the framework. That is the types, an interface per operation,
   and the operation registry.

```
leyline-model/      model package: Smithy sources + the JSON AST its smithy build produces
leyline-service/    service package
  Cargo.toml          [dependencies] crucible-server-runtime-rust, leyline-model   [build-dependencies] crucible-build
  build.rs            crucible_build::model("leyline_model").server().generate()
  src/                impl GetGraphOperation for Leyline { ... }   Crucible::builder()...
```

## Start with one crate

Generate server models and bindings inside the service crate. Add `crucible-server-runtime-rust`
as a normal dependency and `crucible-build` as a build dependency, pinned to the
same revision. Hand wiring needs two hooks:

```rust
// build.rs
fn main() {
    crucible_build::model("smithy").server().generate();
}
```

```rust
// src/main.rs or src/lib.rs
crucible_server_runtime_rust::include_service!();
```

For Smithy `service Weather`, the inclusion hook declares `pub mod weather`.
Models, enums, exceptions, operation markers and traits are available directly:

```rust
use crate::weather::{GetCityInput, GetCityOutput, GetCityOperation, NotFoundError};
use crucible_server_runtime_rust::Result;
```

There is no wrapper module to create. The source path is relative to this
crate's manifest; it can also point outside the crate, such as
`../smithy/model`. With a published model dependency, replace the build call
with `crucible_build::model("weather_model").server().generate()`; inclusion is the
same, and Smithy is not compiled again.

The helper provides the pinned compiler, imports Crucible definitions, watches
the model, and writes the AST and generated bindings under `OUT_DIR`. The
inclusion macro makes those types visible to Rust without exposing the output
filename or writing generated code into the source tree. The
[single-crate quick start](quickstart.md) has a complete runnable service.

The starter declares `crucible.errors#ValidationException` (HTTP 400),
`crucible.errors#InternalServerException` (HTTP 500), and
`crucible.errors#UnknownOperationException` (HTTP 404) as service errors. The high-level
helper diagnoses an unfillable or missing validation error before startup.
Advanced services that supply their own `on_rejection` handler can call
`.custom_rejections()`. Existing JSON-AST build helpers retain that advanced
behavior.

## Model packages

Services and clients can select `crucible.toml` with `.policy("crucible.toml")`
on the build helper. API conventions run after model lowering and before
generation. Config changes trigger another Cargo build check. Compatibility is
a separate old/new contract comparison in CI. See
[API policy and compatibility](api-policy.md).

A model package is a Cargo crate with `links = "<name>"` holding shared Smithy
sources. Its build script compiles and publishes the AST with:

```rust
fn main() {
    crucible_build::model("smithy").publish();
}
```

The helper manages the pinned compiler, imports Crucible definitions, flattens
mixins, and keeps a stable AST under `OUT_DIR`. The service and client depend
on this package and read the AST from `DEP_<NAME>_MODEL`, using
`crucible_build::model("<name>").server()` or `crucible_build::model("<name>").client()`.
"This service depends on leyline-model 1.4" is an ordinary, version-pinned
Cargo dependency.

Alternatively, check in a precompiled JSON AST and keep it fresh in CI. Publish
its path with `crucible_build::model("model.json").publish()`; that layout needs
no JVM when building the service or client.

For model-owned Rust and Java SDK outputs, compile once with
`crucible_build::model("model")`, publish the model, and add a
named generation call per SDK variant. Another package can generate from
`crucible_build::model("<links>")` without assembling Smithy again.
See [building named SDK packages](build-outputs.md) for feature selection, native
package export, and direct service/SDK integration.

## Generated code

`crucible_build::model(..).server().generate()` writes one file into `OUT_DIR`; the service
includes it with `crucible_server_runtime_rust::include_service!()`. Per operation `GetGraph` it
generates:

```rust
pub struct GetGraph;                         // identity: impl crucible_server_runtime_rust::Operation
impl Operation for GetGraph {
    const INFO: &'static OperationInfo = &...; // name, resource, http binding, errors, traits
    type Input = GetGraphInput;
    type Output = GetGraphOutput;
}

pub trait GetGraphOperation: crucible_server_runtime_rust::WithContext {
    fn get_graph(&self, input: GetGraphInput) -> impl Future<Output = crucible_server_runtime_rust::Result<GetGraphOutput>> + Send;
}
```

The service normally owns this build script and includes the service namespace at its crate
root. A separate contract crate is optional when multiple
Rust crates need to share the generated server API. A reusable Rust SDK package
can own client inclusion so applications import its service-named client.
The [named SDK guide](build-outputs.md#consuming-the-rust-sdk-and-service-contract)
shows how to wire both directly by hand.

The service implements the trait. Grouping is free: one type can implement
every operation, or one type per resource.

```rust
use crate::leyline::{GetGraphInput, GetGraphOperation, GetGraphOutput, ResourceNotFoundException};
use crucible_server_runtime_rust::Result;

impl GetGraphOperation for Leyline {
    async fn get_graph(&self, input: GetGraphInput) -> Result<GetGraphOutput> {
        let caller = self.context().attribute::<Principal>();
        let graph = self.graphs.find(&input.graph_id).await?;          // any error: `?`
        graph.ok_or_else(|| ResourceNotFoundException {
            message: "no such graph".into(),
            resource_id: Some(input.graph_id),
        }.into())
    }
}
```

Registration is checked at compile time: the generated
`operations(service: Arc<T>)` requires `T` to implement every operation
trait. For split implementations, `Operations::builder()` takes one operation
at a time and `build()` names the missing ones.

Types are generated with serde and `Validate`. Errors implement
`crucible_server_runtime_rust::ModelledError` (wire name, status, fault). None of it is
protocol-specific: bindings (label, query, header, body, payload) are data
in `OperationInfo`, read by whichever protocol serves the request.

A Smithy `blob` is `crucible_server_runtime_rust::Blob`, which holds `bytes::Bytes`: base64 in
a JSON body, its byte count checked by `@length`. An `@httpPayload` blob is
the request body itself, and its `Blob` shares the body's buffer rather than
copying it.

## Model Context Protocol

Generate static public tool descriptions with `.mcp()` on the service build
helper, then opt in at runtime with
`.mcp(McpConfig::new())` on the service builder. MCP serves
the descriptors carried by generated `SERVICE.mcp_tools`; runtime setup does
not pass the descriptor constant separately. It handles
stateless JSON-RPC requests on `/mcp` alongside the modeled protocols. Tool calls
enter the normal operation pipeline; endpoint guards also protect initialization
and discovery. HTTP authentication refusals preserve their status and challenge.

Descriptors use the External audience projection and share JSON Schema emission
with OpenAPI. Streaming operations receive explicit build warnings. Outputs are
checked against their advertised schemas before publication. See
[serving MCP](mcp.md) for configuration, agent connections, Origin rules,
authentication hooks, and transport limits.

## Internal operations

Servers implement all operations regardless of client audience. Generated static
`OperationInfo::internal` records Smithy's `@internal` annotation. An application's
`OperationAwareMiddleware::before` can check `job.operation().internal` together
with an authenticated principal in `job.context()` and reject external callers.
The framework supplies metadata and extension interfaces; caller policy belongs
to the service. Client filtering does not secure a server endpoint.

The [audience integration test](../e2e/weather-service/tests/audience.rs) demonstrates
rejecting an external caller while allowing an internal caller to reach the same
operation. Its example header is only a test caller marker; production services
should use their authenticated principal.

## Laying out a service

One type per file, named for what it holds, Java-package style:

```text
src/
  lib.rs or main.rs     include_service!() declares the service-named API module
  operations/
    mod.rs              the service type, and what operations share
    service/            operations the service itself binds
      get_me.rs
    graph/              one folder per resource, its Smithy name in snake_case
      mod.rs            what the graph operations share
      create_graph.rs   impl CreateGraphOperation for Service
  middlewares/
    mod.rs              pub use authenticate::Authenticate; ...
    authenticate.rs     one middleware per file
  interceptors/         decoded invocation extensions
```

Each operation file holds one `impl XOperation for Service`; there is no
dispatch module, and `weather::operations(..)` still refuses to build while
any operation lacks one. `crucible scaffold <model.json>` writes a stub for
every operation that has no file yet, in its resource's folder, and adds missing
`mod` declarations without replacing existing operation code. (`--out` is the
operations folder, `src/operations` by default; `--service-type` the
implementing type, `crate::operations::Service` by default.) With that default,
a newly created root `mod.rs` also declares `pub struct Service;`; existing
service types are preserved. Custom service types must be declared by the
application. Rust keyword identifiers are escaped in module declarations and
generated stubs. `--contract` optionally overrides the derived namespace path
when bindings live in another Rust crate. Crucible's own
built-ins follow the same rule: `middlewares::ServiceLog` is
`middlewares/service_log.rs`, `JsonProtocol` is `protocol/json_protocol.rs`.

## The request context

`self.context()` inside an implementation, or `crucible_server_runtime_rust::Context::current()`
anywhere on the request's task, returns the request context. It holds the
request id, the operation's `OperationInfo`, the deadline, request headers,
and typed attributes that extensions set (an authenticated principal, say).
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
id for a direct invoke, and the invocation deadline, so middleware can budget
work against the time the function has left. Hosts pass both through a
`RequestMeta` request extension.

## The request pipeline

The orchestrator owns the phase order. Registration order matters within a
phase; a protocol is selected from alternatives, rather than placed between
arbitrary extensions.

```text
Host
  -> Middleware.before (raw transport)
  -> Protocol.identify (choose operation)
  -> OperationAwareMiddleware.before (metadata and raw transport)
  -> Protocol.decode (read and validate input)
  -> Interceptor.before (decoded invocation)
  -> Operation implementation
  <- Interceptor.after
  <- OperationAwareMiddleware.after
  <- Protocol.write (encode outcome)
  <- Middleware.after (encoded response)
  <- Completion observers (response ready)
```

Each family has `before`, `after`, `complete`, and `prepare` defaults. A failing
`before` stops forward execution. Only entries whose `before` succeeded unwind,
in reverse order within each phase. An extension answering with `respond`
also unwinds itself, skips later phases, and bypasses protocol encoding.
The orchestrator classifies and sanitizes every failure, including failures
before identification; configured exception rules normalize failures before outcome
extensions unwind, regardless of where the failure originated. If a return hook
replaces an error, the new failure is normalized before the next outer hook.
Rules are not run repeatedly on an unchanged failure.

| Interface | Before view | After view | Use |
|---|---|---|---|
| `Middleware` | `Job`, operation not yet selected | `Job` and mutable encoded `Response` | Headers, transport rejection, body wrappers, final-status observation |
| `OperationAwareMiddleware` | `OperationJob`, operation guaranteed | `OperationJob`, outcome available | Model-driven authentication, admission, relay, audit of invalid input |
| `Interceptor` | `DecodedJob`, validated input and operation guaranteed | `OperationJob`, outcome available | Typed authorization, invocation policy, outcome inspection and error changes |
| `TypedInterceptor<O>` | `&O::Input` and `&Context` | `after(Option<&O::Output>, &Context)`; on failure, `failed(&Error, &Context)` runs first | One operation's typed extension |

`OperationJob` and `DecodedJob` cannot be constructed by consumers and do not
expose unrestricted decoding or input replacement. Operation-aware middleware
cannot be registered as raw middleware. An unknown operation or failed
identification skips operation-aware middleware and interceptors. Decoding
failure skips interceptors, while earlier operation policies still unwind.
The request context, attributes, request id and deadline survive selection;
its tracing span is updated when the operation becomes known.

```rust
Crucible::builder(weather::operations(Arc::new(service)))
    .middleware(ServiceLog::errors_only())
    .middleware(MyHeaderAuthentication)
    .operation_middleware(MyThrottle.when_trait("acme#operationClass"))
    .protocol(JsonProtocol)
    .protocol(InvokeProtocol)
    .interceptor(for_operation(MyTypedAuthorization))
    .exception_translator(ExceptionTranslator::new().translate(|e: &DdbError| ...))
    .build()?
```

`defaults()` adds the error-only service log, the HTTP protocol selected by
the registered operations, and `InvokeProtocol`. Empty services use
`JsonProtocol` as their HTTP default.
It does not add authentication, admission policies, interceptors, or metrics.
`Middleware`, `OperationAwareMiddleware`, and `Interceptor` return `Hook::Ready`
for synchronous work and `Hook::wait` for asynchronous work. The corresponding
`AsyncMiddleware`, `AsyncOperationAwareMiddleware`, and `AsyncInterceptor`
helpers let extensions write `async fn` hooks, boxing their futures. A `Job`
contains a streaming body and is `Send`, not `Sync`; do not retain `&Job`
across an asynchronous suspension. Borrow a mutable phase view or extract
shared input/context references first.

### Extension helpers

- `OperationMiddlewareExt`, `InterceptorExt`, and `ProtocolExt` provide
  `.only`, `.except`, `.when`, and `.when_trait`. Raw middleware can inspect
  headers before identification; operation scoping belongs to later stages.
  Nested scopes only narrow `prepare.operations()` and request selection.
- `before_fn` and `after_fn` construct synchronous operation-aware middleware;
  `raw_before_fn` constructs a synchronous transport check.
- `for_operation` adapts `TypedInterceptor<O>` to the interceptor registry.
  It skips other operations and prepares only for its named operation.
- Libraries can ship separate bundles of boxed middleware, operation-aware
  middleware, interceptors and protocols through the matching plural builder
  methods. A bundle retains the phase guarantees.
- Every extension can read model traits once in `prepare`. Invalid required
  configuration fails service construction. `prepare.service()` exposes the
  whole model; `prepare.operations()` exposes only its scope.

### Bodies and responses

`Job::body()` and `OperationJob::body()` are empty until `read_body()` or the
selected protocol reads the body. A transport policy needing buffered bytes
must explicitly await `read_body()`. Header-only admission need not wait for
a client to send its body. Reads are idempotent, including rejected or cancelled
reads. The service-wide limit applies before selection; a smaller selected
operation limit also rejects previously buffered bytes. Streaming HTTP
payloads remain bounded streams; reading before selection buffers those bytes.

Native transport `after` hooks receive the encoded response and can change its
headers, status, or body, including wrapping its stream. `complete` observers
run after all native response transformations and cannot change the response.
Completion means response ready, not successful delivery or body exhaustion.
Cancellation drops request work; hooks are not guaranteed to run when their
future is dropped. Streaming failures after headers cannot replace the answer;
body wrappers can observe end, error, and drop for delivery-specific metering.

### Cross-origin requests

Smithy `@cors` on a service automatically installs the built-in `middlewares::Cors`
when `Builder::build` runs, including builders that do not call `defaults()`.
An empty trait uses origin `*` and a preflight cache age of 600 seconds. Set
`origin`, `maxAge` (including `-1` to disable caching),
`additionalAllowedHeaders`, or `additionalExposedHeaders` in the model.

Lowering computes sorted, case-insensitively deduplicated request and response
header lists across all operations and errors, including authentication headers,
`content-type`, `x-request-id`, and `X-Crucible-Error`. The generated service
uses the full server contract, including internal operations and headers;
client audience projection does not narrow its CORS policy. CORS permission
does not replace application caller authorization. The generated service carries
only a static description; the runtime prepares header values once.
Input `@httpPrefixHeaders` bindings retain their prefixes separately: preflight
adds matching concrete names from `Access-Control-Request-Headers`, since
browsers do not interpret partial wildcards such as `x-meta-*`.

Preflight `OPTIONS` with an allowed origin and modeled requested method receives
an empty 204 response before routing or application authentication, including
paths that would otherwise return an unknown-operation or method rejection.
Other requests follow the ordinary pipeline. Every HTTP response
carries the configured origin and exposed headers, including modeled errors, validation and
transport rejections, recovered panics, and outer Tower rejection responses.
Direct JSON invocations do not run HTTP CORS behavior.

Support currently requires an HTTP-binding protocol (Crucible JSON, REST JSON,
or REST XML; the selected generator must also support that protocol). Named
`origins` require deployment selection and are diagnosed as unsupported.
Browser-managed Basic or Digest credentials require an explicit origin; those
policies also return `Access-Control-Allow-Credentials: true`.

`Builder::tower_layer` is an explicit compatibility adapter for existing Tower
middleware, outside the native pipeline and on every host. It can reuse custom CORS,
compression, around-future task scopes, and similar transport machinery. The
first adapter added is outermost. Its errors must be answered by that adapter,
and its transformations/body delivery happen after native completion observers.
A service can implement response transforms entirely through Crucible
middleware. Request heads are read-only through `Job`; use the adapter
for request-header, URI, or body rewriting. In transport `after`, modify the
provided response directly: staging job headers, errors, or `respond` there
occurs after encoding. Tower is not required for application extension interfaces.


A service whose `.cru` model has migrations also serves older API
versions ([api-versions.md](api-versions.md)). After the raw middleware
runs, the request's version is settled from its `Crucible-Version`
header, so routing and decoding use that version's operations. After
decoding, an older version's input is converted to the current one, so
interceptors and the handler see only current types. Before encoding,
after the interceptors' and operation middleware's `after` phases, the
output or error is converted back.

## Protocols

A protocol implements `identify`, `decode`, and `write`. Identification returns
`ProtocolMatch::Declined` or `Claimed(Option<Route>)`. The first claim fixes
the protocol even if its envelope is malformed or its operation is unknown;
a claimed invalid request never falls through to another protocol. Use
`Claimed(None)` for an unknown operation and return `Err(rejection.into())`
for a malformed envelope. An error from identification is answered by that
protocol. With no claim, the first registered fallback protocol writes the
unknown-operation response. The defaulted, hidden `fallback` and `admit` hooks
support built-in endpoint admission and error encoding; ordinary protocol
extensions inherit their defaults.

Header/URI protocols can implement synchronous `route`, `route_scoped`, and
`owns`; default identification uses them. Filter scoped candidates before
specificity ranking. A malformed URI can return `Route::rejection`, retaining
operation metadata for admission and audit before decoding is refused.
Envelope-based RPC protocols implement asynchronous `identify_scoped` to
apply scopes while identifying. They may parse a bounded envelope there and
retain it in context attributes for `decode`, avoiding a second parse. A
protocol declining a request must leave it available to later protocols.

- `JsonProtocol` implements `crucible.protocols#json` (`smithy/crucible.smithy`):
  HTTP bindings, a JSON body, errors with `"__type"`. Blob and text payloads
  use the whole body as bytes or UTF-8 text; structure, union, and document
  payloads are decoded as JSON. Payloads use their target's media type or
  the protocol default. An absent output payload sends no body.
- `RestJsonProtocol` implements `aws.protocols#restJson1`: HTTP bindings and
  JSON documents, with errors identified by `X-Amzn-Errortype`.
- `AwsJsonProtocol::v10()` and `AwsJsonProtocol::v11()` implement AWS JSON
  1.0 and 1.1: `POST /`, `X-Amz-Target: Service.Operation`, and the version's
  `application/x-amz-json-*` media type. HTTP binding traits are ignored.
- `RpcV2JsonProtocol` and `RpcV2CborProtocol` implement Smithy RPC v2:
  `POST /service/Service/operation/Operation`, with an optional runtime path
  prefix and `Smithy-Protocol: rpc-v2-json` or `rpc-v2-cbor`. Both reject AWS
  target headers. Errors carry their full Smithy shape id in `__type`.
  CBOR preserves native byte strings, tag-1 timestamps, bignums, decimal
  fractions, and non-finite floating-point values. An absent input or output
  type uses an empty body (RPC v2 JSON servers also accept `{}` for an absent
  input); an empty modeled structure still uses a document.
- `InvokeProtocol` serves direct invocations: an operation called by name
  with its whole input as JSON, answered `{"output": ..}` or
  `{"error": {"__type": .., ..}}`. A payload blob is base64 there, like any
  blob in JSON. Lambda's direct invokes arrive this way,
  and `Crucible::invoke(name, input)` calls it in process. The request is
  marked by an extension only the framework sets, so no HTTP client can
  forge one. Operations with streaming input or output payloads are refused
  before their implementation runs; invoke envelopes cannot carry these
  streams. Call those operations through HTTP.

### Model-driven authentication

The optional [JWT bearer authentication](auth.md) crate (with its opt-in
[OAuth scope enforcement](auth.md#oauth-scopes)) supplies a model-driven
authenticator chain, Cognito presets, cached JWT verification, and test issuers.
Install it explicitly; `defaults()` continues to leave authentication off.

`Prepare::metadata(operation)`, `OperationJob::metadata()`, and
`DecodedJob::metadata()` expose the registered `OperationMetadata`: the wire
protocol, effective auth schemes, required OAuth scopes (and the service default),
optional-auth and unsigned-payload flags,
input/output presence, and event-stream descriptions. Context retains it after
selection through `Context::operation_metadata()`.

Server generation accepts bearer, Basic, API-key, optional-auth, and SigV4
metadata. Install `OperationAwareMiddleware` to verify credentials or signatures
before decoding. `defaults()` does not authenticate requests. Providers can add
custom authentication, signing policy, routing, and serialization through the
middleware and `Protocol` interfaces; AWS service names are not special cases
inside the orchestrator. Signed event-stream envelopes require a custom
protocol that verifies chunk signatures before exposing the enclosed events.
Protocol extensions can use `Job::take_body_stream()` and `decode_events()`
to retain generated input decoding and validation. HTTP checksums remain a
client capability.

### Event streams

HTTP protocols accept streaming unions as AWS event-stream frames. Inputs stay
lazy and obey the operation's body limit. Each event is deserialized and
validated when the operation reads it. Frame checksums and malformed messages
are checked by the shared wire reader. REST protocols bind other input members
to the URI or headers; RPC protocols receive an `initial-request` document first
and send an `initial-response` document before response events. RPC v2 rejects
content-length on event-stream requests.

Generated server inputs and outputs use `EventStream<Union>`. Read inputs with
`open()` and `recv()`; produce outputs with `new`, `try_new`, or `from_events`.
A modeled error arm becomes an exception frame and ends the response stream.
Source and encoding failures become a sanitized error frame. CBOR RPC v2 event
documents use the same native model codecs as buffered calls. Direct invocation
refuses streaming input or output. Lambda HTTP adapters buffer bodies and do
not provide interactive streaming; use the native HTTP host for streaming calls.

Response-body consumption happens after pipeline completion. Completion hooks
observe the response becoming ready, not the last event leaving the connection.

## Errors

An implementation returns `crucible_server_runtime_rust::Result<T>`, whose error is
`crucible_server_runtime_rust::Error`, a boxed error. `?` works on anything. Failures anywhere
in the pipeline are classified:

- **A modelled error the selected operation declares**: answered as itself.
  Before operation selection, an error known to the service model is accepted;
  this lets raw authentication return its modelled refusal.
- **A modelled error it does not declare**: a bug. It becomes an internal
  failure.
- **Anything else**: an internal failure.

The orchestrator-owned `ExceptionTranslator` maps internal failures to modelled errors
with registered translations (`|e: &ThrottledByDependency| ThrottlingException
{..}`). What nothing translates is answered as the service's internal error
(its one 500 server error, filled by the generator) or the protocol's own
`InternalFailure`. Either way the response has a fixed message, never the
error's text. An applicable `ServiceLog` with failure logging enabled records
the cause with the request id, provided it ran before any transport middleware whose
`before` hook failed (see [The service log](#the-service-log)). **Decision:** typed
per-operation error enums would check at compile time which errors an
operation returns. They are traded away for "return anything, translate
centrally". The check moves to runtime, where an undeclared error is caught
as a bug and logging follows the configured policy.

A request that breaks the model becomes the service's validation error. The
generator fills it when it can: `message`, a `fieldList` of `path`/`name`
plus `message`, and the standard `reason` enum (`cannotParse` for a request
that does not read as its input, `fieldValidationFailed` for a broken
constraint). The `fieldList` holds at most 100 violations
(`validate::MAX_VIOLATIONS`) and then one more, with an empty path, counting
the rest. Messages name the kind of a value that does not read, never the value
itself, so a `@sensitive` member's value cannot reach the response or the
service log. A declared `crucible.errors#UnknownOperationException` answers
an unmatched route as a modelled 404. It takes precedence over the validation
error's `unknownOperation` reason. A custom routing error can be selected with
`.unknown_operation_error("namespace#Error")` in the build helper, or
`generate rust --unknown-operation-error namespace#Error` in the CLI; it must
be a reachable client error with HTTP status 404 and fillable required members
(`message` and `requestId`). Other 404 errors are never selected implicitly.
When the path and literal query constraints match an operation but its method
does not, REST JSON protocols return HTTP 405 `MethodNotAllowed` with an `Allow`
header listing the supported methods. For example, a starter models
`POST /hello`: `GET /hello` returns 405 with `Allow: POST`, while `/missing`
follows the unknown-operation rules. GET operations also accept HEAD.
Without a dedicated routing error, when that enum also has `unknownOperation`, a request no
operation serves gets the validation error with that reason (400), as the
standard asks; without it, the protocol answers its own `UnknownOperation`
(404). The values match in any casing (`cannotParse`, `cannot_parse`), so a
model keeps its own enum convention. Otherwise the service gives the builder
`on_rejection(|rejection| ..)`, and `Violation::kind` tells a malformed
request from a broken constraint.

The shared exception models follow the API Standards exception names and
structured validation fields: required `message` and `reason`, plus an optional
`fieldList` of `name` and `message`. `InternalServerException` is retryable and
has an optional `Retry-After` header. Crucible supplies `x-request-id` on error
responses and fills internal errors with a fixed safe message. The standalone
`UnknownOperationException` (404) is an intentional Crucible choice; the API
Standards otherwise classify unknown operations as validation errors (400).
The earlier `ValidationError` and `InternalError` shapes remain available for
existing contracts. New services declare the exception shapes explicitly;
updating an existing service's error names or statuses changes its wire contract.

A required member that is missing (a payload sent as an empty body, say)
is a broken constraint naming the member.

Invalid percent escapes and URI components that do not decode to UTF-8 are
malformed requests. Query strings read `+` as a space; path labels preserve it.
Malformed query data does not erase readable literal-query constraints: a
known mismatch excludes that operation. Among equally specific paths, fully
matched literals take precedence over candidates whose relevant components
are malformed. Path specificity and operation scopes still apply first.
An ordinary JSON object input body containing zero bytes or only spaces, tabs,
CR or LF is read as `{}` with or without HTTP bindings. An empty JSON payload
is absent instead; required-member validation still applies. Other whitespace
bytes are invalid JSON. Raw text and blob payloads retain every byte.

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

## Protocol conformance

The Smithy `smithy.test` vectors in a model run against the real pipeline.
`httpRequestTests`, `httpResponseTests` and `httpMalformedRequestTests` are
lowered once, in `crucible-smithy`; `testParameters` of a malformed-request
vector are expanded there (`$name` is replaced in the request and the
expected response; ids become `Id[name=value]`). Request and response vectors
without `appliesTo: client`, and every malformed-request vector, are server
vectors.

Server conformance always uses the full model, including internal operations and
members, regardless of the selected client audience.

```rust
// build.rs
crucible_build::model("weather_model").server()
    .conformance()
    .skip_vector("SomeVector", "why the server fails it today")
    .generate();
```

```rust
// lib.rs; Cargo.toml: crucible-server-runtime-rust = { features = ["test-support"] }
// under [dev-dependencies]
#[cfg(test)]
mod conformance {
    crucible_server_runtime_rust::include_conformance!();
}
```

This writes one `#[test]` per vector for the service's protocol. The generated
file is a vector table and calls into `crucible_server_runtime_rust::conformance`;
what passes or fails is decided there, once:

- **Request vector**: the request is sent, a stub operation captures the
  input the pipeline decoded, and it must equal the vector's `params`.
- **Response vector**: the stub operation returns `params` (as the output, or
  as the modelled error for an error vector), and the status, headers and
  body the pipeline serializes are compared with the vector's. JSON bodies
  compare as documents, other bodies exactly; headers by name without case.
- **Malformed-request vector**: the request must be refused with the stated
  status, headers and body (`contents` as a document, or `messageRegex`
  against the body's `message`), and no operation may run.

`params` are Smithy's, not the model's: blobs are plain text and timestamps
epoch seconds. A `ParamShape` table generated from the IR translates them to
the wire form the generated types read, so the comparison is between typed
values. A response vector is answered through a request the operation accepts:
the operation's own when it has no labels and no required input, else the first
request vector of that operation. A service built for the tests applies
`defaults()`; pass a function to `include_conformance!(|builder| ..)` to add the
service's own middleware.

Nothing is dropped silently. A vector the generator cannot run (`vendorParams`,
`authScheme`, a streaming member, no usable request) is a build error naming
it; a vector the server fails is listed with `skip_vector(id, reason)`, which
keeps it as an `#[ignore = "reason"]` test so it appears in every run, and a
skip that names no vector is a build error too. A model with no applicable
vector, or one using `smithy.protocols#rpcv2Cbor`, fails the build.
`seed_request(operation, method, uri, headers, body)` names the request for an
operation whose response vectors have no other (an RPC operation without input,
for example).

`e2e/rpcv2-json-service` runs Smithy's official RPC v2 JSON protocol tests this
way (the `rpcv2-json` fixture), and `e2e/weather-service` runs the weather
model's own, merged from `weather-vectors.json` the way an `apply` statement
would add them.

## The service log

`ServiceLog` emits one typed `LogRecord` per request enabled by its policy
through its `LogSink`, after native response middleware finishes. It records
the response status, including serialization failures and middleware changes.
Before operation selection, records use `operation: "unknown"` and the default
policy. The record contains the
request id, operation, status, outcome, latency, error type, and optional
payload fields. By default it records failures only.

Place `ServiceLog` first in the transport middleware registry, with failure logging enabled
for every operation whose failures should be recorded. A missing service log or
a policy with failures disabled emits no failure record. A service log placed after
transport middleware whose `before` hook fails never runs its completion hook, so it
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
`crucible_server_runtime_rust::service_log`: `request_id`, `operation`, `status`, `outcome`,
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
crucible_server_runtime_rust::logging::init(crucible_server_runtime_rust::logging::Format::Json)?;
```

Use `Format::Pretty` for local output. The helper reads `RUST_LOG`, defaults
to INFO, and returns an error if a subscriber is already installed. Applications
with their own subscriber, exporters, or filtering can keep those unchanged.

Custom sinks implement `LogSink::write(&LogRecord<'_>)`. The call is synchronous;
a sink doing I/O should enqueue records to a bounded writer and choose its
backpressure policy. `serde_json::to_value(record)` provides the previous
camelCase JSON layout for consumers that need it. Serialization-failure
records carry an internal cause but the response retains a fixed message.
Completion describes the response after native middleware; it does not
claim successful delivery or include subsequent tower-layer transformations.
Invalid modelled response header names or values also produce a serialization
failure, including headers on modelled errors and streaming responses.

### Migrating existing services

See [Migrating from 0.1 to 0.2](migrating.md#010-to-020) for the registration and import
changes, protocol hooks, typed interceptors, validation checklist, and rollback.

## Metrics

The `Metrics` middleware builds a `RequestRecord` per request (operation,
latency, status, class, error) and hands it to a `Recorder` the service
supplies. Place it early in the chain so its completion hook runs for requests a
later extension refuses. It records in `complete`, using the final native
response's status and including serialization in latency. A serialization
failure therefore records an internal failure even if the operation
succeeded. Native middleware status changes are reflected in the record.
Tower layers and delivery of streaming bytes happen afterward.
There are no built-in metric names yet: this is
plumbing.

## Hosting

### Local API explorer

Enable the runtime's optional `explorer` feature, generate the description with
`crucible_build::model(..).server().explorer()`, and register
`Builder::explorer(Explorer::new(service::DESCRIPTION))` inside
`if cfg!(debug_assertions)`. The service then serves `/explorer/` for loopback
peers. The endpoint works on HTTP and Lambda; unknown peers are denied unless
access is explicitly changed. Try-it calls use normal HTTP routes and the
ordinary request pipeline. See [Crucible Explorer](explorer.md) for configuration,
authentication, audience selection and browser limits.

### Host selection

`crucible_server_runtime_rust::host::run(service)` reads the environment at startup:

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
runtime loop of your own). Layers given to `Builder::tower_layer` run on every
host, so they move with the service; `into_router()` gives an
`axum::Router` for layers that belong to one HTTP deployment only.

## Clients

For package setup, calls, and runtime limits, see the [Rust client guide](clients/rust.md).
The [client index](clients.md) links every other language.

For a reusable SDK, `model.rust_client("rust").generate_into("../target/rust-sdk")`
creates a complete Cargo package with its runtime declared automatically. An
application depends on that package normally and imports configuration and
errors through the SDK. See [building named SDK packages](build-outputs.md) for
model-owned and separate-producer layouts.

For generation directly inside an existing Rust crate,
`crucible_build::model("<links>").client()` generates a client from the same model
package, into `crucible_client_runtime_rust::include_client!()`. The client uses
`crucible-client-runtime-rust`, which re-exports the shared `crucible-wire` runtime. The IR
settles member bindings and constraints for both sides. Client models use
`Field<T>` for client-optional members and retain unknown enum and union values;
server models keep server nullability and closed enums and unions. See the
[Rust model migration](clients/rust.md#migrating-client-models) when updating
existing consumers.

For a standalone client, `crucible new hello-client --client` creates a library
whose build script calls `crucible_build::model("smithy").client().generate()`.
It uses the same managed compiler as the service starter, and needs no separate
model package or hand-written Smithy build configuration.

The generated file exposes models and modeled exceptions directly beside the
client, with private serde implementation modules. It also exposes an operation
marker and an error enum (`GetCityError`) for each operation, plus
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

| request | allocations at first | before serving on hyper | now | bare axum |
|---|---|---|---|---|
| `get_city` (label, query, header out) | 89 | 46 | 38 | 47 |
| `get_city` through `into_router()` | | 57 | 46 | |
| `list_cities` (10 items) | 200 | 103 | 95 | |
| `create_city` (a replay: the token names an existing city) | 62 | 38 | 30 | |
| `ping` | 20 | 14 | 8 | |
| rejected label (validation error) | 83 | 46 | 39 | |
| direct invoke, JSON bytes in and out | 116 | 36 | 31 | |
| Lambda direct invoke, from the raw event | | 40 | 35 | |

In time, `get_city` is about on a par with hand-written axum (3.3 µs against
3.6 µs on the same run), down from 7 µs. These benchmarks call
`Crucible::handle` in process; over sockets, `host::http_on` serves the
service on hyper directly, with no router in front.

What got it there:

- **Input in one pass.** An input that is all body is deserialized straight
  from the body bytes. One with bound members is deserialized from a single
  map that chains the bound members (checked against their kinds first, so
  violations still name the parameter or header) with the body.
- **Output in one pass.** An output, an error and an invoke answer are each
  serialized straight to bytes. Header members are taken out as the body is
  written.
- **Hooks that do not wait do not allocate.** Native extension hooks return
  `Hook::Ready` or a future; async helpers and waiting hooks box one. Scoped asynchronous protocol
  identification also boxes the scope adapter future.
- **Lambda direct invokes from the raw event.** The input is a slice of the
  event text, read once.
- **Lazy validation paths.** A violation's path is built into text only
  when a violation happens.
- **Borrowed text.** Query parameters are looked up and decoded per bound
  member, borrowing when nothing needs decoding. Label names are the model's
  `'static` strings.
- **Smaller fixed costs.** The request id is formatted once and shared with
  the response header, the context owns the request head and the job reads
  it there, routes are indexed by path length in a list, per-operation body
  limits are settled at build, and generated header names are lowercase.
- **No router in front.** Crucible routes every request itself, so the HTTP
  host hands hyper's requests straight to it; `into_router()` remains for
  mounting the service in an application's own axum router.
- **Routing without collecting.** Path segments are byte ranges of the path,
  a greedy label is one slice of it, and the methods allowed at a path (for
  a 405) are gathered only when no operation matched.
- **Nothing built for the common case.** A body read in progress is a state,
  not a pre-built rejection; the validator keeps its first steps inline; body
  members bound elsewhere are excluded by checking the operation's members in
  place; and an operation's future is boxed once, with its output conversion.
