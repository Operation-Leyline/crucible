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
  build.rs            crucible_build::generate("leyline_model")
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

`crucible_build::generate` writes one file into `OUT_DIR`; the service
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
translation see failures too.

```rust
#[crucible::async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn before(&self, job: &mut Job) -> Result<(), crucible::Error> { Ok(()) }
    async fn after(&self, job: &mut Job) {}
}

Crucible::builder(leyline::operations(Arc::new(service)))
    .handler(RequestId::default())
    .handler(ServiceLog::errors_only())
    .handler(ExceptionTranslator::new().translate(|e: &DdbError| ...))
    .handler(MyAuthentication)          // sets a Principal attribute
    .handler(MyThrottle)                // reads job.operation()
    .handler(JsonProtocol::default())   // deserialize + validate | serialize
    .build()
```

`Crucible::builder(ops).defaults()` is the chain above, without the
service's own handlers. Handlers placed before the protocol see the raw
request. Handlers placed after it see the typed input.

## Protocols

A protocol is a handler that also routes. In `before` it reads the input
(bindings, JSON body), validates it, and refuses a broken request with every
violation. In `after` it writes the response. Several can be registered, an
RPC one and a REST one, and the first claim wins. `JsonProtocol` implements
`crucible.protocols#json` (`smithy/crucible.smithy`).

## Errors

An implementation returns `crucible::Result<T>`, whose error is `crucible::Error`,
a boxed error. `?` works on anything. After the implementation runs, the error is
classified:

- **A modelled error the operation declares**: answered as itself.
- **A modelled error it does not declare**: a bug. It becomes an internal
  failure and is logged.
- **Anything else**: an internal failure.

The `ExceptionTranslator` handler maps internal failures to modelled errors
with registered translations (`|e: &ThrottledByDependency| ThrottlingException
{..}`). What nothing translates is answered as the service's internal error:
a 500 with a fixed message, never the error's text, which is logged with the
request id. **Decision:** typed per-operation error enums would check at
compile time which errors an operation returns. They are traded away for
"return anything, translate centrally", which is the model you described. The
check moves to runtime, where an undeclared error is caught and logged as a
bug.

## The service log

`ServiceLog` writes one structured record per request through `tracing`
(target `crucible::service_log`): request id, operation, status, outcome
(success, client error, fault), latency, error type, and chosen fields.
**Default: errors only.** Per operation, choose whether successes are logged
and which input, output and error members appear. `@sensitive` members are
never written, even when chosen.

## Metrics

The orchestrator builds a `RequestRecord` per request (operation, latency,
status class, fault). The `Metrics` handler hands it to a `Recorder` the
service supplies. There are no built-in metric names yet: this is plumbing.

## Transport

A built `Crucible` is a `tower::Service` over HTTP, so it serves through axum,
hyper, or `lambda_http`. HTTP-only concerns (compression, egress metering)
stay as tower layers around it.
