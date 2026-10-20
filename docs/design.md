# Design

## Why not openapi-generator

Traditional pipelines often went from an IDL → OpenAPI 3.0.x → openapi-generator. The
OpenAPI step is lossy, and each loss was frequently patched after generation, by string
search in the generated source:

| Lost in OpenAPI | Patch it needed |
|---|---|
| `@idempotencyToken` | a hand-kept list of operations to inject a token into |
| Integer vs Long | every bare integer widened to int64 |
| unions | `deny_unknown_fields` added to shapes by name |
| one error shape per status | manual checks or forks refusing two errors on one status |
| `@paginated` | missing or lossy paginators |
| custom traits | side generators reading internal ASTs directly |
| nested constraints, pattern escaping | complex template forks |

On top of that, generated Rust server code was often one monolithic trait with every operation
on it (thousands of lines in one file), and generated clients needed ad-hoc
splicing for package metadata.

Crucible reads the model itself, so none of those are lost.

## Why not other generator frameworks

Other generator frameworks often bring their own external routers and heavyweight
runtimes, and customizing them means deep plugins against internal foreign-language
APIs. That moves the patching into someone else's framework. Crucible owns a unified
protocol and a shared semantic IR, with native emitters and lightweight runtimes for
the [client language portfolio](clients.md).

## Protocol

crucible generates both ends, so it defines its own protocol,
`crucible.protocols#json`, and it is the only wire protocol. It uses HTTP bindings, JSON bodies and
status codes, with these conventions:

- Errors are identified by `X-Crucible-Error` (the shape name), with the
  legacy body `"__type"` as a fallback. Ordinary JSON errors retain `__type`.
  An error with an explicit payload requires the header, because its body
  belongs to the payload. Services exposing these errors to browsers must
  include `X-Crucible-Error` in `Access-Control-Expose-Headers`.
- Timestamps default to RFC 3339 `date-time` everywhere, not epoch seconds
  in bodies and HTTP dates in headers.
- The request id travels in `x-request-id`.

Crucible validates HTTP bindings itself, and [emits OpenAPI from its IR](openapi.md). OpenAPI is a downstream export; SDKs still consume the IR directly.
[Standalone JSON Schema export](json-schema.md) shares the schema engine and
exports a selected shape with local definitions.

[Service API references](docs.md) consume an audience-projected IR description
directly, preserving pagination, resources, idempotency, and documentation notes.

## Pipeline

```
crucible.toml + .cru sources
  → crucible-lang: parse → resolve → check → lower → ir::Service
  → emitters: rust server · native clients · CDK constructs · Terraform provider
```

`crucible-lang` is the one front end: it validates the package, resolves
imports and templates, and lowers to the IR. Generators never read source.

## The IR

`ir::Service` holds the operations (sorted by name) and every shape they
reach, keyed by shape id. Lowering settles, once:

- **Nullability**: a member is non-null when it is required or has a
  non-null default. A member's optionality is one fact for client and
  server (decision 20 in [idl/decisions.md](idl/decisions.md)), so clients
  apply the same defaults the server does.
- **Authentication**: effective schemes inherit from the service, with operation
  `@auth` overrides resolved here. An empty override disables bearer credentials.
- **Constraints**: the target's `@length`/`@range`/`@pattern`/`@uniqueItems`
  with the member's own laid over them.
- **Visibility**: `@internal` becomes explicit `internal` flags on operations,
  shapes and members. Audience projection in `crucible-ir` consumes these
  flags and validates removed references once. Shared output selection invokes
  projection before target capability checks; servers keep the full IR.
- **Documentation notes**: `@deprecated` (message, since), `@since`, `@unstable`
  and `@externalDocs` (sorted name to URL) become one typed `notes`
  (`DocNotes`) on the service, resources, operations, shapes, members and enum
  values. A malformed trait is a diagnostic against its shape. Every native
  generator renders `@deprecated` in the target's own form (Rust `#[deprecated]`,
  TypeScript/Java `@deprecated`, Kotlin `@Deprecated`, C# `[Obsolete]`, Swift
  `@available`, Dart `@Deprecated`, Go `Deprecated:`, YARD/PHPDoc/Doxygen tags,
  OpenAPI `deprecated`), falling back to a doc paragraph; `@since`, `@unstable`
  and `@externalDocs` are documentation and appear where a target
  already emits doc notes (OpenAPI: `externalDocs`, `x-crucible-since`,
  `x-crucible-unstable`). Generated code that uses a deprecated item itself opts
  out of the warning.
- **Doc links**: each `[Name]` or `[Name.member]` in documentation resolves to
  the id it names in `Service::doc_links`, and one that names nothing is an
  error; `docs` keep the author's Markdown. Audience projection drops the
  links whose target it removes, and every renderer finds links with the same
  `crucible_ir::doc_links::links` and renders them in its own form (decision
  80 in [idl/decisions.md](idl/decisions.md)).
- **Sensitivity**: member and target annotations resolve through the IR; generated
  static metadata supports recursive logging redaction.
- **HTTP bindings**: label, query, header, body or payload for each input
  member, header, body or payload for each output or error member. A payload
  (`@httpPayload`) is the whole body, so its structure has no body members.
  The IR records its kind and media type. Labels
  are checked against the URI in both
  directions. Literal query parameters are preserved
  in both clients and participate in server route matching.
  Media-type strings in headers (including list elements) carry an explicit
  base64 encoding description. Each target must opt into these capabilities;
  unsupported payload or header encodings remain generation diagnostics.
- **Errors**: the operation's own, then the service's, each with fault,
  status and retryability.
- **Pagination**: the operation's `@paginated` merged over the service's.
- **Timestamps**: member format, else target format, else the protocol default
  for where the value travels.
- **Resources**: every resource the service reaches, with its identifiers
  (its ancestors' and its own, keyed by name), properties, children, the lifecycle slots its
  operations' roles fill, and which members of each attached operation's
  input and output carry its entries (decision 67 in
  [idl/decisions.md](idl/decisions.md)). A type only a resource names is
  not one of the shapes, so the SDKs and servers see the operation contract
  alone.

Raw `traits` maps stay on the service, operations, shapes and members, so a
project's own traits reach emitters intact.

`crucible-policy` consumes this same IR for configured build-time API conventions
and directional contract comparisons. `crucible-build` invokes static policy
before generation; `crucible diff` and the shared GitHub action compare two
lowered services. Policy does not re-read the AST or override subset validation.
See [API policy and compatibility](api-policy.md).

One lowering serves every generator. Infrastructure emitters consume a
selected projection validated by `crucible-codegen-infra`; deployment policy
remains separate from HTTP semantics. See
[infrastructure generation](infrastructure.md) for target boundaries. The
description builder consumes the audience-projected IR, including explicit
service errors, resources and declared protocols; the Markdown renderer
consumes the versioned description. See [API reference generation](docs.md).

## Targets and phases

The current portfolio and setup guides are in [client setup and conformance](clients.md).
The sequence below records the original rollout; use the language guides for current APIs.

Each phase runs beside the openapi-generator output until it matches the
consumer's tests, then replaces it.

1. **TypeScript client.** Built (`crucible generate ts`). Fetch-based, with
   a runtime copied into each package, so a client and its runtime can never
   be out of step and there is nothing else to publish. Idempotency tokens
   filled from the trait, paginators, an error class per error shape (named
   by `X-Crucible-Error` or the body's `__type`/`code`, else by status when
   one of the operation's errors has it), retries from `@readonly`/
   `@idempotent`/token/`@retryable`, and middleware (the body digest is
   built in, opt-in per package). Inputs type required members as required,
   so a missing one fails at compile time. Integration test suites verify
   behavior against live HTTP endpoints.
2. **Rust client.** reqwest-based, with the same features.
3. **Rust server.** Became the framework: see [framework.md](framework.md). A
   service depends on its model package, `crucible-build` generates a trait
   per operation in `build.rs`, and the `crucible` crate runs everything
   around them. Real-world services generate and compile cleanly against it. Its
   own traits reach operation-aware middleware through `OperationInfo::traits` and
   `OperationMiddlewareExt::when_trait`.

## Client API design

Name the public client after the service (`WeatherClient`, for example).
Use the target language's spelling conventions for function-based APIs, such as
C prefixes and R constructors. Shared runtime types can keep generic names.

Give modeled SDK exceptions a service-wide type named after the service
(`WeatherException`). Inheritance-based targets extend this type from the
shared `ServiceError` base; unknown and malformed error responses use it too.
Network, cancellation and other local failures keep their own types.
Swift uses a service protocol, Go a service interface, R a condition class,
and Rust a service enum with conversions from operation error enums. C retains
its service-prefixed error representation. These are public SDK types, not
structure inheritance or changes to protocol error codes.

The shared IR defines wire behavior, not one public API for every language.
Generate top-level model types in normal packages or modules. Ordinary request
construction, response access, and pagination should use the language's native
values, collections, and conventions: Kotlin builder lambdas, Java and Rust
builders, Go/C setters, or named arguments and object literals where appropriate.
Do not require callers to wrap every assigned value in a serialization type.

Omitted, explicit null, and present values still differ on the wire. Keep that
state internally where practical, and expose an advanced presence API when the
language cannot express the distinction through ordinary assignment. A nullable
convenience getter may collapse omitted and null; serialization and explicit
presence inspection must retain them. Document each language's construction
rules and verify normal calls from an independently compiled consumer, alongside
the shared wire fixtures. Purely recursive collection types may need nominal
types when the language cannot express recursive aliases.

## Wire rules to get right

For Crucible JSON, nonempty JSON request bodies use
`application/json`. Media type matching ignores case and accepts parameters
such as `charset=utf-8`; another JSON media type such as `application/hal+json`
does not replace the modeled type. Empty bodies may omit Content-Type.
Nonempty requests with missing, duplicate or incompatible Content-Type headers
receive HTTP 415. Payloads use their derived media type, unless
an explicit modeled Content-Type header overrides it; Crucible raw blob and
string payloads accept any media type. A default header from client
configuration never replaces a document's or a payload's media type.

Protocol details owned here, with round-trip tests from the first emitter:
timestamp formats by location, list encoding in query strings (repeated
keys) and headers (comma-separated, quoted when needed), percent-encoding of
labels, `null` versus absent, document values, blobs (base64 in a JSON body,
raw bytes as a payload), error type identification, and empty bodies for
Unit output.

The Rust server rejects invalid percent escapes and decoded URI bytes that are
not UTF-8. Literal-query routing retains valid constraints even when another
query component is malformed. Invalid modeled output headers become observable
serialization failures. Empty JSON input bodies, including bodies containing
only space, tab, CR, or LF, follow the same rule with and without HTTP bindings;
raw payload bytes remain unchanged. See [framework validation](framework.md#errors).

Blob payloads preserve raw bytes. String payloads use UTF-8 text; structures,
unions and documents use JSON. An empty
body is an absent payload. An explicitly null structure, union or document
payload is sent as the body
`null`. Readers take a `null`
payload as the member's explicit null, exactly as a JSON body member, never as
a structure built from null. A structure is only ever a JSON object, never an
array of positional values. Error
payloads require a discriminator outside the body: `X-Crucible-Error`.
Lowering support does not imply client or server support.
A blob in a label, query parameter or ordinary header remains a diagnostic.

