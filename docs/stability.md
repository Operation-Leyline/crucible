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
field meanings require a new format version. The static HTML reference's file
names (`index.html`, `explorer.js`, `explorer.css`, `describe.js`) and the
`CrucibleExplorer.registerOperationPanel` hook are stable as described there;
the renderer's CSS class names and internal functions are not. The
`crucible.changes/1` contract (`changes.json`, beside `describe.json`) is
stable under the same rules, as described in
[the docs guide](docs.md#changesjson); `Description::versions` and
`describe::sort_keys` are additive. `llms.txt` and `llms-full.txt` (the Markdown pages'
index for agents, beside `index.html`) and the `src/` page paths they link
are stable file names; their prose is not. `DocsOutput::schemas`,
`crucible generate docs --schemas`, `crucible_codegen::{docs, DocsOptions,
Schemas}`, `crucible_codegen_docs::{generate_with, Options, Schemas}`,
`Description::schemas` (the method and the field) and
`CrucibleExplorer.operationSchema` are additive; `Schemas` and the options
are non-exhaustive. `DocsOutput::all_releases`, `crucible generate docs
--all-releases`, `crucible_codegen::{docs_releases, release_directory}`,
`crucible_codegen_docs::{generate_releases, release_directory}` and
`CrucibleExplorer.releaseDirectory` and `describe::describe_cached` are additive; the
layout they write (a directory per release, named by `release_directory`,
with `index.html` and `llms.txt` at the root) is stable as described in
[the docs guide](docs.md#every-releases-reference), and the index's prose
is not.

Upgrade guides ([the docs guide](docs.md#upgrade-guides)) add `crucible
upgrade-guide`, `crucible_codegen_docs::upgrade_guide` and the
`src/upgrade/<from>_to_<to>.md` pages of a reference with more than one
release, linked from `llms.txt`; they are additive. A pair's page path
is stable while the reference carries that pair, but which pairs it
carries moves with the chain: a release to the next and to the current
one, so `v1_to_v3.md` gives way to `v1_to_v4.md` once `v4` is current. A
guide's prose and layout are not stable, and neither is its refusal
text.

`DocsOutput::sdks`, `crucible generate docs --sdks` and `crucible sdk
describe` add the `crucible.sdks/1` contract (`sdks.json`), stable as
described in [the docs guide](docs.md#the-cruciblesdks1-contract) under
`describe/1`'s rules. Its in-memory form,
`crucible_codegen_common::surface` (`Surface`, `Package`, `ClientType`,
`Middleware`, `Operation`, `Structure`, `Member`; the hidden
`unqualified` is generator support), the
`surface(&Service, &Options)` function of each client generator,
`crucible_codegen::Client::surface`, `crucible_codegen::sdks::{FORMAT,
FILE_NAME, visible_in, document, render, file}` and
`crucible_codegen_ts::Middleware::{name, headers}` are additive. The
structs are non-exhaustive: start from `new` and set fields, so new
contract fields are not breaking. `Operation::snippet` and
`Operation::paginated_snippet` are such fields. The modules
`crucible_codegen_common::snippet` and `crucible_codegen_common::guide`,
which generators share to write snippets and package READMEs, are
generator support, not part of the stable surface.

SDK lines ([api-versions-clients.md](api-versions-clients.md#5c-one-sdk-line-per-release))
add `crucible_build::Model::api_version`, `ModelPackage::api_version`,
the `--api-version` option of `crucible generate` and `crucible check`,
the SDK file's `api_version` key and `crucible_codegen::sdks::lines_of`,
all additive: without them every output is the current release's, as
before. A reference's `sdks.json` describes the SDKs of the release it
documents.

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
| Local API explorer (optional `explorer` feature) | `explorer` |
| Error normalization | `exception_translator`, `on_rejection` |
| Hosting integration | `tower_layer`, `request_id` |
| API versions ([docs](api-versions.md)) | `api_version` |
| Request limits | `body_limit`, `operation_body_limit`, `body_limits` |
| Response streams ([guide](streaming.md)) | `stream_options` |
| Construction | `build` |

`BuildError` is non-exhaustive. Handle known variants and include a fallback
when matching it. `MissingOperations` reports incomplete registration.

`EventStream::from_try_stream`, `StreamOptions`, `StreamCompletion`, `StreamEnd`,
the extension interfaces' `stream_complete` observers, and
`RequestRecord::stream` and `Job::stream_body` support response streams. `complete` retains its
response-ready meaning. `Protocol::{supports_server_stream, write_stream,
stream_item, stream_end, stream_heartbeat}` opt a protocol into encoding them.
`Host::LambdaStreaming`, `host::lambda_streaming`, and
`host::lambda_streaming_event_json_with` select or test the streaming HTTP
Lambda adapter. See the guide for host restrictions.

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
configuration; `ServedOperation` is one API version's entry for an operation,
as `Prepare::served_operations()` lists it. `OperationMiddlewareExt`,
`InterceptorExt`, and `ProtocolExt` provide scoping; `before_fn`, `after_fn`,
`raw_before_fn`, and `for_operation` adapt application functions and typed
interceptors.

`Job`, `OperationJob`, and `DecodedJob` expose the request at their respective
pipeline phases. Use their methods to inspect headers, body, context, operation,
input, and outcome, or to reject and respond. Their private construction and
phase transitions belong to the runtime. `Context`, `WithContext`, `RequestMeta`,
`Resolution`, `Outcome`, and `OperationMetadata` support these views.
`OperationMetadata` is non-exhaustive: the runtime builds it at
registration, and services read its fields. `ApiVersion` and
`Unversioned` (from `versioning`) name a request's API version; the rest of
`versioning` is generated-code support.
See [the framework pipeline](framework.md#the-request-pipeline) for phase and
unwind guarantees.

`Result`, `Error`, `ModelledError`, `FrameworkError`, `ExceptionTranslator`,
and `OpaqueError` are the error contract. `FrameworkError` is non-exhaustive: a
`match` on it needs a wildcard arm, and `name`, `status`, `message` and
`shape_id` cover every variant. Modeled exceptions come from the generated
service API.
Applications can inspect errors and translate application failures without
calling generated serialization helpers.

## Middleware, protocols, and hosting

`middlewares::{ServiceLog, Metrics, LogPolicy, LogSink, Recorder}` are
supported building blocks. `LogRecord` and `RequestRecord` expose observation
fields and are non-exhaustive; sinks and recorders read them instead of
constructing them.

The built-in protocols are `JsonProtocol` and `InvokeProtocol`. The `protocol`
module documents the interface types used by custom protocol implementations.

`host::{run, serve}` select or configure the host. `host::{http, http_on,
http_on_until}` serve HTTP with the `http` feature; `host::{lambda, lambda_event}`
support Lambda with the `lambda` feature. Model value types such as `Blob`,
`BigInteger`, and `BigDecimal` remain available
through documented public paths.

`crucible_client_runtime_rust::Document` names a document value
(`serde_json::Value`) without a direct `serde_json` dependency. Rust client
applications use `OpenEnum::{from_string, as_str}` to construct and
inspect string enum values, including unknown values from a newer service.
`Field` and `UnknownUnion` support advanced presence and forward-compatibility
handling. These consumer types are re-exported directly by the client runtime;
their serialization machinery belongs to `__generated`.

## Typed permissions

The `permissions` module is part of the documented surface. Its static
descriptions are always present, since generated bindings state them:
`ResourceInfo`, `PermissionInfo`, `RequirementInfo`, `Requirements`,
`Obligation`, `Check`, `ResourceRef` and `Target`, with the non-exhaustive
`Mode`, `Origin` and `Tier`. Generated bindings own the `'static`
descriptions; services read them, and build `ResourceRef` and
`Check::handler` values for handler checks. `Checks` and `ErasedChecks` are
generated-code support.

Behind the `permissions` feature, `Authorizer` (`decide`, and `visible` with
an erroring default; a later provided method keeps a default) is the
interface a service implements, and `Permissions::{new, refusal_factory,
not_found_factory, allow_undeclared, shadow, assert_obligations,
undeclared_operations}` installs it as an interceptor.
`UndeclaredOperations::get`, `PermissionRefusal::{public_permission,
permission_for_logging, resource, origin, default_error}`, and the
non-exhaustive `Verdict` and `Visibility` complete that configuration.
`Verdicts` (what `decide` answers: a slice of `Verdict`, `into_vec`) is
`#[must_use]`. `AuthorizationRequest`,
`OperationFacts`, `Listing`, `AuthorizationRecord`, `Decision`,
`ObligationRecord` and `ObligationOutcome` are non-exhaustive: the runtime
constructs them and services read them. Handlers use `require`, `decide`,
`discharge`, `not_applicable`, `visible` and `not_found`.

Building a service enforces the wiring: every served operation that
declares requirements must be covered by a `Permissions` at every API
version that serves it, and `Permissions`
must be registered before `page_tokens::PageTokens` and
`idempotency::Idempotency`. A service that breaks either rule gets a
`BuildError::Prepare` naming it. See
[typed permissions](framework.md#typed-permissions).

## Reading generated metadata

`explorer::{Explorer, Description}` is an additive service-author API behind the
optional `explorer` feature. `Explorer::{new, path, title, allow_remote,
endpoint_guard, read_only}` configure the local-development endpoint. Generated
bindings own `Description` construction through `__generated::explorer_description`;
the public struct is non-exhaustive. The build API's `ServerOutput::explorer`
emits `DESCRIPTION`, defaults to Internal audience, and honors `audience` without
changing server operations.

`endpoint::{EndpointGuard, EndpointAdmission}` is shared by dedicated endpoints.
The original `mcp::{EndpointGuard, EndpointAdmission}` paths remain available.
`crucible_auth::EndpointAuthenticator::new` adapts an `Authenticator` with an
explicit scheme and rejects unsupported schemes at construction.

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
error writers, and generated MCP descriptions use
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
