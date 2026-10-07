# Design

## Why not openapi-generator

Leyline and Pigeon both went Smithy → OpenAPI 3.0.2 → openapi-generator. The
OpenAPI step is lossy, and each loss was patched after generation, by string
search in the generated source:

| Lost in OpenAPI | Patch it needed |
|---|---|
| `@idempotencyToken` | a hand-kept list of 31 operations to inject a token into (the model has 34) |
| Integer vs Long | every bare integer widened to int64 |
| unions | `deny_unknown_fields` added to 13 shapes by name |
| one error shape per status | a check that refuses two errors on one status (#251) |
| `@paginated` | nothing: no paginators |
| custom traits | four side generators reading `model.json` directly |
| nested constraints, pattern escaping | a 1,073-line fork of the rust-axum models template |

On top of that, generated Rust server code was one trait with every operation
on it (Leyline's `api/mod.rs`, 2,100 lines), and generated clients needed
splicing for request signing digests and package metadata.

crucible reads the Smithy model itself, so none of those are lost.

## Why not smithy-rs / smithy-typescript

They are model-native and solve most of the above, but they bring their own
router and runtimes (`aws-smithy-*`, `@smithy/*`), and customizing them means
Kotlin/Java decorators against internal APIs. That just moves the patching into
someone else's framework. Crucible owns one protocol and a shared semantic
IR, with native emitters and runtimes for the [client language portfolio](clients.md).

## Protocol

crucible generates both ends, so it defines its own protocol,
`crucible.protocols#json` ([smithy/crucible.smithy](../smithy/crucible.smithy)),
instead of AWS's restJson1. The two share HTTP bindings, JSON bodies and
status codes. They differ where restJson1 is AWS-shaped:

- Errors are identified by `X-Crucible-Error` (the shape name), with the
  legacy body `"__type"` as a fallback. Ordinary JSON errors retain `__type`.
  An error with an explicit payload requires the header, because its body
  belongs to the payload. Services exposing these errors to browsers must
  include `X-Crucible-Error` in `Access-Control-Expose-Headers`.
- Timestamps default to RFC 3339 `date-time` everywhere, not epoch seconds
  in bodies and HTTP dates in headers.
- The request id travels in `x-request-id`.

Smithy's HTTP binding validators are part of core Smithy and still run. What
the switch gives up is Smithy's OpenAPI converter, which only knows AWS
protocols. Crucible [emits OpenAPI from its IR](openapi.md) for supported REST
JSON services. OpenAPI is a downstream export; SDKs still consume the IR directly.

## Pipeline

```
smithy build (projection with flattenAndRemoveMixins)
  → model.json (JSON AST)
  → crucible-smithy: parse → subset check → lower → ir::Service
  → emitters: rust server · native clients · CDK constructs · Terraform provider
```

Smithy stays the authority on the model: it validates, resolves `apply`, and
flattens mixins. crucible never reimplements a Smithy transform. It reads the
result.

## The IR

`ir::Service` holds the operations (sorted by name) and every shape they
reach, keyed by shape id. Lowering settles, once:

- **Nullability** (Smithy 2.0): a member is non-null on the server when it is
  `@required` or has a non-null `@default`. A client may also omit
  `@clientOptional` members and every member of an `@input` structure.
  Client defaults on those members are ignored; server defaults remain intact.
- **Authentication**: effective schemes inherit from the service, with operation
  `@auth` overrides resolved here. An empty override disables bearer credentials.
- **Constraints**: the target's `@length`/`@range`/`@pattern`/`@uniqueItems`
  with the member's own laid over them.
- **Sensitivity**: member and target annotations resolve through the IR; generated
  static metadata supports recursive logging redaction.
- **HTTP bindings**: label, query, header, body or payload for each input
  member, header, body or payload for each output or error member. A payload
  (`@httpPayload`) is the whole body, so its structure has no body members.
  The IR records its kind, media type and streaming requirements. Labels
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

Raw `traits` maps stay on the service, operations, shapes and members, so a
project's own traits reach emitters intact.

`Model::lower` preserves the operation contract for SDKs and servers without
resource-only shapes or provisioning diagnostics. Infrastructure generators
opt in through `Model::lower_infrastructure`, which also preserves the resource graph,
identifiers, properties, explicit
lifecycle slots, collection operations, and resolved resource member/reference
paths. Infrastructure emitters consume a selected projection validated by
`crucible-codegen-infra`; deployment policy remains separate from HTTP semantics.
See [infrastructure generation](infrastructure.md) for target boundaries.

## Targets and phases

The current portfolio and setup guides are in [client setup and conformance](clients.md).
The sequence below records the original rollout; use the language guides for current APIs.

Each phase runs beside the openapi-generator output until it matches the
consumer's tests, then replaces it.

1. **TypeScript client.** Built (`crucible generate ts`). Fetch-based, with
   a runtime copied into each package, so a client and its runtime can never
   be out of step and there is nothing else to publish. Idempotency tokens
   filled from the trait, paginators, an error class per error shape (named
   by `X-Amzn-Errortype` or the body's `__type`/`code`, else by status when
   one of the operation's errors has it), retries from `@readonly`/
   `@idempotent`/token/`@retryable`, and middleware (the body digest is
   built in, opt-in per package). Inputs type `@required` members as required,
   so a missing one fails at compile time. Smithy's client nullability would
   make every `@input` member optional. Leyline's integ suite is the oracle
   once it moves to the new client.
2. **Rust client.** reqwest-based, with the same features. The oracle is
   `sdk/rust/tests/wire.rs`.
3. **Rust server.** Became the framework: see [framework.md](framework.md). A
   service depends on its model package, `crucible-build` generates a trait
   per operation in `build.rs`, and the `crucible` crate runs everything
   around them. Leyline's full model generates and compiles against it. Its
   own traits reach operation-aware middleware through `OperationInfo::traits` and
   `OperationMiddlewareExt::when_trait`, replacing Leyline's side generators.

## Client API design

Name the public client after the Smithy service (`LeylineClient`, for example).
Use the target language's spelling conventions for function-based APIs, such as
C prefixes and R constructors. Shared runtime types can keep generic names.

Give modeled SDK exceptions a service-wide type named after the service
(`LeylineException`). Inheritance-based targets extend this type from the
shared `ServiceError` base; unknown and malformed error responses use it too.
Network, cancellation, checksum and other local failures keep their own types.
Swift uses a service protocol, Go a service interface, R a condition class,
and Rust a service enum with conversions from operation error enums. C retains
its service-prefixed error representation. These are public SDK types, not
Smithy structure inheritance or changes to protocol error codes.

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
unions and documents use the selected protocol's document encoding. Error
payloads require a discriminator outside the body: `X-Crucible-Error` for
Crucible or `X-Amzn-Errortype` for AWS restJson1. These are separate target
capabilities, so lowering support does not imply client or server support.
A blob in a label, query parameter or ordinary header remains a diagnostic.


The Rust server and client share native CBOR model codecs in `crucible-wire`.
Smithy RPC v2 uses `Smithy-Protocol` and service/operation paths instead of AWS
JSON target headers. RPC v2 honors modeled error status codes, uses full error
shape ids, and ignores HTTP member bindings and timestamp-format overrides.
Its JSON arbitrary-precision numbers are strings; CBOR uses native byte strings
and numeric tags. Streaming unions use the shared event framing and protocol
specific document codecs. Raw streaming blobs remain unsupported for RPC v2.
See [server protocols](framework.md#protocols) and the
[Rust client guide](clients/rust.md#smithy-rpc-v2).
