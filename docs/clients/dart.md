# Dart clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate dart` emits typed models, asynchronous operations, and
`Stream` pagination. Generated packages require Dart 3.6.2 or later within
Dart 3.x, `package:http` 1.6, and `package:crypto` 3.x. They use the same
runtime on the native VM and in browsers, with no Flutter dependency.

## Generating and installing

```sh
cargo run -p crucible-cli -- generate dart model.json \
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

## Migrating existing callers

Pass ordinary typed values instead of `Field.value(...)` and read response
properties directly instead of extracting `.value`. Move explicit null
arguments and state checks to the companion `...Presence` API. Const constructors
remain available. Regenerate and rebuild the package and its consumers together.

## Qualification

`python tools/test-dart.py` checks shared retry and header cases,
real HTTP, minimum-version analysis, native and JavaScript consumer compilation,
and empty and renamed packages. Set `DART` to select the SDK executable and
`CHROME_EXECUTABLE` if Chrome is not on its usual path. These checks do not
establish Flutter device, published-package, or authenticated deployed-Leyline
compatibility.
