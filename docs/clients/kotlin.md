# Kotlin clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate kotlin` emits a Kotlin Multiplatform library. Models, codecs,
request preparation, retries, and pagination share `commonMain` source code.
The generated Gradle project targets JVM, Android (API 26+), JavaScript (browser
and Node), iOS arm64/device and simulator, macOS arm64, and Windows x64 Native.
It uses Kotlin 2.2.20, Ktor 3.3.3, kotlinx.coroutines, kotlinx.serialization JSON,
and kotlinx-datetime. Arbitrary precision numbers use Kotlin Multiplatform Bignum.
A Maven build packages the same common sources for JVM
and Android callers who do not need Gradle Multiplatform metadata.

## Generating and installing

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate kotlin model \
  --out generated/client --group-id example --artifact-id weather-client \
  --package-name example.weather --package-version 0.1.0 \
  --endpoint https://api.example
# JVM jar:
mvn -f generated/client/pom.xml package
# Multiplatform artifacts (run from generated/client):
gradle jvmJar jsJar
# Publish target artifacts and Kotlin Multiplatform metadata to a local repository:
gradle publishJvmPublicationToQualificationRepository \
  publishJsPublicationToQualificationRepository \
  publishKotlinMultiplatformPublicationToQualificationRepository \
  -PqualificationRepository=file:///ABSOLUTE/PATH/repository
```

Use Gradle 8.13. For a JVM/JS-only build, pass `-Pcrucible.native=false` to omit
Native targets and their compiler setup; omit this property when publishing
Apple or Windows artifacts. Kotlin 2.2 consumers using timestamp properties
must opt in to `kotlin.time.ExperimentalTime`.

The client name comes from the service: `LeylineClient` for `Leyline`,
or `WeatherClient` for the example below. The package also contains `ClientConfig`, error metadata, and top-level
model types in the `.model` package. Structures expose read-only properties and
type-safe builders; unions are sealed interfaces with one data class per variant,
and string and integer enums accept unknown values.
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
A unique list rejects duplicate wire values. Sparse collections use nullable
element or value types.
`BigInteger` and `BigDecimal` use `com.ionspin.kotlin.bignum` types, preserving
numeric JSON tokens without conversion to `Double`. Construct them with
`BigInteger.parseString(text)` or `BigDecimal.parseString(text)`.
Whole-body payloads support bytes, strings, and JSON structures/unions/documents,
including modeled errors. Media-type string headers use base64 wire encoding.
Each build applies modeled defaults to unassigned nonoptional members, creating
fresh mutable defaults. Builder properties remain null until assigned. Required
members without defaults must be assigned. Client-optional members remain omitted
until assigned, even when the model declares a default.

## Transport and cancellation

Operations are suspending functions; paginators return a cold `Flow`. Each
collection has its own pagination state, preserves caller input, and detects
token cycles. Token providers and middleware are suspending hooks invoked on
each attempt. The overall timeout includes those hooks, network I/O, and retry
delays. Hooks must cooperate with coroutine cancellation. Cancellation cancels
the active Ktor exchange, including a body read.

The default Ktor transport uses CIO on JVM/Android, the JS engine in browsers
and Node, Darwin on Apple platforms, and WinHttp on Windows Native. Redirects
are disabled, and the overall coroutine deadline includes transport work.
An injected Ktor `HttpClient` stays caller-owned and retains its own policies.
Transport injection also supports Ktor's MockEngine for common tests.
Close the generated client to release its owned transport. Credentials require
HTTPS except on loopback. Middleware runs before authentication. Successful responses without
modeled body members do not read the response body.

## Protocol

The Crucible JSON protocol applies the lowered HTTP bindings and timestamp formats.

The shipped
transport configuration requires HTTP/1.1 in the modeled protocol
transport list; an HTTP/2-only model is diagnosed during generation.

## Authentication

Bearer authentication is the only scheme. An operation the model protects with
a bearer token takes it from `token = { accessToken() }`, a suspending provider
called once per attempt after middleware, so a provider that refreshes its
token sees every retry. An anonymous operation never invokes the provider, and
a protected operation sent without a token carries no `Authorization` header.
The provider participates in the overall deadline.

## Models and errors

Optional members expose `T?`. A builder tracks assignment internally: leaving a
property untouched omits it, assigning `null` sends explicit null, and assigning
a value sends that value. Use `hasLocation()` when the distinction matters on a
response, or `unsetLocation()` in a builder to omit a previously assigned member.
`copy { ... }` preserves untouched presence states. Required output members expose
non-null types; building such a model checks that its required values are set. Missing response members receive modeled defaults; explicit null
does not receive a default. `Long` preserves signed 64-bit integers and
`kotlin.time.Instant` preserves nanosecond timestamps. Documents use immutable
`kotlinx.serialization.json.JsonElement` values, retaining numeric lexemes on
JavaScript as well as JVM and Native. Lists and maps use native Kotlin collection types through named type aliases.
Directly recursive collection aliases retain a nominal collection type because
Kotlin cannot represent a recursive type alias.
Blobs use `ByteArray` and standard base64 in JSON. A modeled blob payload is
sent and received as raw bytes with its declared request media type.

Modeled errors extend `ServiceError`, which retains the wire status, error
name, request ID, headers, and retry metadata. Malformed modeled error fields
fall back to that metadata instead of erasing the HTTP failure. Member names
that conflict with exception metadata receive a `Value` suffix. Remaining
model, accessor, union variant, and operation naming collisions are diagnostics.
GET and HEAD request bodies remain diagnosed for portable transport compatibility.

Retry eligibility follows [the shared contract](../clients.md). A valid
`Retry-After` overrides jitter, including zero. Idempotency tokens are generated
once per call. Transport failures retry only safe operations; malformed
successful JSON and cancellation do not retry. `retryMode` (below) limits
retries further; it never makes an ineligible failure eligible.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `<package>.permissions` package of its own, in common code for every
target, so no model name collides with it. Each resource type with permissions
a client may see has an open permission class named for the resource,
`GraphPermission`, in the enum classes' convention: one upper-snake companion
constant per permission (`mailSend` is `MAIL_SEND`), in declaration order, whose `value` is what grants store,
documented with the permission's own documentation (Java reads the constants
as static fields). Any string is accepted (`GraphPermission("graph:export")`):
a value this client does not know, from a newer server, is kept and written
back unchanged, so compare with the constants or `value`. A deprecated alias
is a `@Deprecated` constant, and `canonical()` maps it to its primary; any
other value, an unknown one included, is itself.

`PERMISSIONS` (`Permissions.PERMISSIONS` from Java) is the registry: a
`List<ResourcePermissions>` sorted by resource type, each with its
`resourceType` and `permissions` sorted by value. A `PermissionDescription`
has `value`, `description`, `tier` (`"public"`, `"permissionOnly"` or
`"notGrantable"`, a plain string because a newer server may add one),
`grantable`, `deprecated`, and `aliasOf` (the primary's value, or `null`).
A grant picker offers only what may be granted:

```kotlin
import example.leyline.permissions.GraphPermission
import example.leyline.permissions.PERMISSIONS

val offered = PERMISSIONS.flatMap { resource ->
    resource.permissions.filter { it.grantable && !it.deprecated }.map { resource.resourceType to it.value }
}
val stored = GraphPermission(grant.permission).canonical() // graph:view reads as GraphPermission.READ
```

Internal permissions never appear, whatever audience the client was generated
for. `tools/test-kotlin-multiplatform.py` runs a consumer test of the package
on every qualified target; `tools/test-kotlin.py` reads it from Java.

## Retry modes and telemetry

These follow the cross-language [runtime policy contract](../sdk-runtime-policy.md)
and pass its shared vectors.

```kotlin
val client = RecordsClient(ClientConfig(
    endpoint = "https://records.example",
    retryMode = RetryMode.ADAPTIVE,
    telemetry = object : TelemetryHook {
        override fun attemptFinished(event: AttemptFinished) = metrics.record(event.operation, event.status, event.latency)
    },
))
```

`retryMode` defaults to `RetryMode.LEGACY`, the behavior before retry modes
existed: every eligible failure retries up to `maxAttempts`. `STANDARD` adds a
retry quota shared by all calls of one client: 500 tokens, a retry costs 5 (10
after a transport timeout), a successful call refunds its last retry's cost or
adds 1, and a retry the quota cannot pay for is not made. `client.retryQuota`
exposes it (null in legacy mode); `retryQuota` and `getRetryQuota` are therefore
reserved client member names. `ADAPTIVE` adds the AWS SDKs' client-side rate
limiter: after a modeled throttling error, every attempt first takes a send token
refilled at a CUBIC-adjusted rate, sleeping under the call's timeout when none is
available (a cancelled sleep returns the token). Backoff is full jitter in every mode.

`telemetry` receives `OperationStarted`, then an `AttemptStarted`/`AttemptFinished`
pair per attempt that reached the transport, then `OperationFinished`. Events
carry an invocation ID, operation, endpoint, the target and headers as sent, statuses, error names (never messages), request IDs,
byte counts, latencies, and the retry reason and delay. They never carry bodies,
inputs or outputs, and replace with `***` the values of `Authorization`,
`Proxy-Authorization`, cookies, and every header, query parameter and path label bound to a
`@sensitive` member. Hooks are synchronous and run on the calling coroutine; one
that throws is logged through Ktor's `crucible.telemetry` logger and ignored.
There is no OpenTelemetry dependency or bundled adapter; map events onto a
tracer inside the hook.

`runtimeClock` (default `RuntimeClock.System`) supplies the monotonic time and
sleeps used for backoff, rate limiting and telemetry; tests inject a fake. A
call's `timeout` always uses the platform clock.

## Qualification

`python tools/test-kotlin.py` installs a Maven jar and compiles a separate
consumer, runs shared retry/header cases and weather-service HTTP tests, and
compiles empty and adversarially renamed models. The package itself compiles
against Java 8 APIs; the JVM test server requires JDK 17. The suite also compiles
a synthetic service with 100 records and 2,000 fields to detect JVM method-size limits.

`python tools/test-kotlin-multiplatform.py` publishes generated JVM/JS artifacts
and metadata into an isolated Maven repository, then compiles a separate
Multiplatform consumer against those artifacts. Its common tests verify exact
64-bit numbers, negative nanosecond timestamps, presence, URI escaping, cold
pagination, cancellation, injected transport ownership, exact arbitrary-precision
numbers, payloads, and bearer authentication on JVM and Node. A client generated
from `runtime-policy.json` runs every shared runtime-policy vector (retry quota
steps and calls, CUBIC, rate measurement and send tokens, telemetry events and
redaction), plus a throwing hook and cancellation telemetry.
`--native` also publishes and runs Windows x64 tests on Windows or
macOS arm64 tests on macOS, and compiles iOS device/simulator tests on macOS.
Apple binaries require an Apple host and Xcode; a local Windows run does not
prove Apple execution. Browser compilation does not establish browser live HTTP
or CORS behavior.

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

## Migrating JVM code to the portable API

Regenerate the SDK and update consumers together. Existing builder, copy,
suspending operation, presence, and `Flow` APIs retain their call syntax.
The platform-dependent value and transport types change as follows:

| Previous JVM type | Portable type |
| --- | --- |
| `java.time.Instant` | `kotlin.time.Instant` |
| `java.time.Duration` | `kotlin.time.Duration` |
| Gson `JsonElement` | `kotlinx.serialization.json.JsonElement` |
| injected `okhttp3.OkHttpClient` | injected `io.ktor.client.HttpClient` |
| middleware `java.net.URI` | `io.ktor.http.Url` |

```kotlin
import kotlin.time.Duration.Companion.seconds
import kotlin.time.Instant
import kotlinx.serialization.json.Json
import io.ktor.http.Url

val timestamp = Instant.fromEpochSeconds(-1, 999_999_999)
val seconds = timestamp.epochSeconds
val nanos = timestamp.nanosecondsOfSecond
val document = Json.parseToJsonElement("""{"count":9223372036854775807}""")
val config = ClientConfig(timeout = 30.seconds, middleware = listOf { request ->
    request.uri = Url("https://api.example" + request.uri.encodedPath)
})
```

Replace Gson mutation with `buildJsonObject` or a copied `JsonObject` map.
Documents are immutable; blobs remain mutable `ByteArray` values. JVM callers
can use kotlinx-datetime's JVM conversion extensions at application boundaries.
To roll back a consumer migration, restore its previous generated artifact and
imports together; never mix the old Gson model jar with the new portable runtime.
