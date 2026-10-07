# TypeScript and JavaScript clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates a dependency-free ESM client for Node 22 and browsers that
provide `fetch`, `Request`, Web Crypto, and `structuredClone`. Models are
TypeScript interfaces and aliases; JavaScript callers use the same built package.
The package declares TypeScript `>=5.7 <7` as a build dependency.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate ts smithy \
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

Use `--service` when the sources define multiple services. Package metadata
records the generator and service versions. The packaged `crucible.json` manifest
also records `generatorVersion`, `service`, `serviceVersion`, and `schemaSha256`:
the SHA-256 digest of the shared lowered wire schema.

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
`Promise<Response>`. The default uses global `fetch`, including its redirect
behavior. The client creates no separate closable connection pool. Default
headers come from `ClientConfig.headers`; per-call headers and `AbortSignal`
come from `RequestOptions`.

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
all 16 shared serialization cases, shared retry cases, the shared checksum
vectors, and the published event stream framing and signing vectors. The event
stream suite also drives generated clients (restJson1, awsJson1_0, and SigV4)
against a loopback Node HTTP/1.1 server. Set `TSC` to use an existing TypeScript compiler and `CRUCIBLE_BIN` to
generate with a prebuilt CLI instead of `cargo run`.
`bash tools/test-e2e.sh` also runs the TypeScript client against the real weather
service. Browser APIs are used by the runtime, but Node tests alone do not prove
a particular browser application's CORS, authentication, or deployment setup.

See [qualification levels](../clients.md#qualification) for the distinction
between local generation, package consumption, and authenticated Leyline proof.

### Dynamic HTTP bindings

Generated clients support `@jsonName`, greedy path labels, `@httpQueryParams`,
`@httpPrefixHeaders`, and `@httpResponseCode`. Public member names stay unchanged;
JSON names affect only the wire. Explicit query/header bindings take precedence
when a map supplies the same name. Greedy labels preserve slash separators and
reject dot-only segments before dispatch. Response-code members read the actual
HTTP status, including modeled errors. Nested pagination token paths continue to
use the public model names.

### Streaming payloads

An `@streaming` blob uses `ByteStream`. Pass a `ReadableStream<Uint8Array>` for a
one-shot upload, or a factory that returns a fresh stream containing the same
bytes for each attempt. One-shot uploads never retry after opening. Factories
follow the operation's existing retry policy.

```ts
const body = new ByteStream(() => openFileStream(), { length: fileSize, sha256: digest });
await client.upload({ body });
const output = await client.download();
const reader = output.body.open().getReader();
try {
  for (;;) {
    const chunk = await reader.read();
    if (chunk.done) break;
    consume(chunk.value);
  }
} finally {
  await reader.cancel();
  reader.releaseLock();
}
```

Call deadlines and cancellation remain active until the response stream ends or
is cancelled. Cancel an unopened response with `output.body.cancel()`. Streaming
never collects the body for hashing: `contentSha256` requires a precomputed
`sha256` value (or an explicitly supplied digest header). `@requiresLength`
requires a declared length, which is checked while reading. Modeled length
bounds are also checked incrementally. Request streaming requires Fetch support
for streaming request bodies; browsers control which HTTP headers can be sent.
The native gate covers Node's Fetch implementation.

### Event streams

A member targeting an `@streaming` union is an `EventStream<Union>`. To send
events, construct one from an iterable or async iterable of union values; an
async generator is pulled only when Fetch reads more of the request body, so a
slow service slows the source. A source that throws aborts the request with
`NetworkError`. A call that sends events is never retried.

```ts
async function* outgoing(): AsyncGenerator<ClientEvents> {
  yield { message: { id: "m1", text: "hello" } };
}
await client.publish({ channel: "general", events: new EventStream(outgoing()) });

const output = await client.subscribe({ channel: "news" });
try {
  for await (const event of output.events) {
    if (event.message) console.log(event.message.text);
  }
} catch (error) {
  if (error instanceof ThrottledError) { /* the stream's modeled error */ }
}
```

A received stream is read once, with `for await` or its async iterator. A
modeled error in the stream is thrown as its generated error class, with the
HTTP response's `$metadata`; the stream's union type leaves those members out.
An `error` message, or an exception type the client does not know, is thrown as
`EventStreamError` with `code` and `message`. Invalid framing, a checksum
mismatch, or an event that does not match the model is a `TypeError` (a
`SyntaxError` for a payload that is not JSON). An event
type the client does not know arrives as
`{ $unknown: { name, json } }`, where `json` is the payload's JSON text (a JSON
string of base64 for a non-JSON payload). Leaving the loop early, or calling
`cancel()` on an unread stream, releases the connection. The call's
`timeoutMs` budget and abort signal stay active until the stream ends and
reject a pending read with `TimeoutError` or the signal's reason.

Each event is one AWS event stream message (`application/vnd.amazon.eventstream`)
with `:message-type`, `:event-type`, and `:content-type` headers. `@eventHeader`
members are typed headers (`boolean`, byte/short/integer `number`, long
`bigint`, `Uint8Array`, `string`, and `Date` at millisecond precision). An
`@eventPayload` blob or string is the raw payload, and a structure is its JSON
document; otherwise the remaining members form a JSON document. Prelude and
message CRC32s are verified; headers are limited to 128 KiB and payloads to
16 MiB. With `restJson1` the operation's other members use HTTP bindings. With
`awsJson1_0` and `awsJson1_1` they are sent as an `initial-request` message
first, and an `initial-response` message, when the service sends one, fills the
output's other members before the call resolves. HTTP errors before the stream
are decoded as the operation's errors.

With SigV4 authentication the request declares
`x-amz-content-sha256: STREAMING-AWS4-HMAC-SHA256-EVENTS`, each message is
wrapped in an envelope whose `:chunk-signature` chains from the request's
signature, and the stream ends with an empty signed message. Signing uses Web
Crypto and is checked against the published smithy-rs string-to-sign vector.
Responses are not signed.

Node 22's Fetch streams request bodies over HTTP/1.1 and resolves the response
as soon as its headers arrive, so input, output, and bidirectional streams are
interactive there; the tests exchange events on one connection. Browsers differ:
Fetch request streaming is half duplex (the response is not available until the
request body is complete), Chromium streams request bodies only over HTTP/2 or
later, and Firefox and Safari do not stream them. Output streams work wherever
response bodies stream; input and bidirectional streams are not qualified in
browsers. A middleware that reads `request.body` would buffer an unbounded
stream, and `contentSha256` is refused for an unsigned event stream. Sent
events are converted with the model's schemas but not checked against length,
range, or pattern constraints. Generation refuses a modeled error in an input
event stream.

### Exact numbers and collections

`bigInteger` uses `bigint`. `bigDecimal` uses the exported immutable `Decimal`:
construct it from decimal text or `bigint`, inspect it with `toString()`, and
compare numeric values with `equals()`. The codecs write JSON numeric tokens;
values, defaults, HTTP headers, and retry snapshots never pass through `number`.
Integer enums expose named numeric constants and accept future signed 32-bit
values when reading responses.

Sparse lists and maps include `null` in their element or value type. Sets and
`@uniqueItems` lists remain arrays; encoding and decoding reject duplicate
values, comparing objects structurally and numbers mathematically. Dense collections reject null elements, including document elements; null nested inside a document is preserved.

### Authentication options

`basicCredentials` accepts `{ username, password }` or a sync/async provider;
`apiKey` accepts a string or provider. The model selects the API-key header or
query parameter and optional scheme prefix. Identities are selected in modeled
priority order and resolved again on each attempt. Explicit matching credentials
in request headers take precedence over providers. `@auth([])` invokes no
identity provider; `@optionalAuth` permits anonymous calls when none is available.
Required Basic/API-key combinations fail before dispatch if no identity exists.
The existing bearer-only behavior remains: without a token, the service decides
whether the anonymous call is allowed. Credential resolution shares the call's
timeout and cancellation signal.

### Endpoints and waiters

Models with endpoint rules accept `region`, `endpointParameters`, and
`endpointBuiltins`; `endpoint` supplies `SDK::Endpoint`. Rules provide the base
URL, headers, and signing properties. Modeled host prefixes expand input labels;
`disableHostPrefix` disables expansion. Invalid labels and rule errors fail
before Fetch runs.

Waiters generate `waitUntil<Name>(input, { maxWaitTimeMs })`. Results distinguish
`{ outcome: "response", output, attempts }` from
`{ outcome: "error", error, attempts }`, because a modeled error can satisfy a
success acceptor. `WaiterError` retains the last response and cause;
`WaiterTimeout` identifies an exhausted budget. `signal`, `minDelayMs`, and
`maxDelayMs` configure cancellation and polling. The total budget includes calls,
retries, and delays, and discarded response streams are cancelled before retry.
Generated helper names gain deterministic suffixes when ordinary operations use
the same name.

Endpoint paths and waiter expressions use public model member names. Timestamps
are compared as epoch seconds, while `bigint` and `Decimal` comparisons preserve
numeric precision. The bundled JMESPath interpreter has no external dependency.

### AWS signing and credential providers

For modeled SigV4 auth, configure `awsCredentials` with
`{ accessKeyId, secretAccessKey, sessionToken?, expiration? }` or a sync/async
provider. `region` and optional `signingName` configure the signing scope;
endpoint auth properties can override it. The client signs the final request
after middleware and resolves credentials for each attempt. Streaming requests
require a precomputed SHA-256 digest unless the model permits unsigned payloads.

`credentialsFromEnvironment` accepts an environment map;
`credentialsFromProfile` parses caller-supplied shared profile text. These work
in browsers without Node filesystem imports. Use `chainCredentials` for ordered
fallbacks and `cacheCredentials` to coalesce refreshes and refresh before
expiration. File loading remains an application-supplied async provider.

### HTTP checksums

Operations with `@httpChecksumRequired` send `Content-MD5`. With
`@aws.protocols#httpChecksum`, the request carries `x-amz-checksum-<algorithm>`
for the algorithm in the modeled member: CRC32, CRC32C, CRC64NVME, MD5, SHA1,
SHA256, or SHA512. Choosing another algorithm, such as an `XXHASH*` value, fails
before dispatch. If the caller chose none, the client selects CRC32, and it sets
the validation-mode member to `ENABLED`, but only in its private copy of the
input, never in the caller's object. A required checksum without an algorithm
member uses MD5.

The checksum covers the final request bytes, after all middleware and before
SigV4 signing; retries compute it again. A checksum header the caller already
supplied (via its modeled member, `RequestOptions.headers`, `ClientConfig.headers`,
or middleware) is sent unchanged. An empty body is still checksummed.

When validation is enabled, the first modeled response algorithm present in the
response headers is verified against the received bytes. A mismatch throws
`ChecksumError` (with `algorithm`) and is not retried, because the request may
already have taken effect. Unmodeled or unsupported response checksums are
ignored, as are error responses.

```ts
const client = new ChecksumsClient({
  endpoint: "https://checksums.example",
  requestChecksumCalculation: "WHEN_REQUIRED",   // default "WHEN_SUPPORTED"
  responseChecksumValidation: "WHEN_SUPPORTED",
  checksumValidated: (operation, algorithm) => metrics.count(operation, algorithm),
});
```

Under `WHEN_REQUIRED`, the client sends a checksum only when the model requires
one or the caller chose an algorithm, and verifies responses only when the
caller enabled validation. `checksumValidated` runs once for each verified
response.

Streaming bodies are never buffered for checksums. A streamed upload gets no
default CRC32. If the model requires a checksum, or the caller chose an
algorithm, the caller must supply that header's value. Otherwise the call fails
before dispatch, and the stream is cancelled. A streamed response is hashed as
it is read. At its end, a mismatch errors the stream with `ChecksumError`
instead of ending it cleanly, and `checksumValidated` runs only once the body
has been read in full. All seven algorithms are bundled in TypeScript, because
Web Crypto offers no CRCs, no MD5, and no incremental digest. One-shot SHA
digests use `crypto.subtle` when it is available. Browsers additionally need
the service's CORS policy to allow the `x-amz-checksum-*` and `Content-MD5`
request headers and to expose the response checksum headers.
