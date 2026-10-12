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

- `crucible-lsp` hover explains the language and shows whole shapes. Every
  keyword, built-in type, constraint and built-in annotation explains itself
  with a short example, by where it is written (`on` attaches an operation,
  but lists a metadata key's targets). A declaration, or a name that refers
  to one, shows its source form with its constraints: a list or map also
  shows its item, key and value types, and the constraints of a scalar, list
  or map are spelled out in words. Structures and enums show their source
  form (members with their types, optionality, defaults and member
  constraints; values). A member's hover shows its type, whether it is
  required, its default, and spells out the constraints that apply to it,
  its own laid over its type's. Code is fenced as `crucible`, so editors,
  the IntelliJ plugin among them, highlight it; prose stays outside.
- `.cru` resources declare property mutability with keywords (decision
  34a): `readonly`, `immutable`, `writeonly` and `immutable writeonly`.
  Lowering writes it to `ir::Member::mutability` on every member that
  carries the property's value (views, outputs, and the inputs of
  `creates`, `puts` and `updates`), and the compiler checks bindings
  against it: an `updates` input cannot write a `readonly` or `immutable`
  property, a `creates` or `puts` input cannot write a `readonly` one, an
  `immutable` property needs a `creates` or `puts` that sets it, a
  `writeonly` one never appears in a view or output, and a `@mutability`
  annotation on a member that binds a property must agree with its
  keyword. A property nothing updates that is left read-write is a
  warning.
- `$*` in a `for` view (decision 34d) expands to the resource's own
  identifiers and every property that is not `writeonly`, in declaration
  order; `$name?` after it loosens one entry for that view.
- A property declares its presence, `property lastMessageAt?: DateTime`
  (decision 34e): optional in every view and output without a `?`, while
  inputs keep per-site presence.
- An identifier names its spelling in its resource's own views and
  outputs, `id inboxId: InboxId as id` (decision 34f); inputs still bind
  it as `inboxId`.
- Two warnings for a resource modelled beside its representation
  (decision 34h): a resource with `reads`, `lists` or `creates` operations
  whose outputs show none of its properties, and a structure without `for`
  that restates a resource's identifier and a property.

### Changed

- **Breaking (`.cru`):** a child resource inherits its ancestors'
  identifiers, not their properties (decision 31). A property name now
  only has to differ from the resource's identifiers, inherited ones
  included, and from its own other properties, so a parent and a child may
  both declare `name` or `createdAt`; `$name` in a child's operation or
  view no longer finds an ancestor's property and reports that it is not
  inherited.
- `ir::Member::mutability` is no longer only an explicit override: from a
  `.cru` package it is the declared mutability of the property the member
  carries.

- **Breaking:** generated Rust client models, enums, unions and per-operation
  error enums are `#[non_exhaustive]`. Code in another crate builds models
  with their builders, destructures with `..` and matches with a `_` arm.
- The `compatibility.sdk` check no longer reports a new optional member (not
  required, no default) added after the existing members of a structure that
  is not an error: no generated client turns it into a source change, so a `.cru` package adds
  one without a migration. One inserted between existing members is still
  reported (it shifts positional constructors in C, C++, PHP and R), as are
  required and defaulted members and union arms.

### Migration

- **Rust client models.** Replace struct literals of generated client types
  with builders (`GetCityInput::builder().city_id(id).build()`), add `..` to
  exhaustive destructuring, and add a `_ =>` arm to matches on generated
  enums, unions and operation error enums. No shim is possible: the attribute
  is on the generated type. Bindings included with `include_client!()` live
  in the including crate and are unaffected.

## [0.5.0] - 2026-10-11

Crucible's own modelling language (`.cru` packages, compiled without Smithy
or a JVM) with the `crucible-lsp` language server; frozen releases, the
compatibility guard and serving older API versions; a stable build and
runtime contract (`crucible_build::model()`, the hidden `__generated`
boundary and a compile-time version check); MCP, JWT/OIDC auth, service API
docs and the Crucible Explorer. Breaking: follow *Migration* and
[docs/migrating.md](docs/migrating.md).

### Added

- `crucible-lang`: an internal front end for Crucible's own modelling language
  (`docs/idl/`), lowering `.cru` packages to the same IR the Smithy front end
  produces. The weather model compiles to identical IR from both. Not yet a
  supported surface.
- `crucible_build::model(..)` accepts a `.cru` package directory, one whose
  `crucible.toml` has a `[model]` table, and compiles it without Smithy or a
  JVM. Server bindings, clients, OpenAPI and the service CLI generate from it;
  documentation, provisioning, the API Explorer and model export report an
  error for a `.cru` model for now.
- `crucible-lsp`, a language server for `.cru` files: the compiler's
  diagnostics as you type across the whole package, context-aware completion,
  semantic highlighting, go to definition, find references, rename,
  highlighting of other uses, hover, document and workspace symbols, folding,
  formatting and quick fixes for unknown types. It also checks `crucible.toml`.
  Releases attach it for Linux, macOS and Windows; the separate
  `crucible-intellij` repository bundles it into the JetBrains plugin.
- `crucible-lsp` editor insight: semantic token modifiers for the declaration
  kinds a token type cannot tell apart (`service`, `protocol`, `resource`,
  `error`, `union`, `rule`), on declarations and every reference; inlay hints
  for the type of an elided `$member` and the implicit value of a string enum
  value; usage-count code lenses (command `crucible.showReferences`, for
  clients that pass `{ "showReferences": true }` as initialization options
  and run it); go to
  type definition from a member's name; links in comments and strings;
  signature help for annotations and `paginated(..)`; and range formatting.
- `crucible-lsp` searches the workspace folders for packages in the
  background (12 levels and 20,000 directories per folder at most, skipping hidden,
  `target`, `node_modules`, `build`, `out` and `dist` directories), compiles
  them between requests and keeps them compiled, so workspace symbols,
  references and rename work with no file open. It registers file watchers
  and recompiles only the packages a change outside the editor touches.
- A `.cru` package's `crucible.toml` can hold its API policy beside `[model]`;
  builds of the package and `crucible check <package>` apply it without
  `.policy(..)`. `schema_version` is now optional.
- Every policy check can set its own severity: `members = { case =
  "camelCase", severity = "warning" }`, and likewise `pattern`, `path`,
  `target` and `enabled` for the other checks. Bare values keep the group's
  severity, so existing files are unchanged. Messages now say what is wrong
  ("operation name `createGraph` is not PascalCase").
- `crucible-lsp` runs the package's policy on every compile and reports each
  finding at the name it concerns, with a rename to the configured case as a
  quick fix, even while the package has other errors (`Compiled::best_effort`
  keeps what lowering made of it). In `crucible.toml` it completes every
  table, key and value,
  explains them on hover, and reports policy errors where they are written;
  an unknown top-level key is an error.
- The compatibility guard for `.cru` packages: `[model.baseline] path`
  names the last release, frozen by `crucible freeze <package>` as
  `releases/<version>.json`: a snapshot of the compiled model (canonical
  JSON with a `format` version), read back without recompiling and
  checksummed in `releases/crucible.lock`, so a build refuses an edited
  release. `crucible freeze <package> --from <git ref>` backfills a release
  from the package as it was at a commit or tag (docs/api-policy.md,
  "Releases"). The IR (`crucible_smithy::ir`) is serializable, and
  `crucible_smithy::snapshot` reads and writes release snapshots. Builds, `crucible check`,
  `crucible generate` and `crucible-lsp` compare the package against it with
  the `crucible diff` checker and the manifest's `[compatibility]` settings;
  a breaking change fails the build and shows in the editor where it was
  made. A directory with its own package manifest is never collected into
  its parent package.
- API versions in the IR: `ir::Service::versions` lists the older versions a
  `.cru` service still serves, oldest first, each with its release's model,
  its lowered migration steps (`ir::VersionStep`) and its sunset, from the
  chain of migrations back from the service's `version`
  (`crucible_lang::versions`; docs/api-versions.md). A release is named by
  its service's `version`: `crucible freeze` writes `releases/<version>.json`,
  a release's file name must match its version, and the
  newest migration must serve the current version.
- Serving older API versions (docs/api-versions.md): a Rust service whose
  `.cru` model has migrations serves every version in the chain from its
  current implementation. A request names its version in the
  `Crucible-Version` header; it is routed, decoded and validated with that
  release's types, its input converted up to the current version, and its
  output or modelled error converted back. The generated `versions` module
  holds each older release and the `ApiVersion` constants;
  `Context::api_version()` is the version a request is served at.
  `Builder::api_version(Unversioned)` settles a request without a version:
  the oldest supported (the default), a resolver's choice, or refused.
  Responses echo the version, with a `Sunset` header when its migration
  declares a `sunset`. MCP tool calls and direct invocations are served at
  the current version. Operations with a stream are served at the current
  version only. An `add ..
  downgrade` step must fill every member the older error requires.
- Generated clients send their release (docs/api-versions-clients.md):
  every client of a `.cru` service names the release it was generated for
  in the `Crucible-Version` header, from the first release. It covers the
  12 bundled runtimes, TypeScript, the Rust client (a defaulted
  `crucible_wire::operation::Operation::API_VERSION` the generated marker
  sets) and the service CLI. Smithy-sourced clients send nothing.
  - **Diagnostics:** a `.cru` service's `version` that cannot travel in a
    header is a diagnostic, and so is an `@header` binding named
    `Crucible-Version`.
  - **Migrations:** every change of `version` against the baseline needs
    a migration, empty when nothing breaks, so every release a client
    names stays served.
  - **CORS:** with `@cors`, the header is allowed and exposed from the
    first release. CORS configured outside the model must allow it too.
  - **Servers:** a `.cru` server without migrations now knows its one
    release. It refuses any other with `400`, echoes `crucible-version`
    with `Vary`, applies `Builder::api_version`, and
    `Context::api_version()` returns it. Freeze a release as the baseline
    before publishing its clients: changing `version` without one refuses
    every client generated before.
- Migration hooks (docs/api-versions.md#hooks): `reshape`, `retype` and
  `require .. by hook` steps and `remove operation` generate a
  `versions::<older>::Migration` trait per migration, whose methods convert
  a value across that migration (`by async hook` for one that does I/O) or
  serve a removed operation at the versions that had it.
  `operations(implementation)` requires every migration's trait; a split
  builder registers them with `<older>_migration(..)` or `migrations(..)`,
  and `build()` names a missing hook. A `retype` without `by hook` is
  reported until declared `conversion`s are generated.
- Declared migrations: a breaking change against the baseline compiles when
  `migration "<new>" from "<baseline version>"` declares it with `rename`,
  `require`, `add`, `move`, `reshape`, `retype` or `remove operation`, and
  every step must match a change that happened (`crucible_lang::guard`).
  Builds, the CLI and `crucible-lsp` share the check; the editor shows an
  undeclared change where it was made and a mismatched step at the step.
- `crucible check` and `crucible generate` accept a `.cru` package directory
  as the model, compiled without Smithy or a JVM, as `crucible_build` does.
  Documentation and infrastructure targets (`docs`, `cdk`, `terraform`)
  report that a `.cru` model is not supported yet.
- `crucible_lang::format`, a conservative `.cru` formatter: indentation by
  bracket depth, trailing whitespace and blank-line runs; blank lines at the
  start and end are dropped and the file ends with one newline, even when it
  does not lex. `format_lines` formats line by line for a selection.
- The IR carries typed facts generators previously read from raw Smithy
  traits: `Structure::strict` and `input`, `Member::idempotency_token` and
  `mutability`, `sensitive` on members and shapes, and `Service::is_bearer`.
  Generated output is unchanged. An invalid `@aws.cloudformation#cfnMutability`
  value is now a lowering diagnostic instead of being ignored.
- Crucible Explorer: optional `explorer` runtime feature and
  `.server().explorer()` build output, serving an embedded API reference with
  schema forms, JSON calls, credentials, curl copying and pagination. Defaults
  to loopback admission and Internal audience; supports every HTTP host,
  explicit remote/custom admission and read-only UI configuration. Service
  starters enable the endpoint only in debug builds; see [Explorer](docs/explorer.md).
- Shared `endpoint::{EndpointGuard, EndpointAdmission}` admission interface and
  `crucible_auth::EndpointAuthenticator` adapter. Existing MCP imports continue
  to work.

- Native service API references: `crucible generate docs`, named `.docs(...)`
  build outputs with audience selection, mdBook Markdown pages, and the stable
  `crucible.describe/1` description contract with shared JSON Schemas. External
  projection also removes internal nested example members and diagnoses examples
  selecting internal union arms or enum values.
- MCP protected-resource discovery derived from configured access-token issuers,
  the public MCP URL and model scopes, with path-suffixed RFC 9728 metadata and
  scope-aware bearer challenges on endpoint and tool refusals. Additive
  `McpConfig::authorization` and `crucible_auth::McpAuthenticator` require audience
  binding by default and offer explicit Cognito compatibility for unbound tokens.
- Static single-page HTML API reference in every docs output: `index.html`,
  `explorer.js`, `explorer.css`, `describe.json`, and `describe.js` (for
  `file://`). The renderer is dependency-free and has a
  `CrucibleExplorer.registerOperationPanel` extension hook; see
  [service API references](docs/docs.md#static-html-reference).

- `McpConfig::authorization_server_metadata(AuthorizationServerMetadata)`: an
  opt-in, public RFC 8414 document that advertises `S256` and points at an
  identity provider's endpoints (`AuthorizationServerMetadata::cognito`), for
  MCP clients that refuse Cognito's own discovery metadata. Derived
  protected-resource metadata then names the service as the authorization
  server. Cognito client setup is in `docs/mcp.md`.

- Stability policy for 0.x, this changelog, `docs/migrating.md`, the
  `consumers` CI job (starter, example service and a Leyline-shaped fixture)
  and a release gate that runs it before a tag is created (#180). The job
  builds with `-D deprecated`, so in-repo consumers stay on the current API.
- Model Context Protocol: `.mcp()` on the server output generates static,
  sorted tool descriptors from the External audience projection, and
  `Builder::mcp(McpProtocol::new(service::MCP_TOOLS))` serves stateless MCP
  Streamable HTTP (`/mcp`, protocol versions 2025-03-26, 2025-06-18 and
  2025-11-25) through the normal pipeline, with endpoint admission so
  authentication can protect discovery. Streaming operations are excluded with
  an explicit `MCP_EXCLUSIONS` note, and tool names over 128 characters are a
  collected diagnostic (#175).
- Smithy `@examples` is validated against the operation's shapes and lowered
  into the IR, and OpenAPI output emits native `examples` (#176). Invalid
  examples are now diagnostics.
- `crucible.auth#scopes` (`smithy/auth.smithy`, also in the traits package and
  model JAR) declares OAuth scopes on a service or operation. Server
  operations carry `Operation::SCOPES` and `SERVICE_SCOPES` (also on
  `OperationMetadata`); `crucible_auth::ScopeEnforcer` is an opt-in
  interceptor that enforces them with RFC 6750 bearer challenges (#178).
- `@deprecated`, `@since`, `@unstable` and `@externalDocumentation` are lowered
  into the IR and rendered by every native generator and OpenAPI (#179).
  Generated Rust marks deprecated shapes `#[deprecated]` and silences the
  warnings inside the generated module only, so code that uses a deprecated
  shape now sees the warning.
- `crucible_build::model(source)`: one entry point for Smithy directories, JSON
  ASTs and model dependencies (`links` names). `.server()`, `.client()` and
  `.publish()` hang off it beside the named outputs, and compiling happens once
  per handle. A missing model link is a typed `MissingModelDependency` with the
  Cargo setup to add (#181).
- A hidden `__generated` module in both Rust runtimes for everything generated
  code needs, and `check_abi!`, emitted into every generated server binding
  and Rust SDK, so a `crucible-build` and runtime version mismatch is a named
  compile error. The stable runtime surface is documented in
  `docs/stability.md`. `OpenEnum` (`from_string`, `as_str`) is the documented
  way to build and read client string enums (#182).
- Generated service CLIs include a local `mcp` stdio server using shared MCP
  schemas and registry, client audience, profiles, authentication and SDK calls
  (#185).
- `crucible-auth`: opt-in bearer authentication for Rust services, with
  `JwtAuthenticator` (JWT issuers verified against their JWKS),
  `AuthenticatorChain`, `ScopeEnforcer` and typed refusals across the server
  protocols; see `docs/auth.md` (#168).
- Smithy `@internal` is lowered: operations, members, error variants and enum
  values carry an internal flag, `OperationInfo::internal` exposes it, and
  outputs select an audience (#166).
- Generated clients in every language carry API documentation from the model
  and readable string representations that redact `@sensitive` values
  (#169, #170, #171, #172).
- Smithy `httpMalformedRequestTests` are lowered into the protocol-test IR,
  and `.conformance()` on the server output generates conformance tests from
  `smithy.test` vectors (#167).
- Legacy `@enum` string shapes are lowered as enums, with diagnostics for
  duplicate, unnamed or malformed definitions (#163).

### Changed

- Generated clients default to the external audience and omit `@internal`
  API; select `Audience::Internal` (or `--audience internal`) for the full
  contract (#166).
- A behavior trait Crucible does not implement is a diagnostic instead of
  being kept silently as a raw trait (`@smithy.rules#endpointBdd`), so a model
  that used one no longer builds (#163).
- Smithy lowering refuses more models that HTTP cannot carry, through rules
  now shared with the `.cru` front end (`crucible_smithy::validate`): a body
  on a HEAD, 204 or 304 response; framing and hop-by-hop headers (`Host`,
  `Content-Length`, `Transfer-Encoding` and the like) and `Authorization`
  under bearer auth as modelled headers; header names that are not HTTP
  tokens; an operation that lists the same error twice. Pagination members
  must have the right types (`items` a list or map, `pageSize` an integer),
  and pagination paths must be identifiers. Several existing messages are
  reworded.
- `ir::EnumValue` and `ir::IntEnumValue` carry their own `traits`, so
  metadata on a single enum value reaches generators.
  `ir::Service::project_metadata_only` keeps only project metadata in every
  raw `traits` map.
- `crucible-lang`: diagnostics have a severity. Warnings (a plain member that
  shadows a resource entry, an operation on a resource the service does not
  reach, a member constraint that loosens its type's, a dangling `[Name]`
  doc link, an orphan `///`) never withhold the IR. The compiler now checks
  roles and resources (decisions 31 to 33), binding placement, idempotency
  tokens, defaults, constraint applicability, map keys, metadata declarations and values, and duplicate
  clauses, and reports problems at the member or name they concern. The
  parser recovers from unclosed brackets and deep nesting without layout
  rules. `[model.dependencies]` and `[model.baseline]` are reported as not
  supported yet.
- `crucible-lang`: an `updates` operation is no longer implied `idempotent`
  (decision 32). Smithy's update lifecycle does not require it, and an
  update that, say, creates a version per call is not safe to retry. Write
  `idempotent operation .. updates ..` where it is; an `updates` on PUT
  without it is now the PUT method error. The language is not yet a
  supported surface, so there is no shim.
- `crucible-lang`: a structure with `@query`, `@header`, `@payload` or
  `@token` members can be `include`d into operation inputs (decision 40);
  its bindings are checked where it is included or used as a type, not on
  the structure alone. The "did you mean `$name`?" warning no longer fires
  for a member of another type than the resource entry, or for a child
  identifier a collection operation may not bind (decision 33). Package
  file paths drop the `.` of `sources = ["."]`.
- The server serves HTTP on hyper directly and dispatches with fewer
  allocations; `Crucible` implements `tower::Service`, `into_router()` still
  returns an Axum `Router`, and `host::http_on_until` accepts a caller's
  shutdown signal (#177).
- **Breaking:** `OperationsBuilder::add::<Op, S>(impl, |s, input| Box::pin(..))`
  becomes `add::<Op, S, _>(impl, |s, input| async move { .. })`, and the `Call`
  alias is removed. Generated code is updated; see *Migration* (#177).
- **Breaking:** the Rust binding builders are `ServerOutput` and `ClientOutput`
  (`RustCodegen` is a deprecated alias of `ServerOutput`), and direct client
  bindings are written to `crucible_client.rs` instead of
  `crucible_client_runtime_rust.rs`; `include_client!()` follows (#181).
- **Breaking:** generated Rust names the runtimes only through `__generated`
  (the old `__private` module is gone) and no longer constructs
  `OperationInfo`, `ServiceInfo`, `MemberInfo` or `ErrorInfo` literally.
  Regenerate; hand-written code is unaffected unless it imported codecs or
  error writers (`codec`, `cbor`, `open`, `field`) from a runtime root (#182).
- **Breaking:** `OperationInfo`, `ServiceInfo`, `MemberInfo`, `ErrorInfo`,
  `Location`, `Scalar`, `Segment`, `Fault`, `AuthScheme`, `operation::Protocol`,
  `BuildError`, `LogRecord` and `RequestRecord` are `#[non_exhaustive]`:
  exhaustive matches need a `_ =>` arm (#182).
- The `crucible-traits` Cargo package and model JAR are published at the
  workspace version (`crucible-traits-X.Y.Z.jar`, tag `traits-vX.Y.Z`) instead
  of `smithy/version.txt` (`0.3.1`), which is removed (#182).

### Deprecated

- `crucible_build::smithy`, `model_from_json`, `model_dependency`,
  `compile_smithy*`, `publish_model*`, `rust_service*`, `service`, `client`,
  `client_from_smithy*`, `RustCodegen`, `ModelBuilder` and the `outputs`
  namespace, since 0.5.0; use `crucible_build::model(..)` (#181). They are
  removed no earlier than the next minor.

### Removed

- The duplicate `crucible_build::clients` namespace; `Audience` is only
  `crucible_build::Audience` (#181).
- The `crucible_server_runtime_rust::{cbor, codec, open}` and
  `crucible_client_runtime_rust::{cbor, codec, field, open}` root re-exports;
  generated code reaches them through `__generated` (#182). `OpenEnum`,
  `UnknownUnion`, `Field` and `ModelledError` stay public at the crate root.

### Security

- `crucible-auth` moves to `jsonwebtoken` 11.1 (pure-Rust `rust_crypto`
  backend), past GHSA-h395-gr6q-cpjc, where a wrongly typed standard claim was
  treated as absent. Crucible already required `exp`/`iss`/`sub` and checks time
  and audience claims itself with typed reads; new tests pin that wrongly typed
  `exp`, `nbf`, `aud`, `iss` and `sub` are refused. `crucible-auth` now needs
  Rust 1.88. `crucible_auth::Algorithm` is re-exported from the new version with
  the same variants (#192).

### Migration

From 0.4 to 0.5. The same steps, with before and after code, are in
[docs/migrating.md](docs/migrating.md#040-to-050).

- **Version everything together.** Pin `crucible-build`, `crucible-server-runtime-rust`,
  `crucible-client-runtime-rust` and `crucible-auth` to one tag or revision.
  `check_abi!` means exactly that: generated code records the version of
  `crucible-build` that wrote it and fails to compile against a runtime of a
  different version.
- **`build.rs`.** Replace the deprecated helpers with
  `crucible_build::model(..)` (the shims still build for one minor):

  ```rust
  // 0.4                                          // 0.5
  crucible_build::publish_model("m.json");        crucible_build::model("m.json").publish();
  crucible_build::rust_service("m").generate();   crucible_build::model("m").server().generate();
  crucible_build::client("m").generate();         crucible_build::model("m").client().generate();
  crucible_build::smithy("smithy").compile();     crucible_build::model("smithy");
  ```

  A client include that named `crucible_client_runtime_rust.rs` by hand now
  names `crucible_client.rs`; `include_client!()` needs no change.
- **Regenerate.** Everything under `__generated` is generated-code-only API
  with no compatibility promise: rebuild so bindings, SDKs and checked-in
  generated packages are regenerated. Do not call it from hand-written code;
  the supported replacements are `OpenEnum`, `UnknownUnion` and `Field`.
- **`#[non_exhaustive]` matches.** Add a `_ =>` arm to any `match` on
  `Location`, `Scalar`, `Segment`, `Fault`, `AuthScheme`, `operation::Protocol`
  or `BuildError`, and read `OperationInfo`, `ServiceInfo`, `MemberInfo`,
  `ErrorInfo`, `LogRecord` and `RequestRecord` by field rather than building them.
- **Operation registration.** Pass the async fn directly to `add` and drop
  `Box::pin`; the `Call` alias is removed without a shim (#177).
- **Removed without a shim.** `crucible_build::clients` is gone: import
  `crucible_build::Audience` (#181). The runtime root re-exports
  `crucible_server_runtime_rust::{cbor, codec, open}` and
  `crucible_client_runtime_rust::{cbor, codec, field, open}` are gone:
  regenerate, and use `OpenEnum`, `UnknownUnion`, `Field` and `ModelledError`
  from the crate root in hand-written code (#182).
- **Client audiences.** A client that used `@internal` operations or members
  needs `.audience(crucible_build::Audience::Internal)` (or
  `--audience internal`) (#166).
- **Traits JAR.** Resolve `crucible-traits` at the Crucible version you build
  with (`crucible-traits-X.Y.Z.jar`), not `0.3.1`; a repository that read
  `smithy/version.txt` should read `[workspace.package].version` (#182).

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
