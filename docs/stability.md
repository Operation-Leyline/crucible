# Rust runtime stability

The documented public API of `crucible-server-runtime-rust` is the contract for
service authors. Generated bindings use a separate, hidden API at
`crucible_server_runtime_rust::__generated`. The Rust SDK uses the corresponding
`crucible_client_runtime_rust::__generated` boundary.

Crucible is still pre-1.0. A release may deliberately change the consumer API;
its migration instructions describe those changes. The hidden generated API
has no compatibility guarantee between versions. Regenerate bindings when
upgrading Crucible, and pin `crucible-build` and the runtime to the same version
or Git revision. Never edit generated files to bypass the version check.

## Service construction and invocation

The build API `crucible_build::Model::docs` and `ModelPackage::docs`, enabled by
the `docs` feature, add a named API-reference output without changing existing
defaults. `DocsOutput::{audience, generate, generate_into, try_generate,
try_generate_into}` and `Generator::Docs` are part of the documented consumer
surface. The serialized `crucible.describe/1` contract is stable as described in
[service API reference documentation](docs.md); its public `Description` struct
is non-exhaustive. Additive metadata fields may join format 1; incompatible
field meanings require a new format version.

`Crucible::builder` takes the operation registry produced by the generated
service module. `Crucible::{invoke, invoke_json, invoke_json_with}` support
direct calls, `handle` serves an HTTP request, and `into_router` integrates with
Axum. Generated `operations` and per-operation registration setters remain the
service's entry points for registering implementations.

`Builder` provides the following configuration methods:

| Purpose | Methods |
|---|---|
| Raw transport extensions | `middleware`, `middlewares` |
| Operation-aware extensions | `operation_middleware`, `operation_middlewares` |
| Decoded invocation extensions | `interceptor`, `interceptors` |
| Wire protocol selection | `protocol`, `protocols`, `defaults` |
| MCP serving | `mcp` |
| Error normalization | `exception_translator`, `on_rejection` |
| Hosting integration | `tower_layer`, `request_id` |
| Request limits | `body_limit`, `operation_body_limit`, `body_limits` |
| Construction | `build` |

`BuildError` is non-exhaustive. Handle known variants and include a fallback
when matching it. `MissingOperations` reports incomplete registration.

`mcp::McpConfig::{new, path, allowed_origins, allow_any_origin, endpoint_guard,
authorization, authorization_server_metadata, server_info}`, `mcp::AuthorizationServerMetadata`
and the `EndpointGuard`/`McpAuthorization` interfaces are additive service
configuration APIs. `authorization` derives discovery and
admission from one configuration; it leaves unauthenticated MCP behavior intact
when omitted. `crucible_auth::McpAuthenticator::{new,
allow_unbound_cognito_tokens}` composes configured access-token verifiers and a
service-owned endpoint scope. Audience binding is the default; unbound Cognito
tokens require explicit compatibility opt-in. See [MCP authentication](mcp.md#oauth-discovery-and-access-tokens).

## Extension interfaces and request views

Services extend the pipeline through `Middleware`, `OperationAwareMiddleware`,
`Interceptor`, `TypedInterceptor`, and `Protocol`. `AsyncMiddleware`,
`AsyncOperationAwareMiddleware`, `AsyncInterceptor`, and `async_trait` provide
boxed async hooks for the corresponding extension interfaces. `Hook` supports
synchronous completion or deferred work. `Prepare` supplies service and scoped
operation metadata during construction, where extensions can validate and cache
configuration. `OperationMiddlewareExt`, `InterceptorExt`, and `ProtocolExt`
provide scoping; `before_fn`, `after_fn`, `raw_before_fn`, and `for_operation`
adapt application functions and typed interceptors.

`Job`, `OperationJob`, and `DecodedJob` expose the request at their respective
pipeline phases. Use their methods to inspect headers, body, context, operation,
input, and outcome, or to reject and respond. Their private construction and
phase transitions belong to the runtime. `Context`, `WithContext`, `RequestMeta`,
`Resolution`, `Outcome`, and `OperationMetadata` support these views.
See [the framework pipeline](framework.md#the-request-pipeline) for phase and
unwind guarantees.

`Result`, `Error`, `ModelledError`, `FrameworkError`, and `ExceptionTranslator`
are the error contract. Modeled exceptions come from the generated service API.
Applications can inspect errors and translate application failures without
calling generated serialization helpers.

## Middleware, protocols, and hosting

`middlewares::{ServiceLog, Metrics, Cors, LogPolicy, LogSink, Recorder}` are
supported building blocks. `LogRecord` and `RequestRecord` expose observation
fields and are non-exhaustive; sinks and recorders read them instead of
constructing them.

The built-in protocols are `JsonProtocol`, `RestJsonProtocol`, `AwsJsonProtocol`,
`RpcV2JsonProtocol`, `RpcV2CborProtocol`, and `InvokeProtocol`. The `protocol`
module documents the interface types used by custom protocol implementations.
The wire metadata enum `operation::Protocol` is separate from the server's
`Protocol` trait and is non-exhaustive.

`host::{run, serve}` select or configure the host. `host::{http, http_on,
http_on_until}` serve HTTP with the `http` feature; `host::{lambda, lambda_event}`
support Lambda with the `lambda` feature. Model value types such as `Blob`,
`ByteStream`, `EventStream`, `BigInteger`, and `BigDecimal` remain available
through documented public paths.

Rust client applications use `OpenEnum::{from_string, as_str}` to construct and
inspect string enum values, including unknown values from a newer service.
`Field` and `UnknownUnion` support advanced presence and forward-compatibility
handling. These consumer types are re-exported directly by the client runtime;
their serialization machinery belongs to `__generated`.

## Reading generated metadata

`OperationInfo` and `ServiceInfo` are static descriptions exposed by operation
markers, request views, and preparation. Read their public fields and use
`trait_value`, `trait_as`, `has_trait`, `declares`, and `ServiceInfo::operation`
where applicable. `MemberInfo` and `ErrorInfo` describe members and modeled
errors. These structs are non-exhaustive: construction is owned by generated
bindings. `Location`, `Scalar`, `Segment`, `AuthScheme`, and `Fault` are also
non-exhaustive; application matches must handle future variants.

Generated bindings construct descriptions using hidden `const fn` constructors.
Descriptions and their slices remain `'static`; this boundary adds no
per-request metadata construction. Registration, codecs, dependency re-exports,
error writers, conformance infrastructure, and generated MCP descriptions use
`__generated`. They are omitted from ordinary runtime documentation.

## Version mismatch diagnostics

Every generated server binding and Rust SDK emits `__generated::check_abi!`
with the generating Crucible workspace version. A mismatched server produces:

```text
this code was generated by crucible-build X but crucible-server-runtime-rust is Y; use the same Crucible version for both
```

The client diagnostic names `crucible-client-runtime-rust` instead. Both checks
run during compilation, before a service starts or a client sends a request.
The runtime's version is embedded when that runtime is built; it is independent
of the consuming application's package version.

Upgrade the build dependency and runtime together, then rebuild to regenerate.
The `crucible-traits` Cargo package and model JAR share that workspace version;
see [release preparation](releases.md#preparing-a-version).

## Generated service CLI

Generated service executables support the additive `mcp` command, documented in
[the CLI guide](clients/cli.md#local-mcp-server). It uses existing configuration,
profiles and authentication. Generated-code transport entry points and descriptor
constructors belong to `crucible_cli_runtime::__generated`; they are hidden and
must be regenerated with the runtime. Existing `parse`, `Invocation` and generated
`dispatch` remain usable by wrappers with their previous defaults.
