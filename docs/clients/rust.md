# Rust clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates native Rust models, one error enum per operation, an async
service-named client (`WeatherClient`, `LeylineClient`), and lazy paginators.
The generated package uses `crucible-client-runtime-rust` and
the shared `crucible-wire` runtime. Use the current stable Rust toolchain
and an async runtime such as Tokio.

## API audience

Clients omit `@internal` API by default. Select `--audience internal` for a full
client, or `.audience(crucible_build::Audience::Internal)` on the Cargo build
helper. Rust `model(...).client()`
helpers accept the same setter. Servers always retain the complete contract.
See [API audience selection](../build-outputs.md#selecting-an-api-audience).

## Generating a client

For shared models, named SDKs, and application packages, follow
[building named SDK packages](../build-outputs.md). It supports Rust and Java
outputs from one compilation. Servers generate their bindings in their own
crate; Rust SDK packages expose a reusable client library. The named Rust
artifact contains `Cargo.toml` and `src/lib.rs`, with its matching runtime
dependency declared automatically. The SDK re-exports `Config`, `Error`, `Token`,
and other runtime APIs. Consumer crates depend on that package normally, with no
build script or inclusion macro. The single-crate options below
remain supported for generating directly inside an existing crate.

`crucible generate rust --client` writes the same package, and takes the
common package metadata keys as flags: `--description`, `--repository`,
`--homepage`, `--documentation`, `--license` or `--license-file <path>`,
`--author` and `--keyword` (repeat them), `--readme <path>` (copied in as
`README.md` and used as the crate's documentation) and `--private`
(`publish = false`). Each package carries `crucible.json` (schema 2) and
`crucible-model.json` at its root; see [package provenance](../sdk-releases.md#package-provenance).
To declare the SDK once, use [`crucible-sdk.toml`](../sdk-releases.md#declare-sdks-crucible-sdktoml).

Create a Rust client library with a `.cru` model and runnable example:

```sh
crucible new hello-client --client
cd hello-client
cargo build
```

The starter pins `crucible-client-runtime-rust` and `crucible-build` to the CLI's matching
release. Its complete build script is:

```rust
fn main() {
    crucible_build::model("model").client()
        .policy("crucible.toml")
        .generate();
}
```

`src/lib.rs` exports the generated API with
`crucible_client_runtime_rust::include_client!()`. Replace `model/hello.cru` with your
service's sources; Cargo compiles the model and regenerates the client on model
edits. Generated files stay under `OUT_DIR`.
Client generation does not require the server's rejection-error declarations.
The starter also checks naming, path and documentation conventions through its
`crucible.toml`; see [API policy and compatibility](../api-policy.md).

Run a Hello service created with `crucible new hello-service`, then call it:

```sh
cargo run --example hello -- http://127.0.0.1:8080
```

The example prints `Hello, World!`. The same `--crucible-rev` and `--runtime-path`
options work for service and client starters. See the
[quick start](../quickstart.md#versioning) for revision pins.

### Sharing a model package

When several packages share a contract, compile and publish it once from a
model package. Set `links = "weather_model"` in that package's manifest,
depend on `crucible-build` in `[build-dependencies]`, and use:

```rust
// weather-model/build.rs
fn main() {
    crucible_build::model("model").publish();
}
```

This replaces hand-written compiler configuration and publication wiring. See
[model packages](../framework.md#model-packages) for the Cargo metadata contract.
A sibling client consumes the published model as follows:

```toml
# weather-client/Cargo.toml
[package]
name = "weather-client"
version = "0.1.0"
edition = "2024"

[dependencies]
crucible-client-runtime-rust = { git = "https://github.com/Operation-Leyline/crucible" }
weather-model = { path = "../weather-model" }

[build-dependencies]
crucible-build = { git = "https://github.com/Operation-Leyline/crucible" }
```

Pin both Crucible dependencies to the same revision when distributing a client.
The repository's [weather client](../../e2e/weather-client/) is a complete local
example using path dependencies.

```rust
// build.rs
fn main() {
    crucible_build::model("weather_model").client().generate();
}
```

```rust
// src/lib.rs
crucible_client_runtime_rust::include_client!();
```

To generate a complete Cargo SDK directly:

```sh
cargo run -p crucible-cli -- generate rust \
  tests/models/weather \
  --client --package-name weather-client --out target/weather-client
```

`generate rust --client` writes a complete Cargo package. Consumers import
`weather_client::{Config, WeatherClient}` through an ordinary Cargo dependency.
Use `--package-version` to set its version. `--runtime-path` selects an absolute
local runtime crate for development; the default pins the generator's matching
Crucible Git revision. `new --client` creates a crate that owns both the model
and generation. Generation does not publish either package.

### Migrating the runtime crate name

The Rust runtime crate is now `crucible-client-runtime-rust`, previously
`crucible-client`. Rename the dependency in `Cargo.toml` and change Rust imports
and macro calls from `crucible_client::` to `crucible_client_runtime_rust::`.
For path dependencies, use `crates/crucible-client-runtime-rust`.

Update the runtime and generator to the same revision, regenerate checked-in
Rust clients and service CLI packages, and refresh Cargo lockfiles. Cargo build
scripts regenerate clients under `OUT_DIR` automatically. Runtime APIs and
behavior are unchanged.

### Migrating flat SDK imports

Generated models and modeled exceptions now live beside the service client.
Replace `weather_sdk::model::GetCityInput` with `weather_sdk::GetCityInput`,
and `weather_sdk::errors::NotFoundError` with `weather_sdk::NotFoundError`.
The implementation modules are private. This applies both to complete Cargo
SDK packages and to APIs included with `include_client!()`.

Regenerate the SDK and update consumer imports together. Generated CLI packages
must also be regenerated. Model names that collide with operation markers,
operation error enums, or the service client produce a generation diagnostic;
use a service `rename` entry to resolve the conflict.
Shapes from different namespaces, or names differing only by case, must also be
renamed apart, and members whose Rust names collide after case conversion
(`fooBar` and `foo_bar`) are diagnosed against the member.

In complete SDK packages, modeled names take precedence over runtime shortcuts.
For example, when the model defines `Config`, use `weather_sdk::runtime::Config`
for runtime configuration. The full runtime remains available as `runtime`.

## Calling operations

The example below uses the generated weather client and needs Tokio's `macros`
and `rt-multi-thread` features in the application:

```rust
use weather_client::{Config, Token};
use weather_client::{GetCityInput, ListCitiesInput, WeatherClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = WeatherClient::new(
        Config::new("http://127.0.0.1:3000")
            .token(Token::fixed(std::env::var("WEATHER_TOKEN")?)),
    );
    let result = client.get_city(GetCityInput::builder().city_id("oslo").build()).await?;
    println!("{}", result.city.name);

    let mut pages = client.list_cities_pages(ListCitiesInput::builder().max_results(50).build());
    while let Some(page) = pages.next_page().await {
        println!("{} cities", page?.cities.len());
    }
    Ok(())
}
```

`WeatherClient` is cheap to clone and shares its transport. Operation methods take
owned inputs. Missing idempotency tokens are filled in that input for one call
and reused across attempts; supply a stable token yourself for separate replays.
`Token::provider` accepts an async function returning `Option<String>` and runs
on every attempt, so refreshed credentials are picked up.

## Models and errors

Structures use public snake_case fields. `long` is `i64`, `integer` is
`i32`, document values use `serde_json::Value` (the runtime's version,
named `weather_client::runtime::Document` without a `serde_json`
dependency), and blobs use `weather_client::Blob`. A JSON blob is base64; an `@httpPayload` blob is raw bytes.
Use `Model::builder()` for ordinary construction. Fluent member methods accept
native values and collections; string methods also accept `&str`. Unspecified
optional members stay absent. `build()` returns the model directly when all
nonoptional members have modeled defaults; otherwise it returns
`Result<Model, BuildError>` and reports a missing required member. It never
invents values for required members. Nested models have the same builders.

Every generated model, enum, union and error enum is `#[non_exhaustive]`.
From another crate, build a model with its builder rather than a struct
literal, destructure it with `..`, and give a `match` on an enum, union or
error enum a `_` arm. In exchange, a member the model adds after the existing
ones, a new enum value, union arm or operation error is not a source change
for code that depends on the SDK package. Code that includes the bindings with
`include_client!()` defines these types in its own crate, where the attribute
has no effect: use the builders there too, or a regenerated model with a new
member breaks your struct literals.

Generated member getters borrow native values: `result.request_id()` returns
`Option<&String>`, and `result.city()` returns `&CitySummary`. Optional getters
return `None` for absent and null. Public fields remain available to inspect or
set those states explicitly. Builder setters use the modeled snake_case name;
reserved method names receive a `member_` prefix and a numeric suffix if needed.
Builder type names also receive a suffix when a model uses the same name.

Members that clients may omit (neither required nor defaulted, the same
rule the server applies) use `weather_client::Field<T>`. `Field::Absent` omits the
member, `Field::Null` writes JSON null, and `Field::Value(value)` writes a value.
Defaults apply only when decoding an absent member; explicit null stays null
for an optional member. Query and header bindings omit absent and null values.
A structure, union or document `@httpPayload` follows the protocol: Crucible
JSON sends `Field::Null` as the body `null`, and a `null` payload reads back as
`Field::Null` (a document too, as it would in a JSON body).
Supply all required labels and the values the operation needs before sending.

Timestamps are `chrono::DateTime<Utc>`. RFC 3339 date-time preserves supported
subsecond precision; HTTP-date has second precision. Epoch seconds preserve all
nine nanosecond digits, including negative values and exponent notation, without
floating-point conversion. Out-of-range and subnanosecond values are rejected.
Named string enums
retain unmodeled strings in `UnknownValue(String)`, including enum map keys.
They implement `Clone`, but no longer `Copy`; `as_str()` borrows from the enum.
Unions retain an unmodeled arm in
`UnknownValue(weather_client::UnknownUnion { name, value })`, where `value` is
`serde_json::Value`. The generated variant receives a numeric suffix if its
name would collide with a modeled variant. A modeled arm named `unknown` remains
a normal modeled arm. Malformed known arms still fail decoding, and an unknown
arm cannot be serialized under a declared arm's name.

These representations apply to generated clients. Rust service models keep
their server nullability, `Option<T>` fields, and closed enums and unions.

Each method returns `Result<Output, Error<OperationError>>`. A modeled exception
is `Error::Service(ServiceError { error, metadata })`; `error` is the generated
operation error enum. `Error::metadata()` provides the same `ResponseMetadata`
for modeled and unknown service errors: HTTP status, request ID, and headers.
Unknown errors also retain their type name, message, and retry classification.
Other variants are `Unknown`, `Transport`, `Invalid`, `Decode`, `Timeout`,
and `Cancelled`.

The generated `WeatherException` enum contains every modeled service error.
Each operation error enum converts into it with `From`, as does each individual
modeled exception. Use it when several operations need one error type:

```rust
use weather_client::Error;
use weather_client::{GetCityError, WeatherException};

fn service_error(error: Error<GetCityError>) -> Error<WeatherException> {
    error.map_service(WeatherException::from)
}
```

The conversion preserves the response status, request ID, headers, unknown
errors and retry classification. Transport and other local failures retain
their existing variants in the outer `Error` enum.

When upgrading, regenerate clients and change patterns such as
`Error::Service(CreateCityError::ConflictError(error))` to
`Error::Service(ServiceError { error: CreateCityError::ConflictError(error), .. })`.
Read unknown status and request ID through `error.metadata` or the shared accessor.

## Transport and retries

Retries follow the [shared contract](../clients.md#http-retry-contract).
`Config::retry(Retry { ... })` controls attempts and backoff; `Retry::none()`
disables retries. Defaults are three attempts, 100ms base delay, and 20s maximum
backoff. A valid `Retry-After` takes precedence, accepting delta seconds or an
HTTP date. Invalid and overflowing values fall back to the configured backoff.
Modeled retry metadata remains available when an error's fields cannot decode.

Generated operation markers declare `Operation::AUTH_BEARER`. Regenerate clients
when updating this runtime so authenticated operations carry that declaration.
Handwritten operation implementations default to no bearer authentication;
set the constant to `true` when the operation requires it. Public operations
bypass the token provider.

The default client does not follow HTTP redirects. Use
`Config::http_client(reqwest::Client)` for connection pools, proxies,
redirect policy, and per-attempt timeout settings. `Config::timeout(Duration)`
sets one total budget covering credential lookup, all attempts, body reads, and
retry delays. There is no total-call timeout unless configured. Dropping a call
future still cancels its active work.

Generated operations also expose `*_with_options(input, CallOptions)` (omit
`input` for a Unit operation). `CallOptions.timeout` overrides the client budget;
`CallOptions.cancellation` accepts a clonable `CancellationToken`. Calling
`cancel()` ends all calls sharing that token with `Error::Cancelled`. Deadline
expiry returns `Error::Timeout`; neither condition retries. A generated helper
(`*_with_options`, `*_pages`) receives a deterministic numeric suffix if
its name collides with an operation.
An already-expired budget, including `Duration::ZERO`, returns `Error::Timeout`
before polling credential providers or dispatching a request. A zero timeout
does not disable the total-call deadline. Leave both the client budget and the
call override unset to run without a total-call deadline.

```rust
use std::time::Duration;
use weather_client::{CallOptions, CancellationToken};

let cancellation = CancellationToken::new();
let options = CallOptions {
    timeout: Some(Duration::from_secs(15)),
    cancellation: Some(cancellation.clone()),
};
let result = client.get_city_with_options(input, options).await;
```

`Config::header` supplies default headers. A default `Content-Type` never
replaces a JSON document's or a payload's media type; only a modeled
`Content-Type` member overrides a payload's. `Config::interceptor` receives a
mutable reqwest request after credentials and headers on every attempt, in
registration order. `Config::content_sha256(true)` adds `x-amz-content-sha256`
before these interceptors, allowing a signing interceptor to consume it. The
hash covers the buffered body, including zero bytes for empty POST/PUT/PATCH
requests. An interceptor that changes the body must recompute the digest after
its change. Other bodyless requests receive no generated digest.

## Qualification

Run `cargo test -p crucible-codegen-rust -p crucible-client-runtime-rust -p crucible-wire`,
then `bash tools/test-e2e.sh` for the generated client over the weather service.
The E2E client tests also drive generated clients for the shared
`error-payloads` fixture against loopback HTTP peers.
The real loopback retry runner consumes the shared retry fixture. The E2E
workspace is a separate Cargo workspace and uses a generated model package.

Local generation and native HTTP proof do not establish published-package or
authenticated Leyline compatibility. See [qualification levels](../clients.md#qualification).

## Updating the runtime

### Migrating client models

The public client type now derives from the service name: `Weather`
generates `WeatherClient`, and `Leyline` generates `LeylineClient`. Replace old
`Client` imports and constructors with that service-specific type. No generic
`Client` alias is generated. Runtime types such as `reqwest::Client` are unchanged.

Builders and getters are additive. Public fields and presence-aware code
continue to work. Struct literals from another crate do not since the models
became `#[non_exhaustive]`: replace
`GetCityInput { city_id: Field::Value(id), at: Field::Absent }` with
`GetCityInput::builder().city_id(id).build()`. Regenerate the package with its
matching runtime to expose these helpers; reverting that package and runtime
together restores the previous API.

#### Raw presence migration

Regenerate the entire client package and update `crucible-client-runtime-rust`,
`crucible-build`, and the generator to the same pinned revision. This changes
the generated Rust API without changing the wire encoding of existing values.
Consumers must update struct literals and matches:

| Previous client API | Current client API |
|---|---|
| Optional field `None` | `Field::Absent` |
| Optional field `Some(value)` | `Field::Value(value)` or `value.into()` |
| No explicit null representation | `Field::Null` |
| Read `Option<T>` | Use `value()` / `into_value()`, or match all three states |
| Exhaustive enum or union match | Handle the generated unknown variant |
| Copy an enum or retain its `&'static str` | Clone or borrow the enum; copy the string if needed |

`Field::from(Option<T>)` maps `None` to `Absent` and `Some(value)` to `Value(value)`.
Use it when adapting an existing optional value; it cannot infer explicit null.
For example, with the generated weather models:

```rust
use weather_client::Field;
use weather_client::{ListCitiesInput, Sky};

let previous_limit: Option<i32> = Some(50);
let mut input = ListCitiesInput::builder().build();
input.max_results = Field::from(previous_limit);
input.tags = Field::Null;
assert_eq!(input.max_results.value(), Some(&50));
assert!(input.next_token.is_absent());
assert!(input.tags.is_null());

let future_sky = Sky::UnknownValue("hazy".into());
assert_eq!(future_sky.as_str(), "hazy");
```

Keep the previous generator revision, lockfile, generated package, and consumer
revision together as the rollback unit. Before adopting the new package, compile
its consumers, run the shared serialization fixtures and weather HTTP tests,
and check the model revision used for generation. Roll back all four together;
new generated clients require the matching runtime. Service packages do not
need a presence-model migration.

### Retry metadata

`ErrorInfo` includes `retryable`, preserving non-throttling `@retryable` metadata.
Regenerate Rust clients and services with the matching runtime. Handwritten
operation descriptions must supply this field. Unmodeled 501 and other unknown
5xx responses are not automatically retried. Roll back the generator, runtime,
and generated code together if reverting this change.

### Greedy path labels

Generated clients support greedy path labels, which preserve slash separators
and reject dot-only segments before dispatch. Nested pagination token paths
continue to use the public model names.

### Exact numbers and collections

`bigInteger` and `bigDecimal` use `BigInteger` and `BigDecimal`. Parse decimal
text with `str::parse`; both serialize as JSON numbers without floating-point
conversion. Their public inner values and `Deref` expose the native `num_bigint`
and `bigdecimal` APIs. Numeric defaults and range validation retain the same
precision, including HTTP headers. Integer enums provide `as_i32()` and client
`UnknownValue(i32)` variants for values introduced by newer servers.

`unique` lists remain `Vec<T>`;
generated member codecs reject duplicates during encoding and decoding using
structural value equality. Object member order and equivalent numeric spellings
do not distinguish elements. Dense document collections reject null elements but preserve null nested inside documents.
Nested timestamp collections use their modeled timestamp formats.

### Payloads and media-type headers

An `@httpPayload` member is the whole HTTP body. A blob is raw bytes. A string
or enum is UTF-8 text; an empty body reads as an empty string. A structure,
union, or document is one JSON value; an absent value sends no body, and an
empty response body reads as absent. An explicit null follows the protocol:
Crucible JSON sends the body `null`, and AWS REST JSON omits a null structure
or union. A `null` response payload reads as `Field::Null`. Each payload is sent under
its target's `@mediaType`, or the protocol default. Modeled errors may also
bind a payload: the error is named by the `x-crucible-error` header
(`X-Amzn-Errortype` for AWS protocols) because the body is no longer a JSON
envelope, and header members still decode beside it.

Header strings whose target has `@mediaType`, including list elements,
travel base64-encoded. The public model holds the decoded text. A response
header that is not base64 of UTF-8 text is `Error::Decode`.
