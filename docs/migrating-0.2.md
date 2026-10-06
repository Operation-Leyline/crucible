# Migrating to Crucible 0.2

[Framework guide](framework.md) · [Version releases](releases.md)

Crucible 0.2 replaces the handler chain with a phased request pipeline. Update
the service's runtime, generator dependencies, custom extensions, and generated
contract together. The release is a GitHub tag and source archive; it does not
publish packages to registries or upgrade Leyline and Pigeon automatically.

## Pinning the release

For Git dependencies, use the same tag for the framework and build integration:

```toml
[dependencies]
crucible = { git = "https://github.com/Operation-Leyline/crucible", tag = "v0.2.0" }

[build-dependencies]
crucible-build = { git = "https://github.com/Operation-Leyline/crucible", tag = "v0.2.0" }
```

Rust clients using `crucible-client` should use the same tag. Keep the service's
model-package dependency and regenerate from its published Smithy AST.

## Choosing an extension phase

Move each extension according to the data it needs, rather than its previous
position relative to a protocol:

| Previous API or responsibility | 0.2 API | Available data |
|---|---|---|
| `Handler` for headers or transport policy | `Middleware`; `.middleware(...)` | Raw request before operation selection; mutable encoded response on return |
| `Handler` needing operation metadata before decoding | `OperationAwareMiddleware`; `.operation_middleware(...)` | Selected operation and raw transport; invalid input still reaches admission |
| `Handler` needing validated model input | `Interceptor`; `.interceptor(...)` | `DecodedJob` before invocation; `OperationJob` and outcome on return |
| `OperationHandler<O>` | `TypedInterceptor<O>` with `for_operation(...)` | `&O::Input` before invocation; optional typed output and failure hooks on return |
| Protocol registered through `.handler(...)` | `Protocol`; `.protocol(...)` | Identification, decoding, and outcome encoding |
| Positioned `ExceptionTranslator` handler | `.exception_translator(...)` | Failures from every phase, including before identification |
| `.layer(...)` | `.tower_layer(...)` | Tower adapter outside the native pipeline |

`AsyncHandler` splits into `AsyncMiddleware`, `AsyncOperationAwareMiddleware`,
or `AsyncInterceptor`. Native traits return `Hook`; async helpers use
`#[crucible::async_trait]` and box each hook future.

Import `ServiceLog`, `Metrics`, log policies, and recorder interfaces from
`crucible::middlewares`. Import `ExceptionTranslator`, `before_fn`, `after_fn`,
`raw_before_fn`, and `for_operation` from `crucible`. The old `handlers` module
is removed. Keep application extensions in `middlewares/` and `interceptors/`.

`before_fn` and `after_fn` now construct operation-aware middleware. Use
`raw_before_fn` when a synchronous check must run before identification.
Operation scoping uses `OperationMiddlewareExt`, `InterceptorExt`, or
`ProtocolExt`; raw middleware has no operation scope. Libraries use separate
`.middlewares`, `.operation_middlewares`, `.interceptors`, and `.protocols`
registries instead of one `.handlers` bundle.

## Registering a service

This example uses the repository's Weather service and requires `crucible`,
`weather-service`, and `tracing` dependencies. It observes only validated
`GetCity` invocations:

```rust
use std::sync::Arc;

use crucible::{Context, Crucible, TypedInterceptor, for_operation};
use weather_service::{MemoryCities, Weather, contract};

struct ObserveGetCity;

#[crucible::async_trait]
impl TypedInterceptor<contract::GetCity> for ObserveGetCity {
    async fn before(
        &self,
        input: &contract::model::GetCityInput,
        context: &Context,
    ) -> crucible::Result<()> {
        tracing::info!(
            request_id = context.request_id(),
            city_id = input.city_id.as_str(),
            "GetCity invocation"
        );
        Ok(())
    }
}

fn service() -> Result<Crucible, crucible::BuildError> {
    let weather = Arc::new(Weather::new(Arc::new(MemoryCities::default())));
    Crucible::builder(contract::operations(weather))
        .defaults()
        .interceptor(for_operation(ObserveGetCity))
        .build()
}
```

`defaults()` adds `ServiceLog::errors_only()`, `JsonProtocol`, and
`InvokeProtocol`. Add authentication, admission, and metrics explicitly.
Registration order across extension families cannot change the phase order.
Malformed input never enters an interceptor; use operation-aware middleware
when admission or auditing must also cover invalid requests.

## Adapting hooks and protocols

Only entries whose `before` hook succeeds receive return and completion hooks.
A successful hook calling `respond` also unwinds, skips later phases, and
bypasses protocol encoding. Interceptor and operation-aware return hooks see
the outcome before encoding. Transport `Middleware::after` receives the
encoded response; modify that response directly. `complete` runs after native
response transformations, before outer Tower adapters and body delivery.

Typed interceptors receive `after(None, context)` on failure or an answered
request. On failure, `failed(error, context)` runs first. These hooks apply
only after the typed interceptor's own `before` succeeded. The operation
consumes its input; request `snapshot_input()` before invocation if an outer
return hook needs a JSON copy.

Protocols no longer implement `Handler`. Move input decoding from `before`
to `Protocol::decode`; implement `identify` or `identify_scoped` for bounded,
asynchronous envelope identification. Header/URI protocols can keep `route`
and `route_scoped`. `ProtocolMatch::Claimed(None)` means an unknown operation;
a malformed claimed envelope returns an error. Both cases keep the selected
protocol and do not fall through to another format.

Custom `Route` values set `rejection: None` for valid requests. A malformed URI
can carry a rejection while retaining operation metadata for admission and
audit. The orchestrator refuses it after operation-aware middleware, before
body reading and decoding. Apply protocol scopes before ranking candidates.
See [the protocol contract](framework.md#protocols) for claiming and declining.

## Checking wire and metadata changes

`crucible_wire::text::percent_decode` returns `Result<Cow<str>, String>`;
`query_values` yields fallible values. Handle malformed percent escapes and
decoded bytes that are not UTF-8 explicitly. Valid Unicode replacement
characters remain valid text; invalid bytes are no longer replaced with them.
For ordinary JSON object input bodies, zero bytes or only space, tab, CR, and
LF read as `{}` with or without bindings. An empty JSON payload is absent;
required-member validation still applies. Raw text and blob payloads retain
their bytes. Invalid modeled response header names or
values produce `InternalFailure` and serialization diagnostics for configured
logs and metrics, including modeled errors and streaming responses.

Services upgrading older generated descriptions must also use recursive
`MemberInfo::redaction` metadata and `OperationInfo::literal_query`. Custom
log sinks receive `&LogRecord<'_>`; directly constructed `LogPolicy` values
need `client_error_level`. Constructors default that level to INFO.

## Validating and rolling back

Pin the runtime, `crucible-build`, and any client runtime to the same release
or commit. Regenerate contracts, then compile the service and its independent
client consumers. Check HTTP and Lambda paths, early transport rejection,
unknown operations, malformed input, scoped admission, translated failures,
and streaming responses. Confirm logs and metrics observe the final native
response status and preserve the selected operation on rejected requests.

Bodies remain deferred. Await `Job::read_body()` or `OperationJob::read_body()`
before inspecting buffered bytes; keep header-only admission ahead of that
read. Direct invocation does not carry streaming payloads; use HTTP for them.
Request context survives phase changes; spawned tasks still need
`Context::scope` to carry it.

Deploy the regenerated code, middleware, and runtime together. Roll back all
three to the prior dependency revision and generated contract. This interface
change requires no stored-data migration. The [Weather phase tests](../e2e/weather-service/tests/phases.rs)
provide executable examples of phase order, early failures, and protocol claims.
