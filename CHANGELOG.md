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

- `crucible_auth::ApiKeyAuthenticator`: bearer API keys checked against a
  service-owned `ApiKeyStore` (`lookup(digest)` returning an `ApiKeyRecord`
  of key ID, tenant, `ApiKeyState`, optional expiry and attributes). It
  claims only credentials with its prefix that pass an optional
  `well_formed` check, so it sits in an `AuthenticatorChain` beside
  `JwtAuthenticator` and a JWT costs no read. It reads `Authorization:
  Bearer` by default, or an ordered `credential_headers` list, with
  `allow_bare` for scheme-less keys. Valid keys are cached for 5 minutes
  from the read (`valid_ttl`; zero turns that cache off alone, for
  immediate revocation) with one background `refresh_ahead` re-read near
  the end, and an answer older than the TTL when it arrives is read again;
  refused keys go in a separate LRU with backoff from 2 s to a 60 s ceiling
  (`refusal_backoff`), a dead row straight to the ceiling. Concurrent misses
  share one read and reads are bounded (`lookup_concurrency`). `forget`
  drops a revoked key on this instance; `fence` answers every key with a
  5xx while a replica is unhealthy; `last_used` hands
  an `ApiKeyUsageRecorder` each key's use at most once per
  `last_used_resolution`. A store error is a sanitized 5xx, never a refusal.
  It serves the bearer scheme only, not `@httpApiKeyAuth`.
  `api_key_digest(credential)` is the lowercase hex SHA-256 lookup key, for
  minting. `test_support::InMemoryApiKeyStore` stands in for the table in
  service tests. See [docs/auth.md](docs/auth.md#api-keys).

- `Principal::tenant`: the workspace or organization a credential is bound
  to. API keys fill it from their record; ID and access tokens leave it
  `None`, and no token claim sets it. `Principal::new(subject, issuer,
  token_kind)` with `with_client_id`, `with_scopes`, `with_groups`,
  `with_claims` and `with_tenant` construct one.

- Sealed pagination tokens, behind the runtime's `page-tokens` feature.
  `PageTokens` is an `Interceptor` that opens the token a `@paginated`
  request carries before the operation runs, and the operation reads its own
  cursor with `page_tokens::resume()` and seals the next one with
  `page_tokens::issue(&cursor)`. Tokens are XChaCha20-Poly1305 under keys
  derived from a secret (`PageKeys`, with retired keys accepted for
  rotation), bound to the service, operation, caller and filters, and expire
  after 24 hours (`ttl`). Nothing is stored. A token that does not open is
  the service's validation error naming the token member, or what
  `PageTokens::refusal` builds. Generated server operations carry
  `Operation::PAGINATION`, the token and page size members, also on
  `OperationMetadata::pagination`. See
  [docs/framework.md](docs/framework.md#pagination-tokens).

- Server-side idempotency (client tokens), behind the runtime's
  `idempotency` feature. `Idempotency::new(store, caller)` is an
  `Interceptor` for every operation whose input has an idempotency token:
  a request without one, or with one over 256 characters or outside visible
  ASCII, is the validation error naming the member (unless
  `optional_tokens`, for a missing one); the token is claimed atomically in
  the service's `IdempotencyStore` (`claim`, then `hold`, `complete` and
  `release`, which answer `Ownership::Held` or `Lost`), keyed by service,
  operation, caller binding and token, with a SHA-256 `Fingerprint` of the
  canonical decoded input without its token. A retry with the same input
  gets the first success re-encoded for its own protocol, API version and
  request id, without the operation running; the same token with other
  input, or while the first request runs, is a 409, and so is a stored
  response from before a deploy that changed it. A failure releases the
  claim so a retry runs again, unless the operation called
  `idempotency::commit()` before a step that is not safe to repeat, after
  which the claim is held for the retention; `commit` refuses an attempt
  that outlived its claim. Successes replay for 24 hours (`retention`); an
  unsettled claim lapses after 15 minutes (`lease`), or a minute past the
  request's deadline. A success that cannot be stored (the write failed, or
  it is over `max_stored_response`, 256 KiB) is held for the retention
  instead. `refusal_factory` maps an `IdempotencyRefusal` (`Missing`,
  `Invalid`, `Mismatch`, `InFlight`, `Lapsed`, `Unreplayable`) to the
  service's own errors. Store errors are sanitized 5xx answers; a failed
  completion write never turns a success into an error. Operations with a
  streaming input or output are refused at build (leave them out with
  `.except`), and an older API version without the token runs unclaimed.
  `MemoryIdempotencyStore` (`test-support`) stands in for the table in
  tests. See
  [docs/framework.md](docs/framework.md#idempotency-tokens).

- `idempotency::abandon()` takes back `idempotency::commit()` when the step
  after it provably did not happen (the provider refused explicitly): a
  failure then releases the claim as if `commit` had never been called, so
  a retry with the same token runs the operation again instead of getting a
  409 for the retention. It changes only what a failure does (a success
  still completes and replays), makes no store call (the request's own
  completion releases the claim, and a process that stops first leaves it
  held as committed), does nothing without a prior `commit` or for an
  unclaimed request, and is an internal error without `Idempotency`. A
  `commit` after it commits again. See
  [docs/framework.md](docs/framework.md#idempotency-tokens).

- Typed permissions, the runtime side. `permissions` holds the static
  descriptions generated code will state, always present: `ResourceInfo`
  (with its fixed 404 message), `PermissionInfo` (with `Tier`, aliases and
  its fixed 403 message), `Requirements` (`Undeclared`,
  `None`, `Pending { reason }`, `List` of `RequirementInfo` with a `Mode`),
  `Obligation`, and the per-request `Check` on a `ResourceRef` (identifiers
  root to leaf, the leaf possibly `Target::All`) with its `Origin`.
  Generated operations carry them as `Operation::REQUIREMENTS`,
  `Operation::CHECKS` and `Operation::PUBLIC` (an operation the model
  explicitly makes public), also on `OperationMetadata` (`requirements`,
  `checks`, `public`), and a service its permission registry as
  `ServiceInfo::permissions` (`service_info(..).with_permissions(&[..])`). Behind the new `permissions` feature, `Permissions` is an
  `Interceptor` enforcing them through a service-supplied `Authorizer`
  (`decide` for a batch of checks, `visible` for a `filters` listing),
  which answers `Verdict::Allowed`, `Denied` or `Hidden`: the first refusal
  in declaration order is a 404 `NotFoundException` for Hidden and a 403
  naming the permission (never an internal one) for Denied, or what
  `refusal_factory` builds from a `PermissionRefusal`. An authenticated
  operation without requirement metadata is refused as a wiring error unless
  `allow_undeclared()`; `requires pending` passes; `shadow()` only records
  and logs. Handlers check what the model cannot bind with
  `permissions::require`, `decide`, `discharge` and `not_applicable` (for
  `by handler` obligations, each closed once; `idempotency::commit()`
  refuses an unsatisfied one in every build, and a success with one fails in
  debug builds, `assert_obligations`), read a listing's
  `permissions::visible()`, and report genuine absence with
  `permissions::not_found` (a handler's modelled `NotFoundException` is
  answered identically). Every request gets an `AuthorizationRecord` on its
  context for audit. Registered before `Idempotency` (the other order fails
  the build), a refused request never claims a token, and a replay is
  authorized again: its declared checks, and the handler checks the first
  success was allowed, which the idempotency record now keeps
  (`StoredResponse` gains an `authorization` field; a record without it
  replays with the declared checks alone). See
  [docs/framework.md](docs/framework.md#typed-permissions).

- Typed permissions, phase B: the generators. Rust server bindings of a
  model with permissions gain a `permissions` module: per resource a closed
  enum (`InboxPermission`, with `ALL`, `value`, `info`, `parse` accepting
  aliases, and each deprecated alias as a constant of its primary) and a
  typed reference (`InboxRef`, with `new`, `all`, `not_found`, and
  conversions to and from `ResourceRef`), `Permission` and `Resource` over
  all of them (`Permission::parse(resource, value)`, `of`, `from_info`),
  and the `ResourceInfo` and `PermissionInfo` statics with their 404 and 403
  messages. Each marker states `REQUIREMENTS`, a `CHECKS` function that
  binds every check and filter clause to the typed input (paths through
  optional members and union arms; `when` conditions), `PUBLIC` for an
  operation the model makes public, and a constant per `by handler` clause
  (`DeleteGrant::ADMIN_ON_WORKSPACE`); `SERVICE` carries the registry.
  Older API versions' modules state no requirements, except an operation a
  later release removed, whose legacy handler is authorized by its own
  release's requirements and `CHECKS`. `Permissions::undeclared_operations()`
  reports the operations the build found without requirement metadata,
  older releases' included. A `when` condition compares a number only as a
  value of its member's type, and never a `bigInteger` or `bigDecimal`
  member (`permissions.condition`). Rust and TypeScript
  clients get open permission types that keep unknown values (Rust
  `client::permissions::InboxPermission` with `UnknownValue` and
  `canonical()`; TypeScript `InboxPermission` constants,
  `InboxPermissionValue` and `canonicalPermission`), and `PERMISSIONS`, the
  registry with tier, `grantable` and aliases
  (`crucible_client_runtime_rust::permissions::{ResourcePermissions,
  PermissionDescription}`). Every client's operation documentation, MCP
  tool descriptions and the bundled runtimes' schema (`"permissions"`) carry
  the requirements and registry; no client output names an internal
  permission. The API reference gains a Permissions page (a table per tier)
  and an Authorization section per operation, and `describe.json`
  `service.permissions` and per operation `requirements` and
  `requirementSentences`; OpenAPI gains `x-crucible-requires` per operation,
  a requirement sentence in its description and `x-crucible-permissions`
  (internal entries only in an internal-audience document). The client
  projection drops a pending operation's reason, which is the server's
  migration note. See
  [docs/framework.md](docs/framework.md#typed-permissions).

- `FrameworkError::NotFound { resource }`: HTTP 404, the modelled
  `crucible.errors#NotFoundException` with the resource's `message` (`No
  such inbox.`) and `resourceType`, sent with `Cache-Control: no-store` (a
  tool error in MCP). `FrameworkError::PermissionDenied { permission }`:
  HTTP 403 `ForbiddenException` with the permission's message, or a generic
  one for an internal permission, without the `insufficient_scope`
  challenge `Forbidden` carries.

- `FrameworkError::Conflict { message }`: HTTP 409 `ConflictException`
  (`smithy.framework#ConflictException` in RPC v2, a tool error in MCP),
  what `Idempotency` answers by default without a modelled conflict error.

- `DecodedJob::replay_output(json)` ends an invocation with an output read
  from its JSON form, which the protocol encodes as the operation's own.
  Generated server operations with an idempotency token carry
  `Operation::IDEMPOTENCY_TOKEN` and, when the output does not stream,
  `Operation::OUTPUT_FROM_JSON` (an `operation::OutputFromJson`), also on
  `OperationMetadata` (`idempotency_token`, `idempotency_token_sent`,
  `replays_output`).

- Deadlines are enforced. A request is stopped at the host's deadline (a
  Lambda invocation's) less `DEADLINE_RESERVE` (250 ms), or at
  `Builder::request_timeout(duration)` after it arrived, whichever comes
  first. It then fails with the new `DeadlineExceeded` error: the phases
  already entered unwind, the exception rules apply, and the service log
  records the cause. Behind CloudFront (30 s origin timeout by default) or a
  gateway, set `request_timeout` below that timeout. Otherwise the caller
  gets the CDN's 504 while the function keeps running, billed, for nobody.
  See [docs/framework.md](docs/framework.md#deadlines).

- `.cru` models may declare recursive shapes: a structure or union that
  reaches itself through its members, directly or through lists and maps.
  They lower into the same IR as Smithy's, `recursive_groups` included,
  computed by the algorithm the Smithy front end uses. Smithy's limits on
  recursion are checked on the IR in `crucible_smithy::validate`, for both
  front ends, decided as Smithy's own `ShapeRecursionValidator` decides
  them: a cycle of lists and maps alone is refused (as before, now in one
  place), and so are a structure that reaches itself through required
  structure members alone and a union that cannot terminate. A Smithy model
  with either was already invalid Smithy; one assembled without Smithy's
  validators is now refused at lowering instead of reaching a generator.

- Typed permissions, phase A1: the `.cru` language, the IR and the
  compiler checks ([docs/permissions.md](docs/permissions.md)). A `.cru`
  resource declares `permission symbol = "value"` entries (`internal`,
  `@permissionOnly`, `@notGrantable`, `@aliasOf(primary)`, and
  `@authorizationName("..")` on the resource); an operation declares
  `requires p [on Target(bindings)] [when condition] [by handler]`,
  `filters p on R`, `requires none` or `requires pending because ".."`;
  `@permissionTypes(exclude: [..])` checks a resource-type enum against the
  resources both ways. Permissions are declared in `.cru` only: a Smithy
  model cannot declare them, and one that uses a `crucible.auth`
  permission trait is refused at lowering. They lower to
  `ir::Service::permissions`, `ir::Service::permission_types` and
  `ir::Operation::requirements` (all `#[serde(default)]`, and not written
  when empty, so existing release snapshots read and write unchanged), with
  `Service::all_permissions()` for the registry, and are checked by
  `crucible_smithy::validate::permissions`: once a service declares any
  permission, an authenticated operation that says nothing about
  authorization is the error `permissions.undeclared`. Lowering adds the new
  `crucible.errors#NotFoundException` (404, `message` and `resourceType`;
  in `smithy/errors.smithy` and the bundled `crucible.errors` package) to
  every operation with a `requires` or `filters` clause, unless every
  clause is on a root resource with `*`. The client audience projection
  drops internal permissions; an operation whose requirements were all
  internal becomes `Requirements::Internal`. `ir::Operation::public` says
  an operation was declared public (`.cru` `public`, or Smithy
  `@auth([])`).
  `crucible check` counts permissions and lists pending operations.
  Generators do not read the new fields yet, but an operation with a
  requirement has `NotFoundException` in its `errors`, so the code
  generated for it changes: its error types gain the 404.

- `crucible-build` and `crucible` print a `.cru` package's advice beside
  its warnings. `crucible_smithy::validate::permissions_from_ir` runs the
  permission checks that need nothing but the IR.

- Release guard and `crucible diff`: `compatibility.permissions` reports a
  removed permission or alias, a changed value, a changed authorization
  name, and a stored value that now satisfies another permission (a swap,
  a reassignment, a retargeted alias, a merge), which break stored grants;
  `compatibility.sdk` reports a removed permission constant or one moved
  to `internal`; adding a requirement to a released operation, with the
  `NotFoundException` it attaches, is a `compatibility.authorization`
  warning.

- `crucible_lang::Severity::Advice` and `Diagnostic::advice`: a question
  for the author, never a failure; the language server shows it as
  information, and builds print it beside warnings. `Severity` is now
  `#[non_exhaustive]`, as is `crucible_smithy::validate::FindingSeverity`:
  a `match` on either needs a wildcard arm.

### Changed

- An idempotency token member is described, in OpenAPI and MCP schemas,
  `crucible.describe/1` member schemas and the API reference, as sent
  automatically by SDKs, and as one a caller of the API itself should send,
  unique per intended operation and the same on a retry, since a service may
  refuse a request without one. It replaces the reference's "Clients fill an
  omitted token" sentence.

- `Context::deadline()` reports the deadline the framework enforces: the
  host's less `DEADLINE_RESERVE`, or the `request_timeout`, whichever is
  earlier. Before, it reported the host's deadline unchanged, and nothing
  enforced it.

### Fixed

- The Rust client's default HTTP transport no longer follows redirects,
  which could forward custom API-key and SigV4 session-token headers to
  another origin. Applications can configure an explicit redirect policy
  with `Config::http_client`.

- Rust clients ignore response JSON fields for members bound to HTTP
  headers, even when the corresponding header is absent. This also applies
  to modeled errors and the initial response of an event stream.

- Rust, Python and Go clients send an explicitly null Document payload as
  JSON `null`, preserving its distinction from an omitted payload. JSON
  payloads follow the protocol's null-member policy: Crucible JSON retains
  an explicit null structure payload, while AWS REST JSON omits it.
  TypeScript no longer tries to serialize an omitted AWS structure payload
  as `undefined`. Rust, Python and Go clients preserve an explicit modeled
  Content-Type header on buffered and streaming payloads instead of
  replacing it with the inferred media type.

- Compatibility checks report added effective OAuth scope requirements
  as authentication changes. Reordering or removing requirements remains
  compatible.

- `.cru` migration defaults are checked against the destination member's
  type and effective constraints before a migration can be generated.
  An invalid default can no longer pass the guard and break older requests.
  Valid timestamp and integer-enum defaults use the destination wire
  representation.

- The language server includes new unsaved buffers inside a package's
  source directories when checking diagnostics and resolving symbols.
  Buffers also work before their source directory or subdirectory exists.
  Excluded source paths and nested packages retain their existing rules.
  Renaming a declaration to a built-in type is rejected before edits are
  returned.

- JWT authenticators refuse foreign, missing, or malformed issuers before
  looking up signing keys. An unrelated issuer's JWKS outage can no longer
  block a valid token from a later authenticator in the API chain or consume
  the unrelated issuer's refresh cooldown. Signature and issuer validation
  still run before authentication; a matching issuer's outage remains a 5xx.

- A Lambda HTTP event whose query string held characters a URI does not
  carry raw (`"`, `<`, `>`, a space: curl and other non-browser clients
  send them unencoded, and a Function URL passes them on as received)
  panicked inside `lambda_http` while the event was converted, and the
  caller got a bare 502. The host percent-encodes those bytes first, which
  query decoding undoes, so the service reads what the client sent. Any
  other event `lambda_http` cannot convert (a body that claims base64 but
  is not) is answered 400 with the request id instead of failing the
  invocation.

- An answer larger than a Lambda invocation returns (6 MB, the event's
  shape and base64 included) was handed to the platform, which refused it
  after the service log had recorded the request as served: the caller
  got a bare 502 from the Function URL or CloudFront and nothing said
  why. The Lambda host now answers 500 `InternalFailure` in the event's
  shape with the request id (the failure envelope for a direct invoke)
  and logs the size through `tracing`.

- An operation that only an older API version serves (a later release
  removed it) answered 500 once any operation-aware middleware was
  registered: `Prepare::operations()` listed the current version's
  operations alone, so the authentication chain had no plan for it and
  failed with "authentication plan missing". It now lists those operations
  too, after the current ones, and `Prepare::metadata` returns their own
  release's metadata. An operation in both keeps the current version's.

- Model text with unusual characters no longer breaks generated clients.
  Each of these was a compile error before: a CR in documentation (rustc
  refuses a bare CR in a doc comment), a byte order mark anywhere (Go
  refuses one but at a file's start), U+2028, U+2029 or NEL in an enum
  value or default (C# ends a line at each, inside a string literal too),
  and `*/` in a PHP member's documentation, which ended the constructor's
  docblock. Documentation reaches every generated comment with its line
  breaks as LF and no control characters or BOM
  (`docs::comment_text`), `*/` is defused in block-comment tags as well
  as prose, and double-quoted literals in C#, Go, Java, TypeScript,
  Python, Dart and R escape what JSON leaves raw (`quoted`); Kotlin,
  Swift and C++ escape the same characters their own way. PHP and Ruby
  keep single-quoted literals, which take every byte as written.
  `fixtures/hostile-text.json` covers documentation, a deprecation
  message, enum values, a default, a pattern and a JSON name.

### Security

- A rejection lists at most 100 violations
  (`validate::MAX_VIOLATIONS`), then one more counting the rest. One
  broken constraint on a list's items used to fail once per item, and the
  response, the service log and the validation error's `fieldList` each
  copied every violation. So a body of a few megabytes of empty strings
  could build an error response, and a log record, of hundreds of
  megabytes.
- Rejections no longer quote the request's values, so a `@sensitive`
  member's value stays out of the response and the service log.
  - A JSON or CBOR value of the wrong type is named by its kind:
    `invalid type: string, expected i64` instead of quoting the string.
  - An unknown enum value now reads `unknown variant, expected ...`.
  - An unknown intEnum value is reported without the number.
  - A header, query or label value that is not an integer or a number
    reads `must be an integer` or `must be a number`.
  - CBOR read errors are words, not the decoder's `Debug` form.
  - An MCP tool call whose arguments, or whose output, break the tool's
    schema names the location and the keyword (`/name does not satisfy
    the schema's `type``) instead of jsonschema's message, which quotes
    the value. The location keeps declared property names and list indexes
    and says `[key]` for a map key (`/settings/[key]/limit`).
  - A value an API version hook cannot read becomes an internal error
    without quoting the value, so its logged cause carries no request data.
  - A violation's path names a map key `[key]` when the key is client data
    under a sensitivity rule: the map's keys are `@sensitive`, or the map is
    reached through a `@sensitive` member or structure (an ordinary
    structure behind a sensitive member included), or is the item of a list
    or map whose items are sensitive (`scoreMaps/outer/[key]`). Generated validation
    opens the new `Validator::sensitive` scope for them. Member names, list
    indexes and public map keys still appear.

  Paths and the model's expectations are unchanged.

### Migration

- **`FrameworkError` has `Conflict`, `NotFound` and `PermissionDenied`
  variants.** A `match` over `FrameworkError` with no wildcard arm needs
  arms for `Conflict { .. }` (HTTP 409), `NotFound { .. }` (404) and
  `PermissionDenied { .. }` (403), including through
  `crucible-client-runtime-rust`, which re-exports `FrameworkError`. No
  shim is possible for a new variant. See
  [migrating](docs/migrating.md#unreleased).

- **`crucible_auth::Principal` is non-exhaustive.** A custom `Authenticator`
  can no longer build one with a struct literal. Replace
  `Principal { subject, issuer, client_id: None, token_kind, scopes, groups, claims }`
  with `Principal::new(subject, issuer, token_kind)` and the `with_*`
  setters for the fields it set (`.with_claims(claims)`, ...), and add `..`
  to any exhaustive destructuring. No shim is possible: the attribute is on
  the type. Reading fields is unchanged. See
  [migrating](docs/migrating.md#unreleased).

## [0.7.0] - 2026-10-12

SDK packaging and releases ([design](docs/sdk-packaging.md)): a service
declares its SDKs once in `crucible-sdk.toml`; `crucible sdk config|generate`
writes them, and `crucible sdk plan|build|verify|publish` releases them on
npm and git channels. Rust and TypeScript packages take common package
metadata (description, URLs, license, authors, README, keywords, `private`,
an npm registry) from that file, CLI flags or `crucible_build` setters, and
carry schema-2 `crucible.json` provenance with a content digest plus
`crucible-model.json`, their audience's model. `crucible --version` names the
Git revision it was built from, generator changes that move SDKs are logged
in `sdk-changes.toml`, and an external audience projection drops raw trait
values that could name what it removed. Breaking for TypeScript
`package_json` overrides: follow *Migration*.

### Added

- `crucible-sdk.toml`, the SDKs a service declares: the model it generates
  from, `[defaults]` with the common keys, and one `[sdk.<name>]` per SDK
  with its language, audience, `out`, the generator's options, and release
  settings (`version` floor, `test`, `lockfiles`, `channel`). One resolver,
  `crucible_codegen::sdk::resolve`, reads it for the CLI and the build
  helper; every error in the file is reported in one run, at its key:
  unknown keys, keys the language does not take, a language's name used
  for another language, keywords crates.io would refuse, channel URLs
  carrying credentials (and published URLs carrying any), paths that leave
  the file's directory (or a README or license that resolves outside it),
  overlapping `out`s in any letter case, and private or internal SDKs on a
  public registry however its host is spelled. No message repeats a
  credential.
  See [releasing generated SDKs](docs/sdk-releases.md#declare-sdks-crucible-sdktoml).
- `crucible sdk config [--sdk <name>]... [--json]` prints each declared
  SDK's effective configuration and its `config_sha256`, and
  `crucible sdk generate [--sdk <name>]...` writes each one to its `out` at
  the development version, through the `.crucible` ownership manifest. Both
  find the file in the current directory or its nearest parent (`--file`
  names another), read the environment of the selected SDKs only, and
  refuse to run at any Crucible revision other than the one the service's
  `Cargo.lock` resolves `crucible-build` or `crucible-server-runtime-rust`
  to from Git, naming the one to install (and warn when no lock is found).
- Common package metadata for Rust and TypeScript SDKs: description,
  repository, homepage, documentation, license or license file, authors,
  README, keywords and `private`. `Cargo.toml` gains the matching keys,
  `publish = false` for a private crate and the README as crate
  documentation; `package.json` gains `keywords`, `homepage`,
  `repository`, `license`, `author`/`contributors`, `"private": true` for
  a private package with no npm registry, and `publishConfig` for a
  declared npm channel. The README and license are copied in with LF line
  endings. They come from `crucible-sdk.toml`, from new
  `crucible generate rust --client|ts` flags (`--description`,
  `--repository`, `--homepage`, `--documentation`, `--license`,
  `--license-file`, `--author`, `--readme`, `--keyword`, `--private`, and
  for TypeScript `--npm-registry`/`--npm-access`), and
  from new `RustSdk`/`TsSdk` setters in `crucible_build` (`description`,
  `repository`, `homepage`, `documentation`, `license`, `license_file`,
  `authors`, `readme`, `keywords`, `private`, `metadata`, and TypeScript's
  `npm_registry`), with `crucible_build::PackageMetadata` and
  `crucible_build::Registry`. A Git channel or `private` gives
  `publish = false`; a README used as crate documentation is left out of
  doctests.
- `crucible --version` prints the Git revision the CLI was built from,
  `0.7.0 (<revision>)`, with `+dirty` for a checkout with uncommitted
  changes: the revision `crucible sdk` compares with a service's lock.
- Rust and TypeScript packages carry `crucible.json` schema 2
  (`schemaVersion`, generator version, verified revision and change-log
  `id`, language, package, version, audience, service, `schemaSha256`,
  `configSha256` for a declared SDK, and `contentDigest`, a
  `crucible-content-1` digest of the generated files) and
  `crucible-model.json`, the compiled model as the package's audience sees
  it, as compact JSON of the typed contract, without documentation strings
  or raw trait maps (`snapshot::write_contract`). Rust packages gain both; npm's `files`
  lists both. A release's `source.repository` is the https URL of the
  service's `origin`, without credentials or `.git`.
- `crucible sdk plan|build|verify|publish` releases declared SDKs
  ([guide](docs/sdk-releases.md#release-declared-sdks-crucible-sdk-planbuildverifypublish)).
  `plan` reads each SDK's channel and chooses its version: no release when
  the content digest is unchanged, otherwise the highest of the last
  version plus a patch, the `version` floor and `--bump`; an empty channel
  needs `--first-release`, and a channel that cannot be read fails.
  `build` packages each release in a fresh directory (npm tarballs with a
  pinned TypeScript compiler; git channels as the archived tree), points
  the SDK's `out` at it, reconciles consumer `Cargo.lock` path entries,
  runs the `test` commands, and records `plan.json` and `release.json`.
  `verify` re-hashes the artifacts; `publish` (dry run unless `--execute`)
  journals uploads in `publication.json`, treats the same bytes already
  published as done, and stops when a channel moved since the plan.
- Release channels in a new crate, `crucible-sdk-release`: npm registries
  (GitHub Packages included; credentials from `.npmrc`, HTTPS-only
  downloads whose token never follows a redirect off its origin, a
  dist-tag that never moves backwards) and git tags (atomic branch and
  tag push, never a re-tag, `.crucible`-owned paths replaced, `adopt` for
  a pre-Crucible tree, tag prefixes for subdirectory modules). Releases
  made before schema 2, Pigeon's `contractHash` tarballs and
  `sdk-release.json` tags, are imported as the last release.
- `crates/crucible-codegen/sdk-changes.toml`, the reviewed log of generator
  and runtime changes that move SDKs, seeded with entry `0` and embedded in
  the generator. A PR that changes a generator's public output or a
  runtime's public API adds an entry ([releases](docs/releases.md#checklist-stable-surface-diff)).

### Changed

- A `crucible_build` client helper fails when a `crucible-sdk.toml` in the
  producer's own Git repository declares its output name and names its
  model source: `crucible sdk generate` writes declared SDKs, so the build
  script and the release cannot drift. Only the file's `model` and SDK
  names are read, so an otherwise invalid file never fails a build.
  Builder-only names are unaffected.
- TypeScript packages no longer write their schema-1 `crucible.json`
  themselves; the shared generator layer writes schema 2, which keeps
  `generatorVersion`, `service`, `serviceVersion` and `schemaSha256`.
  `crucible_codegen_ts::emit` alone emits neither provenance file.
- The `RustOptions` and `TsOptions` generator structs gain a `metadata`
  field (exempt from the stability policy). `PackageMetadata` and
  `Registry` (also re-exported by `crucible_build`) are `#[non_exhaustive]`:
  start from `PackageMetadata::new()` and set fields, build a registry with
  `Registry::npm(..)` or `Registry::Git`, and match with a fallback arm.
- An external audience projection keeps, in its raw trait maps, only the
  standard traits (`smithy.*`, `aws.*`, `crucible.*`) whose values cannot
  name a member, shape or enum value it removed, such as HTTP bindings,
  constraints and documentation, and drops the rest (`smithy.api#enum`,
  `smithy.api#references`, protocol tests, examples, waiters, endpoint
  tests); their projected typed forms remain. A custom trait stays unless
  its definition is `@internal`, as in Smithy's projection. The IR records
  those definitions in `Service::internal_traits`. Server bindings and
  internal views keep every trait. OpenAPI's `x-smithy-traits` follow.
- The generator revision is embedded by a new leaf crate,
  `crucible-revision`, so a commit no longer rebuilds every generator, and
  it marks a build whose compiled source trees (`crates`, `smithy`,
  `vendor`, the workspace manifest and lock) have uncommitted changes:
  `generatorRevision` reads `<rev>+dirty`, and `crucible sdk` treats it as
  unverified. `crucible_codegen_rust::source_revision` is unchanged.

### Migration

- **TypeScript `package_json` cannot set what the package owns.**
  `crucible generate ts --package-json`, `TsSdk::package_json` and
  `TsOptions::package_json` refuse a fragment that sets `name`, `version`,
  `crucible`, `files`, `private` or `publishConfig`, naming each key. Set
  the name and version with `--package-name`/`.package(..)` and
  `--package-version`/`.version(..)`, `private` with `--private`/
  `.private(true)`, and `publishConfig` with `--npm-registry`/
  `--npm-access`, `TsSdk::npm_registry(..)`, or an npm `channel` in
  `crucible-sdk.toml`. See [migrating](docs/migrating.md#060-to-070).

## [0.6.1] - 2026-10-12

Editor support: `crucible-lsp` completes `$` bindings with what the member
list may bind. No changes to the generated code or the runtimes.

### Added

- `crucible-lsp` completes `$` bindings. `$` is a completion trigger, and
  after `$` (or `$` and part of a name) in the `input` or `output` of an
  operation attached to a resource, or in a `structure .. for` view, it
  offers what that list may bind: the resource's identifiers, its
  ancestors' identifiers and its own properties, never an ancestor's
  property. Identifiers come first. The role decides the rest, as the
  compiler does: a `creates`, `lists` or `on .. collection` input does not
  bind the resource's own identifier, an `updates` input does not write a
  `readonly` or `immutable` property, a `creates` or `puts` input does not
  write a `readonly` one, and a view or output does not show a `writeonly`
  one; a `@query` binding only reads, so it may name any property. In a
  view with `$*`, what `$*` includes is not offered before it and is offered
  after it only as `name?`, which loosens it. A view or output shows an own
  identifier's `as` spelling beside its name. Each item gives the entry's
  type, its keywords and the resource that declares it, with its doc
  comment, and replaces only the name after the `$`. Names the list already
  has are left out. A view also offers `$*` while it has none and nothing
  `$*` would include is named already. A `$` in a comment or string opens no
  list, and completion offers nothing inside comments and strings. It works
  before the package compiles, from the open file.

## [0.6.0] - 2026-10-12

Resources in the Crucible language say more about themselves: property
mutability keywords, `$*` views, property presence and identifier spelling,
with children inheriting only their ancestors' identifiers. `crucible-lsp`
hover explains the language and shows whole shapes with their constraints.
An optional member appended to a structure that is not an error is no
longer an SDK break, and
generated Rust client types are `#[non_exhaustive]`. Breaking: follow
*Migration*.

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
  the IntelliJ plugin among them, highlight it; prose stays outside. The
  new resource syntax is completed, highlighted and explained on hover too:
  `readonly`, `immutable`, `writeonly`, `as` on identifiers, and `$*`, which
  lists what it includes.
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
  keyword. An `on .. collection` binding counts as a create (it carries
  mutability and sets an `immutable` property) and an instance `on`
  binding as a change. A property left read-write that no `updates`,
  `puts` or instance `on` input binds is a warning, as is a `for` view
  used as a member type in a writing input, which binds nothing.
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
- **Breaking:** generated Rust client models, error structures, enums,
  unions and per-operation error enums are `#[non_exhaustive]`. Code in another crate builds models
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
- **`.cru` child resources.** A `$name` in a child resource's operation or
  view that bound an ancestor's property is now an error: bind the
  ancestor's identifier, or declare the property on the child. A property
  renamed only to avoid an ancestor's name (`workspaceName` for `name`) can
  take its natural name back; bind it as `$name as workspaceName` where it
  was `$workspaceName` to keep the wire and SDK names.
- **`.cru` `@mutability` on a property binding.** It must now agree with the
  property's keyword, and a property with no keyword is read-write, so
  `@mutability(read) $createdAt` is an error. Declare the property
  `readonly`, `immutable` or `writeonly` and drop the annotation.

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
