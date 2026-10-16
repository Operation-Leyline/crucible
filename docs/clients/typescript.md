# TypeScript and JavaScript clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates a dependency-free ESM client for Node 22 and browsers that
provide `fetch`, `Request`, Web Crypto, and `structuredClone`. Models are
TypeScript interfaces and aliases; JavaScript callers use the same built package.
The package declares TypeScript `>=5.7 <7` as a build dependency.

## API audience

Clients omit `@internal` API by default. Select `--audience internal` for a full
client, or `.audience(crucible_build::Audience::Internal)` on the Cargo build
helper. Servers always retain the complete contract.
See [API audience selection](../build-outputs.md#selecting-an-api-audience).

## Generating and installing

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate ts model \
  --out target/weather-ts --package-name @example/weather \
  --endpoint http://127.0.0.1:3000 --middleware content-sha256
cd target/weather-ts
npm install
npm run build
npm pack
```

The output contains models, error classes, the client, schemas, a bundled
runtime, and npm/TypeScript configuration. Generation tracks its files in
`.crucible`, updates generated files, and removes obsolete generated files.
Handwritten files, `node_modules`, and `dist` survive regeneration. Keep the
ownership manifest. Set `--package-version` for distribution, `--client-name` to override
`WeatherClient`, or `--package-json` to merge additional package metadata.
Install the built package directory or archive in your application; generation
and `npm pack` do not publish it.

Use `--service` when the sources define multiple services. Package metadata
records the generator and service versions. The package carries
`crucible.json` (schema 2: generator version and revision, service,
`schemaSha256` of the shared lowered wire schema, and a `contentDigest` of
the generated files) and `crucible-model.json`, the model its audience
sees; `files` lists both. See [package provenance](../sdk-releases.md#package-provenance).

The common package metadata keys are flags: `--description`,
`--repository`, `--homepage`, `--license` or `--license-file <path>`,
`--author` (repeat it; the first is `author`, the rest `contributors`),
`--readme <path>`, `--keyword` (repeat it) and `--private`
(`"private": true`, unless `--npm-registry` names where it publishes).
`--npm-registry <url>` and `--npm-access public|restricted` set
`publishConfig`. `--documentation` is accepted, as for every language, but
package.json has no documentation field (`homepage` is the project's home
page), so a TypeScript package does not record it. A README and license are copied into the package.
`--package-json` is merged last and cannot set `name`, `version`,
`crucible`, `files`, `private` or `publishConfig`. To declare the SDK
once, with its npm registry, use [`crucible-sdk.toml`](../sdk-releases.md#declare-sdks-crucible-sdktoml).

## Calling operations

With the generated package installed and a weather service running:

```typescript
import { WeatherClient, ServiceError } from "@example/weather";

const client = new WeatherClient({
  endpoint: "http://127.0.0.1:3000",
  token: () => process.env.WEATHER_TOKEN,
});

try {
  const result = await client.getCity(
    { cityId: "oslo" },
    { signal: AbortSignal.timeout(30_000) },
  );
  console.log(result.city.name);
} catch (error) {
  if (error instanceof ServiceError) {
    console.error(error.name, error.$metadata.status, error.$metadata.requestId);
  }
  throw error;
}

for await (const page of client.paginateListCities({ maxResults: 50 })) {
  console.log(page.cities);
}
```

The environment-token example is for Node; a TypeScript application needs
Node type declarations for `process`. Browser applications supply their
own token provider. A token provider can return a string, `undefined`, or
a promise; it runs on every attempt. A fixed string is also accepted.

Generated operation methods return promises. Operations without required input
members accept an omitted input. Paginators return async generators and request
one page at a time. They copy the initial input, stop on an absent or empty token,
and reject a token repeated immediately; they do not track all past tokens.

## Models and errors

Strings, booleans, arrays, and objects use JavaScript values. Blobs are
`Uint8Array`: standard padded base64 in JSON and raw bytes for `@httpPayload`.
Optional fields use `undefined` for omission. The runtime preserves explicit
JSON nulls where present, but generated optional properties are not a general
three-state presence API. Missing modeled defaults are applied during decoding.
The decoder rejects absent or null nonoptional members (except a required
`document` may hold JSON null), fractional integers, and integers outside their
modeled width. Client optionality comes from the lowered IR and matches the
service's; TypeScript input interfaces require required input properties at
compile time.

`long` uses native `bigint`, covering the complete signed 64-bit range.
The client preserves numeric JSON tokens until typed decoding, so a valid
`9223372036854775807` is accepted exactly while `9223372036854775808` is rejected.
The wire still uses ordinary, unquoted JSON numbers. Byte, short, and integer
use `number` with signed 8-, 16-, and 32-bit validation; float and double use
finite `number` values.

Document numbers use JavaScript `number`; values beyond its safe integer range
can round. Nonfinite numbers are rejected. Modeled `long` members and the raw
payload of an unknown union arm preserve their numeric values.

### Migrating long values to bigint

Regenerate the package, rebuild it, and fix TypeScript errors at call sites.
Replace long literals such as `expectedRevision: 7` with `expectedRevision: 7n`;
use `BigInt("9223372036854775807")` when reading decimal strings. Do not convert
an already rounded JavaScript number to bigint. Compare and calculate long
results with bigint operands, for example `result.city.population === 0n`.
Converting back with `Number(value)` can lose precision.

Generated calls encode bigints automatically. `JSON.stringify` cannot serialize
a bigint. For application-owned JSON containing long values, the package exports
`stringifyJson`, which emits bigint values as unquoted numbers and rejects
nonfinite numbers and non-JSON objects. Convert `Date` and `Uint8Array` values to
the required wire representation first; this helper does not replace the
client's schema conversion. Middleware reading `request.text()` sees the exact
encoded body, suitable for computing a digest without reparsing it.


Timestamps use `Date`, with millisecond precision; submillisecond fractions are
lost. HTTP-date has second precision. Known union arms use native object types.
A future arm has the exported `UnknownUnion` form:

```typescript
const location: Location = {
  $unknown: { name: "futureCoordinate", json: '{"precise":1790812800.123456789}' },
};
```

`json` is the arm's payload, not its enclosing union object. Decoding preserves
unknown nested structure and numeric values without converting them to `number`;
encoding restores the original arm name and JSON value. Whitespace need not be
preserved. Null payloads use `json: "null"`. Malformed JSON and unknown arms that
use a declared member name are rejected. When upgrading, replace reads of
undeclared union properties with `$unknown.name` and `$unknown.json`. Known arms
remain unchanged and unions still require exactly one member.

Each modeled exception is a `ServiceError` subclass with its modeled fields,
`$metadata` (status, request ID, headers), `$fault`, and optional `$retryable`
metadata. Error identity comes from body `__type`, or an unambiguous declared
status when the name is absent. Unknown names stay generic `ServiceError`s.
Transport failures use `NetworkError`. Invalid input construction and malformed
successful JSON fail without retrying.

Shapes may share a name with a JavaScript global such as `Record`, `Partial`,
`Promise`, `Map`, `Date`, `Uint8Array`, `Error`, or `Object`: generated code
reaches every global through `globalThis`, so the model's type never shadows it.
Your own modules that import such a model type should do the same
(`globalThis.Uint8Array`) or import the model under another name. `globalThis`,
the generated namespaces `m`, `e`, and `spec`, `errors`, JavaScript reserved
words, and TypeScript's predefined type names are diagnostics instead.

## Transport, middleware, and retries

`ClientConfig.fetch` accepts a custom function from `Request` to
`Promise<Response>`. The default uses global `fetch`. Every request the client
dispatches, including one a middleware rebuilt, carries `redirect: "manual"`,
so a redirect is never followed with the request's credential headers: Node
returns the 3xx response and a browser an opaque redirect (status 0), and
either becomes a `ServiceError`. A custom `fetch` may choose another redirect
mode for the requests it sends. The client creates no separate closable
connection pool. Default headers come from `ClientConfig.headers`; per-call
headers and `AbortSignal` come from `RequestOptions`. Neither replaces the
`Content-Type` of a JSON body or a payload's media type; only a modeled
`Content-Type` member overrides a payload's.

Retries follow the [shared contract](../clients.md#http-retry-contract).
`retry: false` disables them. `maxAttempts` includes the first attempt; defaults
are three attempts, 100ms base delay, and 20s maximum backoff, with full jitter.
Numeric or HTTP-date `Retry-After` takes precedence. `ClientConfig.timeoutMs`
sets a total-call budget across credentials, middleware, fetch, body reads, and
retry sleeps; expiry throws `TimeoutError`. `RequestOptions.timeoutMs` overrides
that budget, and `null` disables it for one call. Zero expires before dispatch.
There is no total-call timeout by default. A supplied abort signal covers the
same stages and rejects with its reason. Timers and signal listeners are removed
when the call completes. Late credentials or middleware cannot dispatch another
request after cancellation.

A missing idempotency token is generated once. If the input object is extensible,
the client writes the token back into it; sending that same object again then
replays the logical request. Frozen inputs are copied, so supply an explicit
token for replays involving them.

Middleware is `(request, next) => Promise<Response>` and runs on every attempt.
The order is bearer authentication, generated middleware, then caller
middleware. Public operations bypass the bearer-token provider.
`--middleware content-sha256` adds a SHA-256 body digest. Empty `POST`, `PUT`, and
`PATCH` requests use the digest of zero bytes. It skips other bodyless requests
and requests with an existing digest header.

If middleware changes a request body, compute its digest after that change.
Generate without the digest option and place exported `contentSha256` last in
your `middleware` array; remove any previous digest header before recomputing.
Do not combine an earlier generated digest with a later body mutation.

## Qualification

Run `bash tools/test-ts.sh` for generated type checks, fake-fetch runtime tests,
all 16 shared serialization cases, and shared retry cases. Set `TSC` to use an
existing TypeScript compiler and `CRUCIBLE_BIN` to generate with a prebuilt CLI
instead of `cargo run`. `bash tools/test-e2e.sh` also runs the TypeScript client
against the real weather service. Browser APIs are used by the runtime, but Node
tests alone do not prove a particular browser application's CORS, authentication,
or deployment setup.

See [qualification levels](../clients.md#qualification) for the distinction
between local generation, package consumption, and authenticated Leyline proof.

### HTTP bindings

Greedy path labels preserve slash separators and reject dot-only segments before
dispatch. Nested pagination token paths use the public model names.

### Exact numbers and collections

`bigInteger` uses `bigint`. `bigDecimal` uses the exported immutable `Decimal`:
construct it from decimal text or `bigint`, inspect it with `toString()`, and
compare numeric values with `equals()`. The codecs write JSON numeric tokens;
values, defaults, HTTP headers, and retry snapshots never pass through `number`.
Integer enums expose named numeric constants and accept future signed 32-bit
values when reading responses.

Sparse lists and maps include `null` in their element or value type. `@uniqueItems` lists remain arrays; encoding and decoding reject duplicate
values, comparing objects structurally and numbers mathematically. Dense collections reject null elements, including document elements; null nested inside a document is preserved.

### Authentication

`token` accepts a string or a sync/async provider and is sent as a bearer
token, resolved again on each attempt. An `Authorization: Bearer` header the
caller already set (through `RequestOptions.headers` or `ClientConfig.headers`)
takes precedence over the provider. A public operation (`@auth([])`)
invokes no provider. Without a token, the service decides whether an anonymous
call is allowed. Token resolution shares the call's timeout and cancellation
signal.
