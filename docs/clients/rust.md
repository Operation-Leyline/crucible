# Rust clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Crucible generates native Rust models, one error enum per operation, an async
`Client`, and lazy paginators. The generated package uses `crucible-client` and
the same wire types as the Rust service. Use the current stable Rust toolchain
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
use weather_client::{Client, model::{GetCityInput, ListCitiesInput}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(
        Config::new("http://127.0.0.1:3000")
            .token(Token::fixed(std::env::var("WEATHER_TOKEN")?)),
    );
    let result = client.get_city(GetCityInput {
        city_id: "oslo".into(),
        at: None,
    }).await?;
    println!("{}", result.city.name);

    let mut pages = client.list_cities_pages(ListCitiesInput {
        max_results: Some(50), next_token: None, tags: None,
    });
    while let Some(page) = pages.next_page().await {
        println!("{} cities", page?.cities.len());
    }
    Ok(())
}
```

`Client` is cheap to clone and shares its transport. Operation methods take
owned inputs. Missing idempotency tokens are filled in that input for one call
and reused across attempts; supply a stable token yourself for separate replays.
`Token::provider` accepts an async function returning `Option<String>` and runs
on every attempt, so refreshed credentials are picked up.

## Models and errors

Structures use public snake_case fields. Smithy `long` is `i64`, `integer` is
`i32`, document values use `serde_json::Value`, and blobs use
`crucible_client::Blob`. A JSON blob is base64; an `@httpPayload` blob is raw bytes.
Optional members use `Option<T>`: `None` is omitted, and absent/null input values
collapse when decoded. There is no independent explicit-null presence wrapper.
Defaults are applied by generated deserialization.

Timestamps are `chrono::DateTime<Utc>`. RFC 3339 date-time preserves supported
subsecond precision; HTTP-date has second precision. The epoch-seconds codec
currently converts through `f64` and milliseconds, so it does not preserve
nanosecond values or guarantee precision at extreme dates. Named string enums
and unions are closed Rust enums: an unmodeled enum value or union arm fails
decoding. A modeled arm named `unknown` is still just that modeled arm.

Each method returns `Result<Output, Error<OperationError>>`. Match
`Error::Service(CreateCityError::ConflictError(error))` for a modeled exception.
Other variants are `Unknown`, `Transport`, `Invalid`, and `Decode`.
Unknown errors retain status, type name, message, and request ID. Modeled error
fields follow their Smithy definitions; the Rust API does not attach a uniform
HTTP metadata object to every modeled error.

## Transport and retries

Retries follow the [shared contract](../clients.md#http-retry-contract).
`Config::retry(Retry { ... })` controls attempts and backoff; `Retry::none()`
disables retries. Defaults are three attempts, 100ms base delay, and 20s maximum
backoff. A valid `Retry-After` takes precedence.

Use `Config::http_client(reqwest::Client)` for connection pools, proxies,
redirect policy, and per-attempt timeout settings. For an overall deadline,
wrap the operation future in `tokio::time::timeout`; dropping the future cancels
the active call and subsequent retries. There is no generated call-scope object.
The default reqwest configuration governs redirects and transport timeouts.

`Config::header` supplies default headers. `Config::interceptor` accepts an
`Interceptor` implementation that receives a mutable reqwest request after
credentials and headers, on every attempt. Interceptors run in registration
order. Rust has no `--content-sha256` generation option: an application that
needs the header supplies an interceptor. With `sha2 = "0.10"` and
`reqwest = { version = "0.13.5", default-features = false }` dependencies:

```rust
use sha2::{Digest, Sha256};

struct BodyDigest;
impl crucible_client::Interceptor for BodyDigest {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
        if let Some(body) = request.body() {
            let bytes = body.as_bytes().ok_or("body must be buffered")?;
            let digest = format!("{:x}", Sha256::digest(bytes));
            request.headers_mut().insert(
                "x-amz-content-sha256",
                digest.parse().map_err(|error| format!("{error}"))?,
            );
        }
        Ok(())
    }
}
```

Register `BodyDigest` last, after any interceptor that changes the body:
`Config::new(endpoint).interceptor(BodyDigest)`.

Paginators send only when `next_page().await` is called. They stop after the
last page, an error, or a token repeated immediately by the service; they do not
retain a set of all earlier tokens. `collect().await` buffers all pages.

## Qualification

Run `cargo test -p crucible-codegen-rust -p crucible-client -p crucible-wire`,
then `bash tools/test-e2e.sh` for the generated client over the weather service.
The real loopback retry runner consumes the shared retry fixture. The E2E
workspace is a separate Cargo workspace and uses a generated model package.

Local generation and native HTTP proof do not establish published-package or
authenticated Leyline compatibility. See [qualification levels](../clients.md#qualification).

## Updating the runtime

`ErrorInfo` includes `retryable`, preserving non-throttling `@retryable` metadata.
Regenerate Rust clients and services with the matching runtime. Handwritten
operation descriptions must supply this field. Unmodeled 501 and other unknown
5xx responses are not automatically retried. Roll back the generator, runtime,
and generated code together if reverting this change.
