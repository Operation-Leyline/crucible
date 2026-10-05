# Rust clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates native Rust models, one error enum per operation, an async
service-named client (`WeatherClient`, `LeylineClient`), and lazy paginators.
The generated package uses `crucible-client` and
the shared `crucible-wire` runtime. Use the current stable Rust toolchain
and an async runtime such as Tokio.

## Generating a client

For normal Cargo integration, depend on a model package that publishes its
Smithy AST. The [framework guide](../framework.md#model-packages) describes that
package. A sibling `weather-model` package with `links = "weather_model"` can
be consumed as follows:

```toml
# weather-client/Cargo.toml
[package]
name = "weather-client"
version = "0.1.0"
edition = "2024"

[dependencies]
crucible-client = { git = "https://github.com/Operation-Leyline/crucible" }
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
    crucible_build::client("weather_model").generate();
}
```

```rust
// src/lib.rs
crucible_client::include_client!();
```

Without a separate model package, call
`crucible_build::Codegen::client_from_model("model.json").generate()` in
`build.rs`. To inspect generated source directly:

```sh
cargo run -p crucible-cli -- generate rust \
  crates/crucible-smithy/tests/fixtures/weather.json \
  --client --out target/weather-client.rs
```

The CLI writes a Rust file, not a Cargo package. Package the client crate with
Cargo, including the model/build inputs its build script needs. Generation
does not publish it.

## Calling operations

The example below uses the generated weather client and needs Tokio's `macros`
and `rt-multi-thread` features in the application:

```rust
use crucible_client::{Config, Token};
use weather_client::{WeatherClient, model::{GetCityInput, ListCitiesInput}};

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

Structures use public snake_case fields. Smithy `long` is `i64`, `integer` is
`i32`, document values use `serde_json::Value`, and blobs use
`crucible_client::Blob`. A JSON blob is base64; an `@httpPayload` blob is raw bytes.
Use `Model::builder()` for ordinary construction. Fluent member methods accept
native values and collections; string methods also accept `&str`. Unspecified
optional members stay absent. `build()` returns the model directly when all
nonoptional members have modeled defaults; otherwise it returns
`Result<Model, BuildError>` and reports a missing required member. It never
invents values for required members. Nested models have the same builders.

Generated member getters borrow native values: `result.request_id()` returns
`Option<&String>`, and `result.city()` returns `&CitySummary`. Optional getters
return `None` for absent and null. Public fields remain available to inspect or
set those states explicitly. Builder setters use the modeled snake_case name;
reserved method names receive a `member_` prefix and a numeric suffix if needed.
Builder type names also receive a suffix when a model uses the same name.

Members that Smithy permits clients to omit use `crucible_client::Field<T>`.
This includes every member of an `@input` structure and members marked
`@clientOptional`, even when the server requires them. `Field::Absent` omits the
member, `Field::Null` writes JSON null, and `Field::Value(value)` writes a value.
Defaults apply only when decoding an absent member; explicit null stays null
for an optional member. Query and header bindings omit absent and null values.
Supply all required labels and the values the operation needs before sending.

Timestamps are `chrono::DateTime<Utc>`. RFC 3339 date-time preserves supported
subsecond precision; HTTP-date has second precision. Epoch seconds preserve all
nine nanosecond digits, including negative values and exponent notation, without
floating-point conversion. Out-of-range and subnanosecond values are rejected.
Named string enums
retain unmodeled strings in `UnknownValue(String)`, including enum map keys.
They implement `Clone`, but no longer `Copy`; `as_str()` borrows from the enum.
Unions retain an unmodeled arm in
`UnknownValue(crucible_client::UnknownUnion { name, value })`, where `value` is
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
Other variants are `Transport`, `Invalid`, `Decode`, `Timeout`, and `Cancelled`.

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

Use `Config::http_client(reqwest::Client)` for connection pools, proxies,
redirect policy, and per-attempt timeout settings. `Config::timeout(Duration)`
sets one total budget covering credential lookup, all attempts, body reads, and
retry delays. There is no total-call timeout unless configured. Dropping a call
future still cancels its active work.

Generated operations also expose `*_with_options(input, CallOptions)` (omit
`input` for a Unit operation). `CallOptions.timeout` overrides the client budget;
`CallOptions.cancellation` accepts a clonable `CancellationToken`. Calling
`cancel()` ends all calls sharing that token with `Error::Cancelled`. Deadline
expiry returns `Error::Timeout`; neither condition retries. A generated helper
receives a deterministic numeric suffix if its name collides with an operation.

```rust
use std::time::Duration;
use crucible_client::{CallOptions, CancellationToken};

let cancellation = CancellationToken::new();
let options = CallOptions {
    timeout: Some(Duration::from_secs(15)),
    cancellation: Some(cancellation.clone()),
};
let result = client.get_city_with_options(input, options).await;
```

`Config::header` supplies default headers. `Config::interceptor` receives a
mutable reqwest request after credentials and headers on every attempt, in
registration order. `Config::content_sha256(true)` adds `x-amz-content-sha256`
before these interceptors, allowing a signing interceptor to consume it. The
hash covers the buffered body, including zero bytes for empty POST/PUT/PATCH
requests. An interceptor that changes the body must recompute the digest after
its change. Other bodyless requests receive no generated digest.

## Qualification

Run `cargo test -p crucible-codegen-rust -p crucible-client -p crucible-wire`,
then `bash tools/test-e2e.sh` for the generated client over the weather service.
The E2E client tests also drive generated clients for the shared
`error-payloads`, `checksums` (with its vectors), and `protocol-json` fixtures
(once per AWS JSON protocol) against loopback HTTP servers, since the server
framework serves only `crucible.protocols#json`.
The real loopback retry runner consumes the shared retry fixture. The E2E
workspace is a separate Cargo workspace and uses a generated model package.

Local generation and native HTTP proof do not establish published-package or
authenticated Leyline compatibility. See [qualification levels](../clients.md#qualification).

## Updating the runtime

### Migrating client models

The public client type now derives from the Smithy service name: `Weather`
generates `WeatherClient`, and `Leyline` generates `LeylineClient`. Replace old
`Client` imports and constructors with that service-specific type. No generic
`Client` alias is generated. Runtime types such as `reqwest::Client` are unchanged.

Builders and getters are additive. Existing struct literals, public fields, and
presence-aware code continue to work. New ordinary calls can replace
`GetCityInput { city_id: Field::Value(id), at: Field::Absent }` with
`GetCityInput::builder().city_id(id).build()`. Regenerate the package with its
matching runtime to expose these helpers; reverting that package and runtime
together restores the previous API.

#### Raw presence migration

Regenerate the entire client package and update `crucible-client`,
`crucible-build`, and the generator to the same pinned revision. This changes
the generated Rust API without changing the wire encoding of existing values.
Consumers must update struct literals and matches:

| Previous client API | Current client API |
|---|---|
| Optional field `None` | `Field::Absent` |
| Optional field `Some(value)` | `Field::Value(value)` or `value.into()` |
| Required field in an `@input` structure | `Field::Value(value)` |
| No explicit null representation | `Field::Null` |
| Read `Option<T>` | Use `value()` / `into_value()`, or match all three states |
| Exhaustive enum or union match | Handle the generated unknown variant |
| Copy an enum or retain its `&'static str` | Clone or borrow the enum; copy the string if needed |

`Field::from(Option<T>)` maps `None` to `Absent` and `Some(value)` to `Value(value)`.
Use it when adapting an existing optional value; it cannot infer explicit null.
For example, with the generated weather models:

```rust
use crucible_client::Field;
use weather_client::model::{ListCitiesInput, Sky};

let previous_limit: Option<i32> = Some(50);
let input = ListCitiesInput {
    max_results: Field::from(previous_limit),
    next_token: Field::Absent,
    tags: Field::Null,
};
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

### Dynamic HTTP bindings

Generated clients support `@jsonName`, greedy path labels, `@httpQueryParams`,
`@httpPrefixHeaders`, and `@httpResponseCode`. Public member names stay unchanged;
JSON names affect only the wire. Explicit query/header bindings take precedence
when a map supplies the same name. Greedy labels preserve slash separators and
reject dot-only segments before dispatch. Response-code members read the actual
HTTP status, including modeled errors. Nested pagination token paths continue to
use the public model names.

### Streaming payloads

An `@streaming` blob uses `ByteStream` on clients and servers. `ByteStream::new`
owns a one-shot stream of `Result<Bytes, E>` chunks. `ByteStream::replay` takes a
factory that must reproduce the same bytes on every attempt; `from_bytes` is a
replayable in-memory convenience. A cloned one-shot value shares its consumption
state. The transport never automatically retries a one-shot upload.

Call `open()` and consume the returned chunk stream with `StreamExt::next`.
Dropping that stream releases its HTTP reader. Call `close()` to release an
unopened response. The original call deadline and cancellation token remain
active during response reads. Streaming errors are `std::io::Error`, with
`TimedOut` and `Interrupted` distinguishing deadline and cancellation failures.

`with_length` supplies a known length for `@requiresLength`; declared and modeled
length bounds are enforced while reading. With `Config::content_sha256(true)`,
streaming inputs require `with_sha256` or an explicit digest header. The runtime
does not collect a streaming body to calculate a hash. Generated servers also
stream request and response bodies, and enforce configured request-body limits
as chunks arrive.

### Exact numbers and collections

`bigInteger` and `bigDecimal` use `BigInteger` and `BigDecimal`. Parse decimal
text with `str::parse`; both serialize as JSON numbers without floating-point
conversion. Their public inner values and `Deref` expose the native `num_bigint`
and `bigdecimal` APIs. Numeric defaults and range validation retain the same
precision, including HTTP headers. Integer enums provide `as_i32()` and client
`UnknownValue(i32)` variants for values introduced by newer servers.

Sparse lists use `Vec<Option<T>>`, and sparse maps use
`BTreeMap<String, Option<T>>`. Sets and `@uniqueItems` lists remain `Vec<T>`;
generated member codecs reject duplicates during encoding and decoding using
structural value equality. Object member order and equivalent numeric spellings
do not distinguish elements. Dense document collections reject null elements but preserve null nested inside documents.
Nested timestamp collections use their modeled timestamp formats.

### Authentication options

`Config::basic_credentials(Identity::fixed(BasicCredentials::new(user, password)))`
configures Basic authentication; `Config::api_key(Token::fixed(key))` supplies the
modeled API-key header or query parameter. `Identity::provider` accepts an async
provider returning `Option<T>`. The runtime selects identities in modeled order
on each attempt, and preserves explicit matching credential headers without
calling unrelated providers. `@auth([])` bypasses identity providers and
`@optionalAuth` permits anonymous calls. Required Basic/API-key combinations fail
before dispatch when no identity is available. Bearer-only services retain the
existing behavior of accepting a call without a configured token and deciding
its authorization at the service. Credential resolution uses the total call
budget and cancellation token.

### Endpoints and waiters

Models with endpoint rules can use `Config::default().region("us-east-1")`;
`Config::new(endpoint)` supplies the `SDK::Endpoint` built-in. Custom parameters
use `endpoint_parameter` and `endpoint_builtin`. The generated metadata resolves
rules, headers, and operation host prefixes. `disable_host_prefix(true)` disables
prefix expansion. Invalid labels and rule errors fail before dispatch.

Waiters generate `wait_until_<name>(input, WaiterOptions::new(max_wait_time))`.
The result is `WaiterResult::Response { output, attempts }` or
`WaiterResult::Error { error, attempts }`: an error-type acceptor can legitimately
mean success. `WaiterError` retains the last response or underlying error and
distinguishes failure, timeout, cancellation, and invalid configuration. Optional
delay overrides and `options.call` configure polling and cancellation. One budget
covers calls, their retries, and jittered delays. Discarded streaming responses
are closed before polling again.

Endpoint paths and waiter expressions use model member names even with
`@jsonName`; timestamps are normalized to exact epoch seconds. Compiled JMESPath
metadata is adapted once, and comparisons and numeric functions preserve exact
decimal values rather than converting to floating point.

### Payloads and media-type headers

An `@httpPayload` member is the whole HTTP body. A blob is raw bytes. A string
or enum is UTF-8 text; an empty body reads as an empty string. A structure,
union, or document is one JSON value; an absent or explicitly null value sends
no body, and an empty response body reads as absent. Each payload is sent under
its target's `@mediaType`, or the protocol default. Modeled errors may also
bind a payload: the error is named by the `x-crucible-error` header
(`X-Amzn-Errortype` for AWS protocols) because the body is no longer a JSON
envelope, and header members still decode beside it.

Header strings whose target has `@mediaType`, including list elements and
`@httpPrefixHeaders` values, travel base64-encoded. The public model holds the
decoded text. A response header that is not base64 of UTF-8 text is
`Error::Decode`.

### AWS JSON protocols

A model can be generated for `aws.protocols#restJson1`, `awsJson1_0`, or
`awsJson1_1`; a service that declares several needs one selected at generation.
`restJson1` uses HTTP bindings like `crucible.protocols#json`, but body
timestamps default to epoch seconds, header timestamps to HTTP dates (lists are
sent unquoted), and query and label timestamps to RFC 3339. The two RPC
protocols POST every member as a JSON body (`{}` for an empty input) to `/`
with `X-Amz-Target: {Service}.{Operation}` and
`application/x-amz-json-1.0` or `-1.1`.

All three omit an explicit `Field::Null` structure member when sending and read
a null member as `Field::Absent`; document members keep their nulls. `NaN`,
`Infinity`, and `-Infinity` floats are sent and read as those strings. A
`__type` key beside a union member is ignored. Errors are named by
`X-Amzn-Errortype`, then the body's `__type` or `code`; namespaces and `:`
suffixes are dropped, and renamed errors still match their wire name. A
capitalized `Message` fills a modeled `message` member and
`UnknownError::message`. Request IDs come from `x-request-id`,
`x-amzn-requestid`, or `x-amz-request-id`.

### HTTP checksums

`@httpChecksumRequired` and `@aws.protocols#httpChecksum` compute request
checksums over the bytes actually sent: after interceptors and before SigV4
signing. CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512 are supported;
choosing another algorithm is `Error::Invalid` before dispatch. A checksum
header the caller supplies, through a bound member or an interceptor, is sent
unchanged.

By default (`ChecksumPolicy::WhenSupported`) an absent algorithm member is sent
as `CRC32` and an absent validation-mode member as `ENABLED`, without changing
the caller's input. `Config::request_checksum_calculation(ChecksumPolicy::WhenRequired)`
computes a checksum only when the model requires one or the caller chose an
algorithm; `response_checksum_validation(ChecksumPolicy::WhenRequired)` verifies
responses only when the caller enabled validation. A required checksum without
an algorithm member is `Content-MD5`.

When validation is enabled, the first modeled, supported response algorithm
that the response carries is verified. A mismatch is
`Error::ChecksumMismatch(algorithm)` and is not retried.
`Config::on_checksum_validated(|operation, algorithm| ..)` reports each verified
response. A streamed response is verified as it is read: a mismatch ends the
stream with an `InvalidData` error, and the callback runs at its end. A streamed
upload cannot be hashed before it is sent, so an optional checksum is skipped
and a required or chosen one needs a caller-supplied header.

These additions change `crucible-wire` descriptions (`Location::TextPayload`,
`Location::DocumentPayload`, `Scalar::Base64`, `Scalar::Timestamp`, and the
`Operation::PROTOCOL` and `Operation::CHECKSUM` constants) and add
`Error::ChecksumMismatch`. Regenerate clients with the matching runtime, and
add the new variant to exhaustive matches on `Error`.

### AWS signing and credential providers

`Config::aws_credentials(Identity::fixed(AwsCredentials::new(access, secret)))`
enables modeled SigV4 authentication. Temporary credentials may include
`session_token` and `expiration`. Configure `region`; `signing_name` optionally
overrides the modeled name. Endpoint auth properties can override the signing
region, name, double encoding, and path normalization. Signing runs after caller
interceptors, and identities are resolved again for each attempt. Streaming
uploads require a precomputed SHA-256 digest unless the model permits unsigned
payloads; signing never buffers a stream.

`AwsCredentials::from_environment`, `from_environment_map`, `from_profile`, and
`from_profile_file` load explicit credential sources. `Identity::chain` tries
providers in order. `.cached(refresh_before)` coalesces concurrent refreshes and
refreshes expiring credentials before they expire. No implicit environment,
profile, or metadata-service lookup runs unless the application selects it.
