# OpenAPI generation

Crucible exports an OpenAPI 3.1.1 JSON document from the same lowered Smithy IR
used by its server and client generators. Smithy remains the source of truth.

## Generating a document

Build the Smithy JSON AST with `flattenAndRemoveMixins`, then run:

```sh
cargo run -p crucible-cli -- generate openapi model.json --out openapi.json --endpoint https://api.example.com
```

For the checked-in Weather fixture:

```sh
cargo run -p crucible-cli -- generate openapi crates/crucible-smithy/tests/fixtures/weather.json --out weather.openapi.json --endpoint http://localhost:8080
```

`--out` names a file, which is overwritten after successful generation. Its
parent directory must exist. Omit `--endpoint` to use relative API paths.
Include a deployment path prefix in the endpoint when needed. Use
`--service example.weather#Weather` when the model defines several services, and
`--protocol aws.protocols#restJson1` to select a declared protocol explicitly.

## Supported contract

The emitter supports Crucible JSON and AWS REST JSON. It exports:

- HTTP methods, paths, literal query parameters, labels, query parameters,
  request and response headers, and successful response status codes.
- JSON body projections, raw blob payloads, UTF-8 string payloads, and
  structure, union, and document payloads with their resolved media types.
- Recursive schemas, JSON property names, enums, integer enums, integer widths,
  arbitrary-precision numbers, unions with one arm, maps, sparse collections,
  sets, effective constraints, defaults, and resolved timestamp representations.
- Modeled errors grouped by HTTP status. Multiple errors on one status retain
  their alternative bodies, headers, and wire discriminators. Success and error
  responses sharing a status also retain both alternatives.
- Bearer, basic, digest, and API key authentication, including effective
  operation overrides and optional authentication.

Schema component keys use `namespace.ShapeName`, avoiding collisions across
namespaces and service renames. Operation IDs use the service's resolved names.
References preserve recursion. Operations and shapes are emitted deterministically;
member order follows the model.

## Smithy metadata

Standard OpenAPI fields describe the representable wire contract. Extensions
retain semantics that OpenAPI does not express directly:

| Extension | Meaning |
|---|---|
| `x-smithy-service`, `x-smithy-shape`, `x-smithy-protocol` | Original service, operation, and protocol IDs |
| `x-smithy-traits` | Original annotations, including custom traits |
| `x-smithy-pagination` | Resolved pagination member paths |
| `x-smithy-idempotency-token`, `x-smithy-idempotent`, `x-smithy-readonly` | Retry and token metadata |
| `x-smithy-default` | Modeled default, including defaults that clients may omit |
| `x-smithy-timestamp-format` | HTTP date or epoch-second representation |
| `x-smithy-length` | Blob length in decoded bytes; base64 character length differs |
| `x-smithy-decoded-schema` | Constraints before media-type header base64 encoding |
| `x-smithy-errors`, `x-smithy-response-variants`, `x-smithy-retryable` | Modeled errors and per-response metadata |
| `x-smithy-api-key-scheme` | Prefix required before an API key |

These extensions document behavior; generic OpenAPI tools need their own support
to implement automatic tokens, pagination, byte-length checks, and API key prefixes.
The export does not create a client, host a documentation page, or add runtime routes.

## Diagnostics and limits

Unsupported capabilities produce diagnostics against their Smithy shapes.
The initial exporter rejects XML, query, AWS JSON, and Smithy RPC protocols;
event streams and streaming blobs; AWS signing; endpoint rules, discovery and
host prefixes; waiters; checksum traits; greedy labels; arbitrary query maps;
prefix headers; and dynamic response status bindings.

OpenAPI permits one operation per method and path. Routes that share that pair
but differ by literal queries are rejected instead of overwriting each other.
Differently named path templates with the same hierarchy, unsupported HTTP
methods, duplicate parameters or headers, modeled `Accept`, `Content-Type`, or
`Authorization` request headers, and modeled `Content-Type` response headers
are also rejected. OpenAPI ignores these header definitions; model content types
or authentication through their normal
Smithy semantics instead.

Unsupported models leave the destination untouched. No Swagger UI assets are
generated. Broader protocol support belongs in this emitter and its capability
tests; it must not reinterpret the Smithy AST.
