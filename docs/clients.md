# Client setup and conformance

[Repository overview](../README.md)

Crucible provides native clients for Leyline's language portfolio. Every emitter
reads the same lowered Smithy IR; the language guide explains its public API
and representation limits.

## Language guides

| Target | Guide | Package / qualification command |
|---|---|---|
| `rust` | [Rust](clients/rust.md) | Cargo; `bash tools/test-e2e.sh` |
| `ts` | [TypeScript and JavaScript](clients/typescript.md) | npm; `bash tools/test-ts.sh` |
| `python` | [Python](clients/python.md) | wheel; `python tools/test-python.py` |
| `go` | [Go](clients/go.md) | Go module; `python tools/test-go.py` |
| `java` | [Java](clients/java.md) | Maven; `python tools/test-java.py` |
| `csharp` | [C#](clients/csharp.md) | NuGet; `python tools/test-csharp.py` |
| `kotlin` | [Kotlin](clients/kotlin.md) | Maven / Android; `python tools/test-kotlin.py` |
| `swift` | [Swift](clients/swift.md) | SwiftPM; `python tools/test-swift.py` |
| `dart` | [Dart](clients/dart.md) | Dart package; `python tools/test-dart.py` |
| `php` | [PHP](clients/php.md) | Composer; `python tools/test-php.py` |
| `ruby` | [Ruby](clients/ruby.md) | gem; `python tools/test-ruby.py` |
| `cpp` | [C++](clients/cpp.md) | CMake; `python tools/test-cpp.py` |
| `c` | [C](clients/c.md) | CMake; `python tools/test-c.py` |
| `r` | [R](clients/r.md) | R source package; `python tools/test-r.py` |

Run repository commands from its root. Install the target's prerequisites as
listed in its guide. The CLI can also be installed from this checkout with
`cargo install --path crates/crucible-cli --locked`; examples using `crucible`
assume that executable is on `PATH`.

## Preparing a model

Use the `crucible.protocols#json` service trait and bearer authentication where
needed. [The protocol definition](../smithy/crucible.smithy) is distributed in
the `crucible-traits` jar on `traits-v<version>` releases. For a Gradle Smithy
build, configure the artifact repository and dependency:

```kotlin
repositories {
    ivy {
        url = uri("https://github.com/Operation-Leyline/crucible/releases/download")
        patternLayout { artifact("traits-v[revision]/[module]-[revision].[ext]") }
        metadataSources { artifact() }
    }
}
dependencies {
    implementation("crucible:crucible-traits:0.2.0")
    smithyBuild("crucible:crucible-traits:0.2.0")
}
```

Add this projection to `smithy-build.json`, then build the Smithy project:

```json
{
  "version": "1.0",
  "projections": {
    "crucible": {
      "transforms": [{ "name": "flattenAndRemoveMixins" }]
    }
  }
}
```

Pass the resulting JSON AST to Crucible. Select a service explicitly when the
model contains more than one:

```sh
cargo run -p crucible-cli -- check model.json --service example.weather#Weather
cargo run -p crucible-cli -- generate ts model.json \
  --service example.weather#Weather --out generated/weather \
  --package-name @example/weather --endpoint https://api.example.com
```

For a ready model in this checkout, use
[`crates/crucible-smithy/tests/fixtures/weather.json`](../crates/crucible-smithy/tests/fixtures/weather.json).
Each target has `generate <target> --help`. Target identity, endpoint, and body
digest configuration are explicit; no runtime checks a Leyline operation name.

Generation replaces the target directory. Rust's CLI target writes one file;
its normal package integration uses `build.rs`. Pin the generator and runtime
versions, preserve package provenance, and regenerate instead of editing output.
A lowered-schema digest identifies the emitted wire description, not the original
Smithy source tree. Provenance fields differ by target; see its package output.

## Shared model and wire behavior

Lowering resolves nullability, defaults, effective constraints, HTTP bindings,
errors, pagination, and timestamp formats once. Emitters handle native syntax
and naming. Unsupported constructs produce diagnostics rather than disappearing
from generated code. See [design and protocol](design.md) for ownership and wire rules.

A `blob` is bytes in the native language, standard padded base64 in JSON, and raw
bytes when used as an `@httpPayload`. Body digests must cover the final bytes sent.
Timestamp and integer precision depend on the native representation: consult the
language guide before using nanosecond timestamps or the entire signed 64-bit range.
Absent, null, unknown enum, and unknown union handling also vary by target.

All modeled operations remain directly available. Database helpers such as
waiting for a job or carrying a read-consistency generation belong above the
client runtime.

## HTTP retry contract

An operation is safe to repeat when it is readonly, idempotent, or carries an
idempotency token. A generated token is filled once per call and reused for
every attempt. Keep an explicit token when replaying a logical write across
separate calls; caller-input mutation differs by language.

| Failure | Retry eligibility |
|---|---|
| Modeled throttling error | Eligible even when the operation is not otherwise safe |
| Explicitly retryable, non-throttling modeled error | Only safe operations |
| HTTP 500, 502, 503, 504 | Only safe operations; use the actual response status |
| Other HTTP status without retry metadata | Ineligible |
| Transport failure | Only safe operations |
| Invalid input, malformed successful JSON, caller cancellation | Ineligible |

Eligible retries are bounded by the configured attempt limit, including the
first request. HTTP 429 alone does not establish modeled throttling. An explicit
unknown error name is preserved and never reclassified by status. Without a name,
a status can identify exactly one declared error. HTTP 402 alone never retries;
501 and other unrecognized 5xx statuses need explicit model retryability.

Backoff, `Retry-After`, deadlines, cancellation, middleware ordering, and transport
ownership are documented per language. A per-attempt transport timeout does not
establish an overall call deadline.

## Qualification

The commands above exercise generated code and its native runtime. Shared fixtures
include [retry decisions](../tests/conformance/retry.json),
[header lists](../tests/conformance/header-lists.json),
[malformed errors](../tests/conformance/malformed-errors.json), and
[serialization](../tests/conformance/serialization.json). Serialization cases
record native precision profiles; unsupported baseline cases must remain named
coverage gaps, not be counted as passing. The Rust and TypeScript guides explain
those representation and validation limits. Language-specific cases additionally
cover bindings, payloads, digests, authentication refresh, body interruptions,
cancellation, and pagination. The weather service supplies real loopback HTTP.

The serialization adapters cover the current 15-case fixture as follows. Counts
include each target's explicit precision profile; they do not imply identical
timestamp or numeric representations.

| Targets | Cases exercised | Explicit gaps |
|---|---|---|
| Python, Go, Java, C#, Kotlin, Swift, Dart, PHP, Ruby, C++, C, R | 15 / 15 | See each guide for native precision limits |
| Rust | 11 / 15 | Explicit optional null; open enums; unknown union/document round trips; a map with an unknown enum key |
| TypeScript / JavaScript | 10 / 15 | Missing/null required members; null-default validation; signed-64 overflow and fractional-integer rejection |

Baseline exclusions remain visible in the test runners. See the
[Rust](clients/rust.md#models-and-errors) and
[TypeScript](clients/typescript.md#models-and-errors) guides before depending on
these semantics. Shared fixtures are a bounded contract sample, not exhaustive
coverage of every operation or transport behavior.

Record these proof levels separately: generation; compilation and model loading;
shared fixtures and native HTTP; package construction and independent consumption;
authenticated deployed-service workflows; publication. Passing an earlier level
does not prove a later one. Platform and minimum-version gates are in
[CI](../.github/workflows/ci.yml) and each language guide.

The private Leyline model is not checked into this repository. Qualification must
record its source revision, source manifest and model hashes, selected public
service, protocol adaptation, and operation coverage. Refresh from current source
before claiming complete coverage; an ignored model left by an earlier build can
be stale. Leyline's current restJson1 deployment and Crucible's JSON protocol need
explicit compatibility checks. Neither a protocol-trait substitution nor a local
package build establishes authenticated deployment compatibility.
