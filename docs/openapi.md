# OpenAPI generation

Crucible exports an OpenAPI 3.1.1 JSON document from the same lowered IR
used by its server and client generators. The `.cru` package remains the source of truth.

For a human-readable reference with resources, pagination, idempotency, and RPC
JSON protocol support, use [service API reference generation](docs.md).

## Generating a document

Point the command at a `.cru` package directory:

```sh
cargo run -p crucible-cli -- generate openapi model --out openapi.json --endpoint https://api.example.com
```

For the checked-in Weather package:

```sh
cargo run -p crucible-cli -- generate openapi tests/models/weather --out weather.openapi.json --endpoint http://localhost:8080
```

`--out` names a file, which is overwritten after successful generation. Its
parent directory must exist. Omit `--endpoint` to use relative API paths.
Include a deployment path prefix in the endpoint when needed. Use
`--service example.weather#Weather` when the model defines several services.

## Supported contract

The emitter supports Crucible JSON. It exports:

- HTTP methods, paths, literal query parameters, labels, query parameters,
  request and response headers, and successful response status codes.
- JSON body projections, raw blob payloads, UTF-8 string payloads, and
  structure, union, and document payloads with their resolved media types.
- Recursive schemas, JSON property names, enums, integer enums, integer widths,
  arbitrary-precision numbers, unions with one arm, maps, unique lists,
  effective constraints, defaults, and resolved timestamp representations.
- Modeled errors grouped by HTTP status. Multiple errors on one status retain
  their alternative bodies, headers, and wire discriminators. Success and error
  responses sharing a status also retain both alternatives.
- Bearer authentication, with public operations marked as unauthenticated.

Schema component keys use `namespace.ShapeName`, avoiding collisions across
namespaces and service renames. Operation IDs use the service's resolved names.
References preserve recursion. Operations and shapes are emitted deterministically;
member order follows the model.

## Extension metadata

Standard OpenAPI fields describe the representable wire contract. Extensions
retain semantics that OpenAPI does not express directly (the `x-smithy-` prefix
is the extension namespace, kept for existing consumers):

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

These extensions document behavior; generic OpenAPI tools need their own support
to implement automatic tokens, pagination, and byte-length checks.
The export does not create a client, host a documentation page, or add runtime routes.

## Examples

`@examples` lowers into `Operation.examples` and is validated against the
operation's input, output and error shapes (members, types, enums, unions,
length, range, pattern, uniqueness; `allowConstraintErrors` waives the
constraint checks only). A malformed example is a diagnostic on the operation.
The exporter places each value where its member travels, as native OpenAPI
`examples` maps keyed by title (repeated titles get ` (2)`, ` (3)`): on
parameters and response headers for bound members, on the request and response
bodies, and on the error response whose shape the example names. Bodies use JSON
names and each member's timestamp format. Members whose bytes are re-encoded
(media-type headers) and raw blob payloads have no faithful rendering and are
omitted. The raw examples annotation is not repeated in `x-smithy-traits`.

## Diagnostics and limits

Unsupported capabilities produce diagnostics against their shapes.
The initial exporter rejects greedy labels.

OpenAPI permits one operation per method and path. Routes that share that pair
but differ by literal queries are rejected instead of overwriting each other.
Differently named path templates with the same hierarchy, unsupported HTTP
methods, duplicate parameters or headers, modeled `Accept`, `Content-Type`, or
`Authorization` request headers, and modeled `Content-Type` response headers
are also rejected. OpenAPI ignores these header definitions; model content types
or authentication through their normal
model constructs instead.

Unsupported models leave the destination untouched. No Swagger UI assets are
generated. Broader protocol support belongs in this emitter and its capability
tests; it must not reinterpret model source.
