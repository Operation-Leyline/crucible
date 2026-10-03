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
someone else's framework. A generator for one protocol and three targets is
small enough to own.

## Pipeline

```
smithy build (projection with flattenAndRemoveMixins)
  → model.json (JSON AST)
  → crucible-core: parse → subset check → lower → ir::Service
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
- **HTTP bindings**: label, query, header or body for each input member,
  header or body for each output and error member. Labels are checked
  against the URI in both directions.
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

1. **TypeScript client.** Fetch-based. Idempotency tokens filled from the
   trait, paginators, typed errors decided by the error shape (the
   `X-Amzn-Errortype` header or `__type`), retries from `@readonly`/
   `@idempotent`/`@retryable`, and a middleware hook for the request signing
   digest. The oracle is Leyline's integ suite.
2. **Rust client.** reqwest-based, with the same features. The oracle is
   `sdk/rust/tests/wire.rs`.
3. **Rust server.** axum. One handler trait per resource, typed input
   structs validated before the handler runs, and typed output and error
   enums. Custom traits feed named extension points (Leyline's audit capture
   and request classes) instead of side generators. The oracle is Leyline's
   service tests plus integ.

## Wire rules to get right

restJson1 details owned here, with round-trip tests from the first emitter:
timestamp formats by location, list encoding in query strings (repeated
keys) and headers (comma-separated, quoted when needed), percent-encoding of
labels, `null` versus absent, document values, error type identification,
and empty bodies for Unit output.
