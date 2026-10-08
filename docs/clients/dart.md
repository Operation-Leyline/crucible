# Dart clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate dart` emits typed models, asynchronous operations, and
`Stream` pagination. Generated packages require Dart 3.6.2 or later within
Dart 3.x, `package:http` 1.6, and `package:crypto` 3.x. They use the same
runtime on the native VM and in browsers, with no Flutter dependency.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate dart smithy \
  --out generated/client --package-name weather_client \
  --package-version 0.1.0 --endpoint https://api.example --content-sha256
cd generated/client
dart pub get
dart analyze
```

Add the generated directory as a path dependency, or distribute it through
your package repository. Generation writes a pubspec and provenance manifest;
it does not publish the package. The package owner supplies repository metadata,
license, and release documentation before publishing.

The public client type follows the service name: `WeatherClient` for Weather
and `LeylineClient` for Leyline. Package and module names remain configurable.

## Calling operations

```dart
import 'package:weather_client/weather_client.dart';

Future<void> readCities(TokenProvider accessToken) async {
  final client = WeatherClient(ClientConfig(
    endpoint: 'https://weather.example', token: accessToken));
  try {
    await client.createCity(const CreateCityInput(name: 'London'));
    await for (final page in client.listCitiesPages(
        const ListCitiesInput(maxResults: 20))) {
      for (final city in page.cities) { print(city.name); }
    }
  } finally { client.close(); }
}
```

## Models and precision

Models expose immutable typed properties and const constructors where possible. Collections
remain ordinary Dart collections. Each call snapshots its input before
asynchronous work, and retries reuse the snapshot. Optional members accept
native nullable values and expose native nullable getters: `name: 'London'`
and `output.caption`. An omitted or `null` ordinary argument means absent.
For advanced presence control, use the companion `namePresence` argument/getter:
`Field.unset()`, `Field.nullValue()`, or `Field.value(value)`. A supplied presence
value takes precedence over the ordinary argument. Required members are
constructor arguments. Missing response members receive modeled defaults;
explicit null does not receive a default.

Smithy `long` uses `BigInt` so browser compilation preserves the full signed
64-bit range. Other integer types use bounded `int`; floating-point values
must be finite. `Timestamp` stores whole seconds and nanoseconds and preserves
negative fractional epochs. Date-time and HTTP-date text require years 1–9999.
Its `DateTime` conversion uses the platform's DateTime precision; keep the
original Timestamp when exact nanoseconds matter. `JsonValue` retains numeric
lexemes for unknown union variants and document values. JSON nesting is bounded
to 255 levels.

String enums preserve unknown values in their `value` field. Unions are sealed
classes with a generated class for each variant and an unknown variant carrying
its name and `JsonValue`. Blobs are `Uint8List`, encoded as padded base64 in JSON.
Raw blob payloads use bytes directly. An empty response payload remains present;
an empty request payload sends no body or content type. Optional SHA-256 covers
the final bytes after middleware.

## API documentation and string representations

Every generated type, member, enum value, operation and the client carry
dartdoc comments built from the model's `@documentation` plus Crucible's own
text for the client, `ClientConfig`, transports, credentials, errors and
paginators; `dart doc` renders them without warnings. Bare `[name]` references
in model text are escaped so dartdoc does not try to resolve them.

Models override `toString` and print as
`Secret(name=db, password=*** Sensitive Data Redacted ***)`. Unset members are
omitted, nested models and collections recurse, a blob shows its length, and
`@sensitive` members (or members of a `@sensitive` shape or collection of
them) print the redaction marker. Unknown union variants show only their name.
A modeled error also shows its transport metadata, without headers or message.

Generated enums implement `==` and `hashCode`; structures and unions do not.

## Transport and errors

Calls return `Future<Output>` and accept a `CancellationToken`. One deadline
covers hooks, network I/O, decoding, and retry delays. Token providers and
middleware run for every attempt; generated idempotency tokens remain stable.
Cancellation returns promptly even when a hook ignores it. Hooks should still
cooperate so their own work can finish. Pagination starts on subscription,
honors pauses, detects token cycles, and cancels its active call when the
subscription is cancelled. Each page has its own deadline.

The default transport refuses redirects and adds `Cache-Control: no-cache,
no-store` to prevent a browser from reusing authenticated responses. Browser
servers must allow this header, Authorization, modeled headers, and the optional
digest header in their CORS policy; expose response headers used by the model.
Fetch reports refused redirects as transport errors. Browsers control their
own connection and cookie policy; cross-origin credentials are not enabled by
the default HTTP client. Credentials require HTTPS except on loopback.

`close()` closes an owned transport and prevents new calls. An injected
`HttpTransport` remains caller-owned and must honor cancellation and `readBody`.
Successful responses with no modeled body are discarded without draining them.
Modeled errors implement `ServiceError`; metadata retains wire status, name,
request ID, headers, and retry traits even if modeled members are malformed.
Retries follow [the shared contract](../clients.md), with bounded attempts and
cancellable Retry-After or jitter delays.

Generation diagnoses naming collisions, recursive collection aliases, and
GET/HEAD bodies unsupported by browser Fetch. Nominal recursive structures are
supported.

## Authentication

Operations use the modeled schemes in priority order: the first scheme with
credentials wins. Configure one provider per scheme; each is called on every
attempt and may return `null` when unavailable.

```dart
final client = AuthClient(ClientConfig(
  endpoint: 'https://api.example',
  token: () => bearerToken,                              // httpBearerAuth
  basicAuth: () => BasicCredentials('user', password),   // httpBasicAuth
  apiKey: () => apiKey,                                  // httpApiKeyAuth
  credentials: CachedCredentials(credentialChain([
      () => environmentCredentials(), () => profileCredentials()])),
  region: 'us-east-1'));                                 // aws.auth#sigv4
```

A matching credential already present on the request (for example an
`Authorization: Basic ...` default header, or the modeled API-key header or
query parameter) is sent as is, and no provider is called. Operations with
`@auth([])` send no credentials; `@optionalAuth` operations call anonymously
when no provider has an identity. Otherwise a missing identity is a
`StateError` before anything is sent. Credentials require HTTPS except on
loopback.

SigV4 signs the final bytes after middleware, checksums, and the body digest,
so each retry is signed again with a fresh identity. `@unsignedPayload`
operations sign `UNSIGNED-PAYLOAD`. S3 keeps its path as sent; other services
normalize the path and encode it twice. `signV4` is also exported for signing
requests directly. `CachedCredentials` shares one provider call across
concurrent requests and refreshes before expiry (60 seconds, or half of a short
lifetime). `environmentCredentials` and `profileCredentials` read the standard
AWS environment variables and shared credentials file on native platforms. They
never run `credential_process`. In browsers they throw `UnsupportedError`.
`BasicCredentials`, `AwsCredentials` and `CachedCredentials` redact secrets in
`toString`.

## Endpoints

When the model has `@smithy.rules#endpointRuleSet`, every call resolves its
endpoint from the rules, so `endpoint` may be omitted. Parameters come from
operation context, input members and paths, `ClientConfig.endpointParameters`,
built-ins (`AWS::Region` from `region`, `SDK::Endpoint` from `endpoint`,
overridable in `endpointBuiltIns`), then defaults. Resolved endpoint headers
are sent, and `authSchemes` properties select and configure the signer, for
example the signing region. Rule errors, missing required parameters, and
invalid URLs raise `EndpointException` before anything is sent.

`@endpoint(hostPrefix: ...)` prepends the expanded prefix to the host. Each
`@hostLabel` value must be one DNS label, and the expanded host must remain a
valid host name, or the call raises `EndpointException`. Set
`disableHostPrefix: true` to keep the configured host.

## Waiters

Each modeled waiter generates `waitUntil<Name>(input, timeout: ...)`, which
returns the successful output, or `null` when an error acceptor succeeds.
The generated return type is `Future<Output?>`. Acceptors are evaluated in
order over the input and output by member name, with timestamps as epoch seconds and exact
numbers. Delays grow exponentially from the modeled `minDelay` to `maxDelay`
with jitter, and are clipped to the deadline; `minDelay` and `maxDelay` may be
overridden. One `timeout` covers every attempt and delay, and each attempt
keeps the client's request timeout. A failure state raises `WaiterException`
with the attempt count, last response or error, and the acceptor's message.
The deadline raises `WaiterTimeoutException` and cancels the attempt in
flight; a `CancellationToken` cancels the waiter and its attempt.

## Checksums

`@httpChecksumRequired` and `@aws.protocols#httpChecksum` operations send
`Content-MD5` or `x-amz-checksum-<algorithm>` over the final request bytes.
CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512 are supported, in the
browser as well. By default (`ChecksumPolicy.whenSupported`) the client sends
CRC32 when the caller chooses no algorithm and enables response validation;
`whenRequired` limits both to operations that require them. A checksum header
supplied by the caller or middleware is kept. Responses are validated against
the first modeled algorithm the server returned; a mismatch raises
`ChecksumException`, and `checksumValidated` reports each verified algorithm.
An unsupported requested algorithm is an `ArgumentError` before sending.

## AWS JSON protocols

Models using `aws.protocols#restJson1`, `awsJson1_0`, or `awsJson1_1` (select
one with `--protocol` when a service lists several) use the same client API.
restJson1 keeps the HTTP bindings, with HTTP-date headers and ISO 8601 query
and label timestamps. The AWS JSON 1.0 and 1.1 protocols POST every member as
JSON to `/` with `X-Amz-Target: <Service>.<Operation>` and their content type,
sending `{}` for empty input. All three use epoch-second body timestamps,
send `NaN` and infinities as strings, treat null structure members as absent,
and ignore a union's `__type`. Errors are identified by `X-Amzn-ErrorType`,
`code`, or `__type` (namespace and URI suffix removed); request IDs come from
`X-Amzn-RequestId` or `X-Amz-Request-Id`, and messages from `message` or
`Message`.

## Migrating existing callers

Pass ordinary typed values instead of `Field.value(...)` and read response
properties directly instead of extracting `.value`. Move explicit null
arguments and state checks to the companion `...Presence` API. Const constructors
remain available. Regenerate and rebuild the package and its consumers together.

## Qualification

`python tools/test-dart.py` checks shared retry and header cases,
real HTTP, minimum-version analysis, native and JavaScript consumer compilation,
and empty and renamed packages. It also generates the authentication, waiter,
endpoint-rules, checksum, and three AWS JSON protocol models and runs their
tests on the VM and in Chrome: the published S3 SigV4 vector, credential
selection and refresh, the shared checksum vectors, rules functions and host
labels, waiter matchers, deadlines and cancellation, and protocol dispatch and
error decoding. Set `DART` to select the SDK executable and
`CHROME_EXECUTABLE` if Chrome is not on its usual path. These checks do not
establish Flutter device, published-package, or authenticated deployed-Leyline
compatibility.
