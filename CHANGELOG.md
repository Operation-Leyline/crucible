# Changelog

All notable changes to Crucible are recorded here, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Crucible is `0.x`:
a minor release may break the documented stable surface only with a
**Migration** entry and a `#[deprecated]` shim kept for one minor (a removal
may skip the shim only when its Migration entry names it); a patch release
never breaks it. See the
[stability policy](docs/releases.md#stability-policy-for-0x). Step-by-step
upgrade instructions live in [docs/migrating.md](docs/migrating.md).

`tools/crucible_release.py prepare|check` refuses a version that has no
`## [X.Y.Z]` section below. Entries under *Unreleased* move into the
version's section when it is prepared.

## [Unreleased]

### Added

- Stability policy for 0.x, this changelog, `docs/migrating.md`, the
  `consumers` CI job (starter, example service and a Leyline-shaped fixture)
  and a release gate that runs it before a tag is created.
- Model Context Protocol: `.mcp()` generates static tool descriptors and
  `Builder::mcp(McpProtocol::new(service::MCP_TOOLS))` serves stateless MCP
  Streamable HTTP through the normal pipeline (#175).
- Smithy `@examples` is validated and lowered into the IR, and OpenAPI output
  emits native `examples` (#176). Invalid examples are now diagnostics.
- `crucible.auth#scopes` model-declared OAuth scopes, `Operation::SCOPES`
  metadata, the opt-in `crucible_auth::ScopeEnforcer` interceptor and
  scope-aware bearer challenges (#178).
- `@deprecated`, `@since`, `@unstable` and `@externalDocumentation` are lowered
  into the IR and rendered by every native generator and OpenAPI (#179).
- `crucible_build::model(source)`: one entry point for Smithy directories, JSON
  ASTs and model dependencies, with `ServerOutput`, `ClientOutput`, publication
  and named outputs hanging off it (#181).
- A hidden `__generated` boundary for generated Rust and `check_abi!`, so a
  `crucible-build` and runtime version mismatch fails with a named compile
  error; the stable runtime surface is documented in `docs/stability.md`
  (#182).

### Changed

- The server serves HTTP on hyper directly and dispatches with fewer
  allocations; `Crucible` implements `tower::Service` and `http_on_until`
  accepts a caller's shutdown signal (#177).
- **Breaking:** `OperationsBuilder::add::<Op, S>(impl, |s, input| Box::pin(..))`
  becomes `add::<Op, S, _>(impl, |s, input| async move { .. })`, and the `Call`
  alias is removed. Generated code is updated; see *Migration* (#177).
- **Breaking:** direct client bindings are written to `crucible_client.rs`;
  `include_client!()` follows (#181).
- Selected metadata enums, structs, logging records and `BuildError` are
  `#[non_exhaustive]`; the traits package and model JAR take the workspace
  version and `smithy/version.txt` is gone (#182).

### Deprecated

- `crucible_build::smithy`, `model_from_json`, `rust_service_from_smithy`,
  `client`, `client_from_smithy`, `publish_model`, `RustCodegen`,
  `ModelBuilder` and the `outputs` namespace, since 0.5.0; use
  `crucible_build::model(..)` (#181).

### Removed

- The duplicate `crucible_build::clients` namespace; `Audience` is only
  `crucible_build::Audience` (#181).

### Migration

- Hand-written operation registrations: pass the async fn directly to `add`
  and drop `Box::pin` (#177).
- Build scripts: replace the deprecated helpers with
  `crucible_build::model(..).server()|client()|publish()` (#181).
- Generated code is no longer compatible across Crucible versions: keep
  `crucible-build` and both runtime crates on the same version (#182).

## [0.4.0] - 2026-10-08

Mostly correctness: wire, validation and generator regressions found by the
conformance suites.

### Added

- Smithy `@cors` is lowered into the IR and served by the Rust server.

### Changed

- Generated Rust SDKs expose models and modeled exceptions beside the client
  (`weather_sdk::GetCityInput`), not under `model::` and `errors::`.
- Stricter wire behavior: timestamp text grammar and year bounds, ECMA-262
  `@pattern` semantics, float bindings and overflow, HTTP routing and JSON
  content types, HTTP-date header lists, repeated response headers.
- Name collisions in generated code are diagnosed and bare identifiers are
  qualified; panics in operations are recovered and Lambda stages are routed
  by name.

### Migration

- Replace `weather_sdk::model::X` and `weather_sdk::errors::X` with
  `weather_sdk::X`, and regenerate checked-in SDKs and service CLI packages.
  See [flat SDK imports](docs/migrating.md#030-to-040).
- Services that depended on lenient parsing now get errors for malformed
  timestamps, patterns and HTTP framing; see the fixes listed above and
  the per-protocol notes in [docs/framework.md](docs/framework.md).

## [0.3.0] - 2026-10-07

The second redesign: crates are named for their role, and builds become
one `crucible_build` vocabulary for services, clients and named outputs.

### Added

- `crucible new` scaffolds a single-crate service or a client; services
  generate bindings in `build.rs` and include them with
  `include_service!()`, which declares a module named for the service.
- Named build outputs from one compiled model (`crucible_build::smithy(..)`
  and fluent helpers for every language, document, CDK, Terraform and CLI
  target), with complete Rust Cargo SDK and Java Gradle packages.
- Build-time API policy and old/new contract compatibility checks
  (`crucible-policy`, `crucible.toml`).
- JSON AST and Smithy source diffing, OpenAPI 3.1 output, AWS JSON and
  Smithy RPC v2 JSON/CBOR servers, shared service exceptions in every SDK.
- `crucible-traits`: canonical Smithy traits as a Cargo package and model JAR.

### Changed

- `crucible` is now `crucible-server-runtime-rust` and `crucible-client` is
  now `crucible-client-runtime-rust`; generated server code exposes flat
  service APIs.
- `crucible_build::service` is `rust_service` and `Codegen` is `RustCodegen`.
- Build helper modules and types are named for outputs, not SDKs
  (`Output`, `CdkOutput`, `export_artifact`).

### Deprecated

- None. The renames above shipped without shims; 0.4 and later follow the
  one-minor shim policy.

### Removed

- The `crucible` and `crucible-client` crate names, `crucible_build::service`
  and `Codegen`.

### Migration

- Rename dependencies and imports to the new crate names, use
  `rust_service(..)`/`RustCodegen`, update `include_service!()` users (the
  generated module is named for the service, not `contract`), and pin the
  runtime and `crucible-build` to the same tag. See
  [0.2 to 0.3](docs/migrating.md#020-to-030).

## [0.2.0] - 2026-10-06

First tagged release, and the first redesign: `feat!: define middleware and
interceptor pipeline`.

### Added

- Phased request pipeline: `Middleware`, `OperationAwareMiddleware`,
  `Interceptor`, `TypedInterceptor`, `Protocol` and exception translators,
  registered on `Crucible::builder(..)`.
- Rust payloads, media-type headers, AWS JSON protocols and checksums.
- Stricter wire validation, recursive `MemberInfo::redaction` metadata and
  `OperationInfo::literal_query`.

### Changed

- Handlers no longer exist; protocols decode through `Protocol::decode`.
- `crucible_wire::text::percent_decode` returns a `Result`; invalid bytes are
  no longer replaced with U+FFFD. `LogSink` receives `&LogRecord<'_>`.

### Removed

- The `handlers` module, `Handler`, `AsyncHandler`, `OperationHandler` and
  `.layer(..)` (use `.tower_layer(..)`).

### Migration

- Move each extension to the phase that matches the data it needs, adapt
  protocols and regenerate contracts. See
  [0.1 to 0.2](docs/migrating.md#010-to-020).
