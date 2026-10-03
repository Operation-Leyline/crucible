# crucible

Generates servers and clients straight from a Smithy model: an axum server and
a Rust client, and a TypeScript client. There is no OpenAPI step in between,
so the parts of the model OpenAPI drops survive: unions, idempotency tokens,
pagination, every error shape, and a project's own traits.

Used by [Leyline](https://github.com/Operation-Leyline/leyline) and Pigeon.

## Use

Add a projection that flattens mixins to the project's `smithy-build.json`:

```json
"projections": {
  "crucible": { "transforms": [{ "name": "flattenAndRemoveMixins" }] }
}
```

Build the model, then point crucible at the projection's JSON AST:

```bash
cargo run -p crucible-cli -- check smithy/build/smithyprojections/smithy/crucible/model/model.json
```

`check` lowers the service and lists every construct crucible cannot
generate from, all of them in one run. `generate` arrives with the first emitter.

## Supported Smithy

restJson1 with bearer auth. Labels, query parameters and headers carry simple
values, enums, or lists of those; everything else goes in a JSON body. Blobs,
streaming, `@httpPayload`, `@httpResponseCode`, greedy labels, big numbers,
`intEnum` and other protocols are refused with a diagnostic, never skipped.
The subset grows when a model needs more. See [docs/design.md](docs/design.md).
