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

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate kotlin smithy \
  --out generated/client --group-id example --artifact-id weather-client \
  --package-name example.weather --package-version 0.1.0 \
  --endpoint https://api.example --content-sha256
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

The client name comes from the Smithy service: `LeylineClient` for `Leyline`,
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
Smithy sets use `Set<T>` and accept `setOf(...)`; duplicate wire values are
rejected. Sparse collections use nullable element or value types.
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
HTTPS except on loopback. Middleware runs before the optional SHA-256 digest,
which covers exactly the buffered bytes sent (a streamed body uses its supplied
`sha256`). Successful responses without
modeled body members do not read the response body.

## Streaming blobs

A `@streaming` blob member has type `ByteStream`, and its bytes are never
buffered by the client. Construct a request body as one of:

| Constructor | Retries | Use |
| --- | --- | --- |
| `ByteStream.once(channel, contentLength, sha256)` | never | a single `ByteReadChannel`; the operation is sent at most once, even when idempotent |
| `ByteStream.replay(contentLength, sha256) { channel }` | per the shared contract | the factory runs once per attempt and must reproduce the same bytes |
| `ByteStream.fromBytes(bytes)` | per the shared contract | a snapshot with its length and SHA-256 |

```kotlin
val body = ByteStream.replay(contentLength = file.size) { openFileChannel(file) }
client.upload { this.body = body }
```

`contentLength` becomes the `Content-Length` header; without it the body is sent
with chunked transfer encoding. The client checks the bytes actually read against
the declared length and the modeled `@length` bounds, and fails with
`IllegalArgumentException` (never retried) on a mismatch. A `@requiresLength` body
without `contentLength` is rejected before network I/O. A one-shot stream can be
opened once; sending it again raises `IllegalStateException`. The caller owns an
unsent request body: `close()` cancels a one-shot channel.

Signing and `--content-sha256` need the payload digest before the body is sent.
Supply `sha256` (64 hexadecimal digits) for a streamed body, or model the operation
with `@unsignedPayload`; otherwise the call fails before sending.

An operation with a streaming response takes a consumer. The response, including
its body, is readable only inside the consumer; the exchange ends when the
consumer returns, and an unread body is cancelled rather than drained:

```kotlin
val bytes = client.download { output ->
    val channel = output.body.openChannel()   // read with Ktor's ByteReadChannel APIs
    copyToFile(channel)
}
val small = client.download { it.body.toByteArray(limit = 1 shl 20) }
```

The overall `timeout` covers the consumer. Coroutine cancellation stops the
transfer and cancels both the response body and an open request source. The
response length is checked against `Content-Length` and the modeled bounds.
Streaming operations cannot be paginated or waited on; such models are diagnosed.
Event streams (`@streaming` unions) are not supported and are diagnosed.

The JVM and Android CIO engine streams both directions. Ktor's JavaScript engine
reads a request body into memory before calling `fetch`; responses still stream.

## AWS JSON protocols and checksums

The generator accepts `aws.protocols#restJson1`, `aws.protocols#awsJson1_0`, and
`aws.protocols#awsJson1_1`. Use `--protocol aws.protocols#restJson1` to choose
among multiple protocol traits declared by one service. RPC protocols send the
modeled operation through `X-Amz-Target` and serialize an empty input as `{}`.
REST JSON applies the lowered HTTP bindings and timestamp formats. AWS JSON
omits null ordinary structure members, retains explicit document nulls, and
supports the protocol's string representations of nonfinite floating-point values.

Payloads support `httpChecksumRequired` and `aws.protocols#httpChecksum`.
The supported algorithms are CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and
SHA512. Selecting an unsupported XXHASH algorithm raises an error before sending.
Required legacy checksums use `Content-MD5`; buffered flexible requests default to CRC32
and response validation defaults to `ENABLED` when the model supplies those input
members. These defaults change only the private request copy. Explicit checksum
headers take precedence. Request checksums cover the final middleware body and
are included in authentication signing. Response verification runs before decoding;
a mismatch raises `ChecksumMismatch` and returns no model.

Streamed requests select a checksum only when required or explicitly configured.
Optional streams keep the default policy without an automatic checksum pass.
Streaming bodies are checksummed without buffering. A replayable request body is
read once more before sending to compute its checksum (once per call, not per
attempt); a SHA256 checksum of a body with a supplied `sha256` needs no extra read.
A one-shot body needs a caller-supplied header when a checksum is selected;
otherwise that checksum request fails before sending. A streamed response is verified as the consumer reads it: at the
end of the body a mismatch fails the read, and the call raises `ChecksumMismatch`
even if the consumer caught that failure. A body the consumer does not read to
the end is not verified, and the callback is not invoked.

`requestChecksumCalculation` and `responseChecksumValidation` default to
`ChecksumPolicy.WHEN_SUPPORTED`. Set either to `WHEN_REQUIRED` to disable its
automatic opt-in while retaining required request checksums and explicit input
choices. The `checksumValidated` callback receives the operation name and the
single algorithm actually verified. It is invoked only after successful validation;
absence of a callback means no checksum was verified for that response.

Kotlin uses KotlinCrypto 0.8.0 for portable cryptographic digests. The shipped
transport configuration requires HTTP/1.1 in the modeled protocol
transport list; an HTTP/2-only model is diagnosed during generation.

## Authentication and credentials

Each operation chooses among its effective modeled schemes in order. Configure
`basicAuth = { BasicCredentials(user, password) }`, `apiKey = { key }`, or
`credentials = { AwsCredentials(accessKey, secret, sessionToken) }` with `region`
for AWS Signature V4. API keys use the modeled header or query location.
Anonymous operations never invoke identity providers. Optional authentication
allows no identity; additional required schemes report absent credentials before
network I/O. Providers suspend, participate in the overall deadline, and run per
attempt. Signing happens after middleware and covers the final bytes and headers.

```kotlin
val cached = CachedCredentials(credentialChain({ applicationCredentials() }, { refreshCredentials() }))
val config = ClientConfig(endpoint = endpoint, region = "us-east-1", credentials = cached::get)
```

`CachedCredentials` permits one refresh at a time and refreshes one minute before
expiry by default. Cancellation releases the refresh lock. These providers work
in common code; applications supply platform-specific credential storage access.
Credential diagnostic strings redact secrets.

## Endpoints and waiters

A service with endpoint rules can resolve its endpoint without a configured URL.
The rules consume `region`, `endpointBuiltins`, `endpointParameters`, and modeled
input sources in shared Smithy precedence. Custom parameter maps contain immutable
`JsonElement` values. An explicit URL is passed to rules as `SDK::Endpoint`; the
rules decide how it affects resolution. Resolved headers and authentication
properties participate in request preparation and signing. Host-prefix labels are
validated before transport; `disableHostPrefix = true` disables that expansion.

A modeled waiter named `Ready` generates `waitUntilReady(input, options)` and a
builder overload. These are suspending functions:

```kotlin
val response = client.waitUntilReady(WaiterOptions(timeout = 2.minutes)) {
    names = listOf("job")
}
```

Import `kotlin.time.Duration.Companion.minutes` for this example. The result is
nullable: an error acceptor can mean success, such as a missing deleted resource,
and then the waiter returns `null`. `WaiterError` retains the attempt count, last
response/error, and modeled failure message; `WaiterTimeout` identifies the total
deadline. The default deadline is five minutes and includes operations, their
retries, and polling delays. Coroutine cancellation stops the active request and
further polling. Optional delay overrides replace modeled bounds; otherwise the
waiter applies their jittered backoff. Compiled query expressions use modeled
member names, including wire-renamed fields and timestamps.

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
sent and received as raw bytes with its declared request media type; body
digests cover those exact bytes.

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

## Retry modes, endpoint discovery, and telemetry

These follow the cross-language [runtime policy contract](../sdk-runtime-policy.md)
and pass its shared vectors.

```kotlin
val client = RecordsClient(ClientConfig(
    endpoint = "https://records.example",
    retryMode = RetryMode.ADAPTIVE,
    endpointDiscovery = true,
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

For a service with `@aws.api#clientEndpointDiscovery`, `endpointDiscovery = null`
(the default) discovers endpoints for operations whose discovered endpoint is
required, `true` also for optional ones, and `false` never: a required operation
then fails with `EndpointDiscoveryError` before sending. The discovery call is an
ordinary authenticated, retried and observed call, bounded by the remaining
timeout of the call that started it. Its first usable address is cached per
client, keyed by operation and identifiers, for `CachePeriodInMinutes`; a response
without one fails a required operation with `EndpointDiscoveryError`. Concurrent
calls needing the same key share one discovery, which runs in the client's own
coroutine scope: cancelling the call that started it does not cancel it for the
others, and closing the client cancels it. An optional operation whose discovery
fails uses the configured endpoint for that key for 60 seconds. HTTP 421 or the
modeled invalid-endpoint error evicts the entry and is retried, even for a write,
within the attempt limit and the retry quota. Host prefixes are not applied to a
discovered address.

`telemetry` receives `OperationStarted`, then an `AttemptStarted`/`AttemptFinished`
pair per attempt that reached the transport, then `OperationFinished`; a discovery
call's own events appear inside the call that needed it. Events carry an invocation
ID, operation, endpoint and its source (`configured`, `rules`, `discovered`), the
target and headers as sent, statuses, error names (never messages), request IDs,
byte counts, latencies, and the retry reason and delay. They never carry bodies,
inputs or outputs, and replace with `***` the values of `Authorization`,
`Proxy-Authorization`, cookies, `X-Amz-Security-Token`, AWS signature query
parameters, API keys, and every header, query parameter and path label bound to a
`@sensitive` member. Hooks are synchronous and run on the calling coroutine; one
that throws is logged through Ktor's `crucible.telemetry` logger and ignored.
There is no OpenTelemetry dependency or bundled adapter; map events onto a
tracer inside the hook.

`runtimeClock` (default `RuntimeClock.System`) supplies the monotonic time and
sleeps used for backoff, rate limiting, discovery expiry and telemetry; tests
inject a fake. It is distinct from `clock`, the wall clock used for signing, and a
call's `timeout` always uses the platform clock.

## Qualification

`python tools/test-kotlin.py` installs a Maven jar and compiles a separate
consumer, runs shared retry/header cases and weather-service HTTP tests, and
compiles empty and adversarially renamed models. The package itself compiles
against Java 8 APIs; the JVM test server requires JDK 17. The suite also compiles
a synthetic service with 100 records and 2,000 fields to detect JVM method-size limits.
Over real CIO HTTP it uploads (replayable and unsized one-shot) and downloads
512 MiB streaming bodies with a 256 MiB heap, checking their SHA-256, and
verifies streamed response checksums.

`python tools/test-kotlin-multiplatform.py` publishes generated JVM/JS artifacts
and metadata into an isolated Maven repository, then compiles a separate
Multiplatform consumer against those artifacts. Its common tests verify exact
64-bit numbers, negative nanosecond timestamps, presence, URI escaping, body
digests, cold pagination, cancellation, injected transport ownership, exact
arbitrary-precision collections, payloads, authentication, compiled waiters,
endpoint rules, and streaming blobs on JVM and Node. The streaming tests cover
one-shot and replayable retry rules, `@requiresLength` and modeled bounds, 32 MiB
transfers whose in-flight bytes stay below 8 MiB, cancellation and deadlines
releasing request and response sources, unread bodies, and streamed checksums for
all seven algorithms. A client generated from `runtime-policy.json` runs every
shared runtime-policy vector (retry quota steps and calls, CUBIC, rate measurement
and send tokens, endpoint discovery scenarios, telemetry events and redaction),
plus one discovery shared by eight concurrent calls whose leader is cancelled, a
shared discovery failure, a throwing hook, and cancellation telemetry.
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
