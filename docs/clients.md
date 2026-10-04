# Client coverage and conformance

Crucible provides Leyline's generated SDKs. Broad language coverage is a
product requirement. Delivery waves order implementation; they do not make
support conditional on finding an individual consumer.

| Wave | Targets | Status |
|---|---|---|
| Baseline | TypeScript/JavaScript, Rust | Shared retry conformance runs in CI |
| 1 | Python, Go, Java, C# | Implemented; native package and HTTP gates run in CI |
| 2 | Kotlin, Swift, Dart, PHP, Ruby | Kotlin, Swift, Dart, and PHP implemented; Ruby planned |
| Extension | C++, C, R | API and packaging scope still to be defined |

## Architecture

Every emitter reads `crucible-smithy::ir::Service`. Smithy semantics belong
in lowering, language syntax and naming in the emitter, and transport behavior
in a small native runtime. New targets initially bundle their runtime with
their generated package. Public APIs follow the language's conventions.

A Smithy `blob` is `crucible_client::Blob` (bytes) in Rust, `Uint8Array` in
TypeScript, `bytes` in Python, `[]byte` in Go and `byte[]` in Java. In a JSON body it is standard, padded
base64. An `@httpPayload` blob is sent as the bytes themselves with the
blob's media type, and the body digest covers exactly those bytes; a payload
response is read as bytes, an empty body as empty bytes.

Leyline's endpoint, authentication configuration, exact-byte body digest,
and package identity are explicit package configuration. Framework code must
not branch on Leyline service or operation names. Database conveniences such
as waiting for jobs or carrying read consistency belong in a separately
specified helper layer; all modeled operations remain directly available.

## HTTP retry contract

An operation is safe to repeat when it is readonly, idempotent, or carries an
idempotency token. A generated token is filled once per call and reused for
every attempt.

| Failure | Retry eligibility |
|---|---|
| Modeled throttling error | Eligible even when the operation is not otherwise safe |
| Explicitly retryable, non-throttling modeled error | Only safe operations |
| HTTP 500, 502, 503, 504 | Only safe operations; use the actual response status |
| Other HTTP status without retry metadata | Ineligible |
| Transport failure | Only safe operations |
| Invalid input, malformed successful JSON, caller cancellation | Ineligible |

All eligible retries remain bounded by the configured attempt limit, which
includes the first request. An HTTP 429 alone does not imply modeled
throttling. An explicit unknown error name is preserved and never reclassified
by status. With no name, a status may identify a single declared error.
An HTTP 402 alone never triggers a retry. HTTP 501 and unrecognized
5xx responses require explicit model retryability.

[`tests/conformance/retry.json`](../tests/conformance/retry.json) is the shared
HTTP decision table. Each case scripts a number of failing responses followed
by success and specifies total attempts and final success. Error names select
ordinary, explicitly transient, or throttling model metadata. The ordinary
error is modeled as 400 even in the case whose response is 503: this catches
clients that replace the wire status with the model's default.

The Rust runner is `e2e/weather-service/tests/client_retry.rs`, using a real
loopback HTTP server. The TypeScript runner is
`crates/crucible-codegen-ts/tests/ts/retry-conformance.test.ts`, using the
generated package's runtime with fake fetch. Both run in the existing CI gates.
This table tests runtime decisions; Rust generator tests separately verify
that explicit retryability reaches generated metadata.

The Python runner in `crates/crucible-codegen-python/tests/test_runtime.py`
executes the same table for synchronous and asynchronous HTTPX clients.
`python tools/test-python.py` also type-checks the generated package, drives
the weather service over HTTP, builds a wheel, and installs it for an import
check. See [Python clients](python-client.md) for usage and limits.

The Go runner in `crates/crucible-codegen-go/tests/runtime_test.go` executes
the retry table over loopback HTTP. `python tools/test-go.py` runs it alongside
weather-service tests, `go vet`, and a separate-module consumer check. Python
and Go also share `tests/conformance/header-lists.json`.
See [Go clients](go-client.md) for the generated API.

`python tools/test-java.py` packages the Java SDK with Maven, then compiles a
separate consumer against the jar and runs shared cases and weather-service
HTTP tests. See [Java clients](java-client.md) for its API and lifecycle.

`python tools/test-csharp.py` builds a NuGet package, installs it in a fresh
consumer, and runs shared cases and weather-service HTTP tests. See
[C# clients](csharp-client.md) for its API, lifecycle, and precision limits.

`python tools/test-kotlin.py` installs a Maven jar and tests a separate coroutine
consumer. `python tools/test-kotlin-android.py` builds and runs an Android
consumer on an emulator. See [Kotlin clients](kotlin-client.md) for model APIs,
transport lifecycle, and platform limits.

`python tools/test-swift.py` builds a Swift package and separate consumer,
runs shared fixtures and native weather-service HTTP tests, and compiles empty
and renamed models. CI runs on Linux and macOS and builds for iOS Simulator.
See [Swift clients](swift-client.md) for its API and representation limits.

`python tools/test-dart.py` tests the native VM and JavaScript in Chrome, uses
the real weather service, and compiles a separate consumer to native code and
JavaScript. CI checks the minimum SDK and current stable SDK. See
[Dart clients](dart-client.md) for model types, browser requirements, and limits.

`python tools/test-php.py` runs exact codecs, shared fixtures, and native HTTP
tests, installs a Composer archive into a separate consumer, and loads empty
and renamed packages. CI checks PHP 8.3 and 8.5. See
[PHP clients](php-client.md) for synchronous calls, model types, and limits.

## Remaining acceptance work

The retry table is the first shared suite, not full client qualification.
Extend common fixtures to cover binding and serialization, absent/null/default
values, numeric and timestamp precision, unknown values and errors, body
digests, token reuse, authentication refresh, retry delays, body interruptions,
cancellation, and pagination. Existing language-specific tests remain useful
until equivalent shared cases exist.

Each supported target must generate and compile the complete Leyline model,
pass shared fixtures and weather-service HTTP tests, build an installable
native package, and exercise authenticated Leyline workflows through the public
API path. Record these proof levels separately. Local tests do not establish
published-package or deployed-service compatibility.

Package provenance includes the generator version, service version, and model
digest. Minimum language/runtime versions are chosen and tested per target.
Distribution visibility is independent of packaging. New accepted Smithy
constructs must be implemented or explicitly diagnosed in every supported
target; no emitter silently drops them.

## Rust retry metadata migration

`ErrorInfo` now includes `retryable`, preserving the model's non-throttling
`@retryable` annotation. Regenerate Rust clients and services together with the
updated runtime. Handwritten descriptions must supply the new field. Rust
also stops automatically retrying unmodeled 501 and unrecognized 5xx responses,
matching TypeScript's fallback policy. To roll back, restore generator, runtime,
and generated code together to the preceding version.
