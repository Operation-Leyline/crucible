# TypeScript and JavaScript clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates a dependency-free ESM client for Node 22 and browsers that
provide `fetch`, `Request`, Web Crypto, and `structuredClone`. Models are
TypeScript interfaces and aliases; JavaScript callers use the same built package.
The package declares TypeScript `>=5.7 <7` as a build dependency.

## Generating and installing

From this repository:

```sh
cargo run -p crucible-cli -- generate ts \
  crates/crucible-smithy/tests/fixtures/weather.json \
  --out target/weather-ts --package-name @example/weather \
  --endpoint http://127.0.0.1:3000 --middleware content-sha256
cd target/weather-ts
npm install
npm run build
npm pack
```

The output contains models, error classes, the client, schemas, a bundled
runtime, and npm/TypeScript configuration. Generation owns and replaces that
directory. Set `--package-version` for distribution, `--client-name` to override
`WeatherClient`, or `--package-json` to merge additional package metadata.
Install the built package directory or archive in your application; generation
and `npm pack` do not publish it.

For another service, replace the fixture with its prepared Smithy JSON model.
Use `--service` when that model defines multiple services. Package metadata
records the generator and service versions; TypeScript does not currently emit
the separate schema-digest manifest used by newer targets.

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
own credential provider and need the service's CORS policy to permit the
request headers they use. A token provider can return a string, `undefined`, or
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
modeled width. Client optionality comes from the lowered Smithy IR, including
`@clientOptional` and `@input`; TypeScript input interfaces continue to require
`@required` input properties at compile time.

Smithy `long` uses native `bigint`, covering the complete signed 64-bit range.
The client preserves numeric JSON tokens until typed decoding, so a valid
`9223372036854775807` is accepted exactly while `9223372036854775808` is rejected.
The wire still uses ordinary, unquoted JSON numbers. Byte, short, and integer
use `number` with signed 8-, 16-, and 32-bit validation; float and double use
finite `number` values.

Document numbers and numbers inside unknown union arms still use JavaScript
`number`; values beyond its safe integer range can round. Nonfinite numbers
are rejected. This limitation does not apply to modeled `long` members.

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
lost. HTTP-date has second precision. Enums and unions describe the known model
at compile time. The runtime retains unrecognized JSON properties and strings,
but it does not expose a dedicated typed unknown-union alternative. Unions must
contain exactly one member; known members are decoded using their generated
schemas. Unknown arms retain their JSON value.

Each modeled exception is a `ServiceError` subclass with its modeled fields,
`$metadata` (status, request ID, headers), `$fault`, and optional `$retryable`
metadata. Error identity comes from body `__type`, or an unambiguous declared
status when the name is absent. Unknown names stay generic `ServiceError`s.
Transport failures use `NetworkError`. Invalid input construction and malformed
successful JSON fail without retrying.

## Transport, middleware, and retries

`ClientConfig.fetch` accepts a custom function from `Request` to
`Promise<Response>`. The default uses global `fetch`, including its redirect
behavior. The client creates no separate closable connection pool. Default
headers come from `ClientConfig.headers`; per-call headers and `AbortSignal`
come from `RequestOptions`.

Retries follow the [shared contract](../clients.md#http-retry-contract).
`retry: false` disables them. `maxAttempts` includes the first attempt; defaults
are three attempts, 100ms base delay, and 20s maximum backoff, with full jitter.
Numeric or HTTP-date `Retry-After` takes precedence. An abort signal covers
fetch and retry sleeps; supply `AbortSignal.timeout(...)` when you need an
overall deadline. There is no default total-call timeout.

A missing idempotency token is generated once. If the input object is extensible,
the client writes the token back into it; sending that same object again then
replays the logical request. Frozen inputs are copied, so supply an explicit
token for replays involving them.

Middleware is `(request, next) => Promise<Response>` and runs on every attempt.
The order is bearer authentication, generated middleware, then caller
middleware. `--middleware content-sha256` adds a SHA-256 body digest. It skips
requests without a body or with an existing digest header.

If middleware changes a request body, compute its digest after that change.
Generate without the digest option and place exported `contentSha256` last in
your `middleware` array; remove any previous digest header before recomputing.
Do not combine an earlier generated digest with a later body mutation.

## Qualification

Run `bash tools/test-ts.sh` for generated type checks, fake-fetch runtime tests,
all 15 shared serialization cases, and shared retry cases. Set `TSC` to use an existing TypeScript compiler.
`bash tools/test-e2e.sh` also runs the TypeScript client against the real weather
service. Browser APIs are used by the runtime, but Node tests alone do not prove
a particular browser application's CORS, authentication, or deployment setup.

See [qualification levels](../clients.md#qualification) for the distinction
between local generation, package consumption, and authenticated Leyline proof.
