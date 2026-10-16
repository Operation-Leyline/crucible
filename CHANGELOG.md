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

- Sender-signed operations (decision 81 in
  [docs/idl/decisions.md](docs/idl/decisions.md)): `@sender("Stripe",
  signature: signature)` on a `public` operation says a third party calls
  it and signs each request in the named input member's header, such as a
  webhook receiver. The IR carries `Operation::sender` (`ir::Sender`,
  `serde(default)`) and `Operation::signature_header`; `describe.json`
  (additive) gives the operation `sender: {name, member, header}`, and the
  references say "Called by Stripe, not by clients of this API" and which
  header carries the signature. `crucible_ir::client_view` leaves such
  operations out, with what only they reach, and every client generator,
  the service CLI, the in-build Rust client, `sdks.json` and MCP tools read
  that view; servers implement them as before. The compiler refuses
  `@sender` on an operation that is not `public` (`sender.public`) or with
  a signature that is not an `@header` input member (`sender.signature`).

- A schema mode for `describe.json`: `DocsOutput::schemas` and `crucible
  generate docs --schemas inline|shared|omit`. `Inline`, the default, is
  unchanged. `Shared` writes every JSON Schema definition once, in a root
  `schemas` object (`{"$defs": ...}`), and makes each operation's
  `inputSchema` and `outputSchema` a `$ref` into it; `Omit` leaves them
  out. Every mode is `crucible.describe/1` (additive), and a consumer must
  handle all three; `CrucibleExplorer.operationSchema` rebuilds an
  operation's schema in each, and the static reference and the Explorer
  read it through that. Leyline's description is 5.8 MB inline, 4.2 MB
  shared. See [docs/docs.md](docs/docs.md#operation-schemas).

- The API reference has Markdown for agents: `llms.txt` beside
  `index.html` (the llms.txt convention: the service, what every request
  sends, and a linked line per page, operations and data types with their
  method, URI, kind and first sentence) and `llms-full.txt`, every page in
  one file. Each page's Markdown keeps its one path under `src/`; the
  mapping from the static reference's routes is in
  [docs/docs.md](docs/docs.md#markdown-for-agents). Operation pages now
  carry what a caller needs without another page: the `Authorization` and
  `Crucible-Version` headers in the request syntax, the authentication
  scheme as the header it sends (or that the operation takes no
  credential), and each error's wire code and the header that carries it.

- Release history in the API reference (decisions 77 and 78 in
  [docs/idl/decisions.md](docs/idl/decisions.md)). `describe.json`
  (`crucible.describe/1`, additive) gives every operation, shape, member
  and enum value a `history`: `added`, the first supported release that
  has it, following renames (`null` when the oldest supported release has
  it); `changed`, the releases whose migration names it; and `missing`,
  the releases that see an older definition without it because it sits
  inside a later `reshape` or `remove operation`. Its root gains
  `versions`, `null` for a service without an `api_version`: the current
  release, the `Crucible-Version` carrier, and every supported release,
  oldest first, with its sunset and the sunset's documentation, the
  operations it retired, its migration's documentation and each step as a
  change, with the step's documentation and what a client of the release
  before it sees instead (`older`, self-contained through `older.shapes`).
  The reference writes the same block to `changes.json`
  (`crucible.changes/1`) beside `describe.json`, for a changelog or a feed.
  The mdBook pages and the static reference date each element ("Added in",
  "Changed in", linked to the release) and, for a chain of more than one
  release, have a page per release, its file named by the escaped release
  name; `@since` is shown only for what predates every supported release.
  `crucible_ir::versions` derives it all (`history`, `history_at`,
  `subjects`, `named`, `older_member`). An external projection marks the
  older releases (`ApiVersion::external`) instead of projecting them, and
  `versions::visible` projects them where the reference reads them, best
  effort and never refused, keeping a step only when the audience sees
  every id it names on each side, so no external output names internal
  API and no older release fails an output. See
  [docs/docs.md](docs/docs.md#release-history).
- `crucible_lang::load_versioned_service`: a package's service with the
  older releases it serves, as a build lowers it, after its compatibility
  guard.

- `describe.json` (`crucible.describe/1`, additive): each operation has a
  `sample` and each error's metadata a `sample`, generated from the model
  alone and labelled `generated: true`: wire messages in the examples' form
  (`input`, `minimalInput`, `output`; an error's `message`) whose values
  pass the model's own validation, with typed ids from their patterns,
  timestamps in each member's format and values within their bounds. A
  value the generator cannot make (a pattern outside its subset) is a
  placeholder listed in `fallbacks`. The static reference and the Markdown
  pages show the sample, marked as generated, for an operation without
  examples. `crucible_codegen_common::sample` builds them
  ([docs/docs.md](docs/docs.md#generated-samples)).
- Policy: `[rules.examples]` reports operations without an `example`
  (`examples.operations`) and errors an operation declares that none of
  its examples shows (`examples.errors`). Its checks are warnings unless
  configured otherwise ([docs/api-policy.md](docs/api-policy.md#example-coverage)).
- Doc links: `[Name]`, `[Name.member]`, `[Name][]` and `[text][Name]` in a
  `.cru` doc comment resolve to the operation, type, member, enum value,
  resource or service they name, recorded in the new
  `ir::Service::doc_links` (label to id; omitted from a snapshot when empty,
  so older snapshots read unchanged), with `crucible_ir::doc_links` to find
  them and `Service::doc_link` to look one up. `[Op.member]` names the
  operation's input member, else its output's, and keeps the operation in
  `Service::doc_link_operations`. `describe.json`
  (`crucible.describe/1`, additive) renders each in `html` as
  `<a data-crucible-ref="<id>">Name</a>`, an anchor with no address for the
  reader to map (with `data-crucible-operation="<id>"` for an operation's
  member), and as its text in `text`; `markdown` is unchanged. The
  static reference and the Explorer link the anchor to the target's page,
  the mdBook pages to the target's page, and
  native SDK doc comments use their language's link to the generated
  symbol, in place of the escaped `\[Name]` or literal `[Name]` they printed.
  OpenAPI, JSON Schema and MCP tool descriptions show a doc link as its text.
  External output renders a link to internal API as plain text, with no id.
  The language server goes to the definition of, finds references through,
  and renames doc link labels, and refuses a lowercase new name for a
  declaration doc links name. See decision 80 in
  [docs/idl/decisions.md](docs/idl/decisions.md).

- `.cru`: the compiler lowers resources. `ir::Service::resources` holds every
  resource the service reaches, with its identifiers (its ancestors' and its
  own, keyed by name), properties, children, the lifecycle slots its operations' roles fill
  (`creates` to `create`, ..., `on` to `operations`, `on .. collection` to
  `collection_operations`) and, per attached operation, the member paths that
  carry its identifiers and properties: `$name` elisions under their lowered
  names, members typed by a `for` view, and lists and maps of one as
  `collections`. `resource_roots` is the service's `resources [..]`. A type
  only a resource names does not join `Service::shapes`, so the generated
  SDK and server code is unchanged; the `crucible-model.json` a package
  carries (outside its content digest) now includes the resources. An entry
  several members carry binds at the first public one, with a warning about
  the others. With them, documentation (`crucible generate docs`,
  `Output` `Generator::Docs`), the in-service Explorer (`.explorer()`), and
  CDK and Terraform output (`generate cdk`, `generate terraform`) work from a
  `.cru` package again; they refused one since the Smithy front end was
  removed. A resource takes the documentation notes `@deprecated`, `@since`,
  `@unstable` and `@externalDocs`. Release snapshots now carry the resources;
  older ones still read. See decision 67 in
  [docs/idl/decisions.md](docs/idl/decisions.md).

- `crucible.sdks/1` (`sdks.json`): each declared SDK's public surface,
  named by its generator, for documentation sites. Per SDK: the package
  (ecosystem, install coordinate, import, the import lines and qualifier
  its types resolve under), the client class and how to construct it, the
  endpoint or its environment variable (never read), default
  middleware with the headers it adds (`content-sha256` adds
  `x-amz-content-sha256`), and per operation the method, async and options
  variants, paginators, and input and output types with each member's
  native name, type and optionality. Names come from the generators' own
  allocators, so collision suffixes and keyword escapes are included. Print it with
  `crucible sdk describe [--sdk <name>]... [--out <file>]`, or write it
  beside `describe.json` with `DocsOutput::sdks("crucible-sdk.toml")` or
  `crucible generate docs --sdks crucible-sdk.toml`. Each client generator
  gains `surface(&Service, &Options)`, and `crucible-codegen-common` the
  `surface` module. See `docs/docs.md`, "The `crucible.sdks/1` contract".

- `crucible_client_runtime_rust::Document`, an alias of the
  `serde_json::Value` that document fields hold, reachable from a generated
  SDK as `runtime::Document` without depending on `serde_json`.

- `.cru`: `template <role> { .. }` compiles. Every operation of the role gets
  the template's `paginated` arguments, `errors`, and `input` and `output`
  members; restating one is an error. Rules read them with the `template`
  condition and the built-in annotations with `has @token`, `@query`,
  `@header`, `@payload` and `@deprecated`. The `docs/idl/examples/leyline-graph`
  example, with `org.standards` as a path dependency, now compiles whole and its
  rules fire (`crates/crucible-lang/tests/examples.rs`).


- `.cru`: `[model.dependencies]` imports a package by path,
  `"org.standards" = { path = "../standards" }`. An importing package sees the
  dependency's shapes and metadata, and the dependency's rules apply to it:
  an importer cannot weaken them, and may except one only if it is declared
  `exceptable`. `crucible_lang::Package` gains `dependencies` and `dir`;
  `crucible-build` reruns when a dependency's directory changes.

- `.cru`: `rule` and `except` declarations compile. A rule is `every <kind>
  [where ..] must ..` or `no <kind> [where ..]` over operations, shapes and
  members, with conditions on keywords, metadata (including `retention
  source`), HTTP bindings, constraints, resources and documentation, each
  checked against its kind when the rule is read. Levels are `strict`, none,
  `exceptable` and `advice`; `except Rule for Name because ".."` is recorded
  in the compiled model (`ir::Service::exceptions`), and an exception that
  excuses nothing is an error. See
  [docs/idl/reference.md](docs/idl/reference.md#policy-rules).

- `.cru`: a view can serve several resources, `structure JobData for Job,
  HistoryJob { ... }`. It is lowered against the first, and each `$name` must
  lower to the identical member against every other (a difference is an error
  naming the fields); `$*` shows what all declare. See decision 34j in
  [docs/idl/decisions.md](docs/idl/decisions.md).

- `.cru`: `sparse list` and `sparse map` declare a collection whose elements
  or values may be null, as `@sparse` does in Smithy: `sparse list ResultRow:
  CypherValue { length ..1000 }`. A dense collection (the default) refuses a
  null element. Changing a collection between dense and sparse is reported by
  the compatibility guard like any other nullability change.

- `crucible compile --json <package>` prints a `.cru` package's compiled
  model, custom metadata and documentation included, in the release snapshot
  format (`--out` writes a file, `--service` picks a service). The build
  helper's `model_output` / `CRUCIBLE_MODEL_OUT` now export the compiled model
  of a `.cru` package too; they used to panic. See
  [docs/api-policy.md](docs/api-policy.md#reading-a-cru-packages-compiled-model).

- `PageTokens::max_cursor` lowers the largest cursor a token seals (default
  and ceiling 4096 bytes), and `max_token_length` / `cursor_capacity(limit)`
  report the longest token that results and the largest cursor a token length
  allows. `build` now refuses a `@paginated` operation whose input token
  member's `@length` max is smaller than the longest token `PageTokens` can
  issue, so a model limit fails at build instead of at runtime on a long
  cursor. `Pagination` gains `input_token_max_length` (generated).
  See [docs/framework.md](docs/framework.md).

- Generated server string enums carry `VALUES: &'static [&'static str]`, every wire
  value in declaration order, beside `as_str`, so a service can enumerate
  them (a quota code list, say) without parsing the model.

- `JwtAuthenticator::credential_headers`: an ordered list of headers to read
  the bearer token from, the first one present being the only one read, as
  `ApiKeyAuthenticator::credential_headers` does. `credential_header` stays as
  the single-header form. An empty list is a configuration error. See
  [docs/auth.md](docs/auth.md).

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
  share one read and reads are bounded (`lookup_concurrency`, with separate
  slots for refresh-ahead so a flood of misses cannot starve it). One read,
  including its wait for a slot, is an outage after `lookup_timeout` (5 s).
  `forget` drops a revoked key on this instance; `fence` answers every key
  with a 5xx while a replica is unhealthy; `last_used` hands
  an `ApiKeyUsageRecorder` each key's use at most once per
  `last_used_resolution`, awaited for at most `last_used_timeout` (1 s). A
  store error, or an active record with an empty key ID or tenant, is a
  sanitized 5xx, never a refusal.
  It serves the bearer scheme only, not `@httpApiKeyAuth`.
  `api_key_digest(credential)` is the lowercase hex SHA-256 lookup key, for
  minting. `test_support::InMemoryApiKeyStore` stands in for the table in
  service tests. See [docs/auth.md](docs/auth.md#api-keys).

- `OpaqueError`: a failure whose causes `ExceptionTranslator` rules never
  see. A rule may match the wrapper itself, but the translator does not walk
  into what it wraps; the service log still shows the whole chain.
  `OpaqueError::new` wraps any error and `OpaqueError::from` a crucible
  `Error`. `AuthenticatorChain` wraps an authentication provider's outage
  in it. See
  [docs/framework.md](docs/framework.md#errors).

- `Principal::tenant`: the workspace or organization a credential is bound
  to. API keys fill it from their record; ID and access tokens leave it
  `None`, and no token claim sets it. `Principal::new(subject, issuer,
  token_kind)` with `with_client_id`, `with_scopes`, `with_groups`,
  `with_claims` and `with_tenant` construct one.
  `Principal::caller_binding` names the principal for `Idempotency` and
  `PageTokens` as the JSON array `[issuer, subject, tenant]`, and
  `Principal::caller_binding_of(context)` is that binding for the request's
  principal, a caller closure as it stands.

- `OperationMetadata::requires_authentication`: whether the operation names
  an authentication scheme and is not `@optionalAuth`.

- Sealed pagination tokens, behind the runtime's `page-tokens` feature.
  `PageTokens` is an `Interceptor` that opens the token a `@paginated`
  request carries before the operation runs, and the operation reads its own
  cursor with `page_tokens::resume()` and seals the next one with
  `page_tokens::issue(&cursor)`. Tokens are XChaCha20-Poly1305 under keys
  derived from a secret (`PageKeys`, with retired keys accepted for
  rotation), bound to the service, operation, caller and filters, and expire
  after 24 hours (`ttl`). The caller is named by the closure given to
  `PageTokens::new`, as for `Idempotency` below: a request it names no one
  for shares one anonymous binding only after `allow_anonymous()`, and never
  on an operation that requires authentication. Nothing is stored. A token
  that does not open is the service's validation error naming the token
  member, or what `PageTokens::refusal` builds. The lifetime is from one
  second to 30 days; a cursor is at most 4096 bytes, and a longer token is
  refused before it is decoded. Rotating needs a new secret:
  `PageKeys::accept` refuses one another key uses
  (`PageKeyError::ReusedSecret`; `PageKeyError` is `#[non_exhaustive]`).
  `build` refuses `PageTokens` for an operation whose token is a map, at any
  API version that serves it. At an older API version the token is read from
  the current version's input members. Generated server operations carry
  `Operation::PAGINATION`, the token and page size members and whether the
  token is a map, also on `OperationMetadata::pagination`. See
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
  canonical decoded input without its token. The caller binding is the
  closure given to `new`; one that names no one (`None`) is the service's
  internal error unless `allow_anonymous()` lets every such request share
  one binding, and on an operation that requires authentication it is
  refused regardless, since the closure missed an authenticated caller.
  `build` refuses `Idempotency` or `PageTokens` without `allow_anonymous`
  over an operation that admits anonymous callers (`@optionalAuth`, no
  authentication scheme), naming it. A retry with the same input
  gets the first success re-encoded for its own protocol, API version and
  request id, without the operation running; the same token with other
  input, or while the first request runs, is a 409, and so is a stored
  response from before a deploy that changed it. A failure releases the
  claim so a retry runs again, unless the operation called
  `idempotency::commit()` before a step that is not safe to repeat, after
  which the failure is recorded for the retention and a retry is refused
  as `Failed`; every `commit` checks the claim is still the attempt's and
  refuses one that outlived it. A request that ends without finishing (a
  client that disconnected, a claim whose store call failed or timed out)
  settles its claim in the background through a drop guard: released, or
  recorded as failed past a commit. Successes replay for 24 hours
  (`retention`); an unsettled claim lapses after 15 minutes (`lease`), or a
  minute past the request's deadline. A success that cannot be stored (the
  write failed, or it is over `max_stored_response`, 256 KiB) is recorded
  without its response, and a retry is refused as `Unreplayable`; when that
  write fails too the claim is held for the retention, retried in the
  background when even that fails. Each store call is bounded by
  `store_timeout` (5 seconds). Store methods may run outside the request's
  task (a dropped request's settlement, background retries), logging in the
  request's span, so a store must not depend on `Context::current()`. `refusal_factory` maps an `IdempotencyRefusal`
  (`Missing`, `Invalid`, `Mismatch`, `InFlight`, `Lapsed`, `Unreplayable`,
  `Failed`) to the service's own errors. Store errors are sanitized 5xx
  answers; a failed completion write never turns a success into an error.
  `Claim` and `Ownership` are `#[non_exhaustive]`. Operations with a
  streaming input or output are refused at build (leave them out with
  `.except`), two `Idempotency` interceptors covering one operation answer
  its requests with an internal error, and an older API version without
  the token runs unclaimed. `MemoryIdempotencyStore` (`test-support`)
  stands in for the table in tests, sweeping expired records on each
  claim. See
  [docs/framework.md](docs/framework.md#idempotency-tokens).

- `idempotency::abandon()` takes back the latest `idempotency::commit()`
  when the step after it provably did not happen (the provider refused
  explicitly); earlier commits stand, since their steps happened. With none
  standing, a failure releases the claim as if `commit` had never been
  called, so a retry with the same token runs the operation again instead
  of getting a 409 for the retention. It changes only what a failure does
  (a success still completes and replays), makes no store call (the
  request's own completion releases the claim, and a process that stops
  first leaves it held as committed), does nothing with no commit standing
  or for an unclaimed request, and is an internal error without
  `Idempotency`. A `commit` after it commits again. See
  [docs/framework.md](docs/framework.md#idempotency-tokens).

- Typed permissions, the runtime side.
  - **Descriptions.** The `permissions` module, always present, holds the
    static descriptions generated code states: `ResourceInfo` (with its
    fixed 404 message), `PermissionInfo` (with `Tier`, aliases and its
    fixed 403 message), `Requirements` (`Undeclared`, `None`,
    `Pending { reason }`, `List` of `RequirementInfo` with a `Mode`) and
    `Obligation`. Each request gets `Check`s on a `ResourceRef`
    (identifiers root to leaf, the leaf possibly `Target::All`), each with
    its `Origin`.
  - **Where they appear.** Generated operations carry them as
    `Operation::REQUIREMENTS`, `Operation::CHECKS` and `Operation::PUBLIC`
    (an operation the model explicitly makes public), and
    `OperationMetadata` repeats them (`requirements`, `checks`, `public`).
    A service carries its permission registry as `ServiceInfo::permissions`
    (`service_info(..).with_permissions(&[..])`).
  - **Enforcement.** `Permissions`, behind the new `permissions` feature,
    is an `Interceptor` that enforces them through a service-supplied
    `Authorizer`: `decide` for a batch of checks, `visible` for a
    `filters` listing. The Authorizer answers `Verdict::Allowed`, `Denied`
    or `Hidden`. The first refusal in declaration order wins: Hidden is a
    404 `NotFoundException`, and Denied is a 403 naming the permission
    (never an internal one) or what `refusal_factory` builds from a
    `PermissionRefusal`.
  - **Undeclared and pending operations.** An authenticated operation
    without requirement metadata is refused as a wiring error unless the
    service calls `allow_undeclared()`. `requires pending` passes.
    `shadow()` only records and logs.
  - **Handler checks.** Handlers check what the model cannot bind with
    `permissions::require` and `decide`, and close each `by handler`
    obligation exactly once with `discharge` or `not_applicable`.
    `idempotency::commit()` refuses an unsatisfied obligation in every
    build (`shadow()` only logs it). A success with one fails the request
    in every build unless `assert_obligations(false)` opts out. Handlers read a listing's
    `permissions::visible()` and report genuine absence with
    `permissions::not_found`, which is answered exactly as a handler's own
    modelled `NotFoundException` would be.
  - **Audit and idempotency.** Every request gets an `AuthorizationRecord`
    on its context for audit. `Permissions` must be registered before
    `PageTokens` and `Idempotency`; the other order fails the build. A
    refused request therefore never has its page token opened or claims an
    idempotency token, and a replay is authorized again: its
    declared checks run, and so do the handler checks that allowed the
    first success. The idempotency record now keeps those
    (`StoredResponse` gains an `authorization` field; a record without it
    replays with the declared checks alone).

  See [docs/framework.md](docs/framework.md#typed-permissions).

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
  projection turns a pending operation into an undeclared one, since its
  reason is the server's migration note. See
  [docs/framework.md](docs/framework.md#typed-permissions).

- Typed permissions in every native client. Python, Go, Java, C#, Kotlin,
  Swift, Dart, PHP, Ruby, C++, C and R clients of a model with permissions
  now get what Rust and TypeScript clients have:
  - per resource an open permission type (`GraphPermission`), with a
    constant per permission that a client may see; it keeps a value it does
    not know and writes it back unchanged;
  - deprecated aliases as deprecated constants, and a canonical mapping
    (`canonical()`, or a function in C and R) from an alias to its primary;
  - the registry a grant picker reads, with each permission's value,
    description, tier (a string), `grantable`, `deprecated` and `aliasOf`.

  Each language follows its own convention for open string enums. The
  types sit in a namespace of their own where the language has one: the
  `permissions` module, package, sub-package or library; `Permissions` in
  C#, PHP, Ruby and Swift; `permissions` in C++. In C and R they share the
  model namespace, so a model name they need is a diagnostic. So is a pair
  of permissions that a language spells alike, and `_` where the language
  reserves it (Kotlin, Swift, Dart). A hyphen in a symbol (`mail-send`)
  starts a word of the constant's name, as Rust and TypeScript already
  spell it. Every language, Rust and
  TypeScript included, now generates from one rule,
  `crucible_codegen_common::permissions::client_resources`, so every
  registry lists the same permissions with the same flags. None names an
  internal permission. Clients of a model without client-visible
  permissions get no new file, except that Python's bundled `runtime.py`
  gains the two record types. A Terraform provider embeds the Go client
  through the new `crucible_codegen_go::emit_without_permissions`, without
  the permissions sub-package or its name checks. The R client renders a documentation code
  span that holds a quote or backtick as `\verb{}`, which `R CMD INSTALL`
  parses, rather than `\code{}`. See
  [docs/permissions.md](docs/permissions.md#63-clients-docs-and-openapi)
  and each language's page under `docs/clients/`.

- A service's own not-found error for typed permissions. A `.cru` service
  whose errors are its own (and so cannot import `crucible.errors`) names
  its 404 with `notFound ResourceNotFoundException` in the service block:
  an error of its package with status 404 (`permissions.not-found-error`
  otherwise). Check 17 attaches it, instead of
  `crucible.errors#NotFoundException`, to every operation with something to
  hide; without a designation nothing changes. The IR carries it as
  `Service::not_found` (`#[serde(default)]`; `Service::not_found_error()`
  answers whichever applies) and the server bindings as
  `ServiceInfo::not_found` (`service_info(..).with_not_found(..)`).
  `requires pending` operations get it too, and it renders on any
  operation. `Permissions::not_found_factory(|resource: &ResourceRef| ..)`
  builds it for a Hidden verdict or a hidden listing, from the refused
  check's resource type and identifiers, so a service gives it the
  constructor its handlers answer a genuine miss with and the two stay
  byte-identical; `permissions::not_found`, a generated reference's
  `not_found()` and `ResourceRef::not_found` (now a
  `permissions::MissingResource`, answered as the framework's NotFound
  without a designation) go through it too. It fails closed: a designation
  without `Permissions` or a factory, a factory without a designation, or a
  factory that builds another error (asked once per resource at build)
  fails the build, and a framework 404 the factory cannot be asked for
  (outside what `Permissions` covers, or `ResourceInfo::not_found_error`)
  is a logged wiring error. Every response carrying the designated error is
  sent with `Cache-Control: no-store`, at older API versions too. The release guard reports an operation that
  switches to a newly designated 404 as a new error. `crucible-lsp`
  completes and explains `notFound`. See
  [docs/framework.md](docs/framework.md#typed-permissions).

- `Visibility::Denied`: an Authorizer refuses a `filters` listing with a
  403 (a suspended workspace or user), answered as a Denied verdict on the
  clause is, through `refusal_factory` with the clause's permission, in
  declaration order with the other clauses. `Visibility::shows` is false
  for it.

- `FrameworkError::NotFound { resource }`: HTTP 404, the modelled
  `crucible.errors#NotFoundException` with the resource's `message` (`No
  such inbox.`) and `resourceType`, sent with `Cache-Control: no-store` (a
  tool error in MCP). `FrameworkError::PermissionDenied { permission }`:
  HTTP 403 `ForbiddenException` with the permission's message, or a generic
  one for an internal permission, without the `insufficient_scope`
  challenge `Forbidden` carries.

- `FrameworkError::Conflict { message }`: HTTP 409 `ConflictException`
  (a tool error in MCP),
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
  recursion are checked on the IR in `crucible_ir::validate`, for both
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
  permission trait is refused at lowering. `crucible-traits` does not
  define those traits, so the Smithy compiler fails first on `.smithy`
  sources with `Model.UnresolvedTrait`. `crucible-build` puts the same
  migrate-to-`.cru` message for each of the nine names before the
  compiler's report. They lower to
  `ir::Service::permissions`, `ir::Service::permission_types` and
  `ir::Operation::requirements` (all `#[serde(default)]`, and not written
  when empty, so existing release snapshots read and write unchanged), with
  `Service::all_permissions()` for the registry, and are checked by
  `crucible_ir::validate::permissions`: once a service declares any
  permission, an authenticated operation that says nothing about
  authorization is the error `permissions.undeclared`. `.cru` lowering adds the new
  `crucible.errors#NotFoundException` (404, `message` and `resourceType`;
  in `smithy/errors.smithy` and the bundled `crucible.errors` package) to
  every operation with a `requires` or `filters` clause, unless every
  clause is on a root resource with `*`. The client audience projection
  drops internal permissions; an operation whose requirements were all
  internal becomes `Requirements::Internal`. `ir::Operation::public` says
  an operation was declared public (`.cru` `public`, or Smithy
  `@auth([])`).
  `crucible check` counts permissions and lists pending operations.
  An operation that gets `NotFoundException` in its `errors` this way
  (or, since the not-found-error entry above, the service's designated
  404) has the 404 in its generated error types too. The phase B entry
  covers the generators reading the rest.

- `crucible-build` and `crucible` print a `.cru` package's advice beside
  its warnings. `crucible_ir::validate::permissions_from_ir` runs the
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
  `#[non_exhaustive]`, as is `crucible_ir::validate::FindingSeverity`:
  a `match` on either needs a wildcard arm.

- `Prepare::served_operations()` and `ServedOperation`: every entry a
  request in the extension's scope can be served by, one per operation per
  API version, with the description the request carries
  (`OperationJob::operation()`) and the metadata it is served with. An
  extension that prepares per operation and looks up per request prepares
  per entry and selects by that description, since an older version's
  entry keeps its own release's authentication facts.
  `Prepare::operations()` is unchanged: each id once.

### Changed

- `crucible check`, `generate` and `compile` load the chain of older
  releases a package serves, as a build does, through one
  `crucible_lang::Releases` for the guard and the versions: `generate docs`
  documents them, and a chain the build refuses (an operation dropped
  without `remove operation`, a missing frozen release) fails these
  commands too. `generate rust` serves older releases as the build's
  server does.

- `.cru`: a migration that drops an operation without `remove operation`
  is an error at its `from` (`crucible check`, the build, the editor),
  whichever migration of the chain it is: nothing would serve the
  operation at that release and the ones before it, and the guard's
  comparison sees only the newest migration. A `compatibility.operation`
  exception for the operation in the manifest accepts the drop: the
  operation is then not served at those releases, with a warning there,
  and `ApiVersion::retired` records it. A setting that turns the finding
  off does not accept it (decision 68). See *Migration*. The guard and
  `versions` read the chain of frozen releases through one
  `crucible_lang::Releases` (`guard_in`, `versions_in`): the build loads
  each release once, and the language server keeps the loaded releases
  across edits, reading one again only when its file changes.

- `.cru`: `rename Enum.A -> B` declares the enum's change, and the Rust
  server maps a changed wire value both ways, in requests and responses,
  except where the enum keys a map or sits in an error, which are reported
  as for `add .. downgrade`. The guard covers the enum's own
  `compatibility.sdk` and `compatibility.enum` findings, for a rename and
  for `add .. downgrade` alike, only when the migration's steps explain
  every value that changed on the wire; otherwise the finding stands and
  names the values no step declares (an `add` beside an undeclared
  removal used to pass). One enum's renames and additions compose into
  one mapping each way, which the server applies in a single lookup per
  value (`Transform::Map` holds the table), so one step never rewrites
  what another wrote; a composition that sends a newer value to two older
  ones, or reads two older values as one newer one, is refused at the
  step. Renaming an integer enum's value must keep its number, which older
  clients send; the compiler refuses one that changes it, at the step, and
  a rename between a string enum and an integer one, whose values change
  JSON type. A renamed or added value keying a map inside an error is
  reported, as one keying a map in an output is.

- The Rust server generator reports what it used to skip silently when it
  converts between API versions, so a release is never served wrong
  without a word: a step naming a shape or member its release does not
  have, which only an edited IR can hold, is an error for each operation
  it bears on (once for the step when neither release has the shape), and
  so is an operation gone without a handler or a retirement. A refused
  conversion no longer stops the other problems of its operation being
  collected. An enum value that only keys a map now counts as reached.

- `.cru`: `require S.m by hook` on a member that a `retype` of the same
  migration also names is refused by the compiler, at the requirement
  (`crucible check`, the build and the editor), instead of by the Rust
  server generator: the hook fills the member with the next version's
  value, which the retype would convert again. Declare one step for it.
  The same refusal for a renamed member was already the compiler's.

- IR (`crucible_ir::ir`, outside the stable surface): migration steps carry
  both their sides and their documentation, so nothing reads one release
  by the other's ids (decision 72). `VersionStep::Require` gains `old` and
  `from` (the member's older name when the migration renamed it),
  `Hook::Require` gains `from`, `AddEnumValue` gains `old`, `Move` gains
  `old_method` and `old_uri`, and `Hook::Reshape` gains `members` and
  `into`. `Hook::Conversion` is gone: a `retype` without `by hook` lowers to
  `Hook::Retype` with `by: RetypeBy::Conversion`, with the same sides as a
  hooked one. Every step has `docs` (`VersionStep::docs()`) and
  `ApiVersion` has `docs`, the migration's `///`, and `sunset_docs`, its
  `sunset` step's. Their `[Name]` doc links resolve in the current model
  like any other's, into `Service::doc_links`; a label naming what only the
  release the migration comes from had (a removed operation, a member's
  older name) is accepted as text, and one naming nothing in either is a
  compile error (decision 80). Only the compiled package's migrations are
  checked: an imported package's describe its own releases. Release snapshots are unchanged:
  `snapshot::write` leaves out `Service::versions`.

- **The built-in types and the bearer auth scheme are in the `crucible.api`
  namespace**, not `smithy.api`: `crucible.api#String`, `crucible.api#Unit`,
  `crucible.api#httpBearerAuth` (decision 62c in
  [docs/idl/decisions.md](docs/idl/decisions.md)). `.cru` source is
  unchanged. Every generated output that names a built-in spells it so:
  `describe.json` targets and `$defs` (`crucible.api.Long`), OpenAPI
  component and security scheme names, MCP tool schemas, the CLI schema and
  the runtime schemas native SDKs bundle (whose `schemaSha256` and content
  digests change with them). Release snapshots are format 5: an older one
  still reads, its `smithy.api#..` ids read as `crucible.api#..` in memory,
  so committed releases and their `crucible.lock` digests stay valid and the
  compatibility guard compares a `.cru`-frozen release with the current
  model as before (Leyline's and Pigeon's report the same findings). A
  release frozen from a Smithy model loses the client-only optionality of
  its `@input` and `@clientOptional` members, which read as the server
  sees them: against such a baseline the guard no longer reports the
  `compatibility.sdk` client-side break for a required member that was
  client-optional there (an SDK field that was `Option`/`Field` becomes
  required), and it may report `compatibility.nullability` for a
  `@clientOptional` output member that is optional now. The
  prelude namespace reserved for packages is `crucible.api`; `smithy.api`,
  which older snapshots and bindings and the `when_trait` shim still read,
  stays reserved. Infrastructure bindings that name a provider context
  type `smithy.api#String` read it as `crucible.api#String`. See [migrating](docs/migrating.md#the-built-in-vocabulary-is-crucibleapi).

- **Generated output says Crucible, not Smithy.** OpenAPI and JSON Schema
  extensions are `x-crucible-*` (`x-crucible-traits`, `x-crucible-errors`,
  `x-crucible-pagination`, `x-crucible-length`, `x-crucible-deprecation`,
  ...; formerly `x-smithy-*`) in OpenAPI documents, `describe.json`, MCP
  tool schemas and the CLI schema. The Python and C# clients' spans set
  `rpc.system` to `crucible`. Java, C# and Kotlin model docs say "Model
  shape", and the Python, R and TypeScript file headers and the R help
  title name the `.cru` model. The recursion diagnostic calls a
  collection-only cycle invalid, not "invalid Smithy".

- `.cru`: a `[Name]` doc link that names nothing is a compile error, reported
  at its label (decision 80); it was a warning (decision 41). So is
  `[Name.member]` naming a member, value or operation input or output member
  that does not exist. Fix the name, or write the text without brackets or in
  a code span.

- `.cru`: a resource identifier or property whose type is `internal` is a
  compile error: a resource is part of the external contract, which would
  name a type it leaves out (decision 67).

- External audience projection refuses an internal `creates`, `puts`,
  `reads`, `updates`, `deletes` or `lists` operation of a resource
  (`audience.resource_lifecycle`), now that `.cru` resources reach the IR: a
  public resource's lifecycle cannot lose an operation. Make such an
  operation an `on` action, or keep it public. An internal `on` action is
  removed with its resource bindings, and a binding whose path ran through a
  removed internal member is dropped.

- `describe.json` (`crucible.describe/1`): a resource's identifier and
  property targets may name a shape absent from `shapes`, a type only the
  resource names. Readers that look such a target up must allow for it.

- `crucible freeze` can no longer restore the deleted file of a release
  frozen before resources were lowered: the snapshot it writes now carries
  the resources, so it differs from the digest the lock records. Restore the
  file from version control instead.

- The API reference lists a resource's lifecycle table only when one of its
  lifecycle slots is filled; a resource with custom actions only lists them.

- The `crucible-smithy` crate is renamed `crucible-ir` (library `crucible_ir`): it holds the
  IR, snapshots and the helpers every generator shares, and no longer reads
  Smithy. Update the dependency and `use crucible_smithy::` paths.

- The compatibility guard counts a migration's `rename` steps toward
  `compatibility.pagination`: when renaming a paginated operation's input
  or output token, items or page size member accounts for the whole
  pagination change, the renames declare it, as they already convert the
  member for older callers, and no policy exception is needed. A
  pagination change the renames do not explain is still reported.

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

- Building a service fails, naming the operations, when a served operation
  declares requirements (`Requirements::List`) and no `Permissions` covers
  it at every API version that serves it: none installed, the
  `permissions` feature off, or `Permissions` scoped away (`only`,
  `except`, `when`, `when_trait`, each asked of the description a request
  at that version carries). Before, such an operation ran with its
  declared checks unchecked. `requires pending`, `requires none`, public
  and undeclared operations may stay outside it (an undeclared one then
  runs without authorization); `Permissions::shadow()` is the explicit way
  to record without refusing.
  Registering `PageTokens` before `Permissions` now fails the build too, as
  the documentation already said.

- `McpAuthenticator` accepts an `aud` sent as a one-member array holding the
  MCP resource, which RFC 7519 makes equivalent to the string. A token for
  several audiences is still refused.

- The documentation, CDK, Terraform and Explorer generators need a `.cru`
  package with resource lowering (decision 63), which the language does not
  have yet. Their resource-dependent tests are `#[ignore]`d until it lands,
  and `generate docs`, `cdk` and `terraform` refuse a `.cru` package for now.
  Their goldens are kept.

### Deprecated

- `{OperationMiddlewareExt, InterceptorExt, ProtocolExt}::when_trait` with
  `"smithy.api#readonly"` or `"smithy.api#idempotent"`: those ids name no
  trait now that the prelude is `crucible.api`. They, and their
  `crucible.api` spellings, still select the readonly or idempotent
  operations, logging a deprecation warning when the scope is built, until
  the next minor release. Use `.when(|op| op.readonly)` and
  `.when(|op| op.idempotent)`. Any other built-in id selects no operation,
  with a warning; `when_trait` with a model's own trait id is unchanged.
  When the shim is removed, `when_trait` with any id in the `smithy.api` or
  `crucible.api` namespace panics when the scope is built, rather than
  scoping an extension (a cache, a retry policy) to no operation.

### Removed

- `crucible_ir::ir::Service::renames`, `Member::client_optional` and
  `Member::{client_default, client_wire_default}`: a `.cru` model never set
  them (decision 62c). `Service::name_of` is the name in the id, and
  `optional_on_client` equals `optional_on_server`. Release snapshots drop
  `client_optional` and an empty `renames` when read, and refuse one that
  renamed a shape. `describe.json` keeps `clientOptional`, now always
  `false`. The unreferenced Smithy JSON AST fixtures
  (`tests/conformance/native-docs.json`,
  `e2e/weather-service/tests/fixtures/auth-chain.json`,
  `crates/crucible-codegen-common/tests/fixtures/recursion.json`) are gone.

- **The Smithy front end.** Crucible reads `.cru` packages only; Leyline and
  Pigeon are on `.cru`. Gone: the Smithy JSON AST front end and
  `Model::from_json`, `.smithy` and `model.json` inputs of the CLI and
  `crucible_build::model(..)` (`crucible diff`, `check`, `generate` and
  `scaffold` take a package directory), the JVM and Gradle build driver with
  its managed Java runtime and `smithy-build.json`, the `crucible-traits`
  Cargo package and model jar with its `traits-v*` releases, the
  `crucible new` Smithy starter and `--smithy-version`, the `smithy-tooling`
  vendor crate, and the one-shot `smithy-to-cru` converter. `crucible-smithy`
  keeps the IR, the snapshot format and the shared validators (it is renamed
  `crucible-ir` later).

- **Features only the Smithy front end could express and nothing used:**
  `@crucible.auth#scopes` and OAuth scopes declared in the model (and the
  model-driven scope documentation), the IR's `rename`, timestamp defaults,
  enum `mediaType`, and 2xx error statuses.

- **Every wire protocol except Crucible JSON.** Crucible's own JSON protocol
  (`@crucible.protocols#json`, `.cru` `protocol json`) and the direct
  `InvokeProtocol` are all that remain. Gone from the Rust server runtime,
  the Rust client runtime, `crucible-wire` and every client generator and
  runtime (Rust, TypeScript, Python, Go, Java, Kotlin, C#, Swift, Dart, PHP,
  Ruby, C++, C, R), the OpenAPI, documentation, CLI and Terraform
  generators and the explorer: `aws.protocols#restJson1`, `awsJson1_0`,
  `awsJson1_1`, `restXml`, `awsQuery` and `ec2Query`, and
  `smithy.protocols#rpcv2Cbor` and `rpcv2Json` (Smithy RPC v2), with their
  routing, the `Smithy-Protocol` and `X-Amz-Target` headers, the CBOR codec
  (`crucible_wire::cbor`, the `ciborium` dependency and the C#/PHP CBOR
  runtimes), XML shapes, namespaces and serialization, AWS query
  compatibility, and the settings that only they used (non-finite floats as
  strings, arbitrary-precision numbers as strings, omitted null members,
  unknown union members, XML error unwrapping). The wire behaviour of
  Crucible JSON is unchanged.
- **The Smithy protocol test suites.** `smithy.test` request/response/
  malformed-message vectors are no longer lowered (`Service::protocol_tests`
  and the `HttpProtocolTest` types are gone), `crucible-codegen-rust` no
  longer generates server conformance tests from them, `crucible export`
  no longer writes `protocol-tests.json`, and the conformance harness,
  `e2e/rpcv2-json-service`, `tools/protocol_conformance.py` with its
  adapters, `tools/xml_qualification.py`, `tools/import-smithy-xml.py` and
  `docs/xml-qualification.md` are deleted.
- **IR and API.** `crucible_smithy::Protocol` has the single variant
  `CrucibleJson`; `Protocol::{uses_http_bindings, is_rpc_v2,
  respects_timestamp_format, uses_json_names, request_headers}`,
  `Model::lower_with_protocol` and `lower_infrastructure_with_protocol` are
  removed, and `Model::lower_documentation` no longer takes a protocol.
  `ProtocolSettings` keeps only `http` and `event_stream_http`. `Service`
  loses `protocols`, `xml_namespace`, `xml_shapes` and `protocol_tests`,
  `Operation` loses `unwrapped_xml_output`, `EventStreamBinding` loses
  `initial_message`, and `Feature` loses the eight protocol variants. The
  runtime schema no longer carries `protocol`, `protocolSettings`,
  `xmlNamespace`, `xmlShapes`, `unwrappedXmlOutput` or `protocolTests`.
- **Build and CLI surface.** `crucible_build`'s `.protocol(..)` and server
  `.conformance()`, `.skip_vector()` and `.seed_request()` builders and
  `render_conformance`, the `--protocol` option of `crucible generate` and
  `crucible check`, and the `crucible conformance` subcommand.

- **Every feature only a Smithy model could reach.** Crucible's own
  language and the two services built with it (Leyline and Pigeon) use none
  of these, so they are gone from the IR, the Smithy lowering, the Rust
  server runtime, the Rust client runtime, `crucible-wire`, every client
  generator and bundled runtime (Rust, TypeScript, Python, Go, Java, Kotlin,
  C#, Swift, Dart, PHP, Ruby, C++, C, R), the OpenAPI, documentation, CLI
  and infrastructure generators, the explorer and the docs:
  - **Streaming and event streams.** `@streaming` blobs and unions,
    `@requiresLength`, `@eventHeader`/`@eventPayload`, the `ByteStream`,
    `ChunkStream`, `EventStream`, `EventReceiver` and `EventStreamError`
    types (`crucible_wire::{stream, event_stream}`), `Location::
    {StreamingPayload, EventStream}`, `PayloadKind::EventStream`,
    `EventStreamBinding`, event signing and the `INPUT_EVENTS`/
    `OUTPUT_EVENTS` operation constants.
  - **Waiters.** `smithy.waiters#waitable`, `wait_until_*` methods,
    `WaiterOptions`/`WaiterResult`/`WaiterError`, and the bundled JMESPath
    evaluators (the `jmespath` dependency).
  - **Endpoint rule sets, `@endpoint` host prefixes and endpoint
    discovery.** `ServiceMetadata`/`OperationMetadata` of the Rust client
    and `Transport::send_with_metadata`, `Config::{region,
    endpoint_parameter, endpoint_builtin, disable_host_prefix}`, endpoint
    caches, the `endpoint` retry reason and the `endpointSource` telemetry
    field.
  - **Authentication other than bearer.** SigV4 and SigV4a, HTTP Basic,
    Digest and API-key client schemes, `@optionalAuth` and
    `aws.auth#unsignedPayload`: `AuthScheme` has the single variant
    `Bearer`; `Config::{basic_credentials, api_key, aws_credentials,
    signing_name}`, `AwsCredentials`, `BasicCredentials`, credential
    providers and the `content_sha256` client option of every language but
    TypeScript and Rust (and the `--content-sha256` flag of `crucible
    generate`); `OperationMetadata::{optional_auth, unsigned_payload}`.
  - **HTTP checksums.** `@httpChecksum`, `@httpChecksumRequired`,
    `ChecksumPolicy`, `Config::on_checksum_validated`,
    `operation::{Checksum, ChecksumAlgorithm}`.
  - **Smithy HTTP bindings and names.** `@httpPrefixHeaders`,
    `@httpQueryParams`, `@httpResponseCode` (and
    `ModelledError::{response_status, write_http, write_http_typed}`,
    `Output::{response_status, write_http}`, `ResponseBindings`,
    `split::write_http`), `@jsonName` (a member's wire name is its name:
    `Member::wire_name` and `PaginationMember::wire_name` are gone).
  - **Sets and legacy enums.** Set shapes (`List::is_set`; a list with
    `unique` is the replacement) and legacy `@enum` strings. Sparse
    collections stay.
  - **CORS.** `@cors`, `Service::cors`, `CorsInfo` and the built-in
    `middlewares::Cors`.
  - **Other.** The generated `MCP_EXCLUSIONS` static (it listed only
    streaming operations), `Feature` loses everything but `GreedyLabels`,
    `IntEnum`, `BigInteger`, `BigDecimal`, `NonBlobPayload`, `ErrorPayload`
    and `MediaTypeHeaders`, `ProtocolSettings::event_stream_http`, the
    `describe/1` keys of the removed features (`wireName` stays and equals
    `name`), and the Smithy-only test fixtures that exercised them.
  A model that uses one of them is refused with a diagnostic naming
  the trait (Smithy models are no longer read at all, see below). Release snapshots move to format 3: older snapshots still
  read, with the unused fields dropped, and a snapshot that used a removed
  feature is refused with a message naming it.

### Fixed

- A project's own traits in an `aws.*` or `smithy.test` namespace are kept:
  `Service::project_metadata_only`, `crucible check`'s custom trait count and
  the traits the Rust generator records for `when_trait` treated them as
  standard, as only a Smithy model needed. An external audience view keeps a
  project's trait in any namespace but the prelude and `crucible.protocols`
  (it dropped one in a `crucible.*`, `aws.*` or `smithy.*` namespace) unless
  its definition is `@internal`.

- `crucible check` without `--target` accepts every capability some generator
  supports. It checked against an empty set, so a model using a gated feature
  such as a `.cru` `sparse list` failed with "requires unsupported target
  capability sparse" although every generator handles it. `--target <client>`
  still narrows the check to that client.

- Framework errors under the Crucible JSON protocol (`crucible.protocols#json`)
  now send `X-Crucible-Error`, as modelled errors and `NotFoundException`
  already did (#308). The 401, 403, 405, 409 (including idempotency
  conflicts), 415 and 500 the framework answers itself carried only the
  body's `__type`, which clients read only as a fallback; the protocol makes
  the header the discriminator.

- A `rename` step declares a member's new name and nothing else (#307).
  The compatibility guard used to accept any other change made to the
  renamed member along with it: an optional input member that became
  required, an output member that lost its non-null guarantee, or a new
  type. So a service that renamed `description` to `notes` and made it
  required refused release-1 requests that omitted it, as `notes: is
  required`. The checker now compares a renamed member with its new self,
  as it compares any other member, and each such change needs its own
  step. Those steps name the member as it is now, through the rename:
  `rename S.description -> notes` with `require S.notes default ""`, or
  with `retype S.notes -> T by hook`, whose hooks read the older value
  under its old name. A step that names the old member is refused with
  the new name, and so is `require .. by hook` on a renamed member, which
  the conversion does not support yet. `crucible_policy::diff_renamed`
  takes the renames; `diff` is unchanged.

- A Go client call with a streaming request body now returns as soon as its
  context is cancelled or its deadline passes, even while the transport is
  blocked reading the caller's stream, when that stream is an `io.Closer`
  whose `Close` ends a blocked `Read` (#298). `net/http` closes the
  connection on cancellation, but `Do` waits for its write loop, and that
  loop stayed blocked reading a pipe that would get no more bytes. So the
  call hung until the caller wrote again or closed the pipe itself. The
  attempt's body is now closed when the context ends. That closes a one-shot
  `io.Closer`, or the reader a `NewByteStream` factory opened, and ends the
  read. A reader that is not an `io.Closer` still holds a cancelled call
  until its pending `Read` returns. Every return, including an early error
  response, also closes the attempt's body, so the transport stops reading
  the caller's stream once the call has ended. The exception is a streamed
  response: there the body stays open until the response ends or is closed.

- An idempotency store outage, or an Authorizer outage, can no longer
  become a modelled 4xx through the service's exception rules. Both are
  wrapped in `OpaqueError`, as `AuthenticatorChain` wraps an authentication
  outage, so a rule that answers the service's own database-client errors
  with, say, a 409 no longer matches the store's error on the same client
  inside the outage. They stay the service's internal error.

- Typed permissions in client output no longer leak or understate internal
  requirements (#258). A requirement list that mixes public and internal
  clauses projects to the new `Requirements::Partial` for clients: its
  sentence ends "; and further permissions internal to the service.", and
  `describe.json` `requirements` and OpenAPI `x-crucible-requires` add
  `partial: true`. An operation whose requirements are all internal now
  says "Requires permissions internal to the service." in every SDK's doc
  comments and MCP tool descriptions, not only in the reference. SDK doc
  comments name only what the SDK's registry has, so an internal-audience
  SDK no longer names internal permissions there (`requirement_sentences`
  and `requirement_paragraph` in `crucible-codegen-common` take an
  `internal` flag, as `registry` does). A clause clients see may no longer
  bind or test an `internal` member, or compare with an `internal` enum
  value (`permissions.internal-member`); for a permission other clauses
  also require, the advice no longer suggests making it `internal`, which
  would hide those too. The client projection drops `@permissionTypes`
  exclusions of resources it removed, and drops the enum when a grantable
  resource's value of it was internal. A `filters` clause on an internal
  permission narrows a listing and refuses nothing, so leaving it out no
  longer makes the list partial; a listing that only filters by internal
  permissions requires none in the client view. `compatibility.authorization`
  compares client-audience models too (an SDK release plan does): a list
  that becomes partial, or requirements that become internal, is reported
  as an added requirement on permissions internal to the service. See
  [docs/permissions.md](docs/permissions.md).

- `AuthenticatorChain` checked a request at an older API version against
  the current version's authentication plan when its scope took both
  versions (an unscoped chain, say): plans were keyed by operation id, and
  `Prepare::operations()` lists an id once, by its current description. An
  operation v1 required bearer auth on and v2 made `@optionalAuth` (or
  dropped auth from) let a v1 request without credentials through; one v1
  served without auth and v2 required it on refused v1's clients. The chain
  now prepares one plan per served entry (`Prepare::served_operations()`)
  and selects it by the description the request is served under, so each
  version is authenticated by its own release's schemes, as
  [API versions](docs/api-versions.md#limits) promise. A construction error
  for an older version's entry names its API version. The chain now needs
  an authenticator for every scheme an older version's entry declares, so
  a service whose older release used a scheme no installed authenticator
  supports, which built before, now fails to construct until one is added.

- A structure is read only from a JSON object. serde's derived struct
  reader also takes an array of positional values, so a JSON body, a
  structure `@httpPayload` or a nested member sent as `[]` or
  `["Oslo", 1]` was accepted as that structure, under every JSON protocol
  (Crucible JSON, AWS REST JSON, AWS JSON 1.0/1.1, RPC v2 JSON, direct
  invocation and input events), in the Rust client's response reader and
  in a generated service CLI's `--input-json` and MCP `tools/call` input.
  It is now a malformed request (or an unreadable response), as RPC v2
  CBOR already treated it. A document member still takes any JSON value.
  (#292)

- A Crucible service accepts an explicitly null structure or union
  `@httpPayload`. Rust, TypeScript, Python and Go clients send one under
  Crucible JSON as the body `null`, which the server refused with a 400
  (`invalid type: null, expected struct`). A JSON payload of `null` now reads
  as the member's explicit null, exactly as `{"member": null}` would in a
  body: an optional structure or union is absent on the server and
  `Field::Null` in a Rust client, which also reads a `null` response
  payload. A document payload follows the same body rule: a Crucible JSON
  server reads `null` as absent (an AWS REST JSON server still keeps
  `Some(Value::Null)`), and a Rust client reads it as `Field::Null` rather
  than `Field::Value(Value::Null)`. Both document changes alter 0.7.0
  behaviour; see Migration.

- Typed permissions in `.cru` (unreleased) check what they let through
  before. A resource that declares permissions declares an identifier of
  its own (`permissions.identifier`): without one, `R(*)` bound the
  parent's identifier, a wildcard over every tenant. `@aliasOf` and
  `@authorizationName` given twice, a `@permissionTypes` enum of another
  package, a permission named `none` or `pending` (`permissions.symbol`),
  `requires none` with a target, `when` or `by handler`, `requires
  pending` without `because`, and `filters p on R` on a listing of another
  resource than `R` are errors. Operations and resources the service
  does not reach get the permission checks too, unless another service of
  the package reaches them. A permission annotation on another target
  says where it applies, rather than suggesting a metadata key that cannot
  take a built-in's name. `when has tags` may test a list or map member; a
  member named `has` or `not` reads as one in a `when`; a binding type is
  checked against an identifier type nothing else uses (a warning when the
  constraints match, as for any other); a constrained string binding an
  unconstrained `string` identifier is an error, where a warning claimed
  the same constraints; and a `when` that fails to lower drops its clause
  from the editor's best-effort IR instead of keeping it unconditional.
  The formatter indents a requirement's `on` and `when` continued on the
  next line.

- A JWKS fetch whose key set holds one key that does not parse (a corrupt
  entry, or a key type `crucible-auth` cannot read) skips that key with a
  warning and keeps the others. It used to refuse the whole set, which was
  an outage for every token. A `kid` named twice is still refused, even when
  one of the two entries does not parse. `JwtAuthenticator::with_jwks`
  refuses a key that does not parse as a configuration error.

- The `ScopeEnforcer::exempt_id_tokens` and `strict` docs said API keys are
  "checked": an API key carries no scopes, so both refuse it on every
  operation that declares scopes. `docs/auth.md` says so and shows the
  `ScopelessPolicy::Exempt` that admits API keys.

- Typed permissions, the request's session (unreleased since #246):
  `Permissions::shadow()` refuses nothing, also when the Authorizer cannot
  answer a read-only operation's `filters` listing (the handler's
  `permissions::visible()` shows nothing instead of failing, and that
  success is never replayed) or a replay's handler checks (the replay is
  served). An operation that changes state still fails when its listing
  cannot be answered, since on an empty listing it would succeed as a
  no-op. A success with a `by handler` obligation unsatisfied now fails
  in release builds too, not only in debug builds: `assert_obligations`
  defaults to `true`, and `false` is the deliberate opt-out. So does a
  success whose `filters` listing shows less than everything and was never
  read with `visible()`, which `idempotency::commit()` also refuses. A
  replay of a filtered listing is refused as unreplayable (409) unless the
  caller now sees all of it, or the same ids the stored success showed; the
  idempotency record's `authorization` becomes `{ "checks", "listing" }`
  (a digest of the ids), and a record in the earlier array form still
  reads. A `discharge` holds its obligation while it asks the Authorizer,
  so a concurrent `not_applicable` or `discharge` of it is refused instead
  of racing past a refusal, and an obligation is only ever closed from
  open. Stored handler checks are kept once per permission and resource.
  Identifiers in permission log lines and internal errors are escaped and
  cut short. `permissions::decide` answers a `#[must_use]` `Verdicts`
  (a slice of `Verdict`, `into_vec`), so dropping its answer unread is a
  warning; `Verdict` and `Visibility` are `#[non_exhaustive]`.
  `PermissionRefusal::resource` no longer claims its identifiers came from
  the request: a handler check's or an obligation's are what the handler
  bound. The record is published once per hook or handler call, and the
  interceptor's settings are shared by one `Arc` per request, instead of
  cloned on every change.

- Typed permissions in generated code (#259). A handler clause's
  documentation keeps its case (`HTTP` no longer becomes `hTTP`); one
  paragraph still reads inline, and anything longer follows the requirement
  sentence whole instead of being flattened into parentheses. A backtick in
  a permission value, a `when` literal or clause documentation gets a
  longer code-span fence. Authorization names read as words in refusals and
  prose, acronyms included (`No such HTTP request.`). Requirement sentences
  are rendered by each target's documentation dialect (KDoc, Javadoc, C#
  XML) like model documentation, and `describe.json` adds
  `requirementDocumentation`, the sentences as `{markdown, html, text}`
  objects, which the explorer renders instead of splitting
  `requirementSentences` on backticks. The API reference's
  Requirements table puts a clause's documentation in a new Notes column,
  not under When. One rule decides `deprecated` (the model's note, or an
  alias) and `grantable` (the primary's tier) for every registry, so
  `describe.json`, OpenAPI and the bundled runtimes now mark an alias
  deprecated as the Rust and TypeScript registries did; their `PERMISSIONS`
  docs no longer claim non-grantable entries are left out. Rust client
  alias variants say what they alias, and permission documentation is
  escaped as model text in the Rust and TypeScript constants, as a handler
  clause's documentation now is in every requirement sentence (a
  line-start `@see` is no JSDoc tag, a bare `[ops]` no rustdoc link). The
  shared model-text escape now ends a code span only on a backtick run as
  long as the one that opened it, and plain-text documentation no longer
  leaves two blank lines after a list. A TypeScript
  permission export a model already names (`GraphPermission`) is left out
  of the package root and reachable as `permissions.GraphPermission`,
  instead of failing generation; a symbol with no letters (`_`) is the
  constant `_`. In the Rust server bindings, a resource named `Self` is the
  `Self_` variant of `Permission` and `Resource`, an obligation's ordinal
  never repeats a name another clause spells, and a float `when` literal
  outside `f32` is a diagnostic. An older release's legacy operation is
  refused when the resource's identifiers changed in any way, not only in
  number, since its bindings are positional, or when the resource moved to
  other parents: releases now record each resource's parents (by
  authorization name, or shape name for a parent without permissions; a
  parent's shape rename that keeps its authorization name is no move), so
  a move to a parent whose identifier has the same name, or a root moved
  under a parent without identifiers, is caught too. A release frozen
  before parents were recorded is compared by identifiers alone.

- The Rust client's default HTTP transport no longer follows redirects,
  which could forward custom API-key and SigV4 session-token headers to
  another origin. Applications can configure an explicit redirect policy
  with `Config::http_client`.

- TypeScript clients no longer follow redirects, which forwarded bearer,
  API-key and SigV4 headers to wherever the `Location` pointed. Every
  request the client dispatches, including one a middleware rebuilt,
  carries `redirect: "manual"`; the 3xx (an opaque redirect, status 0, in a
  browser) surfaces as a `ServiceError`. A custom `fetch` may still choose
  its own redirect mode.

- A client-wide default `Content-Type` header no longer replaces a
  payload's media type in the Rust, Python, Go and TypeScript clients, and
  no longer replaces `application/json` on an ordinary TypeScript request
  body (a mismatch the service answered with HTTP 415). A payload request's
  Content-Type is the modeled Content-Type member if one is set, then the
  payload's media type, then default headers; TypeScript per-call
  `RequestOptions.headers` rank with the defaults. An ordinary JSON document
  body always carries its protocol's media type. Go also canonicalizes
  `Config.Headers` keys, so a default given as `content-type` is replaced
  rather than sent beside the request's own. To send a payload under
  another media type, model a Content-Type member or set the header in a
  middleware or interceptor.

- Java, C#, Kotlin, Swift, Dart, PHP, Ruby, C++, C and R clients now send a
  modeled Content-Type header member on a payload request instead of
  replacing it with the payload's media type (a Kotlin upload with
  `contentType = "image/png"` was sent as `application/octet-stream`). They
  follow the same order as the Rust, Python, Go and TypeScript clients: the
  modeled member, then the payload's media type, then default headers, with
  exactly one Content-Type sent whatever case the default was given in.
  Kotlin and C++ streaming uploads, which sent no Content-Type unless a
  default supplied one, now carry the payload's media type. A Java header
  member, JSON body or XML body replaces a default of the same name in any
  case, so it never sends a duplicate. A C# client sends an empty or
  malformed modeled Content-Type as given, as the Python and Go clients do,
  instead of throwing `FormatException`.

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
  representation. As in a member's default, an unquoted enum literal names
  a symbol and a quoted string is the wire value itself, even when it
  spells another symbol's name.

- `.cru` member and migration defaults resolve enum symbols at any depth:
  `[RED]` for a list of enums, or `{ color: RED }` in a structure, lowers
  to RED's wire value instead of the text `"RED"`, which was refused or,
  when another value's wire value spelled it, silently meant that value.
  An unquoted key of a map keyed by an enum names a symbol too
  (`{ RED: "x" }` is keyed by RED's wire value; `{ "RED": "x" }` by `RED`),
  and two keys that name one wire value are refused rather than one of
  them dropped. A migration's `require ... default null` is refused for a
  `document` member as for any other: `require` makes the member required,
  and a document's null is a value only where the member is optional.

- Examples and defaults may set a `document` member that is optional on
  the server (neither required nor defaulted) to `null`; the null is kept
  as the document's value. This is narrower than Smithy's example
  validation, which reads nullability as a client does and so also accepts
  null for a required member of an `@input` structure or a
  `@clientOptional` one: the server requires those, and would refuse the
  example's request. A member of any other type still may not be null.

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
  is not, from any event source, or a `Host` or `X-Forwarded-Proto` no
  URI can be built from) is answered 400 with the request id instead of
  failing the invocation. The host refuses those before the conversion
  rather than catching its panic, so a `panic = "abort"` build answers
  them too. A path with such characters (a space, `<`, non-ASCII, a `?`
  or `#` an HTTP API decoded) is percent-encoded the same way: it failed
  the invocation with a bare 502. A `%` in the path is encoded too where
  the source decoded the path (an HTTP API, payload format 2.0 or 1.0),
  since it is the client's `%25`, and kept as an escape where the source
  passes the path as sent (a REST API, an ALB, a Function URL, told from
  an HTTP API by its `*.lambda-url.<region>.on.aws` domain name).

- An answer larger than a Lambda invocation returns (6 MB, the event's
  shape and base64 included) was handed to the platform, which refused it
  after the service log had recorded the request as served: the caller
  got a bare 502 from the Function URL or CloudFront and nothing said
  why. The Lambda host now answers 500 `InternalFailure` in the event's
  shape with the request id (the failure envelope for a direct invoke)
  and logs the size through `tracing`: `body_size` when the body alone
  passed the limit and its length was known ahead, `body_at_least` when
  it was a stream, `answer_size` when the body fit and the answer around
  it did not; never the limit plus one. An ALB event's
  answer is held to the load balancer's 1 MB limit, not Lambda's 6.

- An operation that only an older API version serves (a later release
  removed it) answered 500 once any operation-aware middleware was
  registered: `Prepare::operations()` listed the current version's
  operations alone, so the authentication chain had no plan for it and
  failed with "authentication plan missing". It now lists those operations
  too, after the current ones, and `Prepare::metadata` returns their own
  release's metadata. An operation in both is listed once, by the current
  description, unless the extension's scope excludes it and includes an
  older one (a scope on a member a later release renamed): it is then
  listed by the older one, so that version's requests find their plan.
  `Prepare::metadata` answers for the description it is given, an older
  version's entry included, so such a plan is prepared from that
  version's own authentication schemes rather than the current
  version's (an extension scoped to a version that required
  authentication no longer prepares it as optional).

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
  Swift and C++ escape the same characters their own way. A PHP literal
  that carries a control character (CR, a line break), a line or paragraph
  separator, a byte order mark or a bidirectional control is double-quoted
  with `\u{...}` escapes, so a committed SDK keeps the value through
  `git`'s line-ending normalisation; any other stays single-quoted.
  Bidirectional
  controls (U+202A–U+202E, U+2066–U+2069, and the LRM, RLM and ALM marks)
  are dropped from documentation and escaped in literals, since rustc's
  deny-by-default `text_direction_codepoint_in_*` lints and GCC/Clang's
  `-Wbidi-chars` refuse them. The Terraform provider and CDK constructs
  escape their embedded literals and descriptors the same way (a BOM broke
  `go build`). A model line starting with `@` no longer becomes a tag in
  TypeScript JSDoc, a Kotlin deprecation note or a PHP waiter's docblock,
  whose documentation is now escaped like the rest; in TypeScript, PHP and
  Ruby, whose doc tools read such a line as a tag even inside a code fence,
  it is escaped there too (`docs::escape_block`). Ruby literals that
  carry a line break or an invisible character are double-quoted with
  escapes: Ruby's lexer read a raw CRLF as LF even in a single-quoted
  literal, so an enum value of `a`, CRLF, `b` was sent with LF alone. The
  Rust headers that name the service version, and the scaffold's `//!`
  summary, carry model text on one comment line. The R client refuses a
  NUL anywhere in
  its embedded schema, not only in shapes and operations. And the Rust
  generators diagnose a model type named for a Rust keyword (`type`,
  `match`, `Self`), which previously generated uncompilable code: rename the
  shape. `fixtures/hostile-text.json` covers documentation, a deprecation
  message, a waiter's documentation, the service version, enum values, a
  default, a pattern and a JSON name; the hostile-text test also runs the
  Rust server, its operation stubs, Terraform, CDK and the service CLI.

- Rust server and client bindings escape every shape's, member's,
  operation's and error's model documentation for rustdoc, as the
  permission documentation already was: a bare `[name]` is no longer an
  intra-doc link that cannot resolve.

- `describe.json` requirement clauses add `renderedDocumentation`, the
  clause's documentation as a `{markdown, html, text}` object, and the
  explorer renders it in the Requirements table's Notes column instead of
  showing the Markdown source.

### Security

- Generated Rust server models, errors, unions and enums write their own
  `Debug`, as the generated clients already did, so a handler that logs
  `?input` prints `"*** Sensitive Data Redacted ***"` in place of a
  `@sensitive` member, a member whose target is sensitive, a member of a
  sensitive structure, a list or map of sensitive values, or a sensitive
  union arm; a sensitive enum or intEnum prints as
  `Name(*** Sensitive Data Redacted ***)`. A modelled error's `Display`
  redacts a sensitive `message` too, so the service log's source chain no
  longer prints it; the response body still carries it. Before, server
  models derived `Debug` and printed every value. The `Debug` output now names fields by
  their model member name (`cityId`), as the client's does, instead of the
  Rust field name (`city_id`); `Debug` output is not a stable format.

- An authentication provider's outage can no longer become a modelled 4xx
  through the service's exception rules. `AuthenticatorChain` wraps it in
  `OpaqueError`, so a rule written for the service's own database-client
  errors no longer matches the key store's error on the same client inside
  the outage.

- A token with a forged `kid` for `JwtAuthenticator`'s issuer no longer starts
  the 30-second `min_refresh_interval` cooldown for every key ID. A `kid` a
  successful fetch did not contain waits the interval before fetching again;
  one not looked up yet may fetch one second after the last fetch. A key the
  provider rotates in is accepted within a second of the last fetch, where it
  used to be refused for up to the interval after any forged token, unless
  its `kid` was looked up before the provider published it: that `kid` is
  remembered as missing and waits the full interval. Forged key IDs can now
  cause about one fetch a second rather than one per interval, which raises
  the chance of the identity provider throttling the JWKS endpoint, and a
  failed fetch still holds every fetch off for the interval.

- A rejection lists at most 100 violations
  (`validate::MAX_VIOLATIONS`), then one more counting the rest. One
  broken constraint on a list's items used to fail once per item, and the
  response, the service log and the validation error's `fieldList` each
  copied every violation. So a body of a few megabytes of empty strings
  could build an error response, and a log record, of hundreds of
  megabytes. The cap covers violations from the query string, labels and
  headers too (`?a=%zz&a=%zz...` once listed one per pair): every
  framework producer collects through the new `validate::Violations` or
  `Rejection::new`, and so does every other one: `Rejection`'s violations
  are private now, read with `Rejection::violations()`, so a custom protocol
  or extension cannot build one around the cap (see *Migration*).
  `Rejection::summary` counts the unlisted violations ("and 149 more" for
  150), and `Rejection::count` says how many there were.
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
  - Request text a client controls is not repeated either. An unsupported
    `Crucible-Version` is refused by listing the versions served. An unknown
    member of a strict structure is reported at `[key]` (JSON and CBOR
    alike; a missing CBOR member is now `is required` at its name, as in
    JSON). A malformed query value is reported at the model's parameter
    name, at `<member>/[key]` for a key of an `@httpQueryParams` map, and at
    `query` for an undeclared parameter; a malformed prefixed header at
    `<member>/[key]`. Event stream frames and received events are refused
    without the sender's `:message-type`, checksum bytes or values. The
    TypeScript client's event stream reader no longer repeats an unknown
    `:message-type` or header value type either.
  - A violation's path names a map key `[key]` when the key is client data
    under a sensitivity rule: the map's keys are `@sensitive`, or the map is
    reached through a `@sensitive` member or structure (an ordinary
    structure behind a sensitive member included), or is the item of a list
    or map whose items are sensitive (`scoreMaps/outer/[key]`). Generated validation
    opens the new `Validator::sensitive` scope for them. Member names, list
    indexes and public map keys still appear. An enum map key is hidden the
    same way: it used to be named like a member, so a sensitive scope let it
    through.

  The model's expectations are unchanged. Paths are unchanged except where
  they named client text: see *Migration*.

### Migration

- **The CLI loads a package's older releases, as a build does.**
  `crucible check`, `generate` and `compile` used to read only the current
  model. Now:
  - `crucible generate rust` (a server) for a package with migrations
    serves its older releases. The service implements the generated
    `versions::Migrations` trait (each migration's hooks), as a build's
    server already does.
  - A `retype` without `by hook` names a declared `conversion`, which the
    Rust server generator does not generate yet, so `generate rust` refuses
    it: add `by hook` to the step for a server.
  - A chain the build refuses fails `check` and every `generate`: a
    migration whose `from` release is not frozen under `releases/`, or an
    operation dropped without `remove operation`. Freeze the release
    (`crucible freeze --from <git ref>`) or declare the step.

- **An operation dropped without `remove operation` no longer builds.**
  Before, a migration that left out the step built, and the server served
  the operation at no older release without saying so; a build that did so
  now fails at the migration's `from`. Declare `remove operation Op` and
  write the handler the older releases are served by, or, to stop serving
  it there, except the drop in `crucible.toml`:

  ```toml
  [[exceptions]]
  rule = "compatibility.operation"
  shape = "example.shop#CountItems"
  reason = "CountItems is retired at every version"
  ```

  The exception stays while a release that had the operation is supported.

- **The built-in vocabulary is `crucible.api`.** A tool that reads
  generated output and recognised a built-in by the `smithy.api#` prefix
  (in `describe.json`, OpenAPI, MCP schemas or an SDK's bundled schema)
  matches `crucible.api#`, and one that read an `x-smithy-*` OpenAPI or JSON
  Schema extension reads `x-crucible-*`. Both appear as keys as well as
  values: shape maps and `$defs`/`components.schemas` are keyed by the
  built-in's id (`crucible.api#Long`, `crucible.api.Long`), and the
  extensions are object keys inside the embedded schemas. Client telemetry dashboards that
  filter on `rpc.system = smithy` filter on `crucible`. A Rust service
  replaces `when_trait("smithy.api#readonly")` and
  `when_trait("smithy.api#idempotent")` with `.when(|op| op.readonly)` and
  `.when(|op| op.idempotent)` before the next minor release; until then the
  old ids work with a warning. Infrastructure bindings files may keep
  `smithy.api#String` as a provider context `target`; it reads as
  `crucible.api#String`. A package in the namespace `crucible.api`, now reserved for the prelude,
  moves to another. `describe.json` stays format 1: only the spelling of
  built-in ids and extension names in its values changed. Committed release
  snapshots need nothing: they read as before. See
  [migrating](docs/migrating.md#the-built-in-vocabulary-is-crucibleapi).

- **A `null` document payload reads as the member's null.** In 0.7.0 a
  document `@httpPayload` sent as the body `null` read as a document value:
  `Some(Value::Null)` in a Crucible JSON server's `Option<Value>` member,
  and `Field::Value(Value::Null)` in a Rust client's response member. It
  now reads as the same member would from `{"member": null}` in a body:
  `None` on the server (an AWS REST JSON server keeps `Some(Value::Null)`),
  and `Field::Null` in the client. A server that told an explicit null
  document apart from an absent one can no longer do so for this payload
  (as it already could not in a body); client code that matched
  `Field::Value(Value::Null)` should use `is_null()`, or `value()`, which
  returns `None` for both. No shim is possible: the change is in how the
  wire is read. See [migrating](docs/migrating.md#unreleased).

- **`FrameworkError` has `Conflict`, `NotFound` and `PermissionDenied`
  variants, and is `#[non_exhaustive]`.** A `match` over `FrameworkError`
  outside `crucible-wire` now needs a wildcard arm. This includes code that
  names it through `crucible-client-runtime-rust`, which re-exports it. Add
  `_ => ..`, after arms for `Conflict { .. }` (HTTP 409), `NotFound { .. }`
  (404) and `PermissionDenied { .. }` (403) if the code treats them
  differently. Variants added later are then not breaking. Code that only
  calls `name()`, `status()` or `message()` is
  unaffected. No shim is possible: the attribute is on the type. See
  [migrating](docs/migrating.md#unreleased).

- **An `AuthenticatorChain` covering older API versions needs their
  authenticators.** Each served API version's operations are now
  authenticated by that release's own schemes, so the chain needs an
  authenticator for every scheme an older release's operations declare.
  Without one, `Builder::build` returns an error naming the API version and
  scheme. Install the missing authenticator, or scope the chain away from
  that release. No shim is possible: the old behaviour authenticated an
  older release by the current one's schemes. See
  [migrating](docs/migrating.md#unreleased).

- **`OperationMetadata` is `#[non_exhaustive]` and has new fields.** Since
  0.7.0 it has gained `pagination`, `idempotency_token`,
  `idempotency_token_sent`, `replays_output`, `requirements`, `checks` and
  `public`. A struct literal no longer compiles outside the runtime, which
  builds the value at registration. Read it from `OperationJob::metadata`,
  `DecodedJob::metadata`, `Operations::metadata` or `Prepare::metadata`
  instead. A pattern that names every field needs `..`. Fields added later
  are then not breaking. Reading fields is unchanged. No shim is possible:
  the attribute is on the type. See
  [migrating](docs/migrating.md#unreleased).

- **`crucible_auth::Principal` is non-exhaustive.** A custom `Authenticator`
  can no longer build one with a struct literal. Replace
  `Principal { subject, issuer, client_id: None, token_kind, scopes, groups, claims }`
  with `Principal::new(subject, issuer, token_kind)` and the `with_*`
  setters for the fields it set (`.with_claims(claims)`, ...), and add `..`
  to any exhaustive destructuring. No shim is possible: the attribute is on
  the type. Reading fields is unchanged. See
  [migrating](docs/migrating.md#unreleased).

- **Rejections no longer name client text in a violation's path.** Since
  0.7.0, a strict structure's unknown member is reported at `[key]`, not at
  the member's name; a malformed value of a query parameter the operation
  does not declare at `query`, not at the parameter's name; a malformed
  entry of an `@httpQueryParams` or `@httpPrefixHeaders` map at
  `<member>/[key]`, not at its query key or header name;
  and a CBOR input missing a required member as a broken constraint
  (`fieldValidationFailed`, `is required` at the member's name) instead of
  an unreadable request (`cannotParse`), as JSON already was. An unsupported
  `Crucible-Version` is refused without repeating it. Code or tests that
  read those paths, messages or reasons, from the response's `fieldList`,
  an `on_rejection` hook or the service log, need the new values. No shim
  is possible: this is wire behaviour. See
  [migrating](docs/migrating.md#unreleased).

- **`Rejection::violations` is a method; the public field is removed.** A
  `Rejection { violations }` literal, or a push onto the field, listed any
  number of violations past `validate::MAX_VIOLATIONS`, and the response,
  service log and `fieldList` copied all of them. Read the list with
  `rejection.violations()` (a `&[Violation]`) or `into_violations()`, and
  build a rejection with `Rejection::new(iter)`, `Rejection::constraint`,
  `Rejection::malformed` or a `validate::Violations` collector. Generated
  server code reads it through the method after regeneration. No shim: a
  deprecated public field would keep the uncapped path open for another
  minor, so this removal is named here instead. See
  [migrating](docs/migrating.md#unreleased).

- **Only Crucible JSON remains; XML, RPC v2 and the AWS protocols are
  removed.** A Smithy model that declares `restJson1`, `awsJson1_0`,
  `awsJson1_1`, `restXml`, `awsQuery`, `ec2Query`, `rpcv2Cbor` or
  `rpcv2Json` now fails lowering with `protocol X is not supported`, as does
  one that uses `@xmlName`, `@xmlFlattened`, `@xmlAttribute`,
  `@xmlNamespace`, `@awsQueryCompatible`, `@awsQueryError`,
  `@ec2QueryName` or `@s3UnwrappedXmlOutput`; declare
  `@crucible.protocols#json` (or `protocol json` in `.cru`) instead and give
  each operation an `@http` binding. Generated clients and servers for the
  other protocols must be regenerated from a Crucible JSON model; there is
  no server that speaks both. Callers of `Model::lower_with_protocol`,
  `lower_infrastructure_with_protocol` or `lower_documentation(service,
  protocol)` drop the protocol argument, as do build scripts calling
  `.protocol(..)` or `.conformance()` and scripts passing `--protocol` or
  running `crucible conformance`; code that matched
  `Protocol::AwsRestJson1` and its siblings, read `Service::xml_*`,
  `protocols` or `protocol_tests`, or generated tests from `smithy.test`
  vectors has no replacement. Release snapshots move to format 2: a format 1
  snapshot of a Crucible JSON service still reads (the removed fields are
  dropped), one frozen with another protocol is refused with a message
  naming it. No shim is possible: the protocols, not an API, are removed.
  See [migrating](docs/migrating.md#unreleased).

- **Smithy-only features are removed.** A service built from a `.cru`
  package is unaffected unless it used one of the Rust runtime items below.
  Server code: `crucible_server_runtime_rust::{ByteStream, ChunkStream,
  EventStream, EventReceiver, EventStreamError}`, `middlewares::Cors`,
  `operation::{CorsInfo, Checksum, ChecksumAlgorithm, ResponseBindings}`,
  `Operation::{OPTIONAL_AUTH, UNSIGNED_PAYLOAD, CHECKSUM, INPUT_EVENTS,
  OUTPUT_EVENTS}` and its `*_stream`/`*_events`/`output_status` methods,
  `OperationMetadata::{optional_auth, unsigned_payload, input_events,
  output_events}`, `ModelledError::{response_status, write_http,
  write_http_typed}`, `Output::{events, stream, response_status,
  write_http}`, `Job::{take_body_stream, decode_events}`, `Location::
  {QueryParams, PrefixHeaders, ResponseCode, StreamingPayload, EventStream}`,
  `AuthScheme::{Basic, ApiKey, SigV4}` and the `wire_name` of
  `PaginationMember` are removed; `OperationMetadata::requires_authentication`
  is true whenever an authentication scheme is named. Rust client code:
  `Config::{region, endpoint_parameter, endpoint_builtin,
  disable_host_prefix, basic_credentials, api_key,
  aws_credentials, signing_name, request_checksum_calculation,
  response_checksum_validation, on_checksum_validated}`, `ChecksumPolicy`,
  `AwsCredentials`, `BasicCredentials`, `ServiceMetadata`,
  `OperationMetadata`, `Transport::send_with_metadata`, `decode_modelled_at`
  (use `decode_modelled`), the waiter and streaming types and
  `Error::ChecksumMismatch` are removed. Generator code: the removed
  `Feature` variants, IR fields and types listed under Removed;
  `Options::content_sha256` of every SDK generator, the `crucible_build`
  `content_sha256(..)` builders and the matching CLI options. Regenerate bindings and SDKs. The Smithy front end
  refuses the traits that used the removed features, so a Smithy model that
  used one has no replacement (Smithy models are no longer read at all, see below). No shim is possible: the features, not an
  API, are removed. See [migrating](docs/migrating.md#unreleased).

- **Smithy is no longer an input.** A model that is still Smithy converts once
  with the `smithy-to-cru` tool of the last release that carried it (0.7.x);
  review the `.cru` package it writes, then point `crucible_build::model(..)`
  and the CLI at the package directory. Drop `smithy-build.json`, the Gradle
  build, the `crucible-traits` dependency and any `--smithy-version` pin.
  `@crucible.auth#scopes` (and `ScopeEnforcer`'s model-declared scopes),
  `rename`, timestamp defaults, enum `mediaType` and 2xx error statuses have
  no `.cru` form and must be removed from the model. No shim is possible: the
  input, not an API, is removed. See [migrating](docs/migrating.md#unreleased).

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
