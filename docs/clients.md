# Client setup and conformance

[Repository overview](../README.md)

Crucible provides native clients for a comprehensive language portfolio. Every emitter
reads the same lowered IR; the language guide explains its public API
and representation limits.

## API audiences

Generated clients default to the external audience and omit `@internal`
API. Use `--audience internal` on `crucible generate`, or
`.audience(crucible_build::Audience::Internal)` on a named output, to include the
full contract. See [selecting an API audience](build-outputs.md#selecting-an-api-audience)
for diagnostics and migration instructions.

A sender-signed operation (`@sender`, a webhook receiver a third party
calls and signs) is in no client, whatever the audience, nor are the types
only it reaches ([decision 81](idl/decisions.md)).

## Language guides

For standalone command-line tools, see [generating a service CLI](clients/cli.md).
For several named SDK variants or separate model and SDK modules, see
[building named SDK packages](build-outputs.md).

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

Run repository qualification commands from its root. Install the target's
prerequisites as listed in its guide. Install the CLI once from your pinned
Crucible checkout with `cargo install --path crates/crucible-cli --locked`.
Examples using `crucible` assume that executable is on `PATH`.

## Package metadata, provenance and declared SDKs

A service declares the SDKs it releases once, in `crucible-sdk.toml`, and
generates them with `crucible sdk generate`; `crucible sdk config` prints
what each one resolves to. Rust and TypeScript packages take the common
metadata keys (description, URLs, license, authors, README, keywords,
`private`) from that file, from `crucible generate` flags, or from the
`crucible_build` helpers, and carry `crucible.json` (schema 2) and
`crucible-model.json`. See [releasing generated SDKs](sdk-releases.md) and
the design in [SDK packaging](sdk-packaging.md).

## Generated package documentation

Every generated SDK package, in all 14 languages, carries two generated
Markdown files at its root (R's `AGENTS.md` is under `inst/`, see the
[R guide](clients/r.md)), both owned by generation like the code beside
them. npm, PyPI, RubyGems, Composer, NuGet, pub, CRAN, Cargo and CMake
packages ship them; Java and Kotlin keep them beside `pom.xml` and
`build.gradle.kts`, outside the jar.

- `README.md`, for a person adding the package: the install line with the
  exact package name and version, constructing the client with its token
  (and endpoint, where none is generated in), one complete call, a
  paginated call, how modelled errors are told apart, the retry and
  idempotency rules, and links (the model's `@externalDocs`, the package's
  documentation, home page and repository, and the language guide). The
  install line follows the release: a private package, or one released
  by Git tag, names its tag or says it is private instead of an install
  from the public registry, which could fetch someone else's package of
  that name; a scoped npm package on another registry configures its
  scope's registry first. A Rust package's README is also its crate
  documentation.
- `AGENTS.md`, for a coding agent working in a project that depends on the
  package: the same facts as terse rules, with the exact install line,
  imports, client name and construction, the headers the client sends
  itself (`Authorization`, `Crucible-Version`, default middleware such as
  `content-sha256`'s `x-amz-content-sha256`), pagination, errors, retries,
  which calls fill an idempotency token, and one complete call. Agents
  read it whenever they work beside the package, so they need not guess a
  package or method name.

A configured `readme` or `agents` (in `crucible-sdk.toml`, `--readme` and
`--agents`, or the build helpers' `.readme()` and `.agents()`) replaces the
generated file, in every language. Model text in either file (the
service's title, `@externalDocs` names and URLs, versions) is escaped
Markdown on one line, and links are `http`, `https` or `mailto` with
`<`, `>`, parentheses and spaces percent-encoded, so a model cannot add a
heading, a paragraph, a link or an HTML comment to what an agent reads as
instructions.

The calls in both are the `crucible.sdks/1` snippets
([docs.md](docs.md#snippets)), compile-checked by each language's
qualification script. Packages also carry their documentation tool's
configuration: `typedoc.json` (TypeScript), `[package.metadata.docs.rs]`
(Rust), a `Doxyfile` (C and C++), `dartdoc_options.yaml` (Dart), a DocC
catalog (Swift), `.yardopts` (Ruby) and `man/*.Rd` (R).

A directory you generate into that already holds a handwritten
`README.md` or `AGENTS.md` is refused, as for any file generation does
not own: declare it as the SDK's `readme` or `agents`, or move it out of
the output directory.

## API documentation and debug formatting

The TypeScript, Python and Go clients preserve documentation on models,
members, enum values and operations. Their client and runtime documentation also
describes configuration, transport ownership, retries and pagination.
TypeScript retains CommonMark in escaped documentation comments. Python docstrings
and Go doc comments render CommonMark as readable text, preserving links and code.

Python model `repr` and Go model `String`/`GoString` describe nested members and
collections. TypeScript models are plain interfaces and aliases; error classes
provide `toString` and Node.js inspection. These formatters omit absent members,
display explicit nulls, and replace sensitive members and sensitive targets with
`*** Sensitive Data Redacted ***`. Unknown union payloads are also redacted.
Native scalar and collection aliases
retain their language's own formatting when used outside a modeled container.

Formatting does not change equality or hashing:

| Language | Existing equality and hashing |
|---|---|
| TypeScript | Objects and errors use identity equality. Scalars use JavaScript equality; `Decimal.equals` compares decimal values. No generated value hash. |
| Python | Model dataclasses use field equality and are unhashable. Errors use identity equality/hash. Enums and native aliases retain native equality/hash; mutable collections are unhashable. |
| Go | Scalars and enums use native equality. Structs are comparable only when all fields are comparable; pointers compare identity. Slices/maps have no generated value equality/hash. `Decimal.Equal` compares numeric values. |

## Catching service errors

Generated modeled exceptions share a service-named base, such as
`WeatherException`. Class-based SDKs retain `ServiceError` above that base,
so existing generic catches still work. Unknown errors from a newer server
and malformed modeled error responses also use the service base and retain
their HTTP metadata and retry classification. Transport failures remain
separate from service responses.

Swift provides a `WeatherException` protocol and Go an interface usable with
`errors.As`. R conditions include `WeatherException` in their class list.
Rust provides a service enum and `Error::map_service` to unify operation error
enums while preserving unknown errors and transport failures. C uses the
existing service-prefixed error structure and kind discriminator. Regenerate
clients to get these public types; the model and wire format do not change.

## Checking target support

Extended language support is tracked per generator. Inspect the current build and
validate a model for its intended language before generation:

```sh
crucible capabilities
crucible check model --target python
```

Each emitter also checks capabilities when called as a library. A feature accepted
by another target never silently changes this target's wire behavior. Crucible JSON
is the only protocol; a model declaring another is refused.

See [SDK releases](sdk-releases.md) for generation, native package builds,
installed-consumer checks, artifact manifests, and publication recipes.

## Preparing a model

Pass a Crucible language package, a directory whose `crucible.toml` has a
`[model]` table beside its `.cru` sources, directly to `check` or `generate`.
Run source-generation commands from your service's model project. For example,
when your service model lives under `model/`:

```sh
crucible check model --target ts
crucible generate ts model --out generated/weather \
  --package-name @example/weather --endpoint https://api.example.com
```

The language guides use Weather package names and call examples. Supply your
Weather service sources or adapt those names and calls to your own service. The
Crucible repository's `tests/models/weather` package is a ready Weather model.

The CLI compiles the package itself; it needs no Java installation, build
file or model-publishing Cargo crate. `--service` selects one service when
your sources define several. `crucible compile --json model` prints the
compiled model for tools that read the contract.

Non-Rust client generators emit native package metadata and a bundled runtime. The
language guide's npm, pip, Go, Maven, or other native commands build and consume
that package. To regenerate during a native build, call `crucible generate`
from a task or script before the native compile or package step. Alternatively,
generate named SDKs from Cargo and use `crucible export-artifact` to expose the native
package. See [building named SDK packages](build-outputs.md) for both model-owned
and separate SDK producers. Pin the CLI version in your toolchain or CI setup.

Rust clients also support automatic generation during Cargo builds through
[`crucible_build::model("model").client()`](clients/rust.md#generating-a-client).

Each target has `generate <target> --help`. Target identity and endpoint
configuration are explicit; no runtime checks a hardcoded operation name.

Native client generation tracks its generated files in `.crucible`. Regeneration
updates those files and removes obsolete generated files, preserving handwritten
files and native dependency/build directories. Keep that manifest; existing
files outside its ownership are never silently replaced. Rust's direct CLI
target writes one file; its normal package integration uses `build.rs`.
Pin the generator and runtime
versions, preserve package provenance, and regenerate instead of editing output.
A lowered-schema digest identifies the emitted wire description, not the original
`.cru` source tree. Provenance fields differ by target; see its package output.

## Shared model and wire behavior

Lowering resolves nullability, defaults, authentication, effective constraints, HTTP bindings,
errors, pagination, and timestamp formats once. Emitters handle native syntax
and naming. Unsupported constructs produce diagnostics rather than disappearing
from generated code. See [design and protocol](design.md) for ownership and wire rules.

A `blob` is bytes in the native language, standard padded base64 in JSON, and raw
bytes when used as an `@httpPayload`.
Timestamp and integer precision depend on the native representation: consult the
language guide before using nanosecond timestamps or the entire signed 64-bit range.
Absent, null, unknown enum, and unknown union handling also vary by target.

Operations inherit service authentication unless they are `public`.
A public operation does not invoke the configured bearer-token provider.
Regenerate clients when upgrading the generator and runtime together; older
generated metadata does not carry these operation-level authentication decisions.

A member's optionality is the same for clients as for the server: modeled
defaults apply when decoding an absent member.
An explicit null remains distinct from an absent value. Dense collections reject
null elements, except that a `document` element may itself contain JSON null.

Path labels equal to `.` or `..` are rejected before sending, since HTTP URL
normalization can change the resource they address. Targets advertising
`media-type-headers` encode media-type strings, including list elements, as
base64 in headers. Other targets diagnose that binding until their native
qualification passes. Media types on blob payloads remain supported.

All modeled operations remain directly available. Service-specific helpers
such as carrying a read-consistency generation belong above the client runtime.

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
ownership are documented per language. Retry quotas, adaptive rate limiting
and telemetry follow the [SDK runtime policy](sdk-runtime-policy.md). A per-attempt transport timeout does not
establish an overall call deadline.

## Public API conventions

Client types carry the service name: `WeatherClient` for the example service.
C uses its configured service prefix; R uses a service-specific constructor
following R naming conventions.

Each client uses its language's normal construction and access patterns. The
guides demonstrate ordinary values and collections without requiring presence
wrappers at every call site. Kotlin has request DSLs, Java and Rust have fluent
builders, Go and C provide native setters, and dynamic languages use named
arguments or object literals. C++, C#, Swift, and Dart expose native value
construction and access appropriate to their type systems.

The wire contract still distinguishes omission from explicit null. Convenience
getters may return the same nullable value for both; use the language's advanced
presence API when the distinction matters. Read the language guide before passing
null or nil: a nullable initializer default cannot always distinguish an omitted
argument from an explicit null. Native API examples are verified by independent
package consumers as well as the shared serialization fixtures.

### API documentation and string representations (Java, Kotlin, C#)

Generated Java, Kotlin and C# clients document every public type in the language's
doc format (Javadoc, KDoc, C# XML documentation). The model's doc comments
are converted from CommonMark once, in `crucible-codegen-common`: paragraphs, lists,
code, emphasis, links and headings carry over, and everything else, raw HTML
included, is escaped so it cannot close the comment or break the markup.
Crucible's own types (client, configuration, errors, credentials, the
bundled runtime) carry hand-written docs. The Java and C# packages build their
docs without warnings (`javadoc -Xdoclint:all`, `dotnet pack -warnaserror` with
`GenerateDocumentationFile`).

Generated structures, unions and errors have a readable `toString`/`ToString`:
`TypeName(member=value, ...)`, nested models and collections recursing, unset
optional members omitted and an explicit null printed as `null`. A member that is
`@sensitive`, targets a `@sensitive` shape, or is a collection of one, prints
`*** Sensitive Data Redacted ***` instead of its value; an unrecognized union
variant never prints its payload. Value equality exists only for enums and union
variants that are records or data classes; generated Java and C# structures,
unions and errors, and Kotlin structures, use reference equality.

## Qualification

The commands above exercise generated code and its native runtime. Shared fixtures
include [retry decisions](../tests/conformance/retry.json),
[header lists](../tests/conformance/header-lists.json),
[malformed errors](../tests/conformance/malformed-errors.json), and
[serialization](../tests/conformance/serialization.json). Serialization cases
record native precision profiles. Every target runs all 16 cases; the language
guides explain the remaining representation limits. Language-specific cases additionally
cover bindings, payloads, authentication refresh, body interruptions,
cancellation, and pagination. The weather service supplies real loopback HTTP.

The serialization adapters cover the current 16-case fixture as follows. Counts
include each target's explicit precision profile; they do not imply identical
timestamp or numeric representations.

| Targets | Cases exercised | Representation notes |
|---|---|---|
| Python, Go, Java, C#, Kotlin, Swift, Dart, PHP, Ruby, C++, C, R | 16 / 16 | See each guide for native precision limits |
| Rust | 16 / 16 | Epoch-seconds timestamps preserve nanosecond precision |
| TypeScript / JavaScript | 16 / 16 | Signed 64-bit fields use `bigint`; document integers beyond the safe range use `bigint` (other numbers use `number`), and dates have millisecond precision |

The [Rust model migration](clients/rust.md#migrating-client-models) and
[TypeScript guide](clients/typescript.md) describe the updated presence and numeric
APIs. Shared fixtures are a bounded contract sample, not exhaustive coverage of
every operation or transport behavior.

Record these proof levels separately: generation; compilation and model loading;
shared fixtures and native HTTP; package construction and independent consumption;
authenticated deployed-service workflows; publication. Passing an earlier level
does not prove a later one. Platform and minimum-version gates are in
[CI](../.github/workflows/ci.yml) and each language guide.

Private consumer models are not checked into this repository. Qualification of
external models must record source revision, source manifest and model hashes, selected public
service, protocol adaptation, and operation coverage. Refresh from current source
before claiming complete coverage; an ignored model left by an earlier build can
be stale. Neither a protocol-trait substitution nor a local
package build establishes authenticated deployment compatibility.
