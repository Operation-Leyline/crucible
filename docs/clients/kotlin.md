# Kotlin clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate kotlin` emits native Kotlin models, coroutine operations,
and a Maven jar for JVM and Android applications. The generated source uses
Kotlin 2.2.20, Java 8 APIs, OkHttp, Gson, and kotlinx.coroutines. Android
consumers need API 26 or later. Kotlin/Native and Kotlin/JS are separate future
targets.

## Generating and installing

```sh
cargo run -p crucible-cli -- generate kotlin model.json \
  --out generated/client --group-id dev.leyline --artifact-id leyline-client \
  --package-name dev.leyline --package-version 0.1.0 \
  --endpoint https://api.example --content-sha256
mvn -f generated/client/pom.xml package
```

The client name comes from the Smithy service: `LeylineClient` for `Leyline`,
or `WeatherClient` for the example below. The package also contains `ClientConfig`, error metadata, and top-level
model types in the `.model` package. Structures expose read-only properties and
type-safe builders; unions are sealed interfaces with one data class per variant,
and string enums accept unknown string values.
Unknown union variants preserve their name and JSON value. Generated codecs
dispatch directly to constructors without model reflection.

## Calling operations

```kotlin
import example.weather.WeatherClient
import example.weather.ClientConfig

WeatherClient(ClientConfig(endpoint = "https://weather.example", token = { accessToken() })).use { client ->
    val city = client.createCity { name = "London" }
    println(city.city.name)
    client.listCitiesPages { maxResults = 20 }.collect { page ->
        page.cities.forEach { println(it.name) }
    }
}
```

Request objects can also be constructed independently:

```kotlin
import example.weather.model.CreateCityInput

val request = CreateCityInput { name = "London" }
val changed = request.copy { name = "Oslo" }
client.createCity(changed)
```

Every operation with a modeled input has a builder overload; paginators have the
same overload. `client.listCities()` builds an input with every optional member
omitted. Lists and maps accept `listOf(...)` and `mapOf(...)` directly.

## Transport and cancellation

Operations are suspending functions; paginators return a cold `Flow`. Each
collection has its own pagination state, preserves caller input, and detects
token cycles. Token providers and middleware are suspending hooks invoked on
each attempt. The overall timeout includes those hooks, network I/O, and retry
delays. Hooks must cooperate with coroutine cancellation. Cancellation cancels
an active OkHttp call, including a body read.

The default transport disables redirects, hidden transport retries, and
independent I/O timeouts so the configured overall deadline controls the call.
An injected `OkHttpClient` stays caller-owned and retains its own policies.
Close the generated client to release its owned transport. Credentials require
HTTPS except on loopback. Middleware runs before the optional SHA-256 digest,
which covers exactly the buffered bytes sent. Successful responses without
modeled body members do not read the response body.

## Models and errors

Optional members expose `T?`. A builder tracks assignment internally: leaving a
property untouched omits it, assigning `null` sends explicit null, and assigning
a value sends that value. Use `hasLocation()` when the distinction matters on a
response, or `unsetLocation()` in a builder to omit a previously assigned member.
`copy { ... }` preserves untouched presence states. Required output members expose
non-null types; building such a model checks that its required values are set. Missing response members receive modeled defaults; explicit null
does not receive a default. `Long` preserves signed 64-bit integers and
`Instant` preserves nanosecond timestamps. Documents use Gson JSON values,
retaining integer tokens. Lists and maps use native Kotlin collection types through named type aliases.
Directly recursive collection aliases retain a nominal collection type because
Kotlin cannot represent a recursive type alias.
Blobs use `ByteArray` and standard base64 in JSON. A modeled blob payload is
sent and received as raw bytes with its declared request media type; body
digests cover those exact bytes.

Modeled errors extend `ServiceError`, which retains the wire status, error
name, request ID, headers, and retry metadata. Malformed modeled error fields
fall back to that metadata instead of erasing the HTTP failure. Member names
that conflict with exception metadata receive a `Value` suffix. Remaining
model, accessor, union variant, and operation naming collisions are diagnostics.
OkHttp cannot send GET or HEAD request bodies, so the generator diagnoses these
bindings explicitly.

Retry eligibility follows [the shared contract](../clients.md). A valid
`Retry-After` overrides jitter, including zero. Idempotency tokens are generated
once per call. Transport failures retry only safe operations; malformed
successful JSON and cancellation do not retry.

## Qualification

`python tools/test-kotlin.py` installs a Maven jar and compiles a separate
consumer, runs shared retry/header cases and weather-service HTTP tests, and
compiles empty and adversarially renamed models. The package itself compiles
against Java 8 APIs; the JVM test server requires JDK 17.

`python tools/test-kotlin-android.py` builds and lints a separate Android app
against the installed jar, then runs precision, cancellation, and weather
tests on an attached emulator. It requires Gradle 8.13, Android SDK 35, and
`adb`; `--build-only` omits device tests. CI uses an API 26 emulator. Android
build configuration follows the [AGP compatibility table](https://developer.android.com/build/releases/agp-8-11-0-release-notes).

The complete Leyline model is checked separately using its adapted Crucible
projection. Package compilation, emulator tests, and local weather tests do
not establish published-package or authenticated deployed-Leyline compatibility.

## Migrating from Models and OptionalValue

Replace the generated `Client` import and constructor with the service-specific
name (`WeatherClient` or `LeylineClient`).

Import `example.weather.model.CreateCityInput` instead of `Models.CreateCityInput`.
Replace `CreateCityInput(name = OptionalValue.of("London"))` with
`CreateCityInput { name = "London" }`, or use `client.createCity { name = "London" }`.
Read `result.caption` instead of `result.caption.value`. `OptionalValue` is now an
internal codec type. Use `hasCaption()` only when you need to distinguish omitted
from explicit null. Replace mutable structure assignments and data-class copies
with `request.copy { ... }`; modeled-error copies retain response metadata.

Replace named collection constructors with `listOf` / `mapOf`, and `Models.Unit`
with `kotlin.Unit`. Regenerate and upgrade the jar and its consumers together.
Keep the previous generator revision, generated jar, and consumer revision as
one rollback unit; the wire contract has not changed.
