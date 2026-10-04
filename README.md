# crucible

Generates servers and clients straight from a Smithy model: an axum server and
a Rust client, and a TypeScript client. There is no OpenAPI step in between,
so the parts of the model OpenAPI drops survive: unions, idempotency tokens,
pagination, every error shape, and a project's own traits.

Used by [Leyline](https://github.com/Operation-Leyline/leyline) and Pigeon.

## Use

Copy [smithy/crucible.smithy](smithy/crucible.smithy) into the model's source
directory and put `@crucible.protocols#json` on the service. It is crucible's
own HTTP+JSON protocol, specified in that file. A model may carry another
protocol beside it for other tools; crucible generates only its own.

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
generate from, all of them in one run.

Generate a TypeScript client package (no dependencies; ESM; `fetch`-based):

```bash
cargo run -p crucible-cli -- generate ts model.json --out generated/client-ts \
  --package-name @acme/client --endpoint https://api.acme.dev --middleware content-sha256
```

```ts
const client = new AcmeClient({ token: () => session.token() });
const { graph } = await client.createGraph({ organizationId, name: "movies" });
for await (const page of client.paginateListGraphs({ organizationId })) { /* … */ }
try { await client.deleteGraph({ organizationId, graphId }); }
catch (error) { if (error instanceof ConflictException) console.log(error.reason); }
```

The client fills idempotency tokens (and writes them back into the input, so
resending the same object replays rather than repeats), retries throttling
always and other transient failures only when the operation is safe to repeat,
decodes each modelled error into its own class, converts timestamps, applies
`@default`s, and runs caller middleware on every attempt. `--package-json`
merges a JSON file over the generated `package.json` for publishing settings.
The output directory belongs to crucible: it is emptied on every run, except
`node_modules` and `dist`.

Generate an axum server crate:

```bash
cargo run -p crucible-cli -- generate rust-server model.json --out generated/server --crate-name acme-api
```

It has a trait per resource (`CityApi`, plus `<Service>Api` for operations
the service binds directly) whose methods take a typed, already-validated
input and return the output or the operation's error enum. `router(service)`
is an `axum::Router` for all of them. A request that breaks the model (bad
JSON, a missing or malformed member, a length, range or pattern broken)
never reaches a handler: it becomes the service's validation error through
the `Reject` trait. crucible fills it when it can (`message`, and a
`fieldList` of `path`/`name` + `message`); otherwise the service implements
`reject`. Generation fails, rather than the server at startup, on a
`@pattern` the regex crate cannot run or routes axum would refuse.

## Supported Smithy

`crucible.protocols#json` with bearer auth. Labels, query parameters and headers carry simple
values, enums, or lists of those; everything else goes in a JSON body. Blobs,
streaming, `@httpPayload`, `@httpResponseCode`, greedy labels, big numbers,
`intEnum` and `@jsonName` are refused with a diagnostic, never skipped.
The subset grows when a model needs more. See [docs/design.md](docs/design.md).
