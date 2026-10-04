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

- Errors are named only by the body's `"__type"` (the shape name), not an
  `X-Amzn-Errortype` header. A browser cannot read a custom response header
  without CORS changes, but it can always read the body.
- Timestamps default to RFC 3339 `date-time` everywhere, not epoch seconds
  in bodies and HTTP dates in headers.
- The request id travels in `x-request-id`.

Smithy's HTTP binding validators are part of core Smithy and still run. What
the switch gives up is Smithy's OpenAPI converter, which only knows AWS
protocols. crucible can emit OpenAPI from its IR when one is needed.

## Pipeline

```
smithy build (projection with flattenAndRemoveMixins)
  → model.json (JSON AST)
  → crucible-smithy: parse → subset check → lower → ir::Service
  → emitters: rust server · rust client · typescript client
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
- **Constraints**: the target's `@length`/`@range`/`@pattern`/`@uniqueItems`
  with the member's own laid over them.
- **Sensitivity**: member and target annotations resolve through the IR; generated
  static metadata supports recursive logging redaction.
- **HTTP bindings**: label, query, header, body or payload for each input
  member, header, body or payload for each output member, header or body
  for each error member. A payload (`@httpPayload`) must target a blob and
  is the whole body, so its structure has no body members; its media type
  is settled here too. Labels are checked against the URI in both
  directions. Literal query parameters are preserved
  in both clients and participate in server route matching.
- **Errors**: the operation's own, then the service's, each with fault,
  status and retryability.
- **Pagination**: the operation's `@paginated` merged over the service's.
- **Timestamps**: member format, else target format, else the protocol default
  for where the value travels.

Raw `traits` maps stay on the service, operations, shapes and members, so a
project's own traits reach emitters intact.

## Targets and phases

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
   own traits reach handlers through `OperationInfo::traits` and
   `HandlerExt::when_trait`, replacing Leyline's side generators.

## Wire rules to get right

Protocol details owned here, with round-trip tests from the first emitter:
timestamp formats by location, list encoding in query strings (repeated
keys) and headers (comma-separated, quoted when needed), percent-encoding of
labels, `null` versus absent, document values, blobs (base64 in a JSON body,
raw bytes as a payload), error type identification, and empty bodies for
Unit output.

A payload is supported only for a blob: a signature over a request (a
payment processor's webhook) needs the exact bytes, which a JSON body cannot
promise. A structure, union, string or document payload, and a payload on an
error (whose body must be JSON naming it), are diagnosed. So is a blob in a
label, query parameter or header.
